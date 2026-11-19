use std::num::NonZeroU64;
use threadline_storage::{
    access::*, pr::*, queue::*, resources::*, reviews::*, writes::Permission, *,
};

fn n(v: u64) -> NonZeroU64 {
    NonZeroU64::new(v).unwrap()
}
fn oid(c: char) -> CommitOid {
    CommitOid::parse(&c.to_string().repeat(40)).unwrap()
}
fn setup(db: &Store) -> (AccountId, RepositoryId) {
    let a = db.ensure_account("github.com", 80).unwrap();
    let p = db
        .reserve_access_profile_credential(
            a,
            n(80),
            "fixture",
            "work",
            CredentialKind::FineGrained,
            None,
        )
        .unwrap();
    db.install_current_credential(&p, || Ok::<_, ()>(()))
        .unwrap()
        .unwrap();
    db.activate_local_profile(a).unwrap();
    db.record_access_result(
        &p,
        p.kind,
        AccessState::Readable,
        DiscoveryState::Complete,
        1,
    )
    .unwrap();
    let repo = db
        .observe_github_repository(
            &p,
            &RepositoryObservation {
                provider_id: n(200),
                owner: "work",
                name: "code",
                private: true,
                source: RepositoryObservationSource::Direct,
            },
            2,
        )
        .unwrap();
    db.select_github_repository(a, repo, true).unwrap();
    (a, repo)
}
fn rev(db: &Store, a: AccountId, r: RepositoryId, num: u64, head: char, at: i64) -> PrRevision {
    let pr = db
        .ingest(
            a,
            ItemLocator {
                repository: r,
                number: num,
            },
            ItemUpdate {
                representation: ItemRepresentation::PullRequest(PullRequestRecord {
                    id: PullRequestApiId::try_from(500 + num).unwrap(),
                    node_id: None,
                    details: PullRequestUpdate {
                        merged: Change::Value(false),
                        base_oid: Change::Value(oid('a').as_str().into()),
                        head_oid: Change::Value(oid(head).as_str().into()),
                        merge_oid: Change::Missing,
                    },
                }),
                remote_updated_at: UtcMillis(at),
                title: Change::Value("pull request".into()),
                body: Change::Null,
                closed: Change::Value(false),
            },
        )
        .unwrap();
    let rev = db
        .record_pr_revision(
            a,
            pr,
            &oid('a'),
            &oid(head),
            Some(&oid('c')),
            &PrHeader::default(),
            at,
        )
        .unwrap();
    let files = PrRecord::File {
        path: "new.rs".into(),
        previous_path: Some("old.rs".into()),
        status: "renamed".into(),
        additions: Some(2),
        deletions: Some(1),
        patch: PatchEvidence::Text(
            "@@ -8,2 +12,3 @@\n same\n-old\n+new\n+again\n@@ -32 +60 @@\n end".into(),
        ),
    };
    let threads = PrRecord::Thread {
        node_id: "THREAD".into(),
        resolved: Some(false),
        outdated: Some(false),
        comments: vec![InlineComment {
            id: 600,
            node_id: None,
            in_reply_to: None,
            author: Some("someone".into()),
            actor: Default::default(),
            body: "first".into(),
            created_at_ms: Some(at),
            updated_at_ms: None,
            anchor: None,
        }],
        replies_complete: true,
        reply_cursor: None,
    };
    for (family, row) in [(PrFamily::Files, files), (PrFamily::Threads, threads)] {
        let fetch = db.start_pr_fetch(&rev, family, WalkMode::Full).unwrap();
        db.commit_pr_page(
            &rev,
            family,
            &fetch,
            PrInputPage {
                records: vec![row],
                next_cursor: None,
                validators: None,
                partial: None,
                fetched_at_ms: at,
            },
        )
        .unwrap();
    }
    rev
}
fn review(event: ReviewEvent) -> ReviewDraft {
    ReviewDraft {
        event,
        summary: "review text".into(),
        comments: vec![],
    }
}
fn submit(db: &Store, rev: &PrRevision, intent: &ReviewIntent, deps: &[&str]) -> String {
    let id = db.new_operation_id().unwrap();
    db.submit_review(
        rev,
        ReviewSubmission {
            id: &id,
            intent,
            draft: None,
            dependencies: deps,
            submitted_at_ms: 4,
        },
    )
    .unwrap()
}
fn observation(claim: &ReviewClaim, now: i64) -> ReviewObservation {
    let r = &claim.route;
    ReviewObservation {
        actor: r.actor,
        repository_id: r.repository_id,
        pr_id: r.pr_id,
        number: r.number,
        credential_generation: r.credential_generation,
        observed_at_ms: now,
        base: r.base.clone(),
        head: r.head.clone(),
        permission: Permission::Allowed,
        pr_open: Some(true),
        pr_author: Some(n(91)),
        pending: match &claim.step {
            ReviewStep::SubmitPending { review_id, .. } => RemotePending::Present {
                id: *review_id,
                actor: r.actor,
                head: r.head.clone(),
            },
            _ => RemotePending::Absent,
        },
        thread: Some(ThreadObservation {
            id: "THREAD".into(),
            outdated: Some(false),
            resolved: Some(false),
            root_comment_id: Some(n(600)),
        }),
    }
}
fn send(db: &Store, c: &ReviewClaim, at: i64) {
    let ReviewAdmission::Send(ticket) = db.authorize_review(c, &observation(c, at), at).unwrap()
    else {
        panic!("unexpected review hold")
    };
    assert!(db.start_review_transport(&ticket, at).unwrap());
    assert!(!db.start_review_transport(&ticket, at).unwrap());
}
fn response(c: &ReviewClaim, result: ReviewResult) -> ReviewResponse {
    ReviewResponse {
        actor: c.route.actor,
        repository_id: c.route.repository_id,
        pr_id: c.route.pr_id,
        number: c.route.number,
        result,
    }
}
#[test]
fn it_013_review_drafts_keep_original_multiline_anchors_after_eviction_force_push_and_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, r) = setup(&db);
    let original = rev(&db, a, r, 70, 'b', 20);
    let left = db
        .capture_inline_anchor(&original, "new.rs", SourceSide::Left, 2, 3)
        .unwrap();
    assert_eq!(
        (
            &*left.path,
            &*left.source_path,
            left.first_line,
            left.last_line
        ),
        ("new.rs", "old.rs", 8, 9)
    );
    let right = db
        .capture_inline_anchor(&original, "new.rs", SourceSide::Right, 4, 5)
        .unwrap();
    assert_eq!((right.first_line, right.last_line), (13, 14));
    assert_eq!(
        db.capture_inline_source_anchor(&original, "new.rs", SourceSide::Right, 13, 14)
            .unwrap(),
        right
    );
    assert_eq!(
        db.capture_inline_source_anchor(&original, "new.rs", SourceSide::Left, 8, 9)
            .unwrap(),
        left
    );
    assert!(
        db.capture_inline_source_anchor(&original, "new.rs", SourceSide::Right, 13, 60)
            .is_err()
    );
    assert!(
        db.capture_inline_source_anchor(&original, "new.rs", SourceSide::Right, 3, 4)
            .is_err()
    );
    assert_eq!(right.source_oid, original.head.as_str());
    assert!(right.original_hunk.contains("+again"));
    assert!(
        db.capture_inline_anchor(&original, "new.rs", SourceSide::Right, 3, 5)
            .is_err()
    );
    assert!(
        db.capture_inline_anchor(&original, "new.rs", SourceSide::Right, 4, 7)
            .is_err()
    );
    let mut draft = review(ReviewEvent::RequestChanges);
    draft.comments = vec![
        DraftInline {
            key: "first".into(),
            body: "old side".into(),
            anchor: left.clone(),
        },
        DraftInline {
            key: "second".into(),
            body: "new side".into(),
            anchor: right.clone(),
        },
    ];
    db.create_review_draft(&original, "review", &draft).unwrap();
    assert!(db.acquire_editor(a, "review", "view-one", 30, 300).unwrap());
    assert!(!db.acquire_editor(a, "review", "view-two", 31, 300).unwrap());
    let current = rev(&db, a, r, 70, 'd', 40);
    let lease = DraftSubmission {
        id: "review",
        revision: 1,
        editor: "view-one",
        now: 43,
    };
    let mut replacement = draft.clone();
    replacement.comments[1].anchor = db
        .capture_inline_anchor(&current, "new.rs", SourceSide::Right, 4, 5)
        .unwrap();
    assert!(db.save_review_draft(&current, lease, &replacement).is_err());
    assert_eq!(
        db.reanchor_review_draft(&current, lease, &replacement)
            .unwrap(),
        2
    );
    assert_eq!(
        db.original_review_draft(a, "review", 1)
            .unwrap()
            .unwrap()
            .draft,
        draft
    );
    db.clear_cache(a).unwrap();
    drop(db);
    let db = Store::open_profile(dir.path()).unwrap();
    let saved = db.review_draft(a, "review").unwrap().unwrap();
    assert_eq!(
        (saved.item, saved.revision_id, saved.revision),
        (current.pr, current.id, 2)
    );
    assert_eq!(saved.draft, replacement);
    let id = db.new_operation_id().unwrap();
    let submit_lease = DraftSubmission {
        id: "review",
        revision: 2,
        editor: "view-one",
        now: 50,
    };
    let intent = ReviewIntent::Review(replacement.clone());
    let input = |op| ReviewSubmission {
        id: op,
        intent: &intent,
        draft: Some(submit_lease),
        dependencies: &[],
        submitted_at_ms: 50,
    };
    assert_eq!(db.submit_review(&current, input(&id)).unwrap(), id);
    assert_eq!(
        db.submit_review(&current, input("duplicate-view-operation"))
            .unwrap(),
        id
    );
    assert_eq!(
        db.operation(a, &id)
            .unwrap()
            .unwrap()
            .snapshot
            .draft
            .unwrap()
            .body,
        replacement.summary
    );
}

#[test]
fn it_011_confirmed_pending_review_identity_survives_crash_and_final_event_never_recreates_review()
{
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, r) = setup(&db);
    let original = rev(&db, a, r, 72, 'b', 20);
    let id = submit(
        &db,
        &original,
        &ReviewIntent::Review(review(ReviewEvent::Approve)),
        &[],
    );
    assert!(db.claim_next(a, 30, 100).unwrap().is_none());
    assert!(db.claim_write(a, 30, 100).unwrap().is_none());
    let create = db.claim_review(a, 30, 100).unwrap().unwrap();
    send(&db, &create, 31);
    db.record_review_failure(a, &id, create.attempt(), ReviewFailure::Ambiguous, 32)
        .unwrap();
    let received = response(
        &create,
        ReviewResult::Pending {
            id: n(999),
            author: n(80),
            head: create.route.head.clone(),
        },
    );
    assert!(
        db.confirm_review_step(
            a,
            &id,
            create.attempt(),
            ReviewProof::AcceptedObjectRead,
            &received,
            34
        )
        .is_err()
    );
    assert!(db.claim_review(a, 2_000, 3_000).unwrap().is_none());
    db.accept_review_object(a, &id, create.attempt(), n(999), "request-1")
        .unwrap();
    db.confirm_review_step(
        a,
        &id,
        create.attempt(),
        ReviewProof::AcceptedObjectRead,
        &received,
        36,
    )
    .unwrap();
    drop(db);
    let db = Store::open_profile(dir.path()).unwrap();
    let final_step = db.claim_review(a, 2_000, 3_000).unwrap().unwrap();
    assert!(
        matches!(final_step.step,ReviewStep::SubmitPending{review_id,event:ReviewEvent::Approve,..} if review_id==n(999))
    );
    assert_eq!(db.receipts(a, &id).unwrap().len(), 1);
    send(&db, &final_step, 2_001);
    assert_eq!(
        db.cancel_operation(a, &id).unwrap(),
        CancelResult::MayAlreadyBeSent
    );
    let mut result = response(
        &final_step,
        ReviewResult::Submitted {
            id: n(998),
            author: n(80),
            head: final_step.route.head.clone(),
            event: ReviewEvent::Approve,
        },
    );
    assert!(
        db.confirm_review_step(
            a,
            &id,
            final_step.attempt(),
            ReviewProof::MutationResponse { request_id: "bad" },
            &result,
            2_002
        )
        .is_err()
    );
    result.result = ReviewResult::Submitted {
        id: n(999),
        author: n(80),
        head: final_step.route.head.clone(),
        event: ReviewEvent::Approve,
    };
    db.confirm_review_step(
        a,
        &id,
        final_step.attempt(),
        ReviewProof::AcceptedObjectRead,
        &result,
        2_003,
    )
    .unwrap();
    assert_eq!(
        db.operation(a, &id).unwrap().unwrap().state,
        State::Confirmed
    );
    assert_eq!(db.receipts(a, &id).unwrap().len(), 2);
    assert!(db.claim_review(a, 4_001, 5_000).unwrap().is_none());
}

#[test]
fn it_013_thread_actions_require_original_thread_and_an_unchanged_authorized_revision() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, r) = setup(&db);
    let first = rev(&db, a, r, 72, 'b', 20);
    let second = rev(&db, a, r, 73, 'e', 21);
    let invalid = ReviewIntent::Reply {
        thread_id: "THREAD".into(),
        root_comment_id: n(601),
        body: "hello".into(),
    };
    assert!(
        db.submit_review(
            &first,
            ReviewSubmission {
                id: &db.new_operation_id().unwrap(),
                intent: &invalid,
                draft: None,
                dependencies: &[],
                submitted_at_ms: 25
            }
        )
        .is_err()
    );
    let reply = ReviewIntent::Reply {
        thread_id: "THREAD".into(),
        root_comment_id: n(600),
        body: "hello".into(),
    };
    let reply_id = submit(&db, &first, &reply, &[]);
    let held = submit(
        &db,
        &second,
        &ReviewIntent::Review(review(ReviewEvent::Comment)),
        &[&reply_id],
    );
    let independent = submit(
        &db,
        &second,
        &ReviewIntent::SetThreadResolved {
            thread_id: "THREAD".into(),
            resolved: true,
        },
        &[],
    );
    let c = db.claim_review(a, 30, 100).unwrap().unwrap();
    let mut observed = observation(&c, 31);
    observed.head = oid('f').as_str().into();
    assert!(matches!(
        db.authorize_review(&c, &observed, 31).unwrap(),
        ReviewAdmission::Held("review_revision_changed")
    ));
    assert!(db.dispatch_hold(a, &held).unwrap().is_some());
    // This operation shares a PR with the dependent row and remains in order behind it.
    assert!(db.claim_review(a, 32, 100).unwrap().is_none());
    db.cancel_operation(a, &held).unwrap();
    let next = db.claim_review(a, 33, 100).unwrap().unwrap();
    assert_eq!(next.operation_id(), independent);
    send(&db, &next, 34);
    let result = response(
        &next,
        ReviewResult::Thread {
            id: "THREAD".into(),
            resolved: true,
        },
    );
    let staged = db
        .start_pr_fetch(&second, PrFamily::Threads, WalkMode::Full)
        .unwrap();
    let cached_thread = |revision| {
        db.pr_record(revision, PrFamily::Threads, "thread:THREAD")
            .unwrap()
            .unwrap()
    };
    assert!(
        db.confirm_review_step(
            a,
            &independent,
            next.attempt(),
            ReviewProof::AcceptedObjectRead,
            &result,
            35
        )
        .is_err()
    );
    let old_thread = cached_thread(&second);
    assert!(matches!(
        old_thread,
        PrRecord::Thread {
            resolved: Some(false),
            ..
        }
    ));
    db.confirm_review_step(
        a,
        &independent,
        next.attempt(),
        ReviewProof::MutationResponse {
            request_id: "thread-action",
        },
        &result,
        36,
    )
    .unwrap();
    assert_eq!(db.receipts(a, &independent).unwrap().len(), 1);
    assert!(matches!(
        cached_thread(&second),
        PrRecord::Thread {
            resolved: Some(true),
            ..
        }
    ));
    assert!(matches!(
        cached_thread(&first),
        PrRecord::Thread {
            resolved: Some(false),
            ..
        }
    ));
    assert!(
        db.commit_pr_page(
            &second,
            PrFamily::Threads,
            &staged,
            PrInputPage {
                records: vec![old_thread],
                next_cursor: None,
                validators: None,
                partial: None,
                fetched_at_ms: 37,
            }
        )
        .is_err()
    );
    assert!(
        db.resume_pr_fetch(&second, PrFamily::Threads)
            .unwrap()
            .is_none()
    );
}

#[test]
fn it_011_cancelled_pending_review_keeps_issued_id_and_holds_dependents_while_other_pr_dispatches()
{
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, r) = setup(&db);
    let first = rev(&db, a, r, 72, 'b', 20);
    let dependent = rev(&db, a, r, 73, 'd', 21);
    let independent = rev(&db, a, r, 74, 'e', 22);
    let review_id = submit(
        &db,
        &first,
        &ReviewIntent::Review(review(ReviewEvent::RequestChanges)),
        &[],
    );
    let child = submit(
        &db,
        &dependent,
        &ReviewIntent::Review(review(ReviewEvent::Approve)),
        &[&review_id],
    );
    let reply = submit(
        &db,
        &independent,
        &ReviewIntent::Reply {
            thread_id: "THREAD".into(),
            root_comment_id: n(600),
            body: "check".into(),
        },
        &[],
    );
    let c = db.claim_review(a, 100, 300).unwrap().unwrap();
    send(&db, &c, 101);
    assert_eq!(
        db.cancel_operation(a, &review_id).unwrap(),
        CancelResult::MayAlreadyBeSent
    );
    let pending = response(
        &c,
        ReviewResult::Pending {
            id: n(877),
            author: n(80),
            head: c.route.head.clone(),
        },
    );
    db.confirm_review_step(
        a,
        &review_id,
        c.attempt(),
        ReviewProof::MutationResponse {
            request_id: "pending-before-cancel",
        },
        &pending,
        103,
    )
    .unwrap();
    assert_eq!(db.receipts(a, &review_id).unwrap()[0].remote_id, "877");
    assert_eq!(
        db.operation(a, &review_id).unwrap().unwrap().state,
        State::Cancelled
    );
    assert_eq!(
        db.dispatch_hold(a, &child).unwrap().as_deref(),
        Some("dependency")
    );
    drop(db);
    let db = Store::open_profile(dir.path()).unwrap();
    let c = db.claim_review(a, 1_200, 2_300).unwrap().unwrap();
    assert_eq!(c.operation_id(), reply);
    send(&db, &c, 1_201);
    db.accept_review_object(a, &reply, c.attempt(), n(899), "reply-created")
        .unwrap();
    assert!(
        db.record_review_failure(
            a,
            &reply,
            c.attempt(),
            ReviewFailure::Unsent { retry_at_ms: 1_210 },
            1_209
        )
        .is_err()
    );
    assert!(db.claim_review_readback(a, 1_900, 2_900).unwrap().is_none());
    let read = db.claim_review_readback(a, 2_300, 3_300).unwrap().unwrap();
    assert_eq!(db.review_readback_profile(&read).unwrap().account, a);
    assert_eq!(read.remote_id, n(899));
    let observed = response(
        &c,
        ReviewResult::Reply {
            id: n(899),
            author: n(80),
            root_comment_id: n(600),
            body: "check".into(),
        },
    );
    db.confirm_review_step(
        a,
        &reply,
        c.attempt(),
        ReviewProof::AcceptedObjectRead,
        &observed,
        2_301,
    )
    .unwrap();
    assert_eq!(
        db.operation(a, &reply).unwrap().unwrap().state,
        State::Confirmed
    );
    assert!(db.next_review_due(a, 2_400).unwrap().is_none());
}

#[test]
fn it_013_approval_rechecks_author_identity_and_rechecks_head_between_preflight_and_send() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, r) = setup(&db);
    let pr = rev(&db, a, r, 72, 'b', 20);
    let first = submit(
        &db,
        &pr,
        &ReviewIntent::Review(review(ReviewEvent::Approve)),
        &[],
    );
    let c = db.claim_review(a, 30, 90).unwrap().unwrap();
    let mut o = observation(&c, 31);
    o.pr_author = Some(c.route.actor);
    assert!(matches!(
        db.authorize_review(&c, &o, 31).unwrap(),
        ReviewAdmission::Held("cannot_review_own_or_unknown_author")
    ));
    db.cancel_operation(a, &first).unwrap();
    let second = submit(
        &db,
        &pr,
        &ReviewIntent::Review(review(ReviewEvent::RequestChanges)),
        &[],
    );
    let c = db.claim_review(a, 33, 100).unwrap().unwrap();
    let ReviewAdmission::Send(ticket) = db.authorize_review(&c, &observation(&c, 34), 34).unwrap()
    else {
        panic!("unexpected veto")
    };
    rev(&db, a, r, 72, 'f', 36);
    assert!(!db.start_review_transport(&ticket, 37).unwrap());
    let held = db.operation(a, &second).unwrap().unwrap();
    assert_eq!(held.state, State::NeedsAttention);
    assert_eq!(db.receipts(a, &second).unwrap().len(), 0);
    assert!(db.claim_review(a, 105, 205).unwrap().is_none());
}

#[test]
fn it_013_preflight_rate_floor_and_credential_hold_do_not_retry_a_denied_review_blindly() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, r) = setup(&db);
    let pr = rev(&db, a, r, 72, 'b', 20);
    let op = submit(
        &db,
        &pr,
        &ReviewIntent::Review(review(ReviewEvent::Comment)),
        &[],
    );
    let c = db.claim_review(a, 30, 300).unwrap().unwrap();
    db.fail_review_preflight(
        &c,
        threadline_storage::writes::WritePreflightFailure::Throttled {
            not_before_ms: 30_000,
        },
        31,
    )
    .unwrap();
    let next = db.next_review_due(a, 32).unwrap().unwrap();
    assert!(next >= 30_000);
    drop(db);
    let db = Store::open_profile(dir.path()).unwrap();
    assert!(db.claim_review(a, next - 1, next + 100).unwrap().is_none());
    let c = db.claim_review(a, next, next + 1000).unwrap().unwrap();
    db.fail_review_preflight(
        &c,
        threadline_storage::writes::WritePreflightFailure::Forbidden,
        next + 1,
    )
    .unwrap();
    assert_eq!(db.resume_reviews(a, next + 2).unwrap(), 0);
    assert_eq!(
        db.operation(a, &op).unwrap().unwrap().state,
        State::NeedsAttention
    );
    assert!(db.next_review_due(a, next + 2).unwrap().is_none());
}
