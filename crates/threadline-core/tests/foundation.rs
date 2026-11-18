mod support;

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use axum::http::{Method, StatusCode, header};
use serde_json::json;
use support::{Reply, Simulator};
use threadline_core::domain::{ItemKind, Revision};
use threadline_core::fixtures::FeasibilityFixture;
use threadline_core::github::{Provider, ProviderError, RestProvider};
use threadline_core::sync::{Cancellation, Scheduler};
use tokio::runtime::Handle;
use tokio::sync::Notify;

const SUMMARY_PATH: &str = "/repos/threadline-fixtures/signal-lab/issues/42";

fn payload(number: u64, pull_request: bool) -> serde_json::Value {
    let mut payload = json!({
        "id": 4201,
        "number": number,
        "repository": { "id": 120 },
        "title": "Native integration fixture",
        "body": null,
        "updated_at": "2026-11-17T12:00:00Z"
    });
    if pull_request {
        payload["pull_request"] = json!({ "url": "https://api.github.com/example" });
    }
    payload
}

#[tokio::test]
async fn reads_a_versioned_issue_endpoint_with_correct_pr_identity_and_revision() {
    let fixture = FeasibilityFixture::new(42);
    let simulator = Simulator::start().await;
    simulator.enqueue(
        Method::GET,
        SUMMARY_PATH,
        Reply::new(StatusCode::OK, payload(42, true).to_string())
            .header(header::ETAG, "W/\"summary-v1\""),
    );
    let provider = RestProvider::loopback(simulator.address).unwrap();

    let result = provider.read_summary(&fixture.request).await.unwrap();

    assert_eq!(result.item(), fixture.request.item());
    assert_eq!(result.kind, ItemKind::PullRequest);
    assert_eq!(result.issue_api_id.get(), 4201);
    assert_eq!(
        result.revision(),
        &Revision::EntityTag("W/\"summary-v1\"".into())
    );
    assert_eq!(result.title(), "Native integration fixture");
    assert_eq!(result.body(), None);
    let log = simulator.take_observations();
    assert_eq!(log.len(), 1);
    assert!(log[0].matched && log[0].versioned && log[0].expected_media_type);
    assert!(!log[0].authorization_sent);
}

#[tokio::test]
async fn same_item_number_from_another_repository_cannot_be_relabelled() {
    let fixture = FeasibilityFixture::new(42);
    let simulator = Simulator::start().await;
    let mut payload = payload(42, true);
    payload["repository"]["id"] = json!(121);
    simulator.enqueue(
        Method::GET,
        SUMMARY_PATH,
        Reply::new(StatusCode::OK, payload.to_string()),
    );
    let provider = RestProvider::loopback(simulator.address).unwrap();

    assert_eq!(
        provider.read_summary(&fixture.request).await.unwrap_err(),
        ProviderError::WrongResource
    );
    let log = simulator.take_observations();
    assert_eq!(log.len(), 1);
    assert!(log[0].matched);
}

#[tokio::test]
async fn reports_http_and_payload_failures_and_ignores_redirects() {
    let fixture = FeasibilityFixture::new(42);
    let simulator = Simulator::start().await;
    let provider = RestProvider::loopback(simulator.address).unwrap();
    let mut without_body = payload(42, false);
    without_body.as_object_mut().unwrap().remove("body");
    let mut without_repository = payload(42, false);
    without_repository
        .as_object_mut()
        .unwrap()
        .remove("repository");
    let cases = [
        (
            Reply::new(StatusCode::UNAUTHORIZED, "private response"),
            ProviderError::Unauthorized,
        ),
        (
            Reply::new(StatusCode::FORBIDDEN, "private response"),
            ProviderError::Forbidden,
        ),
        (
            Reply::new(StatusCode::NOT_FOUND, "private response"),
            ProviderError::NotFound,
        ),
        (
            Reply::new(StatusCode::TOO_MANY_REQUESTS, "private response")
                .header(header::RETRY_AFTER, "5"),
            ProviderError::RateLimited {
                retry_after: Some(Duration::from_secs(5)),
            },
        ),
        (
            Reply::new(StatusCode::FORBIDDEN, "private response").header(
                header::HeaderName::from_static("x-ratelimit-remaining"),
                "0",
            ),
            ProviderError::RateLimited { retry_after: None },
        ),
        (
            Reply::new(StatusCode::SERVICE_UNAVAILABLE, "private response"),
            ProviderError::HttpStatus(503),
        ),
        (
            Reply::new(StatusCode::FOUND, "private response")
                .header(header::LOCATION, "/unexpected"),
            ProviderError::HttpStatus(302),
        ),
        (
            Reply::new(StatusCode::OK, "{invalid"),
            ProviderError::InvalidResponse,
        ),
        (
            Reply::new(StatusCode::OK, without_body.to_string()),
            ProviderError::InvalidResponse,
        ),
        (
            Reply::new(StatusCode::OK, without_repository.to_string()),
            ProviderError::InvalidResponse,
        ),
        (
            Reply::new(StatusCode::OK, payload(43, false).to_string()),
            ProviderError::WrongResource,
        ),
        (
            Reply::new(StatusCode::OK, "x".repeat(1_048_577)),
            ProviderError::ResponseTooLarge,
        ),
    ];
    let count = cases.len();
    for (reply, expected) in cases {
        simulator.enqueue(Method::GET, SUMMARY_PATH, reply);
        let error = provider.read_summary(&fixture.request).await.unwrap_err();
        assert_eq!(error, expected);
        assert!(!error.to_string().contains("private response"));
    }
    let observations = simulator.take_observations();
    assert_eq!(observations.len(), count);
    assert!(observations.iter().all(|request| request.matched));
}

#[tokio::test]
async fn delay_allows_other_http_work_and_cancelled_selection_cannot_publish() {
    let fixture = FeasibilityFixture::new(42);
    let simulator = Simulator::start().await;
    let release = Arc::new(Notify::new());
    simulator.enqueue(
        Method::GET,
        SUMMARY_PATH,
        Reply::new(StatusCode::OK, payload(42, true).to_string()).delayed_until(release.clone()),
    );
    simulator.enqueue(
        Method::GET,
        SUMMARY_PATH,
        Reply::new(StatusCode::OK, payload(42, true).to_string()),
    );
    let provider = Arc::new(RestProvider::loopback(simulator.address).unwrap());
    let scheduler = Scheduler::new(Handle::current(), provider, NonZeroUsize::new(2).unwrap());

    let cancellation = Cancellation::new();
    let earlier = scheduler.spawn(fixture.request.clone(), None, cancellation.clone());
    simulator.wait_for_requests(1).await;
    let later = scheduler.spawn(fixture.request, None, Cancellation::new());
    let summary = later.await.unwrap().result.unwrap();
    assert!(!earlier.is_finished());
    assert_eq!(
        summary.revision(),
        &Revision::UpdatedAt("2026-11-17T12:00:00Z".into())
    );

    cancellation.cancel();
    assert_eq!(earlier.await.unwrap().result, Err(ProviderError::Cancelled));
    release.notify_one();
    assert_eq!(simulator.take_observations().len(), 2);
}

#[test]
fn test_endpoint_rejects_non_loopback_addresses() {
    for address in ["0.0.0.0:1234", "192.0.2.1:1234", "[::]:1234", "127.0.0.1:0"] {
        assert!(matches!(
            RestProvider::loopback(address.parse().unwrap()),
            Err(ProviderError::InvalidEndpoint)
        ));
    }
}
