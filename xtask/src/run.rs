use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde_json::{Value, json};

use crate::{
    access_fixture::{AccessFixture, TraceCheck},
    cli::{Lane, Options, public_path},
    report::{CaseResult, Status, sha256},
};

struct CargoTest {
    id: &'static str,
    arguments: &'static [&'static str],
}

const UNIT: &[CargoTest] = &[
    CargoTest {
        id: "UT-002",
        arguments: &["-p", "threadline-core", "--lib", "ut_002"],
    },
    CargoTest {
        id: "UT-005",
        arguments: &["-p", "threadline-core", "--lib", "ut_005"],
    },
    CargoTest {
        id: "UT-006",
        arguments: &["-p", "threadline-core", "--lib", "ut_006"],
    },
    CargoTest {
        id: "UT-007",
        arguments: &["-p", "threadline-core", "--lib", "ut_007"],
    },
    CargoTest {
        id: "UT-011",
        arguments: &["-p", "threadline-storage", "--lib", "ut_011"],
    },
    CargoTest {
        id: "UT-012",
        arguments: &["-p", "threadline-core", "--lib", "ut_012"],
    },
    CargoTest {
        id: "UT-008",
        arguments: &["-p", "threadline-core", "--lib", "ut_008"],
    },
    CargoTest {
        id: "UT-013",
        arguments: &["-p", "threadline-core", "--lib", "ut_013"],
    },
    CargoTest {
        id: "UT-014",
        arguments: &["-p", "threadline-storage", "--lib", "ut_014"],
    },
    CargoTest {
        id: "UT-015",
        arguments: &["-p", "threadline-storage", "--lib", "ut_015"],
    },
    CargoTest {
        id: "UT-016",
        arguments: &["-p", "threadline-storage", "--lib", "ut_016"],
    },
    CargoTest {
        id: "UT-017",
        arguments: &["-p", "threadline-storage", "--lib", "ut_017"],
    },
    CargoTest {
        id: "UT-018",
        arguments: &["-p", "threadline-storage", "--lib", "ut_018"],
    },
    CargoTest {
        id: "UT-019",
        arguments: &["-p", "threadline-installer", "--lib", "ut_019"],
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
        id: "IT-021",
        arguments: &[
            "-p",
            "threadline-installer",
            "--lib",
            "--test",
            "bootstrap_command",
            "it_021",
        ],
    },
    CargoTest {
        id: "IT-018",
        arguments: &[
            "-p",
            "threadline-storage",
            "--test",
            "sync",
            "--test",
            "notifications",
            "--test",
            "pr",
            "it_018",
        ],
    },
    CargoTest {
        id: "IT-019",
        arguments: &[
            "-p",
            "threadline-storage",
            "--test",
            "identity",
            "--test",
            "durability",
            "it_019",
        ],
    },
    CargoTest {
        id: "IT-006",
        arguments: &[
            "-p",
            "threadline-storage",
            "--test",
            "sync",
            "--test",
            "pr",
            "--test",
            "history",
            "--test",
            "history_monitor",
            "it_006",
        ],
    },
    CargoTest {
        id: "IT-007",
        arguments: &["-p", "threadline-storage", "--test", "sync", "it_007"],
    },
    CargoTest {
        id: "S10-HISTORY-HTTP",
        arguments: &["-p", "threadline-core", "--test", "github_history"],
    },
    CargoTest {
        id: "S10-HISTORY-MONITOR",
        arguments: &[
            "-p",
            "threadline-app",
            "--lib",
            "accounts::tests::monitored_history_",
        ],
    },
    CargoTest {
        id: "S05-FEED-HTTP",
        arguments: &[
            "-p",
            "threadline-core",
            "--test",
            "github_reader",
            "sync_feeds_",
        ],
    },
    CargoTest {
        id: "S05-SYNC-WORKER",
        arguments: &[
            "-p",
            "threadline-app",
            "--lib",
            "accounts::tests::sync_worker_",
        ],
    },
    CargoTest {
        id: "IT-008",
        arguments: &[
            "-p",
            "threadline-storage",
            "-p",
            "threadline-core",
            "--test",
            "writes",
            "--test",
            "github_writes",
            "it_008",
        ],
    },
    CargoTest {
        id: "IT-009",
        arguments: &[
            "-p",
            "threadline-storage",
            "-p",
            "threadline-core",
            "--lib",
            "--test",
            "writes",
            "--test",
            "github_writes",
            "it_009",
        ],
    },
    CargoTest {
        id: "IT-010",
        arguments: &["-p", "threadline-storage", "--test", "writes", "it_010"],
    },
    CargoTest {
        id: "IT-011",
        arguments: &[
            "-p",
            "threadline-storage",
            "-p",
            "threadline-core",
            "--test",
            "writes",
            "--test",
            "reviews",
            "--test",
            "github_review_writes",
            "it_011",
        ],
    },
    CargoTest {
        id: "IT-013",
        arguments: &[
            "-p",
            "threadline-storage",
            "-p",
            "threadline-core",
            "--test",
            "reviews",
            "--test",
            "github_review_writes",
            "it_013",
        ],
    },
    CargoTest {
        id: "IT-012",
        arguments: &["-p", "threadline-storage", "--test", "writes", "it_012"],
    },
    CargoTest {
        id: "S07-WRITE-WORKER",
        arguments: &["-p", "threadline-app", "--lib", "write_dispatch_"],
    },
    CargoTest {
        id: "IT-014",
        arguments: &[
            "-p",
            "threadline-storage",
            "-p",
            "threadline-core",
            "--test",
            "merges",
            "--test",
            "github_merge",
            "it_014",
        ],
    },
    CargoTest {
        id: "IT-015",
        arguments: &["-p", "threadline-storage", "--test", "merges", "it_015"],
    },
    CargoTest {
        id: "IT-016",
        arguments: &["-p", "threadline-storage", "--test", "pr", "it_016"],
    },
    CargoTest {
        id: "IT-017",
        arguments: &["-p", "threadline-storage", "--test", "pr", "it_017"],
    },
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
const UI: &[CargoTest] = &[
    CargoTest {
        id: "UI-004",
        arguments: &[
            "-p",
            "threadline-app",
            "--features",
            "e2e",
            "--lib",
            "ui_004_",
        ],
    },
    CargoTest {
        id: "S01-GPUI",
        arguments: &["-p", "threadline-app", "--test", "feasibility_ui"],
    },
    CargoTest {
        id: "UI-001",
        arguments: &[
            "-p",
            "threadline-app",
            "--features",
            "e2e",
            "--lib",
            "ui_001_",
        ],
    },
    CargoTest {
        id: "UI-002",
        arguments: &[
            "-p",
            "threadline-app",
            "--features",
            "e2e",
            "--lib",
            "ui_002_",
        ],
    },
    CargoTest {
        id: "UI-003",
        arguments: &[
            "-p",
            "threadline-app",
            "--features",
            "e2e",
            "--lib",
            "ui_003_",
        ],
    },
    CargoTest {
        id: "UI-006",
        arguments: &[
            "-p",
            "threadline-app",
            "--features",
            "e2e",
            "--lib",
            "ui_006_",
        ],
    },
    CargoTest {
        id: "UI-005",
        arguments: &[
            "-p",
            "threadline-app",
            "--features",
            "e2e",
            "--lib",
            "ui_005_",
        ],
    },
    CargoTest {
        id: "UI-008",
        arguments: &[
            "-p",
            "threadline-app",
            "--features",
            "e2e",
            "--lib",
            "ui_008_",
        ],
    },
    CargoTest {
        id: "UI-007",
        arguments: &[
            "-p",
            "threadline-app",
            "--features",
            "e2e",
            "--lib",
            "ui_007_",
        ],
    },
];
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
        Lane::Install => crate::install::run(options, root, dir),
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
        ("S05-WINDOWS-SYNC", NativeMode::WindowsSync, "windows-sync"),
        (
            "E2E-013",
            NativeMode::WindowsNotificationSchedule,
            "windows-notification-schedule",
        ),
        ("E2E-005", NativeMode::WindowsPullRequest, "windows-pr"),
        (
            "E2E-007",
            NativeMode::WindowsReviewDraft,
            "windows-review-draft",
        ),
        ("E2E-011", NativeMode::WindowsHistory, "windows-history"),
        (
            "E2E-010",
            NativeMode::WindowsCredentialBoundary,
            "windows-credential-boundary",
        ),
        (
            "E2E-009",
            NativeMode::WindowsDirectMerge,
            "windows-direct-merge",
        ),
        (
            "E2E-008",
            NativeMode::WindowsReviewContext,
            "windows-review-context",
        ),
        (
            "E2E-014",
            NativeMode::WindowsLargeWorkspace,
            "windows-large-workspace",
        ),
        (
            "E2E-015",
            NativeMode::WindowsAccessibility,
            "windows-accessibility",
        ),
        ("E2E-006", NativeMode::WindowsIssueWrites, "windows-writes"),
        (
            "E2E-019",
            NativeMode::WindowsWriteSurface,
            "windows-write-surface",
        ),
    ];
    let full = options.suite.as_deref() == Some("full");
    match options.case.as_deref() {
        Some("E2E-001") => {}
        Some(id @ ("E2E-012" | "E2E-018" | "E2E-020")) => {
            return vec![crate::activity::run(id, options, root, dir)];
        }
        Some(id) => {
            let mode = if matches!(id, "S04-READER-PERF" | "S12-PHYSICAL-READER") {
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
        for (id, name) in [
            ("E2E-012", "windows-activity-cold"),
            ("E2E-018", "windows-activity-fallback"),
            ("E2E-020", "windows-installed-runtime"),
        ] {
            let subdir = dir.join(name);
            let mut result = match fs::create_dir(&subdir) {
                Ok(()) => crate::activity::run(id, options, root, &subdir),
                Err(error) => CaseResult::new(id, Status::Fail, error.to_string()),
            };
            result.evidence = result.evidence.map(|path| format!("{name}/{path}"));
            cases.push(result);
        }
    }
    cases
}

fn unimplemented(id: &str) -> CaseResult {
    CaseResult::new(
        id,
        Status::Blocked,
        "This scheduled case has no implementation yet.",
    )
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
    WindowsLargeWorkspace,
    WindowsAccessibility,
    WindowsSync,
    WindowsNotificationSchedule,
    WindowsPullRequest,
    WindowsHistory,
    WindowsReviewDraft,
    WindowsReviewContext,
    WindowsDirectMerge,
    WindowsCredentialBoundary,
    WindowsIssueWrites,
    WindowsWriteSurface,
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
        NativeMode::WindowsReaderPerformance if id == "S12-PHYSICAL-READER" => {
            Some("PerformancePhysical")
        }
        NativeMode::WindowsReaderPerformance => Some("Performance"),
        NativeMode::WindowsLargeWorkspace => Some("LargeWorkspace"),
        NativeMode::WindowsSync => Some("Sync"),
        NativeMode::WindowsNotificationSchedule => Some("NotificationSchedule"),
        NativeMode::WindowsPullRequest => Some("PullRequest"),
        NativeMode::WindowsHistory => Some("History"),
        NativeMode::WindowsReviewDraft => Some("ReviewDraft"),
        NativeMode::WindowsReviewContext => Some("ReviewContext"),
        NativeMode::WindowsDirectMerge => Some("DirectMerge"),
        NativeMode::WindowsCredentialBoundary => Some("CredentialBoundary"),
        NativeMode::WindowsIssueWrites => Some("Writes"),
        NativeMode::WindowsWriteSurface => Some("WriteSurface"),
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
        }
        if matches!(
            mode,
            NativeMode::Measure | NativeMode::WindowsLargeWorkspace
        ) {
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
        if mode == NativeMode::WindowsAccessibility {
            cmd.arg("-AccessibilityProof");
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
        NativeMode::WindowsLargeWorkspace => "large-workspace-native.json",
        NativeMode::WindowsAccessibility => "accessibility-native.json",
        NativeMode::WindowsSync => "sync-native.json",
        NativeMode::WindowsNotificationSchedule => "notification-schedule-native.json",
        NativeMode::WindowsPullRequest => "pr-native.json",
        NativeMode::WindowsHistory => "history-native.json",
        NativeMode::WindowsReviewDraft => "review-draft-native.json",
        NativeMode::WindowsReviewContext => "review-context-native.json",
        NativeMode::WindowsDirectMerge => "direct-merge-native.json",
        NativeMode::WindowsCredentialBoundary => "credential-boundary-native.json",
        NativeMode::WindowsIssueWrites | NativeMode::WindowsWriteSurface => "writes-native.json",
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
            (_, NativeMode::WindowsLargeWorkspace)
                if details
                    .as_ref()
                    .is_some_and(|json| json["backlog_checks_passed"] == true) =>
            {
                TraceCheck::LargeWorkspaceBacklog
            }
            (Status::Pass, NativeMode::WindowsReaderResume) => TraceCheck::ReaderResume,
            (Status::Pass, NativeMode::WindowsSync) => TraceCheck::SyncJourney,
            (Status::Pass, NativeMode::WindowsNotificationSchedule) => {
                TraceCheck::NotificationSchedule
            }
            (Status::Pass, NativeMode::WindowsPullRequest) => TraceCheck::PullRequestJourney,
            (Status::Pass, NativeMode::WindowsHistory) => TraceCheck::HistoryJourney,
            (Status::Pass, NativeMode::WindowsReviewDraft) => TraceCheck::ReviewDraftJourney,
            (Status::Pass, NativeMode::WindowsReviewContext) => TraceCheck::ReviewContextJourney,
            (Status::Pass, NativeMode::WindowsDirectMerge) => TraceCheck::MergeJourney,
            (Status::Pass, NativeMode::WindowsCredentialBoundary) => TraceCheck::CredentialBoundary,
            (Status::Pass, NativeMode::WindowsIssueWrites) => TraceCheck::WritesWithLoss,
            (Status::Pass, NativeMode::WindowsWriteSurface) => TraceCheck::WriteSurface,
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
        "sync-native.json",
        "pr-native.json",
        "writes-native.json",
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
            "UT-014".into(),
        ])
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let outcomes = run_cargo_tests(&[], &options, dir.path(), dir.path());
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].case, "UT-014");
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
