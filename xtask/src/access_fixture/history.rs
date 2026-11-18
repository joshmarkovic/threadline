use super::*;

const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OTHER: &str = "cccccccccccccccccccccccccccccccccccccccc";
const TIME: &str = "2026-01-02T03:04:05Z";

pub(super) fn older_target() -> String {
    format!("/repositories/71/commits?sha={HEAD}&per_page=20&page=2")
}

fn comparison(base: &str) -> String {
    format!("/repositories/71/compare/{base}...{HEAD}?per_page=20&page=1")
}

pub(super) fn route(phase: Phase, target: &str) -> Option<Route> {
    if phase != Phase::History {
        return None;
    }
    match target {
        "/repositories/71/branches?per_page=20&page=1" => Some(Route::HistoryBranches),
        "/repositories/71/tags?per_page=20&page=1" => Some(Route::HistoryTags),
        "/repositories/71/branches/main" => Some(Route::HistoryBranch),
        target if target == format!("/repositories/71/commits?sha={HEAD}&per_page=20&page=1") => {
            Some(Route::HistoryFirst)
        }
        target if target == older_target() => Some(Route::HistoryOlder),
        target if target == comparison(OTHER) => Some(Route::HistoryParentDiff),
        target if target == comparison(BASE) => Some(Route::HistoryPairDiff),
        _ => None,
    }
}

pub(super) fn response(phase: Phase, route: Route) -> Option<Response> {
    if phase != Phase::History {
        return None;
    }
    let commit = |oid, title, parents| {
        json!({
            "sha":oid, "parents":parents, "commit":{
                "message":title, "author":{"name":"Synthetic author","date":TIME},
                "committer":{"name":"Synthetic committer","date":TIME}
            }
        })
    };
    let compare = |base, file| {
        json!({
            "base_commit":{"sha":base}, "merge_base_commit":{"sha":base},
            "files":[{"filename":file,"status":"modified","additions":1,"deletions":1,
                      "patch":"@@ -1 +1 @@\n-old fixture entry\n+new fixture entry"}]
        })
    };
    let body = match route {
        Route::HistoryBranches => json!([{"name":"main","commit":{"sha":HEAD}}]),
        Route::HistoryTags => json!([{"name":"fixture-v1","commit":{"sha":BASE}}]),
        Route::HistoryBranch => json!({"name":"main","commit":{"sha":HEAD}}),
        Route::HistoryFirst => {
            json!([commit(
                HEAD,
                "Synthetic merge commit",
                json!([{"sha":BASE},{"sha":OTHER}])
            )])
        }
        Route::HistoryOlder => json!([
            commit(BASE, "First parent", json!([])),
            commit(OTHER, "Second parent", json!([])),
        ]),
        Route::HistoryParentDiff => compare(OTHER, "parent-two.txt"),
        Route::HistoryPairDiff => compare(BASE, "pair.txt"),
        _ => return None,
    };
    Some(Response {
        status: 200,
        reason: "OK",
        body,
        sso: None,
    })
}

pub(super) fn trace(records: &[RequestEvidence]) -> bool {
    records
        .iter()
        .all(|row| row.fixture_authorization && row.route != Route::Unexpected)
        && [
            Route::HistoryBranches,
            Route::HistoryTags,
            Route::HistoryBranch,
            Route::HistoryFirst,
            Route::HistoryOlder,
            Route::HistoryParentDiff,
            Route::HistoryPairDiff,
        ]
        .into_iter()
        .all(|route| {
            records.iter().any(|entry| {
                entry.phase == Phase::History && entry.route == route && entry.status == 200
            })
        })
}
