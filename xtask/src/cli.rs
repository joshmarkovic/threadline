use std::path::PathBuf;

pub const USAGE: &str =
    "cargo xtask test <unit|integration|ui> [--case ID] [--seed N] [--report-dir DIR]
cargo xtask test e2e --suite <smoke|full> [--binary PATH] [--case ID] [--seed N] [--report-dir DIR]
cargo xtask test e2e --suite full [--binary PATH] [--local-fixture TRUSTED_JSON] [--artifact PATH] [--seed N] [--report-dir DIR]
cargo xtask test e2e --suite full --case E2E-014 [--binary PATH] [--artifact PATH] [--seed N] [--report-dir DIR]
cargo xtask test e2e --suite full --case <E2E-012|E2E-018|E2E-020> --local-fixture TRUSTED_JSON [--report-dir DIR]
cargo xtask test install --artifact PATH [--local-fixture TRUSTED_JSON] [--case ID] [--report-dir DIR]
cargo xtask test live --repo OWNER/REPO --case LIVE-001 [--credential-stdin [--allow-existing-cli-oauth]] [--first-issue-pr-page] [--report-dir DIR]
cargo xtask test live --repo OWNER/REPO --case <LIVE-003|LIVE-004> [--allow-live-writes | --readback-only] --credential-stdin [--allow-existing-cli-oauth] [--report-dir DIR]
cargo xtask measure windows [--binary PATH] [--artifact PATH] [--seed N] [--report-dir DIR]";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    Unit,
    Integration,
    Ui,
    E2e,
    Install,
    Live,
    Measure,
}

impl Lane {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unit => "unit",
            Self::Integration => "integration",
            Self::Ui => "ui",
            Self::E2e => "e2e",
            Self::Install => "install",
            Self::Live => "live",
            Self::Measure => "measure-windows",
        }
    }
}

#[derive(Debug)]
pub struct Options {
    pub lane: Lane,
    pub case: Option<String>,
    pub seed: u64,
    pub report_dir: Option<PathBuf>,
    pub binary: Option<PathBuf>,
    pub artifact: Option<PathBuf>,
    pub local_fixture: Option<PathBuf>,
    pub repo: Option<String>,
    pub allow_live_writes: bool,
    pub readback_only: bool,
    pub credential_stdin: bool,
    pub allow_existing_cli_oauth: bool,
    pub first_issue_pr_page: bool,
    pub suite: Option<String>,
}

pub fn report_dir_for_error(args: &[String]) -> Option<PathBuf> {
    let mut matches = args
        .windows(2)
        .filter(|pair| pair[0] == "--report-dir" && is_option_value(&pair[1]))
        .map(|pair| PathBuf::from(&pair[1]));
    let value = matches.next();
    if matches.next().is_some() {
        None
    } else {
        value
    }
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    if args.iter().any(|arg| arg.contains('\0')) {
        return Err("Arguments must be valid Unicode.".into());
    }
    let lane = command_lane(args)?;

    let mut options = Options {
        lane,
        case: None,
        seed: 12345,
        report_dir: None,
        binary: None,
        artifact: None,
        local_fixture: None,
        repo: None,
        allow_live_writes: false,
        readback_only: false,
        credential_stdin: false,
        allow_existing_cli_oauth: false,
        first_issue_pr_page: false,
        suite: None,
    };
    let mut seen = std::collections::HashSet::new();
    let mut remaining = args[2..].iter();
    while let Some(flag) = remaining.next() {
        if !is_value_option(flag) && !is_switch(flag) {
            return Err("Unknown runner option.".into());
        }
        if !seen.insert(flag.as_str()) {
            return Err(format!("Duplicate option {flag}"));
        }
        if flag == "--allow-live-writes" {
            if lane != Lane::Live {
                return Err("--allow-live-writes is available only for live tests.".into());
            }
            options.allow_live_writes = true;
            continue;
        }
        if flag == "--readback-only" {
            options.readback_only = true;
            continue;
        }
        if flag == "--credential-stdin" {
            if lane != Lane::Live {
                return Err("--credential-stdin is available only for live tests.".into());
            }
            options.credential_stdin = true;
            continue;
        }
        if flag == "--allow-existing-cli-oauth" {
            if lane != Lane::Live {
                return Err("--allow-existing-cli-oauth is available only for live tests.".into());
            }
            options.allow_existing_cli_oauth = true;
            continue;
        }
        if flag == "--first-issue-pr-page" {
            if lane != Lane::Live {
                return Err("--first-issue-pr-page is available only for live tests.".into());
            }
            options.first_issue_pr_page = true;
            continue;
        }
        let value = remaining
            .next()
            .filter(|value| is_option_value(value))
            .ok_or_else(|| format!("Expected a value for {flag}"))?;
        match flag.as_str() {
            "--case" if lane != Lane::Measure => options.case = Some(value.clone()),
            "--seed" => options.seed = value.parse().map_err(|_| "Expected an unsigned seed")?,
            "--report-dir" => options.report_dir = Some(value.into()),
            "--binary" if matches!(lane, Lane::E2e | Lane::Measure) => {
                options.binary = Some(value.into());
            }
            "--artifact" if matches!(lane, Lane::E2e | Lane::Install | Lane::Measure) => {
                options.artifact = Some(value.into());
            }
            "--local-fixture" if matches!(lane, Lane::Install | Lane::E2e) => {
                options.local_fixture = Some(value.into());
            }
            "--repo" if lane == Lane::Live => {
                if !valid_repo(value) {
                    return Err("Use OWNER/REPO with valid GitHub name components".into());
                }
                options.repo = Some(value.clone());
            }
            "--suite" if lane == Lane::E2e && matches!(value.as_str(), "smoke" | "full") => {
                options.suite = Some(value.clone());
            }
            "--suite" if lane == Lane::E2e => {
                return Err("Use --suite smoke or --suite full".into());
            }
            _ => return Err(format!("{flag} is unavailable for {}", lane.as_str())),
        }
    }

    match lane {
        Lane::E2e
            if options.artifact.is_some()
                && (options.suite.as_deref() != Some("full")
                    || !matches!(options.case.as_deref(), None | Some("E2E-014"))) =>
        {
            return Err("--artifact for native tests requires --suite full, with no case or with --case E2E-014.".into());
        }
        Lane::E2e
            if options.local_fixture.is_some()
                && (!matches!(
                    options.case.as_deref(),
                    None | Some("E2E-012" | "E2E-018" | "E2E-020")
                ) || options.suite.as_deref() != Some("full")) =>
        {
            return Err(
                "Use the full suite or a named installed-package case with --local-fixture.".into(),
            );
        }
        Lane::E2e if options.suite.is_none() => return Err("Specify --suite smoke or full".into()),
        Lane::Install if options.artifact.is_none() => return Err("Specify --artifact PATH".into()),
        Lane::Live if options.repo.is_none() => return Err("Specify --repo OWNER/REPO".into()),
        _ => {}
    }
    if let Some(case) = options.case.as_deref()
        && !known_case(lane, case)
    {
        return Err(format!("Unknown {} case.", lane.as_str()));
    }
    if options.readback_only
        && (lane != Lane::Live
            || !matches!(options.case.as_deref(), Some("LIVE-003" | "LIVE-004"))
            || options.allow_live_writes)
    {
        return Err(
            "--readback-only requires LIVE-003 or LIVE-004 without write authorization.".into(),
        );
    }
    Ok(options)
}

pub fn report_arguments(args: &[String]) -> Vec<String> {
    const OMITTED: &str = "<redacted-argument>";
    let lane = command_lane(args).ok();
    let mut public = Vec::with_capacity(args.len());
    if let Some(verb) = args.first() {
        public.push(if matches!(verb.as_str(), "test" | "measure") {
            verb.as_str()
        } else {
            OMITTED
        });
    }
    if let Some(target) = args.get(1) {
        public.push(if lane.is_some() {
            target.as_str()
        } else {
            OMITTED
        });
    }
    let mut remaining = args.iter().skip(2).peekable();
    while let Some(option) = remaining.next() {
        if is_switch(option) {
            public.push(option);
        } else if is_value_option(option) {
            public.push(option);
            if remaining
                .peek()
                .is_some_and(|value| !value.starts_with("--"))
            {
                let value = remaining.next().expect("value was peeked");
                public.push(match option.as_str() {
                    "--case" if lane.is_some_and(|lane| known_case(lane, value)) => value,
                    "--seed" if value.parse::<u64>().is_ok() => value,
                    "--suite"
                        if lane == Some(Lane::E2e)
                            && matches!(value.as_str(), "smoke" | "full") =>
                    {
                        value
                    }
                    "--binary" if matches!(lane, Some(Lane::E2e | Lane::Measure)) => {
                        public_path(value, "<binary-path>")
                    }
                    "--artifact"
                        if matches!(lane, Some(Lane::E2e | Lane::Install | Lane::Measure)) =>
                    {
                        public_path(value, "<artifact-path>")
                    }
                    "--local-fixture" if matches!(lane, Some(Lane::Install | Lane::E2e)) => {
                        "<local-fixture-path>"
                    }
                    "--report-dir" => public_path(value, "<report-dir>"),
                    "--repo" if lane == Some(Lane::Live) && value == "OWNER/REPO" => value,
                    "--repo" => "<repository>",
                    _ => OMITTED,
                });
            }
        } else {
            public.push(OMITTED);
        }
    }
    public.into_iter().map(str::to_owned).collect()
}

fn command_lane(args: &[String]) -> Result<Lane, &'static str> {
    match args {
        [verb, target, ..] if verb == "measure" && target == "windows" => Ok(Lane::Measure),
        [verb, lane, ..] if verb == "test" => match lane.as_str() {
            "unit" => Ok(Lane::Unit),
            "integration" => Ok(Lane::Integration),
            "ui" => Ok(Lane::Ui),
            "e2e" => Ok(Lane::E2e),
            "install" => Ok(Lane::Install),
            "live" => Ok(Lane::Live),
            _ => Err("Unknown test lane."),
        },
        _ => Err("Expected a test lane or the Windows measurement command"),
    }
}

pub fn public_path<'a>(value: &'a str, placeholder: &'static str) -> &'a str {
    let slash_path = value.replace('\\', "/");
    let relative = slash_path.strip_prefix("./").unwrap_or(&slash_path);
    // A standard directory can still contain caller-chosen private labels.
    if matches!(
        relative.trim_end_matches('/'),
        "artifacts/verification"
            | "artifacts/xtask"
            | "docs/architecture/windows-budgets.json"
            | "windows-budgets.json"
            | "threadline"
            | "threadline.exe"
            | "target/debug/threadline"
            | "target/debug/threadline.exe"
            | "target/release/threadline"
            | "target/release/threadline.exe"
    ) {
        value
    } else {
        placeholder
    }
}

fn is_value_option(option: &str) -> bool {
    matches!(
        option,
        "--case"
            | "--seed"
            | "--report-dir"
            | "--binary"
            | "--artifact"
            | "--local-fixture"
            | "--repo"
            | "--suite"
    )
}

fn is_switch(option: &str) -> bool {
    matches!(
        option,
        "--allow-live-writes"
            | "--readback-only"
            | "--credential-stdin"
            | "--allow-existing-cli-oauth"
            | "--first-issue-pr-page"
    )
}

fn is_option_value(value: &str) -> bool {
    !value.is_empty() && !value.starts_with("--") && !value.contains('\0')
}

fn valid_repo(value: &str) -> bool {
    let valid = |name: &str| {
        !name.is_empty()
            && name != "."
            && name != ".."
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    };
    value
        .split_once('/')
        .is_some_and(|(owner, repo)| valid(owner) && valid(repo))
}

pub fn catalog_cases(lane: Lane) -> impl Iterator<Item = &'static str> {
    let prefix = match lane {
        Lane::Unit => "UT-",
        Lane::Integration => "IT-",
        Lane::Ui => "UI-",
        Lane::E2e => "E2E-",
        Lane::Install => "PKG-",
        Lane::Live => "LIVE-",
        Lane::Measure => "",
    };
    crate::catalog::scenario_ids().filter(move |id| id.starts_with(prefix))
}

fn known_case(lane: Lane, case: &str) -> bool {
    matches!(
        (lane, case),
        (Lane::Unit, "S01-CORE")
            | (Lane::Unit, "S01-DIFF-FIXTURE")
            | (Lane::Integration, "S01-SIMULATOR")
            | (Lane::Integration, "S01-PLATFORM")
            | (Lane::Integration, "S02-RESOURCE-CONTRACTS")
            | (Lane::Integration, "S02-DURABILITY")
            | (Lane::Integration, "S02-DISK-FULL")
            | (Lane::Integration, "S02-FIXTURES")
            | (Lane::Integration, "S03-ACCOUNT-PROFILES")
            | (Lane::Integration, "S03-ACCESS-STORAGE")
            | (
                Lane::Integration,
                "S10-HISTORY-HTTP" | "S10-HISTORY-MONITOR"
            )
            | (
                Lane::Integration,
                "S04-READ-TRANSPORT" | "S04-CACHED-STORE" | "S04-READ-SERVICE"
            )
            | (Lane::Ui, "S01-GPUI")
            | (
                Lane::Integration,
                "S05-FEED-HTTP" | "S05-SYNC-WORKER" | "S07-WRITE-WORKER"
            )
            | (Lane::E2e, "S01-WINDOWS-SHELL")
            | (
                Lane::E2e,
                "S04-READER-PERF"
                    | "S12-PHYSICAL-READER"
                    | "S12-READER-CONCURRENT"
                    | "S12-HISTORICAL-READER"
                    | "S12-NATIVE-LIFECYCLE"
            )
            | (Lane::E2e, "S05-WINDOWS-SYNC" | "S05-WINDOWS-RATE-LIMIT")
            | (Lane::E2e, "S06-SHARED-PR-PANES")
            | (Lane::E2e, "S09-MERGE-QUEUE")
            | (Lane::E2e, "S08-REVIEW-DECISIONS")
            | (Lane::E2e, "S08-THREAD-QUEUE")
            | (Lane::E2e, "S08-COMMENT-DEPENDENCY")
            | (Lane::E2e, "S07-ISSUE-CONFLICT")
            | (Lane::E2e, "S08-REVIEW-PREDECESSOR")
            | (Lane::E2e, "S11-NATIVE-ACTIVITY")
            | (Lane::E2e, "S01-WINDOWS-TOAST")
            | (Lane::Install, crate::install::LOCAL_CASE)
    ) || catalog_cases(lane).any(|id| id == case)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).into()).collect()
    }

    #[test]
    fn historical_reader_is_admitted_in_the_native_e2e_lane() {
        let case = "S12-HISTORICAL-READER";
        let options = parse(&arguments(&[
            "test", "e2e", "--suite", "full", "--case", case,
        ]))
        .unwrap();
        assert_eq!(options.case.as_deref(), Some(case));
        assert!(parse(&arguments(&["test", "integration", "--case", case])).is_err());
    }

    #[test]
    fn accepts_the_shared_pr_panes_in_the_native_full_suite() {
        let id = "S06-SHARED-PR-PANES";
        let options = parse(&arguments(&[
            "test", "e2e", "--suite", "full", "--case", id,
        ]))
        .unwrap();
        assert_eq!(options.case.as_deref(), Some(id));
        assert!(parse(&arguments(&["test", "integration", "--case", id])).is_err());
    }

    #[test]
    fn accepts_issue_conflict_only_in_the_native_lane() {
        let id = "S07-ISSUE-CONFLICT";
        let args = arguments(&["test", "e2e", "--suite", "full", "--case", id]);
        assert_eq!(parse(&args).unwrap().case.as_deref(), Some(id));
        assert!(parse(&arguments(&["test", "integration", "--case", id])).is_err());
    }

    #[test]
    fn accepts_the_installed_runtime_fixture_for_its_named_full_case() {
        let options = parse(&arguments(&[
            "test",
            "e2e",
            "--suite",
            "full",
            "--case",
            "E2E-020",
            "--local-fixture",
            "trusted.json",
        ]))
        .unwrap();
        assert_eq!(options.local_fixture, Some("trusted.json".into()));
        assert_eq!(options.case.as_deref(), Some("E2E-020"));
    }

    #[test]
    fn accepts_the_documented_seed_and_scenario() {
        let options = parse(&arguments(&[
            "test",
            "unit",
            "--case",
            "UT-021",
            "--seed",
            "27",
            "--report-dir",
            "output",
        ]))
        .unwrap();
        assert_eq!(options.case.as_deref(), Some("UT-021"));
        assert_eq!(options.seed, 27);
        assert_eq!(options.report_dir, Some("output".into()));
    }

    #[test]
    fn native_artifact_is_only_for_the_full_suite_or_named_large_workspace() {
        let input = |suite, case, artifact| {
            arguments(&[
                "test",
                "e2e",
                "--artifact",
                artifact,
                "--case",
                case,
                "--suite",
                suite,
            ])
        };
        let named = input("full", "E2E-014", "docs/architecture/windows-budgets.json");
        assert_eq!(
            parse(&named).unwrap().artifact.unwrap(),
            PathBuf::from(&named[3])
        );
        assert_eq!(report_arguments(&named), named);
        for (suite, case) in [
            ("full", "E2E-015"),
            ("full", "E2E-012"),
            ("smoke", "E2E-014"),
        ] {
            let invalid = input(suite, case, "private-fixtures/unnamed-test.zip");
            assert!(parse(&invalid).is_err());
            assert_eq!(report_arguments(&invalid)[3], "<artifact-path>");
        }
        let aggregate = arguments(&[
            "test",
            "e2e",
            "--suite",
            "full",
            "--binary",
            "target/release/threadline.exe",
            "--local-fixture",
            "/private-fixtures/installed.json",
            "--artifact",
            "/private-fixtures/ThreadlineSetup.exe",
        ]);
        let parsed = parse(&aggregate).unwrap();
        assert!(parsed.case.is_none());
        assert_eq!(
            parsed.artifact,
            Some("/private-fixtures/ThreadlineSetup.exe".into())
        );
        assert_eq!(
            parsed.local_fixture,
            Some("/private-fixtures/installed.json".into())
        );
        let receipt = report_arguments(&aggregate);
        assert_eq!(receipt[7], "<local-fixture-path>");
        assert_eq!(receipt[9], "<artifact-path>");

        let smoke = arguments(&[
            "test",
            "e2e",
            "--suite",
            "smoke",
            "--artifact",
            "ThreadlineSetup.exe",
        ]);
        assert!(parse(&smoke).is_err());
    }

    #[test]
    fn keeps_safe_paths_values_and_option_order_in_the_receipt() {
        let args = arguments(&[
            "test",
            "install",
            "--report-dir",
            "artifacts/verification",
            "--case",
            "PKG-001",
            "--artifact",
            "docs/architecture/windows-budgets.json",
            "--seed",
            "00027",
        ]);
        assert!(parse(&args).is_ok());
        assert_eq!(report_arguments(&args), args);
        let args = arguments(&[
            "test",
            "e2e",
            "--binary",
            "target\\release\\threadline.exe",
            "--suite",
            "full",
        ]);
        assert!(parse(&args).is_ok());
        assert_eq!(report_arguments(&args), args);
    }

    #[test]
    fn protects_private_inputs_and_rejected_arguments() {
        let args = arguments(&[
            "test",
            "live",
            "--repo",
            "example/internal-repo",
            "--report-dir",
            "/home/user/internal",
        ]);
        assert!(parse(&args).is_ok());
        assert_eq!(
            report_arguments(&args),
            [
                "test",
                "live",
                "--repo",
                "<repository>",
                "--report-dir",
                "<report-dir>"
            ]
        );

        let args = arguments(&[
            "test",
            "unit",
            "--case",
            "secret-unknown-id",
            "--seed",
            "17",
            "--ghp_example",
            "another-secret",
        ]);
        let error = parse(&args).unwrap_err();
        assert_eq!(error, "Unknown runner option.");
        assert_eq!(
            report_arguments(&args),
            [
                "test",
                "unit",
                "--case",
                "<redacted-argument>",
                "--seed",
                "17",
                "<redacted-argument>",
                "<redacted-argument>",
            ]
        );
        assert_eq!(
            public_path("artifacts/probe/github_pat_fixture.json", "<artifact-path>"),
            "<artifact-path>"
        );
        assert_eq!(
            public_path("docs/alice-client", "<artifact-path>"),
            "<artifact-path>"
        );
        assert_eq!(
            public_path("reports/acme-confidential", "<report-dir>"),
            "<report-dir>"
        );
    }

    #[test]
    fn rejects_unknown_mismatched_and_duplicate_selectors() {
        for args in [
            &["test", "unit", "--case", "UT-999"][..],
            &["test", "unit", "--case", "E2E-001"],
            &["test", "unit", "--case", "UT-021", "--case", "UT-001"],
            &["test", "unit", "--typo"],
            &["test", "unit", "--binary", "test.exe"],
            &["test", "e2e", "--suite", "native"],
            &["test", "e2e"],
            &["test", "install"],
            &["test", "live", "--repo", "owner/repo/extra"],
            &["test", "live", "--repo", "owner/.."],
        ] {
            assert!(parse(&arguments(args)).is_err(), "{args:?}");
        }
    }

    #[test]
    fn knows_named_future_scenarios_so_they_can_be_reported_blocked() {
        assert!(
            parse(&arguments(&[
                "test", "e2e", "--suite", "full", "--case", "E2E-006",
            ]))
            .is_ok()
        );
        assert_eq!(catalog_cases(Lane::Live).count(), 4);
    }

    #[test]
    fn accepts_the_registered_toast_case_as_native_e2e() {
        let args = arguments(&[
            "test",
            "e2e",
            "--suite",
            "full",
            "--case",
            "S01-WINDOWS-TOAST",
        ]);
        assert_eq!(
            parse(&args).unwrap().case.as_deref(),
            Some("S01-WINDOWS-TOAST")
        );
        assert!(parse(&arguments(&["test", "unit", "--case", "S01-WINDOWS-TOAST"])).is_err());
    }
}
