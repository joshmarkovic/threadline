use rusqlite::Connection;
use threadline_storage::{Store, resources::*};

struct TestStore {
    dir: tempfile::TempDir,
    key: ResourceKey,
    store: Store,
}

impl TestStore {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("profile.sqlite")).unwrap();
        let account = store.ensure_account("fixtures.invalid", 11).unwrap();
        let repo = store
            .ensure_repository(account, 22, "fixture", "repo")
            .unwrap();
        let item = store.ensure_placeholder(account, repo, 41).unwrap();
        let key = ResourceKey {
            account,
            target: ResourceTarget::Item(item),
            family: ResourceFamily::Comments,
        };
        Self { dir, key, store }
    }
}

fn record(key: &str, body: &str) -> ResourceRecord {
    ResourceRecord {
        key: key.into(),
        body: body.into(),
    }
}

fn page<'a>(
    records: &'a [ResourceRecord],
    next_cursor: Option<&'a str>,
    at: i64,
) -> ResourcePage<'a> {
    ResourcePage {
        records,
        next_cursor,
        fetched_at_ms: at,
        completed_validators: None,
    }
}

#[test]
fn it_002_failed_page_is_not_partially_committed_and_restarts_without_duplicates() {
    let TestStore { dir, key, store } = TestStore::new();
    let started = store.start_resource_fetch(key, WalkMode::Full).unwrap();
    let cursor = store
        .commit_resource_page(
            key,
            &started,
            page(&[record("a", "first")], Some("page-two"), 100),
        )
        .unwrap()
        .unwrap();
    let sql = Connection::open(dir.path().join("profile.sqlite")).unwrap();
    sql.execute_batch(
        "CREATE TRIGGER reject_download BEFORE INSERT ON cached_resource_pending
         WHEN NEW.record_key='trigger' BEGIN SELECT RAISE(ABORT, 'simulated disk write failure'); END;",
    ).unwrap();
    assert!(
        store
            .commit_resource_page(
                key,
                &cursor,
                page(
                    &[record("b", "candidate"), record("trigger", "failed")],
                    Some("page-three"),
                    200
                )
            )
            .is_err()
    );
    assert_eq!(store.resource_records(key).unwrap(), [record("a", "first")]);
    assert_eq!(
        store.resource_state(key).unwrap().last_page_at_ms,
        Some(100)
    );
    sql.execute_batch("DROP TRIGGER reject_download").unwrap();
    drop(sql);
    drop(store);

    let store = Store::open(&dir.path().join("profile.sqlite")).unwrap();
    let resume = store.resume_resource_fetch(key).unwrap().unwrap();
    assert_eq!(resume.cursor.as_deref(), Some("page-two"));
    assert!(
        store
            .commit_resource_page(key, &cursor, page(&[record("stale", "attempt")], None, 201))
            .is_err()
    );
    assert!(
        store
            .commit_resource_page(
                key,
                &resume,
                page(&[record("never", "written")], Some("page-two"), 202)
            )
            .is_err()
    );
    assert!(
        store
            .commit_resource_page(
                key,
                &resume,
                page(&[record("a", "first"), record("b", "second")], None, 210)
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.resource_records(key).unwrap(),
        [record("a", "first"), record("b", "second")]
    );
    assert_eq!(
        store.resource_state(key).unwrap().coverage,
        Coverage::Complete
    );
    assert!(store.resume_resource_fetch(key).unwrap().is_none());
}

#[test]
fn resource_coverage_freshness_errors_and_families_remain_independent() {
    let TestStore { dir, key, store } = TestStore::new();
    let metadata = ResourceKey {
        family: ResourceFamily::Metadata,
        ..key
    };
    let download = store
        .start_resource_fetch(metadata, WalkMode::Full)
        .unwrap();
    let validators = Validators {
        etag: Some("verified-etag".into()),
        last_modified: None,
        revision: Some("body-rev-1".into()),
    };
    store
        .commit_resource_page(
            metadata,
            &download,
            ResourcePage {
                completed_validators: Some(&validators),
                ..page(&[], None, 100)
            },
        )
        .unwrap();
    assert!(store.resource_state(metadata).unwrap().shows_empty(0));
    assert_eq!(
        store.resource_state(key).unwrap().coverage,
        Coverage::Missing
    );
    assert!(
        store
            .start_resource_fetch(key, WalkMode::Incremental)
            .is_err()
    );
    let refresh = store
        .start_resource_fetch(metadata, WalkMode::Full)
        .unwrap();
    store
        .resource_not_modified(metadata, &refresh, &validators, 150)
        .unwrap();
    assert_eq!(
        store.resource_state(metadata).unwrap().last_fetch_at_ms,
        Some(100)
    );

    let download = store.start_resource_fetch(key, WalkMode::Full).unwrap();
    store
        .commit_resource_page(key, &download, page(&[record("one", "cached")], None, 100))
        .unwrap();
    let refresh = store.start_resource_fetch(key, WalkMode::Full).unwrap();
    let next = store
        .commit_resource_page(
            key,
            &refresh,
            page(
                &[record("new", "partial replacement")],
                Some("expired"),
                175,
            ),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        store.resource_records(key).unwrap(),
        [record("one", "cached")]
    );
    store
        .record_resource_error(key, &next, ResourceError::Unauthorized)
        .unwrap();
    let failed = ResourceKey {
        family: ResourceFamily::Files,
        ..key
    };
    let download = store.start_resource_fetch(failed, WalkMode::Full).unwrap();
    store
        .record_resource_error(failed, &download, ResourceError::Transport)
        .unwrap();
    drop(store);

    let store = Store::open(&dir.path().join("profile.sqlite")).unwrap();
    let comments = store.resource_state(key).unwrap();
    assert_eq!(
        (comments.coverage, comments.fetch, comments.error),
        (
            Coverage::Complete,
            FetchState::Unavailable,
            Some(ResourceError::Unauthorized)
        )
    );
    assert!(comments.is_stale(200, 10));
    assert!(!store.resource_state(metadata).unwrap().is_stale(160, 10));
    assert_eq!(
        store.resource_state(failed).unwrap().fetch,
        FetchState::Failed
    );
    assert_eq!(
        store.resource_records(key).unwrap(),
        [record("one", "cached")]
    );
    let resumed = store.resume_resource_fetch(key).unwrap().unwrap();
    store
        .record_resource_error(key, &resumed, ResourceError::CursorExpired)
        .unwrap();
    assert!(store.resume_resource_fetch(key).unwrap().is_none());
    let fresh = store.start_resource_fetch(key, WalkMode::Full).unwrap();
    store
        .commit_resource_page(key, &fresh, page(&[], None, 230))
        .unwrap();
    assert!(
        store
            .resource_state(key)
            .unwrap()
            .shows_empty(store.resource_records(key).unwrap().len())
    );
}

#[test]
fn resource_graph_comparisons_and_events_are_revision_and_account_scoped() {
    let TestStore { dir, key, store } = TestStore::new();
    let ResourceTarget::Item(item) = key.target else {
        panic!("expected item")
    };
    let repo = store
        .ensure_repository(key.account, 22, "fixture", "repo")
        .unwrap();
    let other = store.ensure_account("fixtures.invalid", 81).unwrap();
    let base = CommitOid::parse(&"a".repeat(40)).unwrap();
    let head = CommitOid::parse(&"b".repeat(40)).unwrap();
    let merge = GraphCommit {
        oid: head.clone(),
        title: "merge".into(),
        committed_at_ms: 40,
        parents: vec![base.clone(), CommitOid::parse(&"c".repeat(40)).unwrap()],
    };
    store
        .save_commits(key.account, repo, std::slice::from_ref(&merge))
        .unwrap();
    let comparison = ComparisonKey {
        repository: repo,
        base: base.clone(),
        head: head.clone(),
        mode: ComparisonMode::Direct,
        ignore_whitespace: false,
        context_lines: 3,
    };
    let files = [ComparisonFile {
        path: "../remote-label.rs".into(),
        previous_path: None,
        patch: Some("+fixture".into()),
    }];
    store
        .save_comparison(key.account, &comparison, &files)
        .unwrap();
    assert!(
        store
            .save_comparison(key.account, &comparison, &[])
            .is_err()
    );
    assert!(store.load_comparison(other, &comparison).is_err());
    assert!(
        store
            .load_comparison(
                key.account,
                &ComparisonKey {
                    ignore_whitespace: true,
                    ..comparison.clone()
                }
            )
            .unwrap()
            .is_none()
    );
    let branch = GraphRef {
        name: "refs/heads/topic".into(),
        target: Some(base.clone()),
        observed_at_ms: 100,
    };
    store.save_graph_ref(key.account, repo, &branch).unwrap();
    store
        .save_graph_ref(
            key.account,
            repo,
            &GraphRef {
                target: None,
                observed_at_ms: 110,
                ..branch.clone()
            },
        )
        .unwrap();
    assert!(!store.save_graph_ref(key.account, repo, &branch).unwrap());
    let check = RevisionState {
        payload: "successful".into(),
        observed_at_ms: 100,
    };
    store
        .save_revision_state(key.account, item, RevisionFamily::Checks, &base, &check)
        .unwrap();

    let event = ActivityEvent {
        repository: repo,
        item: Some(item),
        kind: "comment".into(),
        revision: None,
        occurred_at_ms: None,
        observed_at_ms: 120,
    };
    assert!(
        store
            .record_activity_event(key.account, "stable-comment-key", &event)
            .unwrap()
    );
    assert!(
        !store
            .record_activity_event(
                key.account,
                "stable-comment-key",
                &ActivityEvent {
                    kind: "mention".into(),
                    ..event.clone()
                }
            )
            .unwrap()
    );
    assert!(
        store
            .record_activity_event(other, "stable-comment-key", &event)
            .is_err()
    );
    let delivered = DeliveryState {
        phase: DeliveryPhase::Delivered,
        group_key: Some("item".into()),
        native_id: Some("native-1".into()),
        reason: None,
        updated_at_ms: 130,
    };
    store
        .save_delivery_state(key.account, "stable-comment-key", &delivered)
        .unwrap();
    assert!(
        !store
            .save_delivery_state(
                key.account,
                "stable-comment-key",
                &DeliveryState {
                    phase: DeliveryPhase::Eligible,
                    updated_at_ms: 140,
                    ..delivered.clone()
                }
            )
            .unwrap()
    );
    let baseline = ActivityBaseline {
        checkpoint: "checkpoint-1".into(),
        observed_at_ms: 130,
    };
    let job = SyncJob {
        phase: SyncPhase::Held,
        resume_after_ms: Some(200),
        checkpoint: Some("last-committed-page".into()),
        overlap_from_ms: Some(170),
        updated_at_ms: 180,
        reason: Some(ResourceError::RateLimited),
    };
    store
        .save_activity_baseline(key.account, repo, "comments", &baseline)
        .unwrap();
    store
        .save_sync_job(key.account, repo, ResourceFamily::Comments, &job)
        .unwrap();
    let page_one = store.start_resource_fetch(key, WalkMode::Full).unwrap();
    store
        .commit_resource_page(
            key,
            &page_one,
            page(&[record("checkpoint", "retained")], Some("next"), 130),
        )
        .unwrap();
    drop(store);

    let store = Store::open(&dir.path().join("profile.sqlite")).unwrap();
    assert_eq!(
        store.load_commit(key.account, repo, &head).unwrap(),
        Some(merge)
    );
    assert_eq!(
        store.load_comparison(key.account, &comparison).unwrap(),
        Some(files.to_vec())
    );
    assert!(
        store
            .load_graph_ref(key.account, repo, &branch.name)
            .unwrap()
            .unwrap()
            .target
            .is_none()
    );
    assert_eq!(
        store
            .load_revision_state(key.account, item, RevisionFamily::Checks, &base)
            .unwrap(),
        Some(check)
    );
    assert!(
        store
            .load_revision_state(key.account, item, RevisionFamily::Checks, &head)
            .unwrap()
            .is_none()
    );
    store.clear_cache(key.account).unwrap();
    assert_eq!(
        store.resource_records(key).unwrap(),
        [record("checkpoint", "retained")]
    );
    assert_eq!(
        store
            .resume_resource_fetch(key)
            .unwrap()
            .unwrap()
            .cursor
            .as_deref(),
        Some("next")
    );
    assert_eq!(
        store
            .load_activity_event(key.account, "stable-comment-key")
            .unwrap(),
        Some(event)
    );
    assert_eq!(
        store
            .load_delivery_state(key.account, "stable-comment-key")
            .unwrap(),
        Some(delivered)
    );
    assert_eq!(
        store
            .load_activity_baseline(key.account, repo, "comments")
            .unwrap(),
        Some(baseline)
    );
    assert_eq!(
        store
            .load_sync_job(key.account, repo, ResourceFamily::Comments)
            .unwrap(),
        Some(job)
    );
}

#[test]
fn resource_activity_requires_a_current_or_verified_historical_locator() {
    use threadline_core::domain::IssueApiId;
    use threadline_storage::{
        Change, IssueClassification, IssueRecord, ItemLocator, ItemRepresentation, ItemUpdate,
        TransferEvidence, TransferOutcome, UtcMillis,
    };

    let TestStore {
        dir: _dir,
        key,
        store,
    } = TestStore::new();
    let old = store
        .ensure_repository(key.account, 22, "fixture", "repo")
        .unwrap();
    let new = store
        .ensure_repository(key.account, 23, "fixture", "destination")
        .unwrap();
    let unrelated = store
        .ensure_repository(key.account, 24, "fixture", "unrelated")
        .unwrap();
    let from = ItemLocator {
        repository: old,
        number: 41,
    };
    let to = ItemLocator {
        repository: new,
        number: 9,
    };
    let issue_id = IssueApiId::try_from(200_u64).unwrap();
    let item = store
        .ingest(
            key.account,
            from,
            ItemUpdate {
                representation: ItemRepresentation::Issue(IssueRecord {
                    id: issue_id,
                    node_id: None,
                    classification: IssueClassification::Issue,
                }),
                remote_updated_at: UtcMillis(100),
                title: Change::Value("issue".into()),
                body: Change::Missing,
                closed: Change::Missing,
            },
        )
        .unwrap();
    assert!(matches!(
        store
            .transfer_issue(
                key.account,
                from,
                to,
                TransferEvidence::VerifiedIssueId(issue_id)
            )
            .unwrap(),
        TransferOutcome::Moved(_)
    ));
    let historical = ActivityEvent {
        repository: old,
        item: Some(item),
        kind: "comment".into(),
        revision: None,
        occurred_at_ms: Some(150),
        observed_at_ms: 200,
    };
    assert!(
        store
            .record_activity_event(key.account, "historical", &historical)
            .unwrap()
    );
    assert!(
        store
            .record_activity_event(
                key.account,
                "current",
                &ActivityEvent {
                    repository: new,
                    ..historical.clone()
                }
            )
            .unwrap()
    );
    assert!(
        store
            .record_activity_event(
                key.account,
                "unrelated",
                &ActivityEvent {
                    repository: unrelated,
                    ..historical
                }
            )
            .is_err()
    );
    assert!(
        store
            .load_activity_event(key.account, "unrelated")
            .unwrap()
            .is_none()
    );
}
