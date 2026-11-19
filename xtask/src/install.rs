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
    retention: Option<RetentionInputs>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetentionInputs {
    fixture: ExpectedPackage,
    bootstrap: ExpectedPackage,
    migration: Option<RetainedMigration>,
    #[serde(default)]
    preserve_files: Vec<ExpectedPackage>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedMigration {
    predecessor_fixture: ExpectedPackage,
    from_schema: u32,
    to_schema: u32,
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
            && self.preseeded_profile_checked
                == (expected.profile_sha256.is_some() || expected.retention.is_some())
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
            "The local matrix can verify a generated protected profile, checkpoint and installed restart. The shipping predecessor and candidate still require their own migration evidence."
        }
        "PKG-004" => {
            "The local lifecycle exercises lock and staging failures. Full package fault coverage is still required."
        }
        "PKG-005" => {
            "Publication is pending. This case requires an offline smoke of the exact shipping package."
        }
        "PKG-006" => {
            "The local retention matrix includes default uninstall, reinstall and separately confirmed removal. The shipping package requires its own replay."
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
    let script = if expected.retention.is_some() {
        "test-retained-package.ps1"
    } else {
        "test-local-lifecycle.ps1"
    };
    let mut native = Command::new(shell);
    native
        .env_remove("PSModulePath")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
        .arg(root.join("tools/windows").join(script))
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
    if let Some(inputs) = &expected.retention {
        native
            .arg("-Fixture")
            .arg(&inputs.fixture.path)
            .arg("-FixtureSha256")
            .arg(&inputs.fixture.sha256)
            .arg("-Bootstrap")
            .arg(&inputs.bootstrap.path)
            .arg("-BootstrapSha256")
            .arg(&inputs.bootstrap.sha256)
            .arg("-EvidenceDirectory")
            .arg(dir);
        if let Some(migration) = &inputs.migration {
            native
                .arg("-PredecessorFixture")
                .arg(&migration.predecessor_fixture.path)
                .arg("-PredecessorFixtureSha256")
                .arg(&migration.predecessor_fixture.sha256)
                .arg("-FromSchema")
                .arg(migration.from_schema.to_string())
                .arg("-ToSchema")
                .arg(migration.to_schema.to_string());
        }
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
            if expected.retention.is_some()
                && dir.join("windows-retained-package-failure.json").is_file()
            {
                "windows-retained-package-failure.json"
            } else {
                CANDIDATE
            },
        );
        failed.child_exit_code = output.status.code();
        return failed;
    };
    let bytes = serde_json::to_vec_pretty(&receipt).expect("the fixed receipt is serializable");
    let recorded = fs::write(dir.join(MATRIX), bytes).is_ok();
    let passed = recorded
        && output.status.success()
        && receipt.completed_steps.len() == STEPS.len()
        && expected.retention.as_ref().is_none_or(|inputs| {
            verified_retention(
                &dir.join("windows-retained-package.json"),
                inputs,
                candidate_digest,
            )
        });
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

fn verified_schema_transition(receipt: &serde_json::Value, inputs: &RetentionInputs) -> bool {
    let before = &receipt["before"];
    let retained = &receipt["after_default_uninstall"];
    let transition = &receipt["migration"];
    let Some(migration) = &inputs.migration else {
        return transition.is_null() && before == retained;
    };
    let mut expected_retained = before.clone();
    if !before.is_object() || before["schema"] != migration.from_schema {
        return false;
    }
    expected_retained["schema"] = json!(migration.to_schema);
    let backup = &transition["backup_sha256"];
    retained == &expected_retained
        && transition["predecessor_fixture_sha256"] == migration.predecessor_fixture.sha256
        && transition["backup_snapshot"] == *before
        && backup.as_str().is_some_and(valid_digest)
        && transition["backup_after_uninstall_sha256"] == *backup
        && transition["backup_after_cleanup_sha256"] == *backup
}

fn verified_retention(path: &Path, inputs: &RetentionInputs, candidate: &str) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    let Ok(receipt) = serde_json::from_reader::<_, serde_json::Value>(file.take(65_537)) else {
        return false;
    };
    let checkpoint = &receipt["copied_checkpoint"];
    let cleanup = &receipt["separate_confirmed_removal"];
    receipt["status"] == "Pass"
        && receipt["retained_at_steps"].as_array().is_some_and(|seen| {
            seen.iter()
                .map(serde_json::Value::as_str)
                .eq(STEPS.iter().map(|(name, _)| Some(*name)))
        })
        && receipt["fixture_sha256"] == inputs.fixture.sha256
        && receipt["bootstrap_sha256"] == inputs.bootstrap.sha256
        && receipt["current_sha256"] == candidate
        && verified_schema_transition(&receipt, inputs)
        && receipt["before"]["protected_credential_present"] == true
        && receipt["before"]["step_receipts"] == 1
        && receipt["before"]["queued_operations"] == 8
        && receipt["before"]["saved_comment_page_cursor"] == "2"
        && receipt["native"]["owned_window"] == true
        && receipt["native"]["selected_before_invocation"] == false
        && receipt["native"]["tab_activated"] == true
        && receipt["native"]["orderly_exit"] == true
        && receipt["closed_database_sha256"]
            .as_str()
            .is_some_and(valid_digest)
        && checkpoint["source_database_sha256"] == receipt["closed_database_sha256"]
        && checkpoint["scheduler_claimed_page"] == 2
        && checkpoint["next_cursor_empty"] == true
        && checkpoint["archived_bytes_unchanged"] == true
        && checkpoint["queued_payloads_and_step_receipts_unchanged"] == true
        && cleanup["bootstrap_sha256"] == inputs.bootstrap.sha256
        && cleanup["normal_preview_exit"] == 0
        && cleanup["normal_confirmation_exit"] == 0
        && [
            "conpty_prompt_verified",
            "actual_confirmation_sent_once",
            "current_db_and_backup_removed",
            "bound_protected_credential_removed",
            "independent_export_unchanged",
            "independent_credential_preserved_then_fixture_cleaned",
            "prior_retention_archive_unchanged",
            "default_profile_absent_before_and_after",
        ]
        .iter()
        .all(|field| cleanup[*field] == true)
        && receipt["default_data_absent"] == true
        && inputs
            .preserve_files
            .iter()
            .all(|file| sha256(&file.path).is_ok_and(|hash| hash == file.sha256))
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
    expected.current_sha256.make_ascii_lowercase();
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
    if expected.retention.is_some() && expected.profile_sha256.is_some() {
        return Err(
            "Choose either the existing profile digest or a newly generated retention fixture.",
        );
    }
    let mut pinned = vec![&mut expected.predecessor, &mut expected.stage_failure];
    if let Some(inputs) = &mut expected.retention {
        if inputs.fixture.path.file_name()
            != Some(std::ffi::OsStr::new("package_retention_fixture.exe"))
            || inputs.bootstrap.path.file_name()
                != Some(std::ffi::OsStr::new("threadline-installer.exe"))
        {
            return Err(
                "Use the named package retention fixture and normal bootstrap executables.",
            );
        }
        if inputs.preserve_files.len() > 64 {
            return Err("Supply at most 64 pinned files from the previous synthetic archives.");
        }
        if let Some(migration) = &mut inputs.migration {
            if migration.from_schema == 0
                || migration.from_schema >= migration.to_schema
                || migration.predecessor_fixture.path.file_name() != inputs.fixture.path.file_name()
                || migration
                    .predecessor_fixture
                    .sha256
                    .eq_ignore_ascii_case(&inputs.fixture.sha256)
            {
                return Err(
                    "Pin the predecessor fixture and an increasing package schema interval.",
                );
            }
            pinned.push(&mut migration.predecessor_fixture);
        }
        pinned.extend([&mut inputs.fixture, &mut inputs.bootstrap]);
        pinned.extend(&mut inputs.preserve_files);
    }
    for package in pinned {
        if !valid_digest(&package.sha256) {
            return Err("Use independently recorded SHA-256 digests for the fixture.");
        }
        package.sha256.make_ascii_lowercase();
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

        let helper = dir.path().join("package_retention_fixture.exe");
        let bootstrap = dir.path().join("threadline-installer.exe");
        fs::write(&helper, b"local verification helper").unwrap();
        fs::write(&bootstrap, b"normal bootstrap").unwrap();
        let mut extended: serde_json::Value =
            serde_json::from_slice(&fs::read(&notes).unwrap()).unwrap();
        extended["retention"] = json!({
            "fixture":{"path":"package_retention_fixture.exe","sha256": sha256(&helper).unwrap()},
            "bootstrap":{"path":"threadline-installer.exe","sha256": sha256(&bootstrap).unwrap()}
        });
        fs::write(&notes, serde_json::to_vec(&extended).unwrap()).unwrap();
        let approved = trusted_fixture(&notes, &digest).unwrap().retention.unwrap();
        assert_eq!(approved.fixture.path, helper);
        assert_eq!(approved.bootstrap.path, bootstrap);
        let predecessor_dir = dir.path().join("previous");
        fs::create_dir(&predecessor_dir).unwrap();
        let predecessor_fixture = predecessor_dir.join("package_retention_fixture.exe");
        fs::write(&predecessor_fixture, b"previous independently built seeder").unwrap();
        extended["retention"]["migration"] = json!({
            "predecessor_fixture": {"path": "previous/package_retention_fixture.exe", "sha256": sha256(&predecessor_fixture).unwrap()},
            "from_schema": 15, "to_schema": 17
        });
        fs::write(&notes, serde_json::to_vec(&extended).unwrap()).unwrap();
        let approved = trusted_fixture(&notes, &digest).unwrap().retention.unwrap();
        assert_eq!(
            approved.migration.unwrap().predecessor_fixture.path,
            predecessor_fixture
        );
        extended["retention"]["migration"]["predecessor_fixture"]["sha256"] =
            json!(sha256(&helper).unwrap());
        fs::write(&notes, serde_json::to_vec(&extended).unwrap()).unwrap();
        assert!(trusted_fixture(&notes, &digest).is_err());
        extended["retention"]
            .as_object_mut()
            .unwrap()
            .remove("migration");
        fs::write(&notes, serde_json::to_vec(&extended).unwrap()).unwrap();
        fs::write(&helper, b"substituted helper").unwrap();
        assert_eq!(
            trusted_fixture(&notes, &digest).err(),
            Some("An approved local package did not match its trusted digest.")
        );
        extended["profile_sha256"] = json!("a".repeat(64));
        fs::write(&notes, serde_json::to_vec(&extended).unwrap()).unwrap();
        assert_eq!(
            trusted_fixture(&notes, &digest).err(),
            Some(
                "Choose either the existing profile digest or a newly generated retention fixture."
            )
        );
        extended.as_object_mut().unwrap().remove("retention");
        extended.as_object_mut().unwrap().remove("profile_sha256");
        fs::write(&notes, serde_json::to_vec(&extended).unwrap()).unwrap();

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
            retention: None,
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
    fn retention_requires_a_completed_checkpoint_and_unchanged_protected_archive() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("prior-archive.sqlite3");
        fs::write(&old, b"previous owned archive").unwrap();
        let inputs = RetentionInputs {
            fixture: ExpectedPackage {
                path: "package_retention_fixture.exe".into(),
                sha256: "a".repeat(64),
            },
            bootstrap: ExpectedPackage {
                path: "threadline-installer.exe".into(),
                sha256: "b".repeat(64),
            },
            migration: None,
            preserve_files: vec![ExpectedPackage {
                path: old.clone(),
                sha256: sha256(&old).unwrap(),
            }],
        };
        let state = json!({"protected_credential_present":true,"saved_comment_page_cursor":"2","step_receipts":1,"queued_operations":8});
        let candidate = "c".repeat(64);
        let mut receipt = json!({
            "status":"Pass","retained_at_steps":STEPS.iter().map(|(name,_)|name).collect::<Vec<_>>(),"fixture_sha256":inputs.fixture.sha256,
            "bootstrap_sha256":inputs.bootstrap.sha256,"current_sha256":candidate,
            "native":{"owned_window":true,"selected_before_invocation":false,"tab_activated":true,"orderly_exit":true},
            "before":state,"after_default_uninstall":state,"default_data_absent":true,
            "closed_database_sha256":"d".repeat(64),
            "copied_checkpoint":{"source_database_sha256":"d".repeat(64),"scheduler_claimed_page":2,"next_cursor_empty":true,
                "archived_bytes_unchanged":true,"queued_payloads_and_step_receipts_unchanged":true},
            "separate_confirmed_removal":{"bootstrap_sha256":inputs.bootstrap.sha256,"normal_preview_exit":0,"normal_confirmation_exit":0,
                "conpty_prompt_verified":true,"actual_confirmation_sent_once":true,"current_db_and_backup_removed":true,
                "bound_protected_credential_removed":true,"independent_export_unchanged":true,
                "independent_credential_preserved_then_fixture_cleaned":true,"prior_retention_archive_unchanged":true,
                "default_profile_absent_before_and_after":true}
        });
        let path = dir.path().join("retained.json");
        fs::write(&path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        assert!(verified_retention(&path, &inputs, &candidate));
        let mut incomplete = receipt.clone();
        incomplete["separate_confirmed_removal"]
            .as_object_mut()
            .unwrap()
            .remove("current_db_and_backup_removed");
        fs::write(&path, serde_json::to_vec(&incomplete).unwrap()).unwrap();
        assert!(!verified_retention(&path, &inputs, &candidate));
        receipt["copied_checkpoint"]["scheduler_claimed_page"] = json!(1);
        fs::write(&path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        assert!(!verified_retention(&path, &inputs, &candidate));
        receipt["copied_checkpoint"]["scheduler_claimed_page"] = json!(2);
        fs::write(&path, serde_json::to_vec(&receipt).unwrap()).unwrap();
        fs::write(old, b"changed archive").unwrap();
        assert!(!verified_retention(&path, &inputs, &candidate));
    }

    #[test]
    fn retained_migration_requires_the_predecessor_and_its_unchanged_backup() {
        let package = |pin: &str| ExpectedPackage {
            path: "package_retention_fixture.exe".into(),
            sha256: pin.repeat(64),
        };
        let inputs = RetentionInputs {
            fixture: package("a"),
            bootstrap: package("b"),
            migration: Some(RetainedMigration {
                predecessor_fixture: package("c"),
                from_schema: 15,
                to_schema: 17,
            }),
            preserve_files: vec![],
        };
        let before = json!({"schema":15,"durable_projection_sha256":"1".repeat(64)});
        let mut after = before.clone();
        after["schema"] = json!(17);
        let record = json!({
            "before":before,"after_default_uninstall":after,
            "migration":{"predecessor_fixture_sha256":"c".repeat(64),"backup_snapshot":before,
                "backup_sha256":"2".repeat(64),"backup_after_uninstall_sha256":"2".repeat(64),
                "backup_after_cleanup_sha256":"2".repeat(64)}
        });
        assert!(verified_schema_transition(&record, &inputs));
        for (pointer, bad) in [
            ("/before/schema", json!(17)),
            ("/after_default_uninstall/schema", json!(15)),
            (
                "/after_default_uninstall/durable_projection_sha256",
                json!("changed"),
            ),
            (
                "/migration/predecessor_fixture_sha256",
                json!("a".repeat(64)),
            ),
            ("/migration/backup_snapshot/schema", json!(17)),
            ("/migration/backup_sha256", json!(null)),
            ("/migration/backup_after_uninstall_sha256", json!(null)),
            (
                "/migration/backup_after_cleanup_sha256",
                json!("f".repeat(64)),
            ),
        ] {
            let mut damaged = record.clone();
            *damaged.pointer_mut(pointer).unwrap() = bad;
            assert!(!verified_schema_transition(&damaged, &inputs), "{pointer}");
        }
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
