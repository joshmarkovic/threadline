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
}

impl Phase {
    fn read(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path)?.trim() {
            "ready" => Ok(Self::Ready),
            "partial" => Ok(Self::Partial),
            "sso" => Ok(Self::Sso),
            "denied" => Ok(Self::Denied),
            "revoked" => Ok(Self::Revoked),
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

    pub fn finish(mut self, report_dir: &Path, require_journey: bool) -> io::Result<()> {
        let records = self.stop()?;
        let trace = serde_json::to_vec_pretty(&records)?;
        fs::write(report_dir.join("github-access-trace.json"), trace)?;
        if require_journey && !complete_journey(&records) {
            return Err(io::Error::other(
                "the synthetic GitHub trace did not cover the access journey",
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
                evidence.push(answer(&mut socket, phase)?);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(12));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(evidence)
}

fn answer(socket: &mut TcpStream, phase: Phase) -> io::Result<RequestEvidence> {
    let timeout = Some(Duration::from_secs(2));
    socket.set_read_timeout(timeout)?;
    socket.set_write_timeout(timeout)?;
    let (route, authorized) = read_request(socket)?;
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
    socket.write_all(b"\r\n")?;
    socket.write_all(&body)?;
    Ok(RequestEvidence {
        phase,
        route,
        fixture_authorization: authorized,
        status: reply.status,
    })
}

fn read_request(socket: &TcpStream) -> io::Result<(Route, bool)> {
    let mut reader = BufReader::new(socket.take(MAX_REQUEST));
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_ascii_whitespace();
    let mut route = match (parts.next(), parts.next(), parts.next()) {
        (Some("GET"), Some(USER), Some("HTTP/1.1")) => Route::User,
        (Some("GET"), Some(DISCOVERY), Some("HTTP/1.1")) => Route::Discovery,
        (Some("GET"), Some(REPOSITORY), Some("HTTP/1.1")) => Route::Repository,
        (Some("POST"), Some("/graphql"), Some("HTTP/1.1")) => Route::Graphql,
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
    if route == Route::Graphql {
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body)?;
        let authorized_query = serde_json::from_slice::<Value>(&body).is_ok_and(|body| {
            body.pointer("/variables/owner").and_then(Value::as_str) == Some("fixture-org")
                && body.pointer("/variables/name").and_then(Value::as_str) == Some("private")
                && body
                    .get("query")
                    .and_then(Value::as_str)
                    .is_some_and(|query| query.contains("RepositoryAccess"))
        });
        if !authorized_query {
            route = Route::Unexpected;
        }
    }
    Ok((route, authorized))
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
    let repository = json!({
        "id":71, "node_id":"R_71", "name":"private", "owner":{"login":"fixture-org"},
        "private":true, "permissions":{"pull":true,"push":false,"admin":false}
    });
    match (phase, route) {
        (_, Route::Unexpected) => {
            result(404, "Not Found", json!({"message":"Unknown fixture route"}))
        }
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
        fixture.finish(temp.path(), false).unwrap();
        let trace = fs::read_to_string(temp.path().join("github-access-trace.json")).unwrap();
        let records: Value = serde_json::from_str(&trace).unwrap();
        assert_eq!(
            records[0],
            json!({"phase":"partial","route":"discovery","fixture_authorization":true,"status":200})
        );
        assert!(!trace.contains("threadline-test-fixture"));
    }
}
