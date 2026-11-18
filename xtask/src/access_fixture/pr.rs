use super::*;
use std::time::Instant;

const A_BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const A_HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const A_MERGE: &str = "cccccccccccccccccccccccccccccccccccccccc";
const B_BASE: &str = "dddddddddddddddddddddddddddddddddddddddd";
const B_HEAD: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const B_MERGE: &str = "ffffffffffffffffffffffffffffffffffffffff";

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
        _ => None,
    }
}

pub(super) fn applies(phase: Phase) -> bool {
    matches!(phase, Phase::PrLoad | Phase::PrDelay | Phase::PrRelease)
}

fn data(route: Route) -> Option<Value> {
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
        Route::PullB => Some(pull(second(), 43, B_BASE, B_HEAD)),
        Route::CompareA => Some(compare(A_BASE, A_HEAD, A_MERGE, "src/first.rs")),
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
        body: data(route)?,
        sso: None,
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
