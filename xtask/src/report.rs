use std::{
    fmt::Write as _,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum Status {
    Pass,
    Fail,
    Blocked,
}

#[derive(Debug, Serialize)]
pub struct CaseResult {
    pub case: String,
    pub status: Status,
    pub detail: String,
    pub tests_passed: Option<usize>,
    pub evidence: Option<String>,
}

impl CaseResult {
    pub fn new(case: &str, status: Status, detail: impl Into<String>) -> Self {
        Self {
            case: case.into(),
            status,
            detail: detail.into(),
            tests_passed: None,
            evidence: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Host {
    os: &'static str,
    architecture: &'static str,
    version: Option<String>,
    display_session: &'static str,
}

impl Host {
    pub fn current() -> Self {
        let (version, display_session) = match std::env::consts::OS {
            "windows" => (
                command_text("cmd.exe", &["/d", "/c", "ver"]),
                "Windows desktop availability is checked by native driver",
            ),
            "macos" => (
                command_text("sw_vers", &["-productVersion"]),
                "macOS desktop availability is checked by native driver",
            ),
            _ => {
                let mode = match (
                    std::env::var_os("WAYLAND_DISPLAY").is_some(),
                    std::env::var_os("DISPLAY").is_some(),
                ) {
                    (true, true) => "Wayland and X11 environment",
                    (true, false) => "Wayland environment",
                    (false, true) => "X11 environment",
                    (false, false) => "No graphical display environment",
                };
                (command_text("uname", &["-sr"]), mode)
            }
        };
        Self {
            os: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            version,
            display_session,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: u8,
    pub lane: String,
    pub suite: Option<String>,
    pub seed: Option<u64>,
    pub host: Host,
    pub source_revision: Option<String>,
    pub source_dirty: Option<bool>,
    pub cargo_lock_sha256: Option<String>,
    pub package_version: &'static str,
    pub fixture_revision: &'static str,
    pub status: Status,
    pub cases: Vec<CaseResult>,
}

impl Report {
    pub fn new(lane: &str, suite: Option<String>, seed: Option<u64>, root: &Path) -> Self {
        let source_revision = git_text(root, &["rev-parse", "HEAD"]);
        let source_dirty = git_text(root, &["status", "--porcelain"]).map(|s| !s.is_empty());
        Self {
            schema: 1,
            lane: lane.into(),
            suite,
            seed,
            host: Host::current(),
            source_revision,
            source_dirty,
            cargo_lock_sha256: sha256(&root.join("Cargo.lock")).ok(),
            package_version: env!("CARGO_PKG_VERSION"),
            fixture_revision: "Sprint 01 synthetic feasibility fixture",
            status: Status::Fail,
            cases: Vec::new(),
        }
    }

    pub fn finish(mut self, dir: &Path) -> io::Result<Status> {
        self.status = combined_status(&self.cases);
        let report_path = dir.join("result.json");
        let document = serde_json::to_vec_pretty(&self)?;
        fs::write(&report_path, document)?;
        fs::write(dir.join("junit.xml"), self.junit())?;
        for case in &self.cases {
            println!("{}: {:?} — {}", case.case, case.status, case.detail);
        }
        println!(
            "{}: {:?} ({} cases). Results: {}",
            self.lane,
            self.status,
            self.cases.len(),
            report_path.display()
        );
        Ok(self.status)
    }

    fn junit(&self) -> String {
        let failed = self
            .cases
            .iter()
            .filter(|case| case.status == Status::Fail)
            .count();
        let skipped = self
            .cases
            .iter()
            .filter(|case| case.status == Status::Blocked)
            .count();
        let lane = xml_escape(&self.lane);
        let mut xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuite name=\"{lane}\" tests=\"{}\" failures=\"{failed}\" skipped=\"{skipped}\">\n",
            self.cases.len()
        );
        for case in &self.cases {
            let id = xml_escape(&case.case);
            let detail = xml_escape(&case.detail);
            let result = match case.status {
                Status::Pass => String::new(),
                Status::Fail => format!("<failure message=\"{detail}\" />"),
                Status::Blocked => format!("<skipped message=\"Blocked: {detail}\" />"),
            };
            writeln!(
                &mut xml,
                "  <testcase classname=\"{lane}\" name=\"{id}\">{result}</testcase>"
            )
            .expect("writing to a string cannot fail");
        }
        xml.push_str("</testsuite>\n");
        xml
    }
}

fn xml_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            '\t'
            | '\n'
            | '\r'
            | '\u{20}'..='\u{d7ff}'
            | '\u{e000}'..='\u{fffd}'
            | '\u{10000}'..='\u{10ffff}' => escaped.push(character),
            _ => escaped.push('\u{fffd}'),
        }
    }
    escaped
}

pub fn allocate_report_dir(parent: &Path, lane: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(parent)?;
    let parent = std::path::absolute(parent)?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let dir = parent.join(format!("{lane}-{millis}-{}", std::process::id()));
    fs::create_dir(&dir)?;
    Ok(dir)
}

pub fn sha256(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut bytes = [0; 32768];
    loop {
        let count = file.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        digest.update(&bytes[..count]);
    }
    let mut encoded = String::with_capacity(64);
    for byte in digest.finalize() {
        write!(&mut encoded, "{byte:02x}").expect("writing to a string cannot fail");
    }
    Ok(encoded)
}

fn command_text(program: &str, args: &[&str]) -> Option<String> {
    output_text(Command::new(program).args(args))
}

fn git_text(root: &Path, args: &[&str]) -> Option<String> {
    output_text(Command::new("git").arg("-C").arg(root).args(args))
}

fn output_text(command: &mut Command) -> Option<String> {
    let output = command.output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn combined_status(cases: &[CaseResult]) -> Status {
    if cases.is_empty() || cases.iter().any(|case| case.status == Status::Fail) {
        Status::Fail
    } else if cases.iter().any(|case| case.status == Status::Blocked) {
        Status::Blocked
    } else {
        Status::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_blocked_and_failed_runs_cannot_pass() {
        let case = |status| CaseResult::new("UT-021", status, "self-check");
        assert_eq!(combined_status(&[]), Status::Fail);
        assert_eq!(combined_status(&[case(Status::Pass)]), Status::Pass);
        assert_eq!(
            combined_status(&[case(Status::Pass), case(Status::Blocked)]),
            Status::Blocked
        );
        assert_eq!(
            combined_status(&[case(Status::Blocked), case(Status::Fail)]),
            Status::Fail
        );
    }

    #[test]
    fn report_records_seed_host_and_result_in_machine_readable_form() {
        let dir = tempfile::tempdir().unwrap();
        let mut report = Report::new("unit", None, Some(941), dir.path());
        report.cases.push(CaseResult::new(
            "UT-021",
            Status::Blocked,
            "deliberate <test> condition & quote \"",
        ));
        assert_eq!(report.finish(dir.path()).unwrap(), Status::Blocked);
        let body = fs::read(dir.path().join("result.json")).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["seed"], 941);
        assert_eq!(json["host"]["os"], std::env::consts::OS);
        assert_eq!(json["host"]["architecture"], std::env::consts::ARCH);
        assert_eq!(json["status"], "Blocked");
        assert_eq!(json["cases"][0]["status"], "Blocked");

        let junit = fs::read_to_string(dir.path().join("junit.xml")).unwrap();
        assert!(junit.contains("tests=\"1\" failures=\"0\" skipped=\"1\""));
        assert!(junit.contains(
            "<skipped message=\"Blocked: deliberate &lt;test&gt; condition &amp; quote &quot;\" />"
        ));
    }
}
