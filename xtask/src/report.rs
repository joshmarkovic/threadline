use std::{
    fmt::Write as _,
    fs::{self, File},
    io::{self, Read, Write as _},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::catalog::{Associations, associations};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum Status {
    Pass,
    Fail,
    Blocked,
}

impl Status {
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Pass => 0,
            Self::Fail => 1,
            Self::Blocked => 78,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CaseResult {
    pub case: String,
    pub status: Status,
    pub detail: String,
    pub tests_passed: Option<usize>,
    pub evidence: Option<String>,
    #[serde(flatten)]
    associations: Associations,
    pub child_exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixture_revision: Option<&'static str>,
}

impl CaseResult {
    pub fn new(case: &str, status: Status, detail: impl Into<String>) -> Self {
        Self {
            case: case.into(),
            status,
            detail: detail.into(),
            tests_passed: None,
            evidence: None,
            associations: associations(case),
            child_exit_code: None,
            fixture_revision: crate::catalog::fixture_revision(case),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Invocation {
    pub program: &'static str,
    pub argv: Vec<String>,
    pub redacted: bool,
    #[serde(skip)]
    started_at_unix_ms: u128,
}

impl Invocation {
    pub fn new(args: &[String], started_at_unix_ms: u128) -> Self {
        let argv = crate::cli::report_arguments(args);
        Self {
            program: "xtask",
            redacted: argv != args,
            argv,
            started_at_unix_ms,
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
    pub package_version: String,
    pub package_version_source: &'static str,
    pub fixture_revision: &'static str,
    pub status: Status,
    pub cases: Vec<CaseResult>,
    pub command: Invocation,
    pub started_at_unix_ms: u128,
    pub finished_at_unix_ms: Option<u128>,
    pub exit_code: u8,
}

impl Report {
    pub fn new(
        lane: &str,
        suite: Option<String>,
        seed: Option<u64>,
        root: &Path,
        command: Invocation,
    ) -> Self {
        let source_revision = git_text(root, &["rev-parse", "HEAD"]);
        let source_dirty = git_text(root, &["status", "--porcelain"]).map(|s| !s.is_empty());
        let (package_version, package_version_source) = app_version(root);
        Self {
            schema: 1,
            lane: lane.into(),
            suite,
            seed,
            host: Host::current(),
            source_revision,
            source_dirty,
            cargo_lock_sha256: sha256(&root.join("Cargo.lock")).ok(),
            package_version,
            package_version_source,
            fixture_revision: "not-applicable",
            status: Status::Fail,
            cases: Vec::new(),
            started_at_unix_ms: command.started_at_unix_ms,
            finished_at_unix_ms: None,
            exit_code: Status::Fail.exit_code(),
            command,
        }
    }

    pub fn finish(mut self, dir: &Path) -> io::Result<Status> {
        self.status = combined_status(&self.cases);
        self.fixture_revision = report_fixture_revision(&self.cases);
        self.exit_code = self.status.exit_code();
        self.finished_at_unix_ms = Some(unix_timestamp_ms());
        let report_path = dir.join("result.json");
        let document = serde_json::to_vec_pretty(&self)?;
        fs::write(dir.join("junit.xml"), self.junit())?;
        fs::write(&report_path, document)?;
        let mut output = io::stdout().lock();
        for case in &self.cases {
            let _ = writeln!(output, "{}: {:?} — {}", case.case, case.status, case.detail);
        }
        let _ = writeln!(
            output,
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

fn report_fixture_revision(cases: &[CaseResult]) -> &'static str {
    let mut revisions = cases.iter().filter_map(|case| case.fixture_revision);
    match revisions.next() {
        Some(first) if revisions.all(|revision| revision == first) => first,
        Some(_) => "mixed",
        None => "not-applicable",
    }
}

fn app_version(root: &Path) -> (String, &'static str) {
    const APP_MANIFEST: &str = include_str!("../../crates/threadline-app/Cargo.toml");
    const WORKSPACE_MANIFEST: &str = include_str!("../../Cargo.toml");
    let workspace = fs::read_to_string(root.join("Cargo.toml")).ok();
    let checkout_version = fs::read_to_string(root.join("crates/threadline-app/Cargo.toml"))
        .ok()
        .and_then(|app| declared_app_version(&app, workspace.as_deref()));
    if let Some(version) = checkout_version {
        (version, "checkout-app-manifest")
    } else {
        let version = declared_app_version(APP_MANIFEST, Some(WORKSPACE_MANIFEST))
            .expect("runner build has a versioned application manifest");
        (version, "runner-build-app-manifest")
    }
}

fn declared_app_version(app: &str, workspace: Option<&str>) -> Option<String> {
    let app: toml::Table = toml::from_str(app).ok()?;
    let package = app.get("package")?;
    if package.get("name")?.as_str()? != "threadline-app" {
        return None;
    }
    let version = package.get("version")?;
    if let Some(version) = version.as_str() {
        return Some(version.into());
    }
    if version.get("workspace")?.as_bool()? {
        let workspace: toml::Table = toml::from_str(workspace?).ok()?;
        return workspace
            .get("workspace")?
            .get("package")?
            .get("version")?
            .as_str()
            .map(str::to_owned);
    }
    None
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
    let millis = unix_timestamp_ms();
    let dir = parent.join(format!("{lane}-{millis}-{}", std::process::id()));
    fs::create_dir(&dir)?;
    Ok(dir)
}

pub fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
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
    fn reports_the_selected_fixture_or_a_mixed_lane() {
        let native = CaseResult::new("E2E-002", Status::Pass, "synthetic account");
        let live = CaseResult::new("LIVE-001", Status::Blocked, "PAT unavailable");
        assert_eq!(
            report_fixture_revision(std::slice::from_ref(&native)),
            "s03-github-access-v1"
        );
        assert_eq!(
            report_fixture_revision(std::slice::from_ref(&live)),
            "s03-live-read-allowlist-v1"
        );
        assert_eq!(report_fixture_revision(&[native, live]), "mixed");
        assert_eq!(report_fixture_revision(&[]), "not-applicable");
    }

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
        let invocation = Invocation::new(&["test".into(), "unit".into()], unix_timestamp_ms());
        let mut report = Report::new("unit", None, Some(941), dir.path(), invocation);
        report.cases.push(CaseResult::new(
            "UT-021",
            Status::Blocked,
            "deliberate <test> condition & quote \"",
        ));
        assert_eq!(report.finish(dir.path()).unwrap(), Status::Blocked);
        let body = fs::read(dir.path().join("result.json")).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["seed"], 941);
        assert_eq!(
            json["fixture_revision"],
            include_str!("../../fixtures/revision.txt").trim_ascii()
        );
        assert_eq!(json["host"]["os"], std::env::consts::OS);
        assert_eq!(json["host"]["architecture"], std::env::consts::ARCH);
        assert_eq!(json["status"], "Blocked");
        assert_eq!(json["exit_code"], 78);
        assert_eq!(json["cases"][0]["status"], "Blocked");
        assert_eq!(
            json["cases"][0]["requirement_ids"],
            serde_json::json!(["TEST-01", "TEST-02", "TEST-08"])
        );
        assert_eq!(
            json["cases"][0]["acceptance_ids"],
            serde_json::json!(["AC-89"])
        );

        let junit = fs::read_to_string(dir.path().join("junit.xml")).unwrap();
        assert!(junit.contains("tests=\"1\" failures=\"0\" skipped=\"1\""));
        assert!(junit.contains(
            "<skipped message=\"Blocked: deliberate &lt;test&gt; condition &amp; quote &quot;\" />"
        ));
    }

    #[test]
    fn writes_timestamps_and_matching_exit_codes_for_all_outcomes() {
        for (status, exit, junit) in [
            (Status::Pass, 0, "failures=\"0\" skipped=\"0\""),
            (Status::Fail, 1, "failures=\"1\" skipped=\"0\""),
            (Status::Blocked, 78, "failures=\"0\" skipped=\"1\""),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let invocation = Invocation::new(
                &[
                    "test".into(),
                    "unit".into(),
                    "--case".into(),
                    "UT-021".into(),
                ],
                123,
            );
            let mut report = Report::new("unit", None, Some(12345), dir.path(), invocation);
            report
                .cases
                .push(CaseResult::new("UT-021", status, "test result"));
            assert_eq!(report.finish(dir.path()).unwrap().exit_code(), exit);
            let body = fs::read(dir.path().join("result.json")).unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(json["exit_code"], exit);
            assert_eq!(json["started_at_unix_ms"], 123);
            assert!(json["finished_at_unix_ms"].as_u64().unwrap() >= 123);
            assert_eq!(json["command"]["program"], "xtask");
            assert_eq!(
                json["command"]["argv"],
                serde_json::json!(["test", "unit", "--case", "UT-021"])
            );
            assert_eq!(json["command"]["redacted"], false);
            assert!(
                fs::read_to_string(dir.path().join("junit.xml"))
                    .unwrap()
                    .contains(junit)
            );
        }
    }

    #[test]
    fn reads_the_app_package_version_even_when_it_differs_from_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let app_dir = dir.path().join("crates/threadline-app");
        fs::create_dir_all(&app_dir).unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace.package]\nversion = '1.2.3'\n",
        )
        .unwrap();
        let app = app_dir.join("Cargo.toml");
        fs::write(
            &app,
            "[package]\nname = 'threadline-app'\nversion = '7.8.9' # app version\n",
        )
        .unwrap();
        assert_eq!(
            app_version(dir.path()),
            ("7.8.9".into(), "checkout-app-manifest")
        );
        fs::write(
            &app,
            "[package]\nname = 'threadline-app'\nversion.workspace = true\n",
        )
        .unwrap();
        assert_eq!(
            app_version(dir.path()),
            ("1.2.3".into(), "checkout-app-manifest")
        );
    }

    #[test]
    fn stages_without_an_app_manifest_use_a_labeled_build_fallback() {
        let staged = tempfile::tempdir().unwrap();
        let (version, source) = app_version(staged.path());
        assert!(!version.is_empty());
        assert_eq!(source, "runner-build-app-manifest");
    }
}
