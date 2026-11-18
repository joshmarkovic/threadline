use super::*;
use crate::sync_worker::SyncWorker;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    thread,
    time::Duration,
};
use threadline_core::connectivity::ProbeTrigger;
use threadline_storage::{
    Change, IssueClassification, IssueRecord, ItemLocator, ItemRepresentation, ItemUpdate,
    PullRequestRecord, PullRequestUpdate, UtcMillis,
    merges::{Gate, MergeMethod, MergePolicy, MergeSubmission, QueueRequirement},
    pr::{PrHeader, PrRevision},
    queue::State,
    resources::CommitOid,
    reviews::{ReviewDraft, ReviewEvent, ReviewIntent, ReviewSubmission},
    writes::{WriteIntent, WriteSubmission},
};

struct WriteFixture {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    sent: Arc<AtomicUsize>,
    server: Option<thread::JoinHandle<()>>,
}

impl WriteFixture {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let sent = Arc::new(AtomicUsize::new(0));
        let (stopping, writes) = (stop.clone(), sent.clone());
        let server = thread::spawn(move || {
            let merged = AtomicBool::new(false);
            while !stopping.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => serve(stream, &writes, &merged),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) => panic!("Could not accept a local write fixture request: {error}"),
                }
            }
        });
        Self {
            address,
            stop,
            sent,
            server: Some(server),
        }
    }
}

impl Drop for WriteFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(server) = self.server.take() {
            let result = server.join();
            if !thread::panicking() {
                result.unwrap();
            }
        }
    }
}

fn serve(mut stream: TcpStream, writes: &AtomicUsize, merged: &AtomicBool) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut input = BufReader::new(&mut stream);
    let mut line = String::new();
    input.read_line(&mut line).unwrap();
    let mut request = line.split_ascii_whitespace();
    let method = request.next().unwrap().to_owned();
    let path = request.next().unwrap().to_owned();
    let mut length = 0;
    let mut authenticated = false;
    loop {
        line.clear();
        if input.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
            break;
        }
        let (name, value) = line.trim().split_once(':').unwrap();
        if name.eq_ignore_ascii_case("content-length") {
            length = value.trim().parse().unwrap();
        }
        if name.eq_ignore_ascii_case("authorization") {
            authenticated = value.trim() == "Bearer threadline-test-fixture";
        }
    }
    assert!(authenticated);
    let mut data = vec![0; length];
    input.read_exact(&mut data).unwrap();
    let repo = json!({"id":71,"node_id":"repo-fixture","name":"private","owner":{"login":"fixture-org"},
        "private":true,"permissions":{"pull":true,"push":true,"admin":false}});
    let pending_review = |state| {
        json!({
            "id":91,"state":state,"user":{"id":41,"login":"fixture-user"},"commit_id":"b".repeat(40),
            "pull_request_url":"https://api.github.com/repos/fixture-org/private/pulls/42"
        })
    };
    let (status, response) = match (method.as_str(), path.as_str()) {
        ("GET", "/user") => ("200 OK", json!({"id":41,"login":"fixture-user"})),
        ("GET", "/repos/fixture-org/private") => ("200 OK", repo),
        ("GET", "/user/repos?per_page=100&page=1") => ("200 OK", json!([repo])),
        ("GET", "/repos/fixture-org/private/issues/42") => (
            "200 OK",
            json!({
                "id":55,"number":42,"title":"Existing issue","body":null,"state":"open","locked":false,
                "updated_at":"2023-01-01T00:00:00Z","user":{"id":41,"login":"fixture-user"},
                "repository_url":"https://api.github.com/repos/fixture-org/private"
            }),
        ),
        ("GET", "/repositories/71/pulls/42") if merged.load(Ordering::SeqCst) => (
            "200 OK",
            json!({
                "id":420,"node_id":"PR_420","number":42,"title":"Fixture review","state":"closed","body":"",
                "updated_at":"2023-01-01T00:00:00Z","merged":true,"locked":false,"draft":false,
                "user":{"id":75,"login":"fixture-author"},"merged_by":{"id":41,"login":"fixture-user"},
                "base":{"sha":"a".repeat(40),"ref":"main","repo":{"id":71,"node_id":"repo-fixture"}},
                "head":{"sha":"b".repeat(40),"ref":"topic"}
            }),
        ),
        ("GET", "/repositories/71/pulls/42") => (
            "200 OK",
            json!({
                "id":420,"node_id":"PR_420","number":42,"title":"Fixture review","state":"open","body":"",
                "updated_at":"2023-01-01T00:00:00Z","merged":false,"locked":false,"draft":false,"user":{"id":75,"login":"fixture-author"},
                "base":{"sha":"a".repeat(40),"ref":"main","repo":{"id":71,"node_id":"repo-fixture"}},
                "head":{"sha":"b".repeat(40),"ref":"topic"}
            }),
        ),
        ("GET", "/repos/fixture-org/private/pulls/42/reviews?per_page=100&page=1") => {
            ("200 OK", json!([]))
        }
        ("GET", "/repos/fixture-org/private/pulls/42/reviews/91") => {
            ("200 OK", pending_review("PENDING"))
        }
        ("POST", "/repos/fixture-org/private/pulls/42/reviews") => {
            let payload: Value = serde_json::from_slice(&data).unwrap();
            assert_eq!(
                payload,
                json!({"commit_id":"b".repeat(40),"body":"Fixture durable review","comments":[]})
            );
            assert_eq!(writes.fetch_add(1, Ordering::SeqCst), 0);
            ("200 OK", pending_review("PENDING"))
        }
        ("POST", "/repos/fixture-org/private/pulls/42/reviews/91/events") => {
            let payload: Value = serde_json::from_slice(&data).unwrap();
            assert_eq!(
                payload,
                json!({"event":"COMMENT","body":"Fixture durable review"})
            );
            assert_eq!(writes.fetch_add(1, Ordering::SeqCst), 1);
            ("200 OK", pending_review("COMMENTED"))
        }
        ("POST", "/graphql") => {
            let payload: Value = serde_json::from_slice(&data).unwrap();
            assert_eq!(payload["variables"], json!({"id":"PR_420"}));
            let query = payload["query"].as_str().unwrap();
            let mut proof = json!({
                "id":"PR_420","number":42,"baseRefName":"main","baseRefOid":"a".repeat(40),
                "headRefOid":"b".repeat(40),"state":"OPEN","isDraft":false,"merged":false,
                "mergeable":"MERGEABLE","mergeStateStatus":"UNSTABLE","reviewDecision":null,
                "mergeQueue":null,"repository":{"id":"repo-fixture","viewerPermission":"WRITE",
                    "mergeCommitAllowed":true,"squashMergeAllowed":true,"rebaseMergeAllowed":false}
            });
            if query.starts_with("query ThreadlineMergedPull") {
                proof["state"] = "MERGED".into();
                proof["merged"] = true.into();
                proof["mergeCommit"] = json!({"oid":"c".repeat(40)});
                proof["mergedBy"] = json!({"__typename":"User","databaseId":41});
            } else if query.starts_with("query ThreadlineOptionalMergeChecks") {
                proof["isMergeQueueEnabled"] = false.into();
                proof["baseRef"] = json!({"name":"main","target":{"oid":"a".repeat(40)},"branchProtectionRule":null});
                proof["commits"] = json!({"nodes":[{"commit":{"oid":"b".repeat(40),
                    "statusCheckRollup":{"contexts":{"totalCount":1,"pageInfo":{"hasNextPage":false},
                        "nodes":[{"__typename":"CheckRun","isRequired":false}]}}}}]});
            } else {
                assert!(query.starts_with("query ThreadlineMergePolicy"));
            }
            ("200 OK", json!({"data":{"node":proof}}))
        }
        ("GET", "/repos/fixture-org/private/branches/main") => (
            "200 OK",
            json!({"name":"main","commit":{"sha":"a".repeat(40)},"protected":false,
                "protection":{"required_status_checks":{"enforcement_level":"off","contexts":[],"checks":[]}}}),
        ),
        ("PUT", "/repos/fixture-org/private/pulls/42/merge") => {
            let payload: Value = serde_json::from_slice(&data).unwrap();
            assert_eq!(
                payload,
                json!({"sha":"b".repeat(40),"merge_method":"squash"})
            );
            assert_eq!(writes.fetch_add(1, Ordering::SeqCst), 0);
            merged.store(true, Ordering::SeqCst);
            ("200 OK", json!({"merged":true,"sha":"c".repeat(40)}))
        }
        ("POST", "/repos/fixture-org/private/issues/42/comments") => {
            let payload: Value = serde_json::from_slice(&data).unwrap();
            let body = payload["body"].as_str().unwrap();
            assert!(matches!(
                body,
                "confirmed fixture" | "accepted with lost response"
            ));
            writes.fetch_add(1, Ordering::SeqCst);
            if body == "accepted with lost response" {
                return;
            }
            (
                "201 Created",
                json!({
                    "id":92,"body":body,"issue_url":"https://api.github.com/repos/fixture-org/private/issues/42",
                    "user":{"id":41,"login":"fixture-user"},"created_at":"2023-01-01T00:01:00Z","updated_at":"2023-01-01T00:01:00Z"
                }),
            )
        }
        _ => panic!("Unexpected route in write fixture"),
    };
    let response = response.to_string();
    write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nX-GitHub-Request-Id: TEST:92:F\r\n\r\n{response}",response.len()).unwrap();
}

async fn prepare_fixture_pr(
    store: &Arc<Store>,
    address: std::net::SocketAddr,
) -> (AccountService, PrRevision) {
    let accounts = AccountService {
        store: store.clone(),
        credentials: Arc::new(FixtureVault::default()),
        github: GitHubClient::loopback(address).unwrap(),
        fixture: true,
    };
    let reply = accounts
        .execute(
            AccountCommand::Connect {
                repository: "fixture-org/private".into(),
                pat: Pat::from_input("threadline-test-fixture".into()),
                renew: None,
            },
            &Cancellation::new(),
        )
        .await;
    assert!(reply.github_error.is_none());
    let account = reply.view.active.unwrap();
    let repository = reply.view.repositories[0].id;
    store
        .select_github_repository(account, repository, true)
        .unwrap();
    let base = CommitOid::parse(&"a".repeat(40)).unwrap();
    let head = CommitOid::parse(&"b".repeat(40)).unwrap();
    let now = UtcMillis::now().unwrap();
    let item = store
        .ingest(
            account,
            ItemLocator {
                repository,
                number: 42,
            },
            ItemUpdate {
                representation: ItemRepresentation::PullRequest(PullRequestRecord {
                    id: 420.try_into().unwrap(),
                    node_id: None,
                    details: PullRequestUpdate {
                        merged: Change::Value(false),
                        base_oid: Change::Value(base.as_str().into()),
                        head_oid: Change::Value(head.as_str().into()),
                        merge_oid: Change::Missing,
                    },
                }),
                remote_updated_at: now,
                title: Change::Value("Fixture review".into()),
                body: Change::Null,
                closed: Change::Value(false),
            },
        )
        .unwrap();
    let rev = store
        .record_pr_revision(
            account,
            item,
            &base,
            &head,
            None,
            &PrHeader {
                base_ref: Some("main".into()),
                ..PrHeader::default()
            },
            now.0,
        )
        .unwrap();
    (accounts, rev)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn review_dispatch_preserves_pending_provider_id_and_submits_exact_original_head() {
    const OPERATION: &str = "60000000-0000-4000-8000-000000000075";
    let fixture = WriteFixture::start();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_profile(dir.path()).unwrap());
    let (accounts, rev) = prepare_fixture_pr(&store, fixture.address).await;
    let account = rev.account;
    let now = UtcMillis::now().unwrap();
    store
        .update_sync_settings(account, |s| {
            s.controls.pause_background = true;
            s.controls.work_offline = true;
        })
        .unwrap();
    store
        .submit_review(
            &rev,
            ReviewSubmission {
                id: OPERATION,
                intent: &ReviewIntent::Review(ReviewDraft {
                    event: ReviewEvent::Comment,
                    summary: "Fixture durable review".into(),
                    comments: vec![],
                }),
                draft: None,
                dependencies: &[],
                submitted_at_ms: now.0,
            },
        )
        .unwrap();
    let worker = SyncWorker::start_fixture(accounts, &tokio::runtime::Handle::current());
    worker.wake(Some(ProbeTrigger::Startup));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(fixture.sent.load(Ordering::SeqCst), 0);
    store
        .update_sync_settings(account, |s| s.controls.work_offline = false)
        .unwrap();
    worker.wake(Some(ProbeTrigger::NetworkChanged));
    let completed = tokio::time::timeout(Duration::from_secs(28), async {
        loop {
            let operation = store.operation(account, OPERATION).unwrap().unwrap();
            if operation.state == State::Confirmed {
                return operation;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(fixture.sent.load(Ordering::SeqCst), 2);
    assert_eq!(completed.state, State::Confirmed);
    let receipts = store.receipts(account, OPERATION).unwrap();
    assert_eq!(
        receipts
            .iter()
            .map(|receipt| (receipt.step.as_str(), receipt.remote_id.as_str()))
            .collect::<Vec<_>>(),
        [("create_pending_review", "91"), ("submit_review", "91")]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn merge_dispatch_rechecks_preflight_and_records_one_exact_head() {
    const OPERATION: &str = "60000000-0000-4000-8000-000000000076";
    let fixture = WriteFixture::start();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_profile(dir.path()).unwrap());
    let (accounts, rev) = prepare_fixture_pr(&store, fixture.address).await;
    let account = rev.account;
    let now = UtcMillis::now().unwrap().0;
    let profile = store
        .github_access_profile(account, "fixture-org")
        .unwrap()
        .unwrap();
    let cached = MergePolicy {
        allowed_methods: Some([MergeMethod::Squash].into_iter().collect()),
        viewer_permission: threadline_storage::writes::Permission::Allowed,
        open: Some(true),
        draft: Some(false),
        merged: Some(false),
        mergeable: Some(true),
        queue: QueueRequirement::NotRequired,
        checks: Gate::Satisfied,
        reviews: Gate::Satisfied,
        branch_rules: Gate::Satisfied,
    };
    store
        .record_merge_policy(&profile, &rev, &cached, now)
        .unwrap();
    store
        .update_sync_settings(account, |settings| {
            settings.controls.pause_background = true;
            settings.controls.work_offline = true;
        })
        .unwrap();
    store
        .submit_merge(
            &rev,
            MergeSubmission {
                id: OPERATION,
                method: MergeMethod::Squash,
                dependencies: &[],
                submitted_at_ms: now,
            },
        )
        .unwrap();
    let worker = SyncWorker::start_fixture(accounts, &tokio::runtime::Handle::current());
    worker.wake(Some(ProbeTrigger::Startup));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(fixture.sent.load(Ordering::SeqCst), 0);
    store
        .update_sync_settings(account, |settings| settings.controls.work_offline = false)
        .unwrap();
    worker.wake(Some(ProbeTrigger::NetworkChanged));
    tokio::time::timeout(Duration::from_secs(27), async {
        loop {
            let operation = store.operation(account, OPERATION).unwrap().unwrap();
            assert!(!matches!(
                operation.state,
                State::NeedsAttention | State::OutcomeUnknown
            ));
            if operation.state == State::Confirmed {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let receipt = store.confirmed_merge(account, OPERATION).unwrap().unwrap();
    assert_eq!(receipt.expected_head.as_str(), rev.head.as_str());
    assert_eq!(receipt.merge_oid.as_str(), "c".repeat(40));
    assert!(receipt.own_action);
    worker.wake(Some(ProbeTrigger::CheckNow));
    tokio::time::sleep(Duration::from_millis(180)).await;
    assert_eq!(fixture.sent.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn write_dispatch_uses_durable_offline_outbox_and_stops_after_lost_response() {
    const CONFIRMED: &str = "60000000-0000-4000-8000-000000000001";
    const LOST: &str = "60000000-0000-4000-8000-000000000002";
    const CANCELLED: &str = "60000000-0000-4000-8000-000000000003";
    let fixture = WriteFixture::start();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_profile(dir.path()).unwrap());
    let accounts = AccountService {
        store: store.clone(),
        credentials: Arc::new(FixtureVault::default()),
        github: GitHubClient::loopback(fixture.address).unwrap(),
        fixture: true,
    };
    let cancel = Cancellation::new();
    let reply = accounts
        .execute(
            AccountCommand::Connect {
                repository: "fixture-org/private".to_owned(),
                pat: Pat::from_input("threadline-test-fixture".to_owned()),
                renew: None,
            },
            &cancel,
        )
        .await;
    assert!(reply.github_error.is_none());
    let account = reply.view.active.unwrap();
    let repository = reply.view.repositories[0].id;
    store
        .select_github_repository(account, repository, true)
        .unwrap();
    store
        .update_sync_settings(account, |settings| {
            settings.controls.work_offline = true;
            settings.controls.pause_background = true;
            settings.controls.notification_quiet = true;
        })
        .unwrap();
    let issue = store
        .ingest(
            account,
            ItemLocator {
                repository,
                number: 42,
            },
            ItemUpdate {
                representation: ItemRepresentation::Issue(IssueRecord {
                    id: 55.try_into().unwrap(),
                    node_id: None,
                    classification: IssueClassification::Issue,
                }),
                remote_updated_at: UtcMillis(1_672_531_200_000),
                title: Change::Value("Existing issue".into()),
                body: Change::Null,
                closed: Change::Value(false),
            },
        )
        .unwrap();
    for (id, body) in [
        (CONFIRMED, "confirmed fixture"),
        (LOST, "accepted with lost response"),
        (CANCELLED, "cancelled fixture"),
    ] {
        store
            .submit_write(
                account,
                WriteSubmission {
                    id,
                    item: issue,
                    intent: &WriteIntent::CreateComment { body: body.into() },
                    draft: None,
                    dependencies: &[],
                    submitted_at_ms: UtcMillis::now().unwrap().0,
                },
            )
            .unwrap();
    }
    let worker = SyncWorker::start_fixture(accounts.clone(), &tokio::runtime::Handle::current());
    worker.wake(Some(ProbeTrigger::Startup));
    let state = |id| store.operation(account, id).unwrap().unwrap().state;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(state(CONFIRMED), State::Queued);
    assert_eq!(fixture.sent.load(Ordering::SeqCst), 0);
    store.cancel_operation(account, CANCELLED).unwrap();
    store
        .update_sync_settings(account, |settings| settings.controls.work_offline = false)
        .unwrap();
    worker.wake(Some(ProbeTrigger::NetworkChanged));
    tokio::time::timeout(Duration::from_secs(24), async {
        while state(CONFIRMED) != State::Confirmed || state(LOST) != State::OutcomeUnknown {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(fixture.sent.load(Ordering::SeqCst), 2);
    assert_eq!(state(CANCELLED), State::Cancelled);
    let own = store.own_write(account, CONFIRMED).unwrap().unwrap();
    assert_eq!(own.remote_id.get(), 92);
    assert!(store.own_write(account, LOST).unwrap().is_none());
    drop(worker);
    let restarted = SyncWorker::start_fixture(accounts, &tokio::runtime::Handle::current());
    restarted.wake(Some(ProbeTrigger::Startup));
    tokio::time::sleep(Duration::from_millis(1250)).await;
    assert_eq!(fixture.sent.load(Ordering::SeqCst), 2);
    assert_eq!(state(LOST), State::OutcomeUnknown);
}

#[cfg(feature = "e2e")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_fixture_authorization_verifies_loopback_and_rehydrates_without_network_offline() {
    let fixture = WriteFixture::start();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_profile(dir.path()).unwrap());
    let service = || AccountService {
        store: store.clone(),
        credentials: Arc::new(FixtureVault::default()),
        github: GitHubClient::loopback(fixture.address).unwrap(),
        fixture: true,
    };
    let cancel = Cancellation::new();
    let first = service()
        .execute(AccountCommand::LoadFixture, &cancel)
        .await;
    assert!(first.github_error.is_none());
    assert_eq!(first.reachability, Reachability::ReceivedResponse);
    let account = first.view.active.unwrap();
    assert_eq!(first.view.profiles[0].user_id.get(), 41);
    assert_eq!(first.view.repositories[0].provider_id.get(), 71);
    assert!(first.view.repositories[0].selected);
    let profile = first.view.profiles[0].clone();
    store
        .update_sync_settings(account, |s| s.controls.work_offline = true)
        .unwrap();

    let restarted = service();
    let next = restarted
        .execute(AccountCommand::LoadFixture, &cancel)
        .await;
    assert_eq!(next.reachability, Reachability::NotAttempted);
    assert!(next.view.offline);
    assert_eq!(next.view.active, Some(account));
    assert_eq!(next.view.profiles[0], profile);
    let key = credential_ref(&profile)
        .ok()
        .expect("isolated fixture credential reference");
    assert!(restarted.credentials.read(&key).unwrap().is_some());
    let signout = restarted
        .execute(AccountCommand::SignOut(account), &cancel)
        .await;
    assert_eq!(signout.view.profiles[0].access, AccessState::SignedOut);
    let signed_out = service()
        .execute(AccountCommand::LoadFixture, &cancel)
        .await;
    assert_eq!(signed_out.view.profiles[0].access, AccessState::SignedOut);
    assert_eq!(signed_out.reachability, Reachability::NotAttempted);
}

#[cfg(feature = "e2e")]
#[tokio::test]
async fn sealed_backlog_reauthorizes_the_original_account_but_keeps_its_offline_hold() {
    use threadline_storage::fixtures::{BACKLOG_ARCHIVE, BACKLOG_OWNER, backlog_comment};

    let fixture = WriteFixture::start();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_profile(dir.path()).unwrap());
    let service = || AccountService {
        store: store.clone(),
        credentials: Arc::new(FixtureVault::default()),
        github: GitHubClient::loopback(fixture.address).unwrap(),
        fixture: true,
    };
    let cancel = Cancellation::new();
    let first = service()
        .execute(AccountCommand::LoadFixture, &cancel)
        .await;
    let account = first.view.active.unwrap();
    let repository = first.view.repositories[0].id;
    let profile = &first.view.profiles[0];
    store
        .observe_github_repository(
            profile,
            &RepositoryObservation {
                provider_id: BACKLOG_ARCHIVE.try_into().unwrap(),
                owner: BACKLOG_OWNER,
                name: "archive",
                private: true,
                source: RepositoryObservationSource::Direct,
            },
            10,
        )
        .unwrap();
    let item = store
        .ingest(
            account,
            ItemLocator {
                repository,
                number: 1,
            },
            ItemUpdate {
                representation: ItemRepresentation::Issue(IssueRecord {
                    id: 1.try_into().unwrap(),
                    node_id: None,
                    classification: IssueClassification::Issue,
                }),
                remote_updated_at: UtcMillis(1),
                title: Change::Value("Local fixture".into()),
                body: Change::Null,
                closed: Change::Value(false),
            },
        )
        .unwrap();
    let marker = "00000000-0000-4000-8001-000000000000";
    store
        .submit_write(
            account,
            WriteSubmission {
                id: marker,
                item,
                intent: &WriteIntent::CreateComment {
                    body: backlog_comment(0),
                },
                draft: None,
                dependencies: &[],
                submitted_at_ms: 20,
            },
        )
        .unwrap();
    store.cancel_operation(account, marker).unwrap();
    store
        .update_sync_settings(account, |settings| settings.controls.work_offline = true)
        .unwrap();
    let before = service()
        .execute(AccountCommand::SignOut(account), &cancel)
        .await;
    assert_eq!(before.view.profiles[0].access, AccessState::SignedOut);

    let restarted = service()
        .execute(AccountCommand::LoadFixture, &cancel)
        .await;
    assert!(restarted.github_error.is_none());
    assert_eq!(restarted.reachability, Reachability::ReceivedResponse);
    assert_eq!(restarted.view.active, Some(account));
    assert!(restarted.view.offline);
    assert_eq!(restarted.view.profiles[0].access, AccessState::Readable);
    assert_eq!(
        store.operation(account, marker).unwrap().unwrap().state,
        State::Cancelled
    );
    assert_eq!(fixture.sent.load(Ordering::SeqCst), 0);
}
