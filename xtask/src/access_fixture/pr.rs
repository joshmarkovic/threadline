use super::*;
use std::time::Instant;

const A_BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const A_HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const A_MERGE: &str = "cccccccccccccccccccccccccccccccccccccccc";
const B_BASE: &str = "dddddddddddddddddddddddddddddddddddddddd";
const B_HEAD: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const B_MERGE: &str = "ffffffffffffffffffffffffffffffffffffffff";
const B_NEW_HEAD: &str = "9999999999999999999999999999999999999999";
const REVIEW_SUMMARY: &str = "Threadline synthetic review summary.";
const INLINE_NOTE: &str = "Threadline synthetic inline review note.";

pub(super) fn route(path: &str) -> Option<Route> {
    if path == format!("/repositories/71/compare/{A_BASE}...{A_HEAD}?per_page=20&page=1") {
        return Some(Route::CompareA);
    }
    if path == format!("/repositories/71/compare/{B_BASE}...{B_HEAD}?per_page=20&page=1") {
        return Some(Route::CompareB);
    }
    match path {
        "/repositories/71/pulls/42" => Some(Route::PullA),
        "/repositories/71/pulls/43" => Some(Route::PullB),
        "/repos/fixture-org/private/issues/44" => Some(Route::ReviewContextIssue),
        "/repos/fixture-org/private/pulls/42/reviews?per_page=100&page=1" => {
            Some(Route::ReviewList)
        }
        "/repos/fixture-org/private/pulls/42/reviews/91" => Some(Route::ReviewRead),
        _ => None,
    }
}

pub(super) fn applies(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::PrLoad
            | Phase::PrDelay
            | Phase::PrRelease
            | Phase::ReviewSend
            | Phase::ReviewChanged
            | Phase::ReviewDecisions
    ) || merge::applies(phase)
        || review_threads::active(phase)
}

pub(super) fn activity_query(phase: Phase, request: &Value) -> Option<Route> {
    let vars = request.get("variables")?;
    let query = request.get("query")?.as_str()?;
    if !applies(phase)
        || !query.starts_with("query ThreadlinePullActivity(")
        || vars.get("owner")?.as_str()? != "fixture-org"
        || vars.get("name")?.as_str()? != "private"
        || !vars.get("cursor")?.is_null()
        || !vars.get("since")?.as_str()?.ends_with('Z')
    {
        return None;
    }
    match vars.get("number")?.as_u64()? {
        42 => Some(Route::PullActivityA),
        43 => Some(Route::PullActivityB),
        _ => None,
    }
}

fn data(phase: Phase, route: Route) -> Option<Value> {
    let summary = |number, title, body, date: &str| {
        json!({
            "id": 400+number, "node_id": format!("I_{number}"), "number":number, "title":title,
            "state":"open","body":body,"updated_at":date,"created_at":date,
            "pull_request":{"url":format!("https://api.github.com/repos/fixture-org/private/pulls/{number}")}
        })
    };
    let first = || {
        summary(
            42,
            "First fixture pull request",
            "Closes #41. Resolves #44.",
            "2021-01-01T00:00:00Z",
        )
    };
    let second = || {
        summary(
            43,
            "Second fixture pull request",
            "Independent branch",
            "2020-01-01T00:00:00Z",
        )
    };
    let pull = |mut value: Value, number: u32, base, head| {
        value["id"] = json!(720 + number);
        value["node_id"] = json!(format!("PR_{number}"));
        value["base"] = json!({"sha":base,"ref":"main","repo":{"id":71,"node_id":"R_71"}});
        value["head"] = json!({"sha":head,"ref":"topic"});
        value["changed_files"] = json!(1);
        value["merged"] = json!(false);
        value["draft"] = json!(false);
        value["locked"] = json!(false);
        value["user"] = json!({"id":75,"login":"fixture-author"});
        value
    };
    let compare = |base, head, merge, path| {
        json!({
            "base_commit":{"sha":base},"merge_base_commit":{"sha":merge},"total_commits":1,
            "commits":[{"sha":head,"commit":{"message":"Fixture commit"}}],
            "files":[{"filename":path,"status":"modified","additions":1,"deletions":1,
                "patch":"@@ -1 +1 @@\n-original\n+changed"}]
        })
    };
    match route {
        Route::Items => Some(json!([first(), second()])),
        Route::PullA => Some(pull(first(), 42, A_BASE, A_HEAD)),
        Route::PullB => Some(pull(
            second(),
            43,
            B_BASE,
            if matches!(phase, Phase::ReviewChanged | Phase::MergeWrongHead) {
                B_NEW_HEAD
            } else {
                B_HEAD
            },
        )),
        Route::CompareA => {
            let mut comparison = compare(A_BASE, A_HEAD, A_MERGE, "src/first.rs");
            if phase == Phase::ReviewDecisions {
                comparison["files"][0]["additions"] = json!(2);
                comparison["files"][0]["deletions"] = json!(2);
                comparison["files"][0]["patch"] = json!(
                    "@@ -1 +1 @@\n-original\n+changed\n@@ -3 +3 @@\n-original again\n+changed again"
                );
            }
            Some(comparison)
        }
        Route::CompareB => Some(compare(B_BASE, B_HEAD, B_MERGE, "src/second.rs")),
        Route::Item => Some(json!({
            "id":410,"node_id":"I_410","number":41,"title":"First pull request issue",
            "state":"closed","body":"Only the first pull request references this synthetic issue.",
            "updated_at":"2018-01-01T00:00:00Z",
            "repository_url":"https://api.github.com/repos/fixture-org/private"
        })),
        Route::ReviewContextIssue => Some(json!({
            "id":414,"node_id":"I_414","number":44,"title":"Alternate review context issue",
            "state":"closed","body":"The other synthetic issue directly referenced by the pull request.",
            "updated_at":"2018-01-01T00:00:00Z",
            "repository_url":"https://api.github.com/repos/fixture-org/private"
        })),
        Route::PullActivityA | Route::PullActivityB => {
            let pull_route = if route == Route::PullActivityA {
                Route::PullA
            } else {
                Route::PullB
            };
            let pull = data(phase, pull_route)?;
            Some(
                json!({"data":{"repository":{"databaseId":71,"name":"private","owner":{"login":"fixture-org"},
                "pullRequest":{"id":pull["node_id"],"fullDatabaseId":pull["id"],"number":pull["number"],
                "title":pull["title"],"closed":false,"merged":false,"updatedAt":pull["updated_at"],
                "baseRefOid":pull["base"]["sha"],"headRefOid":pull["head"]["sha"],
                "timelineItems":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[]}}}}}),
            )
        }
        Route::ReviewList => Some(json!([])),
        Route::ReviewRead | Route::ReviewCreated | Route::ReviewSubmitted
            if phase == Phase::ReviewSend =>
        {
            Some(
                json!({"id":91,"state": if route == Route::ReviewSubmitted { "COMMENTED" } else { "PENDING" },
                "user":{"id":41,"login":"fixture-user"},"commit_id":A_HEAD, "body":REVIEW_SUMMARY,
                "pull_request_url":"https://api.github.com/repos/fixture-org/private/pulls/42"}),
            )
        }
        _ => None,
    }
}

pub(super) fn response(phase: Phase, route: Route) -> Option<Response> {
    if !applies(phase) {
        return None;
    }
    Some(Response {
        status: 200,
        reason: "OK",
        body: data(phase, route)?,
        sso: None,
    })
}

pub(super) fn post(phase: Phase, path: &str, body: &Value) -> Option<Route> {
    if phase != Phase::ReviewSend {
        return None;
    }
    match path {
        "/repos/fixture-org/private/pulls/42/reviews"
            if *body
                == json!({
                    "commit_id": A_HEAD, "body": REVIEW_SUMMARY,
                    "comments": [{"path":"src/first.rs", "line":1, "side":"RIGHT", "body":INLINE_NOTE}]
                }) =>
        {
            Some(Route::ReviewCreated)
        }
        "/repos/fixture-org/private/pulls/42/reviews/91/events"
            if *body
                == json!({
                    "event":"COMMENT", "body": REVIEW_SUMMARY,
                }) =>
        {
            Some(Route::ReviewSubmitted)
        }
        _ => None,
    }
}

pub(super) fn trace_review_draft(records: &[RequestEvidence]) -> bool {
    let exact = |route, phase| {
        records
            .iter()
            .filter(|row| row.route == route && row.phase == phase && row.status == 200)
            .count()
    };
    records
        .iter()
        .all(|row| row.fixture_authorization && row.route != Route::Unexpected)
        && exact(Route::ReviewCreated, Phase::ReviewSend) == 1
        && exact(Route::ReviewSubmitted, Phase::ReviewSend) == 1
        && exact(Route::PullB, Phase::ReviewChanged) >= 1
        && records.iter().all(|row| {
            row.phase != Phase::ReviewChanged
                || !matches!(row.route, Route::ReviewCreated | Route::ReviewSubmitted)
        })
}

pub(super) fn wait_for_release(phase_file: &Path, stopping: &AtomicBool) -> io::Result<()> {
    let marker = phase_file.with_file_name("pr-issue-pending.txt");
    fs::write(marker, "verified issue request received\n")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && !stopping.load(Ordering::Acquire) {
        if Phase::read(phase_file).ok() == Some(Phase::PrRelease) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(io::Error::other(
        "the native proof did not release the bounded issue response",
    ))
}

pub(super) fn trace(records: &[RequestEvidence]) -> bool {
    let successful = |route| {
        records
            .iter()
            .any(|row| row.route == route && row.status == 200 && row.fixture_authorization)
    };
    records
        .iter()
        .all(|row| row.route != Route::Unexpected && row.fixture_authorization)
        && [Route::PullA, Route::PullB, Route::CompareA, Route::CompareB]
            .into_iter()
            .all(successful)
        && records
            .iter()
            .filter(|row| {
                row.route == Route::Item && row.phase == Phase::PrDelay && row.status == 200
            })
            .count()
            == 1
}

pub(super) fn trace_review_context(records: &[RequestEvidence]) -> bool {
    records
        .iter()
        .all(|row| row.fixture_authorization && row.route != Route::Unexpected)
        && [
            Route::PullA,
            Route::PullB,
            Route::CompareA,
            Route::CompareB,
            Route::Item,
            Route::ReviewContextIssue,
        ]
        .into_iter()
        .all(|route| {
            records
                .iter()
                .any(|row| row.route == route && row.status == 200)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_review_steps_return_distinct_required_request_receipts() {
        let temp = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(temp.path()).unwrap();
        fs::write(fixture.phase_file(), "review-send").unwrap();
        let post = |path: &str, payload: Value| {
            let mut socket = TcpStream::connect(fixture.address()).unwrap();
            let body = payload.to_string();
            write!(socket, "POST {path} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let mut reply = String::new();
            socket.read_to_string(&mut reply).unwrap();
            reply
        };
        let created = post(
            "/repos/fixture-org/private/pulls/42/reviews",
            json!({
                "commit_id":A_HEAD,"body":REVIEW_SUMMARY,
                "comments":[{"path":"src/first.rs","line":1,"side":"RIGHT","body":INLINE_NOTE}]
            }),
        );
        let submitted = post(
            "/repos/fixture-org/private/pulls/42/reviews/91/events",
            json!({
                "event":"COMMENT","body":REVIEW_SUMMARY
            }),
        );
        assert!(
            created.starts_with("HTTP/1.1 200")
                && created.contains("X-GitHub-Request-Id: SYNTHETIC:4201:08\r\n")
        );
        assert!(
            submitted.starts_with("HTTP/1.1 200")
                && submitted.contains("X-GitHub-Request-Id: SYNTHETIC:4202:08\r\n")
        );
        fixture.finish(temp.path(), TraceCheck::Observe).unwrap();
    }

    #[test]
    fn isolates_both_heads_and_holds_only_the_exact_linked_issue() {
        let temp = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(temp.path()).unwrap();
        let address = fixture.address();
        let get = move |path: &str| {
            let mut socket = TcpStream::connect(address).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(12)))
                .unwrap();
            write!(
                socket,
                "GET {path} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\n\r\n"
            )
            .unwrap();
            let mut reply = String::new();
            socket.read_to_string(&mut reply).unwrap();
            reply
        };
        fs::write(fixture.phase_file(), "pr-load").unwrap();
        assert!(get("/repositories/71/pulls/42").contains(A_HEAD));
        assert!(get("/repositories/71/pulls/43").contains(B_HEAD));
        assert!(
            get(&format!(
                "/repositories/71/compare/{A_BASE}...{A_HEAD}?per_page=20&page=1"
            ))
            .contains("src/first.rs")
        );
        assert!(
            get(&format!(
                "/repositories/71/compare/{B_BASE}...{B_HEAD}?per_page=20&page=1"
            ))
            .contains("src/second.rs")
        );
        assert!(
            route(&format!(
                "/repositories/71/compare/{A_BASE}...{B_HEAD}?per_page=20&page=1"
            ))
            .is_none()
        );

        fs::write(fixture.phase_file(), "pr-delay").unwrap();
        let response = thread::spawn(move || get(ITEM));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !temp.path().join("pr-issue-pending.txt").exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(temp.path().join("pr-issue-pending.txt").is_file());
        assert!(!response.is_finished());
        fs::write(fixture.phase_file(), "pr-release").unwrap();
        assert!(
            response
                .join()
                .unwrap()
                .contains("First pull request issue")
        );
        fixture
            .finish(temp.path(), TraceCheck::PullRequestJourney)
            .unwrap();
    }
}
