use rusqlite::Connection;
use std::num::NonZeroU64;
use threadline_storage::{
    Store,
    access::{
        AccessState, CredentialKind, DiscoveryState, RepositoryObservation,
        RepositoryObservationSource,
    },
    fixtures::{Scale, backlog_comment, populate, populate_large_workspace},
    queue::CancelResult,
    resources::{ResourceFamily, ResourceKey, ResourceTarget},
    writes::WriteIntent,
};

fn seed() -> u64 {
    std::env::var("THREADLINE_TEST_SEED")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(12345)
}

#[test]
fn same_seed_reproduces_small_fixture_and_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let base = Store::open(&dir.path().join("base.sqlite")).unwrap();
    let replay = Store::open(&dir.path().join("replay.sqlite")).unwrap();
    let left = populate(&base, seed(), Scale::Small).unwrap();
    let right = populate(&replay, seed(), Scale::Small).unwrap();
    assert_eq!(left, right);
    let comments = ResourceKey {
        account: left.account,
        target: ResourceTarget::Item(left.long_discussion),
        family: ResourceFamily::Comments,
    };
    assert_eq!(
        base.resource_records(comments).unwrap(),
        replay.resource_records(comments).unwrap()
    );
    assert_eq!(
        base.workspace(left.account).unwrap(),
        replay.workspace(right.account).unwrap()
    );
    assert_eq!(
        base.load_comparison(left.account, &left.comparison)
            .unwrap(),
        replay
            .load_comparison(right.account, &right.comparison)
            .unwrap()
    );
    assert!(populate(&base, seed(), Scale::Small).is_err());
}

#[test]
fn section_16_fixture_contains_full_persisted_workload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("section-16.sqlite");
    let store = Store::open(&path).unwrap();
    let fixture = populate(&store, seed(), Scale::Section16).unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    let sql = Connection::open(&path).unwrap();
    for (query, count) in [
        ("SELECT count(*) FROM items", 50_000),
        ("SELECT count(*) FROM cached_resource_records", 250_000),
        ("SELECT count(*) FROM cached_comparison_files", 40),
        ("SELECT count(*) FROM cached_commits", 400),
        ("SELECT count(*) FROM operations", 1_000),
        ("SELECT count(*) FROM drafts WHERE kind='review'", 1),
    ] {
        let actual: i64 = sql.query_row(query, [], |row| row.get(0)).unwrap();
        assert_eq!(actual, count, "{query}");
    }
    let full_graph_merges: i64 = sql
        .query_row(
            "SELECT count(*) FROM cached_commit_parents WHERE parent_index=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(full_graph_merges > 0);
    let dependencies: i64 = sql
        .query_row("SELECT count(*) FROM operation_dependencies", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(dependencies > 0);
    for state in ["queued", "attention", "confirmed"] {
        let count: i64 = sql
            .query_row(
                "SELECT count(*) FROM operations WHERE state=?1",
                [state],
                |row| row.get(0),
            )
            .unwrap();
        assert!(count > 0, "{state}");
    }
    let ws = store.workspace(fixture.account).unwrap().unwrap();
    assert_eq!(ws.panes.len(), 2);
    assert_eq!(
        ws.panes.iter().map(|pane| pane.tabs.len()).sum::<usize>(),
        20
    );
    let right = ws.panes[1].tabs.first().unwrap();
    assert_eq!(right.item, fixture.reviewed_pr);
    assert_eq!(
        right.view.context.as_ref().unwrap().selected_issue,
        Some(fixture.long_discussion)
    );
    assert_eq!(right.view.draft.as_deref(), Some("fixture-review-draft"));
    let alternate = &ws.panes[1].tabs[1];
    assert_ne!(alternate.item, fixture.reviewed_pr);
    assert_eq!(alternate.view.context.as_ref().unwrap().ratio, 0.23);
    let discussion = ResourceKey {
        account: fixture.account,
        target: ResourceTarget::Item(fixture.long_discussion),
        family: ResourceFamily::Comments,
    };
    assert_eq!(store.resource_records(discussion).unwrap().len(), 5_005);
}

#[test]
fn large_workspace_cancelled_dependency_leaves_an_independent_ordered_backlog() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("section-16-backlog.sqlite");
    let store = Store::open(&path).unwrap();
    let fixture = populate_large_workspace(&store, seed()).unwrap();
    let account = fixture.account;
    const CANCELLED: &str = "00000000-0000-4000-8001-000000000000";
    const DEPENDENT: &str = "00000000-0000-4000-8001-000000000001";
    const FIRST: &str = "00000000-0000-4000-8001-000000000002";
    assert_eq!(
        store.cancel_operation(account, CANCELLED).unwrap(),
        CancelResult::Cancelled
    );
    assert!(store.claim_write(account, 20, 1000).unwrap().is_none());
    drop(store);

    let store = Store::open(&path).unwrap();
    let previous = store
        .github_access_profile(account, "fixture-org")
        .unwrap()
        .unwrap();
    let profile = store
        .reserve_access_profile_credential(
            account,
            NonZeroU64::new(41).unwrap(),
            "fixture-user",
            "fixture-org",
            CredentialKind::FineGrained,
            Some(&previous),
        )
        .unwrap();
    store
        .install_current_credential(&profile, || Ok::<_, ()>(()))
        .unwrap()
        .unwrap();
    store
        .observe_github_repository(
            &profile,
            &RepositoryObservation {
                provider_id: NonZeroU64::new(71).unwrap(),
                owner: "fixture-org",
                name: "private",
                private: true,
                source: RepositoryObservationSource::Direct,
            },
            22,
        )
        .unwrap();
    store
        .record_access_result(
            &profile,
            profile.kind,
            AccessState::Readable,
            DiscoveryState::Unchecked,
            23,
        )
        .unwrap();
    store.set_dispatch_controls(account, false, false).unwrap();

    assert_eq!(
        store.dispatch_hold(account, DEPENDENT).unwrap().as_deref(),
        Some("dependency")
    );
    let first = store.claim_write(account, 24, 1000).unwrap().unwrap();
    assert_eq!(first.operation_id(), FIRST);
    assert_eq!(first.route.item, fixture.long_discussion);
    assert_eq!(first.route.number.get(), 1);
    assert_eq!(
        first.intent,
        WriteIntent::CreateComment {
            body: backlog_comment(2)
        }
    );
    assert!(store.claim_write(account, 25, 1000).unwrap().is_none());
}
