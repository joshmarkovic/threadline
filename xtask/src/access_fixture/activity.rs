use super::*;
use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use std::sync::OnceLock;

const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const MOVED: &str = "8888888888888888888888888888888888888888";
const QUIET_BRANCH: &str = "9999999999999999999999999999999999999999";
const API: &str = "https://api.github.com/repos/fixture-org/private";

pub(super) fn active(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::ActivityBaseline
            | Phase::ActivityFresh
            | Phase::ActivityOffline
            | Phase::ActivityCatchup
            | Phase::ActivityReplay
    )
}

fn date(minutes: i64) -> String {
    static CLOCK: OnceLock<DateTime<Utc>> = OnceLock::new();
    (*CLOCK.get_or_init(Utc::now) - TimeDelta::minutes(minutes))
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(super) fn read_route(phase: Phase, target: &str) -> Option<Route> {
    if !active(phase) {
        return None;
    }
    match target {
        "/repositories/71/branches?per_page=20&page=1" => Some(Route::ActivityBranches),
        "/repositories/71/branches/main" => Some(Route::ActivityWatched),
        "/repositories/71/branches/unwatched" => Some(Route::ActivityUnwatched),
        "/repositories/71/issues/42" | "/repositories/71/issues/43" => {
            Some(if target.ends_with("42") {
                Route::PullA
            } else {
                Route::PullB
            })
        }
        "/repositories/71/commits?sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa&per_page=20&page=1" => {
            Some(Route::ActivityBaselineTip)
        }
        "/repositories/71/commits?sha=8888888888888888888888888888888888888888&per_page=20&page=1" => {
            Some(Route::ActivityMovedTip)
        }
        "/repositories/71/commits?sha=9999999999999999999999999999999999999999&per_page=20&page=1" => {
            Some(Route::ActivityUnwatched)
        }
        "/repositories/71/commits/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/check-runs?per_page=20&page=1" => {
            Some(Route::ActivityOldReview)
        }
        "/repositories/71/commits/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/statuses?per_page=20&page=1" => {
            Some(Route::ActivityThreads)
        }
        _ => None,
    }
}

pub(super) fn graphql(phase: Phase, value: &Value) -> Option<Route> {
    if !active(phase) {
        return None;
    }
    let query = value.get("query")?.as_str()?;
    let variables = value.get("variables")?;
    if query.starts_with("query ThreadlinePullActivity(")
        && variables.get("owner")?.as_str()? == "fixture-org"
        && variables.get("name")?.as_str()? == "private"
        && variables.get("cursor")?.is_null()
        && variables.get("since")?.as_str()?.ends_with('Z')
    {
        return match variables.get("number")?.as_u64()? {
            42 => Some(Route::PullActivityA),
            43 => Some(Route::PullActivityB),
            _ => None,
        };
    }
    if query.starts_with("query ThreadlineReviews(")
        && variables.get("id")?.as_str()? == "PR_42"
        && variables.get("cursor")?.is_null()
    {
        Some(Route::ActivityReviews)
    } else {
        None
    }
}

fn summary(number: u64, closed: bool, stamp: &str) -> Value {
    json!({
        "id":400+number, "number":number,"node_id":format!("I_{number}"),
        "title":format!("Activity fixture pull request {number}"), "body":"Retained fixture pull request body",
        "state":if closed {"closed"} else {"open"}, "updated_at":stamp,"created_at":date(240),
        "repository_url":API, "pull_request":{"url":format!("{API}/pulls/{number}")},
    })
}

fn pull_meta(number: u64, merged: bool) -> Value {
    let mut value = summary(number, merged, &date(0));
    value["id"] = json!(720 + number);
    value["node_id"] = json!(format!("PR_{number}"));
    value["base"] = json!({"sha":BASE,"ref":"main","repo":{"id":71,"node_id":"R_71"}});
    value["head"] = json!({"sha":HEAD,"ref":"topic"});
    value["draft"] = json!(false);
    value["merged"] = json!(merged);
    value["merged_at"] = if merged {
        json!(date(if number == 43 { 240 } else { 20 }))
    } else {
        Value::Null
    };
    value
}

fn message(id: u64, number: u64, at: &str, author: u64, inline: bool) -> Value {
    let mut result = json!({
        "id":id, "node_id":format!("comment-{id}"),
        "body":format!("Fixture provider activity {id}"),
        "user":{"id":author,"login":if author == 91 {"fixture-bot"} else {"fixture-human"},"type": if author == 91 {"Bot"} else {"User"}},
        "created_at":at,"updated_at":at
    });
    let (key, family) = if inline {
        ("pull_request_url", "pulls")
    } else {
        ("issue_url", "issues")
    };
    result[key] = json!(format!("{API}/{family}/{number}"));
    result
}

fn review(id: u64, at: &str, bot: bool) -> Value {
    json!({"__typename":"PullRequestReview","fullDatabaseId":id,"state":"COMMENTED",
        "submittedAt":at,"body":if bot {"Fixture bot retained original review"} else {"Fixture human retained original review"},
        "author":{"__typename":if bot {"Bot"} else {"User"},"databaseId":if bot {91} else {92}}})
}

fn merge(at: &str) -> Value {
    json!({"__typename":"MergedEvent","createdAt":at,"actor":{"__typename":"User","databaseId":92},"commit":{"oid":MOVED}})
}

fn revision_identity(number: u64, merged: bool) -> Value {
    json!({
        "id":format!("PR_{number}"), "fullDatabaseId":720+number, "number":number,
        "title":format!("Activity fixture pull request {number}"),
        "closed":merged,"merged":merged,"updatedAt":date(0),"baseRefOid":BASE,"headRefOid":HEAD
    })
}

fn commit(oid: &str, parents: Value, stamp: &str) -> Value {
    json!({"sha":oid, "parents":parents,"commit":{"message":"Activity fixture branch commit",
        "author":{"name":"Fixture human","date":stamp},"committer":{"name":"Fixture human","date":stamp}}})
}

pub(super) fn response(phase: Phase, route: Route) -> Option<Response> {
    if !active(phase) {
        return None;
    }
    let fresh = matches!(phase, Phase::ActivityFresh);
    let catchup = matches!(phase, Phase::ActivityCatchup | Phase::ActivityReplay);
    let old = date(240);
    let recent = date(0);
    let delayed = date(20);
    let reply = |body| {
        Some(Response {
            status: 200,
            reason: "OK",
            body,
            sso: None,
        })
    };
    match route {
        Route::Items | Route::UpdatedItems => reply(json!([
            summary(42, catchup, &recent),
            summary(43, true, &old)
        ])),
        Route::UpdatedComments if fresh => reply(json!([message(6101, 44, &recent, 92, false)])),
        Route::UpdatedComments if catchup => reply(json!([
            message(6102, 44, &delayed, 92, false),
            message(6103, 45, &delayed, 92, false)
        ])),
        Route::UpdatedInlineComments if fresh => reply(json!([
            message(6201, 42, &recent, 91, true),
            message(6202, 42, &recent, 92, true)
        ])),
        Route::UpdatedComments | Route::UpdatedInlineComments => reply(json!([])),
        Route::OpenPullIndex if catchup => reply(json!([])),
        Route::OpenPullIndex => reply(json!([{"id":762,"node_id":"PR_42","number":42,
            "updated_at":recent,"base":{"repo":{"id":71}}}])),
        Route::PullA => reply(pull_meta(42, catchup)),
        Route::PullB => reply(pull_meta(43, true)),
        Route::PullActivityA | Route::PullActivityB => {
            let historical = route == Route::PullActivityB;
            let mut pull =
                revision_identity(if historical { 43 } else { 42 }, historical || catchup);
            let events = if historical {
                vec![merge(&old)]
            } else if fresh {
                vec![review(6301, &recent, true), review(6302, &recent, false)]
            } else if catchup {
                vec![merge(&delayed)]
            } else {
                vec![review(6300, &old, false)]
            };
            pull["timelineItems"] =
                json!({"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":events});
            reply(
                json!({"data":{"repository":{"databaseId":71,"name":"private",
                "owner":{"login":"fixture-org"},"pullRequest":pull}}}),
            )
        }
        Route::ActivityReviews => {
            let mut pull = revision_identity(42, false);
            pull["repository"] =
                json!({"id":"R_71","name":"private","owner":{"login":"fixture-org"}});
            pull["reviews"] = json!({"pageInfo":{"hasNextPage":false,"endCursor":null},
            "nodes":[
            {"id":"R_6301","databaseId":6301,"fullDatabaseId":6301,"state":"COMMENTED",
                "body":"Fixture bot retained original review","submittedAt":recent,
                "author":{"login":"fixture-bot","__typename":"Bot","databaseId":91},"commit":{"oid":HEAD}},
            {"id":"R_6302","databaseId":6302,"fullDatabaseId":6302,"state":"COMMENTED",
                "body":"Fixture human retained original review","submittedAt":recent,
                "author":{"login":"fixture-human","__typename":"User","databaseId":92},"commit":{"oid":HEAD}}
            ]});
            reply(json!({"data":{"node":pull}}))
        }
        Route::ActivityBranches => reply(json!([
            {"name":"main","commit":{"sha":if catchup {MOVED} else {BASE}}},
            {"name":"unwatched","commit":{"sha":if catchup {QUIET_BRANCH} else {HEAD}}}
        ])),
        Route::ActivityWatched => {
            reply(json!({"name":"main","commit":{"sha":if catchup {MOVED} else {BASE}}}))
        }
        Route::ActivityBaselineTip => reply(json!([commit(BASE, json!([]), &old)])),
        Route::ActivityMovedTip => reply(json!([
            commit(MOVED, json!([{"sha":BASE}]), &delayed),
            commit(BASE, json!([]), &old)
        ])),
        Route::ActivityOldReview => reply(json!({"total_count":0,"check_runs":[]})),
        Route::ActivityThreads => reply(json!([])),
        Route::CompareA => pr::response(Phase::PrLoad, route),
        _ => None,
    }
}

pub(super) fn trace(records: &[RequestEvidence]) -> bool {
    let has = |phase, route| {
        records
            .iter()
            .any(|row| row.phase == phase && row.route == route && row.status == 200)
    };
    records.iter().all(|row| {
        row.fixture_authorization
            && row.phase != Phase::ActivityOffline
            && !matches!(
                row.route,
                Route::Unexpected
                    | Route::ActivityUnwatched
                    | Route::MergePut
                    | Route::ReviewCreated
                    | Route::ReviewSubmitted
            )
            && row.status == 200
    }) && has(Phase::ActivityBaseline, Route::PullActivityB)
        && has(Phase::ActivityFresh, Route::UpdatedComments)
        && has(Phase::ActivityFresh, Route::UpdatedInlineComments)
        && has(Phase::ActivityFresh, Route::PullActivityA)
        && has(Phase::ActivityCatchup, Route::PullActivityA)
        && has(Phase::ActivityCatchup, Route::ActivityMovedTip)
        && has(Phase::ActivityReplay, Route::UpdatedComments)
}
