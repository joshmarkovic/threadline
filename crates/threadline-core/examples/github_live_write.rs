//! One explicitly authorized mutation proof for the disposable existing issue.
use std::{
    env, io,
    num::NonZeroU64,
    path::Path,
    process::ExitCode,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(test)]
use std::fs;

use serde::Serialize;
use serde_json::json;
use threadline_core::{
    domain::RepositoryName,
    github::{
        AccessError, CancellationToken, GitHubClient, OwnerScope, SecretToken,
        writer::{
            DispatchEvidence, GitHubWriter, IssueMutation, IssuePatch, IssueWriteTarget,
            MutationObject, MutationResponse, VerifiedIssue,
        },
    },
};

const CASE: &str = "LIVE-002";
const PROFILE: &str = "threadline-disposable-issue-1";
const REPO: &str = "joshmarkovic/threadline";
const USER: u64 = 52_184_130;
const REPO_ID: u64 = 1_462_273_296;
const ISSUE: u64 = 6_476_663_523;
const NUMBER: u64 = 1;
const ORIGINAL: &str = "Dogfood issue for edits and comments";
const UPDATED: &str = "Dogfood issue for edits and comments [Threadline LIVE-002]";
const CREATED: &str = "Threadline LIVE-002 synthetic ordinary comment.";
const EDITED: &str = "Threadline LIVE-002 synthetic ordinary comment. Edit verified.";
const ARMED: &str = "live-002-issue-1-attempt.json";
const COMPLETED: &str = "live-002-issue-1-complete.json";

mod live_support;
use live_support::{Options, Scope, checkpoint};
const SCOPE: Scope = Scope {
    repository: REPO,
    actor: USER,
    profile: PROFILE,
};

#[derive(Serialize)]
struct Step {
    action: &'static str,
    http_status: u16,
    request_id: String,
}

#[derive(Serialize)]
struct Failure {
    stage: &'static str,
    outcome: &'static str,
    kind: Option<String>,
    http_status: Option<u16>,
}
impl Failure {
    fn guard(stage: &'static str, outcome: &'static str) -> Self {
        Self {
            stage,
            outcome,
            kind: None,
            http_status: None,
        }
    }

    fn read(stage: &'static str, error: AccessError) -> Self {
        Self {
            stage,
            outcome: "read_failed",
            kind: Some(format!("{:?}", error.kind)),
            http_status: error.http_status,
        }
    }
}

#[derive(Default, Serialize)]
struct Proof {
    identity_verified: bool,
    disposable_issue_verified: bool,
    journal_armed: bool,
    title_read_back: bool,
    comment_id: Option<u64>,
    comment_creation_read_back: bool,
    comment_edit_read_back: bool,
    original_title_restored: bool,
    steps: Vec<Step>,
    failure: Option<Failure>,
    cleanup_failure: Option<Failure>,
    #[serde(skip)]
    server_hold: bool,
}

fn arm(path: &Path) -> io::Result<()> {
    checkpoint(
        path,
        ARMED,
        &json!({"scenario":CASE,"issue_number":NUMBER,"state":"mutation_attempt_reserved"}),
    )
}

fn completed(path: &Path, report: &serde_json::Value) -> io::Result<()> {
    checkpoint(path, COMPLETED, report)
}

fn target() -> IssueWriteTarget {
    IssueWriteTarget {
        number: NUMBER.try_into().unwrap(),
        issue_id: NonZeroU64::new(ISSUE).unwrap(),
        pull_request: false,
    }
}

async fn current(
    writer: &GitHubWriter<'_>,
    cancel: &CancellationToken,
    stage: &'static str,
) -> Result<VerifiedIssue, Failure> {
    writer
        .observe_issue(target(), cancel)
        .await
        .map(|value| value.0)
        .map_err(|error| Failure::read(stage, error))
}

async fn change(
    writer: &GitHubWriter<'_>,
    issue: &VerifiedIssue,
    mutation: IssueMutation<'_>,
    cancel: &CancellationToken,
    proof: &mut Proof,
    action: &'static str,
    status: u16,
) -> Result<MutationResponse, Failure> {
    if proof.server_hold {
        return Err(Failure::guard(action, "server_hold_respected"));
    }
    if !proof.steps.is_empty() {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    match writer.mutate(issue, mutation, cancel).await {
        Ok(receipt) => {
            proof.server_hold = receipt.rate.not_before_ms(now).is_some();
            proof.steps.push(Step {
                action,
                http_status: status,
                request_id: receipt.request_id.clone(),
            });
            Ok(receipt)
        }
        Err(error) => {
            proof.server_hold = error.evidence == DispatchEvidence::Throttled
                || error
                    .access
                    .rate
                    .is_some_and(|rate| rate.not_before_ms(now).is_some());
            Err(Failure {
                stage: action,
                outcome: match error.evidence {
                    DispatchEvidence::DefinitelyUnsent => "definitely_unsent",
                    DispatchEvidence::AccessRejected => "access_rejected",
                    DispatchEvidence::InvalidInput => "invalid_input",
                    DispatchEvidence::Throttled => "throttled",
                    DispatchEvidence::Ambiguous => "outcome_unknown",
                },
                kind: Some(format!("{:?}", error.access.kind)),
                http_status: error.access.http_status,
            })
        }
    }
}

async fn edit_title(
    writer: &GitHubWriter<'_>,
    issue: &VerifiedIssue,
    title: &str,
    cancel: &CancellationToken,
    proof: &mut Proof,
    action: &'static str,
) -> Result<(), Failure> {
    let patch = IssuePatch {
        title: Some(title.to_owned()),
        ..Default::default()
    };
    let reply = change(
        writer,
        issue,
        IssueMutation::EditIssue(&patch),
        cancel,
        proof,
        action,
        200,
    )
    .await?;
    if !matches!(reply.object,MutationObject::Issue(observed) if observed.observed.title.as_deref()==Some(title))
    {
        return Err(Failure::guard(action, "response_value_unverified"));
    }
    Ok(())
}

async fn comments(
    writer: &GitHubWriter<'_>,
    issue: &VerifiedIssue,
    cancel: &CancellationToken,
    proof: &mut Proof,
) -> Result<(), Failure> {
    let receipt = change(
        writer,
        issue,
        IssueMutation::CreateComment { body: CREATED },
        cancel,
        proof,
        "create_comment",
        201,
    )
    .await?;
    let MutationObject::Comment(created) = receipt.object else {
        return Err(Failure::guard(
            "create_comment",
            "response_value_unverified",
        ));
    };
    let id = created.id();
    proof.comment_id = Some(id.get());
    let (observed, _) = writer
        .observe_comment(issue, id, cancel)
        .await
        .map_err(|e| Failure::read("read_created_comment", e))?;
    if observed.author_id() != Some(writer.actor()) || observed.body != CREATED {
        return Err(Failure::guard(
            "read_created_comment",
            "response_value_unverified",
        ));
    }
    proof.comment_creation_read_back = true;
    let _ = change(
        writer,
        issue,
        IssueMutation::EditComment {
            comment: &observed,
            body: EDITED,
        },
        cancel,
        proof,
        "edit_comment",
        200,
    )
    .await?;
    let (edited, _) = writer
        .observe_comment(issue, id, cancel)
        .await
        .map_err(|e| Failure::read("read_edited_comment", e))?;
    if edited.author_id() != Some(writer.actor()) || edited.body != EDITED {
        return Err(Failure::guard(
            "read_edited_comment",
            "response_value_unverified",
        ));
    }
    proof.comment_edit_read_back = true;
    Ok(())
}

async fn journey(
    client: &GitHubClient,
    token: SecretToken,
    journal: &Path,
    proof: &mut Proof,
) -> Result<(), Failure> {
    let cancel = CancellationToken::new();
    let owner = OwnerScope::new("joshmarkovic").expect("pinned owner");
    let session = client
        .authenticate(token, owner, NonZeroU64::new(USER), &cancel)
        .await
        .map_err(|e| Failure::read("verify_actor", e))?;
    let route = RepositoryName::new("joshmarkovic", "threadline").expect("pinned repository");
    let (writer, _) = session
        .issue_writer(&route, REPO_ID.try_into().unwrap(), &cancel)
        .await
        .map_err(|e| Failure::read("verify_repository", e))?;
    if !writer.is_private() {
        return Err(Failure::guard("verify_repository", "unexpected_visibility"));
    }
    let issue = current(&writer, &cancel, "verify_existing_issue").await?;
    if issue.author_id() != NonZeroU64::new(USER)
        || issue.observed.title.as_deref() != Some(ORIGINAL)
        || issue.observed.closed != Some(false)
    {
        return Err(Failure::guard(
            "verify_existing_issue",
            "disposable_issue_changed",
        ));
    }
    proof.identity_verified = true;
    if writer
        .has_ordinary_comments(&issue, &cancel)
        .await
        .map_err(|e| Failure::read("verify_no_existing_comment", e))?
    {
        return Err(Failure::guard(
            "verify_no_existing_comment",
            "disposable_issue_has_comments",
        ));
    }
    proof.disposable_issue_verified = true;
    if let Err(error) = arm(journal) {
        let reason = if error.kind() == io::ErrorKind::AlreadyExists {
            "previous_attempt_requires_review"
        } else {
            "journal_unavailable"
        };
        return Err(Failure::guard("arm_journal", reason));
    }
    proof.journal_armed = true;

    edit_title(&writer, &issue, UPDATED, &cancel, proof, "edit_title").await?;
    let comment_result = async {
        if proof.server_hold {
            return Err(Failure::guard("read_edited_title", "server_hold_respected"));
        }
        let changed = current(&writer, &cancel, "read_edited_title").await?;
        if changed.observed.title.as_deref() != Some(UPDATED) {
            return Err(Failure::guard(
                "read_edited_title",
                "response_value_unverified",
            ));
        }
        proof.title_read_back = true;
        comments(&writer, &changed, &cancel, proof).await
    }
    .await;
    let cleanup = async {
        if proof.server_hold {
            return Err(Failure::guard(
                "verify_title_before_restore",
                "server_hold_respected",
            ));
        }
        let latest = current(&writer, &cancel, "verify_title_before_restore").await?;
        if latest.observed.title.as_deref() != Some(UPDATED) {
            return Err(Failure::guard(
                "verify_title_before_restore",
                "changed_title_left_untouched",
            ));
        }
        edit_title(&writer, &latest, ORIGINAL, &cancel, proof, "restore_title").await?;
        let final_issue = current(&writer, &cancel, "read_restored_title").await?;
        if final_issue.observed.title.as_deref() != Some(ORIGINAL) {
            return Err(Failure::guard(
                "read_restored_title",
                "response_value_unverified",
            ));
        }
        proof.original_title_restored = true;
        Ok(())
    }
    .await;
    proof.cleanup_failure = cleanup.err();
    comment_result
}

fn receipt(status: &str, credential: &str, source: &str, proof: &Proof) -> serde_json::Value {
    json!({
        "scenario":CASE,"execution_profile":PROFILE,"repository":REPO,"github_user_id":USER,"github_repository_id":REPO_ID,
        "github_issue_id":ISSUE,"issue_number":NUMBER,"status":status,
        "credential_kind":credential,"credential_source":source,
        "dedicated_pat_coverage":if status=="Pass" && matches!(credential,"fine_grained_pat"|"classic_pat") {"Verified"} else {"Not tested"},
        "proof":proof,
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut proof = Proof::default();
    let options = match Options::parse(env::args().skip(1), SCOPE) {
        Ok(value) => value,
        Err(reason) => {
            proof.failure = Some(Failure::guard("authorize", reason));
            println!("{}", receipt("Blocked", "unverified", "unverified", &proof));
            return ExitCode::from(78);
        }
    };
    let source = if options.stdin {
        "stdin_pipe"
    } else {
        "dedicated_environment"
    };
    let (credential, token) = match options.credential() {
        Ok(value) => value,
        Err(reason) => {
            proof.failure = Some(Failure::guard("authenticate_input", reason));
            println!("{}", receipt("Blocked", "unverified", source, &proof));
            return ExitCode::from(78);
        }
    };
    if options.journal.join(ARMED).exists() || options.journal.join(COMPLETED).exists() {
        proof.failure = Some(Failure::guard(
            "arm_journal",
            "previous_attempt_requires_review",
        ));
    } else {
        proof.failure = match GitHubClient::production() {
            Ok(client) => journey(&client, token, &options.journal, &mut proof)
                .await
                .err(),
            Err(error) => Some(Failure::read("start_transport", error)),
        };
    }
    let mut status = if proof.failure.is_none() && proof.cleanup_failure.is_none() {
        "Pass"
    } else if !proof.journal_armed {
        "Blocked"
    } else {
        "Fail"
    };
    let mut report = receipt(status, credential, source, &proof);
    if proof.journal_armed && completed(&options.journal, &report).is_err() {
        status = "Fail";
        if proof.cleanup_failure.is_none() {
            proof.cleanup_failure = Some(Failure::guard("persist_receipt", "journal_unavailable"));
        }
        report = receipt(status, credential, source, &proof);
    }
    println!("{report}");
    ExitCode::from(match status {
        "Pass" => 0,
        "Blocked" => 78,
        _ => 1,
    })
}

#[cfg(test)]
#[allow(dead_code)]
#[path = "../tests/support/mod.rs"]
mod support;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderName, Method, StatusCode};
    use std::path::PathBuf;
    use support::{Reply, Simulator};

    #[test]
    fn accepts_only_pinned_actor_scope_and_explicit_private_oauth_pipe() {
        let args = |source| {
            [
                "--repo",
                REPO,
                "--expected-user-id",
                "52184130",
                "--journal-dir",
                "fixture",
                "--allow-live-writes",
                "--profile",
                PROFILE,
                source,
                "--allow-existing-cli-oauth",
            ]
            .into_iter()
            .map(str::to_owned)
        };
        assert!(Options::parse(args("--credential-stdin"), SCOPE).is_ok());
        assert!(Options::parse(args("--credential-env"), SCOPE).is_err());
        assert!(
            Options::parse(
                args("--credential-stdin").filter(|arg| arg != "--allow-live-writes"),
                SCOPE
            )
            .is_err()
        );
    }

    #[test]
    fn persisted_reservation_prevents_a_second_send_after_process_loss() {
        let fixture = scratch();
        arm(&fixture).unwrap();
        assert_eq!(
            arm(&fixture).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(!fixture.join(COMPLETED).exists());
        assert!(
            fs::read_to_string(fixture.join(ARMED))
                .unwrap()
                .contains("mutation_attempt_reserved")
        );
        fs::remove_dir_all(&fixture).unwrap();
    }

    fn scratch() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture = env::temp_dir().join(format!(
            "threadline-live-guard-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&fixture).unwrap();
        fixture
    }

    #[tokio::test]
    async fn loopback_ambiguous_comment_remains_unconfirmed_and_proven_title_is_restored() {
        const PATH: &str = "/repos/joshmarkovic/threadline/issues/1";
        const COMMENTS: &str = "/repos/joshmarkovic/threadline/issues/1/comments";
        let simulator = Simulator::start().await;
        let fixture = scratch();
        let reply = |status, value: serde_json::Value| {
            let mut response = Reply::new(status, value.to_string());
            response.authenticated = true;
            response.header(
                HeaderName::from_static("x-github-request-id"),
                "FIXTURE:01:02",
            )
        };
        simulator.enqueue(
            Method::GET,
            "/user",
            reply(StatusCode::OK, json!({"id":USER,"login":"fixture-user"})),
        );
        simulator.enqueue(Method::GET,"/repos/joshmarkovic/threadline",reply(StatusCode::OK,json!({
            "id":REPO_ID,"node_id":"R_FIXTURE","name":"threadline","owner":{"login":"joshmarkovic"},
            "private":true,"permissions":{"pull":true,"push":true,"admin":false}
        })));
        let issue = |title| {
            json!({
                "id":ISSUE,"number":NUMBER,"title":title,"state":"open","locked":false,
                "updated_at":"2023-01-01T00:00:00Z","user":{"id":USER,"login":"fixture-user"},
                "repository_url":"https://api.github.com/repos/joshmarkovic/threadline"
            })
        };
        simulator.enqueue(Method::GET, PATH, reply(StatusCode::OK, issue(ORIGINAL)));
        simulator.enqueue(
            Method::GET,
            &format!("{COMMENTS}?per_page=1&page=1"),
            reply(StatusCode::OK, json!([])),
        );
        simulator.enqueue(
            Method::PATCH,
            PATH,
            reply(StatusCode::OK, issue(UPDATED)).expects_json(json!({"title":UPDATED})),
        );
        simulator.enqueue(Method::GET, PATH, reply(StatusCode::OK, issue(UPDATED)));
        simulator.enqueue(
            Method::POST,
            COMMENTS,
            reply(
                StatusCode::BAD_GATEWAY,
                json!({"message":"Reply unavailable"}),
            )
            .expects_json(json!({"body":CREATED})),
        );
        simulator.enqueue(Method::GET, PATH, reply(StatusCode::OK, issue(UPDATED)));
        simulator.enqueue(
            Method::PATCH,
            PATH,
            reply(StatusCode::OK, issue(ORIGINAL)).expects_json(json!({"title":ORIGINAL})),
        );
        simulator.enqueue(Method::GET, PATH, reply(StatusCode::OK, issue(ORIGINAL)));
        let client = GitHubClient::loopback(simulator.address).unwrap();
        let mut proof = Proof::default();
        let error = journey(&client, SecretToken::fixture(), &fixture, &mut proof)
            .await
            .err()
            .unwrap();
        assert_eq!(
            (error.stage, error.outcome),
            ("create_comment", "outcome_unknown")
        );
        assert!(proof.original_title_restored && proof.comment_id.is_none());
        assert!(arm(&fixture).is_err());
        let requests = simulator.take_observations();
        assert_eq!(requests.len(), 10);
        assert!(
            requests
                .iter()
                .all(|row| row.matched && row.authorization_sent && row.versioned)
        );
        fs::remove_dir_all(&fixture).unwrap();
    }
}
