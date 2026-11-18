use super::*;

const CASE: &str = "LIVE-003";
const PROFILE: &str = "threadline-disposable-pr-2";
const PR: u64 = 5_384_062_919;
const HEAD: &str = "66864f02bdcad8df554d6c24442dccb401f87df4";
const BLOB: &str = "06bb43c6dd5b93145dad99f6e3f2cc9b8bc20183";

pub(super) fn run(options: &Options, root: &Path, dir: &Path) -> CaseResult {
    let configured_repo = std::env::var("THREADLINE_LIVE_WRITE_REPO").ok();
    let configured_profile = std::env::var("THREADLINE_LIVE_PROFILE").ok();
    let actor = std::env::var("THREADLINE_LIVE_EXPECTED_USER_ID")
        .ok()
        .and_then(|id| id.parse().ok());
    if !authorized(
        options,
        configured_repo.as_deref(),
        configured_profile.as_deref(),
        actor,
    ) {
        return CaseResult::new(
            CASE,
            Status::Blocked,
            "The approved review scope, execution profile, actor and explicit input are required.",
        );
    }
    let binary = match build_harness(root, "github_live_review") {
        Ok(value) => value,
        Err(status) => {
            return CaseResult::new(
                CASE,
                status,
                "Could not build the authorized review harness.",
            );
        }
    };
    let mut command = Command::new(binary);
    command
        .current_dir(root)
        .args([
            "--repo",
            REPOSITORY,
            "--expected-user-id",
            &EXPECTED_USER_ID.to_string(),
            "--profile",
            PROFILE,
            if options.readback_only {
                "--readback-only"
            } else {
                "--allow-live-writes"
            },
            "--journal-dir",
        ])
        .arg(root.join("artifacts/verification/live-write-journal"));
    command
        .arg("--credential-stdin")
        .env_remove("THREADLINE_LIVE_PAT")
        .stdin(Stdio::inherit());
    if options.allow_existing_cli_oauth {
        command.arg("--allow-existing-cli-oauth");
    }
    let output = match command.output() {
        Ok(value) => value,
        Err(_) => {
            return CaseResult::new(
                CASE,
                Status::Blocked,
                "The authorized review harness could not start.",
            );
        }
    };
    let code = output.status.code();
    let status = match code {
        Some(0) => Status::Pass,
        Some(78) => Status::Blocked,
        Some(1) => Status::Fail,
        _ => {
            return CaseResult::new(
                CASE,
                Status::Fail,
                "The authorized review harness exited unexpectedly.",
            );
        }
    };
    let mut result = if matches_receipt(
        &output.stdout,
        status,
        options.allow_existing_cli_oauth,
        options.readback_only,
    ) && fs::write(dir.join("github-review.json"), &output.stdout).is_ok()
    {
        let mut result = CaseResult::new(
            CASE,
            status,
            "See the exact anchored comment-only review and the redacted readback evidence.",
        );
        result.evidence = Some("github-review.json".to_owned());
        result
    } else {
        CaseResult::new(
            CASE,
            Status::Fail,
            "The authorized review did not return a matching structured receipt.",
        )
    };
    result.child_exit_code = code;
    result
}

fn authorized(
    options: &Options,
    repo: Option<&str>,
    profile: Option<&str>,
    actor: Option<u64>,
) -> bool {
    options.case.as_deref() == Some(CASE)
        && (options.allow_live_writes != options.readback_only)
        && options.credential_stdin
        && !options.first_issue_pr_page
        && options.repo.as_deref() == Some(REPOSITORY)
        && repo == Some(REPOSITORY)
        && profile == Some(PROFILE)
        && actor == Some(EXPECTED_USER_ID)
}

fn matches_receipt(bytes: &[u8], status: Status, allow_oauth: bool, readback_only: bool) -> bool {
    if bytes.len() > 8192 {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    let expected = [
        ("scenario", serde_json::json!(CASE)),
        ("execution_profile", serde_json::json!(PROFILE)),
        ("repository", serde_json::json!(REPOSITORY)),
        ("github_user_id", serde_json::json!(EXPECTED_USER_ID)),
        (
            "github_repository_id",
            serde_json::json!(EXPECTED_REPOSITORY_ID),
        ),
        ("github_pull_request_id", serde_json::json!(PR)),
        ("pull_request_number", serde_json::json!(2)),
        ("head", serde_json::json!(HEAD)),
        ("blob", serde_json::json!(BLOB)),
        (
            "path",
            serde_json::json!("docs/fixtures/review-checklist.md"),
        ),
        ("side", serde_json::json!("RIGHT")),
        ("line", serde_json::json!(5)),
        ("event", serde_json::json!("COMMENT")),
        ("approval_verification", serde_json::json!("Not tested")),
        (
            "request_changes_verification",
            serde_json::json!("Not tested"),
        ),
        ("credential_source", serde_json::json!("stdin_pipe")),
        (
            "status",
            serde_json::json!(match status {
                Status::Pass => "Pass",
                Status::Blocked => "Blocked",
                Status::Fail => "Fail",
                Status::NotApplicable => return false,
            }),
        ),
    ];
    if value
        .as_object()
        .is_none_or(|object| object.len() != expected.len() + 2)
        || expected
            .iter()
            .any(|(field, expected)| value[*field] != *expected)
    {
        return false;
    }
    let credential = match value["credential_kind"].as_str() {
        Some("fine_grained_pat" | "classic_pat") => true,
        Some("existing_cli_oauth") => allow_oauth,
        Some("unverified") => status == Status::Blocked,
        _ => false,
    };
    let evidence = &value["evidence"];
    let booleans = [
        "scope_verified",
        "source_verified",
        "readback_only",
        "reconciled_unknown_submit",
        "review_read_back",
    ];
    let additional_fields = [
        "review_id",
        "inline_id",
        "pending_http_status",
        "submit_http_status",
        "pending_request_id",
        "submit_request_id",
        "failure",
    ];
    if !credential
        || evidence.as_object().is_none_or(|object| {
            object.len() != booleans.len() + additional_fields.len()
                || !additional_fields
                    .iter()
                    .all(|field| object.contains_key(*field))
        })
        || !booleans.into_iter().all(|key| evidence[key].is_boolean())
    {
        return false;
    }
    let valid_id = |key: &str| {
        evidence[key].is_null() || evidence[key].as_u64().is_some_and(|number| number > 0)
    };
    let valid_code = |key: &str| safe_code(&evidence[key]);
    let optional_request = |key: &str| evidence[key].is_null() || request_id(&evidence[key]);
    if !valid_id("review_id")
        || !valid_id("inline_id")
        || !valid_code("pending_http_status")
        || !valid_code("submit_http_status")
        || !optional_request("pending_request_id")
        || !optional_request("submit_request_id")
    {
        return false;
    }
    if status != Status::Pass {
        return safe_failure(&evidence["failure"]);
    }
    evidence["readback_only"] == readback_only
        && evidence["scope_verified"] == true
        && evidence["source_verified"] == true
        && evidence["review_read_back"] == true
        && !evidence["review_id"].is_null()
        && !evidence["inline_id"].is_null()
        && evidence["pending_http_status"] == 200
        && request_id(&evidence["pending_request_id"])
        && (if evidence["readback_only"] == true {
            evidence["submit_request_id"].is_null()
                && evidence["submit_http_status"].is_null()
                && evidence["reconciled_unknown_submit"] == false
        } else if evidence["reconciled_unknown_submit"] == true {
            evidence["submit_request_id"].is_null()
        } else {
            evidence["submit_http_status"] == 200 && request_id(&evidence["submit_request_id"])
        })
        && evidence["failure"].is_null()
}

pub(super) fn safe_code(value: &Value) -> bool {
    value.is_null()
        || value
            .as_u64()
            .is_some_and(|code| (100..=599).contains(&code))
}

pub(super) fn request_id(value: &Value) -> bool {
    value.as_str().is_some_and(|s| {
        !s.is_empty()
            && s.len() <= 160
            && s.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_'))
    })
}

fn safe_failure(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        let fields = ["stage", "outcome", "kind", "http_status"];
        object.len() == fields.len() && fields.iter().all(|field| object.contains_key(*field))
    }) && matches!(
        value["stage"].as_str(),
        Some(
            "authorize"
                | "credential"
                | "transport"
                | "recover"
                | "authenticate"
                | "repository"
                | "pull_request"
                | "comparison"
                | "source"
                | "review_target"
                | "existing_reviews"
                | "pending_preflight"
                | "create_journal"
                | "create_review"
                | "pending_journal"
                | "verify_pending"
                | "submit_journal"
                | "submit_review"
                | "review_readback"
                | "inline_readback"
                | "complete_journal"
        )
    ) && matches!(
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
                | "journal_unavailable_or_preexisting"
                | "read_failed"
                | "unverified_or_rate_held"
                | "target_changed"
                | "previous_remote_review_exists"
                | "response_mismatch"
                | "definitely_unsent"
                | "access_rejected"
                | "invalid_input"
                | "throttled"
                | "outcome_unknown"
        )
    ) && (value["kind"].is_null()
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
        && safe_code(&value["http_status"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_review_requires_distinct_profile_and_private_input() {
        let parse = |commands: &str| {
            crate::cli::parse(
                &commands
                    .split_ascii_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
            )
            .unwrap()
        };
        let approved = parse(
            "test live --case LIVE-003 --repo joshmarkovic/threadline --allow-live-writes --credential-stdin --allow-existing-cli-oauth",
        );
        assert!(authorized(
            &approved,
            Some(REPOSITORY),
            Some(PROFILE),
            Some(EXPECTED_USER_ID)
        ));
        assert!(!authorized(
            &approved,
            Some(REPOSITORY),
            Some("threadline-disposable-issue-1"),
            Some(EXPECTED_USER_ID)
        ));
        let no_stdin =
            parse("test live --case LIVE-003 --repo joshmarkovic/threadline --allow-live-writes");
        assert!(!authorized(
            &no_stdin,
            Some(REPOSITORY),
            Some(PROFILE),
            Some(EXPECTED_USER_ID)
        ));
    }

    #[test]
    fn review_receipt_requires_verified_readback_and_discards_unknown_data() {
        let mut receipt = serde_json::json!({
            "scenario":CASE,"execution_profile":PROFILE,"repository":REPOSITORY,
            "github_user_id":EXPECTED_USER_ID,"github_repository_id":EXPECTED_REPOSITORY_ID,
            "github_pull_request_id":PR,"pull_request_number":2,"head":HEAD,"blob":BLOB,
            "path":"docs/fixtures/review-checklist.md","side":"RIGHT","line":5,"event":"COMMENT",
            "approval_verification":"Not tested","request_changes_verification":"Not tested",
            "credential_kind":"existing_cli_oauth","credential_source":"stdin_pipe","status":"Pass",
            "evidence":{
                "scope_verified":true,"source_verified":true,"review_id":91,
                "pending_http_status":200,"pending_request_id":"TEST:1","submit_http_status":502,
                "submit_request_id":null,"readback_only":false,"reconciled_unknown_submit":true,
                "review_read_back":true,"inline_id":92,"failure":null,
            },
        });
        let verifies = |value: &Value, oauth| {
            matches_receipt(
                &serde_json::to_vec(value).unwrap(),
                Status::Pass,
                oauth,
                false,
            )
        };
        assert!(verifies(&receipt, true));
        assert!(!verifies(&receipt, false));
        receipt["evidence"]["inline_id"] = Value::Null;
        assert!(!verifies(&receipt, true));
        receipt["evidence"]["inline_id"] = 92.into();
        let evidence = receipt["evidence"].as_object_mut().unwrap();
        evidence.remove("submit_request_id");
        evidence.insert(
            "private_content".into(),
            "must not leave the harness".into(),
        );
        assert!(!verifies(&receipt, true));
    }
}
