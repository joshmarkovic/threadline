//! Direct merge proof for the approved disposable pull request.
use std::{
    env,
    fs::File,
    io::Read,
    num::NonZeroU64,
    path::Path,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

use live_support::{Options, Scope, checkpoint};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use threadline_core::{
    domain::RepositoryName,
    github::{
        CancellationToken, GitHubClient, OwnerScope, RateLimit, SecretToken,
        writer::{
            DispatchEvidence, GitHubWriter,
            merge::{MergeEligibility, MergeMethod, QueueRequirement},
            review::VerifiedPull,
        },
    },
};

mod live_support;

const CASE: &str = "LIVE-004";
const REPO: &str = "joshmarkovic/threadline";
const PROFILE: &str = "threadline-disposable-pr-3";
const REPO_ID: u64 = 1_462_273_296;
const ACTOR: u64 = 52_184_130;
const NUMBER: u64 = 3;
const PR: u64 = 5_385_043_122;
const NODE: &str = "PR_kwDOVyiFEM8AAAABQPk8sg";
const BASE_REF: &str = "dogfood/windows-merge-base";
const HEAD_REF: &str = "dogfood/windows-merge-head";
const BASE: &str = "4f1e2087d726c7208f937fd6510ea346c5471198";
const HEAD: &str = "501a4528e98d07e413c6f56bac76a50a9f5d61b5";
const ARMED: &str = "live-004-pr-3-attempt.json";
const ACCEPTED: &str = "live-004-pr-3-accepted.json";
const COMPLETE: &str = "live-004-pr-3-confirmed.json";
const SCOPE: Scope = Scope {
    repository: REPO,
    profile: PROFILE,
    actor: ACTOR,
    allow_readback: true,
};

#[derive(Default, Serialize)]
struct Evidence {
    target_verified: bool,
    eligibility: Option<&'static str>,
    verified_optional_contexts: Option<usize>,
    readback_only: bool,
    dispatch: &'static str,
    merge_oid: Option<String>,
    request_id: Option<String>,
    provider_status: Option<u16>,
    read_back: bool,
    own_action: bool,
    reason: Option<&'static str>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Acceptance {
    pull_request_id: u64,
    expected_head: String,
    merge_oid: String,
    request_id: String,
}

fn valid_oid(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn valid_request(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'-' | b'_'))
}

fn recover(journal: &Path) -> Option<Acceptance> {
    let mut bytes = Vec::new();
    File::open(journal.join(ACCEPTED))
        .ok()?
        .take(1025)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 1024 {
        return None;
    }
    let accepted: Acceptance = serde_json::from_slice(&bytes).ok()?;
    (accepted.pull_request_id == PR
        && accepted.expected_head == HEAD
        && valid_oid(&accepted.merge_oid)
        && valid_request(&accepted.request_id))
    .then_some(accepted)
}

fn held(rate: RateLimit) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
        });
    rate.not_before_ms(now).is_some()
}

fn target_matches(pull: &VerifiedPull) -> bool {
    pull.id().get() == PR
        && pull.number().get() == NUMBER
        && pull.node_id() == NODE
        && pull.head() == HEAD
        && pull.base_ref() == Some(BASE_REF)
        && pull.head_ref() == Some(HEAD_REF)
        && pull.author_id() == NonZeroU64::new(ACTOR)
}

async fn observe(
    writer: &GitHubWriter<'_>,
    cancel: &CancellationToken,
) -> Result<VerifiedPull, &'static str> {
    let (pull, rate) = writer
        .observe_pull_for_review(
            NUMBER.try_into().unwrap(),
            NonZeroU64::new(PR).unwrap(),
            cancel,
        )
        .await
        .map_err(|_| "pull_read_failed")?;
    if held(rate) {
        return Err("provider_rate_hold");
    }
    if !target_matches(&pull) {
        return Err("target_changed");
    }
    Ok(pull)
}

fn eligibility(value: &MergeEligibility) -> &'static str {
    if value.allows(MergeMethod::Merge) {
        return "eligible";
    }
    match value.queue {
        QueueRequirement::Required => "merge_queue_required",
        QueueRequirement::Unknown => "merge_queue_unknown",
        QueueRequirement::NotRequired => "direct_merge_policy_held",
    }
}

async fn run(
    client: &GitHubClient,
    token: SecretToken,
    journal: &Path,
    evidence: &mut Evidence,
) -> Result<(), &'static str> {
    if journal.join(ARMED).exists() != evidence.readback_only || journal.join(COMPLETE).exists() {
        return Err("journal_unavailable_or_preexisting");
    }
    let recovered = if evidence.readback_only {
        Some(recover(journal).ok_or("journal_unavailable_or_preexisting")?)
    } else {
        None
    };
    let cancel = CancellationToken::new();
    let session = client
        .authenticate(
            token,
            OwnerScope::new("joshmarkovic").unwrap(),
            NonZeroU64::new(ACTOR),
            &cancel,
        )
        .await
        .map_err(|_| "authenticate_failed")?;
    let repo = RepositoryName::new("joshmarkovic", "threadline").unwrap();
    let (writer, rate) = session
        .issue_writer(&repo, REPO_ID.try_into().unwrap(), &cancel)
        .await
        .map_err(|_| "repository_read_failed")?;
    if !writer.is_private() {
        return Err("target_changed");
    }
    if held(rate) {
        return Err("provider_rate_hold");
    }

    let accepted = if let Some(accepted) = recovered {
        accepted
    } else {
        let pull = observe(&writer, &cancel).await?;
        if !pull.is_open()
            || pull.is_merged()
            || pull.base() != BASE
            || pull.head_repository() != Some(writer.repository())
        {
            return Err("target_changed");
        }
        evidence.target_verified = true;
        let (mut policy, rate) = writer
            .observe_merge_policy(&pull, &cancel)
            .await
            .map_err(|_| "merge_policy_read_failed")?;
        if held(rate) {
            return Err("provider_rate_hold");
        }
        if policy.needs_optional_checks() {
            let (proof, rate) = writer
                .observe_optional_merge_checks(&pull, &policy, &cancel)
                .await
                .map_err(|_| "optional_checks_read_failed")?;
            if held(rate) {
                return Err("provider_rate_hold");
            }
            if let Some(proof) = proof {
                let checked = proof.contexts();
                let rate = writer
                    .confirm_optional_merge_checks(&pull, &mut policy, proof, &cancel)
                    .await
                    .map_err(|_| "branch_protection_read_failed")?;
                if held(rate) {
                    return Err("provider_rate_hold");
                }
                if !policy.needs_optional_checks() {
                    evidence.verified_optional_contexts = Some(checked);
                }
            }
        }
        let decision = eligibility(policy.eligibility());
        evidence.eligibility = Some(decision);
        if decision != "eligible" {
            return Err(decision);
        }
        checkpoint(
            journal,
            ARMED,
            &json!({"scenario":CASE,"pull_request_id":PR,"expected_head":HEAD,"method":"merge"}),
        )
        .map_err(|_| "journal_unavailable_or_preexisting")?;

        let response = match writer
            .merge_at_head(&pull, &policy, MergeMethod::Merge, HEAD, &cancel)
            .await
        {
            Ok(receipt) => receipt,
            Err(error) => {
                evidence.provider_status = error.transport.access.http_status;
                evidence.dispatch = match error.transport.evidence {
                    DispatchEvidence::DefinitelyUnsent => "definitely_unsent",
                    DispatchEvidence::AccessRejected => "access_rejected",
                    DispatchEvidence::InvalidInput => "invalid_input",
                    DispatchEvidence::Throttled => "throttled",
                    DispatchEvidence::Ambiguous => "unknown",
                };
                return Err("merge_not_confirmed");
            }
        };
        evidence.dispatch = "acknowledged";
        evidence.provider_status = Some(200);
        evidence.request_id = Some(response.request_id.clone());
        evidence.merge_oid = Some(response.merge_oid.clone());
        let accepted = Acceptance {
            pull_request_id: PR,
            expected_head: HEAD.into(),
            merge_oid: response.merge_oid,
            request_id: response.request_id,
        };
        checkpoint(journal, ACCEPTED, &accepted)
            .map_err(|_| "journal_unavailable_or_preexisting")?;
        if held(response.rate) {
            return Err("provider_rate_hold");
        }
        accepted
    };

    evidence.request_id = Some(accepted.request_id);
    evidence.merge_oid = Some(accepted.merge_oid.clone());
    let merged = observe(&writer, &cancel).await?;
    evidence.target_verified = true;
    if !merged.is_merged()
        || merged.is_open()
        || merged.merge_oid() != Some(&accepted.merge_oid)
        || merged.merged_by() != NonZeroU64::new(ACTOR)
    {
        return Err("merge_readback_not_confirmed");
    }
    evidence.read_back = true;
    evidence.own_action = true;
    checkpoint(
        journal,
        COMPLETE,
        &json!({"scenario":CASE,"pull_request_id":PR,
        "expected_head":HEAD,"merge_oid":evidence.merge_oid,"actor":ACTOR}),
    )
    .map_err(|_| "journal_unavailable_or_preexisting")?;
    Ok(())
}

fn receipt(status: &str, kind: &str, source: &str, evidence: Evidence) -> Value {
    json!({"scenario":CASE,"status":status,"repository":REPO,"profile":PROFILE,
        "provider_repo_id":REPO_ID,"provider_user_id":ACTOR,"provider_pull_id":PR,"pull_number":NUMBER,
        "base_ref":BASE_REF,"head_ref":HEAD_REF,"base":BASE,"head":HEAD,"method":"merge",
        "credential_kind":kind,"credential_source":source,"evidence":evidence})
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut evidence = Evidence {
        dispatch: "not_sent",
        ..Evidence::default()
    };
    let options = Options::parse(env::args().skip(1), SCOPE);
    let Ok(options) = options else {
        evidence.reason = Some("missing_scope_or_explicit_mode");
        println!(
            "{}",
            receipt("Blocked", "unverified", "unverified", evidence)
        );
        return ExitCode::from(78);
    };
    evidence.readback_only = options.readback_only;
    let source = if options.stdin {
        "stdin_pipe"
    } else {
        "dedicated_environment"
    };
    let Ok((kind, token)) = options.credential() else {
        evidence.reason = Some("credential_unavailable");
        println!("{}", receipt("Blocked", "unverified", source, evidence));
        return ExitCode::from(78);
    };
    let result = match GitHubClient::production() {
        Ok(client) => run(&client, token, &options.journal, &mut evidence).await,
        Err(_) => Err("transport_unavailable"),
    };
    evidence.reason = result.err();
    let (status, code) = if evidence.reason.is_none() {
        ("Pass", 0)
    } else if evidence.dispatch != "not_sent"
        || (options.readback_only && evidence.merge_oid.is_some())
    {
        ("Fail", 1)
    } else {
        ("Blocked", 78)
    };
    println!("{}", receipt(status, kind, source, evidence));
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

    fn reply(body: Value) -> Reply {
        let mut reply = Reply::new(StatusCode::OK, body.to_string());
        reply.authenticated = true;
        reply.header(
            HeaderName::from_static("x-github-request-id"),
            "FIXTURE:S09:MERGE",
        )
    }

    #[tokio::test]
    async fn merges_only_complete_optional_checks_on_the_exact_unprotected_branch() {
        const NUMERIC_PR: &str = "/repositories/1462273296/pulls/3";
        const BRANCH_PATH: &str =
            "/repos/joshmarkovic/threadline/branches/dogfood%2Fwindows-merge-base";
        const PUT_PATH: &str = "/repos/joshmarkovic/threadline/pulls/3/merge";
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let root = env::temp_dir().join(format!(
            "threadline-live-004-{}-{}",
            std::process::id(),
            now.as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        for (reason, required, incomplete, protected) in [
            (None, false, false, false),
            (Some("direct_merge_policy_held"), true, false, false),
            (Some("direct_merge_policy_held"), false, true, false),
            (Some("direct_merge_policy_held"), false, false, true),
        ] {
            let journal = root.join(match (required, incomplete, protected) {
                (true, _, _) => "required",
                (_, true, _) => "incomplete",
                (_, _, true) => "protected",
                _ => "complete",
            });
            let sim = Simulator::start().await;
            let client = GitHubClient::loopback(sim.address).unwrap();
            let mut evidence = Evidence {
                dispatch: "not_sent",
                readback_only: true,
                ..Evidence::default()
            };
            assert_eq!(
                run(&client, SecretToken::fixture(), &journal, &mut evidence).await,
                Err("journal_unavailable_or_preexisting")
            );
            assert!(sim.take_observations().is_empty());
            sim.enqueue(
                Method::GET,
                "/user",
                reply(json!({"id":ACTOR,"login":"joshmarkovic"})),
            );
            sim.enqueue(Method::GET, "/repos/joshmarkovic/threadline", reply(json!({
                "id":REPO_ID,"node_id":"fixture-repo","owner":{"login":"joshmarkovic"},"name":"threadline",
                "private":true,"permissions":{"pull":true,"push":true,"admin":true}
            })));
            let pull = json!({
                "id":PR,"node_id":NODE,"number":NUMBER,"title":"Synthetic merge","state":"open",
                "updated_at":"2020-02-02T04:05:06Z","merged":false,"draft":false,"user":{"id":ACTOR,"login":"joshmarkovic"},
                "base":{"sha":BASE,"ref":BASE_REF,"repo":{"id":REPO_ID,"node_id":"fixture-repo"}},
                "head":{"sha":HEAD,"ref":HEAD_REF,"repo":{"id":REPO_ID,"node_id":"fixture-repo"}}
            });
            sim.enqueue(Method::GET, NUMERIC_PR, reply(pull.clone()));
            let policy = json!({"id":NODE,"number":NUMBER,"baseRefName":BASE_REF,"baseRefOid":BASE,"headRefOid":HEAD,
                "state":"OPEN","isDraft":false,"merged":false,"mergeable":"MERGEABLE","mergeStateStatus":"UNSTABLE",
                "reviewDecision":null,"mergeQueue":null,"repository":{"id":"fixture-repo","viewerPermission":"ADMIN",
                "mergeCommitAllowed":true,"squashMergeAllowed":true,"rebaseMergeAllowed":true}});
            sim.enqueue(
                Method::POST,
                "/graphql",
                reply(json!({"data":{"node":policy}}))
                    .graph_node_without_cursor("ThreadlineMergePolicy", NODE),
            );
            let mut optional = policy;
            optional["isMergeQueueEnabled"] = false.into();
            optional["baseRef"] =
                json!({"name":BASE_REF,"target":{"oid":BASE},"branchProtectionRule":null});
            optional["commits"] = json!({"nodes":[{"commit":{"oid":HEAD,"statusCheckRollup":{"contexts":{
                "totalCount":2,"pageInfo":{"hasNextPage":incomplete},
                "nodes":[{"__typename":"CheckRun","isRequired":required},{"__typename":"StatusContext","isRequired":false}]
            }}}}]});
            sim.enqueue(
                Method::POST,
                "/graphql",
                reply(json!({"data":{"node":optional}}))
                    .graph_node_without_cursor("ThreadlineOptionalMergeChecks", NODE),
            );
            if !required && !incomplete {
                sim.enqueue(Method::GET, BRANCH_PATH, reply(json!({"name":BASE_REF,"commit":{"sha":BASE},
                    "protected":protected,"protection":{"required_status_checks":{"enforcement_level":"off","contexts":[],"checks":[]}}})));
            }
            let expected_merge = "b".repeat(40);
            if reason.is_none() {
                sim.enqueue(
                    Method::PUT,
                    PUT_PATH,
                    reply(json!({"merged":true,"sha":expected_merge}))
                        .expects_json(json!({"sha":HEAD,"merge_method":"merge"})),
                );
                let mut merged = pull;
                merged["merged"] = true.into();
                merged["state"] = "closed".into();
                merged["merge_commit_sha"] = expected_merge.clone().into();
                merged["merged_by"] = json!({"id":ACTOR,"login":"joshmarkovic"});
                sim.enqueue(Method::GET, NUMERIC_PR, reply(merged));
            }
            evidence.readback_only = false;
            let actual = run(&client, SecretToken::fixture(), &journal, &mut evidence).await;
            if let Some(reason) = reason {
                assert_eq!(actual, Err(reason));
                assert!(!journal.join(ARMED).exists());
            } else {
                assert_eq!(actual, Ok(()));
                assert!(evidence.own_action && evidence.read_back);
                assert_eq!(evidence.verified_optional_contexts, Some(2));
                assert_eq!(evidence.merge_oid.as_deref(), Some(expected_merge.as_str()));
                assert!(journal.join(COMPLETE).is_file());
                assert!(
                    run(&client, SecretToken::fixture(), &journal, &mut evidence)
                        .await
                        .is_err()
                );
            }
            let observations = sim.take_observations();
            let expected = if reason.is_none() {
                8
            } else if protected {
                6
            } else {
                5
            };
            assert_eq!(observations.len(), expected);
            assert!(
                observations
                    .iter()
                    .enumerate()
                    .all(|(index, row)| row.matched
                        && row.authorization_sent
                        && (matches!(index, 3 | 4) || row.versioned)),
                "{observations:?}"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
