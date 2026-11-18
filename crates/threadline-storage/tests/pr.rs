use std::num::NonZeroU64;
use threadline_storage::{
    access::*,
    pr::*,
    reader::{HitSource, ReaderScope},
    resources::*,
    *,
};
fn id(n: u64) -> NonZeroU64 {
    NonZeroU64::new(n).unwrap()
}
fn oid(byte: char) -> CommitOid {
    CommitOid::parse(&byte.to_string().repeat(40)).unwrap()
}
fn setup(store: &Store) -> (AccountId, RepositoryId, ItemId) {
    let account = store.ensure_account("github.com", 80).unwrap();
    let profile = store
        .reserve_access_profile_credential(
            account,
            id(80),
            "user",
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
    let repo = store
        .observe_github_repository(
            &profile,
            &RepositoryObservation {
                provider_id: id(200),
                owner: "work",
                name: "main",
                private: true,
                source: RepositoryObservationSource::Direct,
            },
            2,
        )
        .unwrap();
    store.select_github_repository(account, repo, true).unwrap();
    let pr = ingest(
        store,
        account,
        repo,
        70,
        true,
        "Fixes #4. Resolves Work/Main#9. Closed other/x#7.",
        20,
    );
    (account, repo, pr)
}
fn ingest(
    s: &Store,
    a: AccountId,
    r: RepositoryId,
    n: u64,
    pr: bool,
    body: &str,
    at: i64,
) -> ItemId {
    s.ingest(
        a,
        ItemLocator {
            repository: r,
            number: n,
        },
        ItemUpdate {
            representation: ItemRepresentation::Issue(IssueRecord {
                id: IssueApiId::new(id(1000 + n)),
                node_id: None,
                classification: if pr {
                    IssueClassification::PullRequest
                } else {
                    IssueClassification::Issue
                },
            }),
            remote_updated_at: UtcMillis(at),
            title: Change::Value(format!("item {n}")),
            body: Change::Value(body.into()),
            closed: Change::Value(false),
        },
    )
    .unwrap()
}
fn input(
    records: Vec<PrRecord>,
    cursor: Option<&str>,
    reason: Option<ResourceError>,
) -> PrInputPage<'_> {
    PrInputPage {
        records,
        next_cursor: cursor,
        partial: reason,
        validators: None,
        fetched_at_ms: 100,
    }
}
fn file() -> PrRecord {
    PrRecord::File {
        path: "src/new.rs".into(),
        previous_path: Some("src/old.rs".into()),
        status: "renamed".into(),
        additions: Some(1),
        deletions: Some(1),
        patch: PatchEvidence::Text("@@ -8,2 +12,2 @@\n same\n-old\n+new".into()),
    }
}
#[test]
fn it_016_independent_pr_families_source_anchors_search_partial_and_stale_heads_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pr.sqlite3");
    let s = Store::open(&path).unwrap();
    let (a, _, pr) = setup(&s);
    let rev = s
        .record_pr_revision(
            a,
            pr,
            &oid('a'),
            &oid('b'),
            Some(&oid('c')),
            &PrHeader {
                draft: Some(false),
                ..Default::default()
            },
            100,
        )
        .unwrap();
    assert_eq!(rev.comparison(false, 3).unwrap().base, oid('c'));
    let files = s
        .start_pr_fetch(&rev, PrFamily::Files, WalkMode::Full)
        .unwrap();
    let next = s
        .commit_pr_page(
            &rev,
            PrFamily::Files,
            &files,
            input(vec![file()], Some("2"), None),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        s.pr_page(&rev, PrFamily::Files, None, 5)
            .unwrap()
            .state
            .coverage,
        Coverage::Partial
    );
    let review = s
        .start_pr_fetch(&rev, PrFamily::Reviews, WalkMode::Full)
        .unwrap();
    s.commit_pr_page(
        &rev,
        PrFamily::Reviews,
        &review,
        input(
            vec![PrRecord::Review {
                id: 8,
                node_id: None,
                author: Some("reviewer".into()),
                actor: Default::default(),
                body: "unique review observation".into(),
                state: "APPROVED".into(),
                submitted_at_ms: Some(90),
                commit_oid: Some(oid('b').as_str().into()),
            }],
            None,
            None,
        ),
    )
    .unwrap();
    assert!(
        s.commit_pr_page(&rev, PrFamily::Reviews, &next, input(vec![], None, None))
            .is_err()
    );
    drop(s);
    let s = Store::open(&path).unwrap();
    let resumed = s.resume_pr_fetch(&rev, PrFamily::Files).unwrap().unwrap();
    assert_eq!(resumed.cursor.as_deref(), Some("2"));
    assert!(
        s.commit_pr_page(&rev, PrFamily::Files, &next, input(vec![], None, None))
            .is_err()
    );
    s.commit_pr_page(
        &rev,
        PrFamily::Files,
        &resumed,
        input(vec![], None, Some(ResourceError::InvalidResponse)),
    )
    .unwrap();
    let left = s
        .pr_source_line(&rev, "src/new.rs", 3, SourceSide::Left)
        .unwrap()
        .unwrap();
    assert_eq!(
        (left.oid, left.path.as_str(), left.line),
        (oid('c'), "src/old.rs", 9)
    );
    let right = s
        .pr_source_line(&rev, "src/new.rs", 4, SourceSide::Right)
        .unwrap()
        .unwrap();
    assert_eq!(
        (right.oid, right.path.as_str(), right.line),
        (oid('b'), "src/new.rs", 13)
    );
    assert!(
        s.pr_source_line(&rev, "src/new.rs", 3, SourceSide::Right)
            .unwrap()
            .is_none()
    );
    let hits = s
        .search_downloaded(a, ReaderScope::Selected, "unique review observation", 5)
        .unwrap();
    assert_eq!(
        (hits[0].source, hits[0].anchor.as_deref()),
        (HitSource::Review, Some("review:8"))
    );
    s.set_pr_file_viewed(&rev, "src/new.rs", true).unwrap();
    let newer = s
        .record_pr_revision(a, pr, &oid('a'), &oid('d'), None, &PrHeader::default(), 110)
        .unwrap();
    assert!(newer.comparison(false, 3).is_none());
    assert!(s.pr_viewed_files(&newer).unwrap().is_empty());
    let check = s
        .start_pr_fetch(&newer, PrFamily::Checks, WalkMode::Full)
        .unwrap();
    let head = PrRecord::Check {
        id: 20,
        head_oid: oid('b').as_str().into(),
        name: "CI".into(),
        status: "completed".into(),
        conclusion: Some("success".into()),
        completed_at_ms: Some(102),
    };
    assert!(
        s.commit_pr_page(
            &newer,
            PrFamily::Checks,
            &check,
            input(vec![head], None, None)
        )
        .is_err()
    );
    s.commit_pr_page(
        &newer,
        PrFamily::Checks,
        &check,
        input(vec![], None, Some(ResourceError::Forbidden)),
    )
    .unwrap();
    assert_eq!(
        s.pr_page(&newer, PrFamily::Checks, None, 5)
            .unwrap()
            .state
            .error,
        Some(ResourceError::Forbidden)
    );
    assert_eq!(
        s.pr_page(&rev, PrFamily::Reviews, None, 5)
            .unwrap()
            .state
            .coverage,
        Coverage::Complete
    );
    assert_eq!(s.current_pr_revision(a, pr).unwrap().unwrap().0, newer);
    assert!(
        s.record_pr_revision(
            a,
            pr,
            &oid('a'),
            &oid('b'),
            Some(&oid('f')),
            &PrHeader::default(),
            80
        )
        .is_err()
    );
}
#[test]
fn it_017_actual_body_references_verify_original_identity_and_reject_delayed_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (a, r, pr) = setup(&s);
    let issue = ingest(&s, a, r, 4, false, "issue", 14);
    let wrong = ingest(&s, a, r, 9, true, "pull", 19);
    let refs = s.index_pr_body(a, pr, None, "body20", 20).unwrap();
    assert_eq!(refs.iter().map(|r| r.number).collect::<Vec<_>>(), [4, 9, 7]);
    assert!(refs.iter().all(|r| r.verification == Verification::Pending));
    let proof = |item, n: u64| ReferenceObservation::Item {
        repository: r,
        item,
        issue_id: IssueApiId::new(id(n + 1000)),
    };
    assert!(
        s.record_pr_reference(a, pr, "body20", 0, proof(wrong, 9), 25)
            .is_err()
    );
    assert_eq!(
        s.record_pr_reference(a, pr, "body20", 0, proof(issue, 4), 25)
            .unwrap(),
        Verification::Verified
    );
    assert_eq!(
        s.record_pr_reference(a, pr, "body20", 1, proof(wrong, 9), 25)
            .unwrap(),
        Verification::PullRequest
    );
    use threadline_storage::queue::{Pane, PrContext, Tab, TabView, Workspace};
    let tab = |id: &str| Tab {
        id: id.into(),
        item: pr,
        view: TabView::default(),
    };
    s.save_workspace(
        a,
        &Workspace {
            focused_pane: 0,
            proportions: vec![1.0],
            panes: vec![Pane {
                active: Some(0),
                tabs: vec![tab("first-pr"), tab("second-pr")],
            }],
        },
    )
    .unwrap();
    let (body, candidates) = s.pr_reference_set(a, pr).unwrap().unwrap();
    let ctx = PrContext {
        selected_issue: Some(issue),
        body_revision: body,
        provenance: candidates[0].provenance.clone(),
        ratio: 0.4,
    };
    s.set_pr_tab_context(a, pr, "first-pr", &ctx, PrContextMode::Issue)
        .unwrap();
    s.set_pr_tab_context(a, pr, "first-pr", &ctx, PrContextMode::Body)
        .unwrap();
    let again = Store::open_profile(dir.path()).unwrap();
    let layout = again.workspace(a).unwrap().unwrap();
    let tabs = &layout.panes[0].tabs;
    assert_eq!(
        tabs[0].view.context.as_ref().unwrap().selected_issue,
        Some(issue)
    );
    assert_eq!(
        tabs[0].view.pr_reader.as_ref().unwrap().context_mode,
        PrContextMode::Body
    );
    assert!(tabs[1].view.context.is_none());
    assert_eq!(
        again.pr_context(a, pr).unwrap().unwrap().selected_issue,
        Some(issue)
    );
    assert!(
        s.set_pr_tab_context(a, pr, "already-closed", &ctx, PrContextMode::Issue)
            .is_err()
    );
    s.set_pr_tab_context(a, pr, "first-pr", &ctx, PrContextMode::Issue)
        .unwrap();
    s.create_draft(
        a,
        "kept",
        issue,
        threadline_storage::queue::DraftKind::Comment,
        &threadline_storage::queue::DraftContent {
            body: "keep original".into(),
            base: "body".into(),
            anchor: "issue-4".into(),
        },
    )
    .unwrap();
    ingest(&s, a, r, 70, true, "Resolves #9.", 30);
    assert!(s.index_pr_body(a, pr, Some("body20"), "old", 20).is_err());
    let changed = s
        .index_pr_body(a, pr, Some("body20"), "body30", 30)
        .unwrap();
    let tab = s
        .workspace(a)
        .unwrap()
        .unwrap()
        .panes
        .remove(0)
        .tabs
        .remove(0);
    assert_eq!(tab.view.context.unwrap().selected_issue, None);
    assert_eq!(
        tab.view.pr_reader.unwrap().context_mode,
        PrContextMode::Body
    );
    assert_eq!(changed.len(), 1);
    assert!(
        s.record_pr_reference(a, pr, "body20", 0, proof(issue, 4), 40)
            .is_err()
    );
    assert_eq!(s.draft(a, "kept").unwrap().unwrap().item, issue);
}

#[test]
fn it_016_selected_offline_download_pins_one_revision_and_safe_retry_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("offline.sqlite3");
    let store = Store::open(&path).unwrap();
    let (account, repo, pr) = setup(&store);
    let rev = store
        .record_pr_revision(
            account,
            pr,
            &oid('a'),
            &oid('b'),
            Some(&oid('c')),
            &PrHeader::default(),
            100,
        )
        .unwrap();
    let mut settings = store.sync_settings(account).unwrap();
    settings.controls.work_offline = true;
    store.save_sync_settings(account, &settings).unwrap();
    let first = store.request_pr_download(&rev, true, false, 101).unwrap();
    assert!(first.body_downloaded);
    assert_eq!(first.state, PrDownloadState::Pending);
    assert!(store.start_pr_download(&rev, first.generation).is_err());
    assert!(store.cancel_pr_download(&rev, first.generation).unwrap());
    let second = store.request_pr_download(&rev, true, false, 102).unwrap();
    assert!(second.generation > first.generation);
    settings.controls.work_offline = false;
    store.save_sync_settings(account, &settings).unwrap();
    store.start_pr_download(&rev, second.generation).unwrap();
    let files = store
        .start_pr_fetch(&rev, PrFamily::Files, WalkMode::Full)
        .unwrap();
    assert!(
        store
            .commit_pr_download_page(
                &rev,
                first.generation,
                PrFamily::Files,
                &files,
                input(vec![file()], None, None)
            )
            .is_err()
    );
    let truncated = PrRecord::File {
        path: "partial.txt".into(),
        previous_path: None,
        status: "modified".into(),
        additions: Some(100),
        deletions: None,
        patch: PatchEvidence::Truncated("@@ -2 +2 @@\n text".into()),
    };
    store
        .commit_pr_download_page(
            &rev,
            second.generation,
            PrFamily::Files,
            &files,
            input(vec![truncated], None, None),
        )
        .unwrap();
    assert_eq!(
        store.finish_pr_download(&rev, second.generation).unwrap(),
        PrDownloadState::Partial
    );
    let current = store.request_pr_download(&rev, true, false, 103).unwrap();
    store.start_pr_download(&rev, current.generation).unwrap();
    for family in [
        PrFamily::Files,
        PrFamily::Reviews,
        PrFamily::Commits,
        PrFamily::Checks,
        PrFamily::Statuses,
        PrFamily::Threads,
        PrFamily::Context,
    ] {
        let fetch = store.start_pr_fetch(&rev, family, WalkMode::Full).unwrap();
        let contents = if family == PrFamily::Files {
            vec![file()]
        } else {
            vec![]
        };
        store
            .commit_pr_download_page(
                &rev,
                current.generation,
                family,
                &fetch,
                input(contents, None, None),
            )
            .unwrap();
    }
    let conversation = ResourceKey {
        account,
        target: ResourceTarget::Item(pr),
        family: ResourceFamily::Comments,
    };
    let fetch = store
        .start_resource_fetch(conversation, WalkMode::Full)
        .unwrap();
    store
        .commit_resource_page(
            conversation,
            &fetch,
            ResourcePage {
                records: &[ResourceRecord {
                    key: "comment:9".into(),
                    body: "persistent conversation".into(),
                }],
                next_cursor: None,
                completed_validators: None,
                fetched_at_ms: 111,
            },
        )
        .unwrap();
    assert_eq!(
        store.finish_pr_download(&rev, current.generation).unwrap(),
        PrDownloadState::Complete
    );
    ingest(
        &store,
        account,
        repo,
        70,
        true,
        "replacement current body",
        150,
    );
    let next = store
        .record_pr_revision(
            account,
            pr,
            &oid('a'),
            &oid('d'),
            Some(&oid('c')),
            &PrHeader::default(),
            160,
        )
        .unwrap();
    assert!(
        matches!(store.pr_revision_body(&rev).unwrap(),Change::Value(t) if t.starts_with("Fixes #4"))
    );
    assert!(
        matches!(store.pr_revision_body(&next).unwrap(),Change::Value(t) if t=="replacement current body")
    );
    let tied = store
        .record_pr_revision(
            account,
            pr,
            &oid('a'),
            &oid('e'),
            Some(&oid('c')),
            &PrHeader::default(),
            160,
        )
        .unwrap();
    assert_ne!(next.id, tied.id);
    assert_eq!(
        store.current_pr_revision(account, pr).unwrap().unwrap().0,
        next
    );
    store.clear_cache(account).unwrap();
    drop(store);
    let reopened = Store::open(&path).unwrap();
    let saved = reopened.pr_download(&rev).unwrap().unwrap();
    assert!(saved.fully_cached());
    assert_eq!(saved.state, PrDownloadState::Complete);
    assert_eq!(
        reopened
            .pr_page(&rev, PrFamily::Files, None, 5)
            .unwrap()
            .rows
            .len(),
        1
    );
}

#[test]
fn it_016_nested_inline_pages_keep_distinct_replies_anchors_and_category_baselines() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_profile(dir.path()).unwrap();
    let (a, _, pr) = setup(&store);
    store.schedule_selected_activity(a, 50).unwrap();
    let rev = store
        .record_pr_revision(
            a,
            pr,
            &oid('a'),
            &oid('b'),
            None,
            &PrHeader {
                requested_reviewers: Some(vec![]),
                ..Default::default()
            },
            52,
        )
        .unwrap();
    store
        .record_pr_revision(
            a,
            pr,
            &oid('a'),
            &oid('b'),
            None,
            &PrHeader {
                requested_reviewers: Some(vec!["octo".into()]),
                ..Default::default()
            },
            55,
        )
        .unwrap();
    let key = format!(
        "github:pr-review-requests:{}:octo:review_requested:55",
        pr.0
    );
    assert!(store.load_activity_event(a, &key).unwrap().is_some());
    let reply = |id, created, updated, body: &str| InlineComment {
        id,
        node_id: None,
        in_reply_to: if id == 4 { None } else { Some(4) },
        author: None,
        actor: Default::default(),
        body: body.into(),
        created_at_ms: Some(created),
        updated_at_ms: Some(updated),
        anchor: if id == 4 {
            Some(ReviewAnchor {
                original_oid: oid('b').as_str().into(),
                path: "x".into(),
                side: SourceSide::Right,
                line: 5,
            })
        } else {
            None
        },
    };
    let row = |comments, replies_complete| PrRecord::Thread {
        reply_cursor: if replies_complete {
            None
        } else {
            Some("inner2".into())
        },
        node_id: "T1".into(),
        resolved: None,
        outdated: None,
        comments,
        replies_complete,
    };
    let fetch = store
        .start_pr_fetch(&rev, PrFamily::Threads, WalkMode::Full)
        .unwrap();
    let next = store
        .commit_pr_page(
            &rev,
            PrFamily::Threads,
            &fetch,
            input(
                vec![row(
                    vec![reply(4, 20, 75, "edited"), reply(5, 61, 61, "second")],
                    false,
                )],
                Some("nested2"),
                None,
            ),
        )
        .unwrap()
        .unwrap();
    let nested = store.pr_reply_checkpoint(&rev, "T1").unwrap().unwrap();
    assert_eq!(nested.cursor, "inner2");
    let reopened = Store::open_profile(dir.path()).unwrap();
    let restart = reopened.pr_reply_checkpoint(&rev, "T1").unwrap().unwrap();
    assert_eq!(nested, restart);
    reopened
        .commit_pr_replies(
            &rev,
            &restart,
            vec![reply(7, 78, 79, "nested")],
            Some("inner3"),
            80,
        )
        .unwrap();
    assert!(
        store
            .commit_pr_replies(&rev, &nested, vec![], None, 81)
            .is_err()
    );
    let last = store.pr_reply_checkpoint(&rev, "T1").unwrap().unwrap();
    store
        .commit_pr_replies(&rev, &last, vec![], None, 82)
        .unwrap();
    assert!(store.pr_reply_checkpoint(&rev, "T1").unwrap().is_none());
    let v = Validators {
        etag: Some("stable-threads".into()),
        ..Default::default()
    };
    store
        .commit_pr_page(
            &rev,
            PrFamily::Threads,
            &next,
            PrInputPage {
                validators: Some(&v),
                ..input(
                    vec![row(
                        vec![reply(4, 20, 20, "old"), reply(6, 70, 70, "third")],
                        true,
                    )],
                    None,
                    None,
                )
            },
        )
        .unwrap();
    let data = store.pr_page(&rev, PrFamily::Threads, None, 5).unwrap();
    let PrRecord::Thread {
        comments,
        replies_complete,
        ..
    } = &data.rows[0].1
    else {
        panic!("expected thread")
    };
    assert_eq!(
        comments.iter().map(|c| c.id).collect::<Vec<_>>(),
        [4, 5, 6, 7]
    );
    assert_eq!(comments[0].body, "edited");
    assert!(comments[0].anchor.is_some() && *replies_complete);
    assert!(
        store
            .load_activity_event(a, "github:pr-inline-comments:4:inline_comment_created:20")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .load_activity_event(a, "github:pr-inline-comments:4:inline_comment_edited:75")
            .unwrap()
            .is_some()
    );
    let conditional = store
        .start_pr_fetch(&rev, PrFamily::Threads, WalkMode::Full)
        .unwrap();
    assert!(
        store
            .pr_not_modified(
                &rev,
                PrFamily::Threads,
                &conditional,
                &Validators::default(),
                120
            )
            .is_err()
    );
    store
        .pr_not_modified(&rev, PrFamily::Threads, &conditional, &v, 121)
        .unwrap();
    let interrupted = store
        .start_pr_fetch(&rev, PrFamily::Threads, WalkMode::Full)
        .unwrap();
    store
        .record_pr_fetch_error(
            &rev,
            PrFamily::Threads,
            &interrupted,
            ResourceError::Transport,
        )
        .unwrap();
    let recovered = store
        .resume_pr_fetch(&rev, PrFamily::Threads)
        .unwrap()
        .unwrap();
    assert!(
        store
            .record_pr_fetch_error(
                &rev,
                PrFamily::Threads,
                &interrupted,
                ResourceError::CursorExpired
            )
            .is_err()
    );
    store
        .record_pr_fetch_error(
            &rev,
            PrFamily::Threads,
            &recovered,
            ResourceError::CursorExpired,
        )
        .unwrap();
    assert!(
        store
            .resume_pr_fetch(&rev, PrFamily::Threads)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .pr_page(&rev, PrFamily::Threads, None, 5)
            .unwrap()
            .rows,
        data.rows
    );
}

#[test]
fn it_018_review_actor_suppression_keeps_content_and_human_replies_after_restart() {
    use threadline_storage::notifications::{
        ActorRule, Category, DeliveryMode, EventActor, RuleActor, RuleScope,
    };
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_profile(dir.path()).unwrap();
    let (account, repository, pr) = setup(&store);
    store.schedule_selected_activity(account, 50).unwrap();
    store
        .update_repository_notification_rules(account, repository, |rules| {
            rules.actor_rules.push(ActorRule {
                actor: RuleActor::KnownAutomation,
                scope: RuleScope::ReviewsAndInline,
            });
        })
        .unwrap();
    let rev = store
        .record_pr_revision(
            account,
            pr,
            &oid('a'),
            &oid('b'),
            None,
            &PrHeader::default(),
            51,
        )
        .unwrap();
    let bot = EventActor {
        user_id: Some(id(91)),
        known_automation: true,
        ..Default::default()
    };
    let human = EventActor {
        user_id: Some(id(92)),
        ..Default::default()
    };
    let formal = PrRecord::Review {
        id: 15,
        node_id: None,
        author: Some("reviewer".into()),
        actor: bot,
        body: "Automated findings".into(),
        state: "CHANGES_REQUESTED".into(),
        submitted_at_ms: Some(60),
        commit_oid: Some(oid('b').as_str().into()),
    };
    let inline = |id, in_reply_to, actor, body: &str| InlineComment {
        id,
        node_id: None,
        in_reply_to,
        author: None,
        actor,
        body: body.into(),
        created_at_ms: Some(60),
        updated_at_ms: Some(60),
        anchor: None,
    };
    let discussion = PrRecord::Thread {
        node_id: "T1".into(),
        resolved: Some(false),
        outdated: Some(false),
        comments: vec![
            inline(21, None, bot, "Automated detail"),
            inline(22, Some(21), human, "I checked this"),
        ],
        replies_complete: true,
        reply_cursor: None,
    };
    for (family, row) in [
        (PrFamily::Reviews, formal.clone()),
        (PrFamily::Threads, discussion.clone()),
    ] {
        let fetch = store.start_pr_fetch(&rev, family, WalkMode::Full).unwrap();
        store
            .commit_pr_page(&rev, family, &fetch, input(vec![row], None, None))
            .unwrap();
    }
    drop(store);
    let reopened = Store::open_profile(dir.path()).unwrap();
    assert_eq!(
        reopened
            .process_notification_events(account, 101, "UTC", true, 20)
            .unwrap(),
        3
    );
    let visible = reopened.notification_feed(account, None, 20).unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(
        visible[0].event_key,
        "github:pr-inline-comments:22:inline_comment_created:60"
    );
    assert_eq!(visible[0].categories, [Category::InlineComment].into());
    assert_eq!(visible[0].decision.mode, DeliveryMode::FeedOnly);
    assert_eq!(
        reopened
            .pr_page(&rev, PrFamily::Reviews, None, 5)
            .unwrap()
            .rows[0]
            .1,
        formal
    );
    assert_eq!(
        reopened
            .pr_page(&rev, PrFamily::Threads, None, 5)
            .unwrap()
            .rows[0]
            .1,
        discussion
    );
}

#[test]
fn it_006_it_016_scheduled_pr_pages_checkpoint_first_etag_and_reject_changed_access() {
    use threadline_storage::sync::ReadPriority;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_profile(dir.path()).unwrap();
    let (account, repository, pr) = setup(&store);
    let rev = store
        .record_pr_revision(
            account,
            pr,
            &oid('a'),
            &oid('b'),
            None,
            &PrHeader::default(),
            105,
        )
        .unwrap();
    let target = rev.read_target(PrFamily::Files);
    store
        .subscribe_read(
            account,
            &target,
            "explicit-files",
            ReadPriority::Explicit,
            1_000,
        )
        .unwrap();
    let claim = store.claim_sync_read(account, 1_000).unwrap().unwrap();
    let first = store
        .prepare_sync_pr_page(&claim, &rev, PrFamily::Files, 1_001)
        .unwrap();
    let first_etag = Validators {
        etag: Some("page-one".into()),
        ..Default::default()
    };
    store
        .commit_sync_pr_page(
            &claim,
            &rev,
            PrFamily::Files,
            &first,
            PrInputPage {
                validators: Some(&first_etag),
                fetched_at_ms: 1_002,
                ..input(vec![file()], Some("next"), None)
            },
            "UTC",
        )
        .unwrap();
    drop(store);
    let store = Store::open_profile(dir.path()).unwrap();
    let next = store.claim_sync_read(account, 2_010).unwrap().unwrap();
    assert_eq!(next.cursor.as_deref(), Some("next"));
    let second = store
        .prepare_sync_pr_page(&next, &rev, PrFamily::Files, 2_011)
        .unwrap();
    assert!(
        store
            .commit_sync_pr_page(
                &claim,
                &rev,
                PrFamily::Files,
                &first,
                PrInputPage {
                    fetched_at_ms: 2_012,
                    ..input(vec![], None, None)
                },
                "UTC"
            )
            .is_err()
    );
    let last = Validators {
        etag: Some("page-two".into()),
        ..Default::default()
    };
    store
        .commit_sync_pr_page(
            &next,
            &rev,
            PrFamily::Files,
            &second,
            PrInputPage {
                validators: Some(&last),
                fetched_at_ms: 2_012,
                ..input(vec![], None, None)
            },
            "UTC",
        )
        .unwrap();
    assert_eq!(
        store
            .pr_page(&rev, PrFamily::Files, None, 3)
            .unwrap()
            .state
            .validators,
        first_etag
    );
    store
        .subscribe_read(
            account,
            &target,
            "explicit-conditional",
            ReadPriority::Explicit,
            3_012,
        )
        .unwrap();
    let conditional = store.claim_sync_read(account, 3_015).unwrap().unwrap();
    assert_eq!(conditional.validators, first_etag);
    let checkpoint = store
        .prepare_sync_pr_page(&conditional, &rev, PrFamily::Files, 3_016)
        .unwrap();
    store
        .revalidate_sync_pr(
            &conditional,
            &rev,
            PrFamily::Files,
            &checkpoint,
            3_017,
            "UTC",
        )
        .unwrap();
    store
        .subscribe_read(
            account,
            &target,
            "explicit-race",
            ReadPriority::Explicit,
            4_016,
        )
        .unwrap();
    let final_claim = store.claim_sync_read(account, 4_020).unwrap().unwrap();
    let expected = store
        .prepare_sync_pr_page(&final_claim, &rev, PrFamily::Files, 4_021)
        .unwrap();
    store
        .select_github_repository(account, repository, false)
        .unwrap();
    assert!(
        store
            .commit_sync_pr_page(
                &final_claim,
                &rev,
                PrFamily::Files,
                &expected,
                PrInputPage {
                    fetched_at_ms: 4_022,
                    ..input(vec![], None, None)
                },
                "UTC"
            )
            .is_err()
    );
    assert_eq!(
        store
            .pr_page(&rev, PrFamily::Files, None, 3)
            .unwrap()
            .rows
            .len(),
        1
    );
}

#[test]
fn it_017_cross_owner_issue_verification_does_not_select_or_monitor_the_other_repository() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_profile(dir.path()).unwrap();
    let (account, _, pr) = setup(&store);
    let refs = store
        .index_pr_body(account, pr, None, "body20", 20)
        .unwrap();
    assert_eq!(
        (
            refs[2].owner.as_str(),
            refs[2].repository.as_str(),
            refs[2].number
        ),
        ("other", "x", 7)
    );
    let profile = store
        .reserve_access_profile_credential(
            account,
            id(80),
            "user",
            "other",
            CredentialKind::FineGrained,
            None,
        )
        .unwrap();
    store
        .install_current_credential(&profile, || Ok::<_, ()>(()))
        .unwrap()
        .unwrap();
    store
        .record_access_result(
            &profile,
            profile.kind,
            AccessState::Readable,
            DiscoveryState::Complete,
            55,
        )
        .unwrap();
    let repository = store
        .observe_github_repository(
            &profile,
            &RepositoryObservation {
                provider_id: id(201),
                owner: "other",
                name: "x",
                private: true,
                source: RepositoryObservationSource::Direct,
            },
            56,
        )
        .unwrap();
    let issue = ingest(
        &store,
        account,
        repository,
        7,
        false,
        "cross-repository issue",
        58,
    );
    assert_eq!(
        store
            .record_pr_reference(
                account,
                pr,
                "body20",
                2,
                ReferenceObservation::Item {
                    repository,
                    item: issue,
                    issue_id: IssueApiId::new(id(1007))
                },
                60
            )
            .unwrap(),
        Verification::Verified
    );
    let observed = store.github_repository_views(account).unwrap();
    assert!(
        !observed
            .iter()
            .find(|r| r.id == repository)
            .unwrap()
            .selected
    );
    let refs = store.pr_body_references(account, pr).unwrap();
    assert_eq!(refs[2].verified_item, Some(issue));
    assert_eq!(
        store
            .record_pr_reference(account, pr, "body20", 2, ReferenceObservation::Offline, 65)
            .unwrap(),
        Verification::Offline
    );
    let offline = store.pr_body_references(account, pr).unwrap();
    assert_eq!(offline[2].verified_item, Some(issue));
    assert_eq!(offline[2].verification, Verification::Offline);
}

#[test]
fn it_017_body_and_reference_proof_are_one_snapshot_across_refresh() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, r, pr) = setup(&db);
    let old = db
        .record_pr_revision(a, pr, &oid('a'), &oid('b'), None, &PrHeader::default(), 20)
        .unwrap();
    assert!(db.pr_revision_context(&old).unwrap().1.is_none());
    db.index_pr_body(a, pr, None, "original", 20).unwrap();
    assert_eq!(db.pr_revision_context(&old).unwrap().1.unwrap().1.len(), 3);
    ingest(&db, a, r, 70, true, "No confirmed issue reference", 30);
    let new = db
        .record_pr_revision(a, pr, &oid('a'), &oid('c'), None, &PrHeader::default(), 30)
        .unwrap();
    assert!(db.pr_revision_context(&old).unwrap().1.is_none());
    assert!(db.pr_revision_context(&new).unwrap().1.is_none());
    assert!(
        db.record_pr_reference(a, pr, "original", 0, ReferenceObservation::Missing, 31)
            .is_err()
    );
    db.index_pr_body(a, pr, Some("original"), "current", 30)
        .unwrap();
    let (body, set) = db.pr_revision_context(&new).unwrap();
    assert!(matches!(body,Change::Value(t) if t=="No confirmed issue reference"));
    let (tag, rows) = set.unwrap();
    assert_eq!(tag, "current");
    assert!(rows.is_empty());
    assert!(db.pr_revision_context(&old).unwrap().1.is_none());
}
