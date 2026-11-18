use threadline_storage as storage;
use threadline_storage::{
    access::*,
    merges::*,
    pr::*,
    queue::*,
    resources::CommitOid,
    writes::{Permission, WritePreflightFailure},
    *,
};
#[path = "support/write_profile.rs"]
mod write_profile;
use write_profile::{id, setup};

fn oid(ch: char) -> CommitOid {
    CommitOid::parse(&ch.to_string().repeat(40)).unwrap()
}
fn policy() -> MergePolicy {
    MergePolicy {
        allowed_methods: Some(
            [MergeMethod::Merge, MergeMethod::Squash, MergeMethod::Rebase]
                .into_iter()
                .collect(),
        ),
        viewer_permission: Permission::Allowed,
        open: Some(true),
        draft: Some(false),
        merged: Some(false),
        mergeable: Some(true),
        queue: QueueRequirement::NotRequired,
        checks: Gate::Satisfied,
        reviews: Gate::NotRequired,
        branch_rules: Gate::Satisfied,
    }
}
fn revision(
    s: &Store,
    a: AccountId,
    r: RepositoryId,
    number: u64,
    head: char,
    at: i64,
) -> PrRevision {
    let pr = s
        .ingest(
            a,
            ItemLocator {
                repository: r,
                number,
            },
            ItemUpdate {
                remote_updated_at: UtcMillis(at),
                representation: ItemRepresentation::PullRequest(PullRequestRecord {
                    id: PullRequestApiId::try_from(900 + number).unwrap(),
                    node_id: None,
                    details: PullRequestUpdate {
                        merged: Change::Value(false),
                        base_oid: Change::Value(oid('a').as_str().into()),
                        head_oid: Change::Value(oid(head).as_str().into()),
                        ..Default::default()
                    },
                }),
                title: Change::Value("PR".into()),
                body: Change::Null,
                closed: Change::Value(false),
            },
        )
        .unwrap();
    let rev = s
        .record_pr_revision(
            a,
            pr,
            &oid('a'),
            &oid(head),
            Some(&oid('c')),
            &PrHeader {
                base_ref: Some("main".into()),
                ..Default::default()
            },
            at,
        )
        .unwrap();
    let p = s.github_access_profile(a, "work").unwrap().unwrap();
    s.record_merge_policy(&p, &rev, &policy(), at).unwrap();
    rev
}
fn submit(s: &Store, rev: &PrRevision, method: MergeMethod) -> String {
    s.submit_merge(
        rev,
        MergeSubmission {
            id: &s.new_operation_id().unwrap(),
            method,
            dependencies: &[],
            submitted_at_ms: 30,
        },
    )
    .unwrap()
}
fn obs(c: &MergeClaim, at: i64) -> MergeObservation {
    let r = &c.route;
    MergeObservation {
        actor: r.actor,
        repository_id: r.repository_id,
        pr_id: r.pr_id,
        number: r.number,
        credential_generation: r.credential_generation,
        base: r.base.clone(),
        base_ref: c.expected_base_ref.clone(),
        head: r.head.clone(),
        observed_at_ms: at,
        policy: policy(),
    }
}
fn merged(c: &MergeClaim, by: u64) -> MergedPr {
    let r = &c.route;
    MergedPr {
        actor: r.actor,
        repository_id: r.repository_id,
        pr_id: r.pr_id,
        number: r.number,
        expected_head: r.head.clone(),
        merge_oid: oid('f'),
        merged_by: Some(id(by)),
        remote_updated_at_ms: Some(300),
    }
}
fn ticket(s: &Store, c: &MergeClaim, at: i64) -> MergeTicket {
    let MergeAdmission::Send(t) = s.authorize_merge(c, &obs(c, at), at).unwrap() else {
        panic!("unexpected merge hold")
    };
    t
}
#[test]
fn it_014_offline_merge_pins_head_method_and_duplicate_view_then_requires_known_direct_rules() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (a, r, issue) = setup(&s);
    let rev = revision(&s, a, r, 72, 'b', 20);
    s.set_dispatch_controls(a, true, false).unwrap();
    let op = submit(&s, &rev, MergeMethod::Squash);
    assert_eq!(submit(&s, &rev, MergeMethod::Squash), op);
    assert!(
        s.submit_merge(
            &rev,
            MergeSubmission {
                id: &s.new_operation_id().unwrap(),
                method: MergeMethod::Rebase,
                dependencies: &[],
                submitted_at_ms: 33
            }
        )
        .is_err()
    );
    assert!(s.claim_merge(a, 40, 90).unwrap().is_none());
    assert!(s.confirmed_merge(a, &op).unwrap().is_none());
    assert_eq!(
        s.item(a, rev.pr)
            .unwrap()
            .unwrap()
            .pull_request
            .unwrap()
            .fields
            .merged,
        Change::Value(false)
    );
    let mut wrong = rev.clone();
    wrong.pr = issue;
    assert!(
        s.submit_merge(
            &wrong,
            MergeSubmission {
                id: &s.new_operation_id().unwrap(),
                method: MergeMethod::Squash,
                dependencies: &[],
                submitted_at_ms: 34
            }
        )
        .is_err()
    );
    drop(s);
    let s = Store::open_profile(dir.path()).unwrap();
    s.set_dispatch_controls(a, false, false).unwrap();
    assert!(s.claim_write(a, 44, 100).unwrap().is_none());
    assert!(s.claim_review(a, 44, 100).unwrap().is_none());
    assert!(s.claim_next(a, 44, 100).unwrap().is_none());
    let claim = s.claim_merge(a, 44, 100).unwrap().unwrap();
    assert_eq!(claim.method, MergeMethod::Squash);
    assert_eq!(claim.route.head, rev.head.as_str());
    let mut current = obs(&claim, 46);
    current.policy.queue = QueueRequirement::Required;
    assert!(matches!(
        s.authorize_merge(&claim, &current, 46).unwrap(),
        MergeAdmission::Held("required_merge_queue_unsupported")
    ));
    assert_eq!(
        s.operation(a, &op).unwrap().unwrap().state,
        State::NeedsAttention
    );
    assert!(s.next_merge_due(a, 200).unwrap().is_none());
    assert!(s.receipts(a, &op).unwrap().is_empty());
}
#[test]
fn it_014_changed_head_or_rejected_queue_never_retargets_or_retries_a_different_method() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (a, r, _) = setup(&s);
    let pr = revision(&s, a, r, 72, 'b', 20);
    let op = submit(&s, &pr, MergeMethod::Rebase);
    let c = s.claim_merge(a, 32, 100).unwrap().unwrap();
    let t = ticket(&s, &c, 33);
    let replacement = revision(&s, a, r, 72, 'e', 34);
    assert!(!s.start_merge_transport(&t, 35).unwrap());
    assert_eq!(
        s.operation(a, &op).unwrap().unwrap().reason.as_deref(),
        Some("expected_merge_revision_changed")
    );
    s.cancel_operation(a, &op).unwrap();
    let second = submit(&s, &replacement, MergeMethod::Merge);
    let c = s.claim_merge(a, 40, 500).unwrap().unwrap();
    let t = ticket(&s, &c, 41);
    assert!(s.start_merge_transport(&t, 41).unwrap());
    assert!(!s.start_merge_transport(&t, 42).unwrap());
    s.record_merge_failure(
        a,
        &second,
        c.attempt(),
        MergeFailure::RequiredMergeQueue,
        43,
    )
    .unwrap();
    let held = s.operation(a, &second).unwrap().unwrap();
    assert_eq!(held.state, State::NeedsAttention);
    assert_eq!(
        held.reason.as_deref(),
        Some("required_merge_queue_unsupported")
    );
    assert!(s.claim_merge(a, 10_000, 20_000).unwrap().is_none());
}
#[test]
fn it_009_it_014_uncertain_merge_only_reads_back_original_pr_never_replays_or_claims_an_external_merge()
 {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (a, r, _) = setup(&s);
    let pr = revision(&s, a, r, 72, 'b', 20);
    let op = submit(&s, &pr, MergeMethod::Merge);
    let c = s.claim_merge(a, 32, 100).unwrap().unwrap();
    let t = ticket(&s, &c, 33);
    assert!(s.start_merge_transport(&t, 33).unwrap());
    drop(s);
    let s = Store::open_profile(dir.path()).unwrap();
    assert_eq!(s.recover_expired(a, 150).unwrap(), 1);
    assert_eq!(
        s.operation(a, &op).unwrap().unwrap().state,
        State::OutcomeUnknown
    );
    assert_eq!(
        s.cancel_operation(a, &op).unwrap(),
        CancelResult::MayAlreadyBeSent
    );
    assert!(s.claim_merge(a, 2_000, 3_000).unwrap().is_none());
    let rb = s.claim_merge_readback(a, 2_000, 3_000).unwrap().unwrap();
    assert_eq!(s.merge_readback_profile(&rb).unwrap().account, a);
    s.defer_merge_readback(&rb, 8_000).unwrap();
    assert_eq!(s.next_merge_due(a, 3_001).unwrap(), Some(8_000));
    let mut wrong = merged(&c, 99);
    wrong.expected_head = oid('e').as_str().into();
    assert!(
        s.confirm_merge(a, &op, c.attempt(), MergeProof::MergedPrRead, &wrong, 4_000)
            .is_err()
    );
    assert!(s.confirmed_merge(a, &op).unwrap().is_none());
    s.confirm_merge(
        a,
        &op,
        c.attempt(),
        MergeProof::MergedPrRead,
        &merged(&c, 99),
        8_010,
    )
    .unwrap();
    let done = s.confirmed_merge(a, &op).unwrap().unwrap();
    assert!(!done.own_action);
    assert_eq!(done.merge_oid, oid('f'));
    assert_eq!(s.receipts(a, &op).unwrap().len(), 1);
    assert_eq!(
        s.operation(a, &op).unwrap().unwrap().reason.as_deref(),
        Some("merged_observed_origin_unproved")
    );
    assert_eq!(
        s.item(a, pr.pr)
            .unwrap()
            .unwrap()
            .pull_request
            .unwrap()
            .fields
            .merged,
        Change::Value(true)
    );
    assert!(s.next_merge_due(a, 9_000).unwrap().is_none());
}
#[test]
fn it_015_renewed_original_credential_can_resume_confirmed_own_merge_without_changing_scope() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (a, r, _) = setup(&s);
    let pr = revision(&s, a, r, 72, 'b', 20);
    let op = submit(&s, &pr, MergeMethod::Squash);
    let c = s.claim_merge(a, 31, 80).unwrap().unwrap();
    s.fail_merge_preflight(&c, WritePreflightFailure::Forbidden, 32)
        .unwrap();
    assert_eq!(s.resume_merges(a, 100).unwrap(), 0);
    let new = s
        .reserve_access_profile_credential(
            a,
            id(81),
            "reader",
            "work",
            CredentialKind::FineGrained,
            None,
        )
        .unwrap();
    s.install_current_credential(&new, || Ok::<_, ()>(()))
        .unwrap()
        .unwrap();
    s.record_access_result(
        &new,
        new.kind,
        AccessState::Readable,
        DiscoveryState::Complete,
        100,
    )
    .unwrap();
    s.observe_github_repository(
        &new,
        &RepositoryObservation {
            provider_id: id(200),
            owner: "work",
            name: "main",
            private: true,
            source: RepositoryObservationSource::Direct,
        },
        101,
    )
    .unwrap();
    assert_eq!(s.resume_merges(a, 102).unwrap(), 1);
    let c = s.claim_merge(a, 103, 1_000).unwrap().unwrap();
    let t = ticket(&s, &c, 104);
    assert!(s.start_merge_transport(&t, 104).unwrap());
    s.accept_merge(a, &op, c.attempt(), "merge-request", Some(&oid('f')))
        .unwrap();
    assert!(s.confirmed_merge(a, &op).unwrap().is_none());
    assert!(
        s.record_merge_failure(a, &op, c.attempt(), MergeFailure::DefinitelyUnsent, 106)
            .is_err()
    );
    let r = s.confirm_merge(
        a,
        &op,
        c.attempt(),
        MergeProof::MergedPrRead,
        &merged(&c, 81),
        1_200,
    );
    r.unwrap();
    assert!(s.confirmed_merge(a, &op).unwrap().unwrap().own_action);
}

#[test]
fn it_014_base_branch_advance_uses_fresh_checks_but_an_explicit_retarget_requires_new_consent() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (a, r, _) = setup(&s);
    let pr = revision(&s, a, r, 72, 'b', 20);
    let op = submit(&s, &pr, MergeMethod::Squash);
    let c = s.claim_merge(a, 30, 100).unwrap().unwrap();
    assert_eq!(c.expected_base_ref.as_deref(), Some("main"));
    let mut o = obs(&c, 32);
    o.base = oid('e').as_str().into();
    let next = s
        .record_pr_revision(
            a,
            pr.pr,
            &oid('e'),
            &pr.head,
            Some(&oid('c')),
            &PrHeader {
                base_ref: Some("main".into()),
                ..Default::default()
            },
            31,
        )
        .unwrap();
    assert_ne!(next.id, pr.id);
    let MergeAdmission::Send(ticket) = s.authorize_merge(&c, &o, 32).unwrap() else {
        panic!("freshly checked same branch")
    };
    assert!(s.start_merge_transport(&ticket, 33).unwrap());
    let actual = merged(&c, 81);
    s.confirm_merge(
        a,
        &op,
        c.attempt(),
        MergeProof::MutationResponse {
            request_id: "direct-merge",
        },
        &actual,
        35,
    )
    .unwrap();
    assert!(s.confirmed_merge(a, &op).unwrap().unwrap().own_action);

    let another = revision(&s, a, r, 73, 'd', 1100);
    let _second = submit(&s, &another, MergeMethod::Squash);
    let c = s.claim_merge(a, 1110, 1250).unwrap().unwrap();
    let mut observed = obs(&c, 1111);
    observed.base_ref = Some("release".into());
    assert!(matches!(
        s.authorize_merge(&c, &observed, 1111).unwrap(),
        MergeAdmission::Held("merge_target_branch_changed")
    ));
}

#[test]
fn it_015_two_wal_connections_cannot_send_review_and_merge_concurrently() {
    use std::sync::{Arc, Barrier};
    use threadline_storage::reviews::*;
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (a, r, _) = setup(&s);
    let first = revision(&s, a, r, 72, 'b', 20);
    let second = revision(&s, a, r, 73, 'e', 21);
    let merge = submit(&s, &first, MergeMethod::Merge);
    let review = s
        .submit_review(
            &second,
            ReviewSubmission {
                id: &s.new_operation_id().unwrap(),
                intent: &ReviewIntent::Review(ReviewDraft {
                    event: ReviewEvent::Comment,
                    summary: "review".into(),
                    comments: vec![],
                }),
                draft: None,
                dependencies: &[],
                submitted_at_ms: 25,
            },
        )
        .unwrap();
    let mc = s.claim_merge(a, 30, 300).unwrap().unwrap();
    let mt = ticket(&s, &mc, 31);
    let rc = s.claim_review(a, 30, 300).unwrap().unwrap();
    let rr = &rc.route;
    let o = ReviewObservation {
        actor: rr.actor,
        repository_id: rr.repository_id,
        pr_id: rr.pr_id,
        number: rr.number,
        credential_generation: rr.credential_generation,
        observed_at_ms: 31,
        base: rr.base.clone(),
        head: rr.head.clone(),
        permission: Permission::Allowed,
        pr_open: Some(true),
        pr_author: Some(id(99)),
        pending: RemotePending::Absent,
        thread: None,
    };
    let ReviewAdmission::Send(rt) = s.authorize_review(&rc, &o, 31).unwrap() else {
        panic!("expected review admission")
    };
    let other = Store::open_profile(dir.path()).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let b = barrier.clone();
    let (m, v) = std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            b.wait();
            other.start_review_transport(&rt, 32).unwrap()
        });
        barrier.wait();
        (
            s.start_merge_transport(&mt, 32).unwrap(),
            worker.join().unwrap(),
        )
    });
    assert_ne!(m, v);
    let expected = if m {
        [State::Sending, State::Queued]
    } else {
        [State::Queued, State::Sending]
    };
    assert_eq!(
        [
            s.operation(a, &merge).unwrap().unwrap().state,
            s.operation(a, &review).unwrap().unwrap().state
        ],
        expected
    );
}
