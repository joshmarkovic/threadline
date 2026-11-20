use std::num::NonZeroU64;

use threadline_core::domain::IssueApiId;
use threadline_storage::{
    AccountId, Change, IssueClassification, IssueRecord, ItemLocator, ItemRepresentation,
    ItemUpdate, Store, UtcMillis,
    access::{
        AccessProfile, AccessState, CredentialKind, DiscoveryState, RepositoryObservation,
        RepositoryObservationSource,
    },
    queue::{ActionKind, Submission},
};

fn id(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn install(
    store: &Store,
    account: AccountId,
    user: u64,
    name: &str,
    owner: &str,
    kind: CredentialKind,
) -> AccessProfile {
    let profile = store
        .reserve_access_profile_credential(account, id(user), name, owner, kind, None)
        .unwrap();
    store
        .install_current_credential(&profile, || Ok::<_, ()>(()))
        .unwrap()
        .unwrap();
    profile
}

#[test]
fn schema_18_upgrade_preserves_pat_bindings_and_backs_up_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("threadline.sqlite3");
    let store = Store::open(&path).unwrap();
    let account = store.ensure_account("github.com", 101).unwrap();
    let classic = install(
        &store,
        account,
        101,
        "alice",
        "first",
        CredentialKind::Classic,
    );
    let fine = install(
        &store,
        account,
        101,
        "alice",
        "second",
        CredentialKind::FineGrained,
    );
    store.activate_local_profile(account).unwrap();
    drop(store);
    let db = rusqlite::Connection::open(&path).unwrap();
    // Reproduce the original schema 18 constraint on an otherwise complete profile.
    db.execute_batch("ALTER TABLE github_access_profiles ADD COLUMN original_kind INTEGER NOT NULL
                      DEFAULT 0 CHECK(original_kind IN (0,1));
                      UPDATE github_access_profiles SET original_kind=credential_kind;
                      ALTER TABLE github_access_profiles DROP COLUMN credential_kind;
                      ALTER TABLE github_access_profiles RENAME COLUMN original_kind TO credential_kind;
                      PRAGMA user_version=18;").unwrap();
    drop(db);

    let store = Store::open(&path).unwrap();
    assert_eq!(store.active_github_account().unwrap(), Some(account));
    assert_eq!(
        store.github_access_profiles().unwrap(),
        vec![classic.clone(), fine.clone()]
    );
    let backup = rusqlite::Connection::open(store.migration_backup().unwrap()).unwrap();
    assert_eq!(
        backup
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        18
    );
    assert!(
        backup
            .execute("UPDATE github_access_profiles SET credential_kind=2", [])
            .is_err()
    );
    let failed = store
        .link_github_cli(account, id(101), "alice", "first", Some(&classic), |_| {
            Err("vault locked")
        })
        .unwrap();
    assert_eq!(failed.unwrap_err(), "vault locked");
    assert_eq!(
        store
            .github_access_profile(account, "first")
            .unwrap()
            .unwrap(),
        classic
    );
    let linked = store
        .link_github_cli(account, id(101), "alice", "first", Some(&classic), |old| {
            assert_eq!(old, &classic);
            Ok::<_, ()>(())
        })
        .unwrap()
        .unwrap();
    assert_eq!(linked.kind, CredentialKind::GitHubCli);
    assert!(
        store
            .with_installed_credential(&classic, || panic!("stale PAT"))
            .is_err()
    );
    assert!(store.with_installed_credential(&linked, || ()).is_ok());
    assert!(store.with_installed_credential(&fine, || ()).is_ok());
    let mut forged = fine.clone();
    forged.kind = CredentialKind::GitHubCli;
    assert!(
        store
            .with_installed_credential(&forged, || panic!("unlinked CLI"))
            .is_err()
    );
    assert!(
        !store
            .record_access_result(
                &fine,
                CredentialKind::GitHubCli,
                AccessState::Readable,
                DiscoveryState::Complete,
                15
            )
            .unwrap()
    );
    let mut wrong_login = linked;
    wrong_login.login = "bob".into();
    assert!(
        store
            .with_installed_credential(&wrong_login, || panic!("wrong CLI login"))
            .is_err()
    );
}

#[test]
fn account_scope_renewal_deselection_and_signout_hold_the_original_outbox() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("access-例.sqlite");
    let store = Store::open(&path).unwrap();
    let alice = store.ensure_account("github.com", 101).unwrap();
    let bob = store.ensure_account("github.com", 202).unwrap();
    let alice_work = install(
        &store,
        alice,
        101,
        "alice",
        "work-org",
        CredentialKind::FineGrained,
    );
    let alice_other = install(
        &store,
        alice,
        101,
        "alice",
        "other-org",
        CredentialKind::Classic,
    );
    let bob_work = install(&store, bob, 202, "bob", "work-org", CredentialKind::Classic);
    assert_ne!(alice_work.credential_ref, alice_other.credential_ref);
    assert_ne!(alice_work.credential_ref, bob_work.credential_ref);
    assert!(
        store
            .reserve_access_profile_credential(
                alice,
                id(202),
                "bob",
                "work-org",
                CredentialKind::Classic,
                None
            )
            .is_err()
    );

    let remote = RepositoryObservation {
        provider_id: id(1),
        owner: "work-org",
        name: "private",
        private: true,
        source: RepositoryObservationSource::Direct,
    };
    assert!(
        store
            .observe_github_repository(&alice_other, &remote, 100)
            .is_err()
    );
    let other_repo = store
        .observe_github_repository(&bob_work, &remote, 100)
        .unwrap();
    let repository = store
        .observe_github_repository(&alice_work, &remote, 100)
        .unwrap();
    assert_ne!(repository, other_repo);
    let item = store
        .ingest(
            alice,
            ItemLocator {
                repository,
                number: 41,
            },
            ItemUpdate {
                representation: ItemRepresentation::Issue(IssueRecord {
                    id: IssueApiId::new(id(41)),
                    node_id: None,
                    classification: IssueClassification::Issue,
                }),
                remote_updated_at: UtcMillis(100),
                title: Change::Value("fixture issue".into()),
                body: Change::Missing,
                closed: Change::Value(false),
            },
        )
        .unwrap();
    let operation = store.new_operation_id().unwrap();
    store
        .enqueue(
            alice,
            Submission {
                id: &operation,
                item,
                kind: ActionKind::Comment,
                payload: "fixture",
                base: "",
                anchor: "",
                projection: "fixture",
                draft: None,
                dependencies: &[],
            },
        )
        .unwrap();
    store.activate_local_profile(alice).unwrap();
    store
        .record_access_result(
            &alice_work,
            alice_work.kind,
            AccessState::Readable,
            DiscoveryState::RestrictedBySso,
            105,
        )
        .unwrap();
    assert_eq!(
        store.dispatch_hold(alice, &operation).unwrap().as_deref(),
        Some("repository_unselected")
    );
    store
        .select_github_repository(alice, repository, true)
        .unwrap();
    assert_eq!(
        store
            .credential_for_bound_operation(alice, &operation)
            .unwrap()
            .unwrap()
            .owner_scope,
        "work-org"
    );

    store
        .mark_repository_access(&alice_work, repository, AccessState::NotFoundOrDenied, 110)
        .unwrap();
    let row = store.github_repository_views(alice).unwrap().pop().unwrap();
    assert_eq!(row.last_success_at_ms, Some(100));
    assert_eq!(row.cached_items, 1);
    assert!(row.selected);
    assert_eq!(row.access, AccessState::NotFoundOrDenied);
    assert!(
        store
            .credential_for_bound_operation(alice, &operation)
            .unwrap()
            .is_none()
    );

    let renewed = install(
        &store,
        alice,
        101,
        "alice",
        "work-org",
        CredentialKind::Classic,
    );
    assert_eq!(renewed.credential_ref, alice_work.credential_ref);
    assert!(
        !store
            .record_access_result(
                &alice_work,
                alice_work.kind,
                AccessState::Readable,
                DiscoveryState::Complete,
                130
            )
            .unwrap()
    );
    assert_eq!(
        store
            .observe_github_repository(&renewed, &remote, 135)
            .unwrap(),
        repository
    );
    assert!(
        store
            .record_access_result(
                &renewed,
                CredentialKind::Classic,
                AccessState::Readable,
                DiscoveryState::Complete,
                140
            )
            .unwrap()
    );
    store.activate_local_profile(bob).unwrap();
    assert!(
        store
            .credential_for_bound_operation(alice, &operation)
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .credential_for_bound_operation(bob, &operation)
            .unwrap()
            .is_none()
    );
    store.activate_local_profile(alice).unwrap();
    store
        .select_github_repository(alice, repository, false)
        .unwrap();
    assert!(
        store
            .credential_for_bound_operation(alice, &operation)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.github_repository_views(alice).unwrap()[0].cached_items,
        1
    );
    store
        .select_github_repository(alice, repository, true)
        .unwrap();

    let references = store.sign_out_github_account(alice, 150).unwrap();
    assert_eq!(references.len(), 2);
    assert!(
        references
            .iter()
            .all(|profile| profile.access == AccessState::SignedOut)
    );
    assert!(
        !store
            .record_access_result(
                &renewed,
                renewed.kind,
                AccessState::Readable,
                DiscoveryState::Complete,
                150
            )
            .unwrap()
    );
    assert!(
        store
            .observe_github_repository(&renewed, &remote, 151)
            .is_err()
    );
    let signed_out = references
        .iter()
        .find(|entry| entry.owner_scope == "work-org")
        .unwrap();
    assert!(
        store
            .with_current_credential(&renewed, || panic!("stale credential callback"))
            .is_err()
    );
    let concurrent = Store::open(&path).unwrap();
    let reconnected = install(
        &concurrent,
        alice,
        101,
        "alice",
        "work-org",
        CredentialKind::Classic,
    );
    assert!(
        store
            .with_current_credential(signed_out, || panic!("stale deletion must not run"))
            .is_err()
    );
    let latest = store.sign_out_github_account(alice, 152).unwrap();
    assert_eq!(latest.len(), 2);
    assert!(
        store
            .with_current_credential(&reconnected, || panic!("stale credential callback"))
            .is_err()
    );
    drop(concurrent);
    drop(store);

    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.active_github_account().unwrap(), Some(alice));
    assert!(reopened.github_repository_views(alice).unwrap()[0].selected);
    assert_eq!(
        reopened.github_repository_views(alice).unwrap()[0].cached_items,
        1
    );
    assert!(
        reopened
            .credential_for_bound_operation(alice, &operation)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        reopened.operation(alice, &operation).unwrap().unwrap().item,
        item
    );
}

#[test]
fn discovery_cannot_wake_a_second_repository_under_a_narrower_replacement_token() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(&temp.path().join("access.sqlite")).unwrap();
    let account = store.ensure_account("github.com", 101).unwrap();
    let profile = install(
        &store,
        account,
        101,
        "alice",
        "work-org",
        CredentialKind::Classic,
    );
    let mut reports = [
        RepositoryObservation {
            provider_id: id(1),
            owner: "work-org",
            name: "primary",
            private: true,
            source: RepositoryObservationSource::Direct,
        },
        RepositoryObservation {
            provider_id: id(2),
            owner: "work-org",
            name: "secondary",
            private: true,
            source: RepositoryObservationSource::Direct,
        },
    ];
    let primary = store
        .observe_github_repository(&profile, &reports[0], 100)
        .unwrap();
    let secondary = store
        .observe_github_repository(&profile, &reports[1], 100)
        .unwrap();
    let item = store
        .ingest(
            account,
            ItemLocator {
                repository: secondary,
                number: 1,
            },
            ItemUpdate {
                representation: ItemRepresentation::Issue(IssueRecord {
                    id: IssueApiId::new(id(101)),
                    node_id: None,
                    classification: IssueClassification::Issue,
                }),
                remote_updated_at: UtcMillis(100),
                title: Change::Value("Pending note".into()),
                body: Change::Missing,
                closed: Change::Value(false),
            },
        )
        .unwrap();
    let operation = store.new_operation_id().unwrap();
    store
        .enqueue(
            account,
            Submission {
                id: &operation,
                item,
                kind: ActionKind::Comment,
                payload: "note",
                base: "",
                anchor: "",
                projection: "note",
                draft: None,
                dependencies: &[],
            },
        )
        .unwrap();
    store.activate_local_profile(account).unwrap();
    let ready = |profile: &AccessProfile, time| {
        assert!(
            store
                .record_access_result(
                    profile,
                    profile.kind,
                    AccessState::Readable,
                    DiscoveryState::Complete,
                    time
                )
                .unwrap()
        )
    };
    ready(&profile, 100);
    for repository in [primary, secondary] {
        store
            .select_github_repository(account, repository, true)
            .unwrap();
    }
    assert_eq!(store.dispatch_hold(account, &operation).unwrap(), None);

    let refresh = store.begin_access_check(&profile).unwrap();
    assert!(store.dispatch_hold(account, &operation).unwrap().is_some());
    store
        .observe_github_repository(&refresh, &reports[0], 101)
        .unwrap();
    ready(&refresh, 101);
    assert_eq!(store.dispatch_hold(account, &operation).unwrap(), None);
    assert!(
        store
            .github_repository_views(account)
            .unwrap()
            .iter()
            .all(|repo| repo.selected && repo.verified_current)
    );

    let pending = store
        .reserve_access_profile_credential(
            account,
            id(101),
            "alice",
            "work-org",
            CredentialKind::FineGrained,
            None,
        )
        .unwrap();
    assert_eq!(
        store
            .install_current_credential(&pending, || Err("vault locked"))
            .unwrap(),
        Err("vault locked")
    );
    assert!(store.begin_access_check(&pending).is_err());
    assert!(
        !store
            .record_access_result(
                &pending,
                pending.kind,
                AccessState::Readable,
                DiscoveryState::Complete,
                102
            )
            .unwrap()
    );
    assert!(store.dispatch_hold(account, &operation).unwrap().is_some());
    store
        .install_current_credential(&pending, || Ok::<_, ()>(()))
        .unwrap()
        .unwrap();
    store
        .observe_github_repository(&pending, &reports[0], 103)
        .unwrap();
    reports[1].source = RepositoryObservationSource::Discovery;
    store
        .observe_github_repository(&pending, &reports[1], 103)
        .unwrap();
    ready(&pending, 103);
    let rows = store.github_repository_views(account).unwrap();
    let original = rows.iter().find(|repo| repo.id == secondary).unwrap();
    assert!(original.selected);
    assert!(!original.verified_current);
    assert_eq!(original.last_success_at_ms, Some(100));
    assert!(
        store
            .credential_for_bound_operation(account, &operation)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.dispatch_hold(account, &operation).unwrap().as_deref(),
        Some("repository_access")
    );
}
