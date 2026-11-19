use super::*;

pub(super) const COMMENTS_NEXT: &str =
    "/repos/fixture-org/private/issues/42/comments?per_page=20&page=2";
const COMMENTS_FIRST: &str = "/repos/fixture-org/private/issues/42/comments?per_page=20&page=1";
const FIRST_HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const FIRST_BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

pub(super) fn active(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::PrPinLoad | Phase::PrPinInterrupt | Phase::PrPinContinue
    )
}

pub(super) fn get(phase: Phase, path: &str) -> Option<Route> {
    if !active(phase) {
        return None;
    }
    match path {
        "/repositories/71/issues/42" => Some(Route::PrPinItem42),
        "/repositories/71/issues/43" => Some(Route::PrPinItem43),
        COMMENTS_FIRST => Some(Route::PrPinCommentsFirst),
        COMMENTS_NEXT => Some(Route::PrPinCommentsNext),
        _ if path
            == format!(
                "/repositories/71/commits/{FIRST_HEAD}/check-runs?filter=all&per_page=20&page=1"
            ) =>
        {
            Some(Route::PrPinChecks)
        }
        _ if path
            == format!("/repositories/71/commits/{FIRST_HEAD}/statuses?per_page=20&page=1") =>
        {
            Some(Route::PrPinStatuses)
        }
        _ => None,
    }
}

pub(super) fn graphql(phase: Phase, request: &Value) -> Option<Route> {
    if !active(phase) || request.get("variables")? != &json!({"id":"PR_42","cursor":null}) {
        return None;
    }
    let query = request.get("query")?.as_str()?;
    if query.starts_with("query ThreadlineReviews(") {
        Some(Route::PrPinReviews)
    } else if query.starts_with("query ThreadlineThreads(") {
        Some(Route::PrPinThreads)
    } else {
        None
    }
}

pub(super) fn comparison_files() -> Vec<Value> {
    let mut files = vec![
        json!({"filename":"src/binary.dat","status":"modified","additions":0,"deletions":0,
            "patch":"Binary files a/src/binary.dat and b/src/binary.dat differ"}),
        json!({"filename":"src/truncated.rs","status":"modified","additions":3,"deletions":1,
            "patch":"@@ -1 +1 @@\n-before\n+partial"}),
        json!({"filename":"src/unavailable.rs","status":"modified","additions":1,"deletions":1}),
    ];
    files.extend((3..300).map(|index| {
        json!({"filename":format!("src/fixture-{index:03}.rs"),"status":"added",
            "additions":1,"deletions":0,"patch":"@@ -0,0 +1 @@\n+cached"})
    }));
    files
}

pub(super) fn response(phase: Phase, route: Route) -> Option<Response> {
    if !active(phase) {
        return None;
    }
    if phase == Phase::PrPinInterrupt && route == Route::PrPinCommentsNext {
        return Some(Response {
            status: 502,
            reason: "Bad Gateway",
            body: json!({"message":"Synthetic interruption on the second discussion page"}),
            sso: None,
        });
    }
    let body = match route {
        Route::PrPinCommentsFirst | Route::PrPinCommentsNext => {
            let first = route == Route::PrPinCommentsFirst;
            let id = if first { 5001 } else { 5002 };
            json!([{
                "id":id,"node_id":format!("IC_{id}"),"user":{"login":"fixture-author","id":75},
                "body":if first { "First cached pull request discussion page" } else { "Resumed pull request discussion page" },
                "created_at":"2021-01-01T00:00:00Z","updated_at":"2021-01-01T00:00:00Z",
                "issue_url":"https://api.github.com/repos/fixture-org/private/issues/42"
            }])
        }
        Route::PrPinChecks => json!({"total_count":0,"check_runs":[]}),
        Route::PrPinStatuses | Route::UpdatedItems | Route::UpdatedComments => json!([]),
        Route::PrPinReviews | Route::PrPinThreads => {
            let connection = if route == Route::PrPinReviews {
                "reviews"
            } else {
                "reviewThreads"
            };
            json!({"data":{"node":{
                "id":"PR_42","fullDatabaseId":762,"baseRefOid":FIRST_BASE,"headRefOid":FIRST_HEAD,
                "repository":{"id":"R_71","name":"private","owner":{"login":"fixture-org"}},
                (connection):{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[]}
            }}})
        }
        _ => return None,
    };
    Some(Response {
        status: 200,
        reason: "OK",
        body,
        sso: None,
    })
}

pub(super) fn mark(phase: Phase, route: Route, status: u16, phase_file: &Path) -> io::Result<()> {
    let marker = match (phase, route, status) {
        (Phase::PrPinInterrupt, Route::PrPinCommentsNext, 502) => "pr-pin-interrupted.txt",
        (Phase::PrPinContinue, Route::PrPinStatuses, 200) => "pr-pin-resumed.txt",
        _ => return Ok(()),
    };
    fs::write(phase_file.with_file_name(marker), b"observed")
}

pub(super) fn trace(records: &[RequestEvidence]) -> bool {
    let discussion = records
        .iter()
        .filter(|entry| {
            matches!(
                entry.route,
                Route::PrPinCommentsFirst | Route::PrPinCommentsNext
            )
        })
        .map(|entry| (entry.phase, entry.route, entry.status))
        .collect::<Vec<_>>();
    discussion
        == [
            (Phase::PrPinInterrupt, Route::PrPinCommentsFirst, 200),
            (Phase::PrPinInterrupt, Route::PrPinCommentsNext, 502),
            (Phase::PrPinContinue, Route::PrPinCommentsNext, 200),
        ]
        && [
            Route::PrPinReviews,
            Route::PrPinThreads,
            Route::PrPinChecks,
            Route::PrPinStatuses,
        ]
        .into_iter()
        .all(|route| {
            records.iter().any(|entry| {
                entry.phase == Phase::PrPinContinue && entry.route == route && entry.status == 200
            })
        })
        && records.iter().any(|entry| {
            entry.phase == Phase::PrPinLoad && entry.route == Route::CompareA && entry.status == 200
        })
        && records.iter().all(|entry| {
            entry.fixture_authorization
                && active(entry.phase)
                && ((entry.phase, entry.route, entry.status)
                    == (Phase::PrPinInterrupt, Route::PrPinCommentsNext, 502)
                    || entry.status == 200
                        && matches!(
                            entry.route,
                            Route::User
                                | Route::Repository
                                | Route::Items
                                | Route::Discovery
                                | Route::Graphql
                                | Route::UpdatedItems
                                | Route::UpdatedComments
                                | Route::UpdatedInlineComments
                                | Route::OpenPullIndex
                                | Route::PullActivityA
                                | Route::PullActivityB
                                | Route::PullA
                                | Route::PullB
                                | Route::CompareA
                                | Route::CompareB
                                | Route::PrPinItem42
                                | Route::PrPinItem43
                                | Route::PrPinCommentsFirst
                                | Route::PrPinCommentsNext
                                | Route::PrPinReviews
                                | Route::PrPinThreads
                                | Route::PrPinChecks
                                | Route::PrPinStatuses
                        ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_pin_requires_the_original_interrupted_cursor_and_bounded_comparison() {
        let mut records = Vec::from(
            [
                (Phase::PrPinLoad, Route::CompareA, 200),
                (Phase::PrPinInterrupt, Route::PrPinCommentsFirst, 200),
                (Phase::PrPinInterrupt, Route::PrPinCommentsNext, 502),
                (Phase::PrPinContinue, Route::PrPinCommentsNext, 200),
                (Phase::PrPinContinue, Route::PrPinReviews, 200),
                (Phase::PrPinContinue, Route::PrPinThreads, 200),
                (Phase::PrPinContinue, Route::PrPinChecks, 200),
                (Phase::PrPinContinue, Route::PrPinStatuses, 200),
            ]
            .map(|(phase, route, status)| RequestEvidence {
                phase,
                route,
                status,
                fixture_authorization: true,
            }),
        );
        assert!(trace(&records));
        records[3].route = Route::PrPinCommentsFirst;
        assert!(!trace(&records));
        records[3].route = Route::PrPinCommentsNext;
        records.push(RequestEvidence {
            phase: Phase::PrPinOffline,
            route: Route::PrPinCommentsNext,
            status: 200,
            fixture_authorization: true,
        });
        assert!(!trace(&records));
        assert_eq!(comparison_files().len(), 300);
        assert_eq!(
            get(Phase::PrPinContinue, COMMENTS_NEXT),
            Some(Route::PrPinCommentsNext)
        );
        assert_eq!(get(Phase::PrPinOffline, COMMENTS_NEXT), None);
    }
}
