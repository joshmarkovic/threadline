use std::{
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use super::*;

struct Simulator {
    address: SocketAddr,
    sso: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    requests: Arc<AtomicUsize>,
    user_requests: Arc<AtomicUsize>,
    repository_id: Arc<AtomicU64>,
    page_two_ready: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Simulator {
    fn start() -> Self {
        Self::start_with_reader(false)
    }

    fn start_with_reader(reader: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let sso = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicUsize::new(0));
        let user_requests = Arc::new(AtomicUsize::new(0));
        let server_user_requests = user_requests.clone();
        let repository_id = Arc::new(AtomicU64::new(42));
        let page_two_ready = Arc::new(AtomicBool::new(false));
        let (server_sso, server_stop, server_requests, server_repository) = (
            sso.clone(),
            stop.clone(),
            requests.clone(),
            repository_id.clone(),
        );
        let server_page_two = page_two_ready.clone();
        let worker = thread::spawn(move || {
            while !server_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        server_requests.fetch_add(1, Ordering::Relaxed);
                        if respond(
                            stream,
                            server_sso.load(Ordering::Relaxed),
                            server_repository.load(Ordering::Relaxed),
                            reader.then(|| (address, server_page_two.load(Ordering::Relaxed))),
                        ) {
                            server_user_requests.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("local simulator accept failed: {e}"),
                }
            }
        });
        Self {
            address,
            sso,
            stop,
            requests,
            user_requests,
            repository_id,
            page_two_ready,
            worker: Some(worker),
        }
    }
}

impl Drop for Simulator {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            if thread::panicking() {
                let _ = worker.join();
            } else {
                worker.join().unwrap();
            }
        }
    }
}

fn respond(
    mut stream: TcpStream,
    sso: bool,
    repository_id: u64,
    reader: Option<(SocketAddr, bool)>,
) -> bool {
    // Windows can inherit the nonblocking listener mode on an accepted socket.
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut line = String::new();
    let mut input = BufReader::new(&mut stream);
    input.read_line(&mut line).unwrap();
    let target = line.split_ascii_whitespace().nth(1).unwrap().to_owned();
    let mut headers = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
            break;
        }
        headers.push_str(&line);
    }
    assert!(
        headers
            .lines()
            .any(|line| line.eq_ignore_ascii_case("authorization: Bearer threadline-test-fixture"))
    );
    let repo = format!(
        r#"{{"id":{repository_id},"node_id":"synthetic-private-repo","name":"private","owner":{{"login":"fixture-org"}},"private":true,"permissions":{{"pull":true,"push":false,"admin":false}}}}"#
    );
    let (status, extra, body) = if let Some(reply) =
        reader.and_then(|(address, page_two)| reader_reply(&target, &repo, address, page_two))
    {
        reply
    } else {
        match target.as_str() {
        "/user" => ("200 OK",String::new(),r#"{"id":101,"login":"fixture-user"}"#.to_owned()),
        "/repos/fixture-org/private" if sso => ("403 Forbidden",
            "X-GitHub-SSO: required; url=https://github.com/orgs/fixture-org/sso?authorization_request=synthetic-request\r\n".into(),
            r#"{"message":"Resource protected by organization SAML enforcement"}"#.into()),
        "/repos/fixture-org/private" => ("200 OK",String::new(),repo.to_owned()),
        target if target.starts_with("/user/repos?") => ("200 OK",if sso {"X-GitHub-SSO: partial-results; organizations=9\r\n".into()} else {String::new()},format!("[{repo}]")),
        _ => panic!("unexpected local fixture route"),
    }
    };
    write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",body.len()).unwrap();
    stream.flush().unwrap();
    target == "/user"
}

fn reader_reply(
    target: &str,
    private: &str,
    address: SocketAddr,
    page_two: bool,
) -> Option<(&'static str, String, String)> {
    use serde_json::json;

    const BASE: &str = "/repos/fixture-org/public";
    const TIME: &str = "2013-09-06T01:01:04Z";
    const NEW_TIME: &str = "2013-09-06T02:01:04Z";
    let open_repo = json!({"id":73,"node_id":"public-repo","name":"public",
        "owner":{"login":"fixture-org"},"private":false,
        "permissions":{"pull":true,"push":false,"admin":false}});
    let issue = json!({"id":84,"node_id":"issue-84","number":41,"title":"Historical issue",
        "state":"closed","body":null,"created_at":"2012-01-01T00:00:00Z","updated_at":TIME,
        "milestone":{"title":"Original release"},"reactions":{"+1":1,"-1":0,"laugh":0,
        "hooray":0,"confused":0,"heart":2,"rocket":0,"eyes":0,"total_count":3}});
    let comment = |id, updated_at, body: &str| {
        json!({
            "id":id,"node_id":format!("comment-{id}"),"user":{"login":"fixture-user"},
            "created_at":TIME,"updated_at":updated_at,"body":body
        })
    };
    let success = |data: serde_json::Value| ("200 OK", String::new(), data.to_string());
    if target.starts_with("/user/repos?") {
        return Some(success(json!([
            serde_json::from_str::<serde_json::Value>(private).unwrap(),
            open_repo
        ])));
    }
    if let Some(path) = target.strip_prefix("/repositories/73")
        && path.ends_with("&per_page=20&page=1")
    {
        if path.starts_with("/issues?state=all&sort=updated&direction=asc&since=") {
            let mut updated = issue.clone();
            updated["repository_url"] = json!("https://api.github.com/repos/fixture-org/public");
            return Some(success(json!([updated])));
        }
        if path.starts_with("/issues/comments?sort=updated&direction=asc&since=") {
            let mut updated = comment(99, NEW_TIME, "Scheduled ordinary conversation");
            updated["issue_url"] =
                json!("https://api.github.com/repos/fixture-org/public/issues/41");
            return Some(success(json!([updated])));
        }
    }
    let response = match target.strip_prefix(BASE)? {
        "" => success(open_repo),
        "/issues?state=all&sort=created&direction=desc&per_page=20&page=1" => {
            success(json!([issue]))
        }
        "/issues/41" => success(issue),
        "/issues/41/comments?per_page=20&page=1" => (
            "200 OK",
            format!(
                "Link: <http://{address}{BASE}/issues/41/comments?per_page=20&page=2>; rel=\"next\"\r\n"
            ),
            json!([comment(11, TIME, "**before edit**")]).to_string(),
        ),
        "/issues/41/comments?per_page=20&page=2" if !page_two => (
            "502 Bad Gateway",
            String::new(),
            json!({"message":"synthetic failure"}).to_string(),
        ),
        "/issues/41/comments?per_page=20&page=2" => success(json!([
            comment(11, NEW_TIME, "**updated digest**"),
            comment(12, TIME, "second reply")
        ])),
        "/issues/41/timeline?per_page=20&page=1" => success(
            json!([{"event":"cross-referenced","node_id":"T_12","source":{"type":"issue","issue":{
                "id":85,"number":42,"url":"https://api.github.com/repos/fixture-org/public/issues/42",
                "repository_url":"https://api.github.com/repos/fixture-org/public"
            }}}]),
        ),
        _ => return None,
    };
    Some(response)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sync_worker_waits_for_hydration_and_restored_offline_then_observes_both_feeds() {
    use crate::sync_worker::SyncWorker;
    use threadline_core::connectivity::{ConnectionStatus, ProbeTrigger};
    use threadline_storage::{ItemLocator, resources::Coverage};

    let simulator = Simulator::start_with_reader(true);
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_profile(dir.path()).unwrap());
    let service = AccountService {
        store: store.clone(),
        credentials: Arc::new(FixtureVault::default()),
        github: GitHubClient::loopback(simulator.address).unwrap(),
        fixture: true,
    };
    let cancel = Cancellation::new();
    let connected = service
        .execute(
            AccountCommand::Connect {
                repository: "fixture-org/private".into(),
                pat: Pat::from_input("threadline-test-fixture".into()),
                renew: None,
            },
            &cancel,
        )
        .await;
    assert!(connected.github_error.is_none());
    let account = connected.view.active.unwrap();
    let public = connected
        .view
        .repositories
        .iter()
        .find(|row| row.name == "public")
        .unwrap()
        .clone();
    let repository = public.id;
    let selected = service
        .execute(
            AccountCommand::Select {
                profile: connected.view.profiles[0].clone(),
                repository: public,
            },
            &cancel,
        )
        .await;
    assert!(selected.github_error.is_none());
    for private in selected
        .view
        .repositories
        .iter()
        .filter(|row| row.selected && row.id != repository)
    {
        store
            .select_github_repository(account, private.id, false)
            .unwrap();
    }
    let mut settings = store.sync_settings(account).unwrap();
    settings.schedule.timezone = Some("UTC".into());
    settings.controls.work_offline = true;
    store.save_sync_settings(account, &settings).unwrap();
    let before = simulator.requests.load(Ordering::Relaxed);
    let worker = SyncWorker::start_fixture(service, &tokio::runtime::Handle::current());
    let mut updates = worker.watch();
    tokio::time::timeout(
        Duration::from_secs(3),
        updates.wait_for(|state| {
            state.account == Some(account) && state.connection.status == ConnectionStatus::Offline
        }),
    )
    .await
    .unwrap()
    .unwrap();
    worker.wake(Some(ProbeTrigger::Startup));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(simulator.requests.load(Ordering::Relaxed), before);

    settings.controls.work_offline = false;
    store.save_sync_settings(account, &settings).unwrap();
    worker.interrupt(Some(ProbeTrigger::CheckNow));
    tokio::time::timeout(
        Duration::from_secs(10),
        updates.wait_for(|state| {
            state.connection.status == ConnectionStatus::Reachable
                && state.cache_generation >= 2
                && state
                    .status
                    .as_ref()
                    .is_some_and(|status| status.last_success_ms.is_some())
        }),
    )
    .await
    .unwrap()
    .unwrap();
    let item = store
        .resolve(
            account,
            ItemLocator {
                repository,
                number: 41,
            },
        )
        .unwrap()
        .unwrap();
    let discussion = store.discussion_page(account, item, None, 20).unwrap();
    assert_eq!(discussion.state.coverage, Coverage::Missing);
    assert!(discussion.records.is_empty());
    for family in ["issues", "comments"] {
        assert!(
            store
                .load_activity_baseline(account, repository, family)
                .unwrap()
                .is_some()
        );
    }
    assert_eq!(simulator.requests.load(Ordering::Relaxed), before + 4);
}

#[tokio::test]
async fn production_pat_policy_rejects_other_credentials_before_http_or_storage() {
    let fixture = Simulator::start();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(&dir.path().join("auth.sqlite")).unwrap());
    let service = AccountService {
        store: store.clone(),
        credentials: Arc::new(FixtureVault::default()),
        github: GitHubClient::loopback(fixture.address).unwrap(),
        fixture: false,
    };
    for invalid in [
        "gho_synthetic",
        "ghs_synthetic",
        "ghu_synthetic",
        "ghr_synthetic",
        "threadline-test-fixture",
        "unclassified",
        "ghp_",
        "github_pat_",
    ] {
        let reply = service
            .execute(
                AccountCommand::Connect {
                    repository: "fixture-org/private".into(),
                    pat: Pat::from_input(invalid.into()),
                    renew: None,
                },
                &Cancellation::new(),
            )
            .await;
        assert_eq!(reply.message, Some("Enter a valid personal access token."));
        assert_eq!(reply.reachability, Reachability::NotAttempted);
    }
    assert_eq!(fixture.requests.load(Ordering::Relaxed), 0);
    assert!(store.github_access_profiles().unwrap().is_empty());
}

#[tokio::test]
async fn entered_private_route_cannot_replace_the_bound_repository_identity() {
    let fixture = Simulator::start();
    let dir = tempfile::tempdir().unwrap();
    let service = AccountService {
        store: Arc::new(Store::open(&dir.path().join("auth.sqlite")).unwrap()),
        credentials: Arc::new(FixtureVault::default()),
        github: GitHubClient::loopback(fixture.address).unwrap(),
        fixture: true,
    };
    let reconnect = |renew| AccountCommand::Connect {
        repository: "fixture-org/private".into(),
        pat: Pat::from_input("threadline-test-fixture".into()),
        renew,
    };
    let cancel = Cancellation::new();
    let connected = service.execute(reconnect(None), &cancel).await;
    assert!(connected.github_error.is_none());
    let original = connected.view.repositories[0].clone();
    let selected = service
        .execute(
            AccountCommand::Select {
                profile: connected.view.profiles[0].clone(),
                repository: original.clone(),
            },
            &cancel,
        )
        .await;
    assert!(selected.view.repositories[0].selected);
    fixture.repository_id.store(43, Ordering::Relaxed);
    let repaired = service
        .execute(reconnect(Some(selected.view.profiles[0].clone())), &cancel)
        .await;
    assert_eq!(
        repaired.github_error.unwrap().kind,
        AccessKind::IdentityMismatch
    );
    assert_eq!(repaired.view.repositories.len(), 1);
    let prior = &repaired.view.repositories[0];
    assert_eq!(prior.id, original.id);
    assert_eq!(prior.provider_id, original.provider_id);
    assert!(prior.selected);
    assert!(!prior.verified_current);
}

#[tokio::test]
async fn account_journey_persists_private_access_partial_sso_and_signout() {
    let fixture = Simulator::start();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("账户.sqlite");
    let store = Arc::new(Store::open(&path).unwrap());
    let memory = Arc::new(FixtureVault::default());
    let service = AccountService {
        store: store.clone(),
        credentials: memory.clone(),
        github: GitHubClient::loopback(fixture.address).unwrap(),
        fixture: true,
    };
    let cancel = Cancellation::new();
    let connected = service
        .execute(
            AccountCommand::Connect {
                repository: "fixture-org/private".into(),
                pat: Pat::from_input("threadline-test-fixture".into()),
                renew: None,
            },
            &cancel,
        )
        .await;
    assert!(
        connected.github_error.is_none(),
        "{:?}",
        connected.github_error
    );
    assert_eq!(connected.reachability, Reachability::ReceivedResponse);
    let profile = connected.view.profiles[0].clone();
    let repo = connected.view.repositories[0].clone();
    assert_eq!(profile.user_id.get(), 101);
    assert_eq!(profile.access, AccessState::Readable);
    assert!(!repo.selected);

    let selected = service
        .execute(
            AccountCommand::Select {
                profile,
                repository: repo,
            },
            &cancel,
        )
        .await;
    assert!(
        selected.github_error.is_none(),
        "{:?}",
        selected.github_error
    );
    assert!(selected.view.repositories[0].selected);
    let profile = selected.view.profiles[0].clone();
    let repository = selected.view.repositories[0].clone();
    let ok_at = repository.last_success_at_ms;
    fixture.sso.store(true, Ordering::Relaxed);
    let restricted = service
        .execute(
            AccountCommand::Refresh {
                profile: profile.clone(),
                repository: Some(repository),
                requested: None,
            },
            &cancel,
        )
        .await;
    let error = restricted.github_error.expect("SSO should be shown");
    assert_eq!(error.kind, AccessKind::SsoRequired);
    assert!(error.authorization_url().is_some());
    assert_eq!(restricted.reachability, Reachability::ReceivedResponse);
    assert_eq!(
        restricted.view.profiles[0].discovery,
        DiscoveryState::RestrictedBySso
    );
    assert_eq!(restricted.view.repositories[0].last_success_at_ms, ok_at);
    assert_eq!(
        restricted.view.repositories[0].access,
        AccessState::SsoRequired
    );

    let stopped = service
        .execute(AccountCommand::SignOut(profile.account), &cancel)
        .await;
    assert_eq!(stopped.view.profiles[0].access, AccessState::SignedOut);
    assert!(stopped.view.repositories[0].selected);
    assert!(
        memory
            .read(&credential_ref(&profile).unwrap_or_else(|_| panic!("missing fixture reference")))
            .unwrap()
            .is_none()
    );
    assert!(fixture.requests.load(Ordering::Relaxed) >= 9);
    assert_eq!(fixture.user_requests.load(Ordering::Relaxed), 3);
    drop(service);
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened.active_github_account().unwrap(),
        Some(profile.account)
    );
    assert_eq!(
        reopened.github_repository_views(profile.account).unwrap()[0].last_success_at_ms,
        ok_at
    );
    assert!(
        !std::fs::read(&path)
            .unwrap()
            .windows(b"threadline-test-fixture".len())
            .any(|bytes| bytes == b"threadline-test-fixture")
    );
}

#[tokio::test]
async fn cache_reader_reopens_the_selected_public_repo_and_resumes_an_edited_comment_page() {
    use crate::reader::{PageStatus, ReaderError, ReaderService};
    use threadline_storage::{
        Change, ItemLocator,
        resources::{Coverage, FetchState, ResourceFamily, ResourceKey, ResourceTarget},
    };

    let fixture = Simulator::start_with_reader(true);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reader-試験.sqlite3");
    let memory = Arc::new(FixtureVault::default());
    let reopen = || AccountService {
        store: Arc::new(Store::open(&path).unwrap()),
        credentials: memory.clone(),
        github: GitHubClient::loopback(fixture.address).unwrap(),
        fixture: true,
    };
    let service = reopen();
    let cancel = Cancellation::new();
    let connected = service
        .execute(
            AccountCommand::Connect {
                repository: "fixture-org/private".into(),
                pat: Pat::from_input("threadline-test-fixture".into()),
                renew: None,
            },
            &cancel,
        )
        .await;
    assert!(connected.github_error.is_none());
    let account = connected.view.active.unwrap();
    let public_repo = connected
        .view
        .repositories
        .iter()
        .find(|repo| repo.name == "public")
        .unwrap();
    assert_eq!(public_repo.private, Some(false));
    let repository = public_repo.id;
    let selected = service
        .execute(
            AccountCommand::Select {
                profile: connected.view.profiles[0].clone(),
                repository: public_repo.clone(),
            },
            &cancel,
        )
        .await;
    assert!(selected.github_error.is_none());
    assert!(
        selected
            .view
            .repositories
            .iter()
            .any(|repo| repo.id == repository && repo.selected)
    );

    let reader = ReaderService::new(service.clone());
    assert!(
        reader
            .fetch_list_page(account, repository, &cancel)
            .await
            .unwrap()
            .complete
    );
    let locator = ItemLocator {
        repository,
        number: 41,
    };
    let item = service.store.resolve(account, locator).unwrap().unwrap();
    reader.fetch_item(account, item, &cancel).await.unwrap();
    assert_eq!(
        service.store.item(account, item).unwrap().unwrap().body,
        Change::Null
    );
    let body = ResourceKey {
        account,
        target: ResourceTarget::Item(item),
        family: ResourceFamily::Body,
    };
    assert_eq!(
        service.store.resource_state(body).unwrap().coverage,
        Coverage::Complete
    );
    let details = service
        .store
        .reader_item_metadata(account, item)
        .unwrap()
        .unwrap();
    assert_eq!(details.created_at_ms, Some(1_325_376_000_000));
    assert_eq!(details.milestone, Change::Value("Original release".into()));
    assert_eq!(details.reactions.unwrap().heart, 2);

    assert_eq!(
        reader.fetch_comments(account, item, &cancel).await.unwrap(),
        PageStatus {
            has_more: true,
            complete: false,
            contacted: true,
        }
    );
    let error = reader
        .fetch_comments(account, item, &cancel)
        .await
        .unwrap_err();
    assert!(matches!(error, ReaderError::Remote(ref e) if e.kind == AccessKind::ServiceFailure));
    let partial = service
        .store
        .discussion_page(account, item, None, 20)
        .unwrap();
    assert_eq!(partial.state.coverage, Coverage::Partial);
    assert_eq!(partial.state.fetch, FetchState::Failed);
    assert_eq!(partial.records.len(), 1);
    assert_eq!(partial.records[0].body, "**before edit**");
    assert!(
        reader
            .fetch_timeline(account, item, &cancel)
            .await
            .unwrap()
            .complete
    );
    let timeline = service
        .store
        .timeline_page(account, item, None, 20)
        .unwrap();
    assert_eq!(timeline.entries.len(), 1);
    assert_eq!(timeline.entries[0].occurred_at_ms, None);
    let linked = timeline.entries[0].linked_item.as_ref().unwrap();
    assert_eq!(
        (
            linked.owner.as_str(),
            linked.repository.as_str(),
            linked.number
        ),
        ("fixture-org", "public", 42)
    );
    assert_eq!(linked.provider_issue_id, Some(85));
    assert_eq!(linked.provider_repository_id, Some(73));

    drop(reader);
    drop(service);
    fixture.page_two_ready.store(true, Ordering::Relaxed);
    let reopened = reopen();
    let resumed = ReaderService::new(reopened.clone());
    assert_eq!(
        reopened.store.resolve(account, locator).unwrap(),
        Some(item)
    );
    let reopened_details = reopened
        .store
        .reader_item_metadata(account, item)
        .unwrap()
        .unwrap();
    assert_eq!(
        reopened_details.milestone,
        Change::Value("Original release".into())
    );
    assert_eq!(
        reopened
            .store
            .timeline_page(account, item, None, 20)
            .unwrap()
            .entries[0]
            .linked_item
            .as_ref(),
        Some(linked)
    );
    assert!(
        resumed
            .fetch_comments(account, item, &cancel)
            .await
            .unwrap()
            .complete
    );
    let comments = reopened
        .store
        .discussion_page(account, item, None, 20)
        .unwrap();
    assert_eq!(comments.state.coverage, Coverage::Complete);
    assert_eq!(comments.records.len(), 2);
    assert_eq!(comments.records[0].body, "**updated digest**");
    assert_eq!(comments.metadata[0].node_id.as_deref(), Some("comment-11"));
    assert_eq!(comments.metadata[1].node_id.as_deref(), Some("comment-12"));
    let before_offline = fixture.requests.load(Ordering::Relaxed);
    reopened
        .execute(
            AccountCommand::SetOffline {
                account,
                offline: true,
            },
            &cancel,
        )
        .await;
    assert!(matches!(
        resumed
            .search_remote(account, repository, "digest", 1, &cancel)
            .await,
        Err(ReaderError::Offline)
    ));
    assert_eq!(fixture.requests.load(Ordering::Relaxed), before_offline);
    assert_eq!(
        reopened
            .store
            .discussion_page(account, item, None, 20)
            .unwrap()
            .records
            .len(),
        2
    );
}

#[cfg(feature = "e2e")]
#[gpui_kit::test]
fn account_form_enters_masked_fixture_credential_and_verifies_the_user(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::{ProfileStore, observation::Observer, workspace::Workspace};
    use gpui_kit::{
        AppContext, Bounds, InputEvent as _, MouseButton, MouseDownEvent, MouseMoveEvent,
        MouseUpEvent, Point, WindowBounds, WindowOptions, point, px, size, test::TestWindowExt,
    };

    cx.executor().allow_parking();
    let simulator = Simulator::start();
    let profile = tempfile::tempdir().unwrap();
    let report = profile.path().join("native-receipt.json");
    let store = Arc::new(Store::open(&profile.path().join("account-form.sqlite")).unwrap());
    let service = AccountService {
        store: store.clone(),
        credentials: Arc::new(FixtureVault::default()),
        github: GitHubClient::loopback(simulator.address).unwrap(),
        fixture: true,
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let handle = runtime.handle().clone();
    let (observer, writer) =
        Observer::new(report.clone(), std::time::Instant::now(), false, &handle);
    let (window, _) = cx.update(|cx| {
        gpui_kit::init(cx);
        crate::workspace::bind_shortcuts(cx);
        cx.set_global(ProfileStore(store.clone()));
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(1060.), px(790.)),
                })),
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| {
                    let mut workspace = Workspace::new_with_service(
                        window,
                        cx,
                        42,
                        handle,
                        false,
                        false,
                        Some(service),
                    );
                    workspace.set_observer(Some(observer.clone()), cx);
                    workspace
                })
            },
        )
        .unwrap()
    });
    let read_account = || {
        let bytes = std::fs::read(&report).ok()?;
        let root: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        Some(root.get("account_access")?.clone())
    };
    wait_form(cx, |cx| {
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
        read_account().is_some_and(|account| account["busy"] == false)
    });
    cx.update_window(window, |_, window, cx| {
        window.press("secondary-shift-a", cx)
    })
    .unwrap();

    let click = |id: &str, cx: &mut gpui_kit::TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let [x, y, width, height] = observer.test_control_bounds(id).unwrap();
            let position = point(px(x + width / 2.), px(y + height / 2.));
            let modifiers = window.modifiers();
            window.dispatch_event(
                MouseMoveEvent {
                    position,
                    pressed_button: None,
                    modifiers,
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                MouseDownEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers,
                    click_count: 1,
                    first_mouse: false,
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                MouseUpEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers,
                    click_count: 1,
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
        })
        .unwrap();
    };
    let wait_receipt = |field: &str, cx: &mut gpui_kit::TestAppContext| {
        wait_form(cx, |cx| {
            cx.update_window(window, |_, window, cx| window.render_frame(cx))
                .unwrap();
            read_account().is_some_and(|account| account[field] == true)
        });
    };
    // Exercise real focus transfer in both directions using the receipt geometry.
    click("account-token", cx);
    wait_receipt("fixture_token_focused", cx);
    click("account-repository", cx);
    wait_receipt("fixture_route_focused", cx);
    cx.update_window(window, |_, window, cx| {
        window.press("secondary-a", cx);
        window.input("fixture-org/private", cx);
    })
    .unwrap();
    wait_receipt("fixture_route_entered", cx);
    assert_eq!(read_account().unwrap()["fixture_token_entered"], false);
    click("account-token", cx);
    wait_receipt("fixture_token_focused", cx);
    cx.update_window(window, |_, window, cx| {
        window.press("secondary-a", cx);
        window.input(FIXTURE_PAT, cx);
    })
    .unwrap();
    wait_receipt("fixture_token_entered", cx);
    assert!(
        !std::fs::read_to_string(&report)
            .unwrap()
            .contains(FIXTURE_PAT)
    );
    click("account-connect", cx);
    wait_form(cx, |cx| {
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
        read_account().is_some_and(|account| account["profile_access"] == "Readable")
    });
    assert_eq!(simulator.user_requests.load(Ordering::SeqCst), 1);
    let account = store.active_github_account().unwrap().unwrap();
    let loaded = store.github_repository_views(account).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].access, AccessState::Readable);
    assert!(loaded[0].verified_current);
    assert_eq!(read_account().unwrap()["active_user_id"], 101);
    assert_eq!(read_account().unwrap()["fixture_token_entered"], false);
    runtime.block_on(writer.finish()).unwrap();
}

#[cfg(feature = "e2e")]
fn wait_form(
    cx: &mut gpui_kit::TestAppContext,
    mut condition: impl FnMut(&mut gpui_kit::TestAppContext) -> bool,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        cx.run_until_parked();
        if condition(cx) {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("The account form did not finish the isolated identity check");
}
