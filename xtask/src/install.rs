use std::{
    fs::{self, File},
    io::Read as _,
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    cli::{Lane, Options, catalog_cases},
    report::{CaseResult, Status, sha256},
};

pub const LOCAL_CASE: &str = "S12-LOCAL-PACKAGE";
const CANDIDATE: &str = "candidate.json";
const MATRIX: &str = "windows-local-lifecycle.json";
const STEPS: &[(&str, i32)] = &[
    ("install_predecessor", 0),
    ("repair_predecessor", 0),
    ("refuse_locked_executable", 14),
    ("fail_staged_write", 16),
    ("upgrade_after_failures", 0),
    ("repair_current", 0),
    ("refuse_installed_downgrade", 12),
    ("uninstall_retains_profile", 0),
    ("refuse_downgrade_after_uninstall", 12),
    ("reinstall_current", 0),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustedFixture {
    predecessor: ExpectedPackage,
    current_sha256: String,
    stage_failure: ExpectedPackage,
    profile_sha256: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedPackage {
    path: PathBuf,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MatrixReceipt {
    channel: String,
    target: String,
    windows_build: String,
    predecessor_setup_sha256: String,
    current_setup_sha256: String,
    staged_fault_setup_sha256: String,
    preseeded_profile_checked: bool,
    completed_steps: Vec<StepReceipt>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StepReceipt {
    step: String,
    exit_code: i32,
}

impl MatrixReceipt {
    fn agrees_with(&self, expected: &TrustedFixture) -> bool {
        self.channel == "local-fixture"
            && self.target == "x86_64-pc-windows-msvc"
            && self
                .windows_build
                .parse::<u32>()
                .is_ok_and(|build| build >= 22_000)
            && self
                .predecessor_setup_sha256
                .eq_ignore_ascii_case(&expected.predecessor.sha256)
            && self
                .current_setup_sha256
                .eq_ignore_ascii_case(&expected.current_sha256)
            && self
                .staged_fault_setup_sha256
                .eq_ignore_ascii_case(&expected.stage_failure.sha256)
            && self.preseeded_profile_checked == expected.profile_sha256.is_some()
            && self.completed_steps.len() <= STEPS.len()
            && self
                .completed_steps
                .iter()
                .zip(STEPS)
                .all(|(actual, &(step, exit_code))| {
                    actual.step == step && actual.exit_code == exit_code
                })
    }
}

pub fn run(options: &Options, root: &Path, dir: &Path) -> Vec<CaseResult> {
    let requested = options.case.as_deref();
    let ids = requested.map_or_else(
        || {
            let mut cases = catalog_cases(Lane::Install).collect::<Vec<_>>();
            if options.local_fixture.is_some() {
                cases.push(LOCAL_CASE);
            }
            cases
        },
        |id| vec![id],
    );
    if requested.is_some_and(future_platform) {
        return ids.into_iter().map(planned_result).collect();
    }

    let Some(artifact) = options.artifact.as_ref().filter(|path| path.is_file()) else {
        return ids
            .into_iter()
            .map(|id| {
                if future_platform(id) {
                    planned_result(id)
                } else {
                    CaseResult::new(id, Status::Blocked, "Candidate package is missing.")
                }
            })
            .collect();
    };
    let identity = sha256(artifact).and_then(|digest| {
        let document = json!({
            "sha256": digest,
            "size_bytes": artifact.metadata()?.len(),
            "evidence_class": "candidate inspected only"
        });
        fs::write(dir.join(CANDIDATE), serde_json::to_vec_pretty(&document)?)?;
        Ok(digest)
    });
    let Ok(candidate_digest) = identity else {
        return ids
            .into_iter()
            .map(|id| {
                CaseResult::new(
                    id,
                    Status::Fail,
                    "Could not record candidate size and digest.",
                )
            })
            .collect();
    };

    let local = if ids.iter().any(|id| matches!(*id, LOCAL_CASE | "PKG-002")) {
        Some(options.local_fixture.as_ref().map_or_else(
            || {
                CaseResult::new(
                    LOCAL_CASE,
                    Status::Blocked,
                    "Provide the independent trusted local fixture configuration.",
                )
            },
            |config| run_matrix(artifact, &candidate_digest, config, root, dir),
        ))
    } else {
        None
    };
    ids.into_iter().map(|id| {
        match (id, local.as_ref()) {
            (LOCAL_CASE, Some(result)) => copy_result(id, result),
            ("PKG-002", Some(result)) if result.status == Status::Pass => {
                let seeded = read_receipt(&dir.join(MATRIX)).is_some_and(|receipt| receipt.preseeded_profile_checked);
                if seeded {
                    let mut completed = copy_result(id, result);
                    completed.detail = "Native predecessor/current repairs preserved the seeded profile and the fixture integration.".into();
                    completed.tests_passed = Some(2);
                    completed
                } else {
                    linked(id, Status::Blocked, "Native repair finished. Supply the seeded profile digest to verify retention.", MATRIX)
                }
            }
            ("PKG-002", Some(result)) => copy_result(id, result),
            _ => planned_result(id),
        }
    }).collect()
}

fn future_platform(id: &str) -> bool {
    matches!(id, "PKG-007" | "PKG-008")
}

fn planned_result(id: &str) -> CaseResult {
    if future_platform(id) {
        return CaseResult::new(
            id,
            Status::NotApplicable,
            "This release phase covers Windows. macOS and Linux packaging are in their later release phases.",
        );
    }
    let reason = match id {
        "PKG-001" => {
            "Publication is pending. The exact public Windows installation command has no approved signed package."
        }
        "PKG-003" => {
            "This case requires an installed application restart and retained-state migration evidence."
        }
        "PKG-004" => {
            "The local lifecycle exercises lock and staging failures. Full package fault coverage is still required."
        }
        "PKG-005" => {
            "Publication is pending. This case requires an offline smoke of the exact shipping package."
        }
        "PKG-006" => {
            "The local lifecycle exercises default retention. The explicit data and credential removal case is still required."
        }
        "PKG-009" => {
            "The local lifecycle exercises downgrade refusal. Full wrong-target and schema compatibility coverage is still required."
        }
        _ => "Run the local Windows packaging adapter with its trusted fixture configuration.",
    };
    linked(id, Status::Blocked, reason, CANDIDATE)
}

fn copy_result(case: &str, source: &CaseResult) -> CaseResult {
    let mut result = CaseResult::new(case, source.status, source.detail.clone());
    result.evidence = source.evidence.clone();
    result.child_exit_code = source.child_exit_code;
    result.tests_passed = source.tests_passed;
    result.fixture_revision = source.fixture_revision;
    result
}

fn linked(case: &str, status: Status, detail: &str, evidence: &str) -> CaseResult {
    let mut result = CaseResult::new(case, status, detail);
    result.evidence = Some(evidence.into());
    result
}

fn run_matrix(
    artifact: &Path,
    candidate_digest: &str,
    config: &Path,
    root: &Path,
    dir: &Path,
) -> CaseResult {
    let expected = match trusted_fixture(config, candidate_digest) {
        Ok(expected) => expected,
        Err(message) => return linked(LOCAL_CASE, Status::Fail, message, CANDIDATE),
    };
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return linked(
            LOCAL_CASE,
            Status::Blocked,
            "The local installation matrix requires native x64 Windows 11.",
            CANDIDATE,
        );
    }
    let Some(shell) = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|path| path.join("System32/WindowsPowerShell/v1.0/powershell.exe"))
        .filter(|path| path.is_file())
    else {
        return linked(
            LOCAL_CASE,
            Status::Blocked,
            "Inbox Windows PowerShell is unavailable.",
            CANDIDATE,
        );
    };
    let mut native = Command::new(shell);
    native
        .env_remove("PSModulePath")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
        .arg(root.join("tools/windows/test-local-lifecycle.ps1"))
        .arg("-Predecessor")
        .arg(&expected.predecessor.path)
        .arg("-PredecessorSha256")
        .arg(&expected.predecessor.sha256)
        .arg("-Current")
        .arg(artifact)
        .arg("-CurrentSha256")
        .arg(&expected.current_sha256)
        .arg("-StageFailure")
        .arg(&expected.stage_failure.path)
        .arg("-StageFailureSha256")
        .arg(&expected.stage_failure.sha256);
    if let Some(profile) = &expected.profile_sha256 {
        native.arg("-ProfileSha256").arg(profile);
    }
    let Ok(output) = native.output() else {
        return linked(
            LOCAL_CASE,
            Status::Blocked,
            "Could not start the trusted Windows lifecycle adapter.",
            CANDIDATE,
        );
    };
    let decoded = serde_json::from_slice::<MatrixReceipt>(&output.stdout)
        .ok()
        .filter(|receipt| receipt.agrees_with(&expected));
    let Some(receipt) = decoded else {
        let mut failed = linked(
            LOCAL_CASE,
            Status::Fail,
            "The Windows adapter did not return a valid local lifecycle receipt.",
            CANDIDATE,
        );
        failed.child_exit_code = output.status.code();
        return failed;
    };
    let bytes = serde_json::to_vec_pretty(&receipt).expect("the fixed receipt is serializable");
    let recorded = fs::write(dir.join(MATRIX), bytes).is_ok();
    let passed =
        recorded && output.status.success() && receipt.completed_steps.len() == STEPS.len();
    let (status, detail) = if passed {
        (
            Status::Pass,
            "Verified the complete native Windows fixture install, repair, refusal and uninstall/reinstall sequence.",
        )
    } else if recorded {
        (
            Status::Fail,
            "The native Windows fixture stopped before its complete expected lifecycle.",
        )
    } else {
        (
            Status::Fail,
            "Could not save the native local lifecycle receipt.",
        )
    };
    let mut result = linked(
        LOCAL_CASE,
        status,
        detail,
        if recorded { MATRIX } else { CANDIDATE },
    );
    result.child_exit_code = output.status.code();
    result.tests_passed = Some(receipt.completed_steps.len());
    result
}

fn read_receipt(path: &Path) -> Option<MatrixReceipt> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn trusted_fixture(config: &Path, candidate_digest: &str) -> Result<TrustedFixture, &'static str> {
    let mut bytes = Vec::new();
    File::open(config)
        .and_then(|input| input.take(65_537).read_to_end(&mut bytes))
        .map_err(|_| "Could not read the trusted local fixture configuration.")?;
    if bytes.len() > 65_536 {
        return Err("The trusted local fixture configuration is too large.");
    }
    let mut expected: TrustedFixture = serde_json::from_slice(&bytes)
        .map_err(|_| "The trusted local fixture configuration has invalid fields.")?;
    if ![
        Some(expected.current_sha256.as_str()),
        Some(expected.predecessor.sha256.as_str()),
        Some(expected.stage_failure.sha256.as_str()),
        expected.profile_sha256.as_deref(),
    ]
    .into_iter()
    .flatten()
    .all(valid_digest)
    {
        return Err("Use independently recorded SHA-256 digests for the fixture.");
    }
    if !expected
        .current_sha256
        .eq_ignore_ascii_case(candidate_digest)
    {
        return Err("The candidate setup digest did not match the trusted local fixture.");
    }
    if expected
        .current_sha256
        .eq_ignore_ascii_case(&expected.predecessor.sha256)
        || expected
            .current_sha256
            .eq_ignore_ascii_case(&expected.stage_failure.sha256)
        || expected
            .predecessor
            .sha256
            .eq_ignore_ascii_case(&expected.stage_failure.sha256)
    {
        return Err("Supply three distinct approved local packages.");
    }
    let parent = config.parent().unwrap_or_else(|| Path::new("."));
    for package in [&mut expected.predecessor, &mut expected.stage_failure] {
        package.path = parent.join(&package.path);
        let digest =
            sha256(&package.path).map_err(|_| "An approved local package could not be read.")?;
        if !digest.eq_ignore_ascii_case(&package.sha256) {
            return Err("An approved local package did not match its trusted digest.");
        }
    }
    Ok(expected)
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_digests_are_verified_independently_of_package_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("predecessor.exe");
        let new = dir.path().join("current.exe");
        let fault = dir.path().join("fault.exe");
        fs::write(&old, b"predecessor").unwrap();
        fs::write(&new, b"current").unwrap();
        fs::write(&fault, b"separate fault candidate").unwrap();
        let digest = sha256(&new).unwrap();
        let notes = dir.path().join("approved.json");
        fs::write(
            &notes,
            serde_json::to_vec(&json!({
                "predecessor": { "path": "predecessor.exe", "sha256": sha256(&old).unwrap() },
                "current_sha256": digest,
                "stage_failure": { "path": "fault.exe", "sha256": sha256(&fault).unwrap() }
            }))
            .unwrap(),
        )
        .unwrap();

        let expected = trusted_fixture(&notes, &digest).unwrap();
        assert_eq!(expected.predecessor.path, old);
        assert!(expected.profile_sha256.is_none());

        fs::write(&new, b"changed").unwrap();
        let actual = sha256(&new).unwrap();
        assert_eq!(
            trusted_fixture(&notes, &actual).err(),
            Some("The candidate setup digest did not match the trusted local fixture.")
        );
        fs::write(&fault, b"different fault candidate").unwrap();
        assert_eq!(
            trusted_fixture(&notes, &digest).err(),
            Some("An approved local package did not match its trusted digest.")
        );
    }

    #[test]
    fn native_receipt_rejects_unverified_results_or_injected_steps() {
        let expected = TrustedFixture {
            predecessor: ExpectedPackage {
                path: "predecessor.exe".into(),
                sha256: "a".repeat(64),
            },
            current_sha256: "b".repeat(64),
            stage_failure: ExpectedPackage {
                path: "failure.exe".into(),
                sha256: "c".repeat(64),
            },
            profile_sha256: Some("d".repeat(64)),
        };
        let mut receipt = MatrixReceipt {
            channel: "local-fixture".into(),
            target: "x86_64-pc-windows-msvc".into(),
            windows_build: "22631".into(),
            predecessor_setup_sha256: "a".repeat(64),
            current_setup_sha256: "b".repeat(64),
            staged_fault_setup_sha256: "c".repeat(64),
            preseeded_profile_checked: true,
            completed_steps: STEPS
                .iter()
                .map(|&(step, exit_code)| StepReceipt {
                    step: step.into(),
                    exit_code,
                })
                .collect(),
        };
        assert!(receipt.agrees_with(&expected));
        receipt.completed_steps[2].exit_code = 0;
        assert!(!receipt.agrees_with(&expected));
        receipt.completed_steps[2].exit_code = 14;
        receipt.preseeded_profile_checked = false;
        assert!(!receipt.agrees_with(&expected));
        receipt.preseeded_profile_checked = true;
        receipt.completed_steps[3].step = "a private or unrecognized step".into();
        assert!(!receipt.agrees_with(&expected));
    }

    #[test]
    fn later_platforms_and_publication_are_distinct_from_the_local_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let fixture = dir.path().join("artifact.exe");
        fs::write(&fixture, b"candidate under inspection only").unwrap();
        for (case, status) in [
            ("PKG-001", Status::Blocked),
            ("PKG-005", Status::Blocked),
            ("PKG-007", Status::NotApplicable),
            ("PKG-008", Status::NotApplicable),
        ] {
            let args = [
                "test",
                "install",
                "--artifact",
                fixture.to_str().unwrap(),
                "--local-fixture",
                "must-not-be-opened.json",
                "--case",
                case,
            ]
            .map(str::to_owned);
            let options = crate::cli::parse(&args).unwrap();
            let receipt = run(&options, dir.path(), dir.path());
            assert_eq!(receipt[0].status, status);
            assert!(receipt[0].child_exit_code.is_none());
        }
    }
}
