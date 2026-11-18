mod support;

use std::{num::NonZeroU64, sync::Arc};

use axum::http::{Method, StatusCode, header};
use serde_json::{Value, json};
use support::{Reply, Simulator};
use threadline_core::{
    domain::RepositoryName,
    github::{
        AccessKind, CancellationToken, GitHubClient, OwnerScope, SecretToken,
        reader::{
            history::{HistoryDiff, HistoryRefKind, HistoryTagObject},
            pr::PatchEvidence,
        },
    },
};

const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const COMMON: &str = "cccccccccccccccccccccccccccccccccccccccc";
const TAG: &str = "dddddddddddddddddddddddddddddddddddddddd";
const TIME: &str = "2026-01-02T03:04:05Z";

fn reply(body: Value) -> Reply {
    let mut reply = Reply::new(StatusCode::OK, body.to_string());
    reply.authenticated = true;
    reply
}

#[tokio::test]
async fn pinned_history_resumes_by_commit_and_never_relabels_a_three_dot_result() {
    let sim = Simulator::start().await;
    let cancel = CancellationToken::new();
    sim.enqueue(
        Method::GET,
        "/user",
        reply(json!({"id":41,"login":"fixture-user"})),
    );
    sim.enqueue(
        Method::GET,
        "/repos/fixture-org/private",
        reply(json!({
            "id":71,"node_id":"R_71","name":"private","private":true,"owner":{"login":"fixture-org"}
        })),
    );
    let session = Arc::new(
        GitHubClient::loopback(sim.address)
            .unwrap()
            .authenticate(
                SecretToken::fixture(),
                OwnerScope::new("fixture-org").unwrap(),
                NonZeroU64::new(41),
                &cancel,
            )
            .await
            .unwrap(),
    );
    let (bound, _) = session
        .bind_reader(
            &RepositoryName::new("fixture-org", "private").unwrap(),
            71.try_into().unwrap(),
            &cancel,
        )
        .await
        .unwrap();
    let reader = bound.reader();
    sim.enqueue(
        Method::GET,
        "/repositories/71/branches?per_page=20&page=1",
        reply(json!([{"name":"topic/x","commit":{"sha":HEAD}}])),
    );
    assert_eq!(
        reader
            .history_refs(HistoryRefKind::Branch, 1, &cancel)
            .await
            .unwrap()
            .value
            .unwrap()
            .records[0]
            .oid,
        HEAD
    );
    sim.enqueue(
        Method::GET,
        "/repositories/71/branches/topic%2Fx",
        reply(json!({"name":"topic/x","commit":{"sha":HEAD}})),
    );
    assert_eq!(
        reader
            .history_branch("topic/x", &cancel)
            .await
            .unwrap()
            .value
            .unwrap()
            .oid,
        HEAD
    );
    sim.enqueue(
        Method::GET,
        "/repositories/71/git/ref/tags/rc%2F1",
        reply(json!({"ref":"refs/tags/rc/1","object":{"type":"tag","sha":TAG}})),
    );
    assert!(
        matches!(reader.history_tag_ref("rc/1", &cancel).await.unwrap().value,
        Some(HistoryTagObject::Tag(oid)) if oid == TAG)
    );
    sim.enqueue(
        Method::GET,
        &format!("/repositories/71/git/tags/{TAG}"),
        reply(json!({"sha":TAG,"object":{"type":"commit","sha":HEAD}})),
    );
    assert!(
        matches!(reader.history_annotated_tag(TAG,&cancel).await.unwrap().value,
        Some(HistoryTagObject::Commit(oid)) if oid == HEAD)
    );

    let commit = |oid: &str, parents: Value| {
        json!({"sha":oid,"parents":parents,"commit":{
        "message":"Scoped commit\n\nDetailed description","author":{"name":"Real commit author","date":TIME},
        "committer":{"name":"Committer","date":TIME}}})
    };
    let first = format!("/repositories/71/commits?sha={HEAD}&per_page=20&page=1");
    let next = format!("/repositories/71/commits?sha={HEAD}&per_page=20&page=2");
    sim.enqueue(
        Method::GET,
        &first,
        reply(json!([commit(HEAD, json!([{"sha":BASE},{"sha":COMMON}]))])).header(
            header::LINK,
            format!("<http://{}{next}>; rel=\"next\"", sim.address),
        ),
    );
    let page = reader
        .history_commits(HEAD, 1, &cancel)
        .await
        .unwrap()
        .value
        .unwrap();
    assert_eq!(page.next_page, Some(2));
    assert_eq!(page.records[0].parents, [BASE, COMMON]);
    assert_eq!(
        page.records[0].author.as_deref(),
        Some("Real commit author")
    );
    sim.enqueue(
        Method::GET,
        &next,
        reply(json!([commit(BASE, json!([])), commit(COMMON, json!([]))])),
    );
    let page = reader
        .history_commits(HEAD, 2, &cancel)
        .await
        .unwrap()
        .value
        .unwrap();
    assert!(!page.limited && page.next_page.is_none());

    let first = format!("/repositories/71/commits?sha={HEAD}&per_page=20&page=1");
    sim.enqueue(
        Method::GET,
        &first,
        reply(json!([commit(HEAD, json!([]))])).header(
            header::LINK,
            format!(
                "<http://{}/repositories/71/commits?sha={BASE}&per_page=20&page=2>; rel=\"next\"",
                sim.address
            ),
        ),
    );
    assert!(
        matches!(reader.history_commits(HEAD, 1, &cancel).await, Err(error) if error.kind==AccessKind::InvalidPagination)
    );

    let comparison = format!("/repositories/71/compare/{BASE}...{HEAD}?per_page=20&page=1");
    let changes = json!({"base_commit":{"sha":BASE},"merge_base_commit":{"sha":COMMON},"files":[
        {"filename":"chapter.md","status":"added","additions":1,"deletions":0,"patch":"@@ -0,0 +1 @@\n+Fixture"}]});
    sim.enqueue(Method::GET, &comparison, reply(changes.clone()));
    let diff = reader
        .history_files(
            HistoryDiff::Direct {
                base: BASE,
                head: HEAD,
            },
            1,
            &cancel,
        )
        .await
        .unwrap()
        .value
        .unwrap();
    assert!(diff.page.limited && diff.page.records.is_empty() && diff.effective_base.is_none());
    sim.enqueue(Method::GET, &comparison, reply(changes));
    let diff = reader
        .history_files(
            HistoryDiff::MergeBase {
                base: BASE,
                head: HEAD,
            },
            1,
            &cancel,
        )
        .await
        .unwrap()
        .value
        .unwrap();
    assert_eq!(diff.effective_base.as_deref(), Some(COMMON));
    assert!(matches!(diff.page.records[0].patch, PatchEvidence::Text(_)));

    let root = format!("/repositories/71/commits/{BASE}?per_page=20&page=1");
    let mut data = commit(BASE, json!([]));
    data["files"] = json!([{"filename":"original.md","status":"added","additions":1,"deletions":0,
        "patch":"@@ -0,0 +1 @@\n+Original"}]);
    sim.enqueue(Method::GET, &root, reply(data));
    let root = reader
        .history_files(HistoryDiff::Root { head: BASE }, 1, &cancel)
        .await
        .unwrap()
        .value
        .unwrap();
    assert!(root.effective_base.is_none() && !root.page.limited);
    assert_eq!(root.page.records[0].path, "original.md");
    let gate = Arc::new(tokio::sync::Notify::new());
    let pending = CancellationToken::new();
    sim.enqueue(
        Method::GET,
        "/repositories/71/branches/topic%2Fx",
        reply(json!({"name":"topic/x","commit":{"sha":HEAD}})).delayed_until(gate.clone()),
    );
    let request = reader.history_branch("topic/x", &pending);
    tokio::pin!(request);
    tokio::select! {
        _ = sim.wait_for_requests(13) => pending.cancel(),
        _ = &mut request => panic!("fixture responded before cancellation"),
    }
    assert!(matches!(request.await, Err(error) if error.kind == AccessKind::Cancelled));
    gate.notify_one();
    let observations = sim.take_observations();
    assert_eq!(observations.len(), 13);
    assert!(
        observations
            .iter()
            .all(|row| row.matched && row.authorization_sent && row.versioned)
    );
}
