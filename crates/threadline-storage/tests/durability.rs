use rusqlite::{Connection, params};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
    thread,
};
use tempfile::TempDir;
use threadline_storage::{
    AccountId, Change, IssueClassification, IssueRecord, ItemId, ItemLocator, ItemRepresentation,
    ItemUpdate, PullRequestRecord, RepositoryId, Store, UtcMillis, queue::*,
};

struct Fixture {
    _dir: TempDir,
    path: PathBuf,
    store: Store,
    account: AccountId,
    repo: RepositoryId,
    issue: ItemId,
    pr: ItemId,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.sqlite3");
        let store = Store::open(&path).unwrap();
        let account = store.ensure_account("github.com", 41).unwrap();
        let repo = store
            .ensure_repository(account, 1, "fixture", "one")
            .unwrap();
        let issue = item(&store, account, repo, 41, false);
        let pr = item(&store, account, repo, 72, true);
        store.authorize_profile(account, "github.com", 41).unwrap();
        Self {
            _dir: dir,
            path,
            store,
            account,
            repo,
            issue,
            pr,
        }
    }
}
fn item(
    store: &Store,
    account: AccountId,
    repository: RepositoryId,
    number: u64,
    pr: bool,
) -> ItemId {
    let representation = if pr {
        ItemRepresentation::PullRequest(PullRequestRecord {
            id: number.try_into().unwrap(),
            node_id: None,
            details: Default::default(),
        })
    } else {
        ItemRepresentation::Issue(IssueRecord {
            id: number.try_into().unwrap(),
            node_id: None,
            classification: IssueClassification::Issue,
        })
    };
    store
        .ingest(
            account,
            ItemLocator { repository, number },
            ItemUpdate {
                representation,
                remote_updated_at: UtcMillis(1),
                title: Change::Value(format!("Title {number}")),
                body: Change::Missing,
                closed: Change::Missing,
            },
        )
        .unwrap()
}
fn raw(path: &Path) -> Connection {
    let db = Connection::open(path).unwrap();
    db.pragma_update(None, "foreign_keys", true).unwrap();
    db
}
const FIRST: &str = "10000000-0000-4000-8000-000000000001";
const SECOND: &str = "10000000-0000-4000-8000-000000000002";
const THIRD: &str = "10000000-0000-4000-8000-000000000003";
fn comment(id: &str, item: ItemId) -> Submission<'_> {
    Submission {
        id,
        item,
        kind: ActionKind::Comment,
        payload: "submitted body",
        base: "original base",
        anchor: "source anchor",
        projection: "pending comment",
        draft: None,
        dependencies: &[],
    }
}
fn draft() -> DraftContent {
    DraftContent {
        body: "my edit".into(),
        base: "issue text and revision".into(),
        anchor: "src/lib.rs:12 at abc123".into(),
    }
}
fn server(step: &str) -> Receipt {
    Receipt {
        step: step.into(),
        object_kind: "comment".into(),
        remote_id: "server-node".into(),
        evidence: "response with returned node ID".into(),
    }
}

#[test]
fn it_019_workspace_views_drafts_refs_and_preferences_survive_cache_cleanup_and_restart() {
    let f = Fixture::new();
    let s = &f.store;
    let a = f.account;
    for (id, target, kind) in [
        ("edit", f.issue, DraftKind::IssueEdit),
        ("review", f.pr, DraftKind::Review),
        ("comment", f.issue, DraftKind::Comment),
    ] {
        s.create_draft(a, id, target, kind, &draft()).unwrap();
    }
    assert!(
        s.create_draft(a, "invalid", f.pr, DraftKind::IssueEdit, &draft())
            .is_err()
    );
    assert!(
        s.create_draft(a, "invalid", f.issue, DraftKind::Review, &draft())
            .is_err()
    );
    assert!(
        s.enqueue(
            a,
            Submission {
                kind: ActionKind::Merge,
                ..comment(FIRST, f.issue)
            }
        )
        .is_err()
    );
    assert!(
        s.enqueue(
            a,
            Submission {
                kind: ActionKind::EditIssue,
                ..comment(FIRST, f.pr)
            }
        )
        .is_err()
    );
    let candidate = ReferenceCandidate {
        position: 12,
        raw: "#41".into(),
        provenance: "body".into(),
        verified: Some((f.issue, 100)),
    };
    s.replace_pr_references(a, f.pr, None, "r1", std::slice::from_ref(&candidate))
        .unwrap();
    let context = PrContext {
        selected_issue: Some(f.issue),
        body_revision: "r1".into(),
        provenance: "body".into(),
        ratio: 0.33,
    };
    let tab = |id: &str, offset, ratio| Tab {
        id: id.into(),
        item: f.pr,
        view: TabView {
            scroll_offset: offset,
            context: Some(PrContext {
                ratio,
                ..context.clone()
            }),
            ..Default::default()
        },
    };
    let mut ws = Workspace {
        focused_pane: 0,
        proportions: vec![0.5, 0.5],
        panes: vec![
            Pane {
                active: Some(0),
                tabs: vec![tab("A", 10, 0.3)],
            },
            Pane {
                active: Some(0),
                tabs: vec![tab("B", 42, 0.45)],
            },
        ],
    };
    s.save_workspace(a, &ws).unwrap();
    s.save_pr_context(a, f.pr, &context).unwrap();
    ws.panes.swap(0, 1);
    s.save_workspace(a, &ws).unwrap();
    s.set_preference(a, "read-pause", "true").unwrap();
    assert!(s.acquire_editor(a, "edit", "left", 10, 20).unwrap());
    assert!(!s.acquire_editor(a, "edit", "right", 10, 20).unwrap());
    let value = DraftContent {
        body: "changed".into(),
        ..draft()
    };
    assert_eq!(s.save_draft(a, "edit", "left", 11, 1, &value).unwrap(), 2);
    assert!(s.save_draft(a, "edit", "right", 11, 2, &value).is_err());
    assert!(s.save_draft(a, "edit", "left", 11, 1, &value).is_err());
    assert!(s.projections(a, f.issue).unwrap().is_empty());
    s.clear_cache(a).unwrap();
    let reopened = Store::open(&f.path).unwrap();
    assert_eq!(reopened.workspace(a).unwrap(), Some(ws.clone()));
    assert_eq!(
        reopened.preference(a, "read-pause").unwrap().as_deref(),
        Some("true")
    );
    assert_eq!(reopened.draft(a, "edit").unwrap().unwrap().content, value);
    assert_eq!(
        reopened.draft(a, "review").unwrap().unwrap().content,
        draft()
    );
    assert_eq!(
        reopened.draft(a, "comment").unwrap().unwrap().content,
        draft()
    );
    // A revised body keeps the same verified locator and provenance; disappearance clears only selection.
    reopened
        .replace_pr_references(
            a,
            f.pr,
            Some("r1"),
            "r2",
            &[ReferenceCandidate {
                position: 19,
                verified: None,
                ..candidate
            }],
        )
        .unwrap();
    assert_eq!(
        reopened
            .pr_context(a, f.pr)
            .unwrap()
            .unwrap()
            .selected_issue,
        Some(f.issue)
    );
    assert!(
        reopened
            .replace_pr_references(a, f.pr, Some("r1"), "stale", &[])
            .is_err()
    );
    reopened
        .replace_pr_references(a, f.pr, Some("r2"), "r3", &[])
        .unwrap();
    let saved = reopened.workspace(a).unwrap().unwrap();
    for (after, before) in saved.panes.iter().zip(&ws.panes) {
        assert_eq!(after.tabs[0].id, before.tabs[0].id);
        assert_eq!(
            after.tabs[0].view.scroll_offset,
            before.tabs[0].view.scroll_offset
        );
        let ctx = after.tabs[0].view.context.as_ref().unwrap();
        assert_eq!(ctx.selected_issue, None);
        assert_eq!(ctx.body_revision, "r3");
        assert_eq!(
            ctx.ratio,
            before.tabs[0].view.context.as_ref().unwrap().ratio
        );
    }
    assert_eq!(reopened.draft(a, "review").unwrap().unwrap().item, f.pr);
}

#[test]
fn sqlite_and_workspace_reads_reject_cross_account_targets() {
    let f = Fixture::new();
    let s = &f.store;
    let other = s.ensure_account("github.com", 2).unwrap();
    let repo = s.ensure_repository(other, 5, "fixture", "private").unwrap();
    let private = item(s, other, repo, 6, false);
    let invalid = Workspace {
        focused_pane: 0,
        proportions: vec![1.0],
        panes: vec![Pane {
            active: Some(0),
            tabs: vec![Tab {
                id: "A".into(),
                item: private,
                view: Default::default(),
            }],
        }],
    };
    assert!(s.save_workspace(f.account, &invalid).is_err());
    assert!(s.enqueue(f.account, comment(FIRST, private)).is_err());
    assert!(s.set_repository_paused(f.account, repo, true).is_err());
    let db = raw(&f.path);
    db.execute(
        "INSERT INTO workspaces VALUES(?,?)",
        params![f.account, serde_json::to_string(&invalid).unwrap()],
    )
    .unwrap();
    assert!(s.workspace(f.account).is_err());
    assert!(db.execute("INSERT INTO drafts(account_id,id,item_id,kind,body,base,anchor,revision) VALUES(?,'private',?,'comment','','','',1)",params![f.account,private]).is_err());
    assert!(db.execute("INSERT INTO operations(account_id,id,item_id,original_repository,kind,payload_version,snapshot,projection)
        VALUES(?,'foreign',?,?,'comment',2,'{}','')",params![f.account,f.issue,repo]).is_err());
}

#[test]
fn ut_009_atomic_submission_dependencies_and_edits_after_enqueue() {
    let f = Fixture::new();
    let s = &f.store;
    let a = f.account;
    s.create_draft(a, "note", f.issue, DraftKind::Comment, &draft())
        .unwrap();
    s.acquire_editor(a, "note", "A", 0, 100).unwrap();
    let submission = Submission {
        draft: Some(DraftSubmission {
            id: "note",
            revision: 1,
            editor: "A",
            now: 10,
        }),
        ..comment(FIRST, f.issue)
    };
    assert!(
        s.enqueue(
            a,
            Submission {
                dependencies: &[SECOND],
                ..submission
            }
        )
        .is_err()
    );
    assert!(s.projections(a, f.issue).unwrap().is_empty());
    assert_eq!(s.draft(a, "note").unwrap().unwrap().content, draft());
    s.enqueue(a, submission).unwrap();
    s.enqueue(a, submission).unwrap();
    assert!(
        s.enqueue(
            a,
            Submission {
                id: SECOND,
                ..submission
            }
        )
        .is_err()
    );
    s.save_draft(
        a,
        "note",
        "A",
        20,
        1,
        &DraftContent {
            body: "later edit".into(),
            ..draft()
        },
    )
    .unwrap();
    assert_eq!(
        s.operation(a, FIRST).unwrap().unwrap().snapshot.draft,
        Some(draft())
    );
    s.enqueue(
        a,
        Submission {
            dependencies: &[FIRST],
            ..comment(SECOND, f.pr)
        },
    )
    .unwrap();
    assert_eq!(s.dependents(a, FIRST).unwrap(), vec![SECOND]);
    assert_eq!(
        s.dispatch_hold(a, SECOND).unwrap().as_deref(),
        Some("dependency")
    );
    assert_eq!(
        s.cancel_operation(a, FIRST).unwrap(),
        CancelResult::Cancelled
    );
    assert!(s.projections(a, f.issue).unwrap().is_empty());
    assert_eq!(s.projections(a, f.pr).unwrap().len(), 1);
    assert!(s.claim_next(a, 1, 2).unwrap().is_none());
    assert_eq!(
        s.draft(a, "note").unwrap().unwrap().content.body,
        "later edit"
    );
}

#[test]
fn ut_009_claim_and_cancel_compete_across_connections() {
    let f = Fixture::new();
    let a = f.account;
    f.store.enqueue(a, comment(FIRST, f.issue)).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let claims: Vec<_> = thread::scope(|scope| {
        (0..2)
            .map(|_| {
                let b = barrier.clone();
                let p = f.path.clone();
                scope.spawn(move || {
                    let s = Store::open(&p).unwrap();
                    b.wait();
                    s.claim_next(a, 0, 100).unwrap()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|t| t.join().unwrap())
            .collect()
    });
    assert_eq!(claims.len(), 1);
    let claim = &claims[0];
    let (sync1, sync2) = (barrier.clone(), barrier);
    let (sent, cancelled) = thread::scope(|scope| {
        let send = scope.spawn(|| {
            let s = Store::open(&f.path).unwrap();
            sync1.wait();
            s.begin_send(claim, "comment", 2).is_ok()
        });
        let cancel = scope.spawn(|| {
            let s = Store::open(&f.path).unwrap();
            sync2.wait();
            s.cancel_operation(a, FIRST).unwrap()
        });
        (send.join().unwrap(), cancel.join().unwrap())
    });
    if sent {
        assert_eq!(cancelled, CancelResult::MayAlreadyBeSent);
        f.store
            .record_delivery(
                a,
                FIRST,
                claim.number,
                Delivery::NotApplied {
                    proof: "verified no remote effect",
                    retry_at: 5,
                },
            )
            .unwrap();
    } else {
        assert_eq!(cancelled, CancelResult::Cancelled);
    }
    assert_eq!(
        f.store.operation(a, FIRST).unwrap().unwrap().state,
        State::Cancelled
    );
    assert!(f.store.claim_next(a, 1000, 1001).unwrap().is_none());
}

#[test]
fn ut_009_recovery_does_not_blindly_replay_and_preserves_step_mapping() {
    let f = Fixture::new();
    let a = f.account;
    let s = &f.store;
    s.enqueue(a, comment(FIRST, f.issue)).unwrap();
    s.enqueue(a, comment(SECOND, f.issue)).unwrap();
    s.enqueue(a, comment(THIRD, f.pr)).unwrap();
    let old = s.claim_next(a, 0, 10).unwrap().unwrap();
    assert_eq!(old.operation.id, FIRST);
    let unrelated = s.claim_next(a, 1, 10).unwrap().unwrap();
    assert_eq!(unrelated.operation.id, THIRD);
    s.finish_validation(&unrelated, Validation::NeedsAttention("base_changed"))
        .unwrap();
    assert_eq!(s.recover_expired(a, 10).unwrap(), 1);
    let next = s.claim_next(a, 11, 20).unwrap().unwrap();
    assert!(s.begin_send(&old, "comment", 12).is_err());
    s.begin_send(&next, "comment", 12).unwrap();
    let raw = raw(&f.path);
    raw.execute_batch("CREATE TRIGGER prevent_completion BEFORE INSERT ON step_receipts BEGIN SELECT RAISE(ABORT,'fault in receipt commit'); END;").unwrap();
    assert!(
        s.record_delivery(a, FIRST, next.number, Delivery::Step(&server("comment")))
            .is_err()
    );
    raw.execute_batch("DROP TRIGGER prevent_completion")
        .unwrap();
    let reopened = Store::open(&f.path).unwrap();
    reopened.recover_expired(a, 20).unwrap();
    assert_eq!(
        reopened.operation(a, FIRST).unwrap().unwrap().state,
        State::OutcomeUnknown
    );
    assert!(reopened.claim_next(a, 21, 30).unwrap().is_none());
    reopened
        .record_delivery(a, FIRST, next.number, Delivery::Step(&server("comment")))
        .unwrap();
    let third = reopened.claim_next(a, 25, 35).unwrap().unwrap();
    assert!(reopened.begin_send(&third, "comment", 26).is_err());
    reopened.begin_send(&third, "thread", 26).unwrap();
    reopened
        .record_delivery(
            a,
            FIRST,
            third.number,
            Delivery::Waiting("remote operation number"),
        )
        .unwrap();
    assert_eq!(
        reopened.cancel_operation(a, FIRST).unwrap(),
        CancelResult::MayAlreadyBeSent
    );
    reopened
        .record_delivery(
            a,
            FIRST,
            third.number,
            Delivery::Confirmed(&Receipt {
                object_kind: "thread".into(),
                ..server("thread")
            }),
        )
        .unwrap();
    assert_eq!(reopened.attempts(a, FIRST).unwrap().len(), 3);
    assert_eq!(reopened.receipts(a, FIRST).unwrap().len(), 2);
    assert_eq!(
        reopened
            .claim_next(a, 30, 40)
            .unwrap()
            .unwrap()
            .operation
            .id,
        SECOND
    );
    assert_eq!(
        reopened.operation(a, FIRST).unwrap().unwrap().state,
        State::Confirmed
    );
}

#[test]
fn it_019_signout_scope_pause_and_backup_hold_survive_restart() {
    let f = Fixture::new();
    let s = &f.store;
    let a = f.account;
    s.enqueue(a, comment(FIRST, f.issue)).unwrap();
    s.set_dispatch_controls(a, true, false).unwrap();
    assert!(s.claim_next(a, 0, 10).unwrap().is_none());
    s.set_dispatch_controls(a, false, true).unwrap();
    assert_eq!(
        s.dispatch_hold(a, FIRST).unwrap().as_deref(),
        Some("paused")
    );
    s.set_dispatch_controls(a, false, false).unwrap();
    s.set_repository_paused(a, f.repo, true).unwrap();
    assert!(s.claim_next(a, 0, 10).unwrap().is_none());
    s.set_repository_paused(a, f.repo, false).unwrap();
    let claim = s.claim_next(a, 0, 10).unwrap().unwrap();
    s.sign_out(a).unwrap();
    assert!(s.begin_send(&claim, "comment", 1).is_err());
    let reopened = Store::open(&f.path).unwrap();
    assert!(reopened.authorize_profile(a, "github.com", 99).is_err());
    assert_eq!(
        reopened.dispatch_hold(a, FIRST).unwrap().as_deref(),
        Some("signed_out")
    );
    reopened.authorize_profile(a, "github.com", 41).unwrap();
    reopened.recover_expired(a, 11).unwrap();
    let backup = f.path.with_extension("backup");
    let recovered = f.path.with_extension("new");
    reopened.backup_to(&backup).unwrap();
    Store::recover_to(&backup, &recovered).unwrap();
    let restored = Store::open(&recovered).unwrap();
    restored.authorize_profile(a, "github.com", 41).unwrap();
    assert_eq!(
        restored.dispatch_hold(a, FIRST).unwrap().as_deref(),
        Some("restored_backup")
    );
    assert!(restored.claim_next(a, 15, 20).unwrap().is_none());
    restored.release_restored_profile(a).unwrap();
    assert!(restored.claim_next(a, 15, 20).unwrap().is_none());
    assert_eq!(
        restored.dispatch_hold(a, FIRST).unwrap().as_deref(),
        Some("restored_operation")
    );
    restored
        .reconcile_restored_operation(
            a,
            FIRST,
            RestoredDecision::NoFurtherEffect {
                proof: "authoritative fixture operation record positively reports no effect",
            },
        )
        .unwrap();
    assert!(restored.claim_next(a, 15, 20).unwrap().is_some());
    assert_eq!(
        reopened.operation(a, FIRST).unwrap().unwrap().state,
        State::Queued
    );
}

#[test]
fn older_payloads_migrate_without_losing_snapshots_or_pending_intent() {
    let f = Fixture::new();
    let s = &f.store;
    let a = f.account;
    s.enqueue(a, comment(FIRST, f.issue)).unwrap();
    let db = raw(&f.path);
    db.execute(
        "UPDATE operations SET payload_version=1,snapshot=? WHERE id=?",
        params![
            r#"{"text":"legacy","base_revision":"base-v1","anchor":"A:3","draft":null}"#,
            FIRST
        ],
    )
    .unwrap();
    let before = s.operation(a, FIRST).unwrap().unwrap();
    assert_eq!(before.snapshot.payload, "legacy");
    assert_eq!(s.migrate_operation_payloads(a).unwrap(), 1);
    assert_eq!(s.operation(a, FIRST).unwrap().unwrap(), before);
    assert_eq!(s.migrate_operation_payloads(a).unwrap(), 0);
}

#[test]
fn inspect_pages_and_dispatch_controls_without_reading_unrelated_payloads() {
    let f = Fixture::new();
    let a = f.account;
    let s = &f.store;
    s.enqueue(a, comment(FIRST, f.issue)).unwrap();
    s.enqueue(a, comment(SECOND, f.issue)).unwrap();
    let page = s.queue_page(a, 0, 1).unwrap();
    assert_eq!(page[0].id, FIRST);
    let page = s.queue_page(a, page[0].sequence, 1).unwrap();
    assert_eq!(page[0].id, SECOND);
    assert!(s.queue_page(a, page[0].sequence, 1).unwrap().is_empty());
    assert!(s.queue_page(a, 0, 257).is_err());
    s.sign_out(a).unwrap();
    let restored = Store::open(&f.path).unwrap();
    assert_eq!(
        restored.dispatch_profile(a).unwrap(),
        DispatchProfile {
            active: true,
            authorized: false,
            offline: false,
            outbox_paused: false,
            restored: false
        }
    );
}

#[test]
fn a_snapshot_from_before_dispatch_cannot_repeat_a_remote_success() {
    let f = Fixture::new();
    let s = &f.store;
    let a = f.account;
    s.enqueue(a, comment(FIRST, f.issue)).unwrap();
    let snapshot = f.path.with_extension("before");
    s.backup_to(&snapshot).unwrap();
    let c = s.claim_next(a, 5, 30).unwrap().unwrap();
    s.begin_send(&c, "comment", 10).unwrap();
    s.record_delivery(a, FIRST, c.number, Delivery::Confirmed(&server("comment")))
        .unwrap();

    let destination = f.path.with_extension("after");
    Store::recover_to(&snapshot, &destination).unwrap();
    let restored = Store::open(&destination).unwrap();
    restored.release_restored_profile(a).unwrap();
    let op = restored.operation(a, FIRST).unwrap().unwrap();
    assert_eq!(op.state, State::OutcomeUnknown);
    assert_eq!(op.attempt, 0);
    assert!(restored.claim_next(a, 40, 50).unwrap().is_none());
    assert_eq!(
        restored.cancel_operation(a, FIRST).unwrap(),
        CancelResult::MayAlreadyBeSent
    );
    assert!(
        restored
            .reconcile_restored_operation(
                a,
                FIRST,
                RestoredDecision::Confirmed {
                    proof: "",
                    receipts: &[server("comment")]
                }
            )
            .is_err()
    );
    assert!(restored.receipts(a, FIRST).unwrap().is_empty());
    restored
        .reconcile_restored_operation(
            a,
            FIRST,
            RestoredDecision::Confirmed {
                proof: "matched verified remote node",
                receipts: &[server("comment")],
            },
        )
        .unwrap();
    assert_eq!(
        restored.receipts(a, FIRST).unwrap(),
        vec![server("comment")]
    );
    assert_eq!(
        restored.operation(a, FIRST).unwrap().unwrap().state,
        State::Confirmed
    );
    assert!(restored.claim_next(a, 40, 50).unwrap().is_none());
}

#[test]
fn verified_transfer_keeps_original_locator_and_requires_new_preflight() {
    use threadline_storage::{TransferEvidence, TransferOutcome};
    let f = Fixture::new();
    let s = &f.store;
    let a = f.account;
    s.enqueue(a, comment(FIRST, f.issue)).unwrap();
    let claim = s.claim_next(a, 0, 20).unwrap().unwrap();
    let repo = s.ensure_repository(a, 2, "fixture", "two").unwrap();
    let from = ItemLocator {
        repository: f.repo,
        number: 41,
    };
    let to = ItemLocator {
        repository: repo,
        number: 82,
    };
    assert_eq!(
        s.transfer_issue(
            a,
            from,
            to,
            TransferEvidence::VerifiedIssueId(41.try_into().unwrap())
        )
        .unwrap(),
        TransferOutcome::Moved(f.issue)
    );
    let op = s.operation(a, FIRST).unwrap().unwrap();
    assert_eq!((op.original_repository, op.original_number), (f.repo, 41));
    assert_eq!(
        s.dispatch_hold(a, FIRST).unwrap().as_deref(),
        Some("locator_changed")
    );
    assert!(s.begin_send(&claim, "comment", 2).is_err());
    assert!(
        s.review_operation_locator(
            a,
            FIRST,
            ItemLocator {
                repository: repo,
                number: 99
            },
            "unrelated number"
        )
        .is_err()
    );
    s.review_operation_locator(a, FIRST, to, "same verified node and reviewed scope")
        .unwrap();
    assert!(s.begin_send(&claim, "comment", 3).is_err());
    s.recover_expired(a, 20).unwrap();
    let fresh = s.claim_next(a, 21, 30).unwrap().unwrap();
    s.begin_send(&fresh, "comment", 22).unwrap();
    let db = raw(&f.path);
    assert!(
        db.execute(
            "UPDATE operations SET original_number=82 WHERE id=?",
            [FIRST]
        )
        .is_err()
    );
}
