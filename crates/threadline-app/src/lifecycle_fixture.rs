//! Windows-only, loopback-scoped protected credentials for the native lifecycle test.
use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::{Ipv4Addr, SocketAddrV4},
    path::{Path, PathBuf},
};

use threadline_platform::{
    Credential, CredentialRef, CredentialStore, NativeCredentialStore, PlatformError,
};
use threadline_storage::{
    Error, Store, UtcMillis,
    fixtures::{
        BACKLOG_OWNER, BACKLOG_USER, LIFECYCLE_REVISION, lifecycle_checkpoint,
        populate_native_lifecycle,
    },
};
use zeroize::Zeroize;

use crate::accounts::FIXTURE_PAT;

const MARKER: &str = "native-lifecycle.owner";
const ERROR: &str =
    "The protected lifecycle fixture requires a runner-owned isolated Windows profile.";

struct FixtureScope {
    root: PathBuf,
    credential: CredentialRef,
}

impl FixtureScope {
    fn from_environment() -> Option<Self> {
        if !cfg!(windows) || std::env::var_os("THREADLINE_E2E_NATIVE_LIFECYCLE").as_deref()? != "1"
        {
            return None;
        }
        let address: SocketAddrV4 = std::env::var("THREADLINE_E2E_GITHUB_ADDR")
            .ok()?
            .parse()
            .ok()?;
        if *address.ip() != Ipv4Addr::LOCALHOST || address.port() == 0 {
            return None;
        }
        let profile = PathBuf::from(std::env::var_os("THREADLINE_PROFILE_DIR")?);
        if !profile.is_absolute() || !fs::symlink_metadata(&profile).ok()?.file_type().is_dir() {
            return None;
        }
        let root = profile.canonicalize().ok()?;
        let runner = root.parent()?;
        let runner_name = runner.file_name()?.to_str()?;
        let nonce = root.file_name()?.to_str()?.strip_prefix("initial-")?;
        if nonce.len() != 32
            || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !runner_name.starts_with("threadline-e2e-")
            || runner.parent()? != std::env::temp_dir().canonicalize().ok()?
            || !fs::symlink_metadata(runner.join("access-fixture-phase.txt"))
                .ok()?
                .is_file()
        {
            return None;
        }
        let credential = CredentialRef::github_profile(BACKLOG_USER, BACKLOG_OWNER, nonce).ok()?;
        Some(Self { root, credential })
    }

    fn marked(&self) -> bool {
        let path = self.root.join(MARKER);
        fs::symlink_metadata(&path).is_ok_and(|entry| {
            entry.file_type().is_file() && entry.len() == LIFECYCLE_REVISION.len() as u64
        }) && fs::read(path).is_ok_and(|content| content == LIFECYCLE_REVISION.as_bytes())
    }
}

pub(crate) struct LifecycleVault(CredentialRef);

impl LifecycleVault {
    pub(crate) fn open() -> Option<Self> {
        let scope = FixtureScope::from_environment()?;
        scope.marked().then_some(Self(scope.credential))
    }

    fn owns(&self, reference: &CredentialRef) -> bool {
        self.0.scope == reference.scope && self.0.name == reference.name
    }
}

fn sentinel(mut value: Credential) -> bool {
    let valid = value.0 == FIXTURE_PAT.as_bytes();
    value.0.zeroize();
    valid
}

impl CredentialStore for LifecycleVault {
    fn read(&self, reference: &CredentialRef) -> Result<Option<Credential>, PlatformError> {
        if !self.owns(reference) {
            return Err(PlatformError::InvalidCredentialReference);
        }
        match NativeCredentialStore.read(reference)? {
            Some(value) if value.0 == FIXTURE_PAT.as_bytes() => Ok(Some(value)),
            Some(mut value) => {
                value.0.zeroize();
                Err(PlatformError::InvalidCredentialValue)
            }
            None => Ok(None),
        }
    }

    fn save(&self, reference: &CredentialRef, value: &Credential) -> Result<(), PlatformError> {
        if !self.owns(reference) || value.0 != FIXTURE_PAT.as_bytes() {
            return Err(PlatformError::InvalidCredentialReference);
        }
        NativeCredentialStore.save(reference, value)
    }

    fn remove(&self, reference: &CredentialRef) -> Result<(), PlatformError> {
        if !self.owns(reference) {
            return Err(PlatformError::InvalidCredentialReference);
        }
        if let Some(value) = NativeCredentialStore.read(reference)? {
            if !sentinel(value) {
                return Err(PlatformError::InvalidCredentialValue);
            }
            NativeCredentialStore.remove(reference)?;
        }
        Ok(())
    }
}

fn seed(scope: FixtureScope) -> Result<serde_json::Value, String> {
    if fs::read_dir(&scope.root)
        .map_err(|_| ERROR)?
        .next()
        .is_some()
        || NativeCredentialStore
            .read(&scope.credential)
            .map_err(|_| ERROR)?
            .is_some()
    {
        return Err(ERROR.into());
    }
    let mut marker = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(scope.root.join(MARKER))
        .map_err(|_| ERROR)?;
    marker
        .write_all(LIFECYCLE_REVISION.as_bytes())
        .map_err(|_| ERROR)?;
    let seed = std::env::var("THREADLINE_TEST_SEED")
        .ok()
        .and_then(|value| value.parse().ok())
        .ok_or(ERROR)?;
    let now = UtcMillis::now().map_err(|_| ERROR)?.0;
    let store = Store::open_profile(&scope.root).map_err(|_| ERROR)?;
    populate_native_lifecycle(&store, seed, &scope.credential.name, now, || {
        NativeCredentialStore
            .save(
                &scope.credential,
                &Credential(FIXTURE_PAT.as_bytes().to_vec()),
            )
            .map_err(|_| Error::Conflict("The isolated Windows protected credential failed"))
    })
    .map_err(|error| format!("Could not seed the isolated lifecycle profile. {error}"))?;
    inspect(&scope, &store, "seeded")
}

fn inspect(scope: &FixtureScope, store: &Store, stage: &str) -> Result<serde_json::Value, String> {
    let mut snapshot = lifecycle_checkpoint(store, stage).map_err(|error| error.to_string())?;
    snapshot["protected_token_present"] = NativeCredentialStore
        .read(&scope.credential)
        .map_err(|_| ERROR)?
        .is_some_and(sentinel)
        .into();
    Ok(snapshot)
}

fn run_command(stage: &str, report: &Path) -> Result<(), String> {
    let scope = FixtureScope::from_environment().ok_or(ERROR)?;
    let snapshot = match stage {
        "seed" => seed(scope)?,
        "inspect" if scope.marked() => {
            let store = Store::open_profile(&scope.root).map_err(|_| ERROR)?;
            inspect(&scope, &store, "closed")?
        }
        "cleanup" if scope.marked() => {
            let vault = LifecycleVault(scope.credential);
            vault.remove(&vault.0).map_err(|_| ERROR)?;
            if NativeCredentialStore
                .read(&vault.0)
                .map_err(|_| ERROR)?
                .is_some()
            {
                return Err(ERROR.into());
            }
            serde_json::json!({"protected_fixture_key_removed": true})
        }
        _ => return Err(ERROR.into()),
    };
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(report)
        .map_err(|_| ERROR)?;
    serde_json::to_writer(file, &snapshot).map_err(|_| ERROR.into())
}

#[doc(hidden)]
pub fn native_lifecycle_command() -> Option<Result<(), String>> {
    let mut args = std::env::args_os().skip(1);
    if args.next()? != "--e2e-native-lifecycle" {
        return None;
    }
    Some((|| {
        let stage = args.next().ok_or(ERROR)?;
        let stage = stage.to_str().ok_or(ERROR)?;
        if args.next().as_deref() != Some(std::ffi::OsStr::new("--e2e-report")) {
            return Err(ERROR.into());
        }
        let report = args
            .next()
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or(ERROR)?;
        if args.next().is_some() {
            return Err(ERROR.into());
        }
        run_command(stage, &report)
    })())
}
