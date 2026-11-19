use super::*;

const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const OTHER: &str = "cccccccccccccccccccccccccccccccccccccccc";
const LEFT_TREE: &str = "dddddddddddddddddddddddddddddddddddddddd";
const RIGHT_TREE: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const SHARED_TREE: &str = "ffffffffffffffffffffffffffffffffffffffff";
const LEFT_BLOB: &str = "1111111111111111111111111111111111111111";
const RIGHT_BLOB: &str = "2222222222222222222222222222222222222222";
const TIME: &str = "2026-01-02T03:04:05Z";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ExactObject {
    LeftCommit,
    RightCommit,
    LeftTree,
    RightTree,
    LeftBlob,
    RightBlob,
}

impl ExactObject {
    const ALL: [Self; 6] = [
        Self::LeftCommit,
        Self::RightCommit,
        Self::LeftTree,
        Self::RightTree,
        Self::LeftBlob,
        Self::RightBlob,
    ];

    fn target(self) -> String {
        let (resource, oid) = match self {
            Self::LeftCommit => ("commits", BASE),
            Self::RightCommit => ("commits", OTHER),
            Self::LeftTree => ("trees", LEFT_TREE),
            Self::RightTree => ("trees", RIGHT_TREE),
            Self::LeftBlob => ("blobs", LEFT_BLOB),
            Self::RightBlob => ("blobs", RIGHT_BLOB),
        };
        format!("/repositories/71/git/{resource}/{oid}")
    }

    fn response(self) -> Value {
        let tree = |oid, blob, size| {
            json!({"sha":oid,"truncated":false,"tree":[
                {"path":"divergent.txt","type":"blob","mode":"100644","size":size,"sha":blob},
                {"path":"unchanged","type":"tree","mode":"040000","sha":SHARED_TREE}
            ]})
        };
        match self {
            Self::LeftCommit => json!({"sha":BASE,"tree":{"sha":LEFT_TREE}}),
            Self::RightCommit => json!({"sha":OTHER,"tree":{"sha":RIGHT_TREE}}),
            Self::LeftTree => tree(LEFT_TREE, LEFT_BLOB, 10),
            Self::RightTree => tree(RIGHT_TREE, RIGHT_BLOB, 11),
            Self::LeftBlob => {
                json!({"sha":LEFT_BLOB,"encoding":"base64","size":10,"content":"bGVmdCBsaW5lCg=="})
            }
            Self::RightBlob => {
                json!({"sha":RIGHT_BLOB,"encoding":"base64","size":11,"content":"cmlnaHQgbGluZQo="})
            }
        }
    }
}

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
        target
            if target
                == format!("/repositories/71/compare/{BASE}...{OTHER}?per_page=20&page=1") =>
        {
            Some(Route::HistoryDivergentPair)
        }
        target => ExactObject::ALL
            .into_iter()
            .find(|object| object.target() == target)
            .map(Route::HistoryExactObject),
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
        Route::HistoryDivergentPair => {
            return Some(Response {
                status: 404,
                reason: "Not Found",
                body: json!({"message":"No common ancestor in the synthetic comparison"}),
                sso: None,
            });
        }
        Route::HistoryExactObject(object) => object.response(),
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
    records.iter().all(|row| {
        row.fixture_authorization
            && row.route != Route::Unexpected
            && row.status
                == if row.route == Route::HistoryDivergentPair {
                    404
                } else {
                    200
                }
    }) && records
        .iter()
        .filter(|row| row.route == Route::HistoryDivergentPair)
        .count()
        == 1
        && ExactObject::ALL.into_iter().all(|object| {
            records
                .iter()
                .filter(|row| row.route == Route::HistoryExactObject(object))
                .count()
                == 1
        })
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
