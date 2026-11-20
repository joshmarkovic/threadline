use super::{CliError, GitHubCli, LOGIN_ARGS};
use std::{
    ffi::{CStr, OsString},
    io::IsTerminal,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Component, Path, PathBuf},
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
    trusted_from(Path::new("/"), path, unsafe { libc::geteuid() }, 40)
}

fn trusted_from(root: &Path, path: &Path, uid: u32, remaining_links: u8) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let trusted_owner = |m: &std::fs::Metadata| m.uid() == 0 || m.uid() == uid;
    let trusted_mode = |m: &std::fs::Metadata| trusted_owner(m) && m.mode() & 0o022 == 0;
    if !root
        .symlink_metadata()
        .is_ok_and(|m| m.is_dir() && trusted_mode(&m))
    {
        return false;
    }
    let mut current = root.to_owned();
    let mut parts = relative.components();
    while let Some(part) = parts.next() {
        match part {
            Component::ParentDir => {
                if current != root {
                    current.pop();
                }
                continue;
            }
            Component::Normal(name) => current.push(name),
            Component::CurDir => continue,
            _ => return false,
        }
        let Ok(meta) = current.symlink_metadata() else {
            return false;
        };
        if meta.is_symlink() {
            if remaining_links == 0 || !trusted_owner(&meta) {
                return false;
            }
            let Ok(target) = current.read_link() else {
                return false;
            };
            current.pop();
            let next = current.join(target).join(parts.as_path());
            return trusted_from(root, &next, uid, remaining_links - 1);
        }
        if !trusted_mode(&meta) || (!parts.as_path().is_empty() && !meta.is_dir()) {
            return false;
        }
    }
    current.is_file()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };

    #[test]
    fn cli_permissions_cover_the_executable_and_each_link_target_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let bin = root.join("bin");
        let packages = root.join("packages");
        let links = root.join("links");
        for path in [&bin, &packages, &links] {
            fs::create_dir(path).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let gh = packages.join("gh");
        fs::write(&gh, b"synthetic fixture").unwrap();
        let entry = bin.join("gh");
        symlink("../links/current", &entry).unwrap();
        symlink(&gh, links.join("current")).unwrap();
        let uid = unsafe { libc::geteuid() };
        let check = || trusted_from(root, &entry, uid, 40);
        for candidate in [&gh, &bin, &packages, &links] {
            fs::set_permissions(candidate, fs::Permissions::from_mode(0o755)).unwrap();
            assert!(check());
            for unsafe_mode in [0o775, 0o757] {
                fs::set_permissions(candidate, fs::Permissions::from_mode(unsafe_mode)).unwrap();
                assert!(!check());
            }
            fs::set_permissions(candidate, fs::Permissions::from_mode(0o755)).unwrap();
        }
        symlink("loop", bin.join("loop")).unwrap();
        assert!(!trusted_from(root, &bin.join("loop"), uid, 40));
    }
}
