use std::num::NonZeroU64;
use threadline_storage::{access::*, reader::*, resources::*, *};

#[test]
fn issue_pin_schema_17_active_job_keeps_its_identity_and_partial_page_on_upgrade() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open_profile(directory.path()).unwrap();
    let fixture = fixtures::populate_reader(&store, 12345, fixtures::Scale::Small).unwrap();
    let account = fixture.account;
    let item = fixture.long_discussion;
    let job = store.request_issue_download(account, item, 10).unwrap();
    assert_eq!(job.state, DownloadState::Complete);
    store
        .cancel_issue_download(account, item, job.generation)
        .unwrap();
    store.clear_cache(account).unwrap();
    let pending = store.request_issue_download(account, item, 11).unwrap();
    let key = ResourceKey {
        account,
        target: ResourceTarget::Item(item),
        family: ResourceFamily::Comments,
    };
    let progress = store.start_resource_fetch(key, WalkMode::Full).unwrap();
    store
        .commit_discussion_page(
            account,
            item,
            &progress,
            DiscussionInputPage {
                comments: vec![DiscussionComment {
                    provider_id: 90,
                    node_id: None,
                    author: None,
                    created_at_ms: 1,
                    updated_at_ms: 1,
                    body: "Saved before upgrade".into(),
                }],
                next_cursor: Some("2"),
                completed_validators: None,
                fetched_at_ms: 12,
            },
        )
        .unwrap();
    drop(store);
    let db = rusqlite::Connection::open(directory.path().join(DATABASE_FILENAME)).unwrap();
    db.execute_batch(include_str!("support/schema_18.sql"))
        .unwrap();
    db.execute_batch(
        "BEGIN;
        UPDATE issue_downloads SET state='active';
        ALTER TABLE issue_downloads DROP COLUMN retry_at;
        ALTER TABLE issue_downloads DROP COLUMN lease_until;
        ALTER TABLE issue_downloads DROP COLUMN lease_token;
        ALTER TABLE issue_downloads DROP COLUMN last_error;
        PRAGMA user_version=17;
        COMMIT;",
    )
    .unwrap();
    drop(db);

    let migrated = Store::open_profile(directory.path()).unwrap();
    let restored = migrated.issue_download(account, item).unwrap().unwrap();
    assert_eq!(restored.state, DownloadState::Active);
    assert_eq!(restored.generation, pending.generation);
    assert_eq!(restored.requested_at_ms, pending.requested_at_ms);
    assert_eq!(restored.comments.coverage, Coverage::Partial);
    assert!(restored.last_error.is_none());
    let page = migrated.discussion_page(account, item, None, 10).unwrap();
    assert_eq!(page.records[0].body, "Saved before upgrade");
    let checkpoint = migrated.resume_resource_fetch(key).unwrap().unwrap();
    assert_eq!(checkpoint.cursor.as_deref(), Some("2"));
}

#[test]
fn issue_pin_durably_fences_two_workers_and_cancelled_continuations() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open_profile(directory.path()).unwrap();
    let user = NonZeroU64::new(80).unwrap();
    let account = store.ensure_account("github.com", user.get()).unwrap();
    let profile = store
        .reserve_access_profile_credential(
            account,
            user,
            "test-user",
            "work",
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
    let repository = store
        .observe_github_repository(
            &profile,
            &RepositoryObservation {
                provider_id: NonZeroU64::new(200).unwrap(),
                owner: "work",
                name: "main",
                private: true,
                source: RepositoryObservationSource::Direct,
            },
            2,
        )
        .unwrap();
    store
        .select_github_repository(account, repository, true)
        .unwrap();
    let profile = store
        .github_access_profile(account, "work")
        .unwrap()
        .unwrap();
    let item = store
        .commit_item_metadata(
            account,
            repository,
            ListSummary {
                number: 41,
                update: ItemUpdate {
                    representation: ItemRepresentation::Issue(IssueRecord {
                        id: 100.try_into().unwrap(),
                        node_id: None,
                        classification: IssueClassification::Issue,
                    }),
                    remote_updated_at: UtcMillis(3),
                    title: Change::Value("Original issue".into()),
                    body: Change::Null,
                    closed: Change::Value(false),
                },
                facets: ListFacets::default(),
            },
            &Validators::default(),
            3,
        )
        .unwrap();
    let page = |id, next_cursor, at| DiscussionInputPage {
        comments: vec![DiscussionComment {
            provider_id: id,
            node_id: None,
            author: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            body: format!("Comment {id}"),
        }],
        next_cursor,
        completed_validators: None,
        fetched_at_ms: at,
    };
    let job = store.request_issue_download(account, item, 5).unwrap();
    let first = store.claim_issue_download(account, 6).unwrap().unwrap();
    let reopened = Store::open_profile(directory.path()).unwrap();
    assert!(reopened.claim_issue_download(account, 6).unwrap().is_none());
    let progress = store
        .issue_download_discussion_progress(&first, &profile, 7)
        .unwrap()
        .unwrap();
    assert!(progress.cursor.is_none());
    let progress = store
        .commit_issue_download_discussion(&first, &profile, &progress, page(1, Some("2"), 8), None)
        .unwrap()
        .unwrap();
    store
        .record_issue_download_discussion_error(
            &first,
            &profile,
            &progress,
            ResourceError::Transport,
            9,
        )
        .unwrap();
    store
        .defer_issue_download(&first, Some(30), Some(ResourceError::Transport))
        .unwrap();

    let partial = reopened.issue_download(account, item).unwrap().unwrap();
    assert_eq!(partial.generation, job.generation);
    assert_eq!(partial.comments.coverage, Coverage::Partial);
    assert_eq!(partial.comments.fetch, FetchState::Failed);
    assert_eq!(partial.last_error, Some(ResourceError::Transport));
    assert_eq!(reopened.next_issue_download_due(account).unwrap(), Some(30));
    assert!(
        reopened
            .claim_issue_download(account, 29)
            .unwrap()
            .is_none()
    );
    let abandoned = reopened.claim_issue_download(account, 30).unwrap().unwrap();
    let pending = reopened
        .issue_download_discussion_progress(&abandoned, &profile, 31)
        .unwrap()
        .unwrap();
    assert_eq!(pending.cursor.as_deref(), Some("2"));
    let lease_expires = reopened.next_issue_download_due(account).unwrap().unwrap();
    assert!(
        store
            .claim_issue_download(account, lease_expires - 1)
            .unwrap()
            .is_none()
    );
    let recovered = store
        .claim_issue_download(account, lease_expires)
        .unwrap()
        .unwrap();
    assert_eq!(recovered.generation, abandoned.generation);
    assert!(
        reopened
            .commit_issue_download_discussion(
                &abandoned,
                &profile,
                &pending,
                page(99, None, lease_expires + 1),
                None,
            )
            .is_err()
    );
    let pending = store
        .issue_download_discussion_progress(&recovered, &profile, lease_expires + 2)
        .unwrap()
        .unwrap();
    assert_eq!(pending.cursor.as_deref(), Some("2"));
    assert!(
        reopened
            .cancel_issue_download(account, item, job.generation)
            .unwrap()
    );
    assert!(
        store
            .commit_issue_download_discussion(
                &recovered,
                &profile,
                &pending,
                page(99, None, lease_expires + 3),
                None,
            )
            .is_err()
    );
    store
        .defer_issue_download(
            &recovered,
            Some(lease_expires + 4),
            Some(ResourceError::Transport),
        )
        .unwrap();
    assert!(reopened.next_issue_download_due(account).unwrap().is_none());
    let cancelled = reopened.issue_download(account, item).unwrap().unwrap();
    assert_eq!(cancelled.state, DownloadState::Cancelled);
    assert_eq!(cancelled.comments.coverage, Coverage::Partial);
    let page_after_cancel = reopened.discussion_page(account, item, None, 10).unwrap();
    assert_eq!(page_after_cancel.records.len(), 1);
    assert_eq!(page_after_cancel.records[0].body, "Comment 1");

    let retried = reopened
        .request_issue_download(account, item, lease_expires + 4)
        .unwrap();
    assert!(retried.generation > recovered.generation);
    let replacement = reopened
        .claim_issue_download(account, lease_expires + 5)
        .unwrap()
        .unwrap();
    let pending = reopened
        .issue_download_discussion_progress(&replacement, &profile, lease_expires + 6)
        .unwrap()
        .unwrap();
    assert_eq!(pending.cursor.as_deref(), Some("2"));
    reopened
        .commit_issue_download_discussion(
            &replacement,
            &profile,
            &pending,
            page(2, None, lease_expires + 7),
            None,
        )
        .unwrap();
    reopened
        .complete_issue_download_step(&replacement, &profile, lease_expires + 8)
        .unwrap();
    let completed = store.issue_download(account, item).unwrap().unwrap();
    assert_eq!(completed.state, DownloadState::Complete);
    assert_eq!(completed.completed_families(), 3);
    let page = store.discussion_page(account, item, None, 10).unwrap();
    assert_eq!(
        page.records
            .iter()
            .map(|row| row.body.as_str())
            .collect::<Vec<_>>(),
        ["Comment 1", "Comment 2"]
    );
}
