use std::num::NonZeroU64;
use threadline_storage::{
    fixtures::{Scale, populate, populate_reader},
    reader::*,
    resources::*,
    *,
};
fn page(repo: u64, name: &str, at: i64, assigned: Option<Vec<String>>) -> ListSummary {
    ListSummary {
        number: repo,
        update: ItemUpdate {
            representation: ItemRepresentation::Issue(IssueRecord {
                id: IssueApiId::new(NonZeroU64::new(repo).unwrap()),
                node_id: None,
                classification: IssueClassification::Issue,
            }),
            remote_updated_at: UtcMillis(at),
            title: Change::Value(name.into()),
            body: Change::Value("downloaded discussion".into()),
            closed: Change::Value(true),
        },
        facets: ListFacets {
            assignees: assigned,
            ..Default::default()
        },
    }
}
fn record(key: &str, body: &str) -> ResourceRecord {
    ResourceRecord {
        key: key.into(),
        body: body.into(),
    }
}
#[test]
fn cached_reader_filters_search_anchors_counts_and_tab_reopen_survive_an_offline_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("profile.sqlite");
    let store = Store::open(&path).unwrap();
    let f = populate_reader(&store, 12345, Scale::Small).unwrap();
    let scope = ReaderScope::Selected;
    let list = store
        .browse_page(f.account, scope, &BrowseFilter::default(), None, 4)
        .unwrap();
    assert_eq!(list.items.len(), 4);
    assert_eq!(list.coverage, Coverage::Complete);
    let more = store
        .browse_page(f.account, scope, &BrowseFilter::default(), list.next, 4)
        .unwrap();
    assert!(more.items[0].item.0 < list.items.last().unwrap().item.0);
    let counts = store.sidebar_counts(f.account, scope).unwrap();
    assert_eq!(
        (
            counts.assigned.known,
            counts.review_requested.known,
            counts.outbox
        ),
        (4, 3, 9)
    );
    assert!(counts.assigned.complete && counts.review_requested.complete);
    let assigned = BrowseFilter {
        assignee: Some("FIXTURE-READER".into()),
        ..Default::default()
    };
    store
        .save_reader_view(f.account, "My assignments", &assigned)
        .unwrap();
    let results = store
        .browse_page(f.account, scope, &assigned, None, 10)
        .unwrap();
    assert_eq!(results.items.len(), 4);
    store
        .record_recent_item(f.account, f.reviewed_pr, 17)
        .unwrap();
    store
        .set_pinned_item(f.account, f.reviewed_pr, true)
        .unwrap();
    let pins = store
        .browse_page(
            f.account,
            scope,
            &BrowseFilter {
                pinned_only: true,
                ..Default::default()
            },
            None,
            10,
        )
        .unwrap();
    assert_eq!(pins.items[0].item, f.reviewed_pr);
    let hits = store
        .search_downloaded(f.account, scope, "Synthetic comment 2 on item 1", 10)
        .unwrap();
    assert!(hits.iter().any(|h| h.row.item == f.long_discussion
        && h.source == HitSource::Comment
        && h.anchor.as_deref() == Some("comment-000002")));
    assert!(
        store
            .search_downloaded(f.account, scope, "\" OR unrelated", 10)
            .unwrap()
            .is_empty()
    );
    let docs = store
        .discussion_page(f.account, f.long_discussion, None, 2)
        .unwrap();
    assert_eq!(docs.records.len(), 2);
    let rest = store
        .discussion_page(f.account, f.long_discussion, docs.next.as_deref(), 100)
        .unwrap();
    assert_eq!(rest.records.len(), 11);
    assert_eq!(rest.state.coverage, Coverage::Complete);
    let mut ws = store.workspace(f.account).unwrap().unwrap();
    let removed = ws.panes[0].tabs.remove(2);
    store.save_reader_layout(f.account, &ws, Some(1)).unwrap();
    assert!(store.save_reader_layout(f.account, &ws, Some(2)).is_err());
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.reader_layout(f.account).unwrap(),
        Some((ws.clone(), Some(1)))
    );
    assert_eq!(
        store.reader_views(f.account).unwrap(),
        [("My assignments".into(), assigned)]
    );
    assert_eq!(
        store.reopen_last_tab(f.account, None).unwrap().unwrap(),
        removed
    );
    assert_eq!(
        store.workspace(f.account).unwrap().unwrap().panes[0].tabs[2],
        removed
    );
    assert!(store.reopen_last_tab(f.account, None).unwrap().is_none());
    let other = store.ensure_account("github.com", 771199).unwrap();
    assert!(
        store
            .browse_page(
                other,
                ReaderScope::Repository(f.repository),
                &BrowseFilter::default(),
                None,
                10
            )
            .is_err()
    );
    assert!(
        store
            .search_downloaded(other, scope, "offline", 10)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .discussion_page(other, f.long_discussion, None, 10)
            .is_err()
    );
    assert!(
        store
            .set_pinned_item(other, f.long_discussion, true)
            .is_err()
    );
    assert!(store.cache_usage(f.account).unwrap().content_bytes > 0);
}
#[test]
fn paged_ingestion_is_atomic_and_preserves_partial_progress_on_denial_or_restart() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("page.sqlite");
    let store = Store::open(&p).unwrap();
    let account = store.ensure_account("github.com", 93).unwrap();
    let repo = store.ensure_repository(account, 1, "org", "repo").unwrap();
    let key = repository_list_key(account, repo);
    let scope = ReaderScope::Repository(repo);
    let walk = store.start_resource_fetch(key, WalkMode::Full).unwrap();
    let tail = store
        .commit_repository_list_page(
            account,
            repo,
            &walk,
            RepositoryListPage {
                summaries: vec![page(
                    41,
                    "Ancient closed issue",
                    1,
                    Some(vec!["Alice".into()]),
                )],
                next_cursor: Some("p2"),
                completed_validators: None,
                fetched_at_ms: 100,
            },
        )
        .unwrap()
        .unwrap();
    let fail = store.commit_repository_list_page(
        account,
        repo,
        &tail,
        RepositoryListPage {
            summaries: vec![
                page(42, "not committed", 2, None),
                page(43, "invalid facet", 2, Some(vec!["".into()])),
            ],
            next_cursor: None,
            completed_validators: None,
            fetched_at_ms: 101,
        },
    );
    assert!(fail.is_err());
    store
        .record_resource_error(key, &tail, ResourceError::Forbidden)
        .unwrap();
    let cached = store
        .browse_page(
            account,
            scope,
            &BrowseFilter {
                state: StateFilter::Closed,
                ..Default::default()
            },
            None,
            5,
        )
        .unwrap();
    assert_eq!(cached.items.len(), 1);
    assert_eq!(cached.coverage, Coverage::Partial);
    drop(store);
    let store = Store::open(&p).unwrap();
    let progress = store.resume_resource_fetch(key).unwrap().unwrap();
    assert_eq!(progress.cursor.as_deref(), Some("p2"));
    store
        .commit_repository_list_page(
            account,
            repo,
            &progress,
            RepositoryListPage {
                summaries: vec![
                    page(41, "older stale edit", 0, None),
                    page(42, "New page", 3, Some(vec![])),
                ],
                next_cursor: None,
                completed_validators: None,
                fetched_at_ms: 102,
            },
        )
        .unwrap();
    let final_page = store
        .browse_page(account, scope, &BrowseFilter::default(), None, 10)
        .unwrap();
    assert_eq!(final_page.coverage, Coverage::Complete);
    assert_eq!(final_page.items.len(), 2);
    let filtered = store
        .browse_page(
            account,
            scope,
            &BrowseFilter {
                assignee: Some("alice".into()),
                ..Default::default()
            },
            None,
            10,
        )
        .unwrap();
    assert_eq!(
        filtered.items[0].title.as_deref(),
        Some("Ancient closed issue")
    );
    assert!(
        store
            .browse_page(
                account,
                ReaderScope::Selected,
                &BrowseFilter::default(),
                None,
                10
            )
            .unwrap()
            .items
            .is_empty()
    );
}
#[test]
fn comments_reindex_on_retry_update_and_cleanup_and_the_offline_pin_is_cancellable() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_profile(dir.path()).unwrap();
    let f = populate_reader(&store, 7, Scale::Small).unwrap();
    let cached = store
        .request_issue_download(f.account, f.long_discussion, 0)
        .unwrap();
    assert_eq!(cached.state, DownloadState::Complete);
    store
        .cancel_issue_download(f.account, f.long_discussion, cached.generation)
        .unwrap();
    store.clear_cache(f.account).unwrap();
    let key = |family| ResourceKey {
        account: f.account,
        target: ResourceTarget::Item(f.long_discussion),
        family,
    };
    let stale = store
        .request_issue_download(f.account, f.long_discussion, 1)
        .unwrap();
    assert_eq!(stale.state, DownloadState::Pending);
    assert!(
        store
            .start_issue_download(f.account, f.long_discussion, stale.generation)
            .is_err()
    );
    assert!(
        store
            .cancel_issue_download(f.account, f.long_discussion, stale.generation)
            .unwrap()
    );
    assert!(
        store
            .start_issue_download(f.account, f.long_discussion, stale.generation)
            .is_err()
    );
    assert!(
        store
            .request_issue_download(f.account, f.reviewed_pr, 1)
            .is_err()
    );
    let job = store
        .request_issue_download(f.account, f.long_discussion, 3)
        .unwrap();
    for family in [
        ResourceFamily::Metadata,
        ResourceFamily::Body,
        ResourceFamily::Comments,
    ] {
        let walk = store
            .start_resource_fetch(key(family), WalkMode::Full)
            .unwrap();
        store
            .commit_resource_page(
                key(family),
                &walk,
                ResourcePage {
                    records: &[record("one", "a long distinct marker")],
                    next_cursor: None,
                    completed_validators: None,
                    fetched_at_ms: 5,
                },
            )
            .unwrap();
    }
    store
        .start_issue_download(f.account, f.long_discussion, job.generation)
        .unwrap();
    store
        .finish_issue_download(f.account, f.long_discussion, job.generation)
        .unwrap();
    let scope = ReaderScope::Selected;
    assert_eq!(
        store
            .search_downloaded(f.account, scope, "distinct marker", 10)
            .unwrap()
            .len(),
        1
    );
    let walk = store
        .start_resource_fetch(key(ResourceFamily::Comments), WalkMode::Incremental)
        .unwrap();
    store
        .commit_resource_page(
            key(ResourceFamily::Comments),
            &walk,
            ResourcePage {
                records: &[record("one", "updated marker")],
                next_cursor: None,
                completed_validators: None,
                fetched_at_ms: 8,
            },
        )
        .unwrap();
    assert!(
        store
            .search_downloaded(f.account, scope, "distinct marker", 10)
            .unwrap()
            .is_empty()
    );
    store.clear_cache(f.account).unwrap();
    assert_eq!(
        store
            .search_downloaded(f.account, scope, "updated marker", 10)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(store.cache_usage(f.account).unwrap().offline_pins, 1);
    store
        .cancel_issue_download(f.account, f.long_discussion, job.generation)
        .unwrap();
    store.clear_cache(f.account).unwrap();
    assert!(
        store
            .search_downloaded(f.account, scope, "updated marker", 10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.queue_page(f.account, 0, 100).unwrap().len(), 10);
}
#[test]
fn older_unenrolled_storage_fixture_can_open_explicit_repository_without_selecting_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_profile(dir.path()).unwrap();
    let f = populate(&store, 9, Scale::Small).unwrap();
    assert!(
        store
            .browse_page(
                f.account,
                ReaderScope::Selected,
                &BrowseFilter::default(),
                None,
                10
            )
            .unwrap()
            .items
            .is_empty()
    );
    let cached = store
        .browse_page(
            f.account,
            ReaderScope::Repository(f.repository),
            &BrowseFilter::default(),
            None,
            40,
        )
        .unwrap();
    assert_eq!(cached.items.len(), 30);
    assert_eq!(cached.coverage, Coverage::Missing);
}
#[test]
fn typed_discussion_and_timeline_preserve_metadata_partial_and_anchors() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_profile(dir.path()).unwrap();
    let f = populate_reader(&store, 12345, Scale::Small).unwrap();
    let key = |family| ResourceKey {
        account: f.account,
        target: ResourceTarget::Item(f.long_discussion),
        family,
    };
    let typed = |id: u64, body: &str, created, updated| DiscussionComment {
        provider_id: id,
        node_id: Some(format!("node-{id}")),
        author: Some("reader".into()),
        created_at_ms: created,
        updated_at_ms: updated,
        body: body.into(),
    };
    let walk = store
        .start_resource_fetch(key(ResourceFamily::Comments), WalkMode::Full)
        .unwrap();
    let next = store
        .commit_discussion_page(
            f.account,
            f.long_discussion,
            &walk,
            DiscussionInputPage {
                comments: vec![typed(90, "fresh token", 2, 10)],
                next_cursor: Some("2"),
                completed_validators: None,
                fetched_at_ms: 20,
            },
        )
        .unwrap()
        .unwrap();
    // Existing complete data remains visible while the replacement scan has only one page.
    assert_eq!(
        store
            .discussion_page(f.account, f.long_discussion, None, 30)
            .unwrap()
            .records
            .len(),
        13
    );
    store
        .commit_discussion_page(
            f.account,
            f.long_discussion,
            &next,
            DiscussionInputPage {
                comments: vec![typed(2, "chronological first", 1, 9)],
                next_cursor: None,
                completed_validators: None,
                fetched_at_ms: 21,
            },
        )
        .unwrap();
    let rows = store
        .discussion_page(f.account, f.long_discussion, None, 1)
        .unwrap();
    assert_eq!(rows.records[0].body, "chronological first");
    assert_eq!(rows.metadata[0].created_at_ms, 1);
    let hits = store
        .search_downloaded(f.account, ReaderScope::Selected, "fresh token", 5)
        .unwrap();
    let anchor = hits[0].anchor.as_deref().unwrap();
    let matched = store
        .discussion_page_at(f.account, f.long_discussion, anchor, 1)
        .unwrap();
    assert_eq!(matched.metadata[0].node_id.as_deref(), Some("node-90"));
    assert!(matches!(
        store.discussion_page_at(f.account, f.long_discussion, "removed", 8),
        Err(Error::NotFound)
    ));
    let walk = store
        .start_resource_fetch(key(ResourceFamily::Comments), WalkMode::Incremental)
        .unwrap();
    store
        .commit_discussion_page(
            f.account,
            f.long_discussion,
            &walk,
            DiscussionInputPage {
                comments: vec![typed(90, "stale token", 2, 3)],
                next_cursor: None,
                completed_validators: None,
                fetched_at_ms: 22,
            },
        )
        .unwrap();
    assert_eq!(
        store
            .discussion_page_at(f.account, f.long_discussion, anchor, 5)
            .unwrap()
            .records[0]
            .body,
        "fresh token"
    );
    let walk = store
        .start_resource_fetch(key(ResourceFamily::Timeline), WalkMode::Full)
        .unwrap();
    let event = |key: &str, time| TimelineEntry {
        remote_key: key.into(),
        actor: None,
        event: "referenced".into(),
        occurred_at_ms: time,
    };
    store
        .commit_timeline_page(
            f.account,
            f.long_discussion,
            &walk,
            TimelineInputPage {
                events: vec![event("undated", None), event("first", Some(1))],
                next_cursor: None,
                completed_validators: None,
                fetched_at_ms: 25,
            },
        )
        .unwrap();
    let first = store
        .timeline_page(f.account, f.long_discussion, None, 1)
        .unwrap();
    assert_eq!(first.entries[0].remote_key, "first");
    let next = store
        .timeline_page(f.account, f.long_discussion, first.next.as_deref(), 1)
        .unwrap();
    assert_eq!(next.entries[0].occurred_at_ms, None);
    let pr = threadline_core::domain::RepositoryName::new("fixture", "relay").unwrap();
    let aliased = threadline_core::domain::RepositoryName::new("fixture", "archive").unwrap();
    assert_eq!(
        store
            .resolve_downloaded_route(f.account, &pr, 2, Some(ItemKind::Issue))
            .unwrap(),
        store
            .resolve_downloaded_route(f.account, &aliased, 2, Some(ItemKind::Issue))
            .unwrap()
    );
    assert!(
        store
            .resolve_downloaded_route(f.account, &pr, 2, Some(ItemKind::PullRequest))
            .unwrap()
            .is_none()
    );
}
#[test]
fn it_003_shared_comment_index_deduplicates_items_and_keeps_unequal_ids_with_tied_dates_in_order() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("reader.sqlite");
    let store = Store::open(&path).unwrap();
    let fixture = populate_reader(&store, 8, Scale::Small).unwrap();
    let key = ResourceKey {
        account: fixture.account,
        target: ResourceTarget::Item(fixture.reviewed_pr),
        family: ResourceFamily::Comments,
    };
    let walk = store.start_resource_fetch(key, WalkMode::Full).unwrap();
    let comment = |id| DiscussionComment {
        provider_id: id,
        node_id: None,
        author: None,
        created_at_ms: 1,
        updated_at_ms: 1,
        body: format!("match ordinary {id}"),
    };
    store
        .commit_discussion_page(
            fixture.account,
            fixture.reviewed_pr,
            &walk,
            DiscussionInputPage {
                comments: vec![comment(100), comment(9), comment(11)],
                next_cursor: None,
                completed_validators: None,
                fetched_at_ms: 2,
            },
        )
        .unwrap();
    let reviews = ResourceKey {
        family: ResourceFamily::Reviews,
        ..key
    };
    let walk = store.start_resource_fetch(reviews, WalkMode::Full).unwrap();
    store
        .commit_resource_page(
            reviews,
            &walk,
            ResourcePage {
                records: &[record("review-9", "match formal")],
                next_cursor: None,
                completed_validators: None,
                fetched_at_ms: 3,
            },
        )
        .unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    let page = store
        .discussion_page(fixture.account, fixture.reviewed_pr, None, 2)
        .unwrap();
    assert_eq!(
        page.records
            .iter()
            .map(|r| r.body.as_str())
            .collect::<Vec<_>>(),
        ["match ordinary 9", "match ordinary 11"]
    );
    assert_eq!(
        store
            .discussion_page(
                fixture.account,
                fixture.reviewed_pr,
                page.next.as_deref(),
                2
            )
            .unwrap()
            .records[0]
            .body,
        "match ordinary 100"
    );
    let matches = store
        .search_downloaded(fixture.account, ReaderScope::Selected, "match", 10)
        .unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].row.item, fixture.reviewed_pr);
    let review = store
        .search_downloaded(fixture.account, ReaderScope::Selected, "formal", 10)
        .unwrap();
    assert_eq!(
        (review[0].source, review[0].anchor.as_deref()),
        (HitSource::Review, Some("review-9"))
    );
}

#[test]
fn direct_metadata_and_terminal_partial_are_independent_from_body_and_prior_list_validation() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_profile(dir.path()).unwrap();
    let a = store.ensure_account("github.com", 3001).unwrap();
    let r = store.ensure_repository(a, 1, "test", "main").unwrap();
    let mut incoming = page(70, "metadata only", 10, None);
    incoming.update.body = Change::Missing;
    let v = Validators {
        etag: Some("\"m10\"".into()),
        ..Default::default()
    };
    let item = store.commit_item_metadata(a, r, incoming, &v, 101).unwrap();
    let key = |family| ResourceKey {
        account: a,
        target: ResourceTarget::Item(item),
        family,
    };
    assert_eq!(
        store
            .resource_state(key(ResourceFamily::Metadata))
            .unwrap()
            .coverage,
        Coverage::Complete
    );
    assert_eq!(
        store
            .resource_state(key(ResourceFamily::Body))
            .unwrap()
            .coverage,
        Coverage::Missing
    );
    let mut empty = page(70, "unchanged", 11, None);
    empty.update.body = Change::Null;
    store.commit_item_metadata(a, r, empty, &v, 102).unwrap();
    assert_eq!(
        store
            .resource_state(key(ResourceFamily::Body))
            .unwrap()
            .coverage,
        Coverage::Complete
    );
    assert_eq!(store.item(a, item).unwrap().unwrap().body, Change::Null);
    let list = repository_list_key(a, r);
    let walk = store.start_resource_fetch(list, WalkMode::Full).unwrap();
    store
        .commit_repository_list_page(
            a,
            r,
            &walk,
            RepositoryListPage {
                summaries: vec![page(71, "original", 8, None)],
                next_cursor: None,
                completed_validators: Some(&v),
                fetched_at_ms: 90,
            },
        )
        .unwrap();
    let walk = store.start_resource_fetch(list, WalkMode::Full).unwrap();
    store
        .commit_repository_list_partial(
            a,
            r,
            &walk,
            RepositoryListPage {
                summaries: vec![page(72, "still accessible", 9, None)],
                next_cursor: None,
                completed_validators: None,
                fetched_at_ms: 104,
            },
            ResourceError::Forbidden,
        )
        .unwrap();
    let state = store.resource_state(list).unwrap();
    assert_eq!(
        (state.coverage, state.fetch, state.last_validation_at_ms),
        (Coverage::Partial, FetchState::Unavailable, Some(90))
    );
    assert_eq!(state.validators, v);
    assert!(
        store
            .resolve(
                a,
                ItemLocator {
                    repository: r,
                    number: 71
                }
            )
            .unwrap()
            .is_some()
    );
    assert!(
        store
            .resolve(
                a,
                ItemLocator {
                    repository: r,
                    number: 72
                }
            )
            .unwrap()
            .is_some()
    );
}
#[test]
fn section_16_reader_fixture_selects_two_repositories_and_exposes_cached_counts_and_search() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.sqlite");
    let store = Store::open(&path).unwrap();
    let fixture = populate_reader(&store, 12345, Scale::Section16).unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    let counts = store
        .sidebar_counts(fixture.account, ReaderScope::Selected)
        .unwrap();
    assert_eq!(
        (
            counts.assigned.known,
            counts.review_requested.known,
            counts.outbox
        ),
        (7142, 5555, 900)
    );
    assert!(counts.assigned.complete && counts.review_requested.complete);
    let usage = store.cache_usage(fixture.account).unwrap();
    assert_eq!(
        (usage.downloaded_summaries, usage.discussion_records),
        (50_000, 250_000)
    );
    let selected = store.github_repository_views(fixture.account).unwrap();
    assert_eq!(selected.iter().filter(|r| r.selected).count(), 2);
    assert_eq!(
        store.active_github_account().unwrap(),
        Some(fixture.account)
    );
    let archive = selected.iter().find(|r| r.name == "archive").unwrap();
    let zero = store
        .sidebar_counts(fixture.account, ReaderScope::Repository(archive.id))
        .unwrap();
    assert_eq!((zero.assigned.known, zero.review_requested.known), (0, 0));
    assert!(zero.assigned.complete && zero.review_requested.complete);
    assert_eq!(
        store
            .timeline_page(fixture.account, fixture.long_discussion, None, 5)
            .unwrap()
            .entries
            .len(),
        1
    );
    let hits = store
        .search_downloaded(
            fixture.account,
            ReaderScope::Selected,
            "Synthetic comment 4999 on item 1",
            4,
        )
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].anchor.is_some());
    let ws = store.workspace(fixture.account).unwrap().unwrap();
    assert_eq!(
        (
            ws.panes.len(),
            ws.panes.iter().map(|p| p.tabs.len()).sum::<usize>()
        ),
        (2, 20)
    );
}

#[test]
fn old_schema_migration_indexes_existing_comments_without_enrolling_a_credential() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prior.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    for schema in [
        include_str!("../src/identity.sql"),
        include_str!("../src/resources.sql"),
        include_str!("../src/queue.sql"),
        include_str!("../src/search.sql"),
    ] {
        db.execute_batch(schema).unwrap();
    }
    db.pragma_update(None, "application_id", 0x5448_5244)
        .unwrap();
    db.pragma_update(None, "user_version", 2).unwrap();
    db.execute_batch("INSERT INTO accounts VALUES(1,'fixtures.invalid','7');
    INSERT INTO repositories VALUES(11,1,'1','fixture','old');
    INSERT INTO items(id,repository_id,account_id,number,kind,title,title_at) VALUES(30,11,1,50,'issue','preserved old issue',1);
    INSERT INTO cached_resources(id,account_id,item_id,family,coverage) VALUES(60,1,30,'comments',2);
    INSERT INTO cached_resource_records VALUES(60,'comment-original','legacy archived phrase');").unwrap();
    drop(db);
    let store = Store::open(&path).unwrap();
    assert!(store.migration_backup().is_some());
    assert!(store.active_github_account().unwrap().is_none());
    let hits = store
        .search_downloaded(
            AccountId(1),
            ReaderScope::Repository(RepositoryId(11)),
            "archived phrase",
            3,
        )
        .unwrap();
    assert_eq!(hits[0].anchor.as_deref(), Some("comment-original"));
    assert_eq!(
        store
            .discussion_page_at(AccountId(1), ItemId(30), "comment-original", 5)
            .unwrap()
            .records[0]
            .body,
        "legacy archived phrase"
    );
    assert!(
        store
            .search_downloaded(AccountId(1), ReaderScope::Selected, "archived phrase", 3)
            .unwrap()
            .is_empty()
    );
}
#[test]
fn stable_image_blobs_are_scoped_bounded_and_clearable() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_profile(dir.path()).unwrap();
    let f = populate_reader(&store, 8, Scale::Small).unwrap();
    let url = "https://github.com/user-attachments/assets/synthetic-id";
    store
        .cache_image(f.account, f.repository, url, "image/png", &[1, 2, 3], 7)
        .unwrap();
    store
        .cache_image(f.account, f.repository, url, "image/png", &[4], 3)
        .unwrap();
    assert_eq!(
        store
            .cached_image(f.account, f.repository, url)
            .unwrap()
            .unwrap()
            .bytes,
        [1, 2, 3]
    );
    assert!(
        store
            .cached_image(AccountId(9999), f.repository, url)
            .is_err()
    );
    assert!(
        store
            .cache_image(
                f.account,
                f.repository,
                &format!("{url}?token=fixture"),
                "image/png",
                &[3],
                8
            )
            .is_err()
    );
    assert!(
        store
            .cache_image(f.account, f.repository, url, "image/svg+xml", &[3], 8)
            .is_err()
    );
    assert!(
        store
            .cache_image(
                f.account,
                f.repository,
                url,
                "image/png",
                &vec![0; 8 * 1024 * 1024 + 1],
                8
            )
            .is_err()
    );
    store.clear_cache(f.account).unwrap();
    assert!(
        store
            .cached_image(f.account, f.repository, url)
            .unwrap()
            .is_none()
    );
}
