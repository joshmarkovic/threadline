use super::*;
use std::path::Path;
use threadline_storage::{
    DATABASE_FILENAME, ItemId,
    queue::{Operation, Receipt},
    writes::{FieldEdit, IssueChanges},
};

const OPERATION: &str = "60000000-0000-4000-8000-000000000062";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Issue,
    Comment,
    Review,
    Merge,
}

impl Action {
    fn fault_sql(self) -> &'static str {
        // Fail the last projection insert after the transaction writes the local step receipt.
        match self {
            Self::Issue | Self::Comment => "own_write_actions WHEN NEW.operation_id",
            Self::Review => "review_activity WHEN NEW.step = 'submit_review' AND NEW.operation_id",
            Self::Merge => "confirmed_merges WHEN NEW.operation_id",
        }
    }

    fn unresolved(self) -> State {
        match self {
            Self::Issue | Self::Review => State::OutcomeUnknown,
            Self::Comment | Self::Merge => State::Waiting,
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn write_dispatch_failed_local_completion_reconciles_without_replay() {
    tokio::join!(
        check(Action::Issue),
        check(Action::Comment),
        check(Action::Review),
        check(Action::Merge),
        issue_completion_control(),
    );
}

async fn issue_completion_control() {
    // The same positive issue response must confirm normally when SQLite can commit it.
    let fixture = WriteFixture::start();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_profile(dir.path()).unwrap());
    let (accounts, account, item) = enqueue(Action::Issue, &store, &fixture).await;
    let worker = start(accounts, account, &store);
    wait_for(Action::Issue, "healthy completion control", || {
        operation(&store, account).state == State::Confirmed
    })
    .await;
    stop(worker).await;
    assert_eq!(fixture.sent.load(Ordering::SeqCst), 1);
    assert_eq!(
        receipt_ids(&store.receipts(account, OPERATION).unwrap()),
        [("issue_edited", "55")]
    );
    assert_eq!(
        store.item(account, item).unwrap().unwrap().title,
        Change::Value(ACCEPTED_ISSUE_TITLE.into())
    );
}

async fn check(action: Action) {
    let fixture = WriteFixture::start();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_profile(dir.path()).unwrap());
    let (accounts, account, item) = enqueue(action, &store, &fixture).await;
    let original = operation(&store, account);
    let db_path = dir.path().join(DATABASE_FILENAME);
    let fault = format!(
        "CREATE TRIGGER fail_completion BEFORE INSERT ON {} = '{OPERATION}'
         BEGIN SELECT RAISE(ABORT, 'synthetic local completion failure'); END",
        action.fault_sql(),
    );
    rusqlite::Connection::open(&db_path)
        .unwrap()
        .execute_batch(&fault)
        .unwrap();

    let worker = start(accounts.clone(), account, &store);
    let updates = worker.watch();
    wait_for(action, "failed local completion", || {
        let current = operation(&store, account);
        assert_ne!(
            current.state,
            State::Confirmed,
            "{action:?} ignored the fault"
        );
        assert_ne!(
            current.state,
            State::NeedsAttention,
            "{action:?}: {current:?}"
        );
        current.state == action.unresolved()
            && (action != Action::Merge
                || (fixture.readbacks.load(Ordering::SeqCst) == 1
                    && updates.borrow().cache_generation >= 2))
    })
    .await;
    drop(updates);
    stop(worker).await;

    let pre_restart = operation(&store, account);
    assert_eq!(pre_restart.state, action.unresolved());
    assert_eq!(pre_restart.snapshot, original.snapshot);
    assert_eq!(
        pre_restart.original_repository,
        original.original_repository
    );
    assert_eq!((pre_restart.item, pre_restart.original_number), (item, 42));
    let pending = store.receipts(account, OPERATION).unwrap();
    if action == Action::Review {
        assert_eq!(receipt_ids(&pending), [("create_pending_review", "91")]);
    } else {
        assert!(pending.is_empty(), "{action:?}: {pending:?}");
    }
    assert!(store.own_write(account, OPERATION).unwrap().is_none());
    assert!(store.confirmed_merge(account, OPERATION).unwrap().is_none());
    let cached = store.item(account, item).unwrap().unwrap();
    if action == Action::Issue {
        assert_eq!(cached.title, Change::Value("Existing issue".into()));
    } else if action == Action::Merge {
        assert_eq!(
            cached.pull_request.unwrap().fields.merged,
            Change::Value(false)
        );
    }
    let sent_attempts = store.attempts(account, OPERATION).unwrap();
    assert_eq!(
        sent_attempts.len(),
        if action == Action::Review { 2 } else { 1 }
    );
    assert!(
        sent_attempts
            .iter()
            .all(|attempt| attempt.sending_at.is_some())
    );
    assert_eq!(fixture.sent.load(Ordering::SeqCst), sent_attempts.len());
    let prior_reads = fixture.readbacks.load(Ordering::SeqCst);

    let credentials = accounts.credentials.clone();
    drop(accounts);
    drop(store);
    let reopened = Arc::new(Store::open_profile(dir.path()).unwrap());
    assert_eq!(operation(&reopened, account), pre_restart);
    assert_eq!(reopened.receipts(account, OPERATION).unwrap(), pending);
    restore_storage(&db_path, &pre_restart);

    let accounts = AccountService {
        store: reopened.clone(),
        credentials,
        github: GitHubClient::loopback(fixture.address).unwrap(),
        fixture: true,
    };
    let restarted = SyncWorker::start_fixture(accounts, &tokio::runtime::Handle::current());
    restarted.wake(Some(ProbeTrigger::Startup));
    if action == Action::Issue {
        let updates = restarted.watch();
        wait_for(action, "authenticated startup without replay", || {
            updates.borrow().connection.status
                == threadline_core::connectivity::ConnectionStatus::Reachable
        })
        .await;
        assert!(
            reopened
                .next_write_due(account, UtcMillis::now().unwrap().0)
                .unwrap()
                .is_none()
        );
        assert_eq!(operation(&reopened, account).state, State::OutcomeUnknown);
        assert!(reopened.own_write(account, OPERATION).unwrap().is_none());
    } else {
        wait_for(action, "exact-object readback after restart", || {
            operation(&reopened, account).state == State::Confirmed
        })
        .await;
        assert_eq!(fixture.readbacks.load(Ordering::SeqCst), prior_reads + 1);
        let receipts = reopened.receipts(account, OPERATION).unwrap();
        match action {
            Action::Comment => {
                assert_eq!(receipt_ids(&receipts), [("comment_created", "92")]);
                assert_eq!(
                    reopened
                        .own_write(account, OPERATION)
                        .unwrap()
                        .unwrap()
                        .remote_id
                        .get(),
                    92
                );
            }
            Action::Review => {
                assert_eq!(
                    receipt_ids(&receipts),
                    [("create_pending_review", "91"), ("submit_review", "91")]
                );
            }
            Action::Merge => {
                let receipt = reopened
                    .confirmed_merge(account, OPERATION)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    (receipt.item, receipt.repository),
                    (item, original.original_repository)
                );
                assert_eq!(receipt.expected_head, "b".repeat(40));
                assert_eq!(receipt.merge_oid.as_str(), "c".repeat(40));
                assert!(receipt.own_action);
            }
            Action::Issue => unreachable!(),
        }
    }
    stop(restarted).await;
    let recovered = operation(&reopened, account);
    assert_eq!(recovered.attempt, pre_restart.attempt);
    assert_eq!(recovered.snapshot, original.snapshot);
    assert_eq!(recovered.original_repository, original.original_repository);
    assert_eq!((recovered.item, recovered.original_number), (item, 42));
    assert_eq!(
        reopened.attempts(account, OPERATION).unwrap().len(),
        sent_attempts.len()
    );
    assert_eq!(fixture.sent.load(Ordering::SeqCst), sent_attempts.len());
}

async fn enqueue(
    action: Action,
    store: &Arc<Store>,
    fixture: &WriteFixture,
) -> (AccountService, AccountId, ItemId) {
    let now = UtcMillis::now().unwrap().0;
    match action {
        Action::Issue | Action::Comment => {
            let (accounts, account, repository) =
                select_fixture_repository(store, fixture.address).await;
            let item = seed_fixture_issue(store, account, repository);
            let intent = if action == Action::Issue {
                WriteIntent::EditIssue(IssueChanges {
                    title: Some(FieldEdit {
                        base: "Existing issue".into(),
                        wanted: ACCEPTED_ISSUE_TITLE.into(),
                    }),
                    ..Default::default()
                })
            } else {
                WriteIntent::CreateComment {
                    body: "confirmed fixture".into(),
                }
            };
            store
                .submit_write(
                    account,
                    WriteSubmission {
                        id: OPERATION,
                        item,
                        intent: &intent,
                        draft: None,
                        dependencies: &[],
                        submitted_at_ms: now,
                    },
                )
                .unwrap();
            (accounts, account, item)
        }
        Action::Review | Action::Merge => {
            let (accounts, revision) = prepare_fixture_pr(store, fixture.address).await;
            if action == Action::Review {
                store
                    .submit_review(
                        &revision,
                        ReviewSubmission {
                            id: OPERATION,
                            intent: &ReviewIntent::Review(ReviewDraft {
                                event: ReviewEvent::Comment,
                                summary: "Fixture durable review".into(),
                                comments: vec![],
                            }),
                            draft: None,
                            dependencies: &[],
                            submitted_at_ms: now,
                        },
                    )
                    .unwrap();
            } else {
                seed_fixture_merge_policy(store, &revision, now);
                store
                    .submit_merge(
                        &revision,
                        MergeSubmission {
                            id: OPERATION,
                            method: MergeMethod::Squash,
                            dependencies: &[],
                            submitted_at_ms: now,
                        },
                    )
                    .unwrap();
            }
            (accounts, revision.account, revision.pr)
        }
    }
}

fn operation(store: &Store, account: AccountId) -> Operation {
    store.operation(account, OPERATION).unwrap().unwrap()
}

fn receipt_ids(receipts: &[Receipt]) -> Vec<(&str, &str)> {
    receipts
        .iter()
        .map(|r| (r.step.as_str(), r.remote_id.as_str()))
        .collect()
}

fn restore_storage(path: &Path, interrupted: &Operation) {
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute_batch("DROP TRIGGER fail_completion").unwrap();
    // Advance only this readback reservation. The send attempt and its replay holds remain intact.
    db.execute(
        "UPDATE operations SET retry_at=? WHERE account_id=? AND id=? AND attempt=?
         AND state IN ('waiting','unknown') AND retry_at>0",
        rusqlite::params![
            UtcMillis::now().unwrap().0,
            interrupted.account,
            OPERATION,
            interrupted.attempt,
        ],
    )
    .unwrap();
}

fn start(accounts: AccountService, account: AccountId, store: &Store) -> SyncWorker {
    store
        .update_sync_settings(account, |settings| {
            settings.controls.pause_background = true;
        })
        .unwrap();
    let worker = SyncWorker::start_fixture(accounts, &tokio::runtime::Handle::current());
    worker.wake(Some(ProbeTrigger::Startup));
    worker
}

async fn wait_for(action: Action, condition: &str, mut ready: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(33), async {
        while !ready() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{action:?}: timed out waiting for {condition}"));
}

async fn stop(worker: SyncWorker) {
    let mut updates = worker.watch();
    drop(worker);
    tokio::time::timeout(Duration::from_secs(5), async {
        while updates.changed().await.is_ok() {}
    })
    .await
    .unwrap();
}
