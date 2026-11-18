mod cli;
mod report;
mod run;

use std::{
    env,
    path::{Path, PathBuf},
    process::ExitCode,
};

use crate::report::{CaseResult, Report, Status, allocate_report_dir};

fn main() -> ExitCode {
    match run() {
        Ok(Status::Pass) => ExitCode::SUCCESS,
        Ok(Status::Fail) => ExitCode::FAILURE,
        Ok(Status::Blocked) => ExitCode::from(78),
        Err(error) => {
            eprintln!("Could not write the test report. {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> std::io::Result<Status> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args == ["--help"] {
        println!("{}", cli::USAGE);
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
    let options = match cli::parse(&args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}\n\n{}", cli::USAGE);
            let base: PathBuf = cli::report_dir_for_error(&args).unwrap_or_else(default_dir);
            let dir = allocate_report_dir(&base, "invalid-request")?;
            let mut report = Report::new("invalid-request", None, None, root);
            report
                .cases
                .push(CaseResult::new("request", Status::Fail, error));
            return report.finish(&dir);
        }
    };

    let lane = options.lane.as_str();
    let base = options.report_dir.clone().unwrap_or_else(default_dir);
    let dir = allocate_report_dir(&base, lane)?;
    let mut report = Report::new(lane, options.suite.clone(), Some(options.seed), root);
    report.cases = run::execute(&options, root, &dir);
    report.finish(&dir)
}
