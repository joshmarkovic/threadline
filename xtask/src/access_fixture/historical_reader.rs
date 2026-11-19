use super::{Phase, RequestEvidence, Response, Route};
use serde_json::json;

const SEARCH: &str =
    "/search/issues?q=repo%3Afixture-org%2Fprivate+%22Observatory%22&per_page=20&page=1";
const DETAILS: &str = "/repos/fixture-org/private/issues/719";
const SCHEDULED_DETAILS: &str = "/repositories/71/issues/719";

pub(super) fn get(phase: Phase, target: &str) -> Option<Route> {
    if !matches!(phase, Phase::HistoricalLocal | Phase::HistoricalRemote) {
        return None;
    }
    match target {
        SEARCH => Some(Route::HistoricalSearch),
        DETAILS | SCHEDULED_DETAILS => Some(Route::HistoricalIssue),
        _ => None,
    }
}

pub(super) fn response(phase: Phase, route: Route) -> Option<Response> {
    if !matches!(phase, Phase::HistoricalLocal | Phase::HistoricalRemote) {
        return None;
    }
    let body = match (phase, route) {
        (Phase::HistoricalRemote, Route::HistoricalSearch | Route::HistoricalIssue) => {
            let mut issue = json!({
                "id":7190, "node_id":"I_7190", "number":719, "title":"Observatory logbook",
                "state":"closed", "created_at":"2018-01-01T00:00:00Z",
                "closed_at":"2018-01-03T00:00:00Z", "updated_at":"2018-01-03T00:00:00Z",
                "repository_url":"https://api.github.com/repos/fixture-org/private",
                "user":{"id":41,"login":"fixture-user"}, "labels":[], "assignees":[]
            });
            if route == Route::HistoricalSearch {
                json!({"total_count":1, "incomplete_results":false, "items":[issue]})
            } else {
                issue["body"] = json!("The Xenolith catalogue was sealed in the field.");
                issue["updated_at"] = json!("2018-01-04T00:00:00Z");
                issue
            }
        }
        (
            _,
            Route::UpdatedItems
            | Route::UpdatedComments
            | Route::UpdatedInlineComments
            | Route::OpenPullIndex,
        ) => json!([]),
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
    let mut search_seen = false;
    let mut details_seen = false;
    for record in records {
        if !record.fixture_authorization || record.status != 200 {
            return false;
        }
        match (record.phase, record.route) {
            (
                Phase::HistoricalLocal | Phase::HistoricalRemote,
                Route::User
                | Route::Discovery
                | Route::Repository
                | Route::Graphql
                | Route::UpdatedItems
                | Route::UpdatedComments
                | Route::UpdatedInlineComments
                | Route::OpenPullIndex,
            ) => (),
            (Phase::HistoricalRemote, Route::HistoricalSearch) if !search_seen => {
                search_seen = true
            }
            (Phase::HistoricalRemote, Route::HistoricalIssue) if search_seen => details_seen = true,
            _ => return false,
        }
    }
    search_seen && details_seen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_http_returns_the_full_body_only_for_the_target_and_exports_redacted_receipts() {
        use crate::access_fixture::{AccessFixture, FIXTURE_AUTHORIZATION, TraceCheck};
        use std::io::{Read, Write};
        use std::net::TcpStream;

        let dir = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(dir.path()).unwrap();
        std::fs::write(fixture.phase_file(), "historical-remote").unwrap();
        let get = |path: &str| {
            let mut socket = TcpStream::connect(fixture.address()).unwrap();
            write!(
                socket,
                "GET {path} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\n\r\n"
            )
            .unwrap();
            let mut response = String::new();
            socket.read_to_string(&mut response).unwrap();
            assert!(response.starts_with("HTTP/1.1 200"));
            response
        };
        let search = get(SEARCH);
        assert!(search.contains("Observatory logbook"));
        assert!(!search.contains("Xenolith"));
        assert!(get(DETAILS).contains("Xenolith"));
        fixture
            .finish(dir.path(), TraceCheck::HistoricalReader)
            .unwrap();
        let trace = std::fs::read_to_string(dir.path().join("github-access-trace.json")).unwrap();
        assert!(!trace.contains(FIXTURE_AUTHORIZATION) && !trace.contains("Xenolith"));
    }

    #[test]
    fn historical_read_requires_the_scoped_search_before_detail_and_no_offline_or_bulk_request() {
        let search = get(Phase::HistoricalRemote, SEARCH).unwrap();
        let details = get(Phase::HistoricalRemote, DETAILS).unwrap();
        assert_eq!(
            get(Phase::HistoricalRemote, "/search/issues?q=Observatory"),
            None
        );
        let search_body = response(Phase::HistoricalRemote, search).unwrap().body;
        let body = response(Phase::HistoricalRemote, details).unwrap().body;
        assert_eq!(search_body["items"][0]["closed_at"], "2018-01-03T00:00:00Z");
        assert!(search_body["items"][0].get("body").is_none());
        assert_eq!(body["number"], 719);
        assert_eq!(body["state"], "closed");
        assert!(body["body"].as_str().unwrap().contains("Xenolith"));
        let entry = |phase, route| RequestEvidence {
            phase,
            route,
            status: 200,
            fixture_authorization: true,
        };
        let mut records = vec![
            entry(Phase::HistoricalLocal, Route::Repository),
            entry(Phase::HistoricalRemote, search),
            entry(Phase::HistoricalRemote, details),
        ];
        assert!(trace(&records));
        records.swap(1, 2);
        assert!(!trace(&records));
        records.swap(1, 2);
        for extra in [
            entry(Phase::SyncOffline, details),
            entry(Phase::HistoricalRemote, Route::Items),
            entry(Phase::HistoricalLocal, search),
        ] {
            records.push(extra);
            assert!(!trace(&records));
            records.pop();
        }
    }
}
