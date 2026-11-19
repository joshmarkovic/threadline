#[allow(dead_code)]
mod support;

use axum::http::{HeaderName, Method, StatusCode};
use serde_json::{Value, json};
use std::num::NonZeroU64;
use support::{Reply, Simulator};
use threadline_core::{
    domain::RepositoryName,
    github::{
        AccessKind, CancellationToken, GitHubClient, OwnerScope, SecretToken,
        writer::{
            DispatchEvidence,
            merge::{DirectMergeRejection, MergeMethod, QueueRequirement},
        },
    },
};

const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const MERGED: &str = "cccccccccccccccccccccccccccccccccccccccc";
const MERGE: &str = "/repos/fixture-org/private/pulls/42/merge";

fn reply(status: StatusCode, body: Value) -> Reply {
    let mut reply = Reply::new(status, body.to_string());
    reply.authenticated = true;
    reply.header(
        HeaderName::from_static("x-github-request-id"),
        "S09:42:MERGE",
    )
}

fn policy(queue: Value, head: &str) -> Value {
    json!({"data":{"node":{
        "id":"PR_420","number":42,"baseRefName":"main","baseRefOid":BASE,"headRefOid":head,
        "state":"OPEN","isDraft":false,"merged":false,"mergeable":"MERGEABLE","mergeStateStatus":"CLEAN",
        "reviewDecision":"APPROVED","mergeQueue":queue,"repository":{
            "id":"R_71","viewerPermission":"WRITE","mergeCommitAllowed":true,
            "squashMergeAllowed":true,"rebaseMergeAllowed":false
        }
    }}})
}

#[tokio::test]
async fn it_014_direct_merge_requires_known_policy_and_sends_exact_compare_and_swap_head() {
    let simulator = Simulator::start().await;
    let cancel = CancellationToken::new();
    simulator.enqueue(
        Method::GET,
        "/user",
        reply(StatusCode::OK, json!({"id":41,"login":"fixture-user"})),
    );
    simulator.enqueue(Method::GET,"/repos/fixture-org/private",reply(StatusCode::OK,json!({
        "id":71,"node_id":"R_71","owner":{"login":"fixture-org"},"name":"private","private":true,
        "permissions":{"pull":true,"push":true,"admin":false}
    })));
    simulator.enqueue(
        Method::GET,
        "/repositories/71/pulls/42",
        reply(
            StatusCode::OK,
            json!({
                "id":420,"node_id":"PR_420","number":42,"title":"Fixture merge","state":"open",
                "updated_at":"2020-01-02T00:00:00Z","merged":false,"draft":false,"locked":false,
                "base":{"sha":BASE,"ref":"main","repo":{"id":71,"node_id":"R_71"}},
                "head":{"sha":HEAD,"ref":"topic"}
            }),
        ),
    );
    let session = GitHubClient::loopback(simulator.address)
        .unwrap()
        .authenticate(
            SecretToken::fixture(),
            OwnerScope::new("fixture-org").unwrap(),
            NonZeroU64::new(41),
            &cancel,
        )
        .await
        .unwrap();
    let route = RepositoryName::new("fixture-org", "private").unwrap();
    let (writer, _) = session
        .issue_writer(&route, 71.try_into().unwrap(), &cancel)
        .await
        .unwrap();
    let (pull, _) = writer
        .observe_pull_for_review(42.try_into().unwrap(), 420.try_into().unwrap(), &cancel)
        .await
        .unwrap();
    simulator.enqueue(
        Method::POST,
        "/graphql",
        reply(StatusCode::OK, policy(Value::Null, BASE)),
    );
    let bad = writer.observe_merge_policy(&pull, &cancel).await;
    assert!(matches!(bad,Err(error) if error.kind==AccessKind::IdentityMismatch));

    let mut required = policy(Value::Null, HEAD);
    required["data"]["node"]["isMergeQueueEnabled"] = json!(true);
    simulator.enqueue(Method::POST, "/graphql", reply(StatusCode::OK, required));
    let (queued, _) = writer.observe_merge_policy(&pull, &cancel).await.unwrap();
    assert!(queued.eligibility().queue == QueueRequirement::Required);
    let observed = simulator.take_observations();
    let blocked = writer
        .merge_at_head(&pull, &queued, MergeMethod::Squash, HEAD, &cancel)
        .await;
    assert!(matches!(blocked,Err(error) if error.reason==DirectMergeRejection::NotPermitted));
    assert!(simulator.take_observations().is_empty());
    assert!(
        observed
            .iter()
            .all(|entry| entry.authorization_sent && entry.matched)
    );

    simulator.enqueue(
        Method::POST,
        "/graphql",
        reply(StatusCode::OK, policy(Value::Null, HEAD)),
    );
    let (allowed, _) = writer.observe_merge_policy(&pull, &cancel).await.unwrap();
    assert!(allowed.eligibility().allows(MergeMethod::Squash));
    assert!(!allowed.eligibility().allows(MergeMethod::Rebase));
    let _ = simulator.take_observations();
    let stale = writer
        .merge_at_head(&pull, &allowed, MergeMethod::Squash, BASE, &cancel)
        .await;
    assert!(
        matches!(stale,Err(error) if error.reason==DirectMergeRejection::HeadChanged &&
        error.transport.evidence==DispatchEvidence::DefinitelyUnsent)
    );
    assert!(simulator.take_observations().is_empty());

    simulator.enqueue(
        Method::PUT,
        MERGE,
        reply(StatusCode::CONFLICT, json!({"message":"Head changed"}))
            .expects_json(json!({"sha":HEAD,"merge_method":"squash"})),
    );
    let conflicted = writer
        .merge_at_head(&pull, &allowed, MergeMethod::Squash, HEAD, &cancel)
        .await;
    assert!(
        matches!(conflicted,Err(error) if error.reason==DirectMergeRejection::HeadChanged &&
        error.transport.access.http_status==Some(409))
    );

    for (message, reason) in [
        (
            "This pull request must be merged via the merge queue.",
            DirectMergeRejection::RequiredMergeQueue,
        ),
        (
            "Merge cannot be performed",
            DirectMergeRejection::NotPermitted,
        ),
    ] {
        simulator.enqueue(
            Method::PUT,
            MERGE,
            reply(StatusCode::METHOD_NOT_ALLOWED, json!({"message":message}))
                .expects_json(json!({"sha":HEAD,"merge_method":"squash"})),
        );
        let rejected = writer
            .merge_at_head(&pull, &allowed, MergeMethod::Squash, HEAD, &cancel)
            .await;
        assert!(
            matches!(rejected, Err(error) if error.reason == reason && error.transport.access.http_status == Some(405))
        );
    }

    simulator.enqueue(
        Method::PUT,
        MERGE,
        reply(StatusCode::OK, json!({"merged":true,"sha":MERGED}))
            .expects_json(json!({"sha":HEAD,"merge_method":"squash"})),
    );
    let merged = writer
        .merge_at_head(&pull, &allowed, MergeMethod::Squash, HEAD, &cancel)
        .await
        .unwrap();
    assert_eq!(merged.merge_oid, MERGED);
    assert_eq!(merged.request_id, "S09:42:MERGE");
    let effects = simulator.take_observations();
    assert_eq!(effects.len(), 4);
    assert!(
        effects
            .iter()
            .all(|entry| entry.authorization_sent && entry.versioned && entry.matched)
    );
}
