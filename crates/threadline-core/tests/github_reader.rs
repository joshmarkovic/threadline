mod support;

use axum::http::{Method, StatusCode, header};
use serde_json::{Value, json};
use std::{num::NonZeroU64, sync::Arc};
use support::{Reply, Simulator};
use threadline_core::{
    domain::{ItemKind, ItemNumber, RepositoryApiId, RepositoryName},
    github::{
        AccessKind, CancellationToken, GitHubClient, GitHubSession, OwnerScope, SecretToken,
        reader::{RemoteSearchQuery, Reported, SearchQueryError},
    },
};
use tokio::sync::Notify;

const BASE: &str = "/repos/fixture-org/private";
const LIST: &str =
    "/repos/fixture-org/private/issues?state=all&sort=created&direction=desc&per_page=20";
const OLD_DATE: &str = "2013-09-06T01:01:04Z";

fn reply(status: StatusCode, value: Value) -> Reply {
    let mut response = Reply::new(status, value.to_string());
    response.authenticated = true;
    response
}

fn ok(value: Value) -> Reply {
    reply(StatusCode::OK, value)
}

fn repo_id() -> RepositoryApiId {
    71.try_into().unwrap()
}

fn number(n: u64) -> ItemNumber {
    n.try_into().unwrap()
}

fn route() -> RepositoryName {
    RepositoryName::new("fixture-org", "private").unwrap()
}

async fn session(sim: &Simulator, visible_repository: u64) -> GitHubSession {
    session_with_visibility(sim, visible_repository, true).await
}

async fn session_with_visibility(
    sim: &Simulator,
    visible_repository: u64,
    private: bool,
) -> GitHubSession {
    sim.enqueue(
        Method::GET,
        "/user",
        ok(json!({"id":41,"login":"fixture-user"})),
    );
    sim.enqueue(Method::GET, BASE, ok(json!({
        "id":visible_repository,"node_id":"R_fixture","name":"private","owner":{"login":"fixture-org"},
            "private":private
    })));
    GitHubClient::loopback(sim.address)
        .unwrap()
        .authenticate(
            SecretToken::fixture(),
            OwnerScope::new("fixture-org").unwrap(),
            NonZeroU64::new(41),
            &CancellationToken::new(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn a_selected_public_repository_uses_the_same_id_verified_reader() {
    let sim = Simulator::start().await;
    let session = session_with_visibility(&sim, 71, false).await;
    let cancel = CancellationToken::new();
    let reader = session.reader(&route(), repo_id(), &cancel).await.unwrap();
    sim.enqueue(
        Method::GET,
        &format!("{BASE}/issues/41"),
        ok(issue(41, false)),
    );
    assert_eq!(
        reader.issue(number(41), &cancel).await.unwrap().number,
        number(41)
    );
    check_requests(&sim, 3);
}

fn issue(number: u64, pr: bool) -> Value {
    let mut issue = json!({
        "id":1000+number, "node_id":format!("I_{number}"), "number":number, "title":"Archived fixture",
        "state":"closed", "updated_at":OLD_DATE,
        "user":{"login":"old-author"}, "labels":[{"name":"priority"}], "assignees":[],
        "repository_url":"https://api.github.com/repos/fixture-org/private"
    });
    if pr {
        issue["pull_request"] = json!({"url":format!("https://api.github.com/repos/fixture-org/private/pulls/{number}")});
        issue["body"] = Value::Null;
    }
    issue
}

fn check_requests(sim: &Simulator, count: usize) {
    let actual = sim.take_observations();
    assert_eq!(actual.len(), count);
    assert!(actual.iter().all(|entry| entry.matched
        && entry.authorization_sent
        && entry.expected_media_type
        && entry.versioned));
}

#[tokio::test]
async fn older_closed_issues_and_prs_use_all_state_pagination_with_no_date_filter() {
    let sim = Simulator::start().await;
    let session = session(&sim, 71).await;
    let cancel = CancellationToken::new();
    let reader = session.reader(&route(), repo_id(), &cancel).await.unwrap();
    let next = format!(
        "<http://{}/repositories/71/issues?state=all&sort=created&direction=desc&per_page=20&page=2>; rel=\"next\"",
        sim.address
    );
    sim.enqueue(
        Method::GET,
        &format!("{LIST}&page=1"),
        ok(json!([issue(41, false)])).header(header::LINK, next),
    );
    sim.enqueue(
        Method::GET,
        &format!("{LIST}&page=2"),
        ok(json!([issue(42, true)])),
    );
    let first = reader.list(1, &cancel).await.unwrap();
    assert_eq!(first.next_page, Some(2));
    assert_eq!(first.records[0].number, number(41));
    assert!(first.records[0].closed);
    assert!(matches!(first.records[0].body, Reported::Absent));
    assert!(first.records[0].updated_at_ms < 1_500_000_000_000);
    assert_eq!(first.records[0].author.as_deref(), Some("old-author"));
    let old_pr = reader
        .list(first.next_page.unwrap(), &cancel)
        .await
        .unwrap();
    assert_eq!(old_pr.next_page, None);
    assert_eq!(old_pr.records[0].kind, ItemKind::PullRequest);
    assert_eq!(old_pr.records[0].issue_id.get(), 1042);
    assert!(matches!(old_pr.records[0].body, Reported::Null));
    check_requests(&sim, 4);
}

#[tokio::test]
async fn pr_metadata_conversation_and_timeline_have_independent_endpoints() {
    let sim = Simulator::start().await;
    let session = session(&sim, 71).await;
    let cancel = CancellationToken::new();
    let reader = session.reader(&route(), repo_id(), &cancel).await.unwrap();
    sim.enqueue(
        Method::GET,
        &format!("{BASE}/issues/42"),
        ok(issue(42, true)),
    );
    sim.enqueue(Method::GET, &format!("{BASE}/pulls/42"), ok(json!({
        "id":420, "node_id":"P_42", "number":42, "title":"Archived fixture", "state":"closed",
        "updated_at": OLD_DATE, "merged":true, "base":{"sha":"a".repeat(40)}, "head":{"sha":"b".repeat(40)},
        "merge_commit_sha":"c".repeat(40)
    })));
    let summary = reader.issue(number(42), &cancel).await.unwrap();
    let detail = reader.pull_request(number(42), &cancel).await.unwrap();
    assert_eq!(summary.issue_id.get(), 1042);
    assert_eq!(detail.pull_request_id.get(), 420);
    assert!(detail.merged);

    let comments = format!("{BASE}/issues/42/comments?per_page=20");
    sim.enqueue(Method::GET, &format!("{comments}&page=1"), ok(json!([{
        "id":291,"node_id":"C_291","user":{"login":"fixture-user"},"body":"**Markdown**",
        "created_at":OLD_DATE,"updated_at":OLD_DATE,"issue_url":"https://api.github.com/repos/fixture-org/private/issues/42"
    }])).header(header::LINK, "<?per_page=20&page=2>; rel=\"next\""));
    sim.enqueue(
        Method::GET,
        &format!("{comments}&page=2"),
        reply(StatusCode::BAD_GATEWAY, json!({"message":"retry fixture"})),
    );
    let first = reader.comments(number(42), 1, &cancel).await.unwrap();
    assert_eq!(first.next_page, Some(2));
    assert_eq!(first.records[0].id.get(), 291);
    assert_eq!(first.records[0].body, "**Markdown**");
    assert_eq!(
        reader
            .comments(number(42), 2, &cancel)
            .await
            .err()
            .unwrap()
            .kind,
        AccessKind::ServiceFailure
    );

    sim.enqueue(
        Method::GET,
        &format!("{BASE}/issues/42/timeline?per_page=20&page=1"),
        ok(json!([
            {"event":"locked","id":100,"actor":{"login":"fixture-user"},"created_at":OLD_DATE},
            {"event":"commented","id":291,"body":"duplicate ordinary conversation"},
            {"event":"cross-referenced","node_id":"T_7"}
        ])),
    );
    let timeline = reader.timeline(number(42), 1, &cancel).await.unwrap();
    assert_eq!(timeline.records.len(), 2);
    assert_eq!(timeline.records[0].kind, "locked");
    assert_eq!(timeline.records[1].created_at_ms, None);
    assert!(timeline.records[1].stable_identity);
    check_requests(&sim, 7);
}

#[tokio::test]
async fn discovery_reports_remote_cap_and_rejects_scope_modification() {
    let sim = Simulator::start().await;
    let session = session(&sim, 71).await;
    let cancel = CancellationToken::new();
    let reader = session.reader(&route(), repo_id(), &cancel).await.unwrap();
    assert!(matches!(
        RemoteSearchQuery::new("needle repo:outside/project"),
        Err(SearchQueryError::Unsupported)
    ));
    assert!(matches!(
        RemoteSearchQuery::new("escape\" org outside"),
        Err(SearchQueryError::Unsupported)
    ));
    let query = RemoteSearchQuery::new("needle in data").unwrap();
    let path = "/search/issues?q=repo%3Afixture-org%2Fprivate+%22needle+in+data%22&per_page=20";
    let next = format!("<http://{}{}&page=51>; rel=\"next\"", sim.address, path);
    sim.enqueue(
        Method::GET,
        &format!("{path}&page=50"),
        ok(json!({
            "total_count":1437, "incomplete_results":true, "items":[issue(43,false)]
        }))
        .header(header::LINK, next),
    );
    let result = reader.search(&query, 50, &cancel).await.unwrap();
    assert_eq!(result.reported_total, 1437);
    assert_eq!(result.hits[0].number, number(43));
    assert!(result.capped && result.incomplete);
    assert_eq!(result.next_page, None);
    assert_eq!(
        reader.search(&query, 51, &cancel).await.err().unwrap().kind,
        AccessKind::InvalidPagination
    );

    let mut foreign = issue(44, false);
    foreign["repository_url"] = Value::from("https://api.github.com/repos/outside/private");
    sim.enqueue(
        Method::GET,
        &format!("{path}&page=1"),
        ok(json!({
            "total_count":1, "incomplete_results":false, "items":[foreign]
        })),
    );
    assert_eq!(
        reader.search(&query, 1, &cancel).await.err().unwrap().kind,
        AccessKind::IdentityMismatch
    );
    check_requests(&sim, 4);
}

#[tokio::test]
async fn repository_mismatch_and_unrelated_link_are_fail_closed() {
    let sim = Simulator::start().await;
    let session = session(&sim, 999).await;
    let cancel = CancellationToken::new();
    assert_eq!(
        session
            .reader(&route(), repo_id(), &cancel)
            .await
            .err()
            .unwrap()
            .kind,
        AccessKind::IdentityMismatch
    );
    check_requests(&sim, 2);

    let session = self::session(&sim, 71).await;
    let reader = session.reader(&route(), repo_id(), &cancel).await.unwrap();
    let wrong = format!(
        "<http://{}/repositories/72/issues?per_page=20&page=2>; rel=\"next\"",
        sim.address
    );
    sim.enqueue(
        Method::GET,
        &format!("{LIST}&page=1"),
        ok(json!([])).header(header::LINK, wrong),
    );
    assert_eq!(
        reader.list(1, &cancel).await.err().unwrap().kind,
        AccessKind::InvalidPagination
    );
    check_requests(&sim, 3);
}

#[tokio::test]
async fn cancellation_ends_one_resource_read_without_inventing_empty_coverage() {
    let sim = Simulator::start().await;
    let session = session(&sim, 71).await;
    let skipped = CancellationToken::new();
    let reader = session.reader(&route(), repo_id(), &skipped).await.unwrap();
    sim.enqueue(
        Method::GET,
        &format!("{BASE}/issues/42/comments?per_page=20&page=1"),
        ok(json!([])).delayed_until(Arc::new(Notify::new())),
    );
    let pending = reader.comments(number(42), 1, &skipped);
    tokio::pin!(pending);
    tokio::select! {
        _ = &mut pending => panic!("comment response was deliberately delayed"),
        _ = sim.wait_for_requests(3) => skipped.cancel(),
    }
    assert_eq!(pending.await.err().unwrap().kind, AccessKind::Cancelled);
    check_requests(&sim, 3);
}
