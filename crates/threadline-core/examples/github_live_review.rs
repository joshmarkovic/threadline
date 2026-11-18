//! One approved comment-only review on the disposable pull request.
use std::{
    env,
    fs::File,
    io::Read,
    num::NonZeroU64,
    path::Path,
    process::ExitCode,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use live_support::{Options, Scope, checkpoint};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use threadline_core::{
    domain::RepositoryName,
    github::{
        AccessError, CancellationToken, GitHubClient, OwnerScope, RateLimit, SecretToken,
        reader::{
            pr::{PatchEvidence, PrSide, PrSource, PrSourceRange},
            refresh::ReadValidators,
        },
        writer::{
            DispatchEvidence, GitHubWriter, MutationFailure, WritePermission,
            review::{
                InlineReview, ObservedReview, PendingReview, ReviewEvent, ReviewMutation,
                ReviewObject, ReviewSide, ReviewState, VerifiedPull,
            },
        },
    },
};

mod live_support;

const CASE: &str = "LIVE-003";
const PROFILE: &str = "threadline-disposable-pr-2";
const REPOSITORY: &str = "joshmarkovic/threadline";
const REPO_ID: u64 = 1_462_273_296;
const ACTOR: u64 = 52_184_130;
const PR: u64 = 5_384_062_919;
const NUMBER: u64 = 2;
const HEAD: &str = "66864f02bdcad8df554d6c24442dccb401f87df4";
const BLOB: &str = "06bb43c6dd5b93145dad99f6e3f2cc9b8bc20183";
const BRANCH: &str = "dogfood/windows-review-base";
const FILE: &str = "docs/fixtures/review-checklist.md";
const LINE: u32 = 5;
const BODY: &str = "Threadline LIVE-003 checked this line in the pinned review fixture.";
const SCOPE: Scope = Scope {
    repository: REPOSITORY,
    actor: ACTOR,
    profile: PROFILE,
};
const ARMED: &str = "live-003-pr-2-attempt.json";
const PENDING: &str = "live-003-pr-2-pending.json";
const SUBMIT: &str = "live-003-pr-2-submit.json";
const COMPLETE: &str = "live-003-pr-2-confirmed.json";

#[derive(Default, Serialize)]
struct Evidence {
    scope_verified: bool,
    source_verified: bool,
    review_id: Option<u64>,
    pending_http_status: Option<u16>,
    pending_request_id: Option<String>,
    submit_http_status: Option<u16>,
    submit_request_id: Option<String>,
    readback_only: bool,
    reconciled_unknown_submit: bool,
    review_read_back: bool,
    inline_id: Option<u64>,
    failure: Option<Failure>,
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
    fn provider(stage: &'static str, outcome: &'static str, error: &AccessError) -> Self {
        Self {
            stage,
            outcome,
            kind: Some(format!("{:?}", error.kind)),
            http_status: error.http_status,
        }
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingReceipt {
    review_id: NonZeroU64,
    request_id: String,
    actor: u64,
    pr: u64,
    head: String,
}

fn no_receipt(stage: &'static str) -> Failure {
    Failure::guard(stage, "journal_unavailable_or_preexisting")
}

fn review_mutation_error(stage: &'static str, error: &MutationFailure) -> Failure {
    let outcome = match error.evidence {
        DispatchEvidence::DefinitelyUnsent => "definitely_unsent",
        DispatchEvidence::AccessRejected => "access_rejected",
        DispatchEvidence::InvalidInput => "invalid_input",
        DispatchEvidence::Throttled => "throttled",
        DispatchEvidence::Ambiguous => "outcome_unknown",
    };
    Failure::provider(stage, outcome, &error.access)
}

fn hold(rate: RateLimit) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    rate.not_before_ms(now).is_some()
}

fn read_pending(path: &Path) -> Option<PendingReceipt> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(1025)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 1024 {
        return None;
    }
    let record: PendingReceipt = serde_json::from_slice(&bytes).ok()?;
    (record.actor == ACTOR
        && record.pr == PR
        && record.head == HEAD
        && !record.request_id.is_empty()
        && record.request_id.len() <= 160
        && record
            .request_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'-' | b'_')))
    .then_some(record)
}

async fn do_review(
    client: &GitHubClient,
    token: SecretToken,
    journal: &Path,
    evidence: &mut Evidence,
) -> Result<(), Failure> {
    let reading = journal.join(ARMED).exists();
    let recovery = if reading {
        let record = read_pending(&journal.join(PENDING)).ok_or_else(|| no_receipt("recover"))?;
        if journal.join(COMPLETE).exists()
            || read_pending(&journal.join(SUBMIT)).as_ref() != Some(&record)
        {
            return Err(no_receipt("recover"));
        }
        Some(record)
    } else {
        None
    };
    evidence.readback_only = reading;
    let guard = CancellationToken::new();
    let owner = OwnerScope::new("joshmarkovic").expect("pinned owner");
    let session = client
        .authenticate(token, owner, NonZeroU64::new(ACTOR), &guard)
        .await
        .map_err(|error| Failure::provider("authenticate", "read_failed", &error))?;
    let name = RepositoryName::new("joshmarkovic", "threadline").expect("pinned repository");
    let (writer, rate) = session
        .issue_writer(&name, REPO_ID.try_into().unwrap(), &guard)
        .await
        .map_err(|error| Failure::provider("repository", "read_failed", &error))?;
    if !writer.is_private() || hold(rate) {
        return Err(Failure::guard("repository", "unverified_or_rate_held"));
    }
    let number = NUMBER.try_into().unwrap();
    let pr_id = NonZeroU64::new(PR).unwrap();
    let reader = writer.reader();
    let snapshot = reader
        .pr_snapshot(number, &ReadValidators::default(), &guard)
        .await
        .map_err(|error| Failure::provider("pull_request", "read_failed", &error))?;
    if hold(snapshot.rate) {
        return Err(Failure::guard("pull_request", "unverified_or_rate_held"));
    }
    let snapshot = snapshot
        .value
        .ok_or_else(|| Failure::guard("pull_request", "target_changed"))?;
    let details = snapshot.details();
    if details.pull_request_id != pr_id
        || details.head_oid != HEAD
        || details.presentation.base_ref.as_deref() != Some(BRANCH)
        || details.presentation.author_id != NonZeroU64::new(ACTOR)
        || details.closed
        || details.merged
    {
        return Err(Failure::guard("pull_request", "target_changed"));
    }
    let comparison = reader
        .pr_comparison(&snapshot, 1, &ReadValidators::default(), &guard)
        .await
        .map_err(|error| Failure::provider("comparison", "read_failed", &error))?;
    if hold(comparison.rate) {
        return Err(Failure::guard("comparison", "unverified_or_rate_held"));
    }
    let comparison = comparison
        .value
        .ok_or_else(|| Failure::guard("comparison", "target_changed"))?;
    let changed = comparison
        .files
        .as_ref()
        .filter(|files| !files.partial)
        .and_then(|files| files.records.iter().find(|file| file.path == FILE));
    if !changed.is_some_and(|file| {
        file.status == "added"
            && file.additions.is_some_and(|n| n >= u64::from(LINE))
            && matches!(file.patch, PatchEvidence::Text(_))
    }) {
        return Err(Failure::guard("comparison", "target_changed"));
    }
    let source = reader
        .pr_source_context(
            &snapshot,
            comparison.identity(),
            PrSourceRange {
                side: PrSide::Right,
                path: FILE,
                start: LINE,
                lines: 1,
            },
            &guard,
        )
        .await
        .map_err(|error| Failure::provider("source", "read_failed", &error))?;
    if hold(source.rate)
        || !matches!(source.value,Some(PrSource::Text {oid,blob_oid,path,first_line,..})
        if oid==HEAD && blob_oid==BLOB && path==FILE && first_line==LINE)
    {
        return Err(Failure::guard("source", "target_changed"));
    }
    evidence.source_verified = true;
    let (pull, rate) = writer
        .observe_pull_for_review(number, pr_id, &guard)
        .await
        .map_err(|error| Failure::provider("review_target", "read_failed", &error))?;
    if hold(rate)
        || pull.head() != HEAD
        || pull.base() != details.base_oid
        || pull.author_id() != Some(writer.actor())
        || writer.review_permission(&pull, Some(ReviewEvent::Comment)) != WritePermission::Allowed
    {
        return Err(Failure::guard("review_target", "target_changed"));
    }
    evidence.scope_verified = true;

    let record = if let Some(record) = recovery {
        evidence.pending_http_status = Some(200);
        evidence.pending_request_id = Some(record.request_id.clone());
        evidence.reconciled_unknown_submit = true;
        record
    } else {
        let (has_reviews, rate) = writer
            .has_reviews(&pull, &guard)
            .await
            .map_err(|error| Failure::provider("existing_reviews", "read_failed", &error))?;
        if hold(rate) {
            return Err(Failure::guard(
                "existing_reviews",
                "unverified_or_rate_held",
            ));
        }
        if has_reviews {
            return Err(Failure::guard(
                "existing_reviews",
                "previous_remote_review_exists",
            ));
        }
        let ((pending, next), rate) = writer
            .observe_own_pending(&pull, 1.try_into().unwrap(), &guard)
            .await
            .map_err(|error| Failure::provider("pending_preflight", "read_failed", &error))?;
        if hold(rate) || next.is_some() || !matches!(pending, PendingReview::Absent) {
            return Err(Failure::guard(
                "pending_preflight",
                "unverified_or_rate_held",
            ));
        }
        checkpoint(
            journal,
            ARMED,
            &json!({"scenario":CASE,"pr":PR,"head":HEAD,"blob":BLOB,"line":LINE}),
        )
        .map_err(|_| no_receipt("create_journal"))?;
        let comments = [InlineReview {
            path: FILE,
            body: BODY,
            side: ReviewSide::Right,
            line: LINE,
            start_line: None,
            start_side: None,
        }];
        let result = writer
            .mutate_review(
                &pull,
                ReviewMutation::CreatePending {
                    pending: &pending,
                    summary: "",
                    comments: &comments,
                },
                &guard,
            )
            .await
            .map_err(|error| review_mutation_error("create_review", &error))?;
        let ReviewObject::Review(created) = result.object else {
            return Err(Failure::guard("create_review", "response_mismatch"));
        };
        evidence.pending_http_status = Some(200);
        evidence.pending_request_id = Some(result.request_id.clone());
        evidence.review_id = Some(created.id().get());
        let record = PendingReceipt {
            review_id: created.id(),
            request_id: result.request_id,
            actor: ACTOR,
            pr: PR,
            head: HEAD.to_owned(),
        };
        checkpoint(journal, PENDING, &record).map_err(|_| no_receipt("pending_journal"))?;
        if hold(result.rate) {
            return Err(Failure::guard("verify_pending", "unverified_or_rate_held"));
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        let (fresh, rate) = writer
            .observe_pull_for_review(number, pr_id, &guard)
            .await
            .map_err(|error| Failure::provider("verify_pending", "read_failed", &error))?;
        if hold(rate) || fresh.head() != HEAD || fresh.base() != pull.base() || !fresh.is_open() {
            return Err(Failure::guard("verify_pending", "target_changed"));
        }
        let (verified, rate) = writer
            .observe_review_object(&fresh, record.review_id, &guard)
            .await
            .map_err(|error| Failure::provider("verify_pending", "read_failed", &error))?;
        if hold(rate) || !owns(&verified, record.review_id, ReviewState::Pending) {
            return Err(Failure::guard("verify_pending", "response_mismatch"));
        }
        checkpoint(journal, SUBMIT, &record).map_err(|_| no_receipt("submit_journal"))?;
        let result = writer
            .mutate_review(
                &fresh,
                ReviewMutation::SubmitPending {
                    review: &verified,
                    event: ReviewEvent::Comment,
                    body: "",
                },
                &guard,
            )
            .await;
        match result {
            Ok(response) => {
                evidence.submit_http_status = Some(200);
                evidence.submit_request_id = Some(response.request_id);
                if hold(response.rate) {
                    return Err(Failure::guard("review_readback", "unverified_or_rate_held"));
                }
            }
            Err(error)
                if error.evidence == DispatchEvidence::Ambiguous
                    && !error.access.rate.is_some_and(hold) =>
            {
                evidence.reconciled_unknown_submit = true;
                evidence.submit_http_status = error.access.http_status;
            }
            Err(error) => return Err(review_mutation_error("submit_review", &error)),
        }
        record
    };

    evidence.review_id = Some(record.review_id.get());
    readback(&writer, &pull, record.review_id, evidence, &guard).await?;
    checkpoint(
        journal,
        COMPLETE,
        &json!({
            "scenario":CASE,"review_id":record.review_id,"inline_id":evidence.inline_id,
            "head":HEAD,"blob":BLOB,"event":"COMMENT",
        }),
    )
    .map_err(|_| no_receipt("complete_journal"))?;
    Ok(())
}

fn owns(review: &ObservedReview, id: NonZeroU64, state: ReviewState) -> bool {
    review.author().get() == ACTOR
        && review.id() == id
        && review.state() == state
        && review.head() == HEAD
}

async fn readback(
    writer: &GitHubWriter<'_>,
    pull: &VerifiedPull,
    id: NonZeroU64,
    evidence: &mut Evidence,
    cancel: &CancellationToken,
) -> Result<(), Failure> {
    let (review, rate) = writer
        .observe_review_object(pull, id, cancel)
        .await
        .map_err(|error| Failure::provider("review_readback", "read_failed", &error))?;
    if hold(rate) || !owns(&review, id, ReviewState::Submitted(ReviewEvent::Comment)) {
        return Err(Failure::guard("review_readback", "response_mismatch"));
    }
    evidence.review_read_back = true;
    let comments = writer
        .review_lines(pull, &review, cancel)
        .await
        .map_err(|error| Failure::provider("inline_readback", "read_failed", &error))?;
    let [comment] = comments.as_slice() else {
        return Err(Failure::guard("inline_readback", "response_mismatch"));
    };
    if comment.author().get() != ACTOR
        || comment.commit_id != HEAD
        || comment.path != FILE
        || comment.side.as_deref() != Some("RIGHT")
        || comment.line != Some(LINE)
        || comment.body != BODY
    {
        return Err(Failure::guard("inline_readback", "response_mismatch"));
    }
    evidence.inline_id = Some(comment.id.get());
    Ok(())
}

fn receipt(status: &str, credential: &str, source: &str, evidence: &Evidence) -> Value {
    json!({
        "scenario":CASE,"execution_profile":PROFILE,"repository":REPOSITORY,"github_user_id":ACTOR,
        "github_repository_id":REPO_ID,"github_pull_request_id":PR,"pull_request_number":NUMBER,
        "head":HEAD,"blob":BLOB,"path":FILE,"side":"RIGHT","line":LINE,"event":"COMMENT",
        "approval_verification":"Not tested","request_changes_verification":"Not tested",
        "credential_kind":credential,"credential_source":source,"status":status,"evidence":evidence,
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut evidence = Evidence::default();
    let options = match Options::parse(env::args().skip(1), SCOPE) {
        Ok(options) => options,
        Err(reason) => {
            evidence.failure = Some(Failure::guard("authorize", reason));
            println!(
                "{}",
                receipt("Blocked", "unverified", "unverified", &evidence)
            );
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
            evidence.failure = Some(Failure::guard("credential", reason));
            println!("{}", receipt("Blocked", "unverified", source, &evidence));
            return ExitCode::from(78);
        }
    };
    let result = match GitHubClient::production() {
        Ok(client) => do_review(&client, token, &options.journal, &mut evidence).await,
        Err(error) => Err(Failure::provider("transport", "read_failed", &error)),
    };
    evidence.failure = result.err();
    let (status, code) = if evidence.failure.is_none() {
        ("Pass", 0)
    } else if options.journal.join(ARMED).exists() && evidence.scope_verified {
        ("Fail", 1)
    } else {
        ("Blocked", 78)
    };
    println!("{}", receipt(status, credential, source, &evidence));
    ExitCode::from(code)
}

#[cfg(test)]
#[path = "../tests/support/mod.rs"]
#[allow(dead_code)]
mod support;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderName, Method, StatusCode};
    use std::fs;
    use support::{Reply, Simulator};

    fn reply(status: StatusCode, body: Value) -> Reply {
        let mut reply = Reply::new(status, body.to_string());
        reply.authenticated = true;
        reply.header(
            HeaderName::from_static("x-github-request-id"),
            "FIXTURE:91:LIVE",
        )
    }

    #[tokio::test]
    async fn one_anchored_review_reconciles_a_lost_response_without_reposting() {
        const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        const MERGE_BASE: &str = "cccccccccccccccccccccccccccccccccccccccc";
        const NUMERIC_PR: &str = "/repositories/1462273296/pulls/2";
        const REVIEWS: &str = "/repos/joshmarkovic/threadline/pulls/2/reviews";
        const KNOWN: &str = "/repos/joshmarkovic/threadline/pulls/2/reviews/91";
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let journal = env::temp_dir().join(format!(
            "threadline-live-003-{}-{}",
            std::process::id(),
            now.as_nanos()
        ));
        fs::create_dir(&journal).unwrap();
        let sim = Simulator::start().await;
        let client = GitHubClient::loopback(sim.address).unwrap();
        sim.enqueue(
            Method::GET,
            "/user",
            reply(
                StatusCode::OK,
                json!({
                    "id":ACTOR,"login":"joshmarkovic",
                }),
            ),
        );
        sim.enqueue(Method::GET, "/repos/joshmarkovic/threadline", reply(StatusCode::OK, json!({
            "id":REPO_ID,"node_id":"fixture-repository","owner":{"login":"joshmarkovic"},
            "name":"threadline","private":true,"permissions":{"pull":true,"push":true,"admin":false},
        })));
        let pull = json!({
            "id":PR,"node_id":"fixture-pr","number":NUMBER,"title":"Synthetic review fixture",
            "state":"open","draft":true,"merged":false,"locked":false,
            "updated_at":"2020-02-02T04:05:06Z","user":{"id":ACTOR,"login":"joshmarkovic"},
            "base":{"sha":BASE,"ref":BRANCH,"repo":{"id":REPO_ID,"node_id":"fixture-repository"}},
            "head":{"sha":HEAD,"ref":"fixture-head"},
        });
        for _ in 0..3 {
            sim.enqueue(Method::GET, NUMERIC_PR, reply(StatusCode::OK, pull.clone()));
        }
        sim.enqueue(Method::GET, &format!("/repositories/{REPO_ID}/compare/{BASE}...{HEAD}?per_page=20&page=1"),
            reply(StatusCode::OK, json!({
                "base_commit":{"sha":BASE},"merge_base_commit":{"sha":MERGE_BASE},"total_commits":1,
                "commits":[{"sha":HEAD,"commit":{"message":"Fixture"}}],
                "files":[{"filename":FILE,"status":"added","additions":5,"deletions":0,
                    "patch":"@@ -0,0 +1,5 @@\n+a\n+b\n+c\n+d\n+e"}],
            })));
        sim.enqueue(
            Method::GET,
            &format!("/repositories/{REPO_ID}/contents/{FILE}?ref={HEAD}"),
            reply(
                StatusCode::OK,
                json!({
                    "path":FILE,"sha":BLOB,"type":"file","size":10,
                    "encoding":"base64","content":"YQpiCmMKZAplCg==",
                }),
            ),
        );
        for limit in [1, 100] {
            sim.enqueue(
                Method::GET,
                &format!("{REVIEWS}?per_page={limit}&page=1"),
                reply(StatusCode::OK, json!([])),
            );
        }
        let pr_url = "https://api.github.com/repos/joshmarkovic/threadline/pulls/2";
        let review = |state: &str| {
            json!({
                "id":91,"user":{"id":ACTOR,"login":"joshmarkovic"},"state":state,"commit_id":HEAD,
                "pull_request_url":pr_url,
            })
        };
        sim.enqueue(Method::POST, REVIEWS, reply(StatusCode::OK, review("PENDING")).expects_json(json!({
            "commit_id":HEAD,"body":"","comments":[{"path":FILE,"body":BODY,"side":"RIGHT","line":LINE}],
        })));
        sim.enqueue(Method::GET, KNOWN, reply(StatusCode::OK, review("PENDING")));
        sim.enqueue(
            Method::POST,
            &format!("{KNOWN}/events"),
            reply(StatusCode::BAD_GATEWAY, Value::Null)
                .expects_json(json!({"event":"COMMENT","body":""})),
        );
        sim.enqueue(
            Method::GET,
            KNOWN,
            reply(StatusCode::OK, review("COMMENTED")),
        );
        sim.enqueue(
            Method::GET,
            &format!("{KNOWN}/comments?per_page=2&page=1"),
            reply(
                StatusCode::OK,
                json!([{
                    "id":92,"pull_request_review_id":91,"user":{"id":ACTOR,"login":"joshmarkovic"},
                    "path":FILE,"side":"RIGHT","line":LINE,"body":BODY,"commit_id":HEAD,
                    "pull_request_url":pr_url,
                }]),
            ),
        );
        let mut evidence = Evidence::default();
        let outcome = do_review(&client, SecretToken::fixture(), &journal, &mut evidence).await;
        assert!(
            outcome.is_ok(),
            "{}",
            serde_json::to_string(&outcome.err()).unwrap()
        );
        assert_eq!(evidence.review_id, Some(91));
        assert_eq!(evidence.inline_id, Some(92));
        assert!(evidence.scope_verified && evidence.source_verified && evidence.review_read_back);
        assert!(evidence.reconciled_unknown_submit);
        assert_eq!(evidence.submit_http_status, Some(502));
        assert!(journal.join(COMPLETE).is_file());
        assert!(
            do_review(
                &client,
                SecretToken::fixture(),
                &journal,
                &mut Evidence::default()
            )
            .await
            .is_err()
        );
        fs::remove_file(journal.join(COMPLETE)).unwrap();
        fs::write(journal.join(SUBMIT), b"incomplete").unwrap();
        assert!(
            do_review(
                &client,
                SecretToken::fixture(),
                &journal,
                &mut Evidence::default()
            )
            .await
            .is_err()
        );
        let observations = sim.take_observations();
        assert_eq!(observations.len(), 14);
        assert!(observations.iter().all(|row| row.matched && row.versioned));
        fs::remove_dir_all(journal).unwrap();
    }
}
