use super::*;

const POST: &str = "/repos/fixture-org/private/issues/41/comments";
const CONFIRMED: &str = "Threadline S07 explicitly queued comment.";
const UNKNOWN: &str = "Threadline S07 accepted comment with a lost response.";

pub(super) fn post(phase: Phase, path: &str, payload: &Value) -> Route {
    if path != POST {
        return Route::Unexpected;
    }
    match phase {
        Phase::WriteOnline | Phase::CredentialReturn | Phase::SyncNotificationSend
            if *payload == json!({"body":CONFIRMED}) =>
        {
            Route::CommentCreated
        }
        Phase::WriteDrop if *payload == json!({"body":UNKNOWN}) => Route::CommentReplyLost,
        _ => Route::Unexpected,
    }
}

pub(super) fn response(phase: Phase, route: Route) -> Option<Response> {
    if !matches!(
        phase,
        Phase::WriteOnline
            | Phase::WriteDrop
            | Phase::IssueEditBaseline
            | Phase::IssueEditRemoteChange
            | Phase::SyncNotificationSend
            | Phase::CredentialReady
            | Phase::CredentialOther
            | Phase::CredentialReturn
    ) {
        return None;
    }
    let issue = || {
        let (title, body, stamp) = if phase == Phase::IssueEditRemoteChange {
            (
                "Remote change to the same issue",
                "The original issue was changed remotely.",
                "2018-01-01T02:00:00Z",
            )
        } else {
            (
                "Existing issue for offline edits",
                "Synthetic existing issue.",
                "2018-01-01T00:00:00Z",
            )
        };
        json!({
            "id":410,"node_id":"I_410","number":41,"title":title,
            "body":body,"state":"open","locked":false,
            "created_at":"2018-01-01T00:00:00Z","updated_at":stamp,
            "repository_url":"https://api.github.com/repos/fixture-org/private",
            "user":{"id":41,"login":"fixture-user"},"labels":[],"assignees":[],"milestone":null
        })
    };
    let (status, reason, body) = match route {
        Route::Items => (200, "OK", json!([issue()])),
        Route::Item | Route::ScheduledIssue => (200, "OK", issue()),
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

pub(super) fn issue_conflict_trace(records: &[RequestEvidence]) -> bool {
    let observed = |phase, route| {
        records
            .iter()
            .any(|entry| entry.phase == phase && entry.route == route)
    };
    observed(Phase::IssueEditBaseline, Route::Items)
        && observed(Phase::IssueEditRemoteChange, Route::Item)
        && records.iter().all(|entry| {
            entry.fixture_authorization
                && entry.status == 200
                && matches!(
                    entry.phase,
                    Phase::Ready | Phase::IssueEditBaseline | Phase::IssueEditRemoteChange
                )
                && matches!(
                    entry.route,
                    Route::User
                        | Route::Repository
                        | Route::Items
                        | Route::Item
                        | Route::ScheduledIssue
                        | Route::CommentsFirst
                        | Route::Timeline
                        | Route::OpenPullIndex
                        | Route::UpdatedItems
                        | Route::UpdatedComments
                        | Route::UpdatedInlineComments
                )
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
    fn issue_conflict_preserves_provider_identity_and_refuses_any_mutation() {
        let base = response(Phase::IssueEditBaseline, Route::Item)
            .unwrap()
            .body;
        let remote = response(Phase::IssueEditRemoteChange, Route::Item)
            .unwrap()
            .body;
        for key in ["id", "node_id", "number", "repository_url"] {
            assert_eq!(base[key], remote[key]);
        }
        assert_ne!(base["title"], remote["title"]);
        assert_ne!(base["body"], remote["body"]);

        let mut observed = vec![
            RequestEvidence {
                phase: Phase::IssueEditBaseline,
                route: Route::Items,
                fixture_authorization: true,
                status: 200,
            },
            RequestEvidence {
                phase: Phase::IssueEditRemoteChange,
                route: Route::Item,
                fixture_authorization: true,
                status: 200,
            },
        ];
        assert!(issue_conflict_trace(&observed));

        let fixture_root = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(fixture_root.path()).unwrap();
        fs::write(fixture.phase_file(), "issue-edit-remote-change").unwrap();
        for (method, path) in [
            ("PATCH", ITEM),
            ("POST", "/repos/fixture-org/private/issues"),
        ] {
            let body = r#"{"title":"Wrongly sent during preflight"}"#;
            let mut socket = TcpStream::connect(fixture.address()).unwrap();
            write!(socket, "{method} {path} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let mut reply = String::new();
            socket.read_to_string(&mut reply).unwrap();
            assert!(reply.starts_with("HTTP/1.1 404"));
        }
        let fixture_run = fixture.finish(fixture_root.path(), TraceCheck::Observe);
        assert!(fixture_run.is_ok());
        observed.push(RequestEvidence {
            phase: Phase::IssueEditRemoteChange,
            route: Route::Unexpected,
            fixture_authorization: true,
            status: 404,
        });
        assert!(!issue_conflict_trace(&observed));
    }

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
        assert!(
            request("GET", SCHEDULED_ISSUE, Value::Null)
                .contains("Existing issue for offline edits")
        );
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
