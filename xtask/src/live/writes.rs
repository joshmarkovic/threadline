use super::*;

const CASE: &str = "LIVE-002";
const PROFILE: &str = "threadline-disposable-issue-1";
const ISSUE: u64 = 6_476_663_523;
const EVIDENCE: &str = "github-write.json";

fn authorize(
    options: &Options,
    repo: Option<&str>,
    actor: Option<u64>,
    profile: Option<&str>,
    credential: bool,
) -> Result<(), &'static str> {
    if options.case.as_deref() != Some(CASE)
        || !options.allow_live_writes
        || options.first_issue_pr_page
    {
        return Err("Select LIVE-002 with the explicit write opt-in.");
    }
    if options.repo.as_deref() != Some(REPOSITORY)
        || repo != Some(REPOSITORY)
        || actor != Some(EXPECTED_USER_ID)
        || profile != Some(PROFILE)
    {
        return Err(
            "Configure the exact disposable execution profile, repository and verified user ID.",
        );
    }
    if options.allow_existing_cli_oauth && !options.credential_stdin {
        return Err("CLI OAuth requires an explicit private standard-input pipe.");
    }
    if !credential {
        return Err(
            "Supply the authorized fixture credential through the designated private source.",
        );
    }
    Ok(())
}

pub(super) fn run(options: &Options, root: &Path, dir: &Path) -> CaseResult {
    let repo = std::env::var("THREADLINE_LIVE_WRITE_REPO").ok();
    let actor = std::env::var("THREADLINE_LIVE_EXPECTED_USER_ID")
        .ok()
        .and_then(|value| value.parse().ok());
    let profile = std::env::var("THREADLINE_LIVE_PROFILE").ok();
    let credential = options.credential_stdin || std::env::var_os("THREADLINE_LIVE_PAT").is_some();
    if let Err(detail) = authorize(
        options,
        repo.as_deref(),
        actor,
        profile.as_deref(),
        credential,
    ) {
        return CaseResult::new(CASE, Status::Blocked, detail);
    }
    let binary = match build_harness(root, "github_live_write") {
        Ok(binary) => binary,
        Err(status) => {
            return CaseResult::new(CASE, status, "The scoped write harness could not be built.");
        }
    };
    let journal = root.join("artifacts/verification/live-write-journal");
    let mut launch = Command::new(binary);
    launch
        .current_dir(root)
        .args([
            "--repo",
            REPOSITORY,
            "--expected-user-id",
            &EXPECTED_USER_ID.to_string(),
            "--allow-live-writes",
            "--profile",
            PROFILE,
            "--journal-dir",
        ])
        .arg(journal);
    let source = if options.credential_stdin {
        launch
            .arg("--credential-stdin")
            .env_remove("THREADLINE_LIVE_PAT")
            .stdin(Stdio::inherit());
        "stdin_pipe"
    } else {
        launch.arg("--credential-env");
        "dedicated_environment"
    };
    if options.allow_existing_cli_oauth {
        launch.arg("--allow-existing-cli-oauth");
    }
    let output = match launch.output() {
        Ok(output) => output,
        Err(_) => {
            return CaseResult::new(
                CASE,
                Status::Blocked,
                "The scoped write harness could not be started.",
            );
        }
    };
    let exit = output.status.code();
    let status = match exit {
        Some(0) => Status::Pass,
        Some(78) => Status::Blocked,
        _ => Status::Fail,
    };
    let mut result = if report_matches(
        &output.stdout,
        status,
        source,
        options.allow_existing_cli_oauth,
    ) && fs::write(dir.join(EVIDENCE), &output.stdout).is_ok()
    {
        let mut result = CaseResult::new(
            CASE,
            status,
            "See the redacted response, exact identity, and cleanup evidence.",
        );
        result.evidence = Some(EVIDENCE.to_owned());
        result
    } else {
        CaseResult::new(
            CASE,
            Status::Fail,
            "The scoped write harness did not produce a matching redacted receipt.",
        )
    };
    result.child_exit_code = exit;
    result
}

fn report_matches(bytes: &[u8], status: Status, source: &str, allow_oauth: bool) -> bool {
    if bytes.len() > 8192 {
        return false;
    }
    let Ok(report) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    if !only_keys(
        &report,
        &[
            "scenario",
            "execution_profile",
            "repository",
            "github_user_id",
            "github_repository_id",
            "github_issue_id",
            "issue_number",
            "credential_source",
            "credential_kind",
            "dedicated_pat_coverage",
            "status",
            "proof",
        ],
    ) {
        return false;
    }
    if report["scenario"] != CASE
        || report["execution_profile"] != PROFILE
        || report["repository"] != REPOSITORY
        || report["github_user_id"] != EXPECTED_USER_ID
        || report["github_repository_id"] != EXPECTED_REPOSITORY_ID
        || report["github_issue_id"] != ISSUE
        || report["issue_number"] != 1
        || report["credential_source"] != source
        || report["status"] != status.label()
    {
        return false;
    }
    let credential = report["credential_kind"].as_str();
    let pat = matches!(credential, Some("fine_grained_pat" | "classic_pat"));
    let oauth = credential == Some("existing_cli_oauth") && allow_oauth && source == "stdin_pipe";
    if !pat && !oauth && !(status == Status::Blocked && credential == Some("unverified")) {
        return false;
    }
    let proof = &report["proof"];
    let confirmed = [
        "identity_verified",
        "disposable_issue_verified",
        "journal_armed",
        "title_read_back",
        "comment_creation_read_back",
        "comment_edit_read_back",
        "original_title_restored",
    ];
    let expected = [
        ("edit_title", 200),
        ("create_comment", 201),
        ("edit_comment", 200),
        ("restore_title", 200),
    ];
    if !only_keys(
        proof,
        &[
            "identity_verified",
            "disposable_issue_verified",
            "journal_armed",
            "title_read_back",
            "comment_id",
            "comment_creation_read_back",
            "comment_edit_read_back",
            "original_title_restored",
            "steps",
            "failure",
            "cleanup_failure",
        ],
    ) || !confirmed.iter().all(|name| proof[*name].is_boolean())
        || !(proof["comment_id"].is_null()
            || proof["comment_id"].as_u64().is_some_and(|id| id != 0))
        || !safe_failure(&proof["failure"])
        || !safe_failure(&proof["cleanup_failure"])
    {
        return false;
    }
    let Some(steps) = proof["steps"].as_array() else {
        return false;
    };
    let mut position = 0;
    for step in steps {
        let found = expected[position..].iter().position(|(action, code)| {
            only_keys(step, &["action", "http_status", "request_id"])
                && step["action"] == *action
                && step["http_status"] == *code
                && step["request_id"].as_str().is_some_and(|id| {
                    !id.is_empty()
                        && id.len() <= 160
                        && id
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'-' | b'_'))
                })
        });
        let Some(found) = found else {
            return false;
        };
        position += found + 1;
    }
    if status != Status::Pass {
        return report["dedicated_pat_coverage"] == "Not tested"
            && (proof["failure"].is_object() || proof["cleanup_failure"].is_object());
    }
    (pat || oauth)
        && report["dedicated_pat_coverage"] == if pat { "Verified" } else { "Not tested" }
        && confirmed.into_iter().all(|name| proof[name] == true)
        && proof["comment_id"].as_u64().is_some_and(|id| id != 0)
        && proof["failure"].is_null()
        && proof["cleanup_failure"].is_null()
        && steps.len() == expected.len()
}

fn only_keys(value: &Value, expected: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == expected.len() && expected.iter().all(|key| object.contains_key(*key))
    })
}

fn safe_failure(value: &Value) -> bool {
    value.is_null()
        || (only_keys(value, &["stage", "outcome", "kind", "http_status"])
            && matches!(
                value["stage"].as_str(),
                Some(
                    "authorize"
                        | "authenticate_input"
                        | "start_transport"
                        | "verify_actor"
                        | "verify_repository"
                        | "verify_existing_issue"
                        | "verify_no_existing_comment"
                        | "arm_journal"
                        | "edit_title"
                        | "read_edited_title"
                        | "create_comment"
                        | "read_created_comment"
                        | "edit_comment"
                        | "read_edited_comment"
                        | "verify_title_before_restore"
                        | "restore_title"
                        | "read_restored_title"
                        | "persist_receipt"
                )
            )
            && matches!(
                value["outcome"].as_str(),
                Some(
                    "invalid_request"
                        | "write_scope_or_opt_in_missing"
                        | "credential_source_missing"
                        | "cli_oauth_requires_explicit_pipe"
                        | "journal_location_missing"
                        | "credential_stdin_requires_a_pipe"
                        | "credential_read_failed"
                        | "dedicated_pat_unavailable"
                        | "credential_type_not_authorized"
                        | "credential_invalid"
                        | "read_failed"
                        | "unexpected_visibility"
                        | "disposable_issue_changed"
                        | "disposable_issue_has_comments"
                        | "previous_attempt_requires_review"
                        | "journal_unavailable"
                        | "definitely_unsent"
                        | "access_rejected"
                        | "invalid_input"
                        | "throttled"
                        | "outcome_unknown"
                        | "response_value_unverified"
                        | "changed_title_left_untouched"
                        | "server_hold_respected"
                )
            )
            && (value["kind"].is_null()
                || matches!(
                    value["kind"].as_str(),
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
                ))
            && (value["http_status"].is_null()
                || value["http_status"]
                    .as_u64()
                    .is_some_and(|code| (100..=599).contains(&code))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_guard_requires_its_own_scope_and_cannot_use_oauth_from_environment() {
        let parse = |args: &str| {
            crate::cli::parse(
                &args
                    .split_ascii_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
            )
            .unwrap()
        };
        let prefix = "test live --case LIVE-002 --repo joshmarkovic/threadline";
        let opts = parse(&format!(
            "{prefix} --allow-live-writes --credential-stdin --allow-existing-cli-oauth"
        ));
        assert!(
            authorize(
                &opts,
                Some(REPOSITORY),
                Some(EXPECTED_USER_ID),
                Some(PROFILE),
                true
            )
            .is_ok()
        );
        assert!(
            authorize(
                &opts,
                Some("fixture-org/private"),
                Some(EXPECTED_USER_ID),
                Some(PROFILE),
                true
            )
            .is_err()
        );
        assert!(authorize(&opts, Some(REPOSITORY), Some(42), Some(PROFILE), true).is_err());
        let wrong_source = parse(&format!(
            "{prefix} --allow-live-writes --allow-existing-cli-oauth"
        ));
        assert!(
            authorize(
                &wrong_source,
                Some(REPOSITORY),
                Some(EXPECTED_USER_ID),
                Some(PROFILE),
                true
            )
            .is_err()
        );
        let no_opt_in = parse(prefix);
        assert!(
            authorize(
                &no_opt_in,
                Some(REPOSITORY),
                Some(EXPECTED_USER_ID),
                Some(PROFILE),
                true
            )
            .is_err()
        );
    }
}
