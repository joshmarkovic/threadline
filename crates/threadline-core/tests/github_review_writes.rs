mod support;

use std::{num::NonZeroU64, sync::Arc};

use axum::http::{HeaderName, Method, StatusCode};
use serde_json::{Value, json};
use support::{Reply, Simulator};
use threadline_core::{
    domain::RepositoryName,
    github::{
        AccessKind, CancellationToken, GitHubClient, GitHubSession, OwnerScope, SecretToken,
        writer::{
            DispatchEvidence, WritePermission,
            review::{
                InlineReview, PendingReview, ReviewEvent, ReviewMutation, ReviewObject, ReviewSide,
                ReviewState,
            },
        },
    },
};

const PR: &str = "/repos/fixture-org/private/pulls/42";
const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TIME: &str = "2020-02-02T04:05:06Z";

fn id(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn reply(status: StatusCode, payload: Value) -> Reply {
    let mut reply = Reply::new(status, payload.to_string());
    reply.authenticated = true;
    reply.header(HeaderName::from_static("x-github-request-id"), "S08:42:RUN")
}

async fn session(simulator: &Simulator, cancel: &CancellationToken) -> GitHubSession {
    simulator.enqueue(
        Method::GET,
        "/user",
        reply(StatusCode::OK, json!({"id":41,"login":"fixture-user"})),
    );
    simulator.enqueue(Method::GET,"/repos/fixture-org/private",reply(StatusCode::OK,json!({
        "id":71,"node_id":"R_71","owner":{"login":"fixture-org"},"name":"private","private":true,
        "permissions":{"pull":true,"push":true,"admin":false}
    })));
    GitHubClient::loopback(simulator.address)
        .unwrap()
        .authenticate(
            SecretToken::fixture(),
            OwnerScope::new("fixture-org").unwrap(),
            Some(id(41)),
            cancel,
        )
        .await
        .unwrap()
}

fn pull(author: u64) -> Value {
    json!({
        "id":420,"node_id":"PR_420","number":42,"title":"Fixture PR","state":"open","body":"Closes #41",
        "updated_at":TIME,"merged":false,"draft":false,"locked":false,"user":{"id":author,"login":"fixture-author"},
        "base":{"sha":BASE,"ref":"main","repo":{"id":71,"node_id":"R_71"}},
        "head":{"sha":HEAD,"ref":"topic"}
    })
}

fn review(state: &str, reviewer: u64, head: &str) -> Value {
    json!({"id":91,"state":state,"user":{"id":reviewer,"login":"fixture-user"},"commit_id":head,
        "pull_request_url":format!("https://api.github.com{PR}")})
}

#[tokio::test]
async fn it_011_lost_submit_response_allows_exact_readback_without_sending_again() {
    let simulator = Simulator::start().await;
    let cancel = CancellationToken::new();
    let session = session(&simulator, &cancel).await;
    let route = RepositoryName::new("fixture-org", "private").unwrap();
    let (writer, _) = session
        .issue_writer(&route, 71.try_into().unwrap(), &cancel)
        .await
        .unwrap();
    simulator.enqueue(
        Method::GET,
        "/repositories/71/pulls/42",
        reply(StatusCode::OK, pull(75)),
    );
    let (pull, _) = writer
        .observe_pull_for_review(42.try_into().unwrap(), id(420), &cancel)
        .await
        .unwrap();
    let known_path = format!("{PR}/reviews/91");
    simulator.enqueue(
        Method::GET,
        &known_path,
        reply(StatusCode::OK, review("PENDING", 41, HEAD)),
    );
    let (known, _) = writer
        .observe_review_object(&pull, id(91), &cancel)
        .await
        .unwrap();
    let held = Arc::new(tokio::sync::Notify::new());
    simulator.enqueue(
        Method::POST,
        &format!("{known_path}/events"),
        reply(StatusCode::OK, review("COMMENTED", 41, HEAD))
            .expects_json(json!({"event":"COMMENT","body":"Fixture summary"}))
            .delayed_until(held.clone()),
    );
    let submitted = writer.mutate_review(
        &pull,
        ReviewMutation::SubmitPending {
            review: &known,
            event: ReviewEvent::Comment,
            body: "Fixture summary",
        },
        &cancel,
    );
    tokio::pin!(submitted);
    tokio::select! {
        reply=&mut submitted=>panic!("fixture replied before cancellation: {}",reply.is_ok()),
        _=simulator.wait_for_requests(5)=>cancel.cancel(),
    }
    assert!(matches!(submitted.await,Err(error) if error.evidence==DispatchEvidence::Ambiguous));
    held.notify_one();
    simulator.enqueue(
        Method::GET,
        &known_path,
        reply(StatusCode::OK, review("COMMENTED", 41, HEAD)),
    );
    let (observed, _) = writer
        .observe_review_object(&pull, id(91), &CancellationToken::new())
        .await
        .unwrap();
    assert!(
        observed.id() == id(91)
            && observed.author() == id(41)
            && observed.head() == HEAD
            && observed.state() == ReviewState::Submitted(ReviewEvent::Comment)
    );
    let evidence = simulator.take_observations();
    assert_eq!(evidence.len(), 6);
    assert!(evidence.iter().all(|row| row.matched && row.versioned));
}

#[tokio::test]
async fn it_011_pinned_pending_review_receipt_required_before_exact_review_submission() {
    let simulator = Simulator::start().await;
    let cancel = CancellationToken::new();
    let session = session(&simulator, &cancel).await;
    let route = RepositoryName::new("fixture-org", "private").unwrap();
    let (writer, _) = session
        .issue_writer(&route, 71.try_into().unwrap(), &cancel)
        .await
        .unwrap();

    simulator.enqueue(
        Method::GET,
        "/repositories/71/pulls/42",
        reply(StatusCode::OK, pull(75)),
    );
    let (pull, _) = writer
        .observe_pull_for_review(42.try_into().unwrap(), id(420), &cancel)
        .await
        .unwrap();
    assert!(
        writer.review_permission(&pull, Some(ReviewEvent::Approve)) == WritePermission::Allowed
    );
    let endpoint = format!("{PR}/reviews");
    simulator.enqueue(
        Method::GET,
        &format!("{endpoint}?per_page=100&page=1"),
        reply(StatusCode::OK, json!([])),
    );
    let ((pending, next), _) = writer
        .observe_own_pending(&pull, 1.try_into().unwrap(), &cancel)
        .await
        .unwrap();
    assert!(matches!(pending, PendingReview::Absent) && next.is_none());

    let inline = [
        InlineReview {
            path: "src/lib.rs",
            body: "Correctness on old line",
            side: ReviewSide::Left,
            line: 3,
            start_line: None,
            start_side: None,
        },
        InlineReview {
            path: "src/lib.rs",
            body: "New range",
            side: ReviewSide::Right,
            line: 8,
            start_line: Some(5),
            start_side: Some(ReviewSide::Right),
        },
    ];
    simulator.enqueue(Method::POST,&endpoint,reply(StatusCode::OK,review("PENDING",41,HEAD)).expects_json(json!({
        "commit_id":HEAD,"body":"Two comments","comments":[
            {"path":"src/lib.rs","body":"Correctness on old line","side":"LEFT","line":3},
            {"path":"src/lib.rs","body":"New range","side":"RIGHT","line":8,"start_line":5,"start_side":"RIGHT"}
        ]
    })));
    let created = writer
        .mutate_review(
            &pull,
            ReviewMutation::CreatePending {
                pending: &pending,
                summary: "Two comments",
                comments: &inline,
            },
            &cancel,
        )
        .await
        .unwrap();
    assert!(
        matches!(created.object,ReviewObject::Review(review) if review.id()==id(91) && review.author()==id(41) && review.head()==HEAD)
    );
    let review_path = format!("{endpoint}/91");
    simulator.enqueue(
        Method::GET,
        &review_path,
        reply(StatusCode::OK, review("PENDING", 41, HEAD)),
    );
    let (known, _) = writer
        .observe_review_object(&pull, id(91), &cancel)
        .await
        .unwrap();
    let send_path = format!("{review_path}/events");
    simulator.enqueue(
        Method::POST,
        &send_path,
        reply(StatusCode::OK, review("APPROVED", 41, HEAD))
            .expects_json(json!({"event":"APPROVE","body":"Reviewed exact head"})),
    );
    let submitted = writer
        .mutate_review(
            &pull,
            ReviewMutation::SubmitPending {
                review: &known,
                event: ReviewEvent::Approve,
                body: "Reviewed exact head",
            },
            &cancel,
        )
        .await
        .unwrap();
    assert_eq!(submitted.request_id, "S08:42:RUN");
    assert!(
        matches!(submitted.object,ReviewObject::Review(review) if review.id()==id(91) && review.state()==ReviewState::Submitted(ReviewEvent::Approve))
    );
    assert!(
        simulator
            .take_observations()
            .iter()
            .all(|row| row.matched && row.versioned && row.authorization_sent)
    );
}

#[tokio::test]
async fn it_013_wrong_actor_revision_and_thread_guard_keep_exact_review_scope() {
    let simulator = Simulator::start().await;
    let cancel = CancellationToken::new();
    let session = session(&simulator, &cancel).await;
    let route = RepositoryName::new("fixture-org", "private").unwrap();
    let (writer, _) = session
        .issue_writer(&route, 71.try_into().unwrap(), &cancel)
        .await
        .unwrap();

    simulator.enqueue(
        Method::GET,
        "/repositories/71/pulls/42",
        reply(StatusCode::OK, pull(41)),
    );
    let wrong = writer
        .observe_pull_for_review(42.try_into().unwrap(), id(999), &cancel)
        .await;
    assert!(matches!(wrong,Err(error) if error.kind==AccessKind::IdentityMismatch));
    simulator.enqueue(
        Method::GET,
        "/repositories/71/pulls/42",
        reply(StatusCode::OK, pull(41)),
    );
    let (pull, _) = writer
        .observe_pull_for_review(42.try_into().unwrap(), id(420), &cancel)
        .await
        .unwrap();
    assert!(writer.review_permission(&pull, Some(ReviewEvent::Approve)) == WritePermission::Denied);
    assert!(
        writer.review_permission(&pull, Some(ReviewEvent::RequestChanges))
            == WritePermission::Denied
    );

    let node = |head: &str, resolved: bool| {
        json!({
            "id":"THREAD_440","isOutdated":false,"isResolved":resolved,"viewerCanResolve":!resolved,
            "viewerCanUnresolve":resolved,"comments":{"nodes":[{"fullDatabaseId":"349"}]},
            "pullRequest":{"id":"PR_420","number":42,"headRefOid":head,"baseRefOid":BASE,"repository":{"id":"R_71"}}
        })
    };
    simulator.enqueue(
        Method::POST,
        "/graphql",
        reply(StatusCode::OK, json!({"data":{"node":node(BASE,false)}})),
    );
    let stale = writer
        .observe_review_thread(&pull, "THREAD_440", &cancel)
        .await;
    assert!(matches!(stale,Err(error) if error.kind==AccessKind::IdentityMismatch));
    simulator.enqueue(
        Method::POST,
        "/graphql",
        reply(StatusCode::OK, json!({"data":{"node":node(HEAD,false)}})),
    );
    let (thread, _) = writer
        .observe_review_thread(&pull, "THREAD_440", &cancel)
        .await
        .unwrap();
    let before = simulator.take_observations();
    let wrong_root = writer
        .mutate_review(
            &pull,
            ReviewMutation::Reply {
                thread: &thread,
                root: id(111),
                body: "Fixture reply",
            },
            &cancel,
        )
        .await;
    assert!(matches!(wrong_root,Err(error) if error.evidence==DispatchEvidence::InvalidInput));
    assert!(simulator.take_observations().is_empty());
    assert!(
        before
            .iter()
            .all(|row| row.matched && row.authorization_sent)
    );

    let reply_body = json!({"id":501,"user":{"id":41,"login":"fixture-user"},"body":"Fixture reply",
        "in_reply_to_id":349,"pull_request_url":format!("https://api.github.com{PR}")});
    simulator.enqueue(
        Method::POST,
        &format!("{PR}/comments/349/replies"),
        reply(StatusCode::CREATED, reply_body).expects_json(json!({"body":"Fixture reply"})),
    );
    let result = writer
        .mutate_review(
            &pull,
            ReviewMutation::Reply {
                thread: &thread,
                root: id(349),
                body: "Fixture reply",
            },
            &cancel,
        )
        .await
        .unwrap();
    assert!(
        matches!(result.object,ReviewObject::Reply(row) if row.id==id(501) && row.root==id(349))
    );

    simulator.enqueue(
        Method::POST,
        "/graphql",
        reply(
            StatusCode::OK,
            json!({"data":{"change":{
                "clientMutationId":"different_attempt","thread":node(HEAD,true)
            }}}),
        ),
    );
    let unsupported = writer
        .mutate_review(
            &pull,
            ReviewMutation::SetThreadResolved {
                thread: &thread,
                resolved: true,
                correlation: "fixture_attempt_9",
            },
            &cancel,
        )
        .await;
    assert!(matches!(unsupported,Err(error) if error.evidence==DispatchEvidence::Ambiguous));
    simulator.enqueue(
        Method::POST,
        "/graphql",
        reply(
            StatusCode::OK,
            json!({"data":{"change":{
                "clientMutationId":"fixture_attempt_10","thread":node(HEAD,true)
            }}}),
        ),
    );
    let changed = writer
        .mutate_review(
            &pull,
            ReviewMutation::SetThreadResolved {
                thread: &thread,
                resolved: true,
                correlation: "fixture_attempt_10",
            },
            &cancel,
        )
        .await
        .unwrap();
    assert!(matches!(changed.object,ReviewObject::Thread{id,resolved:true} if id=="THREAD_440"));
    let rows = simulator.take_observations();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.matched && row.authorization_sent));
}
