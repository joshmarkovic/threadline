mod access_fixture;
mod catalog;
mod cli;
mod live;
mod report;
mod run;

use std::{
    env,
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::ExitCode,
};

use crate::report::{
    CaseResult, Invocation, Report, Status, allocate_report_dir, unix_timestamp_ms,
};

fn main() -> ExitCode {
    match run() {
        Ok(status) => ExitCode::from(status.exit_code()),
        Err(error) => {
            let _ = writeln!(io::stderr(), "Could not write the test report. {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> std::io::Result<Status> {
    let started_at = unix_timestamp_ms();
    let args: Vec<String> = env::args_os()
        .skip(1)
        .map(|arg| arg.into_string().unwrap_or_else(|_| "\0".into()))
        .collect();
    if args == ["--help"] {
        let _ = writeln!(io::stdout(), "{}", cli::USAGE);
        return Ok(Status::Pass);
    }

    let source_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is directly inside the workspace");
    let current = env::current_dir()?;
    let root = if current.join("xtask/Cargo.toml").is_file()
        && current.join("docs/spec/test_scenarios.csv").is_file()
    {
        current.as_path()
    } else {
        source_root
    };
    let default_dir = || root.join("artifacts").join("xtask");
    let invocation = Invocation::new(&args, started_at);
    let options = match cli::parse(&args) {
        Ok(options) => options,
        Err(error) => {
            let _ = writeln!(io::stderr(), "{error}\n\n{}", cli::USAGE);
            let base: PathBuf = cli::report_dir_for_error(&args).unwrap_or_else(default_dir);
            let dir = allocate_report_dir(&base, "invalid-request")?;
            let mut report = Report::new("invalid-request", None, None, root, invocation);
            report
                .cases
                .push(CaseResult::new("request", Status::Fail, error));
            return report.finish(&dir);
        }
    };

    let lane = options.lane.as_str();
    let base = options.report_dir.clone().unwrap_or_else(default_dir);
    let dir = allocate_report_dir(&base, lane)?;
    let mut report = Report::new(
        lane,
        options.suite.clone(),
        Some(options.seed),
        root,
        invocation,
    );
    report.cases = run::execute(&options, root, &dir);
    report.finish(&dir)
}
