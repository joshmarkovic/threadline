mod support;

use axum::http::{Method, StatusCode, header};
use serde_json::{Value, json};
use std::{num::NonZeroU64, sync::Arc};
use support::{Reply, Simulator};
use threadline_core::{
    domain::RepositoryName,
    github::{
        AccessKind, CancellationToken, GitHubClient, GitHubSession, OwnerScope, SecretToken,
        reader::{
            pr::{PatchEvidence, PrSide, PrSource, PrSourceRange, PrThreadLocation},
            refresh::ReadValidators,
        },
    },
};
use tokio::sync::Notify;

const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const MERGE_BASE: &str = "cccccccccccccccccccccccccccccccccccccccc";
const TIME: &str = "2020-02-02T04:05:06Z";

fn reply(status: StatusCode, payload: Value) -> Reply {
    let mut reply = Reply::new(status, payload.to_string());
    reply.authenticated = true;
    reply
}

async fn authorized(sim: &Simulator, cancel: &CancellationToken) -> Arc<GitHubSession> {
    sim.enqueue(
        Method::GET,
        "/user",
        reply(StatusCode::OK, json!({"id":41,"login":"fixture-user"})),
    );
    sim.enqueue(Method::GET, "/repos/fixture-org/private", reply(StatusCode::OK, json!({
        "id":71,"node_id":"R_71","owner":{"login":"fixture-org"},"name":"private","private":true
    })));
    Arc::new(
        GitHubClient::loopback(sim.address)
            .unwrap()
            .authenticate(
                SecretToken::fixture(),
                OwnerScope::new("fixture-org").unwrap(),
                NonZeroU64::new(41),
                cancel,
            )
            .await
            .unwrap(),
    )
}

fn pull() -> Value {
    json!({"id":420,"node_id":"PR_420","number":42,"title":"Fixture change","state":"closed",
        "body":"Closes #12","merged":true,"draft":false,"created_at":TIME,"updated_at":TIME,"merged_at":TIME,
        "user":{"login":"fixture-user"},"requested_reviewers":[{"login":"fixture-reviewer"}],"changed_files":302,
        "base":{"sha":BASE,"ref":"main","repo":{"id":71,"node_id":"R_71"}},
        "head":{"sha":HEAD,"ref":"topic"}})
}

#[tokio::test]
async fn it_018_unopened_pull_index_and_recent_timeline_preserve_identity_partial_access_and_cursor()
 {
    use threadline_core::github::reader::activity::PullEvent;
    let sim = Simulator::start().await;
    let cancel = CancellationToken::new();
    let session = authorized(&sim, &cancel).await;
    let (bound, _) = session
        .bind_reader(
            &RepositoryName::new("fixture-org", "private").unwrap(),
            71.try_into().unwrap(),
            &cancel,
        )
        .await
        .unwrap();
    let reader = bound.reader();
    let path = "/repositories/71/pulls?state=open&sort=created&direction=asc&per_page=20&page=1";
    sim.enqueue(Method::GET,path,reply(StatusCode::OK,json!([{"id":420,"node_id":"PR_420","number":42,"updated_at":TIME,"base":{"repo":{"id":71}}}])));
    let open = reader
        .open_pull_index(1, &ReadValidators::default(), &cancel)
        .await
        .unwrap()
        .value
        .unwrap();
    assert_eq!(open.records[0].id.get(), 420);
    let cursor = Some("next-recent-events");
    let response = json!({"data":{"repository":{"databaseId":71,"owner":{"login":"fixture-org"},"name":"private",
    "pullRequest":{"id":"PR_420","fullDatabaseId":"420","number":42,"title":"Fixture change","closed":false,"merged":false,
        "updatedAt":TIME,"baseRefOid":BASE,"headRefOid":HEAD,"timelineItems":{"pageInfo":{"hasNextPage":true,"endCursor":cursor},"nodes":[
            {"__typename":"PullRequestReview","fullDatabaseId":501,"submittedAt":TIME,"state":"APPROVED","body":"@fixture-user",
                "author":{"__typename":"Bot","databaseId":92}},
            {"__typename":"ReadyForReviewEvent","createdAt":TIME,"actor":{"__typename":"User","databaseId":91}},
            {"__typename":"ReviewRequestedEvent","createdAt":TIME,"requestedReviewer":{"login":"fixture-user"}},
            {"__typename":"MergedEvent","createdAt":TIME,"commit":{"oid":MERGE_BASE}}
        ]}}}}});
    let graph = |payload, cursor: Option<&str>| {
        reply(StatusCode::OK,payload).graph_variables("ThreadlinePullActivity",
        json!({"owner":"fixture-org","name":"private","number":42,"since":"2017-07-14T02:40:00Z","cursor":cursor}))
    };
    sim.enqueue(Method::POST, "/graphql", graph(response.clone(), None));
    let n = 42.try_into().unwrap();
    let page = reader
        .pull_activity(n, 1_500_000_000_000, None, &cancel)
        .await
        .unwrap();
    assert_eq!(page.identity.id.get(), 420);
    assert_eq!(page.next.as_deref(), cursor);
    assert!(
        matches!(&page.events[0],PullEvent::Review{id,actor,..} if id.get()==501 && actor.known_automation)
    );
    assert!(matches!(&page.events[2],PullEvent::Requested{user,..} if user=="fixture-user"));
    assert!(!page.partial);
    assert!(
        sim.take_observations()
            .iter()
            .all(|event| event.matched && event.authorization_sent)
    );
    assert!(
        reader
            .pull_activity(n, 1_500_000_000_000, Some("not a cursor"), &cancel)
            .await
            .is_err()
    );
    assert!(sim.take_observations().is_empty());
    let mut partial = response.clone();
    partial["errors"] = json!([{"type":"FORBIDDEN"}]);
    partial["data"]["repository"]["pullRequest"]["timelineItems"]["pageInfo"] =
        json!({"hasNextPage":false,"endCursor":null});
    sim.enqueue(Method::POST, "/graphql", graph(partial, cursor));
    let limited = reader
        .pull_activity(n, 1_500_000_000_000, cursor, &cancel)
        .await
        .unwrap();
    assert!(limited.partial && limited.permission_limited);
    assert_eq!(limited.events.len(), 4);
    let mut wrong = response;
    wrong["data"]["repository"]["databaseId"] = json!(72);
    sim.enqueue(Method::POST, "/graphql", graph(wrong, cursor));
    assert_eq!(
        reader
            .pull_activity(n, 1_500_000_000_000, cursor, &cancel)
            .await
            .err()
            .unwrap()
            .kind,
        AccessKind::IdentityMismatch
    );
    assert!(
        sim.take_observations()
            .iter()
            .all(|event| event.matched && event.authorization_sent)
    );
}

#[tokio::test]
async fn pr_compare_checks_and_statuses_keep_immutable_source_and_explicit_partial_data() {
    let sim = Simulator::start().await;
    let cancel = CancellationToken::new();
    let session = authorized(&sim, &cancel).await;
    let route = RepositoryName::new("fixture-org", "private").unwrap();
    let (bound, _) = session
        .bind_reader(&route, 71.try_into().unwrap(), &cancel)
        .await
        .unwrap();
    let reader = bound.reader();
    let validators = ReadValidators::default();
    let number = 42.try_into().unwrap();
    sim.enqueue(
        Method::GET,
        "/repositories/71/pulls/42",
        reply(StatusCode::OK, pull()),
    );
    let pinned = reader
        .pr_snapshot(number, &validators, &cancel)
        .await
        .unwrap()
        .value
        .unwrap();
    assert!(pinned.details().merged);
    assert_eq!(
        pinned.details().presentation.base_ref.as_deref(),
        Some("main")
    );
    assert_eq!(
        pinned
            .details()
            .presentation
            .requested_reviewers
            .as_ref()
            .unwrap(),
        &["fixture-reviewer"]
    );

    let compare = format!("/repositories/71/compare/{BASE}...{HEAD}?per_page=20&page=1");
    let page = json!({"base_commit":{"sha":BASE},"merge_base_commit":{"sha":MERGE_BASE},"total_commits":1,
    "commits":[{"sha":HEAD,"author":{"login":"fixture-user"}, "commit":{"message":"Fixture change",
        "author":{"date":TIME},"committer":{"date":TIME}}}],"files":[
        {"filename":"new.rs","previous_filename":"old.rs","status":"renamed","additions":1,"deletions":1,
            "patch":"@@ -1 +1 @@\n-old\n+new"},
        {"filename":"short.rs","status":"modified","additions":3,"deletions":1,"patch":"@@ -1 +1 @@\n-old\n+new"},
        {"filename":"asset.dat","status":"modified","patch":"Binary files a/asset.dat and b/asset.dat differ"},
        {"filename":"missing.txt","status":"modified","additions":10,"deletions":0}
    ]});
    sim.enqueue(
        Method::GET,
        &compare,
        reply(StatusCode::OK, page.clone()).header(header::ETAG, "\"compare-page\""),
    );
    let comparison = reader
        .pr_comparison(&pinned, 1, &validators, &cancel)
        .await
        .unwrap();
    let data = comparison.value.unwrap();
    assert_eq!(data.merge_base(), MERGE_BASE);
    assert_eq!(data.commits.records[0].oid, HEAD);
    assert!(!data.commits.partial);
    let files = data.files.as_ref().unwrap();
    assert!(files.partial);
    assert!(!files.api_capped);
    assert_eq!(files.records[0].previous_path.as_deref(), Some("old.rs"));
    assert!(matches!(files.records[0].patch, PatchEvidence::Text(_)));
    assert!(matches!(
        files.records[1].patch,
        PatchEvidence::Truncated(_)
    ));
    assert!(matches!(files.records[2].patch, PatchEvidence::Binary));
    assert!(matches!(files.records[3].patch, PatchEvidence::Unavailable));
    let mut unchanged = reply(StatusCode::NOT_MODIFIED, Value::Null);
    unchanged.expected_etag = Some("\"compare-page\"");
    sim.enqueue(Method::GET, &compare, unchanged);
    assert!(
        reader
            .pr_comparison(&pinned, 1, &comparison.validators, &cancel)
            .await
            .unwrap()
            .value
            .is_none()
    );

    let mut capped = page;
    let records = capped["files"].as_array_mut().unwrap();
    for index in records.len()..threadline_core::github::reader::pr::COMPARE_FILES_CAP {
        records.push(
            json!({"filename":format!("src/fill-{index}.rs"),"status":"added",
            "additions":1, "deletions":0,"patch":"@@ -0,0 +1 @@\n+line"}),
        );
    }
    sim.enqueue(Method::GET, &compare, reply(StatusCode::OK, capped.clone()));
    let capped_comparison = reader
        .pr_comparison(&pinned, 1, &validators, &cancel)
        .await
        .unwrap()
        .value
        .unwrap();
    let files = capped_comparison.files.unwrap();
    assert!(files.partial && files.api_capped);
    assert_eq!(
        files.records.len(),
        threadline_core::github::reader::pr::COMPARE_FILES_CAP
    );

    let mut exact = pull();
    exact["changed_files"] = json!(300);
    sim.enqueue(
        Method::GET,
        "/repositories/71/pulls/42",
        reply(StatusCode::OK, exact),
    );
    let exact = reader
        .pr_snapshot(number, &validators, &cancel)
        .await
        .unwrap()
        .value
        .unwrap();
    sim.enqueue(Method::GET, &compare, reply(StatusCode::OK, capped));
    let exact_comparison = reader
        .pr_comparison(&exact, 1, &validators, &cancel)
        .await
        .unwrap()
        .value
        .unwrap();
    let files = exact_comparison.files.unwrap();
    assert!(!files.partial && !files.api_capped);

    let checks =
        format!("/repositories/71/commits/{HEAD}/check-runs?filter=all&per_page=20&page=1");
    let run = |head| {
        json!({"total_count":2,"check_runs":[{"id":8,"head_sha":head,"name":"Unit checks","status":"completed",
        "conclusion":null,"completed_at":null}]})
    };
    sim.enqueue(Method::GET, &checks, reply(StatusCode::OK, run(HEAD)));
    let results = reader
        .pr_checks(&pinned, 1, &validators, &cancel)
        .await
        .unwrap()
        .value
        .unwrap();
    assert!(results.partial);
    assert!(results.records[0].conclusion.is_none());
    assert_eq!(results.records[0].head_oid, HEAD);
    sim.enqueue(Method::GET, &checks, reply(StatusCode::OK, run(BASE)));
    assert_eq!(
        reader
            .pr_checks(&pinned, 1, &validators, &cancel)
            .await
            .err()
            .unwrap()
            .kind,
        AccessKind::IdentityMismatch
    );

    let statuses = format!("/repositories/71/commits/{HEAD}/statuses?per_page=20&page=1");
    sim.enqueue(
        Method::GET,
        &statuses,
        reply(
            StatusCode::OK,
            json!([{"id":9,"context":"build","state":"pending","updated_at":TIME}]),
        ),
    );
    let results = reader
        .pr_statuses(&pinned, 1, &validators, &cancel)
        .await
        .unwrap()
        .value
        .unwrap();
    assert_eq!(results.records[0].head_oid, HEAD);
    assert_eq!(results.records[0].state, "pending");

    let source =
        format!("/repositories/71/contents/folder/file%20with%20space.txt?ref={MERGE_BASE}");
    sim.enqueue(Method::GET, &source, reply(StatusCode::OK, json!({
        "path":"folder/file with space.txt","type":"file","size":14,"sha":BASE,
        "encoding":"base64","content":"b25lCnR3bwp0aHJlZQo=", "download_url":"https://outside.invalid/unused"
    })));
    let decoded = reader
        .pr_source_context(
            &pinned,
            data.identity(),
            PrSourceRange {
                side: PrSide::Left,
                path: "folder/file with space.txt",
                start: 2,
                lines: 1,
            },
            &cancel,
        )
        .await
        .unwrap()
        .value
        .unwrap();
    assert!(
        matches!(decoded, PrSource::Text { oid, first_line:2, text, .. } if oid == MERGE_BASE && text == "two")
    );

    let observations = sim.take_observations();
    assert_eq!(observations.len(), 12);
    assert!(observations.iter().all(|event| event.matched
        && event.expected_media_type
        && event.authorization_sent
        && event.versioned));
}

#[tokio::test]
async fn graph_reads_verify_current_pr_identity_and_resume_a_single_thread_without_flattening_it() {
    let sim = Simulator::start().await;
    let cancel = CancellationToken::new();
    let session = authorized(&sim, &cancel).await;
    let route = RepositoryName::new("fixture-org", "private").unwrap();
    let (bound, _) = session
        .bind_reader(&route, 71.try_into().unwrap(), &cancel)
        .await
        .unwrap();
    let reader = bound.reader();
    sim.enqueue(
        Method::GET,
        "/repositories/71/pulls/42",
        reply(StatusCode::OK, pull()),
    );
    let pinned = reader
        .pr_snapshot(42.try_into().unwrap(), &ReadValidators::default(), &cancel)
        .await
        .unwrap()
        .value
        .unwrap();
    let identity = || {
        json!({"id":"PR_420","fullDatabaseId":"420","baseRefOid":BASE,"headRefOid":HEAD,
        "repository":{"id":"R_71","name":"private","owner":{"login":"fixture-org"}}})
    };
    let comment = |id, reply_to| {
        let author = if id == 51 {
            json!({"login":"reviewer","__typename":"Bot","databaseId":91})
        } else {
            json!({"login":"name-looks-like-a-bot","__typename":"User","databaseId":92})
        };
        json!({"id":format!("PRRC_{id}"),"fullDatabaseId":id,"replyTo":reply_to,"author":author,
        "body":"Review draft fixture","createdAt":TIME,"updatedAt":TIME,"path":"src/lib.rs","originalLine":5,
        "originalCommit":{"oid":HEAD}})
    };
    let send = |operation, id, cursor, payload| {
        sim.enqueue(
            Method::POST,
            "/graphql",
            reply(StatusCode::OK, payload).graph_node(operation, id, cursor),
        );
    };

    let mut reviews = identity();
    reviews["reviewDecision"] = json!("CHANGES_REQUESTED");
    reviews["reviews"] = json!({"nodes":[{"id":"PRR_5000000001","fullDatabaseId":"5000000001","body":"Formal review",
        "state":"CHANGES_REQUESTED","author":{"login":"reviewer","__typename":"Bot","databaseId":91},"submittedAt":TIME,"commit":{"oid":HEAD}}],
        "pageInfo":{"hasNextPage":false,"endCursor":null}});
    send(
        "ThreadlineReviews",
        "PR_420",
        None,
        json!({"data":{"node":reviews}}),
    );
    let review = reader.pr_reviews(&pinned, None, &cancel).await.unwrap();
    assert_eq!(review.review_decision.as_deref(), Some("CHANGES_REQUESTED"));
    assert_eq!(review.records[0].id, 5_000_000_001);
    assert_eq!(
        review.records[0].actor.user_id,
        std::num::NonZeroU64::new(91)
    );
    assert!(review.records[0].actor.known_automation);

    let mut thread_page = identity();
    thread_page["reviewThreads"] = json!({"nodes":[
        {"id":"PRRT_1","path":"src/renamed.rs","line":17,"diffSide":"RIGHT","subjectType":"LINE","isResolved":true,"isOutdated":false,
            "comments":{"nodes":[comment(51, Value::Null)],"pageInfo":{"hasNextPage":true,"endCursor":"next-reply"}}},
        {"id":"PRRT_2","path":"src/renamed.rs","line":23,"diffSide":"LEFT","subjectType":"LINE","isResolved":false,"isOutdated":false,
            "comments":{"nodes":[comment(53, Value::Null)],"pageInfo":{"hasNextPage":false,"endCursor":null}}},
        {"id":"PRRT_3","path":"src/renamed.rs","line":null,"diffSide":"RIGHT","subjectType":"FILE","isResolved":false,"isOutdated":false,
            "comments":{"nodes":[comment(54, Value::Null)],"pageInfo":{"hasNextPage":false,"endCursor":null}}}],
        "pageInfo":{"hasNextPage":false,"endCursor":null}});
    send(
        "ThreadlineThreads",
        "PR_420",
        None,
        json!({"data":{"node":thread_page}}),
    );
    let threads = reader.pr_threads(&pinned, None, &cancel).await.unwrap();
    let first = &threads.records[0];
    assert_eq!(first.node_id(), "PRRT_1");
    assert_eq!(first.resolved, Some(true));
    assert_eq!(
        first.location,
        Some(PrThreadLocation::Line {
            path: "src/renamed.rs".into(),
            side: PrSide::Right,
            line: 17
        })
    );
    assert_eq!(
        threads.records[1].location,
        Some(PrThreadLocation::Line {
            path: "src/renamed.rs".into(),
            side: PrSide::Left,
            line: 23
        })
    );
    assert_eq!(
        threads.records[1].comments[0]
            .anchor
            .as_ref()
            .unwrap()
            .original_oid,
        HEAD
    );
    assert_eq!(
        threads.records[2].location,
        Some(PrThreadLocation::File {
            path: "src/renamed.rs".into()
        })
    );
    assert!(!first.replies_complete);
    assert_eq!(first.comments[0].actor, review.records[0].actor);
    let anchor = first.comments[0].anchor.as_ref().unwrap();
    assert_eq!(
        (anchor.side, anchor.line, anchor.original_oid.as_str()),
        (PrSide::Right, 5, HEAD)
    );
    assert_eq!(anchor.path, "src/lib.rs");
    let cursor = first.reply_cursor.as_deref().unwrap();
    send(
        "ThreadlineReplies",
        "PRRT_1",
        Some("next-reply"),
        json!({"data":{"node":{
            "id":"PRRT_1","diffSide":"RIGHT","pullRequest":identity(),"comments":{
                "nodes":[comment(52,json!({"fullDatabaseId":"51"}))],"pageInfo":{"hasNextPage":false,"endCursor":null}}
        }}}),
    );
    let replies = reader
        .pr_thread_replies(&pinned, first, cursor, &cancel)
        .await
        .unwrap();
    assert_eq!(replies.records[0].in_reply_to, Some(51));
    assert_eq!(
        replies.records[0].actor.user_id,
        std::num::NonZeroU64::new(92)
    );
    assert!(!replies.records[0].actor.known_automation);
    assert!(replies.next_cursor.is_none());

    let mut old = identity();
    old["headRefOid"] = json!(BASE);
    let delayed = Arc::new(Notify::new());
    sim.enqueue(
        Method::POST,
        "/graphql",
        reply(StatusCode::OK, json!({"data":{"node":old}}))
            .graph_node("ThreadlineThreads", "PR_420", None)
            .delayed_until(delayed.clone()),
    );
    let reading = reader.pr_threads(&pinned, None, &cancel);
    let release = async {
        sim.wait_for_requests(7).await;
        delayed.notify_one();
    };
    let (mismatched, ()) = tokio::join!(reading, release);
    let mismatched = mismatched.err().unwrap();
    assert_eq!(mismatched.kind, AccessKind::IdentityMismatch);
    let observations = sim.take_observations();
    assert_eq!(observations.len(), 7);
    assert!(
        observations
            .iter()
            .all(|event| event.matched && event.authorization_sent)
    );
}
