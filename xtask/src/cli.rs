use std::path::PathBuf;

const SCENARIOS: &str = include_str!("../../docs/spec/test_scenarios.csv");

pub const USAGE: &str =
    "cargo xtask test <unit|integration|ui> [--case ID] [--seed N] [--report-dir DIR]
cargo xtask test e2e --suite <smoke|full> [--binary PATH] [--case ID] [--seed N] [--report-dir DIR]
cargo xtask test install --artifact PATH [--case ID] [--report-dir DIR]
cargo xtask test live --repo OWNER/REPO [--allow-live-writes] [--case ID] [--report-dir DIR]
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
    pub repo: Option<String>,
    pub allow_live_writes: bool,
    pub suite: Option<String>,
}

pub fn report_dir_for_error(args: &[String]) -> Option<PathBuf> {
    let mut matches = args
        .windows(2)
        .filter(|pair| pair[0] == "--report-dir" && !pair[1].starts_with("--"))
        .map(|pair| PathBuf::from(&pair[1]));
    let value = matches.next();
    if matches.next().is_some() {
        None
    } else {
        value
    }
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    let lane = match args {
        [verb, target, ..] if verb == "measure" && target == "windows" => Lane::Measure,
        [verb, lane, ..] if verb == "test" => match lane.as_str() {
            "unit" => Lane::Unit,
            "integration" => Lane::Integration,
            "ui" => Lane::Ui,
            "e2e" => Lane::E2e,
            "install" => Lane::Install,
            "live" => Lane::Live,
            _ => return Err(format!("Unknown test lane {lane}")),
        },
        _ => return Err("Expected a test lane or the Windows measurement command".into()),
    };

    let mut options = Options {
        lane,
        case: None,
        seed: 12345,
        report_dir: None,
        binary: None,
        artifact: None,
        repo: None,
        allow_live_writes: false,
        suite: None,
    };
    let mut seen = std::collections::HashSet::new();
    let mut remaining = args[2..].iter();
    while let Some(flag) = remaining.next() {
        if !seen.insert(flag.as_str()) {
            return Err(format!("Duplicate option {flag}"));
        }
        if flag == "--allow-live-writes" && lane == Lane::Live {
            options.allow_live_writes = true;
            continue;
        }
        if !matches!(
            flag.as_str(),
            "--case" | "--seed" | "--report-dir" | "--binary" | "--artifact" | "--repo" | "--suite"
        ) {
            return Err(format!("Unknown option {flag}"));
        }
        let value = remaining
            .next()
            .filter(|value| !value.is_empty() && !value.starts_with("--"))
            .ok_or_else(|| format!("Expected a value for {flag}"))?;
        match flag.as_str() {
            "--case" if lane != Lane::Measure => options.case = Some(value.clone()),
            "--seed" => options.seed = value.parse().map_err(|_| "Expected an unsigned seed")?,
            "--report-dir" => options.report_dir = Some(value.into()),
            "--binary" if matches!(lane, Lane::E2e | Lane::Measure) => {
                options.binary = Some(value.into());
            }
            "--artifact" if matches!(lane, Lane::Install | Lane::Measure) => {
                options.artifact = Some(value.into());
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
        Lane::E2e if options.suite.is_none() => return Err("Specify --suite smoke or full".into()),
        Lane::Install if options.artifact.is_none() => return Err("Specify --artifact PATH".into()),
        Lane::Live if options.repo.is_none() => return Err("Specify --repo OWNER/REPO".into()),
        _ => {}
    }
    if let Some(case) = options.case.as_deref()
        && !known_case(lane, case)
    {
        return Err(format!("Unknown {} case {case}", lane.as_str()));
    }
    Ok(options)
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
    SCENARIOS
        .lines()
        .skip(1)
        .filter_map(|row| row.split_once(',').map(|(id, _)| id))
        .filter(move |id| id.starts_with(prefix))
}

fn known_case(lane: Lane, case: &str) -> bool {
    matches!(
        (lane, case),
        (Lane::Unit, "S01-CORE")
            | (Lane::Unit, "S01-DIFF-FIXTURE")
            | (Lane::Integration, "S01-SIMULATOR")
            | (Lane::Integration, "S01-PLATFORM")
            | (Lane::Ui, "S01-GPUI")
            | (Lane::E2e, "S01-WINDOWS-SHELL")
    ) || catalog_cases(lane).any(|id| id == case)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).into()).collect()
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
}
