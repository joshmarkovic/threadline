mod support;

use axum::http::{Method, StatusCode, header};
use serde_json::{Value, json};
use std::{num::NonZeroU64, sync::Arc};
use support::{Reply, Simulator};
use threadline_core::{
    domain::{ItemKind, ItemNumber, RepositoryApiId, RepositoryName},
    github::{
        AccessKind, CancellationToken, GitHubClient, GitHubSession, OwnerScope, SecretToken,
        reader::{RemoteSearchQuery, Reported, SearchQueryError, refresh::ReadValidators},
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

#[tokio::test]
async fn issue_timestamps_with_equivalent_offsets_use_the_same_utc_millis() {
    let sim = Simulator::start().await;
    let session = session(&sim, 71).await;
    let cancel = CancellationToken::new();
    let reader = session.reader(&route(), repo_id(), &cancel).await.unwrap();
    for (created_at, updated_at) in [
        (
            "2026-11-01T01:30:00.123-04:00",
            "2026-11-01T01:30:00.123-05:00",
        ),
        ("2026-11-01T05:30:00.123Z", "2026-11-01T06:30:00.123Z"),
    ] {
        let mut response = issue(41, false);
        response["created_at"] = json!(created_at);
        response["updated_at"] = json!(updated_at);
        sim.enqueue(Method::GET, &format!("{BASE}/issues/41"), ok(response));
        let row = reader.issue(number(41), &cancel).await.unwrap();
        assert_eq!(row.created_at_ms, Some(1_793_511_000_123));
        assert_eq!(row.updated_at_ms, 1_793_514_600_123);
    }
    check_requests(&sim, 4);
}

#[tokio::test]
async fn private_dogfood_reader_reuses_one_verification_and_stops_after_the_first_get_page() {
    let sim = Simulator::start().await;
    let session = session(&sim, 71).await;
    let cancel = CancellationToken::new();
    let (_, reader) = session
        .private_reader(&route(), repo_id(), &cancel)
        .await
        .unwrap();
    let only_page = format!("{LIST}&page=1");
    let next = format!("<{LIST}&page=2>; rel=\"next\"");
    sim.enqueue(
        Method::GET,
        &only_page,
        ok(json!([issue(41, false), issue(42, true)])).header(header::LINK, next),
    );
    let page = reader.list(1, &cancel).await.unwrap();
    assert_eq!(page.records.len(), 2);
    assert_eq!(page.next_page, Some(2));
    check_requests(&sim, 3);
}

#[tokio::test]
async fn sync_feeds_keep_the_same_bounded_window_and_respect_conditional_headers() {
    const SINCE_MS: i64 = 1_378_429_264_333;
    const ISSUES: &str = "/repositories/71/issues?state=all&sort=updated&direction=asc&since=2013-09-06T01%3A01%3A04Z&per_page=20";
    const COMMENTS: &str = "/repositories/71/issues/comments?sort=updated&direction=asc&since=2013-09-06T01%3A01%3A04Z&per_page=20";
    const LAST_MODIFIED: &str = "Fri, 06 Sep 2013 01:01:04 GMT";
    let sim = Simulator::start().await;
    let session = Arc::new(session(&sim, 71).await);
    let cancel = CancellationToken::new();
    let (bound, _) = session
        .bind_reader(&route(), repo_id(), &cancel)
        .await
        .unwrap();
    let reader = bound.reader();
    let first_path = format!("{ISSUES}&page=1");
    let second_path = format!("{ISSUES}&page=2");
    let mut assigned = issue(12, false);
    assigned["assignees"] = json!([{"id":91,"login":"first"},{"id":41,"login":"second"}]);
    let mut partial = issue(13, false);
    partial["assignees"] = json!([{"id":91,"login":"first"},{"login":"second"}]);
    let first = ok(json!([assigned, partial]))
        .header(header::ETAG, "W/\"first-issue-page\"")
        .header(header::LAST_MODIFIED, LAST_MODIFIED)
        .header(header::LINK, format!("<{second_path}>; rel=\"next\""))
        .header(header::HeaderName::from_static("x-poll-interval"), "30");
    sim.enqueue(Method::GET, &first_path, first);
    sim.enqueue(Method::GET, &second_path, ok(json!([issue(900, true)])));
    let response = reader
        .updated_issues(SINCE_MS, 1, &ReadValidators::default(), &cancel)
        .await
        .unwrap();
    assert_eq!(
        response.rate.not_before_ms(SINCE_MS),
        Some(SINCE_MS + 30_000)
    );
    let first_page = response.value.unwrap();
    assert_eq!(first_page.records[0].number.get(), 12);
    assert!(first_page.records[0].closed);
    assert_eq!(
        first_page.records[0].assignee_ids,
        Some([41, 91].map(|id| NonZeroU64::new(id).unwrap()).into())
    );
    assert_eq!(first_page.records[1].assignee_ids, None);
    assert_eq!(first_page.next_page, Some(2));
    let next = reader
        .updated_issues(SINCE_MS, 2, &response.validators, &cancel)
        .await
        .unwrap()
        .value
        .unwrap();
    assert_eq!(next.records[0].number.get(), 900);
    assert_eq!(next.records[0].kind, ItemKind::PullRequest);
    assert_eq!(next.records[0].assignee_ids, Some(Default::default()));
    let mut unchanged = reply(StatusCode::NOT_MODIFIED, Value::Null);
    unchanged.expected_etag = Some("W/\"first-issue-page\"");
    unchanged.expected_modified = Some(LAST_MODIFIED);
    sim.enqueue(Method::GET, &first_path, unchanged);
    assert!(
        reader
            .updated_issues(SINCE_MS, 1, &response.validators, &cancel)
            .await
            .unwrap()
            .value
            .is_none()
    );

    let old_comment = json!({"id":55,"node_id":"C_55","body":"old item freshly edited", "created_at":"2012-01-01T00:00:00Z",
        "user":{"login":"reviewer","id":9001,"type":"Bot"},"performed_via_github_app":{"id":93},
        "updated_at":OLD_DATE,"issue_url":"https://api.github.com/repos/fixture-org/private/issues/12"});
    let comments = format!("{COMMENTS}&page=1");
    sim.enqueue(Method::GET, &comments, ok(json!([old_comment.clone()])));
    let result = reader
        .updated_comments(SINCE_MS, 1, &ReadValidators::default(), &cancel)
        .await
        .unwrap()
        .value
        .unwrap();
    assert_eq!(result.records[0].issue.get(), 12);
    assert!(result.records[0].comment.created_at_ms < result.records[0].comment.updated_at_ms);
    assert_eq!(result.records[0].comment.id.get(), 55);
    assert_eq!(result.records[0].comment.author_id, NonZeroU64::new(9001));
    assert_eq!(result.records[0].comment.via_app_id, NonZeroU64::new(93));
    assert!(result.records[0].comment.author_is_bot);

    let inline = "/repositories/71/pulls/comments?sort=updated&direction=asc&since=2013-09-06T01%3A01%3A04Z&per_page=20&page=1";
    let mut review_comment = old_comment.clone();
    review_comment.as_object_mut().unwrap().remove("issue_url");
    review_comment["pull_request_url"] =
        json!("https://api.github.com/repos/fixture-org/private/pulls/44");
    sim.enqueue(Method::GET, inline, ok(json!([review_comment.clone()])));
    let review = reader
        .updated_inline_comments(SINCE_MS, 1, &ReadValidators::default(), &cancel)
        .await
        .unwrap()
        .value
        .unwrap();
    assert_eq!(review.records[0].issue.get(), 44);
    assert!(review.records[0].comment.author_is_bot);
    review_comment["pull_request_url"] =
        json!("https://api.github.com/repos/other/private/pulls/44");
    sim.enqueue(Method::GET, inline, ok(json!([review_comment])));
    assert_eq!(
        reader
            .updated_inline_comments(SINCE_MS, 1, &ReadValidators::default(), &cancel)
            .await
            .err()
            .unwrap()
            .kind,
        AccessKind::IdentityMismatch
    );

    let mut transferred = old_comment;
    transferred["issue_url"] = json!("https://api.github.com/repos/another-org/private/issues/12");
    sim.enqueue(Method::GET, &comments, ok(json!([transferred])));
    let wrong_owner = reader
        .updated_comments(SINCE_MS, 1, &ReadValidators::default(), &cancel)
        .await
        .err()
        .unwrap();
    assert_eq!(wrong_owner.kind, AccessKind::IdentityMismatch);
    let mut unavailable = reply(
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"message":"fixture"}),
    );
    unavailable = unavailable.header(header::RETRY_AFTER, "Fri, 06 Sep 2013 01:03:04 GMT");
    sim.enqueue(Method::GET, &comments, unavailable);
    let outage = reader
        .updated_comments(SINCE_MS, 1, &ReadValidators::default(), &cancel)
        .await
        .err()
        .unwrap();
    assert_eq!(outage.kind, AccessKind::ServiceFailure);
    assert_eq!(
        outage.rate.unwrap().not_before_ms(SINCE_MS),
        Some(1_378_429_384_000)
    );
    check_requests(&sim, 10);
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
        issue["milestone"] = Value::Null;
        issue["reactions"] = json!({"heart": 9});
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
    let mut older = issue(41, false);
    older["created_at"] = json!("2012-01-01T00:00:00Z");
    older["milestone"] = json!({"title":"Earlier release"});
    older["reactions"] = json!({"+1":2,"-1":0,"laugh":1,"hooray":0,"confused":0,"heart":3,
        "rocket":0,"eyes":2,"total_count":8});
    sim.enqueue(
        Method::GET,
        &format!("{LIST}&page=1"),
        ok(json!([older])).header(header::LINK, next),
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
    assert_eq!(first.records[0].created_at_ms, Some(1_325_376_000_000));
    assert_eq!(first.records[0].author.as_deref(), Some("old-author"));
    assert!(
        matches!(&first.records[0].milestone, Reported::Value(title) if title == "Earlier release")
    );
    let reactions = first.records[0].reactions.as_ref().unwrap();
    assert_eq!(
        (
            reactions.positive,
            reactions.negative,
            reactions.heart,
            reactions.total
        ),
        (2, 0, 3, Some(8))
    );
    let old_pr = reader
        .list(first.next_page.unwrap(), &cancel)
        .await
        .unwrap();
    assert_eq!(old_pr.next_page, None);
    assert_eq!(old_pr.records[0].kind, ItemKind::PullRequest);
    assert_eq!(old_pr.records[0].issue_id.get(), 1042);
    assert!(matches!(old_pr.records[0].body, Reported::Null));
    assert!(matches!(old_pr.records[0].milestone, Reported::Null));
    assert!(old_pr.records[0].reactions.is_none());
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
            {"event":"cross-referenced","node_id":"T_7","source":{"type":"issue","issue":{
                "id":2050,"number":50,"url":"https://api.github.com/repos/another-org/public/issues/50",
                "repository_url":"https://api.github.com/repos/another-org/public","pull_request":{}
            }}},
            {"event":"cross-referenced","node_id":"T_8","source":{"type":"issue","issue":{
                "id":2051,"number":51,"url":"https://api.github.com/repos/fixture-org/private/issues/51",
                "repository_url":"https://api.github.com/repos/fixture-org/private"
            }}},
            {"event":"cross-referenced","node_id":"T_9","source":{"type":"issue","issue":{"number":51}}},
            {"event":"cross-referenced","node_id":"T_10","source":{"type":"issue","issue":{
                "number":51,"url":"https://api.github.com/repos/fixture-org/private/issues/50",
                "repository_url":"https://api.github.com/repos/fixture-org/private"
            }}},
            {"event":"cross-referenced","node_id":"T_11","source":{"type":"issue","issue":{
                "number":51,"url":"https://outside.invalid/repos/fixture-org/private/issues/51",
                "repository_url":"https://outside.invalid/repos/fixture-org/private"
            }}}
        ])),
    );
    let timeline = reader.timeline(number(42), 1, &cancel).await.unwrap();
    assert_eq!(timeline.records.len(), 6);
    assert_eq!(timeline.records[0].kind, "locked");
    assert_eq!(timeline.records[1].created_at_ms, None);
    assert!(timeline.records[1].stable_identity);
    let cross_repo = timeline.records[1].linked_item.as_ref().unwrap();
    assert_eq!(cross_repo.route.owner(), "another-org");
    assert_eq!(cross_repo.route.name(), "public");
    assert_eq!(cross_repo.number.get(), 50);
    assert!(cross_repo.pull_request);
    assert_eq!(cross_repo.repository_id, None);
    assert_eq!(cross_repo.issue_id.unwrap().get(), 2050);
    assert_eq!(
        timeline.records[2]
            .linked_item
            .as_ref()
            .unwrap()
            .repository_id,
        Some(repo_id())
    );
    assert!(
        timeline.records[3..]
            .iter()
            .all(|event| event.linked_item.is_none())
    );
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
