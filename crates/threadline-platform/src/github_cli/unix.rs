use super::{CliError, GitHubCli, LOGIN_ARGS};
use std::{
    ffi::{CStr, OsString},
    io::IsTerminal,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
    process::Stdio,
};

pub(super) fn candidates() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    let paths = [
        "/opt/homebrew/bin/gh",
        "/usr/local/bin/gh",
        "/opt/local/bin/gh",
    ];
    #[cfg(not(target_os = "macos"))]
    let paths = ["/usr/bin/gh", "/usr/local/bin/gh", "/opt/gh/bin/gh"];
    paths.into_iter().map(PathBuf::from).collect()
}

pub(super) fn trusted(path: &Path) -> bool {
    let Ok(resolved) = path.canonicalize() else {
        return false;
    };
    let uid = unsafe { libc::geteuid() };
    let trusted = |p: &Path| {
        p.metadata()
            .is_ok_and(|m| (m.uid() == 0 || m.uid() == uid) && m.mode() & 0o002 == 0)
    };
    path.ancestors().all(trusted) && resolved.ancestors().all(trusted)
}

pub(super) fn environment() -> Result<Vec<(OsString, OsString)>, CliError> {
    let mut storage = vec![0; 16384];
    let mut info = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    // getpwuid_r writes its strings into our live buffer, including the account's home directory.
    let status = unsafe {
        libc::getpwuid_r(
            libc::geteuid(),
            info.as_mut_ptr(),
            storage.as_mut_ptr(),
            storage.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return Err(CliError::Unavailable);
    }
    let home = unsafe { (*result).pw_dir };
    if home.is_null() {
        return Err(CliError::Unavailable);
    }
    let home = Path::new(std::ffi::OsStr::from_bytes(
        unsafe { CStr::from_ptr(home) }.to_bytes(),
    ));
    if !home.is_absolute() {
        return Err(CliError::Unavailable);
    }
    let mut entries = vec![
        ("GH_NO_EXTENSION_UPDATE_NOTIFIER".into(), "1".into()),
        ("GH_NO_UPDATE_NOTIFIER".into(), "1".into()),
        ("HOME".into(), home.as_os_str().into()),
        ("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin".into()),
    ];
    for key in [
        "LANG",
        "LC_CTYPE",
        "TERM",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
    ] {
        if let Some(value) = std::env::var_os(key) {
            entries.push((key.into(), value));
        }
    }
    Ok(entries)
}

pub(super) async fn login(cli: &GitHubCli) -> Result<(), CliError> {
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return Err(CliError::TerminalRequired);
    }
    let status = cli
        .command(LOGIN_ARGS)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(|_| CliError::Unavailable)?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::LoginFailed)
    }
}
