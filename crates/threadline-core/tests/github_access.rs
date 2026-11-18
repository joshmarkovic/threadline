mod support;

use axum::http::{HeaderName, Method, StatusCode, header};
use serde_json::{Value, json};
use std::{num::NonZeroU64, sync::Arc, time::Duration};
use support::{Reply, Simulator};
use threadline_core::domain::{RepositoryApiId, RepositoryName};
use threadline_core::github::*;
use tokio::sync::Notify;

const REPO_PATH: &str = "/repos/fixture-org/private";
const PAGE_ONE: &str = "/user/repos?per_page=100&page=1";
const PAGE_TWO: &str = "/user/repos?per_page=100&page=2";
fn ok(body: Value) -> Reply {
    reply(StatusCode::OK, body.to_string())
}
fn reply(status: StatusCode, body: impl Into<String>) -> Reply {
    let mut reply = Reply::new(status, body);
    reply.authenticated = true;
    reply
}
fn route() -> RepositoryName {
    RepositoryName::new("fixture-org", "private").unwrap()
}
fn scope(name: &str) -> OwnerScope {
    OwnerScope::new(name).unwrap()
}
fn repository(owner: &str, name: &str, id: u64, private: bool) -> Value {
    json!({"id":id,"node_id":format!("R_{id}"),"name":name,"owner":{"login":owner},"private":private,
        "permissions":{"pull":true,"push":false,"admin":false}})
}
async fn session(sim: &Simulator, owner: &str, id: u64) -> GitHubSession {
    sim.enqueue(
        Method::GET,
        "/user",
        ok(json!({"id":id,"login":"fixture-user"})),
    );
    GitHubClient::loopback(sim.address)
        .unwrap()
        .authenticate(
            SecretToken::fixture(),
            scope(owner),
            NonZeroU64::new(id),
            &CancellationToken::new(),
        )
        .await
        .unwrap()
}
fn checked(sim: &Simulator, requests: usize, graph: usize, case: &str, outcomes: &[&str]) {
    let observations = sim.take_observations();
    assert_eq!(observations.len(), requests);
    assert_eq!(observations.iter().filter(|o| !o.versioned).count(), graph);
    assert!(
        observations
            .iter()
            .all(|o| o.matched && o.expected_media_type && o.authorization_sent)
    );
    if let Some(dir) = std::env::var_os("THREADLINE_HTTP_TRACE_DIR") {
        let dir = std::path::Path::new(&dir);
        std::fs::create_dir_all(dir).unwrap();
        let evidence = json!({"schema_version":1,"scenario":if case.starts_with("it_005"){"IT-005"}else{"IT-004"},
            "case":case,"requests":observations,"verified":outcomes});
        let file = std::fs::File::create(dir.join(format!("{case}.json"))).unwrap();
        serde_json::to_writer_pretty(file, &evidence).unwrap();
    }
}
#[tokio::test]
async fn it_004_verifies_user_owner_private_and_immutable_id_then_revalidates() {
    let sim = Simulator::start().await;
    let session = session(&sim, "fixture-org", 41).await;
    let cancel = CancellationToken::new();
    assert_eq!(session.user().id.get(), 41);
    let id = RepositoryApiId::try_from(71).unwrap();
    let repo = repository("FIXTURE-ORG", "private", 71, true);
    sim.enqueue(Method::GET, REPO_PATH, ok(repo.clone()));
    let access = session
        .verify_private_repository(&route(), Some(id), &cancel)
        .await
        .unwrap();
    assert_eq!(access.id, id);
    assert!(access.private);
    assert_eq!(
        access.credential_capabilities().repository_metadata_read,
        Capability::Verified
    );
    assert_eq!(
        access.credential_capabilities().issue_write,
        Capability::Unverified
    );
    assert!(!access.permissions.unwrap().push);
    sim.enqueue(
        Method::GET,
        REPO_PATH,
        ok(repo).header(header::ETAG, "W/\"initial\""),
    );
    let RepositoryRead::Fresh {
        validator: Some(tag),
        ..
    } = session
        .repository_if_changed(&route(), Some(id), None, &cancel)
        .await
        .unwrap()
    else {
        panic!("missing verified entity tag")
    };
    let mut unchanged = reply(StatusCode::NOT_MODIFIED, "");
    unchanged.expected_etag = Some("W/\"initial\"");
    sim.enqueue(Method::GET, REPO_PATH, unchanged);
    assert!(matches!(
        session
            .repository_if_changed(&route(), Some(id), Some(&tag), &cancel)
            .await
            .unwrap(),
        RepositoryRead::NotModified
    ));
    let other = RepositoryName::new("fixture-org", "elsewhere").unwrap();
    assert_eq!(
        session
            .repository_if_changed(&other, None, Some(&tag), &cancel)
            .await
            .unwrap_err()
            .reachability,
        Reachability::NotAttempted
    );
    let foreign = RepositoryName::new("outside", "private").unwrap();
    assert_eq!(
        session
            .repository(&foreign, None, &cancel)
            .await
            .unwrap_err()
            .kind,
        AccessKind::WrongScope
    );
    for (body, kind) in [
        (
            repository("fixture-org", "private", 71, false),
            AccessKind::PrivateRepositoryRequired,
        ),
        (
            repository("fixture-org", "private", 72, true),
            AccessKind::IdentityMismatch,
        ),
        (
            repository("outside", "private", 71, true),
            AccessKind::IdentityMismatch,
        ),
        (
            repository("fixture-org", "another", 71, true),
            AccessKind::IdentityMismatch,
        ),
    ] {
        sim.enqueue(Method::GET, REPO_PATH, ok(body));
        assert_eq!(
            session
                .verify_private_repository(&route(), Some(id), &cancel)
                .await
                .unwrap_err()
                .kind,
            kind
        );
    }
    checked(
        &sim,
        8,
        0,
        "it_004_identity_and_conditional",
        &[
            "owner_and_id_bound",
            "private_required",
            "conditional_304",
            "no_write_grant_inferred",
        ],
    );
}
#[tokio::test]
async fn it_004_walks_link_pages_and_retains_partial_sso_across_an_empty_terminal_page() {
    let sim = Simulator::start().await;
    let first = session(&sim, "fixture-org", 41).await;
    let cancel = CancellationToken::new();
    sim.enqueue(
        Method::GET,
        PAGE_ONE,
        ok(json!([
            repository("fixture-org", "fork", 1, true),
            repository("elsewhere", "private", 2, true)
        ]))
        .header(
            header::LINK,
            "</user/repos?page=2&per_page=100>; rel=\"next\", </user/repos?page=2>; rel=\"last\"",
        )
        .header(
            HeaderName::from_static("x-github-sso"),
            "partial-results; organizations=17, 24",
        ),
    );
    let page = first.discover(None, &cancel).await.unwrap();
    assert!(!page.complete());
    assert_eq!(page.repositories.len(), 1);
    assert_eq!(page.repositories[0].route.name(), "fork");
    let cursor = page.next.as_ref().unwrap();
    let second = session(&sim, "personal-user", 41).await;
    assert_eq!(
        second
            .discover(Some(cursor), &cancel)
            .await
            .unwrap_err()
            .kind,
        AccessKind::WrongScope
    );
    let third = session(&sim, "fixture-org", 42).await;
    assert_eq!(
        third
            .discover(Some(cursor), &cancel)
            .await
            .unwrap_err()
            .kind,
        AccessKind::WrongScope
    );
    sim.enqueue(Method::GET, PAGE_TWO, ok(json!([])));
    let end = first.discover(Some(cursor), &cancel).await.unwrap();
    assert!(end.next.is_none());
    assert!(!end.complete());
    assert_eq!(
        end.sso_restricted_organizations
            .unwrap()
            .into_iter()
            .map(NonZeroU64::get)
            .collect::<Vec<_>>(),
        [17, 24]
    );
    sim.enqueue(
        Method::GET,
        PAGE_ONE,
        ok(json!([repository("personal-user", "a", 3, false)])),
    );
    assert!(second.discover(None, &cancel).await.unwrap().complete());
    for bad in [
        "<https://outside.invalid/user/repos?page=2>; rel=\"next\"",
        "</repos/fixture-org/private?page=2>; rel=\"next\"",
        "</user/repos?page=1>; rel=\"next\"",
        "</user/repos?page=2&access_token=example>; rel=\"next\"",
        "malformed",
    ] {
        sim.enqueue(
            Method::GET,
            PAGE_ONE,
            ok(json!([])).header(header::LINK, bad),
        );
        assert_eq!(
            first.discover(None, &cancel).await.unwrap_err().kind,
            AccessKind::InvalidPagination
        );
    }
    checked(
        &sim,
        11,
        0,
        "it_004_repository_pages",
        &[
            "partial_sso_retained",
            "foreign_owners_filtered",
            "invalid_links_rejected",
            "personal_owner_complete",
        ],
    );
}
#[tokio::test]
async fn it_004_graphql_sends_variables_and_keeps_partial_data_with_typed_errors() {
    let sim = Simulator::start().await;
    let session = session(&sim, "fixture-org", 41).await;
    let cancel = CancellationToken::new();
    let node = json!({"id":"R_71","name":"private","owner":{"login":"fixture-org"},"isPrivate":true,"viewerPermission":"READ"});
    let results = [
        (json!({"data":{"repository":node}}), vec![], true),
        (
            json!({"data":{"repository":node},"errors":[{"type":"FORBIDDEN","message":"withheld"}]}),
            vec![GraphIssue::Forbidden],
            true,
        ),
        (
            json!({"data":{"repository":null},"errors":[{"extensions":{"code":"RATE_LIMITED"}},{"type":"NOT_FOUND"},{"type":"SAML_SSO_REQUIRED"}]}),
            vec![
                GraphIssue::RateLimited,
                GraphIssue::NotFound,
                GraphIssue::SsoRequired,
            ],
            false,
        ),
    ];
    for (body, issues, has_data) in results {
        let mut response = ok(body);
        response.graph_repository = Some(("fixture-org", "private"));
        sim.enqueue(Method::POST, "/graphql", response);
        let result = session.graphql_repository(&route(), &cancel).await.unwrap();
        assert_eq!(result.complete(), issues.is_empty() && has_data);
        assert_eq!(result.errors, issues);
        assert_eq!(result.repository.is_some(), has_data);
        if let Some(repo) = result.repository {
            assert_eq!(repo.permission, Some(ViewerPermission::Read));
            assert_eq!(repo.node_id, "R_71");
        }
    }
    for (payload, kind) in [
        (json!({"data":{}}), AccessKind::MissingOrInaccessible),
        (
            json!({"data":{"repository":{"id":"R_71"}}}),
            AccessKind::MalformedResponse,
        ),
    ] {
        let mut response = ok(payload);
        response.graph_repository = Some(("fixture-org", "private"));
        sim.enqueue(Method::POST, "/graphql", response);
        assert_eq!(
            session
                .graphql_repository(&route(), &cancel)
                .await
                .unwrap_err()
                .kind,
            kind
        );
    }
    checked(
        &sim,
        6,
        5,
        "it_004_graphql",
        &[
            "variables_match",
            "partial_data_preserved",
            "errors_classified",
            "malformed_rejected",
        ],
    );
}
#[tokio::test]
async fn it_004_http_statuses_preserve_reachability_and_only_offer_scoped_sso_actions() {
    let sim = Simulator::start().await;
    let session = session(&sim, "fixture-org", 41).await;
    let cancel = CancellationToken::new();
    let sso = HeaderName::from_static("x-github-sso");
    let cases=[
        (reply(StatusCode::UNAUTHORIZED,"sensitive echoed content"),AccessKind::Unauthorized),
        (reply(StatusCode::FORBIDDEN,r#"{"message":"Token pending organization approval"}"#),AccessKind::ApprovalPending),
        (reply(StatusCode::FORBIDDEN,"not json"),AccessKind::PermissionDenied),
        (reply(StatusCode::NOT_FOUND,"{}"),AccessKind::MissingOrInaccessible),
        (reply(StatusCode::SERVICE_UNAVAILABLE,""),AccessKind::ServiceFailure),
        (reply(StatusCode::FOUND,"").header(header::LOCATION,"/user"),AccessKind::RedirectRejected),
        (reply(StatusCode::OK,"not json"),AccessKind::MalformedResponse),
        (reply(StatusCode::NOT_MODIFIED,""),AccessKind::UnexpectedStatus),
        (reply(StatusCode::OK,"x".repeat(2_097_153)),AccessKind::ResponseTooLarge),
        (reply(StatusCode::FORBIDDEN,"").header(sso.clone(),"required; url=https://github.com/orgs/fixture-org/sso?authorization_request=test123"),AccessKind::SsoRequired),
    ];
    for (response, kind) in cases {
        sim.enqueue(Method::GET, REPO_PATH, response);
        let error = session
            .repository(&route(), None, &cancel)
            .await
            .unwrap_err();
        assert_eq!(error.kind, kind);
        assert_eq!(error.reachability, Reachability::ReceivedResponse);
        assert!(error.http_status.is_some());
        let diagnostics = format!("{error} {error:?}");
        assert!(!diagnostics.contains("sensitive") && !diagnostics.contains("test123"));
        if kind == AccessKind::SsoRequired {
            assert_eq!(
                error.authorization_url().unwrap().as_str(),
                "https://github.com/orgs/fixture-org/sso?authorization_request=test123"
            );
        }
    }
    for advisory in [
        "required; url=https://github.com/orgs/elsewhere/sso",
        "required; url=https://outside.invalid/orgs/fixture-org/sso",
        "required; url=https://github.com/orgs/fixture-org/sso?authorization_request=threadline-test-fixture",
        "required; url=https://github.com/orgs/fixture-org/sso?return_to=outside",
        "required; url=http://github.com/orgs/fixture-org/sso",
    ] {
        sim.enqueue(
            Method::GET,
            REPO_PATH,
            reply(StatusCode::FORBIDDEN, "").header(sso.clone(), advisory),
        );
        let error = session
            .repository(&route(), None, &cancel)
            .await
            .unwrap_err();
        assert_eq!(error.kind, AccessKind::SsoRequired);
        assert!(error.authorization_url().is_none());
    }
    sim.enqueue(
        Method::GET,
        REPO_PATH,
        reply(StatusCode::TOO_MANY_REQUESTS, "")
            .header(header::RETRY_AFTER, "7")
            .header(HeaderName::from_static("x-ratelimit-reset"), "1800000000"),
    );
    let rate = session
        .repository(&route(), None, &cancel)
        .await
        .unwrap_err()
        .rate
        .unwrap();
    assert_eq!(rate.retry_after_seconds, Some(7));
    assert_eq!(rate.reset_at_unix_seconds, Some(1800000000));
    checked(
        &sim,
        17,
        0,
        "it_004_http_faults",
        &[
            "auth_sso_and_approval_distinct",
            "ratelimit_timing",
            "private_404_ambiguous",
            "redaction",
        ],
    );
}
#[tokio::test]
async fn it_004_cancels_pending_reads_and_distinguishes_no_response_from_identity_mismatch() {
    let sim = Simulator::start().await;
    let client = GitHubClient::loopback(sim.address).unwrap();
    let cancel = CancellationToken::new();
    let denied = client
        .authenticate(
            SecretToken::new("real-looking-synthetic").unwrap(),
            scope("fixture-org"),
            None,
            &cancel,
        )
        .await
        .unwrap_err();
    assert_eq!(
        (denied.kind, denied.reachability),
        (AccessKind::InvalidCredential, Reachability::NotAttempted)
    );
    sim.enqueue(
        Method::GET,
        "/user",
        ok(json!({"id":41,"login":"fixture-user"})),
    );
    let wrong = client
        .authenticate(
            SecretToken::fixture(),
            scope("fixture-org"),
            NonZeroU64::new(73),
            &cancel,
        )
        .await
        .unwrap_err();
    assert_eq!(
        (wrong.kind, wrong.http_status),
        (AccessKind::IdentityMismatch, Some(200))
    );
    let session = session(&sim, "fixture-org", 41).await;
    let gate = Arc::new(Notify::new());
    sim.enqueue(
        Method::GET,
        PAGE_ONE,
        ok(json!([])).delayed_until(gate.clone()),
    );
    let reading = session.discover(None, &cancel);
    let interrupt = async {
        sim.wait_for_requests(3).await;
        cancel.cancel()
    };
    let (read, ()) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(reading, interrupt)
    })
    .await
    .unwrap();
    let error = read.unwrap_err();
    assert_eq!(
        (error.kind, error.reachability),
        (AccessKind::Cancelled, Reachability::NoResponse)
    );
    let precancelled = session.discover(None, &cancel).await.unwrap_err();
    assert_eq!(precancelled.reachability, Reachability::NotAttempted);
    gate.notify_waiters();

    // A bound socket that has not started listening cannot receive a request.
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let offline = GitHubClient::loopback(socket.local_addr().unwrap()).unwrap();
    let error = offline
        .authenticate(
            SecretToken::fixture(),
            scope("fixture-org"),
            None,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        (error.kind, error.reachability),
        (AccessKind::Transport, Reachability::NoResponse)
    );
    checked(
        &sim,
        3,
        0,
        "it_004_cancellation",
        &[
            "pending_read_cancelled",
            "pre_cancelled_request_not_sent",
            "no_response_distinct_from_authenticated_mismatch",
            "loopback_rejects_production_secrets",
        ],
    );
}

#[tokio::test]
async fn it_005_cross_origin_redirects_and_owner_changes_do_not_forward_authorization() {
    let backend = Simulator::start().await;
    let foreign = Simulator::start().await;
    let session = session(&backend, "fixture-org", 41).await;
    let cancel = CancellationToken::new();
    let redirect = format!("http://{}/forbidden", foreign.address);
    for status in [
        StatusCode::MOVED_PERMANENTLY,
        StatusCode::FOUND,
        StatusCode::TEMPORARY_REDIRECT,
        StatusCode::PERMANENT_REDIRECT,
    ] {
        backend.enqueue(
            Method::GET,
            REPO_PATH,
            reply(status, "").header(header::LOCATION, &redirect),
        );
        let error = session
            .repository(&route(), None, &cancel)
            .await
            .unwrap_err();
        assert_eq!(error.kind, AccessKind::RedirectRejected);
        assert_eq!(error.http_status, Some(status.as_u16()));
    }
    backend.enqueue(
        Method::GET,
        PAGE_ONE,
        ok(json!([])).header(header::LINK, format!("<{redirect}?page=2>; rel=\"next\"")),
    );
    assert_eq!(
        session.discover(None, &cancel).await.unwrap_err().kind,
        AccessKind::InvalidPagination
    );
    let other = RepositoryName::new("outside", "private").unwrap();
    assert_eq!(
        session
            .repository(&other, None, &cancel)
            .await
            .unwrap_err()
            .kind,
        AccessKind::WrongScope
    );
    assert_eq!(
        session
            .graphql_repository(&other, &cancel)
            .await
            .unwrap_err()
            .kind,
        AccessKind::WrongScope
    );
    assert!(foreign.take_observations().is_empty());
    checked(
        &backend,
        6,
        0,
        "it_005_origin_and_owner",
        &[
            "four_cross_origin_redirects_rejected",
            "foreign_pagination_rejected",
            "foreign_owner_rest_and_graphql_blocked_locally",
            "foreign_listener_received_zero_requests",
        ],
    );
}
