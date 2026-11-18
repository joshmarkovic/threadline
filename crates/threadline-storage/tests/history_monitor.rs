use threadline_storage as storage;
#[path = "support/write_profile.rs"]
mod write_profile;

use storage::{
    AccountId, RepositoryId, Store,
    history::{HistoryCoverage, HistoryEntry, HistoryMonitoring, HistoryRef, RefMovement},
    resources::{CommitOid, GraphCommit, Validators},
    sync::{HistoryRead, MonitoredAncestors, MonitoredBranches, ReadClaim, ReadCompletion},
};
use write_profile::{id, setup};

fn oid(digit: char) -> CommitOid {
    CommitOid::parse(&digit.to_string().repeat(40)).unwrap()
}

fn branch(name: &str) -> HistoryRef {
    HistoryRef::Branch(name.into())
}

#[track_caller]
fn next(
    store: &Store,
    account: AccountId,
    now: &mut i64,
    expected: impl Fn(HistoryRead) -> bool,
) -> ReadClaim {
    for _ in 0..25 {
        *now += 1_000;
        if let Some(claim) = store.claim_sync_read(account, *now).unwrap() {
            if HistoryRead::from_target(&claim.target).is_some_and(&expected) {
                return claim;
            }
            store
                .finish_sync_read(
                    &claim,
                    ReadCompletion::Fetched(Validators::default()),
                    *now,
                    "UTC",
                )
                .unwrap();
        }
    }
    panic!("Expected monitored history job was not claimed");
}

#[track_caller]
fn inventory(store: &Store, account: AccountId, now: &mut i64) -> ReadClaim {
    next(store, account, now, |job| {
        matches!(job, HistoryRead::Branches)
    })
}

fn refs(store: &Store, account: AccountId, repo: RepositoryId) -> Vec<HistoryRef> {
    store
        .history_refs(account, repo, None, 20)
        .unwrap()
        .into_iter()
        .map(|row| row.reference)
        .collect()
}

#[test]
fn it_006_scoped_inventory_resumes_and_only_a_complete_first_page_can_prove_absence() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_profile(dir.path()).unwrap();
    let (account, repo, _) = setup(&store);
    let mut now = 1_780_000_000_000;
    store
        .set_history_monitoring(
            account,
            repo,
            &HistoryMonitoring {
                default_branch: Some("main".into()),
                branches: ["manual".into()].into(),
                branch_patterns: ["release/*".into()].into(),
                followed_prs: [id(42)].into(),
            },
        )
        .unwrap();
    store.schedule_selected_activity(account, now).unwrap();

    let page = inventory(&store, account, &mut now);
    let rows = [
        ("release/one".into(), oid('a')),
        ("unwatched".into(), oid('e')),
    ];
    assert!(
        store
            .commit_history_branches(
                &page,
                MonitoredBranches {
                    rows: &rows,
                    next: Some(2),
                    partial: false,
                },
                now,
                "UTC"
            )
            .unwrap()
    );
    assert_eq!(refs(&store, account, repo), [branch("release/one")]);
    assert!(
        store
            .history_ref_moves(account, repo, 0, 20)
            .unwrap()
            .is_empty()
    );
    drop(store);

    let store = Store::open_profile(dir.path()).unwrap();
    let mut settings = store.sync_settings(account).unwrap();
    for offline in [true, false] {
        settings.controls.work_offline = offline;
        settings.controls.pause_background = !offline;
        store.save_sync_settings(account, &settings).unwrap();
        now += 1_000;
        assert!(store.claim_sync_read(account, now).unwrap().is_none());
    }
    settings.controls.pause_background = false;
    store.save_sync_settings(account, &settings).unwrap();
    store.schedule_selected_activity(account, now).unwrap();
    let page = inventory(&store, account, &mut now);
    assert_eq!(page.cursor.as_deref(), Some("2"));
    store
        .commit_history_branches(
            &page,
            MonitoredBranches {
                rows: &[],
                next: None,
                partial: false,
            },
            now,
            "UTC",
        )
        .unwrap();
    now += 86_400_000;
    let followed = HistoryRef::FollowedPr(id(42));
    let pr = next(
        &store,
        account,
        &mut now,
        |job| matches!(job, HistoryRead::Reference(r) if r == followed),
    );
    assert!(
        store
            .commit_history_ref(&pr, Some(&oid('f')), now, "UTC")
            .unwrap()
    );
    assert_eq!(
        refs(&store, account, repo),
        [branch("release/one"), followed]
    );

    now += 86_400_000;
    let partial = inventory(&store, account, &mut now);
    assert!(
        !store
            .commit_history_branches(
                &partial,
                MonitoredBranches {
                    rows: &[],
                    next: None,
                    partial: true,
                },
                now,
                "UTC"
            )
            .unwrap()
    );
    assert_eq!(
        store
            .history_ref(account, repo, &branch("release/one"))
            .unwrap()
            .unwrap()
            .tip,
        Some(oid('a'))
    );
    now += 86_400_000;
    let complete = inventory(&store, account, &mut now);
    assert!(
        store
            .commit_history_branches(
                &complete,
                MonitoredBranches {
                    rows: &[],
                    next: None,
                    partial: false,
                },
                now,
                "UTC"
            )
            .unwrap()
    );
    assert_eq!(
        store
            .history_ref(account, repo, &branch("release/one"))
            .unwrap()
            .unwrap()
            .tip,
        None
    );
}

fn commit(digit: char, parent: char) -> HistoryEntry {
    HistoryEntry {
        commit: GraphCommit {
            oid: oid(digit),
            title: "Fixture change".into(),
            committed_at_ms: 10,
            parents: vec![oid(parent)],
        },
        message: "Fixture change".into(),
        author: Some("Git author".into()),
        authored_at_ms: Some(9),
    }
}

#[test]
fn it_006_ancestry_budget_preserves_older_checkpoint_and_read_claims_do_not_outlive_scope() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_profile(dir.path()).unwrap();
    let (account, repo, _) = setup(&store);
    let mut now = 1_780_000_000_000;
    let settings = HistoryMonitoring {
        branches: ["main".into()].into(),
        ..Default::default()
    };
    store
        .set_history_monitoring(account, repo, &settings)
        .unwrap();
    store.schedule_selected_activity(account, now).unwrap();
    let main = |job| matches!(job, HistoryRead::Reference(r) if r == branch("main"));
    let ancestor = |job| matches!(job, HistoryRead::Ancestors(r) if r == branch("main"));
    let initial = next(&store, account, &mut now, main);
    assert!(
        store
            .commit_history_ref(&initial, Some(&oid('a')), now, "UTC")
            .unwrap()
    );
    assert!(
        store
            .history_ref_moves(account, repo, 0, 20)
            .unwrap()
            .is_empty()
    );

    now += 86_400_000;
    let unavailable = next(&store, account, &mut now, main);
    assert!(
        !store
            .commit_history_ref(&unavailable, None, now, "UTC")
            .unwrap()
    );
    assert_eq!(
        store
            .history_ref(account, repo, &branch("main"))
            .unwrap()
            .unwrap()
            .tip,
        Some(oid('a'))
    );
    now += 86_400_000;
    let moved = next(&store, account, &mut now, main);
    assert!(
        store
            .commit_history_ref(&moved, Some(&oid('d')), now, "UTC")
            .unwrap()
    );
    for (tip, parent, cursor) in [('d', 'c', "2"), ('c', 'a', "3")] {
        let claim = next(&store, account, &mut now, ancestor);
        let walk = store
            .prepare_history_ancestors(&claim, now)
            .unwrap()
            .unwrap();
        assert_eq!(walk.tip, oid('d'));
        assert!(
            store
                .commit_history_ancestors(
                    &claim,
                    MonitoredAncestors {
                        walk: Some(&walk),
                        rows: &[commit(tip, parent)],
                        next: Some(cursor),
                        partial: false,
                    },
                    now,
                    "UTC"
                )
                .unwrap()
        );
    }
    assert!(
        !store
            .claim_sync_read(account, now + 1_000)
            .unwrap()
            .is_some_and(|claim| HistoryRead::from_target(&claim.target).is_some_and(ancestor))
    );
    let profile = store
        .github_access_profile(account, "work")
        .unwrap()
        .unwrap();
    assert_eq!(
        store
            .begin_history_page(&profile, repo, &branch("main"))
            .unwrap()
            .unwrap()
            .cursor
            .as_deref(),
        Some("3")
    );
    assert_eq!(
        store
            .history_page(account, repo, &oid('d'), None, 10)
            .unwrap()
            .coverage,
        HistoryCoverage::Partial
    );
    let moves = store.history_ref_moves(account, repo, 0, 20).unwrap();
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].movement, RefMovement::FastForward);
    assert_eq!(moves[0].previous, oid('a'));
    assert_eq!(moves[0].current, oid('d'));

    now += 86_400_000;
    let orphan = next(&store, account, &mut now, main);
    store
        .set_history_monitoring(account, repo, &HistoryMonitoring::default())
        .unwrap();
    store.schedule_selected_activity(account, now).unwrap();
    store
        .set_history_monitoring(account, repo, &settings)
        .unwrap();
    store.schedule_selected_activity(account, now).unwrap();
    let current = next(&store, account, &mut now, main);
    assert_ne!(orphan.generation, current.generation);
    assert!(
        store
            .commit_history_ref(&orphan, Some(&oid('e')), now, "UTC")
            .is_err()
    );
    store
        .select_github_repository(account, repo, false)
        .unwrap();
    assert!(
        store
            .commit_history_ref(&current, Some(&oid('e')), now, "UTC")
            .is_err()
    );
    assert_eq!(
        store
            .history_ref(account, repo, &branch("main"))
            .unwrap()
            .unwrap()
            .tip,
        Some(oid('d'))
    );
}
