use super::*;

const CASE: &str = "LIVE-004";
const PROFILE: &str = "threadline-disposable-pr-3";
const PR: u64 = 5_385_043_122;
const BASE: &str = "4f1e2087d726c7208f937fd6510ea346c5471198";
const HEAD: &str = "501a4528e98d07e413c6f56bac76a50a9f5d61b5";
const EVIDENCE: &str = "github-merge.json";

pub(super) fn run(options: &Options, root: &Path, dir: &Path) -> CaseResult {
    let repo = std::env::var("THREADLINE_LIVE_WRITE_REPO").ok();
    let profile = std::env::var("THREADLINE_LIVE_PROFILE").ok();
    let actor = std::env::var("THREADLINE_LIVE_EXPECTED_USER_ID")
        .ok()
        .and_then(|id| id.parse().ok());
    if options.case.as_deref() != Some(CASE)
        || options.allow_live_writes == options.readback_only
        || !options.credential_stdin
        || options.first_issue_pr_page
        || options.repo.as_deref() != Some(REPOSITORY)
        || repo.as_deref() != Some(REPOSITORY)
        || profile.as_deref() != Some(PROFILE)
        || actor != Some(EXPECTED_USER_ID)
    {
        return CaseResult::new(
            CASE,
            Status::Blocked,
            "Set the approved merge fixture and explicitly select write or readback with a private input pipe.",
        );
    }
    let binary = match build_harness(root, "github_live_merge") {
        Ok(value) => value,
        Err(status) => {
            return CaseResult::new(CASE, status, "Could not build the scoped merge harness.");
        }
    };
    let mut command = Command::new(binary);
    command
        .current_dir(root)
        .args([
            "--repo",
            REPOSITORY,
            "--profile",
            PROFILE,
            "--expected-user-id",
            &EXPECTED_USER_ID.to_string(),
            if options.readback_only {
                "--readback-only"
            } else {
                "--allow-live-writes"
            },
            "--credential-stdin",
            "--journal-dir",
        ])
        .arg(root.join("artifacts/verification/live-write-journal"))
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
                "Could not start the scoped merge harness.",
            );
        }
    };
    let exit = output.status.code();
    let status = match exit {
        Some(0) => Status::Pass,
        Some(78) => Status::Blocked,
        _ => Status::Fail,
    };
    let mut result = if matches_receipt(&output.stdout, status, options)
        && fs::write(dir.join(EVIDENCE), &output.stdout).is_ok()
    {
        let mut result = CaseResult::new(
            CASE,
            status,
            "See the expected-head eligibility, request and scoped readback evidence.",
        );
        result.evidence = Some(EVIDENCE.to_owned());
        result
    } else {
        CaseResult::new(
            CASE,
            Status::Fail,
            "The merge harness did not return a matching redacted receipt.",
        )
    };
    result.child_exit_code = exit;
    result
}

fn matches_receipt(bytes: &[u8], status: Status, options: &Options) -> bool {
    if bytes.len() > 4096 {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    let name = match status {
        Status::Pass => "Pass",
        Status::Blocked => "Blocked",
        Status::Fail => "Fail",
        Status::NotApplicable => return false,
    };
    let fixed: &[(&str, Value)] = &[
        ("scenario", CASE.into()),
        ("status", name.into()),
        ("repository", REPOSITORY.into()),
        ("profile", PROFILE.into()),
        ("provider_repo_id", EXPECTED_REPOSITORY_ID.into()),
        ("provider_user_id", EXPECTED_USER_ID.into()),
        ("provider_pull_id", PR.into()),
        ("pull_number", 3.into()),
        ("base_ref", "dogfood/windows-merge-base".into()),
        ("head_ref", "dogfood/windows-merge-head".into()),
        ("base", BASE.into()),
        ("head", HEAD.into()),
        ("method", "merge".into()),
        ("credential_source", "stdin_pipe".into()),
    ];
    if value
        .as_object()
        .is_none_or(|obj| obj.len() != fixed.len() + 2)
        || fixed.iter().any(|(key, val)| value[*key] != *val)
    {
        return false;
    }
    let credential_valid = match value["credential_kind"].as_str() {
        Some("fine_grained_pat" | "classic_pat") => true,
        Some("existing_cli_oauth") => options.allow_existing_cli_oauth,
        Some("unverified") => status == Status::Blocked,
        _ => false,
    };
    let proof = &value["evidence"];
    let flags = [
        "target_verified",
        "readback_only",
        "read_back",
        "own_action",
    ];
    let fields = [
        "eligibility",
        "verified_optional_contexts",
        "dispatch",
        "merge_oid",
        "request_id",
        "provider_status",
        "reason",
    ];
    let keys: Vec<_> = flags.iter().chain(&fields).copied().collect();
    if !credential_valid
        || !writes::only_keys(proof, &keys)
        || !flags.iter().all(|key| proof[*key].is_boolean())
    {
        return false;
    }
    let oid = proof["merge_oid"].as_str().is_some_and(|s| {
        s.len() == 40
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    });
    let request = reviews::request_id(&proof["request_id"]);
    let policy = proof["eligibility"].as_str();
    let optional = &proof["verified_optional_contexts"];
    if !(optional.is_null()
        || optional
            .as_u64()
            .is_some_and(|count| (1..=100).contains(&count)))
    {
        return false;
    }
    if !(proof["eligibility"].is_null()
        || matches!(
            policy,
            Some(
                "eligible"
                    | "merge_queue_required"
                    | "merge_queue_unknown"
                    | "direct_merge_policy_held"
            )
        ))
        || !matches!(
            proof["dispatch"].as_str(),
            Some(
                "not_sent"
                    | "acknowledged"
                    | "definitely_unsent"
                    | "access_rejected"
                    | "invalid_input"
                    | "throttled"
                    | "unknown"
            )
        )
        || !(proof["merge_oid"].is_null() || oid)
        || !(proof["request_id"].is_null() || request)
        || !reviews::safe_code(&proof["provider_status"])
    {
        return false;
    }
    if status != Status::Pass {
        return matches!(
            proof["reason"].as_str(),
            Some(
                "missing_scope_or_explicit_mode"
                    | "credential_unavailable"
                    | "transport_unavailable"
                    | "journal_unavailable_or_preexisting"
                    | "authenticate_failed"
                    | "repository_read_failed"
                    | "provider_rate_hold"
                    | "target_changed"
                    | "pull_read_failed"
                    | "merge_policy_read_failed"
                    | "optional_checks_read_failed"
                    | "branch_protection_read_failed"
                    | "merge_queue_required"
                    | "merge_queue_unknown"
                    | "direct_merge_policy_held"
                    | "merge_not_confirmed"
                    | "merge_readback_not_confirmed"
            )
        );
    }
    proof["target_verified"] == true
        && proof["read_back"] == true
        && proof["own_action"] == true
        && proof["reason"].is_null()
        && oid
        && request
        && proof["readback_only"] == options.readback_only
        && if options.readback_only {
            proof["dispatch"] == "not_sent" && proof["provider_status"].is_null()
        } else {
            proof["dispatch"] == "acknowledged"
                && proof["provider_status"] == 200
                && policy == Some("eligible")
        }
}
