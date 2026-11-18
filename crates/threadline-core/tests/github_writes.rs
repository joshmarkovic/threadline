mod support;

use std::{num::NonZeroU64, sync::Arc};

use axum::http::{HeaderName, Method, StatusCode, header};
use serde_json::{Value, json};
use support::{Reply, Simulator};
use threadline_core::{
    domain::RepositoryName,
    github::{
        AccessKind, CancellationToken, GitHubClient, GitHubSession, OwnerScope, SecretToken,
        writer::{
            ChoiceValues, DispatchEvidence, IssueChoices, IssueMutation, IssuePatch,
            IssueWriteTarget, MutationObject, WritePermission,
        },
    },
};
use tokio::sync::Notify;

const ROUTE: &str = "/repos/fixture-org/private";
const ISSUE: &str = "/repos/fixture-org/private/issues/42";
const COMMENT: &str = "/repos/fixture-org/private/issues/comments/74";
const CREATE: &str = "/repos/fixture-org/private/issues/42/comments";
const TIME: &str = "2020-02-02T04:05:06Z";

fn id(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn reply(status: StatusCode, body: Value) -> Reply {
    let mut reply = Reply::new(status, body.to_string());
    reply.authenticated = true;
    reply.header(
        HeaderName::from_static("x-github-request-id"),
        "E118:44:892",
    )
}

fn issue() -> Value {
    json!({
        "id":55, "number":42, "title":"Existing issue","body":null,"labels":[{"name":"bug"}],
        "assignees":[{"login":"fixture-user","id":41}],"milestone":{"number":2,"title":"Next"},
        "state":"open","locked":false,"updated_at":TIME,
        "user":{"id":41,"login":"fixture-user"},
        "repository_url":"https://api.github.com/repos/fixture-org/private"
    })
}

fn comment(body: &str) -> Value {
    json!({
        "id":74,"body":body,"user":{"login":"fixture-user","id":41},"created_at":TIME,"updated_at":TIME,
        "issue_url":"https://api.github.com/repos/fixture-org/private/issues/42"
    })
}

fn target() -> IssueWriteTarget {
    IssueWriteTarget {
        number: 42.try_into().unwrap(),
        issue_id: id(55),
        pull_request: false,
    }
}

async fn session(sim: &Simulator, cancel: &CancellationToken) -> GitHubSession {
    sim.enqueue(
        Method::GET,
        "/user",
        reply(StatusCode::OK, json!({"id":41,"login":"fixture-user"})),
    );
    sim.enqueue(Method::GET,ROUTE,reply(StatusCode::OK,json!({
        "id":71,"node_id":"R_71","owner":{"login":"fixture-org"},"name":"private","private":true,
        "permissions":{"pull":true,"push":true,"admin":false}
    })));
    GitHubClient::loopback(sim.address)
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

#[tokio::test]
async fn it_008_issue_patch_and_ordinary_comment_create_and_edit_are_exactly_scoped() {
    let sim = Simulator::start().await;
    let cancel = CancellationToken::new();
    let session = session(&sim, &cancel).await;
    let (writer, _) = session
        .issue_writer(
            &RepositoryName::new("fixture-org", "private").unwrap(),
            71.try_into().unwrap(),
            &cancel,
        )
        .await
        .unwrap();
    sim.enqueue(Method::GET, ISSUE, reply(StatusCode::OK, issue()));
    let (observed, _) = writer.observe_issue(target(), &cancel).await.unwrap();
    assert_eq!(observed.observed.body, Some(None));
    assert_eq!(observed.milestone_title.as_deref(), Some("Next"));

    sim.enqueue(
        Method::GET,
        "/repos/fixture-org/private/milestones?per_page=100&page=1&state=all",
        reply(StatusCode::OK, json!([{"number":2,"title":"Next"}])),
    );
    let choices = writer
        .issue_choices(IssueChoices::Milestones, 1.try_into().unwrap(), &cancel)
        .await
        .unwrap();
    assert!(
        matches!(choices.values, ChoiceValues::Milestones(ref entries) if entries.get(&id(2)).is_some_and(|v|v=="Next"))
    );
    assert!(choices.next.is_none() && !choices.sso_limited);

    let patch = IssuePatch {
        title: Some("Updated issue".into()),
        milestone: Some(None),
        ..Default::default()
    };
    let mut after = issue();
    after["title"] = json!("Updated issue");
    after["milestone"] = Value::Null;
    sim.enqueue(
        Method::PATCH,
        ISSUE,
        reply(StatusCode::OK, after)
            .expects_json(json!({"title":"Updated issue","milestone":null})),
    );
    let mutation = IssueMutation::EditIssue(&patch);
    assert!(writer.permission(&observed, &mutation) == WritePermission::Allowed);
    let result = writer.mutate(&observed, mutation, &cancel).await.unwrap();
    assert_eq!(result.request_id, "E118:44:892");
    assert!(
        matches!(result.object, MutationObject::Issue(issue) if issue.observed.title.as_deref() == Some("Updated issue"))
    );

    sim.enqueue(
        Method::POST,
        CREATE,
        reply(StatusCode::CREATED, comment("new body")).expects_json(json!({"body":"new body"})),
    );
    let result = writer
        .mutate(
            &observed,
            IssueMutation::CreateComment { body: "new body" },
            &cancel,
        )
        .await
        .unwrap();
    assert!(
        matches!(result.object, MutationObject::Comment(comment) if comment.id() == id(74) && comment.author_id() == Some(id(41)))
    );

    sim.enqueue(
        Method::GET,
        COMMENT,
        reply(StatusCode::OK, comment("new body")),
    );
    let (comment_to_edit, _) = writer
        .observe_comment(&observed, id(74), &cancel)
        .await
        .unwrap();
    sim.enqueue(
        Method::PATCH,
        COMMENT,
        reply(StatusCode::OK, comment("edit body")).expects_json(json!({"body":"edit body"})),
    );
    let result = writer
        .mutate(
            &observed,
            IssueMutation::EditComment {
                comment: &comment_to_edit,
                body: "edit body",
            },
            &cancel,
        )
        .await
        .unwrap();
    assert!(
        matches!(result.object, MutationObject::Comment(comment) if comment.body == "edit body")
    );
    let rows = sim.take_observations();
    assert_eq!(rows.len(), 8);
    assert!(
        rows.iter()
            .all(|row| row.matched && row.versioned && row.authorization_sent)
    );
}

#[tokio::test]
async fn it_009_write_rejection_throttle_wrong_identity_and_post_send_loss_are_distinct() {
    let sim = Simulator::start().await;
    let cancel = CancellationToken::new();
    let session = session(&sim, &cancel).await;
    let (writer, _) = session
        .issue_writer(
            &RepositoryName::new("fixture-org", "private").unwrap(),
            71.try_into().unwrap(),
            &cancel,
        )
        .await
        .unwrap();
    let mut wrong = issue();
    wrong["repository_url"] = json!("https://api.github.com/repos/fixture-org/replaced");
    sim.enqueue(Method::GET, ISSUE, reply(StatusCode::OK, wrong));
    let result = writer.observe_issue(target(), &cancel).await;
    assert!(matches!(result,Err(error) if error.kind == AccessKind::IdentityMismatch));

    sim.enqueue(Method::GET, ISSUE, reply(StatusCode::OK, issue()));
    let (observed, _) = writer.observe_issue(target(), &cancel).await.unwrap();
    let mutation = IssueMutation::CreateComment { body: "only once" };
    let outcomes = [
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            DispatchEvidence::InvalidInput,
        ),
        (StatusCode::UNAUTHORIZED, DispatchEvidence::AccessRejected),
        (StatusCode::TOO_MANY_REQUESTS, DispatchEvidence::Throttled),
        (StatusCode::SERVICE_UNAVAILABLE, DispatchEvidence::Ambiguous),
    ];
    for (status, expected) in outcomes {
        sim.enqueue(
            Method::POST,
            CREATE,
            reply(status, json!({"message":"Synthetic failure"})).header(header::RETRY_AFTER, "12"),
        );
        let result = writer.mutate(&observed, mutation, &cancel).await;
        assert!(
            matches!(result,Err(error) if error.evidence == expected && error.access.http_status == Some(status.as_u16()))
        );
    }
    let before = sim.take_observations();
    assert_eq!(before.len(), 8);
    assert!(before.iter().all(|row| row.matched));

    let hold_response = Arc::new(Notify::new());
    sim.enqueue(
        Method::POST,
        CREATE,
        reply(StatusCode::CREATED, comment("only once"))
            .expects_json(json!({"body":"only once"}))
            .delayed_until(hold_response.clone()),
    );
    let send = writer.mutate(&observed, mutation, &cancel);
    tokio::pin!(send);
    tokio::select! {
        _ = &mut send => panic!("fixture responded before the synthetic loss"),
        _ = sim.wait_for_requests(before.len()+1) => cancel.cancel(),
    }
    let result = send.await;
    assert!(
        matches!(result,Err(error) if error.evidence == DispatchEvidence::Ambiguous && error.access.kind == AccessKind::Cancelled)
    );
    hold_response.notify_one();
    assert_eq!(sim.take_observations().len(), 1);

    let resubmit = writer.mutate(&observed, mutation, &cancel).await;
    assert!(matches!(resubmit,Err(error) if error.evidence == DispatchEvidence::DefinitelyUnsent));
    assert!(sim.take_observations().is_empty());
}
