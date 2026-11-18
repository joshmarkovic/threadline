use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::{Value, json};

use crate::{
    access_fixture::{AccessFixture, TraceCheck},
    cli::{Lane, Options, catalog_cases, public_path},
    report::{CaseResult, Status, sha256},
};

struct CargoTest {
    id: &'static str,
    arguments: &'static [&'static str],
}

const UNIT: &[CargoTest] = &[
    CargoTest {
        id: "UT-008",
        arguments: &["-p", "threadline-core", "--lib", "ut_008"],
    },
    CargoTest {
        id: "UT-013",
        arguments: &["-p", "threadline-core", "--lib", "ut_013"],
    },
    CargoTest {
        id: "UT-018",
        arguments: &["-p", "threadline-storage", "--lib", "ut_018"],
    },
    CargoTest {
        id: "UT-021",
        arguments: &["-p", "xtask", "--bin", "xtask"],
    },
    CargoTest {
        id: "S01-CORE",
        arguments: &["-p", "threadline-core", "--lib"],
    },
    CargoTest {
        id: "S01-DIFF-FIXTURE",
        arguments: &["-p", "threadline-app", "--lib"],
    },
    CargoTest {
        id: "UT-001",
        arguments: &["-p", "threadline-storage", "--lib", "ut_001"],
    },
    CargoTest {
        id: "UT-003",
        arguments: &["-p", "threadline-storage", "--lib", "ut_003"],
    },
    CargoTest {
        id: "UT-004",
        arguments: &["-p", "threadline-storage", "--lib", "ut_004"],
    },
    CargoTest {
        id: "UT-009",
        arguments: &["-p", "threadline-storage", "--lib", "ut_009"],
    },
    CargoTest {
        id: "UT-010",
        arguments: &["-p", "threadline-core", "--lib", "ut_010"],
    },
    CargoTest {
        id: "UT-020",
        arguments: &["-p", "threadline-core", "--lib", "ut_020"],
    },
];
const INTEGRATION: &[CargoTest] = &[
    CargoTest {
        id: "IT-003",
        arguments: &["-p", "threadline-storage", "--test", "reader", "it_003"],
    },
    CargoTest {
        id: "S01-SIMULATOR",
        arguments: &["-p", "threadline-core", "--test", "foundation"],
    },
    CargoTest {
        id: "S01-PLATFORM",
        arguments: &[
            "-p",
            "threadline-platform",
            "--lib",
            "--",
            "--skip",
            "native_synthetic_protected_store_round_trip",
            "--skip",
            "native_restart_fixture_reader",
        ],
    },
    CargoTest {
        id: "IT-001",
        arguments: &["-p", "threadline-storage", "--test", "identity"],
    },
    CargoTest {
        id: "IT-002",
        arguments: &["-p", "threadline-storage", "--test", "resources", "it_002"],
    },
    CargoTest {
        id: "S02-RESOURCE-CONTRACTS",
        arguments: &[
            "-p",
            "threadline-storage",
            "--test",
            "resources",
            "resource_",
        ],
    },
    CargoTest {
        id: "S02-DURABILITY",
        arguments: &["-p", "threadline-storage", "--test", "durability"],
    },
    CargoTest {
        id: "S02-DISK-FULL",
        arguments: &[
            "-p",
            "threadline-storage",
            "--lib",
            "disk_full_rolls_back_enqueue_and_keeps_the_saved_editor",
        ],
    },
    CargoTest {
        id: "S02-FIXTURES",
        arguments: &["-p", "threadline-storage", "--test", "fixtures"],
    },
    CargoTest {
        id: "IT-004",
        arguments: &["-p", "threadline-core", "--test", "github_access", "it_004"],
    },
    CargoTest {
        id: "IT-005",
        arguments: &["-p", "threadline-core", "--test", "github_access", "it_005"],
    },
    CargoTest {
        id: "S03-ACCOUNT-PROFILES",
        arguments: &["-p", "threadline-app", "--lib", "accounts::tests::"],
    },
    CargoTest {
        id: "S03-ACCESS-STORAGE",
        arguments: &["-p", "threadline-storage", "--test", "access"],
    },
    CargoTest {
        id: "S04-READ-TRANSPORT",
        arguments: &["-p", "threadline-core", "--test", "github_reader"],
    },
    CargoTest {
        id: "S04-CACHED-STORE",
        arguments: &["-p", "threadline-storage", "--test", "reader"],
    },
    CargoTest {
        id: "S04-READ-SERVICE",
        arguments: &[
            "-p",
            "threadline-app",
            "--lib",
            "accounts::tests::cache_reader_",
        ],
    },
];
const UI: &[CargoTest] = &[CargoTest {
    id: "S01-GPUI",
    arguments: &["-p", "threadline-app", "--test", "feasibility_ui"],
}];
const WINDOWS_CREDENTIALS: CargoTest = CargoTest {
    id: "IT-020",
    arguments: &[
        "-p",
        "threadline-platform",
        "--lib",
        "credentials::windows::tests::native_synthetic_protected_store_round_trip",
        "--",
        "--ignored",
        "--exact",
    ],
};

pub fn execute(options: &Options, root: &Path, dir: &Path) -> Vec<CaseResult> {
    match options.lane {
        Lane::Unit => run_cargo_tests(UNIT, options, root, dir),
        Lane::Integration if options.case.as_deref() == Some("IT-020") => {
            vec![if cfg!(windows) {
                run_cargo_test(&WINDOWS_CREDENTIALS, options.seed, root, dir)
            } else {
                CaseResult::new(
                    "IT-020",
                    Status::Blocked,
                    "The native protected-store fixture requires Windows.",
                )
            }]
        }
        Lane::Integration => run_cargo_tests(INTEGRATION, options, root, dir),
        Lane::Ui => run_cargo_tests(UI, options, root, dir),
        Lane::E2e => run_e2e(options, root, dir),
        Lane::Measure => vec![run_native(
            "S01-WINDOWS-BASELINE",
            NativeMode::Measure,
            options,
            root,
            dir,
        )],
        Lane::Install => run_install(options, dir),
        Lane::Live => vec![crate::live::run(options, root, dir)],
    }
}

fn run_e2e(options: &Options, root: &Path, dir: &Path) -> Vec<CaseResult> {
    const WINDOWS_CASES: &[(&str, NativeMode, &str)] = &[
        (
            "S01-WINDOWS-SHELL",
            NativeMode::WindowsShell,
            "windows-shell",
        ),
        (
            "S01-WINDOWS-TOAST",
            NativeMode::WindowsToast,
            "windows-toast",
        ),
        (
            "E2E-002",
            NativeMode::WindowsGitHubAccess,
            "windows-github-access",
        ),
        (
            "E2E-003",
            NativeMode::WindowsReaderOffline,
            "windows-reader-offline",
        ),
        (
            "E2E-004",
            NativeMode::WindowsReaderResume,
            "windows-reader-resume",
        ),
    ];
    let full = options.suite.as_deref() == Some("full");
    match options.case.as_deref() {
        Some("E2E-001") => {}
        Some(id) => {
            let mode = if id == "S04-READER-PERF" {
                Some(NativeMode::WindowsReaderPerformance)
            } else {
                WINDOWS_CASES
                    .iter()
                    .find(|case| id == case.0)
                    .map(|case| case.1)
            };
            let Some(mode) = mode else {
                return vec![unimplemented(id)];
            };
            if !full {
                return vec![CaseResult::new(
                    id,
                    Status::Fail,
                    format!("Use --suite full for {id}."),
                )];
            }
            return vec![run_native(id, mode, options, root, dir)];
        }
        None => {}
    }
    let smoke = run_native("E2E-001", NativeMode::Smoke, options, root, dir);
    let mut cases = vec![smoke];
    if options.case.is_none() && full && cfg!(windows) {
        for &(id, mode, subdir) in WINDOWS_CASES {
            let platform_dir = dir.join(subdir);
            let mut platform = match fs::create_dir(&platform_dir) {
                Ok(()) => run_native(id, mode, options, root, &platform_dir),
                Err(error) => CaseResult::new(id, Status::Fail, error.to_string()),
            };
            platform.evidence = platform.evidence.map(|path| format!("{subdir}/{path}"));
            cases.push(platform);
        }
    }
    cases
}

fn unimplemented(id: &str) -> CaseResult {
    let detail = if id == "IT-019" {
        "Full IT-019 remains a Sprint 12 gate. Use S02-DURABILITY for the Sprint 02 storage subset."
    } else {
        "This scheduled case has no implementation yet."
    };
    CaseResult::new(id, Status::Blocked, detail)
}

fn run_cargo_tests(
    tests: &[CargoTest],
    options: &Options,
    root: &Path,
    dir: &Path,
) -> Vec<CaseResult> {
    let selected: Vec<&CargoTest> = tests
        .iter()
        .filter(|test| options.case.as_deref().is_none_or(|id| id == test.id))
        .collect();
    if selected.is_empty() {
        let id = options.case.as_deref().unwrap_or("selection");
        return vec![unimplemented(id)];
    }
    selected
        .into_iter()
        .map(|test| run_cargo_test(test, options.seed, root, dir))
        .collect()
}

fn cargo(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO"));
    command.current_dir(root);
    command
}

fn run_cargo_test(test: &CargoTest, seed: u64, root: &Path, dir: &Path) -> CaseResult {
    let mut command = cargo(root);
    command
        .args(["test", "--locked"])
        .args(test.arguments)
        .env("CARGO_TERM_COLOR", "never")
        .env("THREADLINE_TEST_SEED", seed.to_string())
        .env_remove("THREADLINE_HTTP_TRACE_DIR");
    if matches!(test.id, "IT-004" | "IT-005") {
        command.env("THREADLINE_HTTP_TRACE_DIR", dir);
    }
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => return CaseResult::new(test.id, Status::Blocked, error.to_string()),
    };

    let mut case = cargo_test_result(test.id, &output.status, &output.stdout);
    match save_output(dir, test.id, &output) {
        Ok(path) => case.evidence = Some(path),
        Err(error) => case = CaseResult::new(test.id, Status::Fail, error.to_string()),
    }
    case.child_exit_code = output.status.code();

    case
}

fn cargo_test_result(id: &str, status: &std::process::ExitStatus, stdout: &[u8]) -> CaseResult {
    if !status.success() {
        return CaseResult::new(id, Status::Fail, format!("Cargo exited with {status}."));
    }
    let (passed, ignored) = harness_counts(&String::from_utf8_lossy(stdout));
    if passed == 0 || ignored != 0 {
        return CaseResult::new(
            id,
            Status::Fail,
            format!(
                "Expected a nonempty suite with no ignored tests. Passed {passed}, ignored {ignored}."
            ),
        );
    }
    let mut result = CaseResult::new(id, Status::Pass, format!("{passed} tests passed."));
    result.tests_passed = Some(passed);
    result
}

fn harness_counts(stdout: &str) -> (usize, usize) {
    let count = |suffix: &str| {
        stdout
            .lines()
            .filter_map(|line| line.strip_prefix("test result: "))
            .flat_map(|line| line.split(';'))
            .filter_map(|part| part.trim().strip_suffix(suffix))
            .filter_map(|part| part.split_whitespace().last())
            .filter_map(|value| value.parse::<usize>().ok())
            .sum()
    };
    (count(" passed"), count(" ignored"))
}

fn save_output(dir: &Path, stem: &str, output: &Output) -> io::Result<String> {
    let path = format!("{stem}.log");
    let mut log = output.stdout.clone();
    log.extend_from_slice(b"\n--- stderr ---\n");
    log.extend_from_slice(&output.stderr);
    fs::write(dir.join(&path), log)?;
    Ok(path)
}

enum NativeError {
    Blocked(String),
    Failed(String),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum NativeMode {
    Smoke,
    Measure,
    WindowsShell,
    WindowsToast,
    WindowsGitHubAccess,
    WindowsReaderOffline,
    WindowsReaderResume,
    WindowsReaderPerformance,
}

fn run_native(
    id: &str,
    mode: NativeMode,
    options: &Options,
    root: &Path,
    dir: &Path,
) -> CaseResult {
    match native_driver(id, mode, options, root, dir) {
        Ok(result) => result,
        Err(NativeError::Blocked(detail)) => CaseResult::new(id, Status::Blocked, detail),
        Err(NativeError::Failed(detail)) => CaseResult::new(id, Status::Fail, detail),
    }
}

fn native_driver(
    id: &str,
    mode: NativeMode,
    options: &Options,
    root: &Path,
    dir: &Path,
) -> Result<CaseResult, NativeError> {
    let os = std::env::consts::OS;
    match (os, mode) {
        ("windows", _) | ("linux", NativeMode::Smoke) => {}
        ("macos", NativeMode::Smoke) => {
            return Err(NativeError::Blocked(
                "macOS native input driver unavailable. Run the documented manual feasibility attempt on a macOS desktop and retain its evidence separately.".into(),
            ));
        }
        _ => {
            return Err(NativeError::Blocked(format!(
                "This native command cannot run on {os} {}.",
                std::env::consts::ARCH
            )));
        }
    }
    if std::env::consts::ARCH != "x86_64" {
        return Err(NativeError::Blocked(format!(
            "The {os} native driver currently supports x86_64 hosts."
        )));
    }
    if os == "linux" && std::env::var_os("DISPLAY").is_none() {
        return Err(NativeError::Blocked(
            "Early Linux feasibility driver needs an X11 desktop and DISPLAY.".into(),
        ));
    }

    let binary = executable(options, root, dir)?;
    let isolated = tempfile::Builder::new()
        .prefix("threadline-e2e-")
        .tempdir()
        .map_err(|error| NativeError::Blocked(error.to_string()))?;
    let reader_proof = match mode {
        NativeMode::WindowsReaderOffline => Some("Offline"),
        NativeMode::WindowsReaderResume => Some("Resume"),
        NativeMode::WindowsReaderPerformance => Some("Performance"),
        _ => None,
    };
    let access_fixture = if mode == NativeMode::WindowsGitHubAccess || reader_proof.is_some() {
        Some(AccessFixture::start(isolated.path()).map_err(|_| {
            NativeError::Blocked("The isolated loopback server could not bind.".into())
        })?)
    } else {
        None
    };
    let mut driver = if os == "windows" {
        let mut cmd = Command::new("powershell.exe");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;

            // A console window can take focus from the desktop controls under test.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        // Windows PowerShell must resolve its own inbox modules when launched through Cargo from PowerShell 7.
        cmd.env_remove("PSModulePath")
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Sta", "-File"])
            .arg(root.join("tools/native/windows-smoke.ps1"))
            .arg("-Binary")
            .arg(&binary)
            .arg("-ReportDir")
            .arg(dir)
            .arg("-ProfileDir")
            .arg(isolated.path())
            .arg("-Seed")
            .arg(options.seed.to_string());
        if mode == NativeMode::Measure {
            cmd.arg("-Measure");
            if let Some(artifact) = &options.artifact {
                let artifact = std::path::absolute(artifact).map_err(|error| {
                    NativeError::Blocked(format!("Candidate artifact: {error}"))
                })?;
                cmd.arg("-Artifact").arg(artifact);
            }
            cmd.arg("-BudgetFile")
                .arg(root.join("docs/architecture/windows-budgets.json"));
        }
        if mode == NativeMode::WindowsShell {
            cmd.arg("-PlatformProof");
        }
        if mode == NativeMode::WindowsToast {
            cmd.arg("-ToastProof");
        }
        if let Some(fixture) = &access_fixture {
            if let Some(proof) = reader_proof {
                cmd.arg("-ReaderProof").arg(proof);
            } else {
                cmd.arg("-AccessProof");
            }
            cmd.arg("-AccessAddress")
                .arg(fixture.address().to_string())
                .arg("-AccessPhaseFile")
                .arg(fixture.phase_file());
        }
        cmd
    } else {
        let mut cmd = Command::new("bash");
        cmd.arg(root.join("tools/native/linux-smoke.sh"))
            .arg(&binary)
            .arg(dir)
            .arg(isolated.path())
            .arg(options.seed.to_string());
        cmd
    };

    let output = driver.output().map_err(|error| {
        NativeError::Blocked(format!("Cannot start {os} native driver: {error}"))
    })?;
    if access_fixture.is_some() && access_output_disclosed_credential(dir, &output)? {
        return Err(NativeError::Failed(
            "The access fixture credential entered routine diagnostics.".into(),
        ));
    }
    let log = save_output(dir, "native-driver", &output)
        .map_err(|error| NativeError::Failed(error.to_string()))?;
    let driver_report = match mode {
        NativeMode::Smoke => "native.json",
        NativeMode::Measure => "measurements.json",
        NativeMode::WindowsShell => "platform-native.json",
        NativeMode::WindowsToast => "toast-native.json",
        NativeMode::WindowsGitHubAccess => "github-access-native.json",
        NativeMode::WindowsReaderOffline | NativeMode::WindowsReaderResume => "reader-native.json",
        NativeMode::WindowsReaderPerformance => "reader-performance.json",
    };
    let details = fs::read(dir.join(driver_report))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let native_status = details
        .as_ref()
        .and_then(|json| json.get("status"))
        .and_then(Value::as_str);
    let native_message = details
        .as_ref()
        .and_then(|json| json.get("detail"))
        .and_then(Value::as_str);
    let status = match (output.status.code(), native_status) {
        (Some(0), Some("Pass")) => Status::Pass,
        (Some(78), Some("Blocked")) => Status::Blocked,
        _ => Status::Fail,
    };
    let detail = format!(
        "{} See {log}.",
        native_message.unwrap_or("The driver did not return a valid completion report.")
    );
    let mut result = CaseResult::new(id, status, detail);
    result.child_exit_code = output.status.code();
    result.evidence = Some(if details.is_some() {
        driver_report.into()
    } else {
        log
    });
    if let Some(fixture) = access_fixture {
        let check = match (status, mode) {
            (Status::Pass, NativeMode::WindowsGitHubAccess) => TraceCheck::AccountJourney,
            (
                Status::Pass,
                NativeMode::WindowsReaderOffline | NativeMode::WindowsReaderPerformance,
            ) => TraceCheck::OfflineReader,
            (Status::Pass, NativeMode::WindowsReaderResume) => TraceCheck::ReaderResume,
            _ => TraceCheck::Observe,
        };
        if fixture.finish(dir, check).is_err() {
            result.status = Status::Fail;
            result.detail = "Could not verify the isolated GitHub trace.".into();
        }
    }
    Ok(result)
}

fn access_output_disclosed_credential(dir: &Path, output: &Output) -> Result<bool, NativeError> {
    let has_credential = crate::access_fixture::contains_fixture_credential;
    let mut leaked = has_credential(&output.stdout) || has_credential(&output.stderr);
    for name in [
        "github-access-native.json",
        "ui-observation.json",
        "github-access-restart-observation.json",
        "reader-native.json",
        "reader-performance.json",
        "reader-restart-observation.json",
    ] {
        let path = dir.join(name);
        if let Ok(bytes) = fs::read(&path)
            && has_credential(&bytes)
        {
            leaked = true;
            fs::remove_file(path).map_err(|_| {
                NativeError::Failed("Could not remove an unsafe test observation.".into())
            })?;
        }
    }
    Ok(leaked)
}

fn executable(options: &Options, root: &Path, dir: &Path) -> Result<PathBuf, NativeError> {
    let binary = if let Some(path) = &options.binary {
        std::path::absolute(path)
            .map_err(|error| NativeError::Blocked(format!("Native executable: {error}")))?
    } else {
        let output = cargo(root)
            .args([
                "build",
                "--locked",
                "--release",
                "-p",
                "threadline-app",
                "--features",
                "e2e",
                "--bin",
                "threadline",
                "--message-format=json",
            ])
            .output()
            .map_err(|error| NativeError::Blocked(format!("Cannot start Cargo: {error}")))?;
        let log = save_output(dir, "native-build", &output)
            .map_err(|error| NativeError::Failed(error.to_string()))?;
        if !output.status.success() {
            return Err(NativeError::Failed(format!(
                "Native build failed. See {log}."
            )));
        }
        compiler_binary(&output.stdout).ok_or_else(|| {
            NativeError::Failed(format!(
                "Cargo reported success without a binary path. See {log}."
            ))
        })?
    };

    check_executable(&binary)
        .map_err(|detail| NativeError::Blocked(format!("Native executable: {detail}")))?;
    let digest = sha256(&binary).map_err(|error| NativeError::Failed(error.to_string()))?;
    let receipt_path = options
        .binary
        .as_deref()
        .or_else(|| binary.strip_prefix(root).ok());
    let receipt_path = public_path(
        receipt_path.and_then(Path::to_str).unwrap_or_default(),
        "<binary-path>",
    );
    let identity = json!({
        "sha256": digest,
        "size_bytes": binary.metadata().ok().map(|m| m.len()),
        "binary": receipt_path,
        "build": if options.binary.is_some() { "external override" } else { "release with e2e feature" },
        "evidence_class": "test-build native process",
    });
    fs::write(
        dir.join("binary.json"),
        serde_json::to_vec_pretty(&identity)
            .map_err(|error| NativeError::Failed(error.to_string()))?,
    )
    .map_err(|error| NativeError::Failed(error.to_string()))?;
    Ok(binary)
}

fn compiler_binary(output: &[u8]) -> Option<PathBuf> {
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|message| {
            if message["reason"] == "compiler-artifact" && message["target"]["name"] == "threadline"
            {
                message["executable"].as_str().map(PathBuf::from)
            } else {
                None
            }
        })
}

fn check_executable(path: &Path) -> Result<(), String> {
    use std::io::{Seek, SeekFrom};

    let read_error = |_: io::Error| "Cannot read the native executable header.".to_owned();
    let wrong_format = || {
        format!(
            "Expected an executable for {} x86_64.",
            std::env::consts::OS
        )
    };
    let mut file = fs::File::open(path).map_err(read_error)?;
    let mut header = [0u8; 64];
    file.read_exact(&mut header).map_err(read_error)?;
    if cfg!(windows) {
        if !header.starts_with(b"MZ") {
            return Err(wrong_format());
        }
        let offset = u32::from_le_bytes(header[60..64].try_into().expect("four bytes"));
        file.seek(SeekFrom::Start(u64::from(offset)))
            .map_err(read_error)?;
        let mut coff = [0u8; 6];
        file.read_exact(&mut coff).map_err(read_error)?;
        if coff != [b'P', b'E', 0, 0, 0x64, 0x86] {
            return Err(wrong_format());
        }
    } else if !header.starts_with(b"\x7fELF\x02\x01") || header[18..20] != [0x3e, 0] {
        return Err(wrong_format());
    }
    Ok(())
}

fn run_install(options: &Options, dir: &Path) -> Vec<CaseResult> {
    let case = options.case.as_deref().unwrap_or("artifact");
    let Some(artifact) = &options.artifact else {
        return vec![CaseResult::new(case, Status::Fail, "Specify --artifact.")];
    };
    if !artifact.is_file() {
        return vec![CaseResult::new(
            case,
            Status::Blocked,
            "Candidate package is missing.",
        )];
    }
    let identity = sha256(artifact).and_then(|digest| {
        let fields = json!({
            "sha256": digest,
            "size_bytes": artifact.metadata()?.len(),
            "evidence_class": "candidate inspected only"
        });
        fs::write(
            dir.join("candidate.json"),
            serde_json::to_vec_pretty(&fields)?,
        )
    });
    if let Err(error) = identity {
        return vec![CaseResult::new(case, Status::Fail, error.to_string())];
    }
    match options.case.as_deref() {
        Some(id) => vec![blocked_install_case(id)],
        None => catalog_cases(Lane::Install)
            .map(blocked_install_case)
            .collect(),
    }
}

fn blocked_install_case(id: &str) -> CaseResult {
    let mut result = CaseResult::new(
        id,
        Status::Blocked,
        "No packaging recipe or trusted installer test adapter is available in Sprint 01.",
    );
    result.evidence = Some("candidate.json".into());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_positive_empty_and_ignored_cargo_output() {
        assert_eq!(
            harness_counts("test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured"),
            (4, 0)
        );
        assert_eq!(
            harness_counts("test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured"),
            (0, 2)
        );
        assert_eq!(harness_counts(""), (0, 0));
    }

    #[test]
    fn future_selector_yields_explicit_blockage_with_no_child_command() {
        let options = crate::cli::parse(&[
            "test".into(),
            "unit".into(),
            "--case".into(),
            "UT-002".into(),
        ])
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let outcomes = execute(&options, dir.path(), dir.path());
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].case, "UT-002");
        assert_eq!(outcomes[0].status, Status::Blocked);
        assert!(outcomes[0].evidence.is_none());
    }

    #[test]
    fn missing_install_artifact_preserves_the_requested_scenario() {
        let dir = tempfile::tempdir().unwrap();
        let mut args = ["test", "install", "--case", "PKG-001", "--artifact"]
            .map(str::to_owned)
            .to_vec();
        args.push(dir.path().join("missing").to_string_lossy().into_owned());
        let options = crate::cli::parse(&args).unwrap();
        let outcomes = execute(&options, dir.path(), dir.path());
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].case, "PKG-001");
        assert_eq!(outcomes[0].status, Status::Blocked);
    }

    #[test]
    fn registered_toast_cannot_start_from_the_smoke_suite() {
        let options = crate::cli::parse(
            &[
                "test",
                "e2e",
                "--suite",
                "smoke",
                "--case",
                "S01-WINDOWS-TOAST",
            ]
            .map(str::to_owned),
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let outcomes = execute(&options, dir.path(), dir.path());
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].case, "S01-WINDOWS-TOAST");
        assert_eq!(outcomes[0].status, Status::Fail);
        assert!(outcomes[0].detail.contains("--suite full"));
        assert!(outcomes[0].evidence.is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn registered_toast_reports_only_its_case_on_an_unsupported_host() {
        let options = crate::cli::parse(
            &[
                "test",
                "e2e",
                "--suite",
                "full",
                "--case",
                "S01-WINDOWS-TOAST",
            ]
            .map(str::to_owned),
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let outcomes = execute(&options, dir.path(), dir.path());
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].case, "S01-WINDOWS-TOAST");
        assert_eq!(outcomes[0].status, Status::Blocked);
        assert!(outcomes[0].evidence.is_none());
    }
}
