use threadline_storage::{queue::*, writes::*, *};

use threadline_storage as storage;
#[path = "support/write_profile.rs"]
mod write_profile;
use write_profile::{id, issue, setup};

fn submit(db: &Store, a: AccountId, item: ItemId, intent: &WriteIntent, at: i64) -> String {
    let uuid = db.new_operation_id().unwrap();
    db.submit_write(
        a,
        WriteSubmission {
            id: &uuid,
            item,
            intent,
            draft: None,
            dependencies: &[],
            submitted_at_ms: at,
        },
    )
    .unwrap()
}
fn obs(claim: &WriteClaim, at: i64) -> WriteObservation {
    let r = &claim.route;
    WriteObservation {
        credential_generation: r.credential_generation,
        actor: r.actor,
        repository: r.repository_id,
        issue: r.issue_id,
        number: r.number,
        pull_request: r.pull_request,
        observed_at_ms: at,
        permission: Permission::Allowed,
        fields: IssueValues {
            title: Some("old".into()),
            body: Some(Some("description".into())),
            closed: Some(false),
            ..Default::default()
        },
        comment: None,
        choices: Default::default(),
    }
}
fn ticket(db: &Store, c: &WriteClaim, observed: &WriteObservation, now: i64) -> MutationTicket {
    let WriteAdmission::Send(ticket) = db.authorize_write(c, observed, now).unwrap() else {
        panic!("unexpected hold")
    };
    assert!(db.start_write_transport(&ticket, now).unwrap());
    assert!(!db.start_write_transport(&ticket, now).unwrap());
    ticket
}
fn remote(body: &str, when: i64) -> RemoteComment {
    RemoteComment {
        id: id(377),
        author: Some("reader".into()),
        author_id: Some(id(81)),
        body: body.into(),
        created_at_ms: 100,
        updated_at_ms: when,
    }
}
fn reply(claim: &WriteClaim, object: ConfirmedObject) -> ConfirmedWrite {
    let r = &claim.route;
    ConfirmedWrite {
        actor: r.actor,
        repository: r.repository_id,
        issue: r.issue_id,
        number: r.number,
        object,
    }
}
fn edit() -> WriteIntent {
    WriteIntent::EditIssue(IssueChanges {
        title: Some(FieldEdit {
            base: "old".into(),
            wanted: "ours".into(),
        }),
        ..Default::default()
    })
}
#[test]
fn it_008_offline_save_and_submit_are_distinct_and_duplicate_editors_share_one_operation() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, _, i) = setup(&db);
    let mut settings = db.sync_settings(a).unwrap();
    settings.controls.work_offline = true;
    db.save_sync_settings(a, &settings).unwrap();
    db.create_draft(
        a,
        "draft",
        i,
        DraftKind::Comment,
        &DraftContent {
            body: "hello".into(),
            base: "".into(),
            anchor: "ordinary".into(),
        },
    )
    .unwrap();
    assert!(db.queue_page(a, 0, 20).unwrap().is_empty());
    db.acquire_editor(a, "draft", "editor", 100, 1000).unwrap();
    let intent = WriteIntent::CreateComment {
        body: "hello".into(),
    };
    let first = db.new_operation_id().unwrap();
    let other = db.new_operation_id().unwrap();
    let go = |uuid: &str| {
        db.submit_write(
            a,
            WriteSubmission {
                id: uuid,
                item: i,
                intent: &intent,
                draft: Some(DraftSubmission {
                    id: "draft",
                    revision: 1,
                    editor: "editor",
                    now: 101,
                }),
                dependencies: &[],
                submitted_at_ms: 101,
            },
        )
    };
    assert_eq!(go(&first).unwrap(), first);
    assert_eq!(go(&other).unwrap(), first);
    assert_eq!(db.projections(a, i).unwrap().len(), 1);
    assert!(db.claim_write(a, 102, 200).unwrap().is_none());
    drop(db);
    let db = Store::open_profile(dir.path()).unwrap();
    assert_eq!(db.write_intent(a, &first).unwrap().unwrap(), intent);
    assert_eq!(db.draft(a, "draft").unwrap().unwrap().content.body, "hello");
    settings.controls.work_offline = false;
    settings.controls.pause_background = true;
    db.save_sync_settings(a, &settings).unwrap();
    let claim = db.claim_write(a, 105, 200).unwrap().unwrap();
    assert_eq!(claim.operation_id(), first);
    assert!(db.claim_next(a, 105, 200).unwrap().is_none());
}

#[test]
fn it_009_timeout_and_cancel_never_replay_identity_free_matches_and_accepted_creation_reconciles() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, _, item) = setup(&db);
    let op = submit(
        &db,
        a,
        item,
        &WriteIntent::CreateComment {
            body: "same text".into(),
        },
        10,
    );
    let c = db.claim_write(a, 100, 500).unwrap().unwrap();
    let o = obs(&c, 101);
    let _sent = ticket(&db, &c, &o, 102);
    db.record_write_failure(a, &op, c.attempt(), WriteFailure::Ambiguous, 110)
        .unwrap();
    assert_eq!(
        db.cancel_operation(a, &op).unwrap(),
        CancelResult::MayAlreadyBeSent
    );
    assert!(!db.retry_write_now(a, &op, 2_000).unwrap());
    let body = reply(&c, ConfirmedObject::Comment(remote("same text", 120)));
    assert!(
        db.confirm_write(
            a,
            &op,
            c.attempt(),
            ConfirmationProof::AcceptedCommentRead,
            &body,
            2_001
        )
        .is_err()
    );
    assert!(db.claim_write(a, 2_002, 2_100).unwrap().is_none());
    assert_eq!(
        db.operation(a, &op).unwrap().unwrap().state,
        State::OutcomeUnknown
    );
    // A late positive mutation response still names the original in-flight attempt after cancellation.
    db.confirm_write(
        a,
        &op,
        c.attempt(),
        ConfirmationProof::MutationResponse {
            request_id: "REQ-1",
        },
        &body,
        2_003,
    )
    .unwrap();
    assert!(db.projections(a, item).unwrap().is_empty());
    assert_eq!(
        db.own_write(a, &op).unwrap().unwrap().action,
        "comment_created"
    );
    let second = submit(
        &db,
        a,
        item,
        &WriteIntent::CreateComment {
            body: "accepted".into(),
        },
        2_004,
    );
    let c = db.claim_write(a, 2_010, 2_500).unwrap().unwrap();
    let t = ticket(&db, &c, &obs(&c, 2_011), 2_012);
    db.accept_created_comment(&t, id(378), "REQ-2").unwrap();
    drop(db);
    let db = Store::open_profile(dir.path()).unwrap();
    let pending = db.write_reconciliations(a, 20).unwrap();
    assert_eq!(pending[0].comment_id, id(378));
    assert_eq!(db.reconciliation_profile(&pending[0]).unwrap().account, a);
    let mut wrong = pending[0].clone();
    wrong.comment_id = id(399);
    assert!(db.reconciliation_profile(&wrong).is_err());
    assert!(
        db.record_write_failure(
            a,
            &second,
            c.attempt(),
            WriteFailure::DefinitelyUnsent,
            2_012
        )
        .is_err()
    );
    assert!(
        db.confirm_write(
            a,
            &second,
            c.attempt(),
            ConfirmationProof::MutationResponse {
                request_id: "wrong-object"
            },
            &reply(&c, ConfirmedObject::Comment(remote("accepted", 2_013))),
            2_014
        )
        .is_err()
    );
    let mut observed = remote("accepted", 2_013);
    observed.id = id(378);
    db.confirm_write(
        a,
        &second,
        c.attempt(),
        ConfirmationProof::AcceptedCommentRead,
        &reply(&c, ConfirmedObject::Comment(observed)),
        2_014,
    )
    .unwrap();
    let page = db.discussion_page(a, item, None, 10).unwrap();
    assert_eq!(page.records.len(), 2);
    assert_eq!(
        page.state.coverage,
        threadline_storage::resources::Coverage::Partial
    );
}

#[test]
fn it_010_atomic_cancel_before_send_and_changed_credential_reject_stale_preflight() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, r, i) = setup(&db);
    let op = submit(&db, a, i, &edit(), 10);
    let c = db.claim_write(a, 30, 500).unwrap().unwrap();
    let o = obs(&c, 31);
    let other = Store::open_profile(dir.path()).unwrap();
    other.cancel_operation(a, &op).unwrap();
    assert!(db.authorize_write(&c, &o, 32).is_err());
    assert_eq!(
        db.operation(a, &op).unwrap().unwrap().state,
        State::Cancelled
    );
    let op = submit(&db, a, i, &edit(), 35);
    let c = db.claim_write(a, 36, 500).unwrap().unwrap();
    let o = obs(&c, 37);
    db.select_github_repository(a, r, false).unwrap();
    assert!(db.authorize_write(&c, &o, 38).is_err());
    assert!(db.own_write(a, &op).unwrap().is_none());
}

#[test]
fn it_012_conflicting_fields_retain_remote_local_original_and_an_unrelated_issue_can_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, r, i) = setup(&db);
    let op = submit(&db, a, i, &edit(), 40);
    let _dependent = submit(&db, a, i, &edit(), 41);
    let unrelated = issue(&db, a, r, 77);
    let other = submit(
        &db,
        a,
        unrelated,
        &WriteIntent::CreateComment {
            body: "unrelated".into(),
        },
        43,
    );
    let c = db.claim_write(a, 50, 500).unwrap().unwrap();
    let mut changed = obs(&c, 51);
    changed.fields.title = Some("remote change".into());
    assert!(matches!(
        db.authorize_write(&c, &changed, 52).unwrap(),
        WriteAdmission::NeedsAttention("issue_fields_conflict")
    ));
    let info = db.write_conflict(a, &op).unwrap().unwrap();
    assert_eq!(info.intended, edit());
    assert_eq!(
        info.current.unwrap().fields.title.as_deref(),
        Some("remote change")
    );
    assert!(!db.retry_write_now(a, &op, 60).unwrap());
    assert_eq!(
        db.claim_write(a, 60, 500).unwrap().unwrap().operation_id(),
        other
    );
}

#[test]
fn it_011_pending_comment_edit_uses_confirmed_predecessor_identity_and_updates_one_cached_object() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, _, i) = setup(&db);
    let original = submit(
        &db,
        a,
        i,
        &WriteIntent::CreateComment { body: "one".into() },
        70,
    );
    let update = submit(
        &db,
        a,
        i,
        &WriteIntent::EditComment {
            target: CommentTarget::CreatedBy(original.clone()),
            body: FieldEdit {
                base: "one".into(),
                wanted: "two".into(),
            },
        },
        71,
    );
    let a1 = db.claim_write(a, 100, 500).unwrap().unwrap();
    ticket(&db, &a1, &obs(&a1, 101), 102);
    assert!(db.claim_write(a, 105, 500).unwrap().is_none());
    db.confirm_write(
        a,
        &original,
        a1.attempt(),
        ConfirmationProof::MutationResponse { request_id: "Q-1" },
        &reply(&a1, ConfirmedObject::Comment(remote("one", 103))),
        104,
    )
    .unwrap();
    drop(db);
    let db = Store::open_profile(dir.path()).unwrap();
    let preserved = db.receipts(a, &original).unwrap();
    assert_eq!(preserved.len(), 1);
    assert_eq!(preserved[0].remote_id, "377");
    assert!(db.claim_write(a, 1_100, 1_500).unwrap().is_none());
    let a2 = db.claim_write(a, 1_102, 1_500).unwrap().unwrap();
    assert_eq!(a2.comment_id, Some(id(377)));
    let mut o = obs(&a2, 1_103);
    o.comment = Some(remote("one", 103));
    ticket(&db, &a2, &o, 1_104);
    db.confirm_write(
        a,
        &update,
        a2.attempt(),
        ConfirmationProof::MutationResponse { request_id: "Q-2" },
        &reply(&a2, ConfirmedObject::Comment(remote("two", 1_105))),
        1_106,
    )
    .unwrap();
    let page = db.discussion_page(a, i, None, 10).unwrap();
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].body, "two");
    assert_eq!(
        db.own_write(a, &update).unwrap().unwrap().action,
        "comment_edited"
    );
}

#[test]
fn it_012_confirmed_issue_fields_and_repository_choices_preserve_unknown_unedited_values() {
    use std::collections::BTreeSet;
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, _, i) = setup(&db);
    let new = BTreeSet::from(["confirmed-label".to_owned()]);
    let change = WriteIntent::EditIssue(IssueChanges {
        title: Some(FieldEdit {
            base: "old".into(),
            wanted: "updated".into(),
        }),
        labels: Some(FieldEdit {
            base: BTreeSet::new(),
            wanted: new.clone(),
        }),
        milestone: Some(FieldEdit {
            base: None,
            wanted: Some(id(23)),
        }),
        body: Some(FieldEdit {
            base: Some("description".into()),
            wanted: None,
        }),
        ..Default::default()
    });
    let op = submit(&db, a, i, &change, 1);
    let c = db.claim_write(a, 2, 100).unwrap().unwrap();
    let mut evidence = obs(&c, 3);
    evidence.fields.labels = Some(BTreeSet::new());
    evidence.fields.milestone = Some(None);
    assert!(matches!(
        db.authorize_write(&c, &evidence, 4).unwrap(),
        WriteAdmission::NeedsAttention("repository_choice_unavailable")
    ));
    assert!(!db.retry_write_now(a, &op, 5).unwrap());
    db.cancel_operation(a, &op).unwrap();
    let op = submit(&db, a, i, &change, 6);
    let c = db.claim_write(a, 7, 100).unwrap().unwrap();
    evidence.observed_at_ms = 8;
    evidence.choices.labels = Some(new.clone());
    evidence.choices.milestones = Some(BTreeSet::from([id(23)]));
    ticket(&db, &c, &evidence, 9);
    let current = IssueValues {
        title: Some("updated".into()),
        body: Some(None),
        labels: Some(new),
        milestone: Some(Some(id(23))),
        ..Default::default()
    };
    let response = reply(
        &c,
        ConfirmedObject::Issue {
            fields: current.clone(),
            milestone_title: Some("Milestone 23".into()),
            updated_at_ms: 50,
        },
    );
    db.confirm_write(
        a,
        &op,
        c.attempt(),
        ConfirmationProof::MutationResponse { request_id: "R-7" },
        &response,
        51,
    )
    .unwrap();
    let known = db.reader_item_metadata(a, i).unwrap().unwrap();
    assert_eq!(known.title, Change::Value("updated".into()));
    assert_eq!(known.body, Change::Null);
    assert_eq!(known.milestone, Change::Value("Milestone 23".into()));
    assert_eq!(
        known.labels.as_deref(),
        Some(["confirmed-label".into()].as_slice())
    );
    assert!(known.assignees.is_none());
    assert_eq!(
        db.own_write(a, &op).unwrap().unwrap().action,
        "issue_edited"
    );
}

#[test]
fn it_010_racing_abort_and_dispatch_across_wal_connections_has_one_durable_winner() {
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, _, i) = setup(&db);
    for cycle in 0..6 {
        let now = 1_000 + 2_000 * cycle;
        let op = submit(
            &db,
            a,
            i,
            &WriteIntent::CreateComment {
                body: "race".into(),
            },
            now,
        );
        let c = db.claim_write(a, now + 1, now + 200).unwrap().unwrap();
        let o = obs(&c, now + 2);
        let WriteAdmission::Send(prepared) = db.authorize_write(&c, &o, now + 2).unwrap() else {
            panic!("preflight held")
        };
        let barrier = Arc::new(Barrier::new(2));
        let cancel_barrier = barrier.clone();
        let second = Store::open_profile(dir.path()).unwrap();
        let (admission, cancel) = std::thread::scope(|scope| {
            let handle = scope.spawn(|| {
                cancel_barrier.wait();
                second.cancel_operation(a, &op).unwrap()
            });
            barrier.wait();
            (
                db.start_write_transport(&prepared, now + 3).unwrap(),
                handle.join().unwrap(),
            )
        });
        match cancel {
            CancelResult::Cancelled => {
                assert!(!admission);
                assert_eq!(
                    db.operation(a, &op).unwrap().unwrap().state,
                    State::Cancelled
                );
            }
            CancelResult::MayAlreadyBeSent => {
                assert!(admission);
                assert!(db.operation(a, &op).unwrap().unwrap().cancel_requested);
                db.record_write_failure(
                    a,
                    &op,
                    c.attempt(),
                    WriteFailure::DefinitelyUnsent,
                    now + 4,
                )
                .unwrap();
                assert_eq!(
                    db.operation(a, &op).unwrap().unwrap().state,
                    State::Cancelled
                );
            }
            CancelResult::AlreadyConfirmed => panic!("the test sent no network request"),
        }
    }
}

#[test]
fn it_009_throttle_survives_restart_and_unknown_work_cannot_skip_the_floor_or_resume() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, _, i) = setup(&db);
    let op = submit(
        &db,
        a,
        i,
        &WriteIntent::CreateComment {
            body: "throttle".into(),
        },
        5,
    );
    let c = db.claim_write(a, 10, 100).unwrap().unwrap();
    ticket(&db, &c, &obs(&c, 11), 12);
    db.record_write_failure(
        a,
        &op,
        c.attempt(),
        WriteFailure::Throttled {
            not_before_ms: 10_000,
        },
        15,
    )
    .unwrap();
    drop(db);
    let db = Store::open_profile(dir.path()).unwrap();
    assert!(db.retry_write_now(a, &op, 20).unwrap());
    assert_eq!(db.next_write_due(a, 20).unwrap(), Some(10_000));
    assert!(db.claim_write(a, 9_999, 15_000).unwrap().is_none());
    let c = db.claim_write(a, 10_000, 12_000).unwrap().unwrap();
    ticket(&db, &c, &obs(&c, 10_001), 10_002);
    db.recover_expired(a, 12_001).unwrap();
    assert_eq!(
        db.operation(a, &op).unwrap().unwrap().state,
        State::OutcomeUnknown
    );
    assert!(!db.retry_write_now(a, &op, 12_002).unwrap());
    assert_eq!(db.resume_writes(a, 12_003).unwrap(), 0);
    assert!(db.claim_write(a, 14_000, 20_000).unwrap().is_none());
}

#[test]
fn it_012_cached_repository_options_and_numeric_edit_base_survive_offline_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, r, item) = setup(&db);
    let p = db.github_access_profile(a, "work").unwrap().unwrap();
    assert_eq!(
        db.issue_write_base(a, item).unwrap().unwrap().milestone,
        None
    );
    let labels = |names: &[&str], complete, checked_at_ms| ChoiceList {
        values: names.iter().map(|n| n.to_string()).collect(),
        complete,
        checked_at_ms,
    };
    let mut choices = CachedRepositoryChoices {
        labels: Some(labels(&["bug"], false, 100)),
        assignees: Some(labels(&[], true, 100)),
        milestones: Some(ChoiceList {
            values: vec![MilestoneChoice {
                number: id(23),
                title: "Winter".into(),
            }],
            complete: true,
            checked_at_ms: 100,
        }),
    };
    db.record_write_choices(&p, r, &choices).unwrap();
    choices.labels = Some(labels(&["bug", "triage"], true, 100));
    db.record_write_choices(&p, r, &choices).unwrap();
    db.record_write_choices(
        &p,
        r,
        &CachedRepositoryChoices {
            labels: Some(labels(&[], true, 99)),
            ..Default::default()
        },
    )
    .unwrap();
    let fields = IssueValues {
        milestone: Some(Some(id(23))),
        body: Some(None),
        ..Default::default()
    };
    let record = |issue_id, number, values: &IssueValues, title, stamp| {
        db.record_issue_write_base(&p, r, item, id(issue_id), id(number), values, title, stamp)
    };
    assert!(record(1041, 72, &fields, Some("Winter"), 110).is_err());
    assert!(record(100, 41, &fields, Some("Winter"), 110).is_err());
    record(1041, 41, &fields, Some("Winter"), 110).unwrap();
    let empty = IssueValues {
        milestone: Some(None),
        ..Default::default()
    };
    record(1041, 41, &empty, None, 109).unwrap();
    let snapshot = db.issue_write_base(a, item).unwrap().unwrap();
    assert_eq!(snapshot.milestone, Some(Some(id(23))));
    assert_eq!(snapshot.body, Some(None));
    assert_eq!(snapshot.title.as_deref(), Some("old"));
    record(1041, 41, &empty, None, 111).unwrap();
    db.select_github_repository(a, r, false).unwrap();
    assert!(db.record_write_choices(&p, r, &choices).is_err());
    assert!(record(1041, 41, &fields, Some("Winter"), 120).is_err());
    drop(db);
    let reopened = Store::open_profile(dir.path()).unwrap();
    assert_eq!(reopened.write_choices(a, r).unwrap(), choices);
    assert_eq!(
        reopened
            .issue_write_base(a, item)
            .unwrap()
            .unwrap()
            .milestone,
        Some(None)
    );
}

#[test]
fn it_011_cached_author_proof_is_bound_to_original_comment_and_cannot_change_actors() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, r, item) = setup(&db);
    let other = issue(&db, a, r, 42);
    let p = db.github_access_profile(a, "work").unwrap().unwrap();
    let proof = CommentAuthorObservation {
        comment_id: id(377),
        author_id: id(81),
        observed_at_ms: 120,
    };
    assert!(db.record_comment_authors(&p, r, item, &[proof]).is_err());
    assert_eq!(db.comment_author_id(a, item, id(377)).unwrap(), None);
    submit(
        &db,
        a,
        item,
        &WriteIntent::CreateComment { body: "one".into() },
        100,
    );
    let claim = db.claim_write(a, 101, 600).unwrap().unwrap();
    ticket(&db, &claim, &obs(&claim, 101), 101);
    db.confirm_write(
        a,
        claim.operation_id(),
        claim.attempt(),
        ConfirmationProof::MutationResponse {
            request_id: "actual-create",
        },
        &reply(&claim, ConfirmedObject::Comment(remote("one", 103))),
        104,
    )
    .unwrap();
    assert_eq!(
        db.comment_author_id(a, item, id(377)).unwrap(),
        Some(id(81))
    );
    db.record_comment_authors(&p, r, item, &[proof]).unwrap();
    assert!(db.record_comment_authors(&p, r, other, &[proof]).is_err());
    assert!(
        db.record_comment_authors(
            &p,
            r,
            item,
            &[CommentAuthorObservation {
                author_id: id(99),
                ..proof
            }]
        )
        .is_err()
    );
    drop(db);
    let reopened = Store::open_profile(dir.path()).unwrap();
    assert_eq!(
        reopened.comment_author_id(a, item, id(377)).unwrap(),
        Some(id(81))
    );
    assert_eq!(reopened.comment_author_id(a, other, id(377)).unwrap(), None);
}

#[test]
fn it_010_interrupted_preflight_releases_only_the_exact_unsent_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, r, item) = setup(&db);
    submit(&db, a, item, &edit(), 5);
    let claim = db.claim_write(a, 6, 10000).unwrap().unwrap();
    db.set_repository_paused(a, r, true).unwrap();
    assert!(db.abandon_write_preflight(&claim, 7).unwrap());
    assert!(db.claim_write(a, 8, 10000).unwrap().is_none());
    db.set_repository_paused(a, r, false).unwrap();
    let next = db.claim_write(a, 9, 10000).unwrap().unwrap();
    assert!(!db.abandon_write_preflight(&claim, 10).unwrap());
    ticket(&db, &next, &obs(&next, 11), 11);
    assert!(!db.abandon_write_preflight(&next, 12).unwrap());
}

#[test]
fn it_009_unreadable_accepted_comment_cannot_starve_another_item_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, r, first) = setup(&db);
    let second = issue(&db, a, r, 42);
    for (item, at, remote) in [(first, 100, 601), (second, 1_200, 602)] {
        submit(
            &db,
            a,
            item,
            &WriteIntent::CreateComment {
                body: "same".into(),
            },
            at,
        );
        let claim = db.claim_write(a, at + 1, at + 900).unwrap().unwrap();
        let t = ticket(&db, &claim, &obs(&claim, at + 2), at + 2);
        db.accept_created_comment(&t, id(remote), "created")
            .unwrap();
    }
    assert!(
        db.claim_write_reconciliation(a, 1_205, 2_500)
            .unwrap()
            .is_none()
    );
    let target = db
        .claim_write_reconciliation(a, 2_300, 3_300)
        .unwrap()
        .unwrap();
    assert_eq!(target.comment_id, id(601));
    assert!(db.defer_write_reconciliation(&target, 62_300).unwrap());
    drop(db);
    let db = Store::open_profile(dir.path()).unwrap();
    assert_eq!(db.next_write_due(a, 2_301).unwrap(), Some(2_301));
    let ready = db
        .claim_write_reconciliation(a, 2_301, 3_301)
        .unwrap()
        .unwrap();
    assert_eq!(ready.comment_id, id(602));
    let mut comment = remote("same", 2_302);
    comment.id = ready.comment_id;
    db.confirm_write(
        a,
        &ready.operation_id,
        ready.attempt,
        ConfirmationProof::AcceptedCommentRead,
        &ConfirmedWrite {
            actor: ready.route.actor,
            repository: ready.route.repository_id,
            issue: ready.route.issue_id,
            number: ready.route.number,
            object: ConfirmedObject::Comment(comment),
        },
        2_303,
    )
    .unwrap();
    assert!(!db.defer_write_reconciliation(&ready, 70_000).unwrap());
    assert!(
        db.claim_write_reconciliation(a, 50_000, 51_000)
            .unwrap()
            .is_none()
    );
    assert_eq!(db.next_write_due(a, 50_000).unwrap(), Some(62_300));
    assert!(db.claim_write(a, 62_300, 63_300).unwrap().is_none());
    db.set_repository_paused(a, r, true).unwrap();
    assert!(db.next_write_due(a, 70_000).unwrap().is_none());
    db.set_repository_paused(a, r, false).unwrap();
    assert_eq!(
        db.claim_write_reconciliation(a, 70_000, 71_000)
            .unwrap()
            .unwrap()
            .comment_id,
        id(601)
    );
}
