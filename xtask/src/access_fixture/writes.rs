use super::*;

const POST: &str = "/repos/fixture-org/private/issues/41/comments";
const CONFIRMED: &str = "Threadline S07 explicitly queued comment.";
const UNKNOWN: &str = "Threadline S07 accepted comment with a lost response.";

pub(super) fn post(phase: Phase, path: &str, payload: &Value) -> Route {
    if path != POST {
        return Route::Unexpected;
    }
    match phase {
        Phase::WriteOnline if *payload == json!({"body":CONFIRMED}) => Route::CommentCreated,
        Phase::WriteDrop if *payload == json!({"body":UNKNOWN}) => Route::CommentReplyLost,
        _ => Route::Unexpected,
    }
}

pub(super) fn response(phase: Phase, route: Route) -> Option<Response> {
    if !matches!(phase, Phase::WriteOnline | Phase::WriteDrop) {
        return None;
    }
    let issue = || {
        json!({
            "id":410,"node_id":"I_410","number":41,"title":"Existing issue for offline edits",
            "body":"Synthetic existing issue.","state":"open","locked":false,
            "created_at":"2018-01-01T00:00:00Z","updated_at":"2018-01-01T00:00:00Z",
            "repository_url":"https://api.github.com/repos/fixture-org/private",
            "user":{"id":41,"login":"fixture-user"},"labels":[],"assignees":[],"milestone":null
        })
    };
    let (status, reason, body) = match route {
        Route::Items => (200, "OK", json!([issue()])),
        Route::Item => (200, "OK", issue()),
        Route::UpdatedItems | Route::UpdatedComments | Route::CommentsFirst | Route::Timeline => {
            (200, "OK", json!([]))
        }
        Route::CommentCreated => (
            201,
            "Created",
            json!({
                "id":4101,"node_id":"IC_4101","body":CONFIRMED,"user":{"id":41,"login":"fixture-user"},
                "issue_url":"https://api.github.com/repos/fixture-org/private/issues/41",
                "created_at":"2018-01-01T01:00:00Z","updated_at":"2018-01-01T01:00:00Z"
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

pub(super) fn trace(records: &[RequestEvidence], include_loss: bool) -> bool {
    if records
        .iter()
        .any(|entry| entry.route == Route::Unexpected || !entry.fixture_authorization)
    {
        return false;
    }
    let effects: Vec<_> = records
        .iter()
        .filter(|record| {
            matches!(
                record.route,
                Route::CommentCreated | Route::CommentReplyLost
            )
        })
        .map(|record| (record.phase, record.route, record.status))
        .collect();
    let confirmed = [(Phase::WriteOnline, Route::CommentCreated, 201)];
    let expected = if include_loss {
        [confirmed[0], (Phase::WriteDrop, Route::CommentReplyLost, 0)].to_vec()
    } else {
        confirmed.to_vec()
    };
    records
        .iter()
        .any(|entry| entry.route == Route::Item && entry.status == 200)
        && effects == expected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sends_only_the_exact_scoped_comments_and_records_the_lost_response() {
        let fixture_root = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(fixture_root.path()).unwrap();
        let request = |method, path, payload: Value| {
            let bytes = payload.to_string();
            let mut socket = TcpStream::connect(fixture.address()).unwrap();
            write!(
                socket,
                "{method} {path} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\n"
            )
            .unwrap();
            if method == "POST" {
                write!(socket, "Content-Length: {}\r\n\r\n{bytes}", bytes.len()).unwrap();
            } else {
                write!(socket, "\r\n").unwrap();
            }
            let mut reply = String::new();
            socket.read_to_string(&mut reply).unwrap();
            reply
        };
        fs::write(fixture.phase_file(), "write-online").unwrap();
        assert!(request("GET", ITEM, Value::Null).contains("Existing issue for offline edits"));
        assert!(request("POST", POST, json!({"body":CONFIRMED})).starts_with("HTTP/1.1 201"));
        fs::write(fixture.phase_file(), "write-drop").unwrap();
        assert!(request("POST", POST, json!({"body":UNKNOWN})).is_empty());
        fixture
            .finish(fixture_root.path(), TraceCheck::WritesWithLoss)
            .unwrap();
        assert_eq!(
            post(
                Phase::WriteOnline,
                "/repos/fixture-org/private/issues",
                &json!({"body":CONFIRMED})
            ),
            Route::Unexpected
        );
        assert_eq!(
            post(
                Phase::WriteOnline,
                POST,
                &json!({"body":"unsent fixture draft"})
            ),
            Route::Unexpected
        );
    }
}
