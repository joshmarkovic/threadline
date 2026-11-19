use super::*;

pub(super) const RETRY_SECONDS: u64 = 180;

pub(super) fn response(phase: Phase, route: Route) -> Option<Response> {
    let (status, reason, body) = match (phase, route) {
        (Phase::SyncRateThrottle, Route::User) => (
            429,
            "Too Many Requests",
            json!({"message":"The synthetic account is throttled"}),
        ),
        (Phase::SyncRateFloor, _) => (
            503,
            "Service Unavailable",
            json!({"message":"A fixture request arrived during the retry floor"}),
        ),
        (
            Phase::SyncRateReady | Phase::SyncRateThrottle,
            Route::Items
            | Route::Item
            | Route::ScheduledIssue
            | Route::UpdatedItems
            | Route::UpdatedComments
            | Route::Timeline,
        ) => return writes::response(Phase::WriteOnline, route),
        _ => return None,
    };
    Some(Response {
        status,
        reason,
        body,
        sso: None,
    })
}

pub(super) fn trace(records: &[RequestEvidence]) -> bool {
    let Some((throttle, setup)) = records.split_last() else {
        return false;
    };
    throttle.fixture_authorization
        && throttle.phase == Phase::SyncRateThrottle
        && throttle.route == Route::User
        && throttle.status == 429
        && setup.iter().any(|entry| entry.route == Route::Items)
        && setup.iter().all(|entry| {
            entry.fixture_authorization
                && matches!(entry.phase, Phase::SyncRateReady | Phase::SyncRateThrottle)
                && entry.status == 200
                && matches!(
                    entry.route,
                    Route::User
                        | Route::Repository
                        | Route::Items
                        | Route::Item
                        | Route::ScheduledIssue
                        | Route::UpdatedItems
                        | Route::UpdatedComments
                        | Route::UpdatedInlineComments
                        | Route::OpenPullIndex
                        | Route::Timeline
                )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_rate_limit_requires_no_requests_after_the_429_and_disallows_mutations() {
        let directory = tempfile::tempdir().unwrap();
        let mut fixture = AccessFixture::start(directory.path()).unwrap();
        let request = |phase, method, path, body: &str| {
            fs::write(fixture.phase_file(), phase).unwrap();
            let mut stream = TcpStream::connect(fixture.address()).unwrap();
            write!(stream, "{method} {path} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            response
        };
        assert!(request("sync-rate-ready", "GET", ITEMS, "").starts_with("HTTP/1.1 200"));
        let throttle = request("sync-rate-throttle", "GET", "/user", "");
        assert!(throttle.starts_with("HTTP/1.1 429"));
        assert!(throttle.contains("Retry-After: 180\r\n"));
        assert!(request("sync-rate-floor", "GET", "/user", "").starts_with("HTTP/1.1 503"));
        let body = r#"{"body":"Threadline S07 explicitly queued comment."}"#;
        assert!(
            request(
                "sync-rate-ready",
                "POST",
                "/repos/fixture-org/private/issues/41/comments",
                body
            )
            .starts_with("HTTP/1.1 404")
        );
        let observed = fixture.stop().unwrap();
        assert!(observed.error.is_none());
        assert!(trace(&observed.records[..2]));
        assert!(!trace(&observed.records[..3]));
        assert!(!trace(&observed.records));
    }
}
