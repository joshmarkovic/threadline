use super::*;

pub(super) fn active(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::RecoverBaseline
            | Phase::RecoverExpired
            | Phase::RecoverRebuild
            | Phase::RecoverResume
    )
}

pub(super) fn response(phase: Phase, route: Route) -> Option<Response> {
    if !active(phase) {
        return None;
    }
    let comment = |id, edited, body| {
        json!({
            "id":id, "node_id":format!("IC_{id}"), "body":body, "user":{"id":75,"login":"fixture-author"},
            "created_at":"2018-01-02T00:00:00Z",
            "updated_at":if edited { "2018-01-05T00:00:00Z" } else { "2018-01-02T00:00:00Z" },
            "issue_url":"https://api.github.com/repos/fixture-org/private/issues/41"
        })
    };
    let (status, reason, body) = match (phase, route) {
        (Phase::RecoverBaseline, Route::CommentsFirst) => (
            200,
            "OK",
            json!([comment(
                5101,
                false,
                "The original cached discussion comment."
            )]),
        ),
        (Phase::RecoverExpired, Route::CommentsNext) => {
            (410, "Gone", json!({"message":"Synthetic cursor expired"}))
        }
        (Phase::RecoverRebuild, Route::CommentsFirst) => (
            200,
            "OK",
            json!([
                comment(
                    5101,
                    true,
                    "The original comment changed while Threadline was closed."
                ),
                comment(5102, false, "A comment repeated on the next page.")
            ]),
        ),
        (Phase::RecoverRebuild | Phase::RecoverResume, Route::CommentsNext) => (
            200,
            "OK",
            json!([
                comment(5102, false, "A comment repeated on the next page."),
                comment(
                    5103,
                    false,
                    "The page whose first publication was interrupted."
                )
            ]),
        ),
        (_, Route::Timeline) => (
            200,
            "OK",
            json!([{
                "id":8101,"node_id":"TE_8101","event":"labeled",
                "created_at":"2018-01-03T00:00:00Z","actor":{"id":75,"login":"fixture-author"},
                "label":{"name":"fixture-retained"}
            }]),
        ),
        (_, Route::CommentsFirst | Route::CommentsNext) => return None,
        _ => return writes::response(Phase::WriteOnline, route),
    };
    Some(Response {
        status,
        reason,
        body,
        sso: None,
    })
}

pub(super) fn trace(rows: &[RequestEvidence]) -> bool {
    rows.iter()
        .filter(|row| matches!(row.route, Route::CommentsFirst | Route::CommentsNext))
        .map(|row| (row.phase, row.route, row.status))
        .eq([
            (Phase::RecoverBaseline, Route::CommentsFirst, 200),
            (Phase::RecoverExpired, Route::CommentsNext, 410),
            (Phase::RecoverRebuild, Route::CommentsFirst, 200),
            (Phase::RecoverRebuild, Route::CommentsNext, 200),
            (Phase::RecoverResume, Route::CommentsNext, 200),
        ])
        && rows
            .iter()
            .filter(|row| row.route == Route::Timeline)
            .map(|row| (row.phase, row.status))
            .eq([(Phase::RecoverBaseline, 200)])
        && rows.iter().all(|row| {
            row.fixture_authorization
                && active(row.phase)
                && (row.status == 200
                    && matches!(
                        row.route,
                        Route::User
                            | Route::Repository
                            | Route::Discovery
                            | Route::Items
                            | Route::Item
                            | Route::ScheduledIssue
                            | Route::Timeline
                            | Route::CommentsFirst
                            | Route::CommentsNext
                            | Route::UpdatedItems
                            | Route::UpdatedComments
                            | Route::UpdatedInlineComments
                            | Route::OpenPullIndex
                            | Route::Graphql
                    )
                    || (row.phase, row.route, row.status)
                        == (Phase::RecoverExpired, Route::CommentsNext, 410))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_retains_exact_repair_and_post_failure_page_sequence() {
        let mut rows = Vec::from(
            [
                (Phase::RecoverBaseline, Route::Timeline, 200),
                (Phase::RecoverBaseline, Route::CommentsFirst, 200),
                (Phase::RecoverExpired, Route::CommentsNext, 410),
                (Phase::RecoverRebuild, Route::CommentsFirst, 200),
                (Phase::RecoverRebuild, Route::CommentsNext, 200),
                (Phase::RecoverResume, Route::CommentsNext, 200),
            ]
            .map(|(phase, route, status)| RequestEvidence {
                phase,
                route,
                status,
                fixture_authorization: true,
            }),
        );
        assert!(trace(&rows));
        rows[5].route = Route::CommentsFirst;
        assert!(!trace(&rows));
        let original = response(Phase::RecoverBaseline, Route::CommentsFirst)
            .unwrap()
            .body;
        let rebuilt = response(Phase::RecoverRebuild, Route::CommentsFirst)
            .unwrap()
            .body;
        let terminal = response(Phase::RecoverResume, Route::CommentsNext)
            .unwrap()
            .body;
        assert_eq!(original[0]["id"], rebuilt[0]["id"]);
        assert_ne!(original[0]["body"], rebuilt[0]["body"]);
        assert_eq!(rebuilt[1]["id"], terminal[0]["id"]);
        assert_eq!(terminal[1]["id"], 5103);
        assert!(response(Phase::RecoverOffline, Route::CommentsFirst).is_none());
    }
}
