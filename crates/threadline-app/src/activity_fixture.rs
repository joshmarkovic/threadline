//! Preserve the native application's delivery decisions without posting to the test desktop.
use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::{Ipv4Addr, SocketAddrV4},
    path::{Path, PathBuf},
};
use threadline_storage::{
    Store, UtcMillis,
    fixtures::{ACTIVITY_FIXTURE_REVISION, inspect_native_activity, prepare_native_activity},
    notifications::PopupGroup,
};

const MARKER: &str = "native-activity.owner";
const CAPTURE: &str = "native-activity-groups.jsonl";
const ERROR: &str = "Native activity proof requires the explicitly isolated loopback profile.";

fn profile() -> Option<PathBuf> {
    if !cfg!(windows) || std::env::var_os("THREADLINE_E2E_NATIVE_ACTIVITY").as_deref()? != "1" {
        return None;
    }
    let addr: SocketAddrV4 = std::env::var("THREADLINE_E2E_GITHUB_ADDR")
        .ok()?
        .parse()
        .ok()?;
    if *addr.ip() != Ipv4Addr::LOCALHOST || addr.port() == 0 {
        return None;
    }
    let path = PathBuf::from(std::env::var_os("THREADLINE_PROFILE_DIR")?);
    if !path.is_absolute() || !fs::symlink_metadata(&path).ok()?.file_type().is_dir() {
        return None;
    }
    let path = path.canonicalize().ok()?;
    let runner = path.parent()?;
    let key = path.file_name()?.to_str()?.strip_prefix("initial-")?;
    if key.len() != 32
        || !key.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !runner.file_name()?.to_str()?.starts_with("threadline-e2e-")
        || runner.parent()? != std::env::temp_dir().canonicalize().ok()?
        || !fs::symlink_metadata(runner.join("access-fixture-phase.txt"))
            .ok()?
            .is_file()
    {
        return None;
    }
    Some(path)
}

fn owned(root: &Path) -> bool {
    let marker = root.join(MARKER);
    fs::symlink_metadata(&marker).is_ok_and(|entry| {
        entry.file_type().is_file() && entry.len() == ACTIVITY_FIXTURE_REVISION.len() as u64
    }) && fs::read(marker).is_ok_and(|value| value == ACTIVITY_FIXTURE_REVISION.as_bytes())
}

pub(crate) struct Capture(PathBuf);

impl Capture {
    pub(crate) fn open() -> Option<Self> {
        if !std::env::args_os().any(|argument| argument == "--e2e-report") {
            return None;
        }
        let root = profile()?;
        owned(&root).then(|| Self(root.join(CAPTURE)))
    }

    pub(crate) fn record(&self, group: &PopupGroup) -> std::io::Result<()> {
        if !fs::symlink_metadata(&self.0)?.file_type().is_file() {
            return Err(std::io::Error::other(
                "The activity capture is not a regular fixture file",
            ));
        }
        let mut output = OpenOptions::new().append(true).open(&self.0)?;
        let mut receipt = serde_json::to_vec(&serde_json::json!({
            "events": group.event_count,
            "destination": group.destination,
            "sound": group.sound,
        }))?;
        receipt.push(b'\n');
        output.write_all(&receipt)?;
        output.sync_data()
    }
}

fn execute(stage: &str, output: PathBuf) -> Result<(), String> {
    let root = profile().ok_or(ERROR)?;
    if stage == "seed" {
        if fs::read_dir(&root).map_err(|_| ERROR)?.next().is_some() {
            return Err(ERROR.into());
        }
        let mut marker = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(root.join(MARKER))
            .map_err(|_| ERROR)?;
        marker
            .write_all(ACTIVITY_FIXTURE_REVISION.as_bytes())
            .map_err(|_| ERROR)?;
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(root.join(CAPTURE))
            .map_err(|_| ERROR)?;
    } else if stage != "inspect" || !owned(&root) {
        return Err(ERROR.into());
    }
    let store = Store::open_profile(&root).map_err(|_| ERROR)?;
    if stage == "seed" {
        prepare_native_activity(&store, UtcMillis::now().map_err(|_| ERROR)?.0)
            .map_err(|error| error.to_string())?;
    }
    let data = inspect_native_activity(&store).map_err(|error| error.to_string())?;
    let receipt = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output)
        .map_err(|_| ERROR)?;
    serde_json::to_writer(receipt, &data).map_err(|_| ERROR.into())
}

#[doc(hidden)]
pub fn native_activity_command() -> Option<Result<(), String>> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next()? != "--e2e-native-activity" {
        return None;
    }
    Some((|| {
        let action = arguments.next().ok_or(ERROR)?;
        if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--e2e-report")) {
            return Err(ERROR.into());
        }
        let report = arguments
            .next()
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or(ERROR)?;
        if arguments.next().is_some() {
            return Err(ERROR.into());
        }
        execute(action.to_str().ok_or(ERROR)?, report)
    })())
}
