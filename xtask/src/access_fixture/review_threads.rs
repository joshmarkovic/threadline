use super::*;

const THREAD: &str = "PRRT_42";
const ROOT: u32 = 771;
const HEAD: &str = merge::A_HEAD;
const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const REPLY: &str = "Threadline synthetic queued thread reply.";

pub(super) fn active(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::ThreadQueue | Phase::ThreadOutdated | Phase::ThreadPlacement
    )
}

pub(super) fn graphql(phase: Phase, body: &Value) -> Option<Route> {
    if !active(phase) {
        return None;
    }
    let query = body.get("query")?.as_str()?;
    let variables = body.get("variables")?;
    if *variables == json!({"id":"PR_42","cursor":null}) {
        if query.starts_with("query ThreadlineThreads(") {
            return Some(Route::QueuedThreadList);
        }
        if query.starts_with("query ThreadlineReviews(") {
            return Some(Route::QueuedThreadReviewList);
        }
    }
    if phase != Phase::ThreadPlacement
        && *variables == json!({"id":THREAD})
        && query.starts_with("query ThreadlineReviewThread(")
    {
        return Some(Route::QueuedThreadTarget);
    }
    if phase == Phase::ThreadQueue
        && query.starts_with("mutation ThreadlineSetReviewThread($input:ResolveReviewThreadInput!)")
        && query.contains("change:resolveReviewThread(input:$input)")
        && variables["input"]["threadId"] == THREAD
        && correlation(variables["input"]["clientMutationId"].as_str()?)
    {
        return Some(Route::QueuedThreadResolve);
    }
    None
}

fn correlation(value: &str) -> bool {
    value.split_once(':').is_some_and(|(operation, attempt)| {
        operation.len() == 36
            && [8, 13, 18, 23]
                .into_iter()
                .all(|index| operation.as_bytes()[index] == b'-')
            && operation
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
            && attempt.parse::<u16>().is_ok_and(|number| number > 0)
    })
}

pub(super) fn post(phase: Phase, path: &str, body: &Value) -> Option<Route> {
    (phase == Phase::ThreadQueue
        && path == "/repos/fixture-org/private/pulls/42/comments/771/replies"
        && *body == json!({"body":REPLY}))
    .then_some(Route::QueuedThreadReply)
}

pub(super) fn response(phase: Phase, route: Route, identifier: Option<&str>) -> Option<Response> {
    if !active(phase) {
        return None;
    }
    let outdated = phase == Phase::ThreadOutdated;
    let info = || json!({"hasNextPage":false,"endCursor":null});
    let thread = |resolved| {
        json!({
            "id":THREAD,"isOutdated":outdated,"isResolved":resolved,"viewerCanResolve":true,"viewerCanUnresolve":true,
            "pullRequest":{"id":"PR_42","number":42,"baseRefOid":BASE,"headRefOid":HEAD,"repository":{"id":"R_71"}},
            "comments":{"nodes":[{"fullDatabaseId":ROOT}]}
        })
    };
    let mut node = json!({"id":"PR_42","fullDatabaseId":762,"baseRefOid":BASE,"headRefOid":HEAD,
        "repository":{"id":"R_71","name":"private","owner":{"login":"fixture-org"}}});
    let (status, reason, body) = match route {
        Route::QueuedThreadList if phase == Phase::ThreadPlacement => {
            node["reviewThreads"] = placement_threads();
            (200, "OK", json!({"data":{"node":node}}))
        }
        Route::QueuedThreadList => {
            node["reviewThreads"] = json!({"pageInfo":info(),"nodes":[{
                "id":THREAD,"diffSide":"RIGHT","isResolved":outdated,"isOutdated":outdated,
                "comments":{"pageInfo":info(),"nodes":[{
                    "id":"PRRC_771","fullDatabaseId":ROOT,"replyTo":null,
                    "author":{"login":"fixture-author","databaseId":75,"__typename":"User"},"body":"Fixture thread.",
                    "createdAt":"2021-01-01T00:00:00Z","updatedAt":"2021-01-01T00:00:00Z",
                    "path":"src/first.rs","originalLine":1,"originalCommit":{"oid":HEAD}
                }]}
            }]});
            (200, "OK", json!({"data":{"node":node}}))
        }
        Route::QueuedThreadReviewList => {
            node["reviews"] = json!({"nodes":[],"pageInfo":info()});
            (200, "OK", json!({"data":{"node":node}}))
        }
        Route::QueuedThreadTarget => (200, "OK", json!({"data":{"node":thread(outdated)}})),
        Route::QueuedThreadResolve => (
            200,
            "OK",
            json!({"data":{"change":{"clientMutationId":identifier?,"thread":thread(true)}}}),
        ),
        Route::QueuedThreadReply => (
            201,
            "Created",
            json!({"id":772,"user":{"id":41,"login":"fixture-user"},
            "in_reply_to_id":ROOT,"body":REPLY,"pull_request_url":"https://api.github.com/repos/fixture-org/private/pulls/42"}),
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

fn placement_threads() -> Value {
    let page = json!({"hasNextPage":false,"endCursor":null});
    let comment = |id, reply, who, body| {
        json!({
            "id":format!("PRRC_{id}"),"fullDatabaseId":id,"replyTo":reply,
            "author":{"login":who,"databaseId":75,"__typename":"User"},"body":body,
            "createdAt":"2021-01-01T00:00:00Z","updatedAt":"2021-01-01T00:00:00Z",
            "path":"src/earlier.rs","originalLine":3,"originalCommit":{"oid":"d".repeat(40)}
        })
    };
    let thread = |id, side, line, subject, outdated, comments| {
        json!({
            "id":id,"path":"src/first.rs","diffSide":side,"line":line,"subjectType":subject,
            "isResolved":false,"isOutdated":outdated,"comments":{"nodes":comments,"pageInfo":page}
        })
    };
    json!({"pageInfo":page,"nodes":[
        thread("PRRT_LEFT","LEFT",Some(9),"LINE",false,vec![
            comment(781, Value::Null, "fixture-left", "Deletion-side root."),
            comment(782, json!({"fullDatabaseId":781}), "fixture-reply", "Downloaded deletion-side reply.")
        ]),
        thread("PRRT_RIGHT","RIGHT",Some(17),"LINE",false,vec![
            comment(783, Value::Null, "fixture-right", "Moved right-side root."),
            comment(784, json!({"fullDatabaseId":783}), "fixture-reply", "Downloaded right-side reply.")
        ]),
        thread("PRRT_FILE","RIGHT",None,"FILE",false,vec![
            comment(785, Value::Null, "fixture-file", "File-level review body.")
        ]),
        thread("PRRT_OUTDATED","RIGHT",Some(17),"LINE",true,vec![
            comment(786, Value::Null, "fixture-outdated", "Outdated review body.")
        ])
    ]})
}

pub(super) fn placement_trace(records: &[RequestEvidence]) -> bool {
    let once = |route| records.iter().filter(|entry| entry.route == route).count() == 1;
    once(Route::CompareA)
        && once(Route::QueuedThreadList)
        && records.iter().all(|entry| {
            entry.phase == Phase::ThreadPlacement
                && entry.fixture_authorization
                && entry.status == 200
                && matches!(
                    entry.route,
                    Route::User
                        | Route::Repository
                        | Route::Discovery
                        | Route::Items
                        | Route::PullA
                        | Route::CompareA
                        | Route::QueuedThreadList
                        | Route::QueuedThreadReviewList
                )
        })
}

pub(super) fn trace(records: &[RequestEvidence]) -> bool {
    let attempts: Vec<_> = records
        .iter()
        .filter_map(|row| match row.route {
            Route::QueuedThreadReply | Route::QueuedThreadResolve => {
                Some((row.phase, row.route, row.status))
            }
            _ => None,
        })
        .collect();
    attempts
        == [
            (Phase::ThreadQueue, Route::QueuedThreadReply, 201),
            (Phase::ThreadQueue, Route::QueuedThreadResolve, 200),
        ]
        && records
            .iter()
            .any(|row| row.route == Route::QueuedThreadList && row.status == 200)
        && records.iter().any(|row| {
            row.phase == Phase::ThreadOutdated
                && row.route == Route::QueuedThreadTarget
                && row.status == 200
        })
        && records.iter().all(|row| {
            active(row.phase)
                && row.fixture_authorization
                && row.route != Route::Unexpected
                && matches!(row.status, 200 | 201)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_thread_fixture_checks_exact_root_scope_and_transport_order() {
        let body = json!({"body":REPLY});
        let endpoint = "/repos/fixture-org/private/pulls/42/comments/771/replies";
        assert_eq!(
            post(Phase::ThreadQueue, endpoint, &body),
            Some(Route::QueuedThreadReply)
        );
        assert_eq!(post(Phase::ThreadOutdated, endpoint, &body), None);
        assert_eq!(
            post(Phase::ThreadQueue, &endpoint.replace("771", "777"), &body),
            None
        );
        let identity = "00000000-0000-4000-8000-000000000042:1";
        let mut payload = json!({"query":"mutation ThreadlineSetReviewThread($input:ResolveReviewThreadInput!) { change:resolveReviewThread(input:$input) { clientMutationId } }",
            "variables":{"input":{"threadId":THREAD,"clientMutationId":identity}}});
        assert_eq!(
            graphql(Phase::ThreadQueue, &payload),
            Some(Route::QueuedThreadResolve)
        );
        assert_eq!(graphql(Phase::ThreadOutdated, &payload), None);
        payload["variables"]["input"]["threadId"] = json!("foreign");
        assert_eq!(graphql(Phase::ThreadQueue, &payload), None);
        let outcome = response(
            Phase::ThreadQueue,
            Route::QueuedThreadResolve,
            Some(identity),
        )
        .unwrap();
        assert_eq!(outcome.body["data"]["change"]["clientMutationId"], identity);
        assert_eq!(outcome.body["data"]["change"]["thread"]["isResolved"], true);

        let mut records: Vec<_> = [
            (Phase::ThreadQueue, Route::QueuedThreadList, 200),
            (Phase::ThreadQueue, Route::QueuedThreadReply, 201),
            (Phase::ThreadQueue, Route::QueuedThreadResolve, 200),
            (Phase::ThreadOutdated, Route::QueuedThreadTarget, 200),
        ]
        .into_iter()
        .map(|(phase, route, status)| RequestEvidence {
            phase,
            route,
            status,
            fixture_authorization: true,
        })
        .collect();
        assert!(trace(&records));
        records.swap(1, 2);
        assert!(!trace(&records));
    }
}
