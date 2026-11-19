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
            | Phase::PrMergeState
            | Phase::PrDelay
            | Phase::PrRelease
            | Phase::PrSharedHold
            | Phase::PrSharedReady
            | Phase::PrSharedRelease
            | Phase::ReviewSend
            | Phase::ReviewChanged
    ) || merge::applies(phase)
        || pr_pins::active(phase)
        || review_threads::active(phase)
        || review_decisions::active(phase)
}

pub(super) fn shared_review_query(phase: Phase, request: &Value) -> Option<Route> {
    let variables = request.get("variables")?;
    (matches!(phase, Phase::PrSharedHold | Phase::PrSharedRelease)
        && request
            .get("query")?
            .as_str()?
            .starts_with("query ThreadlineReviews(")
        && variables == &json!({"id":"PR_42","cursor":null}))
        .then_some(Route::SharedPrReview)
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
    let shared_body = {
        let mut body = "Closes #41. Resolves #44.".to_owned();
        if matches!(
            phase,
            Phase::PrSharedReady | Phase::PrSharedHold | Phase::PrSharedRelease
        ) {
            for index in 1..=18 {
                use std::fmt::Write as _;
                write!(body, "\n\nReading checkpoint {index}. The original provider text stays in both views.").expect("write to String");
            }
        }
        body
    };
    let first = || {
        summary(
            42,
            "First fixture pull request",
            shared_body.as_str(),
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
        Route::Items if phase == Phase::PrMergeState => {
            let mut first = first();
            let mut second = second();
            first["state"] = json!("closed");
            second["state"] = json!("closed");
            Some(json!([first, second]))
        }
        Route::PrStateItemsNext if phase == Phase::PrMergeState => {
            let mut updated = first();
            updated["state"] = json!("closed");
            updated["title"] = json!("First fixture pull request after issue refresh");
            updated["updated_at"] = json!("2021-01-03T00:00:00Z");
            Some(json!([updated]))
        }
        Route::Items => Some(json!([first(), second()])),
        Route::PrPinItem42 => Some(first()),
        Route::PrPinItem43 => Some(second()),
        Route::PullA => {
            let mut value = pull(first(), 42, A_BASE, A_HEAD);
            if pr_pins::active(phase) {
                value["changed_files"] = json!(302);
            }
            if phase == Phase::PrMergeState {
                value["state"] = json!("closed");
                value["merged"] = json!(true);
                value["merged_at"] = json!("2021-01-02T00:00:00Z");
                value["merge_commit_sha"] = json!(A_MERGE);
                value["updated_at"] = json!("2021-01-02T00:00:00Z");
            }
            if matches!(phase, Phase::PrSharedHold | Phase::PrSharedRelease) {
                value["draft"] = json!(true);
                value["updated_at"] = json!("2021-01-02T00:00:00Z");
            }
            Some(value)
        }
        Route::SharedPrReview if matches!(phase, Phase::PrSharedHold | Phase::PrSharedRelease) => {
            Some(json!({
                "data": { "node": {
                    "id":"PR_42", "fullDatabaseId":762, "baseRefOid":A_BASE, "headRefOid":A_HEAD,
                    "repository":{"id":"R_71","name":"private","owner":{"login":"fixture-org"}},
                    "reviewDecision":"REVIEW_REQUIRED",
                    "reviews": {
                        "pageInfo":{"hasNextPage":true,"endCursor":"fixture-more-reviews"},
                        "nodes":[{"id":"RV_92","fullDatabaseId":92,"body":"First known review page",
                            "state":"COMMENTED","submittedAt":"2021-01-02T00:00:00Z",
                            "author":{"__typename":"User","login":"fixture-human","databaseId":92},
                            "commit":{"oid":A_HEAD}}]
                    }
                }}
            }))
        }
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
            if pr_pins::active(phase) {
                comparison["files"] = json!(pr_pins::comparison_files());
            }
            if review_decisions::active(phase) {
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

pub(super) fn trace_merge_state(records: &[RequestEvidence]) -> bool {
    records.iter().all(|entry| {
        entry.phase == Phase::PrMergeState
            && entry.fixture_authorization
            && entry.status == 200
            && matches!(
                entry.route,
                Route::User
                    | Route::Repository
                    | Route::Items
                    | Route::PrStateItemsNext
                    | Route::PullA
                    | Route::CompareA
                    | Route::UpdatedItems
                    | Route::UpdatedComments
                    | Route::UpdatedInlineComments
                    | Route::OpenPullIndex
            )
    }) && records
        .iter()
        .filter_map(|entry| {
            matches!(
                entry.route,
                Route::Items | Route::PullA | Route::PrStateItemsNext
            )
            .then_some(entry.route)
        })
        .eq([Route::Items, Route::PullA, Route::PrStateItemsNext])
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
    hold_reply(
        phase_file,
        stopping,
        "pr-issue-pending.txt",
        Phase::PrRelease,
        Duration::from_secs(10),
    )
}

pub(super) fn wait_for_shared_review(phase_file: &Path, stopping: &AtomicBool) -> io::Result<()> {
    hold_reply(
        phase_file,
        stopping,
        "pr-shared-review-pending.txt",
        Phase::PrSharedRelease,
        Duration::from_secs(12),
    )
}

fn hold_reply(
    phase_file: &Path,
    stopping: &AtomicBool,
    filename: &str,
    release: Phase,
    timeout: Duration,
) -> io::Result<()> {
    fs::write(
        phase_file.with_file_name(filename),
        "verified request received\n",
    )?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline && !stopping.load(Ordering::Acquire) {
        if Phase::read(phase_file).ok() == Some(release) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(io::Error::other(
        "the native proof did not release the bounded response",
    ))
}

pub(super) fn trace_shared(records: &[RequestEvidence]) -> bool {
    records.iter().all(|row| {
        row.fixture_authorization
            && row.status == 200
            && matches!(
                row.route,
                Route::User
                    | Route::Repository
                    | Route::Items
                    | Route::PullA
                    | Route::CompareA
                    | Route::SharedPrReview
            )
    }) && [Route::Items, Route::PullA, Route::CompareA]
        .into_iter()
        .all(|route| {
            records
                .iter()
                .any(|row| row.phase == Phase::PrSharedReady && row.route == route)
        })
        && records
            .iter()
            .filter(|row| row.route == Route::SharedPrReview)
            .map(|row| row.phase)
            .eq([Phase::PrSharedHold])
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
    fn shared_pr_refresh_changes_the_header_without_replacing_its_reading_text() {
        let list = response(Phase::PrSharedReady, Route::Items).unwrap().body;
        let body = list[0]["body"].as_str().unwrap();
        assert_eq!(body.matches("Reading checkpoint ").count(), 18);
        for phase in [
            Phase::PrSharedReady,
            Phase::PrSharedHold,
            Phase::PrSharedRelease,
        ] {
            let pull = response(phase, Route::PullA).unwrap().body;
            assert_eq!(pull["body"], body);
            assert_eq!(pull["head"]["sha"], A_HEAD);
            assert_eq!(pull["draft"], phase != Phase::PrSharedReady);
        }
    }

    #[test]
    fn merge_state_journey_keeps_separate_issue_and_pull_request_identities() {
        let phase = Phase::PrMergeState;
        let baseline = data(phase, Route::Items).unwrap();
        let details = data(phase, Route::PullA).unwrap();
        let next = data(phase, Route::PrStateItemsNext).unwrap();
        assert_eq!(baseline[0]["id"], 442);
        assert_eq!(details["id"], 762);
        assert_eq!(details["merged"], true);
        assert_eq!(next[0]["id"], baseline[0]["id"]);
        assert_eq!(next[0]["pull_request"], baseline[0]["pull_request"]);
        assert_eq!(next[0]["state"], "closed");
        assert!(next[0].get("merged").is_none());
        assert!(next[0]["pull_request"].get("merged_at").is_none());
        assert_eq!(baseline[1]["id"], 443);
        assert_eq!(baseline[1]["state"], "closed");
        assert!(baseline[1]["pull_request"].get("merged_at").is_none());

        let mut records = Vec::from([Route::Items, Route::PullA, Route::PrStateItemsNext].map(
            |route| RequestEvidence {
                phase,
                route,
                status: 200,
                fixture_authorization: true,
            },
        ));
        assert!(trace_merge_state(&records));
        records.push(RequestEvidence {
            phase,
            route: Route::PullB,
            status: 200,
            fixture_authorization: true,
        });
        assert!(!trace_merge_state(&records));
    }

    #[test]
    fn shared_pr_fixture_holds_the_exact_review_page_until_its_own_release() {
        let directory = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(directory.path()).unwrap();
        let address = fixture.address();
        let get = |path: &str| {
            let mut socket = TcpStream::connect(address).unwrap();
            write!(
                socket,
                "GET {path} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\n\r\n"
            )
            .unwrap();
            let mut output = String::new();
            socket.read_to_string(&mut output).unwrap();
            output
        };
        fs::write(fixture.phase_file(), "pr-shared-ready").unwrap();
        let list = get(ITEMS);
        assert!(list.contains("First fixture pull request"));
        assert!(list.contains("Reading checkpoint 18"));
        assert!(get("/repositories/71/pulls/42").contains("\"draft\":false"));
        assert!(
            get(&format!(
                "/repositories/71/compare/{A_BASE}...{A_HEAD}?per_page=20&page=1"
            ))
            .contains("src/first.rs")
        );
        fs::write(fixture.phase_file(), "pr-shared-hold").unwrap();
        assert!(get("/repositories/71/pulls/42").contains("\"draft\":true"));
        let request = json!({"query":"query ThreadlineReviews($id:ID!,$cursor:String) { node(id:$id) { id } }",
            "variables":{"id":"PR_42","cursor":null}});
        let accepted = shared_review_query(Phase::PrSharedHold, &request);
        assert_eq!(accepted, Some(Route::SharedPrReview));
        assert_eq!(shared_review_query(Phase::PrLoad, &request), None);
        let mut other_revision = request.clone();
        other_revision["variables"]["id"] = json!("PR_43");
        assert_eq!(
            shared_review_query(Phase::PrSharedHold, &other_revision),
            None
        );
        let response = thread::spawn(move || {
            let mut socket = TcpStream::connect(address).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(14)))
                .unwrap();
            let body = request.to_string();
            write!(socket, "POST /graphql HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let mut output = String::new();
            socket.read_to_string(&mut output).unwrap();
            output
        });
        let marker = directory.path().join("pr-shared-review-pending.txt");
        let deadline = Instant::now() + Duration::from_secs(3);
        while !marker.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(marker.is_file());
        assert!(!response.is_finished());
        fs::write(fixture.phase_file(), "pr-shared-release").unwrap();
        let output = response.join().unwrap();
        assert!(output.starts_with("HTTP/1.1 200"));
        assert!(output.contains("\"hasNextPage\":true"));
        fixture
            .finish(directory.path(), TraceCheck::SharedPrPanes)
            .unwrap();
        let mut captured: Vec<Value> = serde_json::from_slice(
            &fs::read(directory.path().join("github-access-trace.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            captured
                .iter()
                .filter(|row| row["route"] == "shared_pr_review")
                .count(),
            1
        );
        let duplicate = captured.pop().unwrap();
        assert_eq!(duplicate["phase"], "pr_shared_hold");
    }

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
