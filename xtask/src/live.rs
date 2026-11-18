use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::{
    cli::Options,
    report::{CaseResult, Status},
};
use serde_json::Value;

const REPOSITORY: &str = "joshmarkovic/threadline";
const EXPECTED_USER_ID: u64 = 52_184_130;
const EXPECTED_REPOSITORY_ID: u64 = 1_462_273_296;
const CASE: &str = "LIVE-001";
const EVIDENCE: &str = "github-read.json";

fn authorize(
    options: &Options,
    authorized_repo: Option<&str>,
    authorized_user: Option<u64>,
    credential_provided: bool,
) -> Result<u64, &'static str> {
    if options.allow_live_writes || options.case.as_deref() != Some(CASE) {
        return Err("Select the explicitly supported read-only LIVE-001 case.");
    }
    if options.allow_existing_cli_oauth && !options.credential_stdin {
        return Err("CLI OAuth dogfood requires an explicit standard-input pipe.");
    }
    if options.repo.as_deref() != Some(REPOSITORY) || authorized_repo != Some(REPOSITORY) {
        return Err("The requested repository must match the designated read-only fixture.");
    }
    if authorized_user != Some(EXPECTED_USER_ID) {
        return Err("Configure the verified user ID for the designated live fixture.");
    }
    if !credential_provided {
        return Err("Supply an authorized PAT via standard input or THREADLINE_LIVE_PAT.");
    }
    Ok(EXPECTED_USER_ID)
}

pub(crate) fn run(options: &Options, root: &Path, dir: &Path) -> CaseResult {
    let case = options.case.as_deref().unwrap_or(CASE);
    let repository = std::env::var("THREADLINE_LIVE_READ_REPO").ok();
    let user_id = std::env::var("THREADLINE_LIVE_EXPECTED_USER_ID")
        .ok()
        .and_then(|value| value.parse().ok());
    let credential_provided =
        options.credential_stdin || std::env::var_os("THREADLINE_LIVE_PAT").is_some();
    let user_id = match authorize(options, repository.as_deref(), user_id, credential_provided) {
        Ok(user_id) => user_id,
        Err(detail) => return CaseResult::new(case, Status::Blocked, detail),
    };
    let binary = match build_harness(root) {
        Ok(binary) => binary,
        Err(status) => {
            return CaseResult::new(case, status, "The live-read harness could not be built.");
        }
    };
    let mut command = Command::new(binary);
    command.current_dir(root).args([
        "--repo",
        REPOSITORY,
        "--expected-user-id",
        &user_id.to_string(),
        "--read-only",
    ]);
    if options.credential_stdin {
        command
            .arg("--credential-stdin")
            .env_remove("THREADLINE_LIVE_PAT")
            .stdin(Stdio::inherit());
    } else {
        command.arg("--credential-env");
    }
    if options.allow_existing_cli_oauth {
        command.arg("--allow-existing-cli-oauth");
    }
    let output = match command.output() {
        Ok(output) => output,
        Err(_) => {
            return CaseResult::new(
                case,
                Status::Blocked,
                "The live-read harness could not be started.",
            );
        }
    };
    let exit = output.status.code();
    // The provider harness emits a dedicated redacted JSON report. Never export its
    // stderr or unstructured output from the build or the HTTP client.
    let (status, label) = match exit {
        Some(0) => (Status::Pass, "Pass"),
        Some(78) => (Status::Blocked, "Blocked"),
        Some(1) => (Status::Fail, "Fail"),
        _ => {
            let mut result = CaseResult::new(
                case,
                Status::Fail,
                "The live-read harness exited unexpectedly.",
            );
            result.child_exit_code = exit;
            return result;
        }
    };
    let credential_source = if options.credential_stdin {
        "stdin_pipe"
    } else {
        "dedicated_environment"
    };
    let mut result = if report_status_matches(
        &output.stdout,
        label,
        options.allow_existing_cli_oauth,
        credential_source,
    ) {
        match fs::write(dir.join(EVIDENCE), &output.stdout) {
            Ok(()) => {
                let detail = if status == Status::Blocked
                    && serde_json::from_slice::<Value>(&output.stdout).is_ok_and(|report| {
                        report["credential_kind"] == "existing_cli_oauth"
                            && report["dogfood_read"] == "Pass"
                    }) {
                    "Read-only metadata access passed with explicitly supplied CLI OAuth. The PAT gate remains blocked."
                } else {
                    "Read-only GitHub adapter observations."
                };
                let mut result = CaseResult::new(case, status, detail);
                result.evidence = Some(EVIDENCE.into());
                result
            }
            Err(_) => CaseResult::new(
                case,
                Status::Fail,
                "Could not write the live-read evidence.",
            ),
        }
    } else {
        CaseResult::new(
            case,
            Status::Fail,
            "The live-read harness did not emit a matching structured report.",
        )
    };
    result.child_exit_code = exit;
    result
}

fn build_harness(root: &Path) -> Result<PathBuf, Status> {
    let output = Command::new(env!("CARGO"))
        .current_dir(root)
        .args([
            "build",
            "--locked",
            "-p",
            "threadline-core",
            "--example",
            "github_live_read",
            "--message-format=json",
        ])
        .env_remove("THREADLINE_LIVE_PAT")
        .output()
        .map_err(|_| Status::Blocked)?;
    if !output.status.success() {
        return Err(Status::Fail);
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|artifact| {
            (artifact["reason"] == "compiler-artifact"
                && artifact["target"]["name"] == "github_live_read"
                && artifact["target"]["kind"] == serde_json::json!(["example"]))
            .then(|| artifact["executable"].as_str().map(PathBuf::from))
            .flatten()
        })
        .filter(|binary| binary.is_absolute())
        .ok_or(Status::Fail)
}

fn report_status_matches(
    stdout: &[u8],
    label: &str,
    allow_cli: bool,
    credential_source: &str,
) -> bool {
    if stdout.len() > 16 * 1024 {
        return false;
    }
    let Ok(report) = serde_json::from_slice::<Value>(stdout) else {
        return false;
    };
    let Some(fields) = report.as_object() else {
        return false;
    };
    report["status"] == label
        && report["scenario"] == CASE
        && report["repository"] == REPOSITORY
        && report["expected_github_user_id"] == EXPECTED_USER_ID
        && report["expected_github_repository_id"] == EXPECTED_REPOSITORY_ID
        && completed_report_is_consistent(&report, label, allow_cli, credential_source)
        && [
            "authenticated_user",
            "private_repository_read",
            "graphql_repository_read",
        ]
        .iter()
        .all(|name| fields.contains_key(*name))
        && fields.iter().all(|(key, value)| match key.as_str() {
            "status"
            | "scenario"
            | "repository"
            | "expected_github_user_id"
            | "expected_github_repository_id" => true,
            "authenticated_user" | "private_repository_read" | "graphql_repository_read" => {
                endpoint_report_is_safe(value)
            }
            "credential_kind" => {
                matches!(
                    value.as_str(),
                    Some("unverified" | "fine_grained_pat" | "classic_pat")
                ) || (allow_cli && label == "Blocked" && value == "existing_cli_oauth")
            }
            "credential_source" => value == credential_source,
            "dogfood_read" => matches!(value.as_str(), Some("Pass" | "Fail" | "not_run")),
            "sso_challenge" => matches!(
                value.as_str(),
                Some("not_tested" | "observed" | "no_explicit_challenge_observed")
            ),
            "reason" => {
                matches!(
                    value.as_str(),
                    Some(
                        "invalid_request"
                            | "read_only_scope_or_expected_identity_missing"
                            | "explicit_credential_source_required"
                            | "dedicated_pat_unavailable"
                            | "credential_stdin_requires_a_pipe"
                            | "credential_read_failed"
                            | "credential_missing_or_invalid"
                            | "credential_type_not_authorized"
                            | "https_client_unavailable"
                            | "expected_login_mismatch"
                            | "graphql_access_incomplete"
                    )
                ) || (allow_cli
                    && label == "Blocked"
                    && value == "read_proven_with_cli_oauth_pat_unverified")
            }
            _ => false,
        })
}

fn completed_report_is_consistent(
    report: &Value,
    label: &str,
    allow_cli: bool,
    source: &str,
) -> bool {
    let dogfood_pass = report["dogfood_read"] == "Pass";
    if !dogfood_pass {
        return label != "Pass";
    }
    let credential_matches = match label {
        "Pass" => {
            matches!(
                report["credential_kind"].as_str(),
                Some("fine_grained_pat" | "classic_pat")
            ) && report["reason"].is_null()
        }
        "Blocked" => {
            allow_cli
                && source == "stdin_pipe"
                && report["credential_kind"] == "existing_cli_oauth"
                && report["reason"] == "read_proven_with_cli_oauth_pat_unverified"
        }
        _ => false,
    };
    let user = &report["authenticated_user"];
    let repository = &report["private_repository_read"];
    let graph = &report["graphql_repository_read"];
    credential_matches
        && report["credential_source"] == source
        && [user, repository, graph]
            .iter()
            .all(|endpoint| endpoint["http_status"] == 200)
        && user["matched"] == true
        && repository["private"] == true
        && repository["immutable_id_matched"] == true
        && repository["repository_metadata_read"] == "verified"
        && repository["write_grants"] == "unverified"
        && graph["immutable_node_matched"] == true
        && graph["complete"] == true
        && graph["errors"].as_array().is_some_and(Vec::is_empty)
}

fn endpoint_report_is_safe(value: &Value) -> bool {
    value == "not_attempted"
        || value.as_object().is_some_and(|fields| {
            !fields.is_empty()
                && fields.iter().all(|(key, value)| match key.as_str() {
                    "matched"
                    | "private"
                    | "immutable_id_matched"
                    | "immutable_node_matched"
                    | "complete" => value.is_boolean(),
                    "kind" => matches!(
                        value.as_str(),
                        Some(
                            "Cancelled"
                                | "InvalidCredential"
                                | "WrongScope"
                                | "IdentityMismatch"
                                | "PrivateRepositoryRequired"
                                | "Unauthorized"
                                | "SsoRequired"
                                | "ApprovalPending"
                                | "PermissionDenied"
                                | "MissingOrInaccessible"
                                | "RateLimited"
                                | "RedirectRejected"
                                | "InvalidPagination"
                                | "ServiceFailure"
                                | "UnexpectedStatus"
                                | "Timeout"
                                | "Transport"
                                | "MalformedResponse"
                                | "ResponseTooLarge"
                        )
                    ),
                    "reachability" => matches!(
                        value.as_str(),
                        Some("NotAttempted" | "NoResponse" | "ReceivedResponse")
                    ),
                    "http_status" => {
                        value.is_null()
                            || value
                                .as_u64()
                                .is_some_and(|status| (100..600).contains(&status))
                    }
                    "repository_metadata_read" => value == "verified",
                    "write_grants" => value == "unverified",
                    "errors" => value.as_array().is_some_and(|errors| {
                        errors.iter().all(|error| {
                            matches!(
                                error.as_str(),
                                Some(
                                    "Forbidden"
                                        | "NotFound"
                                        | "RateLimited"
                                        | "SsoRequired"
                                        | "Other"
                                )
                            )
                        })
                    }),
                    _ => false,
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli;

    #[test]
    fn live_guard_requires_exact_scope_explicit_case_verified_user_and_credential() {
        let options = |repo, case, flags: &[&str]| {
            let mut args = vec!["test", "live", "--repo", repo, "--case", case];
            args.extend_from_slice(flags);
            cli::parse(&args.into_iter().map(str::to_owned).collect::<Vec<_>>()).unwrap()
        };
        let valid = options(REPOSITORY, CASE, &["--credential-stdin"]);
        assert!(valid.credential_stdin);
        assert_eq!(
            authorize(&valid, Some(REPOSITORY), Some(EXPECTED_USER_ID), true),
            Ok(EXPECTED_USER_ID)
        );
        assert!(authorize(&valid, Some(REPOSITORY), Some(99), true).is_err());
        assert!(authorize(&valid, Some(REPOSITORY), Some(EXPECTED_USER_ID), false).is_err());
        assert!(authorize(&valid, None, Some(EXPECTED_USER_ID), true).is_err());
        let another_repo = options("someone/else", CASE, &[]);
        assert!(
            authorize(
                &another_repo,
                Some("someone/else"),
                Some(EXPECTED_USER_ID),
                true
            )
            .is_err()
        );
        let mutation = options(REPOSITORY, CASE, &["--allow-live-writes"]);
        assert!(authorize(&mutation, Some(REPOSITORY), Some(EXPECTED_USER_ID), true).is_err());
    }

    fn redacted_report() -> Value {
        serde_json::json!({
            "scenario": CASE,
            "repository": REPOSITORY,
            "expected_github_user_id": EXPECTED_USER_ID,
            "expected_github_repository_id": EXPECTED_REPOSITORY_ID,
            "authenticated_user": "not_attempted",
            "private_repository_read": "not_attempted",
            "graphql_repository_read": "not_attempted",
            "credential_kind": "unverified",
            "dogfood_read": "not_run",
            "status": "Blocked",
            "reason": "dedicated_pat_unavailable",
        })
    }

    fn verify(report: &Value, status: &str, oauth: bool) -> bool {
        report_status_matches(
            &serde_json::to_vec(report).unwrap(),
            status,
            oauth,
            "stdin_pipe",
        )
    }

    #[test]
    fn report_guard_rejects_unstructured_output_and_unknown_error_fields() {
        let mut report = redacted_report();
        assert!(verify(&report, "Blocked", false));
        assert!(!verify(&report, "Pass", false));
        assert!(!report_status_matches(
            b"not a structured report",
            "Fail",
            false,
            "stdin_pipe"
        ));
        report["authenticated_user"] = serde_json::json!({"value":"synthetic-sensitive-output"});
        assert!(!verify(&report, "Blocked", false));
        report["authenticated_user"] = serde_json::json!({
            "kind":"OutOfContractValue","reachability":"ReceivedResponse","http_status":403,
        });
        assert!(!verify(&report, "Blocked", false));
        report["authenticated_user"]["kind"] = Value::from("SsoRequired");
        assert!(verify(&report, "Blocked", false));
    }

    #[test]
    fn metadata_pass_requires_matching_endpoints_credential_and_exit_class() {
        let mut report = redacted_report();
        report["authenticated_user"] = serde_json::json!({"matched":true,"http_status":200});
        report["private_repository_read"] = serde_json::json!({
            "private":true,"immutable_id_matched":true,"http_status":200,
            "repository_metadata_read":"verified","write_grants":"unverified",
        });
        report["graphql_repository_read"] = serde_json::json!({
            "immutable_node_matched":true,"complete":true,"http_status":200,"errors":[],
        });
        report["credential_kind"] = Value::from("classic_pat");
        report["credential_source"] = Value::from("stdin_pipe");
        report["dogfood_read"] = Value::from("Pass");
        report["status"] = Value::from("Pass");
        report.as_object_mut().unwrap().remove("reason");
        assert!(verify(&report, "Pass", false));

        for name in ["credential_kind", "credential_source", "dogfood_read"] {
            let mut invalid = report.clone();
            invalid.as_object_mut().unwrap().remove(name);
            assert!(!verify(&invalid, "Pass", false), "{name}");
        }
        for endpoint in [
            "authenticated_user",
            "private_repository_read",
            "graphql_repository_read",
        ] {
            let mut invalid = report.clone();
            invalid[endpoint]["http_status"] = Value::from(403);
            assert!(!verify(&invalid, "Pass", false), "{endpoint}");
        }
        let mut invalid = report.clone();
        invalid["graphql_repository_read"]["errors"] = serde_json::json!(["Forbidden"]);
        assert!(!verify(&invalid, "Pass", false));
        invalid["graphql_repository_read"] = report["graphql_repository_read"].clone();
        invalid["credential_kind"] = Value::from("unverified");
        assert!(!verify(&invalid, "Pass", false));

        report["credential_kind"] = Value::from("existing_cli_oauth");
        report["status"] = Value::from("Blocked");
        report["reason"] = Value::from("read_proven_with_cli_oauth_pat_unverified");
        assert!(verify(&report, "Blocked", true));
        assert!(!verify(&report, "Blocked", false));
        assert!(!verify(&report, "Pass", true));
        assert!(!report_status_matches(
            &serde_json::to_vec(&report).unwrap(),
            "Blocked",
            true,
            "dedicated_environment"
        ));
    }
}
