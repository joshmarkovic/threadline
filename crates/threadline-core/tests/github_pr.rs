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
            pr::{PatchEvidence, PrSide, PrSource, PrSourceRange},
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
        reply(StatusCode::OK, page).header(header::ETAG, "\"compare-page\""),
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
    assert_eq!(observations.len(), 9);
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
        json!({"id":format!("PRRC_{id}"),"fullDatabaseId":id,"replyTo":reply_to,"author":{"login":"reviewer"},
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
        "state":"CHANGES_REQUESTED","author":null,"submittedAt":TIME,"commit":{"oid":HEAD}}],
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

    let mut thread_page = identity();
    thread_page["reviewThreads"] = json!({"nodes":[{"id":"PRRT_1","diffSide":"RIGHT","isResolved":true,"isOutdated":false,
        "comments":{"nodes":[comment(51, Value::Null)],"pageInfo":{"hasNextPage":true,"endCursor":"next-reply"}}}],
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
    assert!(!first.replies_complete);
    let anchor = first.comments[0].anchor.as_ref().unwrap();
    assert_eq!(
        (anchor.side, anchor.line, anchor.original_oid.as_str()),
        (PrSide::Right, 5, HEAD)
    );
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
