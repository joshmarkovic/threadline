use super::*;

const CREATE: &str = "/repos/fixture-org/private/issues/41/comments";
const EXACT: &str = "/repos/fixture-org/private/issues/comments/4101";
const ORIGINAL: &str = "Threadline dependency original comment.";
const EDIT: &str = "Threadline dependency revised comment.";
const LOST: &str = "Threadline dependency comment with a lost response.";

fn active(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::CommentDependencyOffline
            | Phase::CommentDependencyCreate
            | Phase::CommentDependencyResume
            | Phase::CommentDependencyLost
    )
}

pub(super) fn get(phase: Phase, target: &str) -> Option<Route> {
    if !active(phase) {
        return None;
    }
    match target {
        SCHEDULED_ISSUE => Some(Route::ScheduledIssue),
        EXACT => Some(Route::DependencyCommentRead),
        _ => None,
    }
}

pub(super) fn post(phase: Phase, path: &str, body: &Value) -> Option<Route> {
    if !active(phase) || path != CREATE {
        return None;
    }
    Some(match phase {
        Phase::CommentDependencyCreate if *body == json!({"body":ORIGINAL}) => {
            Route::DependencyCommentCreated
        }
        Phase::CommentDependencyLost if *body == json!({"body":LOST}) => {
            Route::DependencyCommentLost
        }
        _ => Route::Unexpected,
    })
}

pub(super) fn patch(phase: Phase, path: &str, body: &Value) -> Route {
    if phase == Phase::CommentDependencyResume && path == EXACT && *body == json!({"body":EDIT}) {
        Route::DependencyCommentEdited
    } else {
        Route::Unexpected
    }
}

pub(super) fn classify_unexpected_request(
    phase: Phase,
    method: Option<&str>,
    target: Option<&str>,
) -> Option<Route> {
    if phase != Phase::CommentDependencyResume {
        return None;
    }
    let target = target?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    match method? {
        "GET" | "PATCH" if path == EXACT && !query.is_empty() => {
            Some(Route::DependencyUnexpectedCommentQuery)
        }
        "PATCH" if path == EXACT => Some(Route::DependencyEditBodyMismatch),
        "GET" | "PATCH" if path.starts_with("/repos/fixture-org/private/issues/comments/") => {
            Some(Route::DependencyWrongCommentId)
        }
        "GET" => Some(Route::DependencyUnexpectedRead),
        "PATCH" => Some(Route::DependencyUnexpectedEdit),
        _ => None,
    }
}

pub(super) fn response(phase: Phase, route: Route) -> Option<Response> {
    if !active(phase) {
        return None;
    }
    let comment = |body, updated| {
        json!({
            "id":4101, "node_id":"IC_4101", "body":body, "user":{"id":41,"login":"fixture-user"},
            "issue_url":"https://api.github.com/repos/fixture-org/private/issues/41",
            "created_at":"2020-01-01T00:00:00Z","updated_at":updated
        })
    };
    let original = "2020-01-01T00:00:00Z";
    let (status, reason, body) = match (phase, route) {
        (Phase::CommentDependencyCreate, Route::DependencyCommentCreated) => {
            (201, "Created", comment(ORIGINAL, original))
        }
        (Phase::CommentDependencyResume, Route::DependencyCommentRead) => {
            (200, "OK", comment(ORIGINAL, original))
        }
        (Phase::CommentDependencyResume, Route::DependencyCommentEdited) => {
            (200, "OK", comment(EDIT, "2020-01-01T00:00:01Z"))
        }
        (
            _,
            Route::DependencyCommentRead
            | Route::DependencyCommentCreated
            | Route::DependencyCommentEdited,
        ) => (
            409,
            "Conflict",
            json!({"message":"The dependent fixture request used an incorrect phase"}),
        ),
        _ => return writes::response(Phase::WriteOnline, route),
    };
    Some(Response {
        status,
        reason,
        body,
        sso: None,
    })
}

pub(super) fn trace(records: &[RequestEvidence]) -> bool {
    let effects: Vec<_> = records
        .iter()
        .filter_map(|entry| {
            matches!(
                entry.route,
                Route::DependencyCommentCreated
                    | Route::DependencyCommentRead
                    | Route::DependencyCommentEdited
                    | Route::DependencyCommentLost
            )
            .then_some((entry.phase, entry.route, entry.status))
        })
        .collect();
    records.iter().all(|entry| {
        entry.fixture_authorization
            && entry.phase != Phase::CommentDependencyOffline
            && !matches!(
                entry.route,
                Route::DependencyUnexpectedRead
                    | Route::DependencyUnexpectedEdit
                    | Route::DependencyWrongCommentId
                    | Route::DependencyUnexpectedCommentQuery
                    | Route::DependencyEditBodyMismatch
            )
            && !matches!(
                entry.route,
                Route::Unexpected | Route::CommentCreated | Route::CommentReplyLost
            )
    }) && effects
        == [
            (
                Phase::CommentDependencyCreate,
                Route::DependencyCommentCreated,
                201,
            ),
            (
                Phase::CommentDependencyResume,
                Route::DependencyCommentRead,
                200,
            ),
            (
                Phase::CommentDependencyResume,
                Route::DependencyCommentEdited,
                200,
            ),
            (
                Phase::CommentDependencyLost,
                Route::DependencyCommentLost,
                0,
            ),
        ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependent_comment_request_diagnostics_use_checked_shapes_only() {
        let phase = Phase::CommentDependencyResume;
        let cases = [
            (
                "GET",
                "/repos/fixture-org/private/issues/comments/99",
                Route::DependencyWrongCommentId,
            ),
            (
                "GET",
                "/repos/fixture-org/private/issues/comments/4101?unknown=value",
                Route::DependencyUnexpectedCommentQuery,
            ),
            ("PATCH", EXACT, Route::DependencyEditBodyMismatch),
            ("GET", "/elsewhere", Route::DependencyUnexpectedRead),
            ("PATCH", "/elsewhere", Route::DependencyUnexpectedEdit),
        ];
        for (method, target, expected) in cases {
            assert_eq!(
                classify_unexpected_request(phase, Some(method), Some(target)),
                Some(expected)
            );
            assert_eq!(
                classify_unexpected_request(Phase::WriteOnline, Some(method), Some(target)),
                None
            );
        }
    }

    #[test]
    fn comment_dependency_fixture_uses_exact_remote_id_and_preserves_order() {
        let fixture_root = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(fixture_root.path()).unwrap();
        let request = |method, path, payload: Option<&str>| {
            let mut socket = TcpStream::connect(fixture.address()).unwrap();
            write!(
                socket,
                "{method} {path} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\n"
            )
            .unwrap();
            if let Some(text) = payload {
                let bytes = json!({"body":text}).to_string();
                write!(socket, "Content-Length: {}\r\n\r\n{bytes}", bytes.len()).unwrap();
            } else {
                write!(socket, "\r\n").unwrap();
            }
            let mut reply = String::new();
            socket.read_to_string(&mut reply).unwrap();
            reply
        };
        fs::write(fixture.phase_file(), "comment-dependency-create").unwrap();
        let created = request("POST", CREATE, Some(ORIGINAL));
        assert!(created.starts_with("HTTP/1.1 201"));
        assert!(created.contains("Retry-After: 25"));
        assert!(created.contains("\"id\":4101"));
        fs::write(fixture.phase_file(), "comment-dependency-resume").unwrap();
        assert!(request("GET", EXACT, None).contains(ORIGINAL));
        assert!(request("PATCH", EXACT, Some(EDIT)).contains(EDIT));
        fs::write(fixture.phase_file(), "comment-dependency-lost").unwrap();
        assert!(request("POST", CREATE, Some(LOST)).is_empty());
        fixture
            .finish(fixture_root.path(), TraceCheck::CommentDependency)
            .unwrap();
        assert_eq!(
            patch(
                Phase::CommentDependencyResume,
                "/repos/fixture-org/private/issues/comments/4102",
                &json!({"body":EDIT})
            ),
            Route::Unexpected
        );
        assert_eq!(
            patch(Phase::CommentDependencyCreate, EXACT, &json!({"body":EDIT})),
            Route::Unexpected
        );
    }
}
