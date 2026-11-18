use super::*;

const ISSUE: &str = "/repos/fixture-org/private/issues/1";
const PERMANENT: &str = "/repositories/71/issues/1";
const COMMENTS: &str = "/repos/fixture-org/private/issues/1/comments";
const PROBE: &str = "/repos/fixture-org/private/issues/1/comments?per_page=1&page=1";
const HELD: &str = "backlog-request-pending.txt";
const FIRST: u8 = 2;
const SENT: [u8; 9] = [FIRST, 4, 5, 6, 7, 8, 9, 10, 11];

pub(super) fn active(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::BacklogAuth | Phase::BacklogHold | Phase::BacklogDrain
    )
}

fn body(index: u8) -> String {
    format!("Threadline synthetic backlog action {index}.")
}

pub(super) fn read_route(phase: Phase, target: &str) -> Option<Route> {
    if !active(phase) {
        return None;
    }
    match target {
        ISSUE | PERMANENT => Some(Route::BacklogIssue),
        PROBE => Some(Route::BacklogPermission),
        _ => None,
    }
}

pub(super) fn post(phase: Phase, path: &str, payload: &Value) -> Option<Route> {
    if !active(phase) || path != COMMENTS {
        return None;
    }
    let index = SENT.into_iter().find(|index| {
        ((phase == Phase::BacklogHold && *index == FIRST)
            || (phase == Phase::BacklogDrain && *index != FIRST))
            && *payload == json!({"body":body(*index)})
    });
    Some(index.map_or(Route::Unexpected, Route::BacklogComment))
}

pub(super) fn response(phase: Phase, route: Route) -> Option<Response> {
    if !active(phase) {
        return None;
    }
    const TIME: &str = "2024-01-01T00:00:00Z";
    let (status, reason, body) = match route {
        Route::BacklogIssue => (
            200,
            "OK",
            json!({
                "id":1, "node_id":"I_1", "number":1, "title":"Investigate relay behavior 1",
                "body":"A cached fixture for offline **discussion** and search.", "state":"open", "locked":false,
                "created_at":TIME, "updated_at":TIME, "repository_url":"https://api.github.com/repos/fixture-org/private",
                "user":{"id":41,"login":"fixture-user"},"labels":[],"assignees":[],"milestone":null
            }),
        ),
        Route::BacklogPermission => (200, "OK", json!([])),
        Route::BacklogComment(index) => (
            201,
            "Created",
            json!({
                "id":71_000+u32::from(index),"node_id":format!("IC_{index}"),
                "body":body(index), "user":{"id":41,"login":"fixture-user"},
                "issue_url":"https://api.github.com/repos/fixture-org/private/issues/1",
                "created_at":TIME, "updated_at":TIME
            }),
        ),
        _ => return None,
    };
    Some(Response {
        status,
        reason,
        body,
        sso: None,
    })
}

pub(super) fn await_release(phase_file: &Path, stopping: &AtomicBool) -> io::Result<bool> {
    use std::time::Instant;
    let marker = phase_file.with_file_name(HELD);
    fs::write(&marker, b"2\n")?;
    let start = Instant::now();
    let mut released = false;
    while !stopping.load(Ordering::Acquire) && start.elapsed() < Duration::from_secs(12) {
        if Phase::read(phase_file).ok() == Some(Phase::BacklogDrain) {
            released = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    fs::remove_file(marker)?;
    Ok(released)
}

pub(super) fn trace(records: &[RequestEvidence]) -> bool {
    let writes: Vec<_> = records
        .iter()
        .filter_map(|row| match row.route {
            Route::BacklogComment(index) if row.status == 201 => Some((row.phase, index)),
            _ => None,
        })
        .collect();
    let expected = SENT.map(|index| {
        (
            if index == FIRST {
                Phase::BacklogHold
            } else {
                Phase::BacklogDrain
            },
            index,
        )
    });
    writes == expected
        && records.iter().all(|row| {
            row.fixture_authorization
                && match row.phase {
                    Phase::BacklogAuth => {
                        matches!(row.route, Route::User | Route::Repository) && row.status == 200
                    }
                    Phase::BacklogHold | Phase::BacklogDrain => match row.route {
                        Route::User
                        | Route::Repository
                        | Route::BacklogIssue
                        | Route::BacklogPermission => row.status == 200,
                        Route::BacklogComment(_) => row.status == 201,
                        _ => false,
                    },
                    _ => false,
                }
        })
        && records
            .iter()
            .any(|row| row.phase == Phase::BacklogAuth && row.route == Route::Repository)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backlog_fixture_rejects_cancelled_dependent_duplicate_and_unrelated_payloads() {
        assert_eq!(
            post(Phase::BacklogHold, COMMENTS, &json!({"body":body(2)})),
            Some(Route::BacklogComment(2))
        );
        for index in [0, 1, 2, 3, 12] {
            assert_eq!(
                post(Phase::BacklogDrain, COMMENTS, &json!({"body":body(index)})),
                Some(Route::Unexpected)
            );
        }
        assert_eq!(
            post(
                Phase::BacklogDrain,
                "/repos/elsewhere/private/issues/1/comments",
                &json!({"body":body(4)})
            ),
            None
        );
        assert_eq!(
            post(Phase::BacklogAuth, COMMENTS, &json!({"body":body(2)})),
            Some(Route::Unexpected)
        );
        assert_eq!(
            post(
                Phase::BacklogDrain,
                COMMENTS,
                &json!({"body":body(4),"extra":true})
            ),
            Some(Route::Unexpected)
        );
    }
    #[test]
    fn backlog_loopback_holds_one_response_and_persists_exact_drain_evidence() {
        use std::time::Instant;

        let profile = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(profile.path()).unwrap();
        let phase = fixture.phase_file().to_owned();
        let request = |first_line: &str, payload: Option<Value>| {
            let mut socket = TcpStream::connect(fixture.address()).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let content = payload.map_or(String::new(), |body| body.to_string());
            write!(socket, "{first_line} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\nContent-Length: {}\r\n\r\n{content}", content.len()).unwrap();
            socket
        };
        let read = |mut socket: TcpStream| {
            let mut reply = String::new();
            socket.read_to_string(&mut reply).unwrap();
            reply
        };
        fs::write(&phase, "backlog-auth").unwrap();
        let checked = read(request("GET /repos/fixture-org/private", None));
        assert!(checked.starts_with("HTTP/1.1 200"));

        fs::write(&phase, "backlog-hold").unwrap();
        let first = request(
            &format!("POST {COMMENTS}"),
            Some(json!({"body":body(FIRST)})),
        );
        let marker = phase.with_file_name(HELD);
        let deadline = Instant::now() + Duration::from_secs(3);
        while !marker.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(marker.exists());
        first.set_nonblocking(true).unwrap();
        assert_eq!(
            first.peek(&mut [0]).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        first.set_nonblocking(false).unwrap();
        fs::write(&phase, "backlog-drain").unwrap();
        assert!(read(first).starts_with("HTTP/1.1 201"));
        assert!(!marker.exists());
        for index in SENT.into_iter().skip(1) {
            let reply = read(request(
                &format!("POST {COMMENTS}"),
                Some(json!({"body":body(index)})),
            ));
            assert!(reply.starts_with("HTTP/1.1 201"));
            assert!(reply.contains(&format!("SYNTHETIC:71:{index:02}")));
        }
        fixture
            .finish(profile.path(), TraceCheck::LargeWorkspaceBacklog)
            .unwrap();
        let receipt = fs::read(profile.path().join("github-access-trace.json")).unwrap();
        assert!(!contains_fixture_credential(&receipt));
    }
}
