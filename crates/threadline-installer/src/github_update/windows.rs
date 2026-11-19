use super::{
    Installed,
    release::{Github, Trust, private_build, private_key},
};
use crate::{Platform, Result, Staged, lock_installer, pin, verify_bytes, windows_host};
use semver::Version;
use std::{
    fs::{self, File},
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use threadline_platform::{NativePaths, PathResolver, publisher::verify};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

impl Trust {
    fn registration(self) -> windows_host::Registration {
        match self {
            Self::Authenticode(_) => windows_host::Registration::Stable,
            Self::Private(_) => windows_host::Registration::Private,
        }
    }
}

pub(crate) struct Restart {
    trust: Trust,
    version: Version,
    build: u32,
    oldest_schema: u32,
    latest_schema: u32,
    size: u64,
    sha256: [u8; 32],
    // Deny renames or reparse point substitution along the launched executable's path.
    _directories: Vec<File>,
}

fn lock_directory(path: &Path) -> Result<File> {
    let file = File::options()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| "Windows could not hold the private update directory.")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Windows could not inspect the private update directory.")?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("The private update directory is not a regular directory.");
    }
    Ok(file)
}

fn cleanup_old_setups(cache: &Path) {
    let Ok(entries) = fs::read_dir(cache) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_name().to_str().is_some_and(|name| {
            name.strip_prefix("Threadline-setup-")
                .is_some_and(|suffix| {
                    suffix.len() == 6 && suffix.bytes().all(|c| c.is_ascii_alphanumeric())
                })
        }) {
            continue;
        }
        let dir = entry.path();
        let Ok(guard) = lock_directory(&dir) else {
            continue;
        };
        let old = guard
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age >= Duration::from_secs(24 * 3600));
        if old {
            // Windows denies deletion while a check or the installer holds this executable.
            let _ = fs::remove_file(dir.join("ThreadlineSetup.exe"));
            drop(guard);
            let _ = fs::remove_dir(dir);
        }
    }
}

fn cache() -> Result<(PathBuf, Vec<File>)> {
    let cache = NativePaths
        .application_paths()
        .map_err(|_| "Could not locate the per-user update cache.")?
        .cache
        .join("release-updates");
    fs::create_dir_all(&cache).map_err(|_| "Could not create the per-user update cache.")?;
    let mut directories = Vec::new();
    let ancestors: Vec<_> = cache
        .ancestors()
        .take_while(|p| p.parent().is_some())
        .collect();
    for path in ancestors.into_iter().rev() {
        directories.push(lock_directory(path)?);
    }
    cleanup_old_setups(&cache);
    Ok((cache, directories))
}

fn verify_setup(path: &Path, size: u64, hash: &[u8; 32], trust: Trust) -> Result<File> {
    let guard = lock_installer(path)?;
    verify_bytes(&guard, size, hash)?;
    if let Trust::Authenticode(publisher) = trust {
        verify(path, publisher).map_err(
            |_| "Windows could not verify the update's signature and approved publisher.",
        )?;
    }
    Ok(guard)
}

impl Restart {
    fn hold_verified_file(&self, path: &Path) -> Result<File> {
        verify_setup(path, self.size, &self.sha256, self.trust)
    }
    fn eligible(&self, installed: &Installed, host: &Platform) -> bool {
        self.version.cmp_precedence(&installed.version).is_gt()
            && self.build > installed.build.max(host.highest_build)
            && host
                .profile_schema
                .is_none_or(|s| (self.oldest_schema..=self.latest_schema).contains(&s))
    }
}

pub(super) fn verified_install() -> Result<Option<(Installed, Trust)>> {
    let trust = match (
        option_env!("THREADLINE_RELEASE_PUBLISHER_SHA256"),
        option_env!("THREADLINE_PRIVATE_UPDATE_PUBLIC_KEY"),
    ) {
        (Some(publisher), None) => Trust::Authenticode(pin(publisher)?),
        (None, Some(key)) => Trust::Private(private_key(key)?),
        (None, None) => return Ok(None),
        (Some(_), Some(_)) => return Err("This build has conflicting Windows update channels."),
    };
    let Some(current) = windows_host::installed(trust.registration())? else {
        return Ok(None);
    };
    match trust {
        Trust::Authenticode(publisher) => {
            let exe =
                std::env::current_exe().map_err(|_| "Could not locate the installed release.")?;
            verify(&exe, publisher)
                .map_err(|_| "Windows could not verify the installed release publisher.")?;
        }
        Trust::Private(_) if private_build(&current.version) != Some(current.build) => {
            return Err("The installed private release version and build do not match.");
        }
        _ => {}
    }
    Ok(Some((current, trust)))
}

pub(super) async fn check(token: Option<&str>) -> Result<Option<Staged>> {
    let Some((current, trust)) = verified_install()? else {
        return Ok(None);
    };
    let host = windows_host::detect_for(trust.registration())?;
    host.check()?;
    let (cache, mut directories) = cache()?;
    let Some(mut staged) = Github::new(trust)?
        .stage(&current, &host, &cache, token)
        .await?
    else {
        return Ok(None);
    };
    directories.push(lock_directory(staged._directory.path())?);
    let manifest = &staged.manifest;
    let restart = Restart {
        trust,
        version: Version::parse(&manifest.version)
            .map_err(|_| "The staged release version is invalid.")?,
        build: manifest.package_build,
        oldest_schema: manifest.oldest_supported_schema,
        latest_schema: manifest.current_schema,
        size: manifest.size,
        sha256: pin(&manifest.sha256)?,
        _directories: directories,
    };
    staged._guard = restart.hold_verified_file(&staged.path)?;
    staged.restart = Some(restart);
    Ok(Some(staged))
}

fn event(name: &str) -> Result<OwnedHandle> {
    use windows_sys::Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, GetLastError},
        System::Threading::CreateEventW,
    };
    let name: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
    let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, name.as_ptr()) };
    if handle.is_null() {
        return Err("Could not initialize the Windows update handoff.");
    }
    let existing = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let owned = unsafe { OwnedHandle::from_raw_handle(handle) };
    if existing {
        return Err("A Windows update handoff is already active.");
    }
    Ok(owned)
}

impl Staged {
    /// Launch the verified silent installer. The caller exits after setup confirms the handoff.
    pub fn launch_for_restart(&self) -> Result<()> {
        let restart = self
            .restart
            .as_ref()
            .ok_or("No verified automatic update is ready.")?;
        let (current, trust) = verified_install()?
            .filter(|(_, trust)| *trust == restart.trust)
            .ok_or("The registered release installation changed.")?;
        let host = windows_host::detect_for(trust.registration())?;
        host.check()?;
        if !restart.eligible(&current, &host) {
            return Err("This update is no longer compatible with the installed release.");
        }
        windows_host::require_launch_ready()?;
        let path = self._directory.path().join("ThreadlineSetup.exe");
        let _guard = restart.hold_verified_file(&path)?;
        use windows_sys::Win32::{
            Foundation::WAIT_OBJECT_0,
            System::Threading::{SetEvent, WaitForMultipleObjects},
        };
        let pid = std::process::id();
        let leaf = self
            ._directory
            .path()
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("Invalid update staging directory.")?;
        let name = format!(r"Local\Threadline.Update.{pid}.{leaf}");
        let ready = event(&format!("{name}.Ready"))?;
        let proceed = event(&format!("{name}.Proceed"))?;
        let child = Command::new(path)
            .arg("/S")
            .arg(format!("/THREADLINEUPDATE={pid}"))
            .spawn()
            .map_err(|_| "Windows could not start the verified update installer.")?;
        let handles = [ready.as_raw_handle(), child.as_raw_handle()];
        if unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, 30_000) } != WAIT_OBJECT_0
            || unsafe { SetEvent(proceed.as_raw_handle()) } == 0
        {
            return Err("Setup could not confirm the update handoff. Threadline will stay open.");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn native_trust_refuses_unsigned_setup_even_when_hash_and_size_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unsigned.exe");
        let bytes = b"MZ synthetic unsigned fixture";
        fs::write(&path, bytes).unwrap();
        assert!(matches!(
            verify_setup(
                &path,
                bytes.len() as u64,
                &Sha256::digest(bytes).into(),
                Trust::Authenticode([7; 32])
            ),
            Err("Windows could not verify the update's signature and approved publisher.")
        ));
    }
}
