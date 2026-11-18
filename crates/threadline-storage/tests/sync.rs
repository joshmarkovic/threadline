use std::collections::BTreeSet;
use std::num::NonZeroU64;
use threadline_storage::access::{
    AccessState, CredentialKind, DiscoveryState, RepositoryObservation, RepositoryObservationSource,
};
use threadline_storage::reader::DiscussionComment;
use threadline_storage::resources::{ResourceFamily, Validators};
use threadline_storage::sync::*;
use threadline_storage::{
    AccountId, Change, IssueApiId, IssueClassification, IssueRecord, ItemLocator,
    ItemRepresentation, ItemUpdate, RepositoryId, Store, UtcMillis,
};

const START: i64 = 1_000_000;
fn id(n: u64) -> NonZeroU64 {
    NonZeroU64::new(n).unwrap()
}
fn install(store: &Store) -> (AccountId, RepositoryId) {
    let account = store.ensure_account("github.com", 102).unwrap();
    let profile = store
        .reserve_access_profile_credential(
            account,
            id(102),
            "test",
            "example",
            CredentialKind::FineGrained,
            None,
        )
        .unwrap();
    store
        .install_current_credential(&profile, || Ok::<_, ()>(()))
        .unwrap()
        .unwrap();
    store.activate_local_profile(account).unwrap();
    store
        .record_access_result(
            &profile,
            profile.kind,
            AccessState::Readable,
            DiscoveryState::RestrictedBySso,
            1,
        )
        .unwrap();
    let repo = store
        .observe_github_repository(
            &profile,
            &RepositoryObservation {
                provider_id: id(10),
                owner: "example",
                name: "selected",
                private: true,
                source: RepositoryObservationSource::Direct,
            },
            2,
        )
        .unwrap();
    store.select_github_repository(account, repo, true).unwrap();
    (account, repo)
}
fn feed(repository: RepositoryId, family: ResourceFamily) -> ReadTarget {
    ReadTarget {
        repository,
        item: None,
        family,
        revision: String::new(),
    }
}
fn tag(key: &str) -> Validators {
    Validators {
        etag: Some(key.into()),
        ..Default::default()
    }
}
fn no_changes() -> ObservationPage<'static> {
    ObservationPage {
        items: vec![],
        comments: vec![],
        next_cursor: None,
        validators: Validators::default(),
    }
}
fn comment(number: u64, id: u64, created: i64, updated: i64) -> ObservedComment {
    ObservedComment {
        number,
        comment: DiscussionComment {
            provider_id: id,
            node_id: None,
            author: Some("fixture".into()),
            created_at_ms: created,
            updated_at_ms: updated,
            body: format!("comment version {updated}"),
        },
    }
}
fn issue(number: u64, updated: i64, closed: bool) -> ObservedItem {
    ObservedItem {
        number,
        provider_id: number + 100,
        created_at_ms: Some(1),
        assignee_ids: None,
        update: ItemUpdate {
            representation: ItemRepresentation::Issue(IssueRecord {
                id: IssueApiId::new(id(number + 100)),
                node_id: None,
                classification: IssueClassification::Issue,
            }),
            remote_updated_at: UtcMillis(updated),
            title: Change::Value(format!("Issue {number}")),
            body: Change::Missing,
            closed: Change::Value(closed),
        },
    }
}

#[test]
fn it_007_panes_share_a_claim_cancel_independently_and_obey_offline_and_repository_holds() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("profile.sqlite3");
    let store = Store::open(&db).unwrap();
    let (a, r) = install(&store);
    let item = store.ensure_placeholder(a, r, 99).unwrap();
    let target = ReadTarget {
        repository: r,
        item: Some(item),
        family: ResourceFamily::Metadata,
        revision: "v1".into(),
    };
    for pane in ["left", "right"] {
        store
            .subscribe_read(a, &target, pane, ReadPriority::Visible, START)
            .unwrap();
    }
    let first = store.claim_sync_read(a, START).unwrap().unwrap();
    assert!(first.credential_generation > 0);
    assert_eq!(store.next_sync_due(a, START).unwrap(), Some(START + 90_000));
    assert!(store.claim_sync_read(a, START + 1000).unwrap().is_none());
    assert!(!store.unsubscribe_read(a, &target, "left").unwrap());
    assert!(store.unsubscribe_read(a, &target, "right").unwrap());
    assert_eq!(store.next_sync_due(a, START).unwrap(), None);
    assert!(
        store
            .finish_sync_read(
                &first,
                ReadCompletion::Fetched(tag("leaked")),
                START + 1,
                "UTC"
            )
            .is_err()
    );
    store
        .subscribe_read(a, &target, "explicit", ReadPriority::Explicit, START + 1000)
        .unwrap();
    let current = store.claim_sync_read(a, START + 1000).unwrap().unwrap();
    store.select_github_repository(a, r, false).unwrap();
    assert!(
        store
            .finish_sync_read(
                &current,
                ReadCompletion::Fetched(tag("old")),
                START + 1001,
                "UTC"
            )
            .is_err()
    );
    store.select_github_repository(a, r, true).unwrap();
    let mut settings = store.sync_settings(a).unwrap();
    settings.controls.work_offline = true;
    settings.controls.pause_background = true;
    settings.controls.pause_queued_actions = true;
    settings.controls.notification_quiet = true;
    store.save_sync_settings(a, &settings).unwrap();
    drop(store);
    let store = Store::open(&db).unwrap();
    assert_eq!(store.sync_settings(a).unwrap(), settings);
    assert!(store.claim_sync_read(a, START + 100_000).unwrap().is_none());
    settings.controls.work_offline = false;
    store.save_sync_settings(a, &settings).unwrap();
    let explicit = store.claim_sync_read(a, START + 100_000).unwrap().unwrap();
    store
        .finish_sync_read(
            &explicit,
            ReadCompletion::Fetched(tag("shown")),
            START + 100_010,
            "UTC",
        )
        .unwrap();
    store
        .subscribe_read(a, &target, "left", ReadPriority::Visible, START + 100_011)
        .unwrap();
    assert!(store.claim_sync_read(a, START + 200_000).unwrap().is_none());
    settings.controls.pause_background = false;
    store.save_sync_settings(a, &settings).unwrap();
    let conditional = store.claim_sync_read(a, START + 200_000).unwrap().unwrap();
    assert_eq!(conditional.validators, tag("shown"));
    store
        .finish_sync_read(
            &conditional,
            ReadCompletion::NotModified(Validators::default()),
            START + 200_010,
            "UTC",
        )
        .unwrap();
    store.sign_out(a).unwrap();
    assert!(store.claim_sync_read(a, START + 999_000).unwrap().is_none());
}

#[test]
fn it_007_shared_budget_gives_every_class_a_turn_under_continuous_user_load() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (a, r) = install(&s);
    let priorities = [
        ReadPriority::Explicit,
        ReadPriority::Visible,
        ReadPriority::Activity,
        ReadPriority::Recent,
        ReadPriority::Reconcile,
    ];
    let mut configured = s.sync_settings(a).unwrap();
    configured.schedule.work_interval_secs = 1;
    configured.schedule.off_hours_interval_secs = 1;
    configured.schedule.visible_interval_secs = 1;
    s.save_sync_settings(a, &configured).unwrap();
    for (n, p) in priorities.iter().enumerate() {
        let item = s.ensure_placeholder(a, r, n as u64 + 1).unwrap();
        let t = ReadTarget {
            repository: r,
            item: Some(item),
            family: ResourceFamily::Metadata,
            revision: n.to_string(),
        };
        s.subscribe_read(a, &t, "test", *p, START).unwrap();
    }
    s.hold_sync_budget(a, START + 60_000).unwrap();
    assert_eq!(s.next_sync_due(a, START).unwrap(), Some(START + 60_000));
    assert!(s.claim_sync_read(a, START + 59_999).unwrap().is_none());
    let mut found = BTreeSet::new();
    for n in 0..14 {
        let now = START + 60_000 + n * 1000;
        let claim = s.claim_sync_read(a, now).unwrap().unwrap();
        found.insert(claim.target.revision.clone());
        assert!(s.claim_sync_read(a, now + 500).unwrap().is_none());
        s.finish_sync_read(
            &claim,
            ReadCompletion::Fetched(Validators::default()),
            now + 1,
            "UTC",
        )
        .unwrap();
        if claim.target.revision == "0" {
            s.subscribe_read(a, &claim.target, "test", ReadPriority::Explicit, now + 2)
                .unwrap();
        }
    }
    assert_eq!(found.len(), 5);
}

#[test]
fn it_006_old_item_changes_and_comment_edits_survive_restarts_with_independent_overlapping_windows()
{
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sync.sqlite3");
    let s = Store::open(&path).unwrap();
    let (a, r) = install(&s);
    let comments = feed(r, ResourceFamily::Comments);
    let issues = feed(r, ResourceFamily::RepositoryItems);
    s.subscribe_read(a, &comments, "activity", ReadPriority::Activity, START)
        .unwrap();
    let first = s.claim_sync_read(a, START).unwrap().unwrap();
    assert_eq!(first.since_ms, Some(START - 300_000));
    s.commit_observed_page(
        &first,
        ObservationPage {
            comments: vec![comment(3, 87, 10, 20)],
            next_cursor: Some("2"),
            validators: tag("first-page"),
            ..no_changes()
        },
        START + 1,
        "UTC",
    )
    .unwrap();
    assert!(
        s.load_activity_baseline(a, r, "comments")
            .unwrap()
            .is_none()
    );
    drop(s);
    let s = Store::open(&path).unwrap();
    let second = s.claim_sync_read(a, START + 1000).unwrap().unwrap();
    assert_eq!(second.cursor.as_deref(), Some("2"));
    assert_eq!(second.validators, Validators::default());
    assert!(
        s.commit_observed_page(
            &second,
            ObservationPage {
                comments: vec![comment(99, 100, 0, 8), comment(1, 0, 0, 9)],
                ..no_changes()
            },
            START + 1001,
            "UTC"
        )
        .is_err()
    );
    assert!(
        s.resolve(
            a,
            ItemLocator {
                repository: r,
                number: 99
            }
        )
        .unwrap()
        .is_none()
    );
    s.commit_observed_page(
        &second,
        ObservationPage {
            comments: vec![comment(3, 88, 15, 25)],
            validators: tag("last-page"),
            ..no_changes()
        },
        START + 1002,
        "UTC",
    )
    .unwrap();
    assert_eq!(
        s.load_activity_baseline(a, r, "comments")
            .unwrap()
            .unwrap()
            .observed_at_ms,
        START
    );
    s.subscribe_read(a, &issues, "explicit", ReadPriority::Explicit, START + 2000)
        .unwrap();
    let initial = s.claim_sync_read(a, START + 2000).unwrap().unwrap();
    s.commit_observed_page(
        &initial,
        ObservationPage {
            items: vec![issue(3, START - 10, true), issue(987, START - 9, false)],
            ..no_changes()
        },
        START + 2001,
        "UTC",
    )
    .unwrap();
    s.subscribe_read(a, &issues, "explicit", ReadPriority::Explicit, START + 3000)
        .unwrap();
    let older = s.claim_sync_read(a, START + 3000).unwrap().unwrap();
    assert_eq!(older.since_ms, Some(START + 2000 - 300_000));
    s.commit_observed_page(
        &older,
        ObservationPage {
            items: vec![issue(3, START + 2500, false)],
            ..no_changes()
        },
        START + 3001,
        "UTC",
    )
    .unwrap();
    assert!(
        s.load_activity_event(
            a,
            &format!("github:issues:103:item_state_changed:{}", START + 2500)
        )
        .unwrap()
        .is_some()
    );
    s.subscribe_read(
        a,
        &comments,
        "explicit",
        ReadPriority::Explicit,
        START + 4000,
    )
    .unwrap();
    let updates = s.claim_sync_read(a, START + 4000).unwrap().unwrap();
    assert_eq!(updates.target, comments);
    let rows = || {
        vec![
            comment(3, 87, 10, START + 3001),
            comment(19, 90, START + 3000, START + 3000),
            comment(3, 88, 15, 25),
        ]
    };
    s.commit_observed_page(
        &updates,
        ObservationPage {
            comments: rows(),
            ..no_changes()
        },
        START + 4001,
        "UTC",
    )
    .unwrap();
    let key = format!("github:comments:90:comment_created:{}", START + 3000);
    let created = s.load_activity_event(a, &key).unwrap().unwrap();
    assert_eq!(
        created.item,
        s.resolve(
            a,
            ItemLocator {
                repository: r,
                number: 19
            }
        )
        .unwrap()
    );
    assert!(
        s.load_activity_event(
            a,
            &format!("github:comments:87:comment_edited:{}", START + 3001)
        )
        .unwrap()
        .is_some()
    );
    s.subscribe_read(
        a,
        &comments,
        "explicit",
        ReadPriority::Explicit,
        START + 5000,
    )
    .unwrap();
    let replay = s.claim_sync_read(a, START + 5000).unwrap().unwrap();
    s.commit_observed_page(
        &replay,
        ObservationPage {
            comments: rows(),
            ..no_changes()
        },
        START + 5001,
        "UTC",
    )
    .unwrap();
    assert_eq!(
        s.load_activity_event(a, &key)
            .unwrap()
            .unwrap()
            .observed_at_ms,
        START + 4001
    );
    assert!(
        s.load_activity_event(a, "github:comments:88:comment_created:15")
            .unwrap()
            .is_none()
    );
}

#[test]
fn it_007_expired_cursors_partial_permissions_budget_floors_and_retry_exhaustion_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved.sqlite3");
    let s = Store::open(&path).unwrap();
    let (account, repo) = install(&s);
    let target = feed(repo, ResourceFamily::Comments);
    s.subscribe_read(account, &target, "watch", ReadPriority::Activity, START)
        .unwrap();
    let initial = s.claim_sync_read(account, START).unwrap().unwrap();
    s.commit_observed_page(
        &initial,
        ObservationPage {
            next_cursor: Some("2"),
            validators: tag("old"),
            ..no_changes()
        },
        START + 1,
        "UTC",
    )
    .unwrap();
    let second = s.claim_sync_read(account, START + 1000).unwrap().unwrap();
    s.fail_sync_read(&second, ReadFailure::ExpiredCursor, START + 1001, None)
        .unwrap();
    let repair = s.claim_sync_read(account, START + 2000).unwrap().unwrap();
    assert_eq!(repair.cursor, None);
    assert_eq!(repair.since_ms, initial.since_ms);
    assert_eq!(repair.validators, Validators::default());
    s.commit_observed_partial(
        &repair,
        ObservationPage {
            comments: vec![comment(88, 11, 3, 10)],
            ..no_changes()
        },
        START + 2001,
    )
    .unwrap();
    assert!(
        s.resolve(
            account,
            ItemLocator {
                repository: repo,
                number: 88
            }
        )
        .unwrap()
        .is_some()
    );
    assert!(
        s.load_activity_baseline(account, repo, "comments")
            .unwrap()
            .is_none()
    );
    assert!(
        s.claim_sync_read(account, START + 10_000)
            .unwrap()
            .is_none()
    );
    s.subscribe_read(
        account,
        &target,
        "retry-now",
        ReadPriority::Explicit,
        START + 10_000,
    )
    .unwrap();
    let retry = s.claim_sync_read(account, START + 10_000).unwrap().unwrap();
    assert!(retry.cursor.is_none());
    s.fail_sync_read(
        &retry,
        ReadFailure::Throttled,
        START + 10_001,
        Some(START + 600_000),
    )
    .unwrap();
    drop(s);
    let s = Store::open(&path).unwrap();
    for n in 0..25 {
        s.schedule_selected_activity(account, START + 11_000 + n)
            .unwrap();
    }
    assert!(
        s.claim_sync_read(account, START + 599_999)
            .unwrap()
            .is_none()
    );
    let mut now = START + 600_000;
    let mut failed = 0;
    while failed < 4 {
        let claim = s.claim_sync_read(account, now).unwrap().unwrap();
        if claim.target == target {
            s.fail_sync_read(&claim, ReadFailure::Transient, now, None)
                .unwrap();
            failed += 1;
        } else {
            s.commit_observed_page(&claim, no_changes(), now, "UTC")
                .unwrap();
        }
        now += 100_000;
    }
    let later = s.claim_sync_read(account, now + 100_000).unwrap();
    assert!(later.is_none_or(|j| j.target != target));
    assert_eq!(
        s.sync_status(account).unwrap().last_failure.as_deref(),
        Some("transport")
    );
}

#[test]
fn it_006_concurrent_full_discussion_refresh_cannot_erase_a_newer_feed_edit() {
    use threadline_storage::reader::DiscussionInputPage;
    use threadline_storage::resources::{ResourceKey, ResourceTarget, WalkMode};
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (a, r) = install(&s);
    let i = s.ensure_placeholder(a, r, 11).unwrap();
    let family = ResourceKey {
        account: a,
        target: ResourceTarget::Item(i),
        family: ResourceFamily::Comments,
    };
    let baseline = s.start_resource_fetch(family, WalkMode::Full).unwrap();
    s.commit_discussion_page(
        a,
        i,
        &baseline,
        DiscussionInputPage {
            comments: vec![comment(11, 164, 1, 2).comment],
            next_cursor: None,
            completed_validators: None,
            fetched_at_ms: START,
        },
    )
    .unwrap();
    let refresh = s.start_resource_fetch(family, WalkMode::Full).unwrap();
    let next = s
        .commit_discussion_page(
            a,
            i,
            &refresh,
            DiscussionInputPage {
                comments: vec![comment(11, 164, 1, 4).comment],
                next_cursor: Some("2"),
                completed_validators: None,
                fetched_at_ms: START + 1,
            },
        )
        .unwrap()
        .unwrap();
    let target = feed(r, ResourceFamily::Comments);
    s.subscribe_read(a, &target, "watch", ReadPriority::Activity, START)
        .unwrap();
    let claim = s.claim_sync_read(a, START).unwrap().unwrap();
    s.commit_observed_page(
        &claim,
        ObservationPage {
            comments: vec![
                comment(11, 164, 1, 20),
                comment(11, 186, START + 1, START + 1),
            ],
            ..no_changes()
        },
        START + 2,
        "UTC",
    )
    .unwrap();
    s.commit_discussion_page(
        a,
        i,
        &next,
        DiscussionInputPage {
            comments: vec![],
            next_cursor: None,
            completed_validators: None,
            fetched_at_ms: START + 3,
        },
    )
    .unwrap();
    let downloaded = s.discussion_page(a, i, None, 20).unwrap();
    assert_eq!(downloaded.metadata.len(), 2);
    assert_eq!(downloaded.metadata[0].updated_at_ms, 20);
    assert_eq!(downloaded.records[0].body, "comment version 20");
}

#[test]
fn it_007_visible_subscriptions_replace_atomically_after_relaunch_without_waking_unchanged_panes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("work.sqlite3");
    let s = Store::open(&path).unwrap();
    let (a, r) = install(&s);
    let target = ReadTarget {
        repository: r,
        item: Some(s.ensure_placeholder(a, r, 1).unwrap()),
        family: ResourceFamily::Metadata,
        revision: String::new(),
    };
    let visible = VisibleRead {
        subscriber: "left-tab".into(),
        target: target.clone(),
        due_ms: START,
    };
    s.replace_visible_read_subscriptions(a, std::slice::from_ref(&visible))
        .unwrap();
    let first = s.claim_sync_read(a, START).unwrap().unwrap();
    let other = VisibleRead {
        subscriber: "right-tab".into(),
        ..visible.clone()
    };
    s.replace_visible_read_subscriptions(a, &[visible.clone(), other.clone()])
        .unwrap();
    s.replace_visible_read_subscriptions(a, std::slice::from_ref(&other))
        .unwrap();
    s.finish_sync_read(&first, ReadCompletion::Fetched(tag("v1")), START + 1, "UTC")
        .unwrap();
    let next = s.next_sync_due(a, START + 2).unwrap().unwrap();
    assert!(next > START + 2);
    drop(s);
    let s = Store::open(&path).unwrap();
    s.replace_visible_read_subscriptions(a, std::slice::from_ref(&other))
        .unwrap();
    assert_eq!(s.next_sync_due(a, START + 2).unwrap(), Some(next));
    let bad = VisibleRead {
        subscriber: "invalid".into(),
        target: ReadTarget {
            item: Some(threadline_storage::ItemId(10_000)),
            ..target.clone()
        },
        due_ms: START,
    };
    assert!(s.replace_visible_read_subscriptions(a, &[bad]).is_err());
    assert_eq!(s.next_sync_due(a, START + 2).unwrap(), Some(next));
    s.subscribe_read(
        a,
        &target,
        "background-watch",
        ReadPriority::Activity,
        START + 2,
    )
    .unwrap();
    s.replace_visible_read_subscriptions(a, &[]).unwrap();
    assert!(s.next_sync_due(a, START + 2).unwrap().is_some());
    assert!(s.unsubscribe_read(a, &target, "background-watch").unwrap());
    assert_eq!(s.next_sync_due(a, START + 3).unwrap(), None);
    s.replace_visible_read_subscriptions(a, &[visible]).unwrap();
    let orphan = s.claim_sync_read(a, START + 2_000).unwrap().unwrap();
    assert_eq!(
        s.replace_visible_read_subscriptions(a, &[]).unwrap(),
        vec![OrphanedRead {
            target,
            generation: orphan.generation
        }]
    );
}

#[test]
fn stale_visible_response_cannot_publish_another_item_or_change_cache_after_deselection() {
    use threadline_storage::reader::ListSummary;
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (a, r) = install(&s);
    let item = s.ensure_placeholder(a, r, 10).unwrap();
    let target = ReadTarget {
        repository: r,
        item: Some(item),
        family: ResourceFamily::Metadata,
        revision: String::new(),
    };
    s.subscribe_read(a, &target, "left", ReadPriority::Visible, START)
        .unwrap();
    let claim = s.claim_sync_read(a, START).unwrap().unwrap();
    let row = |n| ListSummary {
        number: n,
        update: issue(n, START, false).update,
        facets: Default::default(),
    };
    assert!(
        s.commit_sync_item_metadata(&claim, row(500), &tag("a"), START + 1, "UTC")
            .is_err()
    );
    assert!(
        s.resolve(
            a,
            ItemLocator {
                repository: r,
                number: 500
            }
        )
        .unwrap()
        .is_none()
    );
    s.select_github_repository(a, r, false).unwrap();
    s.select_github_repository(a, r, true).unwrap();
    assert!(
        s.commit_sync_item_metadata(&claim, row(10), &tag("a"), START + 2, "UTC")
            .is_err()
    );
    assert_eq!(s.item(a, item).unwrap().unwrap().title, Change::Missing);
    let fresh = s.claim_sync_read(a, START + 1000).unwrap().unwrap();
    let (route, remote, profile) = s.read_sync_repository(&fresh, START + 1000).unwrap();
    assert_eq!(
        (route.owner(), route.name(), remote.get()),
        ("example", "selected", 10)
    );
    assert_eq!(profile.account, a);
    assert!(s.read_sync_repository(&claim, START + 1000).is_err());
    assert_eq!(
        s.commit_sync_item_metadata(&fresh, row(10), &tag("a"), START + 1001, "UTC")
            .unwrap(),
        item
    );
    assert_eq!(
        s.item(a, item).unwrap().unwrap().title,
        Change::Value("Issue 10".into())
    );
}

#[test]
fn foreground_admission_distinguishes_local_spacing_from_the_persisted_server_floor() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (account, _) = install(&s);
    assert_eq!(
        s.reserve_read_budget(account, START).unwrap(),
        ReadBudget::Granted
    );
    assert_eq!(
        s.reserve_read_budget(account, START + 1).unwrap(),
        ReadBudget::Deferred {
            until_ms: START + 1000,
            server: false
        }
    );
    s.hold_sync_budget(account, START + 50_000).unwrap();
    s.hold_sync_budget(account, START + 10_000).unwrap();
    drop(s);
    let s = Store::open_profile(dir.path()).unwrap();
    assert_eq!(s.server_read_budget(account).unwrap(), Some(START + 50_000));
    assert_eq!(
        s.reserve_read_budget(account, START + 10_000).unwrap(),
        ReadBudget::Deferred {
            until_ms: START + 50_000,
            server: true
        }
    );
    assert_eq!(
        s.reserve_read_budget(account, START + 50_000).unwrap(),
        ReadBudget::Granted
    );
}

#[test]
fn it_018_unopened_issue_comment_uses_real_feed_and_preserves_comment_off_mention_popup() {
    use threadline_storage::notifications::*;
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (a, r) = install(&s);
    s.update_notification_settings(a, |n| {
        n.fallback = DeliveryMode::Off;
        n.categories
            .insert(Category::DirectMention, DeliveryMode::Popup);
    })
    .unwrap();
    let target = feed(r, ResourceFamily::Comments);
    s.subscribe_read(a, &target, "activity", ReadPriority::Activity, START)
        .unwrap();
    let first = s.claim_sync_read(a, START).unwrap().unwrap();
    let mut historical = comment(44, 31, 50, 50);
    historical.comment.body = "@test, old".into();
    s.commit_observed_page(
        &first,
        ObservationPage {
            comments: vec![historical],
            ..no_changes()
        },
        START + 1,
        "UTC",
    )
    .unwrap();
    assert_eq!(
        s.process_notification_events(a, START + 2, "UTC", true, 10)
            .unwrap(),
        0
    );
    let later = START + 3_600_000;
    let next = s.claim_sync_read(a, later).unwrap().unwrap();
    let mut new = comment(44, 32, later - 2, later - 2);
    new.comment.body = "Follow up @TeSt about this.".into();
    let mut literal = comment(45, 33, later - 1, later - 1);
    literal.comment.body = "contact mail@test or `@test`".into();
    s.commit_observed_page(
        &next,
        ObservationPage {
            comments: vec![new, literal],
            ..no_changes()
        },
        later,
        "UTC",
    )
    .unwrap();
    assert_eq!(
        s.process_notification_events(a, later + 1, "UTC", true, 10)
            .unwrap(),
        2
    );
    let popup = s
        .claim_notification_popup(a, later + 2, "UTC", true)
        .unwrap()
        .unwrap();
    assert_eq!(popup.event_count, 1);
    let visible = s.notification_feed(a, None, 10).unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].event_kind, "comment_created");
    assert_eq!(
        visible[0].categories,
        [Category::OrdinaryComment, Category::DirectMention].into()
    );
    assert_eq!(
        visible[0].decision.eligible,
        [Category::DirectMention].into()
    );
    assert!(
        matches!(&popup.destination.target, NotificationTarget::Item{thread:Some(k),..} if k=="issue-comment:32")
    );
}

#[test]
fn it_018_issue_assignment_tracks_stable_ids_across_restarts_and_missing_fields() {
    use threadline_storage::notifications::{
        Category, DeliveryMode, NotificationTarget, Relevance,
    };
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_profile(dir.path()).unwrap();
    let (account, repository) = install(&store);
    store
        .update_notification_settings(account, |settings| {
            settings.fallback = DeliveryMode::Off;
            settings
                .categories
                .insert(Category::IssueAssignment, DeliveryMode::Popup);
            settings.relevance = Relevance::InvolvingMe;
        })
        .unwrap();
    store
        .subscribe_read(
            account,
            &feed(repository, ResourceFamily::RepositoryItems),
            "activity",
            ReadPriority::Activity,
            START,
        )
        .unwrap();
    let row = |number, updated, assignees: Option<&[u64]>| {
        let mut row = issue(number, updated, false);
        row.assignee_ids = assignees.map(|ids| ids.iter().copied().map(id).collect());
        row
    };
    let walk = |store: &Store, now, items| {
        let claim = store.claim_sync_read(account, now).unwrap().unwrap();
        store
            .commit_observed_page(
                &claim,
                ObservationPage {
                    items,
                    ..no_changes()
                },
                now,
                "UTC",
            )
            .unwrap();
        store
            .process_notification_events(account, now + 1, "UTC", true, 100)
            .unwrap()
    };
    let hour = 3_600_000;
    assert_eq!(
        walk(
            &store,
            START,
            vec![row(44, 50, Some(&[5])), row(45, 50, None)]
        ),
        0
    );
    let next = START + hour;
    let mut pr = row(46, next, Some(&[102]));
    if let ItemRepresentation::Issue(record) = &mut pr.update.representation {
        record.classification = IssueClassification::PullRequest;
    }
    // Changes to another issue's general fields do not establish who assigned it.
    assert_eq!(
        walk(
            &store,
            next,
            vec![
                row(44, next - 4, Some(&[5, 102])),
                row(44, next - 3, None),
                row(44, next - 4, Some(&[])),
                row(44, next - 2, Some(&[102, 5])),
                row(45, next, Some(&[102])),
                pr,
            ]
        ),
        1
    );
    let first = store.notification_feed(account, None, 10).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].categories, [Category::IssueAssignment].into());
    assert_eq!(first[0].decision.mode, DeliveryMode::Popup);
    let target = store.ensure_placeholder(account, repository, 44).unwrap();
    assert_eq!(
        first[0].target,
        NotificationTarget::Item {
            item: target,
            thread: None
        }
    );
    drop(store);
    let store = Store::open_profile(dir.path()).unwrap();
    let latest = next + hour;
    assert_eq!(
        walk(
            &store,
            latest,
            vec![
                row(44, next - 4, Some(&[5, 102])),
                row(44, latest - 2, Some(&[5])),
                row(44, latest - 1, Some(&[5, 77])),
                row(44, latest, Some(&[77, 5])),
            ]
        ),
        2
    );
    let visible = store.notification_feed(account, None, 10).unwrap();
    assert_eq!(visible.len(), 2);
    assert_eq!(visible[0].occurred_at_ms, Some(latest - 2));
    assert_eq!(visible[1].event_key, first[0].event_key);
}
