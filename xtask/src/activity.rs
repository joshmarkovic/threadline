use crate::{
    cli::Options,
    report::{CaseResult, Status, sha256},
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

mod runtime;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    app_sha256: String,
    fixture_tool: PathBuf,
    fixture_sha256: String,
    runtime_inventory: Option<runtime::Configuration>,
}

fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn corroborates(id: &str, receipt: &Value) -> bool {
    if id == "E2E-020" {
        return runtime::corroborates(receipt);
    }
    let last = receipt["actions"]
        .as_array()
        .and_then(|actions| actions.last());
    let Some(state) = last.map(|a| &a["observed"]) else {
        return false;
    };
    if receipt["status"] != "Pass"
        || receipt["case"] != id
        || receipt["synthetic_profile_archived"] != true
        || state["draft_preserved"] != true
        || state["draft_tab_preserved"] != true
        || state["feed_count"] != 37
    {
        return false;
    }
    match id {
        "E2E-012" => {
            let first = receipt["initial_pid"].as_u64();
            let cold = receipt["cold_pid"].as_u64();
            matches!((first,cold),(Some(a),Some(b)) if a>0 && b>0 && a!=b)
                && state["selected_issue"] == 42
                && state["activated"] == true
                && state["read_count"] == 37
                && state["pending_opens"] == 0
                && state["outcome"] == "delivered"
                && state["group_count"] == 1
        }
        "E2E-018" => {
            state["group_count"] == 0
                && state["os_feed_only"] == 37
                && state["snoozed_targets"] == 1
                && state["read_count"] == 1
        }
        _ => false,
    }
}

pub fn run(id: &str, options: &Options, root: &Path, dir: &Path) -> CaseResult {
    let blocked = |reason| CaseResult::new(id, Status::Blocked, reason);
    let failed = |reason| CaseResult::new(id, Status::Fail, reason);
    if options.suite.as_deref() != Some("full") {
        return failed("Select the full native suite for this case.");
    }
    let Some(config) = &options.local_fixture else {
        return blocked(
            "Supply --local-fixture with the test-owned installed app and fixture helper digests.",
        );
    };
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return blocked("An installed x64 Windows 11 test session is required.");
    }
    let Some(fixture): Option<Fixture> = fs::read(config).ok().and_then(|raw| {
        serde_json::from_slice(raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&raw)).ok()
    }) else {
        return failed("The trusted activity fixture configuration is unreadable or invalid.");
    };
    let Some(base) = config.parent() else {
        return failed("Invalid fixture configuration path.");
    };
    let Some(helper) = base.join(&fixture.fixture_tool).canonicalize().ok() else {
        return blocked("The selected native fixture helper is unavailable.");
    };
    let Some(exe) = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|p| p.join("Programs/Threadline Installer Test/threadline.exe"))
        .and_then(|p| p.canonicalize().ok())
    else {
        return blocked("The local Threadline test package is not installed.");
    };
    if !digest(&fixture.app_sha256)
        || !digest(&fixture.fixture_sha256)
        || !helper
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("exe"))
        || !sha256(&exe).is_ok_and(|got| got.eq_ignore_ascii_case(&fixture.app_sha256))
        || !sha256(&helper).is_ok_and(|got| got.eq_ignore_ascii_case(&fixture.fixture_sha256))
        || options.case.is_some()
            && options
                .binary
                .as_ref()
                .is_some_and(|given| given.canonicalize().ok().as_ref() != Some(&exe))
    {
        return failed(
            "The selected installed app or fixture helper did not match its trusted file identity.",
        );
    }
    let Some(shell) = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .map(|p| p.join("System32/WindowsPowerShell/v1.0/powershell.exe"))
        .filter(|p| p.is_file())
    else {
        return blocked("Inbox Windows PowerShell is unavailable.");
    };
    let mut command = Command::new(shell);
    let (script, receipt_name) = if id == "E2E-020" {
        let Some(inventory) = fixture.runtime_inventory.as_ref() else {
            return blocked(
                "Supply the matching release source and the pinned Windows dependency inventory in runtime_inventory.",
            );
        };
        match runtime::inventory(
            &exe,
            &base.join(&inventory.dependency_tree),
            inventory,
            &fixture.app_sha256,
            dir,
        ) {
            Ok(true) => (),
            Ok(false) => {
                return failed(
                    "A browser runtime was present in the installed PE or its Windows dependency inventory. See runtime-build.json.",
                );
            }
            Err(message) => return failed(message),
        }
        ("test-installed-runtime.ps1", "installed-runtime.json")
    } else {
        ("test-installed-activity.ps1", "installed-activity.json")
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x08000000);
    }
    let child = command
        .env_remove("PSModulePath")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-STA", "-File"])
        .arg(root.join("tools/windows").join(script))
        .arg("-Case")
        .arg(id)
        .arg("-AppSha256")
        .arg(&fixture.app_sha256)
        .arg("-FixtureTool")
        .arg(helper)
        .arg("-FixtureSha256")
        .arg(&fixture.fixture_sha256)
        .arg("-ReportDir")
        .arg(dir)
        .output();
    let Ok(child) = child else {
        return blocked("Could not start the installed Windows activity test.");
    };
    let receipt: Option<Value> = fs::read(dir.join(receipt_name)).ok().and_then(|bytes| {
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes)).ok()
    });
    let valid = receipt.as_ref().is_some_and(|r| {
        r["app_sha256"]
            .as_str()
            .is_some_and(|hash| hash.eq_ignore_ascii_case(&fixture.app_sha256))
            && r["fixture_sha256"]
                .as_str()
                .is_some_and(|hash| hash.eq_ignore_ascii_case(&fixture.fixture_sha256))
            && corroborates(id, r)
    });
    let mut result = match child.status.code() {
        Some(0) if valid => CaseResult::new(
            id,
            Status::Pass,
            "The installed Windows case completed with matching native and durable-state evidence.",
        ),
        Some(78)
            if receipt
                .as_ref()
                .is_some_and(|r| r["case"] == id && r["status"] == "Blocked") =>
        {
            blocked(
                "An installed window or OS delivery prerequisite was unavailable. See the native receipt.",
            )
        }
        _ => failed("The installed Windows activity case did not produce its required evidence."),
    };
    result.child_exit_code = child.status.code();
    result.evidence = receipt.is_some().then(|| receipt_name.into());
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn activity_proof_binds_initial_and_cold_process_to_the_durable_route() {
        let mut proof = json!({"status":"Pass","case":"E2E-012","initial_pid":12,"cold_pid":17,"synthetic_profile_archived":true,
            "actions":[{"observed":{"draft_preserved":true,"draft_tab_preserved":true,"feed_count":37,"selected_issue":42,
            "activated":true,"read_count":37,"pending_opens":0,"outcome":"delivered","group_count":1}}]});
        assert!(corroborates("E2E-012", &proof));
        proof["cold_pid"] = json!(12);
        assert!(!corroborates("E2E-012", &proof));
        assert!(!corroborates("E2E-018", &proof));
    }
}
