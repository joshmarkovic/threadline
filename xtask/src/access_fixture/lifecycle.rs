use super::*;
use std::time::Instant;

const COMMENT: &str = "Threadline lifecycle submitted comment.";
const COMMENTS: &str = "/repos/fixture-org/private/issues/1/comments";

fn active(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::LifecycleOffline
            | Phase::LifecycleReady
            | Phase::LifecycleDelay
            | Phase::LifecycleCrash
            | Phase::LifecycleRecovery
            | Phase::LifecycleSignedOut
            | Phase::LifecycleReturn
    )
}

pub(super) fn read_route(phase: Phase, path: &str) -> Option<Route> {
    if !active(phase) {
        return None;
    }
    backlog::read_route(Phase::BacklogAuth, path)
}

pub(super) fn post(phase: Phase, path: &str, body: &Value) -> Option<Route> {
    if !active(phase) || path != COMMENTS {
        return None;
    }
    Some(if *body == json!({"body": COMMENT}) {
        Route::LifecycleComment
    } else {
        Route::Unexpected
    })
}

pub(super) fn hold(
    phase: Phase,
    route: Route,
    phase_file: &Path,
    stopping: &AtomicBool,
) -> io::Result<Option<u16>> {
    let marker = match (phase, route) {
        (Phase::LifecycleDelay, Route::User) => "lifecycle-probe-held",
        (Phase::LifecycleCrash, Route::LifecycleComment) => "lifecycle-comment-held",
        _ => return Ok(None),
    };
    let marker = phase_file.with_file_name(marker);
    // The record is fixture-owned and contains no provider data or credential.
    fs::write(&marker, b"held\n")?;
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline && !stopping.load(Ordering::Acquire) {
        if let Ok(next) = Phase::read(phase_file)
            && next != phase
        {
            fs::remove_file(&marker)?;
            return Ok((phase == Phase::LifecycleCrash).then_some(0));
        }
        thread::sleep(Duration::from_millis(20));
    }
    fs::remove_file(&marker)?;
    Ok(Some(0))
}

pub(super) fn response(phase: Phase, route: Route) -> Option<Response> {
    if !active(phase) {
        return None;
    }
    if matches!(route, Route::UpdatedItems | Route::UpdatedComments) {
        return Some(Response {
            status: 200,
            reason: "OK",
            body: json!([]),
            sso: None,
        });
    }
    backlog::response(Phase::BacklogAuth, route)
}

pub(super) fn trace(records: &[RequestEvidence]) -> bool {
    records.iter().all(|entry| {
        entry.fixture_authorization
            && !matches!(
                entry.phase,
                Phase::LifecycleOffline | Phase::LifecycleSignedOut
            )
            && entry.route != Route::Unexpected
            && matches!(entry.status, 0 | 200)
    }) && records.iter().any(|entry| {
        entry.phase == Phase::LifecycleDelay && entry.route == Route::User && entry.status == 200
    }) && records
        .iter()
        .filter(|entry| entry.route == Route::LifecycleComment)
        .count()
        == 1
        && records.iter().any(|entry| {
            entry.phase == Phase::LifecycleCrash
                && entry.route == Route::LifecycleComment
                && entry.status == 0
        })
        && records.iter().any(|entry| {
            entry.phase == Phase::LifecycleRecovery
                && entry.route == Route::User
                && entry.status == 200
        })
        && [Route::User, Route::Repository].into_iter().all(|route| {
            records.iter().any(|entry| {
                entry.phase == Phase::LifecycleReturn && entry.route == route && entry.status == 200
            })
        })
        && !records.iter().any(|entry| {
            matches!(
                entry.route,
                Route::MergePut
                    | Route::ReviewCreated
                    | Route::ReviewSubmitted
                    | Route::CommentCreated
            )
        })
}

pub(super) fn scope_hold_trace(records: &[RequestEvidence]) -> bool {
    !records.is_empty()
        && records.iter().all(|entry| {
            entry.phase == Phase::LifecycleReady
                && entry.route == Route::User
                && entry.fixture_authorization
                && entry.status == 200
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_outbox_trace_requires_online_probe_and_refuses_all_other_traffic() {
        let row = |phase, route| RequestEvidence {
            phase,
            route,
            fixture_authorization: true,
            status: 200,
        };
        assert!(!scope_hold_trace(&[]));
        let online = row(Phase::LifecycleReady, Route::User);
        assert!(scope_hold_trace(std::slice::from_ref(&online)));
        for (phase, route) in [
            (Phase::LifecycleOffline, Route::User),
            (Phase::LifecycleReady, Route::Repository),
            (Phase::LifecycleReady, Route::IssueEditCommitted),
            (Phase::LifecycleReady, Route::ReviewCreated),
            (Phase::LifecycleReady, Route::MergePut),
        ] {
            assert!(!scope_hold_trace(&[
                row(Phase::LifecycleReady, Route::User),
                row(phase, route)
            ]));
        }
    }

    #[test]
    fn native_lifecycle_fixture_holds_an_authenticated_startup_and_records_one_lost_write() {
        let dir = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(dir.path()).unwrap();
        let address = fixture.address();
        let phase = fixture.phase_file();
        let request = |method: &str, route: &str, payload: &str| {
            let mut socket = TcpStream::connect(address).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            write!(
                socket,
                "{method} {route} HTTP/1.1\r\nHost: {address}\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\nContent-Length: {}\r\n\r\n{payload}",
                payload.len()
            ).unwrap();
            let mut output = String::new();
            socket.read_to_string(&mut output).unwrap();
            output
        };
        let marker = |name: &str| {
            let path = dir.path().join(name);
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline && !path.exists() {
                thread::sleep(Duration::from_millis(10));
            }
            assert!(path.exists(), "fixture never received {name}");
        };
        thread::scope(|scope| {
            fs::write(phase, "lifecycle-delay").unwrap();
            let first = scope.spawn(|| request("GET", "/user", ""));
            marker("lifecycle-probe-held");
            assert!(!first.is_finished());
            fs::write(phase, "lifecycle-ready").unwrap();
            assert!(first.join().unwrap().starts_with("HTTP/1.1 200"));

            fs::write(phase, "lifecycle-crash").unwrap();
            let mutation =
                scope.spawn(|| request("POST", COMMENTS, &json!({"body": COMMENT}).to_string()));
            marker("lifecycle-comment-held");
            assert!(!mutation.is_finished());
            fs::write(phase, "lifecycle-recovery").unwrap();
            assert!(mutation.join().unwrap().is_empty());
            assert!(request("GET", "/user", "").starts_with("HTTP/1.1 200"));
            fs::write(phase, "lifecycle-return").unwrap();
            assert!(request("GET", "/user", "").starts_with("HTTP/1.1 200"));
            assert!(request("GET", "/repos/fixture-org/private", "").starts_with("HTTP/1.1 200"));
        });
        fixture
            .finish(dir.path(), TraceCheck::NativeLifecycle)
            .unwrap();
        assert_eq!(
            post(Phase::LifecycleCrash, COMMENTS, &json!({"body":"other"})),
            Some(Route::Unexpected)
        );
    }
}
