use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use serde::Serialize;
use serde_json::{Value, json};

mod pr;
mod writes;

const MAX_REQUEST: u64 = 16 * 1024;
const FIXTURE_AUTHORIZATION: &str = "Bearer threadline-test-fixture";

pub fn contains_fixture_credential(bytes: &[u8]) -> bool {
    let credential = FIXTURE_AUTHORIZATION
        .strip_prefix("Bearer ")
        .expect("fixed scheme")
        .as_bytes();
    bytes
        .windows(credential.len())
        .any(|window| window == credential)
}

const USER: &str = "/user";
const DISCOVERY: &str = "/user/repos?per_page=100&page=1";
const REPOSITORY: &str = "/repos/fixture-org/private";
const ITEMS: &str =
    "/repos/fixture-org/private/issues?state=all&sort=created&direction=desc&per_page=20&page=1";
const ITEM: &str = "/repos/fixture-org/private/issues/41";
const SCHEDULED_ISSUE: &str = "/repositories/71/issues/41";
const COMMENTS_FIRST: &str = "/repos/fixture-org/private/issues/41/comments?per_page=20&page=1";
const COMMENTS_NEXT: &str = "/repos/fixture-org/private/issues/41/comments?per_page=20&page=2";
const TIMELINE: &str = "/repos/fixture-org/private/issues/41/timeline?per_page=20&page=1";
const SYNC_ITEMS: &str = "/repositories/71/issues?state=all&sort=updated&direction=asc&since=";
const SYNC_COMMENTS: &str = "/repositories/71/issues/comments?sort=updated&direction=asc&since=";
const AUTHORIZATION_URL: &str =
    "https://github.com/orgs/fixture-org/sso?authorization_request=fixture123";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Ready,
    Partial,
    Sso,
    Denied,
    Revoked,
    ReaderFailure,
    ReaderResume,
    SyncLive,
    SyncOffline,
    SyncService,
    SyncThrottle,
    SyncTransport,
    PrLoad,
    PrDelay,
    PrRelease,
    WriteOnline,
    WriteDrop,
}

impl Phase {
    fn read(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path)?.trim() {
            "ready" => Ok(Self::Ready),
            "partial" => Ok(Self::Partial),
            "sso" => Ok(Self::Sso),
            "denied" => Ok(Self::Denied),
            "revoked" => Ok(Self::Revoked),
            "reader-failure" => Ok(Self::ReaderFailure),
            "reader-resume" => Ok(Self::ReaderResume),
            "sync-live" => Ok(Self::SyncLive),
            "sync-offline" => Ok(Self::SyncOffline),
            "sync-service" => Ok(Self::SyncService),
            "sync-throttle" => Ok(Self::SyncThrottle),
            "sync-transport" => Ok(Self::SyncTransport),
            "pr-load" => Ok(Self::PrLoad),
            "pr-delay" => Ok(Self::PrDelay),
            "pr-release" => Ok(Self::PrRelease),
            "write-online" => Ok(Self::WriteOnline),
            "write-drop" => Ok(Self::WriteDrop),
            _ => Err(io::Error::other("invalid access fixture phase")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Route {
    User,
    Discovery,
    Repository,
    Graphql,
    Items,
    Item,
    ScheduledIssue,
    CommentsFirst,
    CommentsNext,
    Timeline,
    UpdatedItems,
    UpdatedComments,
    PullA,
    PullB,
    CompareA,
    CompareB,
    CommentCreated,
    CommentReplyLost,
    Unexpected,
}

#[derive(Serialize)]
struct RequestEvidence {
    phase: Phase,
    route: Route,
    fixture_authorization: bool,
    status: u16,
}

pub struct AccessFixture {
    address: SocketAddr,
    phase_file: PathBuf,
    stopping: Arc<AtomicBool>,
    worker: Option<JoinHandle<io::Result<Vec<RequestEvidence>>>>,
}

pub enum TraceCheck {
    Observe,
    AccountJourney,
    OfflineReader,
    ReaderResume,
    SyncJourney,
    PullRequestJourney,
    WritesWithLoss,
    WriteSurface,
}

impl AccessFixture {
    pub fn start(profile: &Path) -> io::Result<Self> {
        let phase_file = profile.join("access-fixture-phase.txt");
        fs::write(&phase_file, "ready\n")?;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let stopping = Arc::new(AtomicBool::new(false));
        let worker = thread::Builder::new()
            .name("access-fixture".into())
            .spawn({
                let phase_file = phase_file.clone();
                let stopping = stopping.clone();
                move || serve(listener, &phase_file, &stopping)
            })?;
        Ok(Self {
            address,
            phase_file,
            stopping,
            worker: Some(worker),
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn phase_file(&self) -> &Path {
        &self.phase_file
    }

    pub fn finish(mut self, report_dir: &Path, check: TraceCheck) -> io::Result<()> {
        let records = self.stop()?;
        let trace = serde_json::to_vec_pretty(&records)?;
        fs::write(report_dir.join("github-access-trace.json"), trace)?;
        let valid = match check {
            TraceCheck::Observe => true,
            TraceCheck::AccountJourney => complete_journey(&records),
            TraceCheck::OfflineReader => records.is_empty(),
            TraceCheck::ReaderResume => complete_reader_resume(&records),
            TraceCheck::SyncJourney => complete_sync_journey(&records),
            TraceCheck::PullRequestJourney => pr::trace(&records),
            TraceCheck::WritesWithLoss => writes::trace(&records, true),
            TraceCheck::WriteSurface => writes::trace(&records, false),
        };
        if !valid {
            return Err(io::Error::other(
                "the synthetic GitHub trace did not match the expected journey",
            ));
        }
        Ok(())
    }

    fn stop(&mut self) -> io::Result<Vec<RequestEvidence>> {
        self.stopping.store(true, Ordering::Release);
        match self.worker.take() {
            Some(worker) => worker
                .join()
                .map_err(|_| io::Error::other("access fixture worker stopped unexpectedly"))?,
            None => Ok(Vec::new()),
        }
    }
}

fn complete_reader_resume(records: &[RequestEvidence]) -> bool {
    let discussion: Vec<_> = records
        .iter()
        .filter(|entry| matches!(entry.route, Route::CommentsFirst | Route::CommentsNext))
        .map(|entry| (entry.phase, entry.route, entry.status))
        .collect();
    records
        .iter()
        .all(|entry| entry.route != Route::Unexpected && entry.fixture_authorization)
        && records
            .iter()
            .any(|entry| entry.route == Route::Items && entry.status == 200)
        && discussion
            == [
                (Phase::ReaderFailure, Route::CommentsFirst, 200),
                (Phase::ReaderFailure, Route::CommentsNext, 502),
                (Phase::ReaderResume, Route::CommentsNext, 200),
            ]
}

fn complete_sync_journey(records: &[RequestEvidence]) -> bool {
    let promised = |phase, route, status| {
        records
            .iter()
            .any(|entry| entry.phase == phase && entry.route == route && entry.status == status)
    };
    records.iter().all(|entry| {
        entry.route != Route::Unexpected
            && entry.phase != Phase::SyncOffline
            && entry.fixture_authorization
    }) && promised(Phase::SyncLive, Route::UpdatedItems, 200)
        && promised(Phase::SyncLive, Route::UpdatedComments, 200)
        && promised(Phase::SyncService, Route::User, 503)
        && promised(Phase::SyncThrottle, Route::User, 429)
        && promised(Phase::SyncTransport, Route::User, 0)
}

impl Drop for AccessFixture {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn complete_journey(records: &[RequestEvidence]) -> bool {
    let observed = |phase, route, status| {
        records.iter().any(|entry| {
            entry.phase == phase
                && entry.route == route
                && entry.status == status
                && entry.fixture_authorization
        })
    };
    records
        .iter()
        .all(|entry| entry.route != Route::Unexpected && entry.fixture_authorization)
        && observed(Phase::Ready, Route::User, 200)
        && observed(Phase::Ready, Route::Repository, 200)
        && observed(Phase::Partial, Route::Discovery, 200)
        && observed(Phase::Sso, Route::Repository, 403)
        && observed(Phase::Denied, Route::Repository, 403)
        && observed(Phase::Revoked, Route::User, 401)
}

fn serve(
    listener: TcpListener,
    phase_file: &Path,
    stopping: &AtomicBool,
) -> io::Result<Vec<RequestEvidence>> {
    let mut evidence = Vec::new();
    while !stopping.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut socket, _)) => {
                if evidence.len() >= 128 {
                    return Err(io::Error::other("too many requests to the access fixture"));
                }
                let phase = Phase::read(phase_file)?;
                evidence.push(answer(&mut socket, phase, phase_file, stopping)?);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(12));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(evidence)
}

fn answer(
    socket: &mut TcpStream,
    phase: Phase,
    phase_file: &Path,
    stopping: &AtomicBool,
) -> io::Result<RequestEvidence> {
    let timeout = Some(Duration::from_secs(2));
    socket.set_nonblocking(false)?;
    socket.set_read_timeout(timeout)?;
    socket.set_write_timeout(timeout)?;
    let (route, authorized) = read_request(socket, phase)?;
    if phase == Phase::PrDelay && route == Route::Item && authorized {
        pr::wait_for_release(phase_file, stopping)?;
    }
    if authorized
        && ((phase == Phase::SyncTransport && route == Route::User)
            || route == Route::CommentReplyLost)
    {
        return Ok(RequestEvidence {
            phase,
            route,
            fixture_authorization: true,
            status: 0,
        });
    }
    let reply = response(phase, route, authorized);
    let body = serde_json::to_vec(&reply.body)?;
    write!(
        socket,
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        reply.status,
        reply.reason,
        body.len()
    )?;
    if let Some(sso) = reply.sso {
        write!(socket, "X-GitHub-SSO: {sso}\r\n")?;
    }
    if route == Route::CommentCreated && reply.status == 201 {
        socket.write_all(b"X-GitHub-Request-Id: SYNTHETIC:4101:07\r\n")?;
    }
    if phase == Phase::SyncThrottle && route == Route::User && reply.status == 429 {
        socket.write_all(b"Retry-After: 2\r\n")?;
    }
    if phase == Phase::ReaderFailure && route == Route::CommentsFirst && reply.status == 200 {
        let local = socket.local_addr()?;
        write!(
            socket,
            "Link: <http://{local}{COMMENTS_NEXT}>; rel=\"next\"\r\n"
        )?;
    }
    socket.write_all(b"\r\n")?;
    socket.write_all(&body)?;
    Ok(RequestEvidence {
        phase,
        route,
        fixture_authorization: authorized,
        status: reply.status,
    })
}

fn read_request(socket: &TcpStream, phase: Phase) -> io::Result<(Route, bool)> {
    let mut reader = BufReader::new(socket.take(MAX_REQUEST));
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_ascii_whitespace();
    let request = (parts.next(), parts.next(), parts.next());
    let post_path = match request {
        (Some("POST"), Some(path), Some("HTTP/1.1")) => Some(path.to_owned()),
        _ => None,
    };
    let mut route = match request {
        (Some("GET"), Some(USER), Some("HTTP/1.1")) => Route::User,
        (Some("GET"), Some(DISCOVERY), Some("HTTP/1.1")) => Route::Discovery,
        (Some("GET"), Some(REPOSITORY), Some("HTTP/1.1")) => Route::Repository,
        (Some("GET"), Some(ITEMS), Some("HTTP/1.1")) => Route::Items,
        (Some("GET"), Some(ITEM), Some("HTTP/1.1")) => Route::Item,
        (Some("GET"), Some(SCHEDULED_ISSUE), Some("HTTP/1.1"))
            if matches!(phase, Phase::WriteOnline | Phase::WriteDrop) =>
        {
            Route::ScheduledIssue
        }
        (Some("GET"), Some(COMMENTS_FIRST), Some("HTTP/1.1")) => Route::CommentsFirst,
        (Some("GET"), Some(COMMENTS_NEXT), Some("HTTP/1.1")) => Route::CommentsNext,
        (Some("GET"), Some(TIMELINE), Some("HTTP/1.1")) => Route::Timeline,
        (Some("GET"), Some(target), Some("HTTP/1.1")) if sync_since(target, SYNC_ITEMS) => {
            Route::UpdatedItems
        }
        (Some("GET"), Some(target), Some("HTTP/1.1")) if sync_since(target, SYNC_COMMENTS) => {
            Route::UpdatedComments
        }
        (Some("POST"), Some("/graphql"), Some("HTTP/1.1")) => Route::Graphql,
        (Some("GET"), Some(target), Some("HTTP/1.1")) => {
            pr::route(target).unwrap_or(Route::Unexpected)
        }
        _ => Route::Unexpected,
    };
    let mut authorized = false;
    let mut content_length = 0usize;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Err(io::Error::other("incomplete access fixture request"));
        }
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("authorization") {
                authorized = value.trim() == FIXTURE_AUTHORIZATION;
            }
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .filter(|length| *length <= MAX_REQUEST as usize)
                    .ok_or_else(|| io::Error::other("invalid access fixture body length"))?;
            }
        }
    }
    if let Some(post_path) = post_path {
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body)?;
        let payload = serde_json::from_slice::<Value>(&body).ok();
        if route != Route::Graphql {
            route = payload.as_ref().map_or(Route::Unexpected, |body| {
                writes::post(phase, &post_path, body)
            });
        } else if !payload.is_some_and(|body| {
            body.pointer("/variables/owner").and_then(Value::as_str) == Some("fixture-org")
                && body.pointer("/variables/name").and_then(Value::as_str) == Some("private")
                && body
                    .get("query")
                    .and_then(Value::as_str)
                    .is_some_and(|query| query.contains("RepositoryAccess"))
        }) {
            route = Route::Unexpected;
        }
    }
    Ok((route, authorized))
}

fn sync_since(target: &str, prefix: &str) -> bool {
    let Some(value) = target
        .strip_prefix(prefix)
        .and_then(|s| s.strip_suffix("&per_page=20&page=1"))
    else {
        return false;
    };
    let bytes = value.as_bytes();
    // Match the fixed encoded UTC form without accepting additional query clauses.
    let structure = b"0000-00-00T00%3A00%3A00Z";
    bytes.len() == structure.len()
        && bytes.iter().zip(structure).all(|(value, pattern)| {
            if *pattern == b'0' {
                value.is_ascii_digit()
            } else {
                value == pattern
            }
        })
}

struct Response {
    status: u16,
    reason: &'static str,
    body: Value,
    sso: Option<String>,
}

fn response(phase: Phase, route: Route, authorized: bool) -> Response {
    let result = |status, reason, body| Response {
        status,
        reason,
        body,
        sso: None,
    };
    if !authorized || phase == Phase::Revoked {
        return result(401, "Unauthorized", json!({"message":"Bad credentials"}));
    }
    if let Some(pr) = pr::response(phase, route) {
        return pr;
    }
    if let Some(write) = writes::response(phase, route) {
        return write;
    }
    let repository = json!({
        "id":71, "node_id":"R_71", "name":"private", "owner":{"login":"fixture-org"},
        "private":true, "permissions":{"pull":true,"push":false,"admin":false}
    });
    let item = || {
        json!({
            "id":410, "node_id":"I_410", "number":41, "title":"Reader pagination fixture",
            "state":"closed", "body":"A downloaded fixture issue.", "user":{"login":"fixture-user"},
            "created_at":"2018-01-01T00:00:00Z", "updated_at":"2018-01-01T00:00:00Z"
        })
    };
    let comment = |id, day, body| {
        json!({
            "id":id, "node_id":format!("IC_{id}"), "body":body, "user":{"login":"fixture-user"},
            "created_at":format!("2018-01-{day:02}T00:00:00Z"),
            "updated_at":format!("2018-01-{day:02}T00:00:00Z")
        })
    };
    match (phase, route) {
        (_, Route::Unexpected) => {
            result(404, "Not Found", json!({"message":"Unknown fixture route"}))
        }
        (Phase::SyncService, Route::User) => result(
            503,
            "Service Unavailable",
            json!({"message":"Fixture service unavailable"}),
        ),
        (Phase::SyncThrottle, Route::User) => result(
            429,
            "Too Many Requests",
            json!({"message":"Fixture throttled"}),
        ),
        (_, Route::User) => result(200, "OK", json!({"id":41,"login":"fixture-user"})),
        (Phase::Partial, Route::Discovery) => Response {
            sso: Some("partial-results; organizations=123".into()),
            ..result(200, "OK", json!([repository]))
        },
        (_, Route::Discovery) => result(200, "OK", json!([repository])),
        (Phase::Sso, Route::Repository) => Response {
            sso: Some(format!("required; url={AUTHORIZATION_URL}")),
            ..result(
                403,
                "Forbidden",
                json!({"message":"Organization SSO authorization required"}),
            )
        },
        (Phase::Denied, Route::Repository) => {
            result(403, "Forbidden", json!({"message":"Access denied"}))
        }
        (_, Route::Repository) => result(200, "OK", repository),
        (Phase::Denied | Phase::Sso, Route::Graphql) => result(
            200,
            "OK",
            json!({"data":{"repository":null},"errors":[{"type":"FORBIDDEN"}]}),
        ),
        (_, Route::Graphql) => result(
            200,
            "OK",
            json!({"data":{"repository":{
                "id":"R_71","name":"private","owner":{"login":"fixture-org"},"isPrivate":true,"viewerPermission":"READ"
            }}}),
        ),
        (Phase::ReaderFailure | Phase::ReaderResume, Route::Items) => {
            result(200, "OK", json!([item()]))
        }
        (Phase::ReaderFailure | Phase::ReaderResume, Route::Item) => result(200, "OK", item()),
        (Phase::ReaderFailure, Route::CommentsFirst) => result(
            200,
            "OK",
            json!([comment(5101, 2, "Downloaded first page.")]),
        ),
        (Phase::ReaderFailure, Route::CommentsNext) => result(
            502,
            "Bad Gateway",
            json!({"message":"Fixture page two failed"}),
        ),
        (Phase::ReaderResume, Route::CommentsNext) => result(
            200,
            "OK",
            json!([comment(5102, 3, "Recovered second page.")]),
        ),
        (Phase::ReaderFailure | Phase::ReaderResume, Route::Timeline) => {
            result(200, "OK", json!([]))
        }
        (Phase::SyncLive, Route::UpdatedItems) => {
            let mut current = item();
            current["repository_url"] = json!("https://api.github.com/repos/fixture-org/private");
            result(200, "OK", json!([current]))
        }
        (Phase::SyncLive, Route::UpdatedComments) => {
            let mut current = comment(5151, 4, "Observed ordinary conversation.");
            current["issue_url"] =
                json!("https://api.github.com/repos/fixture-org/private/issues/41");
            result(200, "OK", json!([current]))
        }
        _ => result(
            404,
            "Not Found",
            json!({"message":"Unexpected fixture phase"}),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_binds_loopback_and_exports_only_redacted_access_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(temp.path()).unwrap();
        assert!(fixture.address().ip().is_loopback());
        fs::write(fixture.phase_file(), "partial").unwrap();
        let mut socket = TcpStream::connect(fixture.address()).unwrap();
        write!(
            socket,
            "GET {DISCOVERY} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\n\r\n"
        )
        .unwrap();
        let mut reply = String::new();
        socket.read_to_string(&mut reply).unwrap();
        assert!(reply.contains("X-GitHub-SSO: partial-results; organizations=123"));
        fixture.finish(temp.path(), TraceCheck::Observe).unwrap();
        let trace = fs::read_to_string(temp.path().join("github-access-trace.json")).unwrap();
        let records: Value = serde_json::from_str(&trace).unwrap();
        assert_eq!(
            records[0],
            json!({"phase":"partial","route":"discovery","fixture_authorization":true,"status":200})
        );
        assert!(!trace.contains("threadline-test-fixture"));
    }

    #[test]
    fn reader_fixture_rejects_repeating_the_first_comment_page_on_resume() {
        let temp = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(temp.path()).unwrap();
        let fetch = |path: &str| {
            let mut socket = TcpStream::connect(fixture.address()).unwrap();
            write!(
                socket,
                "GET {path} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\n\r\n"
            )
            .unwrap();
            let mut reply = String::new();
            socket.read_to_string(&mut reply).unwrap();
            reply
        };
        fs::write(fixture.phase_file(), "reader-failure").unwrap();
        assert!(fetch(ITEMS).contains("Reader pagination fixture"));
        assert!(
            fetch(COMMENTS_FIRST).contains(&format!("http://{}{COMMENTS_NEXT}", fixture.address()))
        );
        assert!(fetch(COMMENTS_NEXT).starts_with("HTTP/1.1 502"));
        fs::write(fixture.phase_file(), "reader-resume").unwrap();
        assert!(fetch(COMMENTS_NEXT).contains("Recovered second page"));
        fixture
            .finish(temp.path(), TraceCheck::ReaderResume)
            .unwrap();

        let mut records: Vec<RequestEvidence> = [
            (Phase::ReaderFailure, Route::Items, 200),
            (Phase::ReaderFailure, Route::CommentsFirst, 200),
            (Phase::ReaderFailure, Route::CommentsNext, 502),
            (Phase::ReaderResume, Route::CommentsNext, 200),
        ]
        .into_iter()
        .map(|(phase, route, status)| RequestEvidence {
            phase,
            route,
            status,
            fixture_authorization: true,
        })
        .collect();
        assert!(complete_reader_resume(&records));
        records.push(RequestEvidence {
            phase: Phase::ReaderResume,
            route: Route::CommentsFirst,
            status: 404,
            fixture_authorization: true,
        });
        assert!(!complete_reader_resume(&records));
    }

    #[test]
    fn sync_journey_replays_concrete_http_failures_and_rejects_offline_traffic() {
        let dir = tempfile::tempdir().unwrap();
        let mut fixture = AccessFixture::start(dir.path()).unwrap();
        let since = "2026-11-18T20%3A11%3A12Z&per_page=20&page=1";
        let items = format!("{SYNC_ITEMS}{since}");
        let comments = format!("{SYNC_COMMENTS}{since}");
        assert!(!sync_since(&format!("{items}&unexpected=1"), SYNC_ITEMS));
        for (phase, route, expected) in [
            ("sync-live", items.as_str(), "200 OK"),
            ("sync-live", comments.as_str(), "200 OK"),
            ("sync-service", USER, "503 Service Unavailable"),
            ("sync-throttle", USER, "429 Too Many Requests"),
            ("sync-transport", USER, ""),
        ] {
            fs::write(fixture.phase_file(), phase).unwrap();
            let mut socket = TcpStream::connect(fixture.address()).unwrap();
            write!(
                socket,
                "GET {route} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\n\r\n"
            )
            .unwrap();
            let mut reply = String::new();
            socket.read_to_string(&mut reply).unwrap();
            if expected.is_empty() {
                assert!(reply.is_empty());
            } else {
                assert!(reply.starts_with(&format!("HTTP/1.1 {expected}")));
            }
            if phase == "sync-throttle" {
                assert!(reply.contains("Retry-After: 2\r\n"));
            }
        }
        let mut records = fixture.stop().unwrap();
        assert!(complete_sync_journey(&records));
        records.push(RequestEvidence {
            phase: Phase::SyncOffline,
            route: Route::User,
            status: 200,
            fixture_authorization: true,
        });
        assert!(!complete_sync_journey(&records));
    }
}
