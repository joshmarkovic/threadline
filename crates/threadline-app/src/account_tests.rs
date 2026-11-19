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
    check_arrived: Arc<AtomicBool>,
    hold_check_response: Arc<AtomicBool>,
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
        let check_arrived = Arc::new(AtomicBool::new(false));
        let hold_check_response = Arc::new(AtomicBool::new(false));
        let (server_check_arrived, server_check_hold) =
            (check_arrived.clone(), hold_check_response.clone());
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
                            &server_check_arrived,
                            &server_check_hold,
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
            check_arrived,
            hold_check_response,
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
    check_arrived: &AtomicBool,
    hold_check_response: &AtomicBool,
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
    let checked_head = ["b".repeat(40), "d".repeat(40)].into_iter().find(|head| {
        target
            == format!("/repositories/73/commits/{head}/check-runs?filter=all&per_page=20&page=1")
    });
    let (status, extra, body) = if let Some(head) = checked_head.filter(|_| reader.is_some()) {
        check_arrived.store(true, Ordering::Release);
        for _ in 0..500 {
            if !hold_check_response.load(Ordering::Acquire) {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        let etag = "ETag: \"checks-v1\"\r\n".into();
        if headers
            .lines()
            .any(|line| line.eq_ignore_ascii_case("if-none-match: \"checks-v1\""))
        {
            ("304 Not Modified", etag, String::new())
        } else {
            (
                "200 OK",
                etag,
                serde_json::json!({"total_count":1, "check_runs":[{
                "id":6,"head_sha":head,"name":"Fixture test","status":"completed",
                    "conclusion":"success","completed_at":"2013-09-06T00:00:00Z"
                }]})
                .to_string(),
            )
        }
    } else if let Some(reply) =
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

fn exact_pair_reply(target: &str) -> Option<serde_json::Value> {
    use serde_json::json;
    let oid = |digit: char| digit.to_string().repeat(40);
    let object = |path, mode, kind, digit, size| {
        json!({
            "path":path,"mode":mode,"type":kind,"sha":oid(digit),"size":size
        })
    };
    let blob = |digit, size, content| {
        json!({
            "sha":oid(digit),"size":size,"encoding":"base64","content":content
        })
    };
    let response = if target
        == format!(
            "/repositories/73/compare/{}...{}?per_page=20&page=1",
            oid('b'),
            oid('d')
        ) {
        json!({"base_commit":{"sha":oid('b')},"merge_base_commit":{"sha":oid('a')},"files":[{
            "filename":"three-dot-only.txt","status":"added","additions":1,"deletions":0,"patch":"@@ -0,0 +1 @@\n+Different scope"
        }]})
    } else if target == format!("/repositories/73/git/commits/{}", oid('b')) {
        json!({"sha":oid('b'),"tree":{"sha":oid('0')}})
    } else if target == format!("/repositories/73/git/commits/{}", oid('d')) {
        json!({"sha":oid('d'),"tree":{"sha":oid('1')}})
    } else if target == format!("/repositories/73/git/trees/{}", oid('0')) {
        json!({"sha":oid('0'),"truncated":false,"tree":[
            object("note.txt","100644","blob",'2',Some(5)),
            object("removed.txt","100644","blob",'2',Some(5)),
            object("shared","040000","tree",'6',None),
            object("src","040000","tree",'7',None)
        ]})
    } else if target == format!("/repositories/73/git/trees/{}", oid('1')) {
        json!({"sha":oid('1'),"truncated":false,"tree":[
            object("note.txt","100644","blob",'3',Some(6)),
            object("binary.bin","100644","blob",'4',Some(3)),
            object("capped.bin","100644","blob",'5',Some(100000)),
            object("shared","040000","tree",'6',None),
            object("src","040000","tree",'8',None)
        ]})
    } else if target == format!("/repositories/73/git/trees/{}", oid('7')) {
        json!({"sha":oid('7'),"truncated":false,"tree":[object("unknown.txt","100644","blob",'2',Some(5))]})
    } else if target == format!("/repositories/73/git/trees/{}", oid('8')) {
        json!({"sha":oid('8'),"truncated":true,"tree":[]})
    } else if target == format!("/repositories/73/git/blobs/{}", oid('2')) {
        blob('2', 5, "bGVmdAo=")
    } else if target == format!("/repositories/73/git/blobs/{}", oid('3')) {
        blob('3', 6, "cmlnaHQK")
    } else if target == format!("/repositories/73/git/blobs/{}", oid('4')) {
        blob('4', 3, "AHh5")
    } else {
        return None;
    };
    Some(response)
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
    if let Some(response) = exact_pair_reply(target) {
        if page_two && target == format!("/repositories/73/git/blobs/{}", "3".repeat(40)) {
            return Some((
                "403 Forbidden",
                String::new(),
                json!({"message":"Fixture contents denied"}).to_string(),
            ));
        }
        return Some(success(response));
    }
    if target == "/repos/fixture-org/private/issues/12" {
        return Some(success(
            json!({"id":120,"node_id":"I_12","number":12,"title":"Linked issue","body":"Context fixture",
            "state":"open","updated_at":TIME,"repository_url":"https://api.github.com/repos/fixture-org/private"}),
        ));
    }
    let main = json!({"name":"main","commit":{"sha":"a".repeat(40)}});
    if target == "/repositories/73/branches/main" {
        return Some(success(main));
    }
    if target == "/repositories/73/branches?per_page=20&page=1" {
        return Some(success(json!([
            main,
            {"name":"release/one","commit":{"sha":"c".repeat(40)}},
            {"name":"unselected","commit":{"sha":"d".repeat(40)}}
        ])));
    }
    for digit in ['a', 'b', 'c'] {
        let sha = digit.to_string().repeat(40);
        if target == format!("/repositories/73/commits?sha={sha}&per_page=20&page=1") {
            return Some(success(json!([{
                "sha":sha,"parents":[],"commit":{"message":"Pinned fixture commit",
                "author":{"name":"Fixture Git author","date":TIME},"committer":{"date":TIME}}
            }])));
        }
    }
    if target == "/repositories/73/pulls/42" {
        return Some(success(
            json!({"id":420,"node_id":"P_42","number":42,"title":"Review fixture","body":"Closes fixture-org/private#12",
            "state":"open","draft":false,"merged":false,"updated_at":if page_two { NEW_TIME } else { TIME },"changed_files":1,
            "base":{"sha":"a".repeat(40),"ref":"main","repo":{"id":73,"node_id":"public-repo"}},
            "head":{"sha":if page_two { "d".repeat(40) } else { "b".repeat(40) },"ref":"topic"}}),
        ));
    }
    if target
        == format!(
            "/repositories/73/compare/{}...{}?per_page=20&page=1",
            "a".repeat(40),
            "b".repeat(40)
        )
    {
        return Some(success(
            json!({"base_commit":{"sha":"a".repeat(40)},"merge_base_commit":{"sha":"c".repeat(40)},
            "total_commits":1,"commits":[{"sha":"b".repeat(40),"commit":{"message":"Fixture change"}}],"files":[{
                "filename":"src/lib.rs","status":"modified","additions":1,"deletions":1,"patch":"@@ -1 +1 @@\n-before\n+after"}]}),
        ));
    }
    if target
        == format!(
            "/repositories/73/contents/src/lib.rs?ref={}",
            "c".repeat(40)
        )
    {
        return Some(success(
            json!({"path":"src/lib.rs","sha":"1".repeat(40),"type":"file","size":7,"encoding":"base64","content":"YmVmb3JlCg=="}),
        ));
    }
    if target == "/repos/fixture-org/public/issues/42/comments?per_page=20&page=1" {
        return Some(success(json!([])));
    }
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
        if path.starts_with("/pulls/comments?sort=updated&direction=asc&since=") {
            return Some(success(json!([])));
        }
        if path == "/pulls?state=open&sort=created&direction=asc&per_page=20&page=1" {
            return Some(success(json!([])));
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
async fn pr_service_commits_verified_files_context_and_cross_repository_reference() {
    use crate::reader::{PrSourceRequest, ReaderError, ReaderService};
    use threadline_storage::{
        Change, IssueClassification, IssueRecord, ItemLocator, ItemRepresentation, ItemUpdate,
        NodeId, UtcMillis,
        pr::{PrDownloadState, PrFamily, PrInputPage, PrRecord, SourceSide, Verification},
        resources::{Coverage, WalkMode},
    };

    let fixture = Simulator::start_with_reader(true);
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_profile(dir.path()).unwrap());
    let service = AccountService {
        store: store.clone(),
        credentials: Arc::new(FixtureVault::default()),
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
    assert!(connected.github_error.is_none());
    let account = connected.view.active.unwrap();
    let public = connected
        .view
        .repositories
        .iter()
        .find(|repo| repo.name == "public")
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
    for repo in selected
        .view
        .repositories
        .iter()
        .filter(|repo| repo.id != repository && repo.selected)
    {
        store
            .select_github_repository(account, repo.id, false)
            .unwrap();
    }
    let pr = store
        .ingest(
            account,
            ItemLocator {
                repository,
                number: 42,
            },
            ItemUpdate {
                representation: ItemRepresentation::Issue(IssueRecord {
                    id: 142.try_into().unwrap(),
                    node_id: Some(NodeId::new("I_142").unwrap()),
                    classification: IssueClassification::PullRequest,
                }),
                remote_updated_at: UtcMillis(1000),
                title: Change::Value("Older summary".into()),
                body: Change::Missing,
                closed: Change::Value(false),
            },
        )
        .unwrap();
    let reader = ReaderService::new(service.clone());
    let left = reader.fetch_pr_family(account, pr, PrFamily::Files, &cancel);
    let right = reader.fetch_pr_family(account, pr, PrFamily::Files, &cancel);
    let before = fixture.requests.load(Ordering::Relaxed);
    let (left, right) = tokio::join!(left, right);
    assert!(left.unwrap().complete && right.unwrap().complete);
    assert_eq!(fixture.requests.load(Ordering::Relaxed), before + 4);
    let (revision, header) = store.current_pr_revision(account, pr).unwrap().unwrap();
    assert_eq!(header.base_ref.as_deref(), Some("main"));
    assert_eq!(
        revision.merge_base.as_ref().unwrap().as_str(),
        "c".repeat(40)
    );
    let file = store.pr_file(&revision, "src/lib.rs").unwrap();
    assert!(matches!(
        file,
        Some(PrRecord::File {
            additions: Some(1),
            ..
        })
    ));

    let (body_revision, refs) = store.pr_reference_set(account, pr).unwrap().unwrap();
    reader
        .verify_pr_reference(account, pr, &body_revision, refs[0].position, &cancel)
        .await
        .unwrap();
    let linked = store
        .pr_body_references(account, pr)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(linked.verification, Verification::Verified);
    assert!(linked.verified_item.is_some());
    assert_eq!(
        store
            .github_repository_views(account)
            .unwrap()
            .iter()
            .filter(|repo| repo.selected)
            .count(),
        1
    );

    let request = || PrSourceRequest {
        revision: &revision,
        file: "src/lib.rs",
        side: SourceSide::Left,
        first_line: 1,
        lines: 1,
    };
    reader
        .fetch_pr_source(account, pr, request(), &cancel)
        .await
        .unwrap();
    let context = store
        .pr_page(&revision, PrFamily::Context, None, 10)
        .unwrap();
    assert!(matches!(&context.rows[0].1,PrRecord::Context {text,..} if text == "before"));

    store
        .update_sync_settings(account, |settings| {
            settings.controls.pause_background = true
        })
        .unwrap();
    let check_target = revision.read_target(PrFamily::Checks);
    let request_check = || {
        store
            .subscribe_read(
                account,
                &check_target,
                "explicit-fixture-check",
                threadline_storage::sync::ReadPriority::Explicit,
                UtcMillis::now().unwrap().0,
            )
            .unwrap();
    };
    request_check();
    let worker =
        crate::sync_worker::SyncWorker::start_fixture(service, &tokio::runtime::Handle::current());
    worker.wake(Some(threadline_core::connectivity::ProbeTrigger::Startup));
    let checks = tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let checks = store
                .pr_page(&revision, PrFamily::Checks, None, 10)
                .unwrap();
            if checks.state.coverage == Coverage::Complete {
                break checks;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        matches!(&checks.rows[0].1, PrRecord::Check {head_oid,..} if *head_oid == "b".repeat(40))
    );
    assert_eq!(
        checks.state.validators.etag.as_deref(),
        Some("\"checks-v1\"")
    );
    let downloaded_at = checks.state.last_fetch_at_ms;
    request_check();
    worker.wake(None);
    let validated = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let checks = store
                .pr_page(&revision, PrFamily::Checks, None, 10)
                .unwrap();
            if checks.state.last_validation_at_ms > downloaded_at {
                break checks;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(validated.state.last_fetch_at_ms, downloaded_at);
    assert_eq!(validated.rows, checks.rows);
    drop(worker);

    for family in [
        PrFamily::Commits,
        PrFamily::Reviews,
        PrFamily::Threads,
        PrFamily::Statuses,
    ] {
        let progress = store
            .start_pr_fetch(&revision, family, WalkMode::Full)
            .unwrap();
        store
            .commit_pr_page(
                &revision,
                family,
                &progress,
                PrInputPage {
                    records: vec![],
                    next_cursor: None,
                    validators: None,
                    partial: None,
                    fetched_at_ms: UtcMillis::now().unwrap().0,
                },
            )
            .unwrap();
    }
    let mut settings = store.sync_settings(account).unwrap();
    settings.controls.work_offline = true;
    store.save_sync_settings(account, &settings).unwrap();
    let job = store
        .request_pr_download(&revision, false, false, UtcMillis::now().unwrap().0)
        .unwrap();
    assert_eq!(job.state, PrDownloadState::Pending);
    assert!(matches!(
        reader
            .run_pr_download(&revision, job.generation, &cancel)
            .await,
        Err(ReaderError::Offline)
    ));
    settings.controls.work_offline = false;
    store.save_sync_settings(account, &settings).unwrap();
    assert_eq!(
        reader
            .run_pr_download(&revision, job.generation, &cancel)
            .await
            .unwrap(),
        PrDownloadState::Complete
    );
    assert_eq!(
        store
            .pr_page(&revision, PrFamily::Files, None, 10)
            .unwrap()
            .state
            .coverage,
        Coverage::Complete
    );
    let optional = store
        .request_pr_download(&revision, true, true, UtcMillis::now().unwrap().0)
        .unwrap();
    assert_eq!(
        reader
            .run_pr_download(&revision, optional.generation, &cancel)
            .await
            .unwrap(),
        PrDownloadState::Partial
    );
    let media = store.pr_page(&revision, PrFamily::Media, None, 1).unwrap();
    let job = store.pr_download(&revision).unwrap().unwrap();
    assert_eq!(
        job.unselected_optional_resources().collect::<Vec<_>>(),
        [PrFamily::Media]
    );
    assert!(!media.state.shows_empty(media.rows.len()));

    fixture.check_arrived.store(false, Ordering::Release);
    fixture.hold_check_response.store(true, Ordering::Release);
    let old_reader = reader.clone();
    let old = tokio::spawn(async move {
        old_reader
            .fetch_pr_family(account, pr, PrFamily::Checks, &Cancellation::new())
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !fixture.check_arrived.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    fixture.page_two_ready.store(true, Ordering::Release);
    let new_revision = store
        .record_pr_revision(
            account,
            pr,
            &revision.base,
            &threadline_storage::resources::CommitOid::parse(&"d".repeat(40)).unwrap(),
            None,
            &header,
            chrono::DateTime::parse_from_rfc3339("2013-09-06T02:01:04Z")
                .unwrap()
                .timestamp_millis(),
        )
        .unwrap();
    let new = reader.fetch_pr_family(account, pr, PrFamily::Checks, &cancel);
    tokio::pin!(new);
    tokio::select! { biased;
        _ = &mut new => panic!("new revision completed while previous HTTP request was pending"),
        _ = tokio::time::sleep(Duration::from_millis(25)) => {}
    }
    fixture.hold_check_response.store(false, Ordering::Release);
    assert!(matches!(old.await.unwrap(), Err(ReaderError::ScopeChanged)));
    assert!(new.await.unwrap().complete);
    let fresh = store
        .pr_page(&new_revision, PrFamily::Checks, None, 10)
        .unwrap();
    assert!(
        matches!(&fresh.rows[0].1, PrRecord::Check {head_oid,..} if *head_oid == "d".repeat(40))
    );
    let previous = store
        .pr_page(&revision, PrFamily::Checks, None, 10)
        .unwrap();
    assert_eq!(
        previous.state.last_validation_at_ms,
        validated.state.last_validation_at_ms
    );

    let before = fixture.requests.load(Ordering::Relaxed);
    assert!(matches!(
        reader
            .fetch_pr_source(account, pr, request(), &cancel)
            .await,
        Err(ReaderError::ScopeChanged)
    ));
    assert_eq!(fixture.requests.load(Ordering::Relaxed), before + 3);
    assert!(store.pr_file(&revision, "src/lib.rs").unwrap().is_some());
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn monitored_history_fetches_only_selected_refs_and_caches_pinned_ancestry() {
    use crate::sync_worker::SyncWorker;
    use threadline_core::connectivity::{ConnectionStatus, ProbeTrigger};
    use threadline_storage::{
        history::{HistoryMonitoring, HistoryRef},
        resources::CommitOid,
    };

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
    let pull = std::num::NonZeroU64::new(42).unwrap();
    store
        .set_history_monitoring(
            account,
            repository,
            &HistoryMonitoring {
                default_branch: Some("main".into()),
                branch_patterns: ["release/*".into()].into(),
                followed_prs: [pull].into(),
                ..Default::default()
            },
        )
        .unwrap();
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
    let expected = [
        (HistoryRef::Branch("main".into()), 'a'),
        (HistoryRef::Branch("release/one".into()), 'c'),
        (HistoryRef::FollowedPr(pull), 'b'),
    ];
    tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            let complete = expected.iter().all(|(reference, digit)| {
                let oid = CommitOid::parse(&digit.to_string().repeat(40)).unwrap();
                store
                    .history_ref(account, repository, reference)
                    .unwrap()
                    .is_some_and(|row| row.tip.as_ref() == Some(&oid))
                    && store
                        .history_entry(account, repository, &oid)
                        .unwrap()
                        .is_some()
            });
            if complete {
                break;
            }
            updates.changed().await.unwrap();
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "History did not arrive: {:?} {:?}",
            store.history_refs(account, repository, None, 20).unwrap(),
            store.sync_status(account).unwrap()
        );
    });
    assert_eq!(
        store
            .history_refs(account, repository, None, 20)
            .unwrap()
            .len(),
        expected.len()
    );
    assert!(
        store
            .history_ref_moves(account, repository, 0, 20)
            .unwrap()
            .is_empty()
    );
    assert!(simulator.requests.load(Ordering::Relaxed) > before);
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
    simulator.sso.store(true, Ordering::Relaxed);
    click("account-refresh", cx);
    wait_form(cx, |cx| {
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
        read_account().is_some_and(|account| account["discovery"] == "RestrictedBySso")
    });
    cx.update_window(window, |_, window, cx| {
        window.press("secondary-shift-a", cx)
    })
    .unwrap();
    wait_form(cx, |cx| {
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
        read_account().is_some_and(|account| account["panel_open"] == false)
    });
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

#[tokio::test]
async fn divergent_pair_uses_exact_trees_and_preserves_bounded_patch_evidence() {
    use crate::reader::{ReaderError, ReaderService};
    use threadline_storage::{
        history::{ComparisonOptions, HistoryCoverage, HistoryEntry, HistoryRef},
        pr::{PatchEvidence, PrRecord},
        resources::{CommitOid, ComparisonMode, GraphCommit},
    };
    let fixture = Simulator::start_with_reader(true);
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_profile(dir.path()).unwrap());
    let accounts = AccountService {
        store: store.clone(),
        credentials: Arc::new(FixtureVault::default()),
        github: GitHubClient::loopback(fixture.address).unwrap(),
        fixture: true,
    };
    let cancel = Cancellation::new();
    let connected = accounts
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
    let repo = connected
        .view
        .repositories
        .iter()
        .find(|repo| repo.name == "public")
        .unwrap()
        .clone();
    let repository = repo.id;
    let selected = accounts
        .execute(
            AccountCommand::Select {
                profile: connected.view.profiles[0].clone(),
                repository: repo,
            },
            &cancel,
        )
        .await;
    assert!(selected.github_error.is_none());
    let profile = &selected.view.profiles[0];
    let oid = |digit: char| CommitOid::parse(&digit.to_string().repeat(40)).unwrap();
    for (reference, tip) in [("left", oid('b')), ("right", oid('d'))] {
        let reference = HistoryRef::Branch(reference.into());
        store
            .observe_history_ref(profile, repository, &reference, Some(&tip), 10)
            .unwrap();
        let walk = store
            .begin_history_page(profile, repository, &reference)
            .unwrap()
            .unwrap();
        store
            .commit_history_page(
                &walk,
                &[HistoryEntry {
                    commit: GraphCommit {
                        oid: tip,
                        title: "Fixture endpoint".into(),
                        committed_at_ms: 5,
                        parents: vec![],
                    },
                    message: "Fixture endpoint".into(),
                    author: None,
                    authored_at_ms: None,
                }],
                None,
                true,
                11,
            )
            .unwrap();
    }
    let comparison = store
        .comparison_between(
            account,
            repository,
            &oid('b'),
            &oid('d'),
            ComparisonMode::Direct,
            ComparisonOptions {
                ignore_whitespace: false,
                context_lines: 3,
            },
        )
        .unwrap();
    let before = fixture.requests.load(Ordering::Relaxed);
    let reader = ReaderService::new(accounts);
    let result = reader
        .fetch_history_diff(&comparison, &cancel)
        .await
        .unwrap();
    assert!(result.contacted && !result.complete && !result.has_more);
    let result = store
        .history_diff_page(account, &comparison, None, 10)
        .unwrap();
    assert_eq!(result.coverage, HistoryCoverage::Limited);
    let files: Vec<_> = result
        .files
        .iter()
        .filter_map(|row| match &row.record {
            PrRecord::File {
                path,
                status,
                previous_path,
                patch,
                ..
            } => {
                assert!(previous_path.is_none());
                Some((path.as_str(), status.as_str(), patch))
            }
            _ => None,
        })
        .collect();
    assert_eq!(files.len(), 4);
    assert_eq!(files[0], ("binary.bin", "added", &PatchEvidence::Binary));
    assert_eq!(files[1], ("capped.bin", "added", &PatchEvidence::ApiCapped));
    assert!(
        matches!(files[2], ("note.txt", "modified", PatchEvidence::Text(patch)) if patch.contains("-left\n+right\n"))
    );
    assert!(
        matches!(files[3], ("removed.txt", "removed", PatchEvidence::Text(patch)) if patch.contains("-left\n"))
    );
    let total = fixture.requests.load(Ordering::Relaxed) - before;
    assert_eq!(total, 13);

    store
        .retry_limited_history_diff(profile, &comparison)
        .unwrap();
    fixture.page_two_ready.store(true, Ordering::Relaxed);
    assert!(
        matches!(reader.fetch_history_diff(&comparison, &cancel).await,
        Err(ReaderError::Remote(error)) if error.kind == AccessKind::PermissionDenied)
    );
    let retained = store
        .history_diff_page(account, &comparison, None, 10)
        .unwrap();
    let prior: Vec<_> = result.files.iter().map(|row| &row.record).collect();
    assert_eq!(
        retained
            .files
            .iter()
            .map(|row| &row.record)
            .collect::<Vec<_>>(),
        prior
    );
}
