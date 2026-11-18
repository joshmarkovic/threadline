use crate::{ApplicationPaths, PathResolver, PlatformError};

/// Stable per-user directories for app data, settings and disposable cache.
/// Resolution alone does not create any directories.
pub struct NativePaths;

impl PathResolver for NativePaths {
    fn application_paths(&self) -> Result<ApplicationPaths, PlatformError> {
        let paths = resolve()?;
        if paths.data.starts_with(&paths.cache) || paths.cache.starts_with(&paths.data) {
            return Err(PlatformError::Unavailable);
        }
        Ok(paths)
    }
}

#[cfg(windows)]
fn resolve() -> Result<ApplicationPaths, PlatformError> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt as _, path::PathBuf, ptr};
    use windows_sys::Win32::{
        Foundation::{ERROR_ACCESS_DENIED, RPC_E_CHANGED_MODE},
        System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
        UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath},
    };

    struct ComInitialization(bool);
    impl Drop for ComInitialization {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    let initialize = unsafe { CoInitializeEx(ptr::null(), COINIT_MULTITHREADED as u32) };
    if initialize < 0 && initialize != RPC_E_CHANGED_MODE {
        return Err(PlatformError::Native {
            operation: "Initialize Windows application paths",
            code: initialize as u32,
        });
    }
    // A thread initialized in another apartment already has COM available.
    let _apartment = ComInitialization(initialize >= 0);
    let mut wide = ptr::null_mut();
    let status = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_LocalAppData,
            KF_FLAG_DONT_VERIFY as u32,
            ptr::null_mut(),
            &mut wide,
        )
    };
    let local = if status >= 0 && !wide.is_null() {
        // A successful Windows call supplies a null-terminated UTF-16 path.
        let len = (0..)
            .take_while(|offset| unsafe { *wide.add(*offset) } != 0)
            .count();
        let units = unsafe { std::slice::from_raw_parts(wide, len) };
        Some(PathBuf::from(OsString::from_wide(units)))
    } else {
        None
    };
    unsafe { CoTaskMemFree(wide.cast()) };
    let unavailable = if status as u32 == (0x8007_0000 | ERROR_ACCESS_DENIED) {
        PlatformError::Denied
    } else {
        PlatformError::Unavailable
    };
    let local = local.filter(|path| path.is_absolute()).ok_or(unavailable)?;

    let root = local.join("Threadline");
    let state = root.join("State");
    Ok(ApplicationPaths {
        data: root.join("Data"),
        config: root.join("Config"),
        cache: root.join("Cache"),
        logs: state.join("Logs"),
        state,
        install: local.join("Programs").join("Threadline"),
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn home_dir() -> Result<std::path::PathBuf, PlatformError> {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .filter(|home| home.is_absolute())
        .ok_or(PlatformError::Unavailable)
}

#[cfg(target_os = "linux")]
fn resolve() -> Result<ApplicationPaths, PlatformError> {
    let home = home_dir()?;
    Ok(relative_to_xdg(&home, |name| {
        std::env::var_os(name).map(Into::into)
    }))
}

#[cfg(target_os = "linux")]
fn relative_to_xdg(
    home: &std::path::Path,
    var: impl Fn(&str) -> Option<std::path::PathBuf>,
) -> ApplicationPaths {
    let directory = |key, default| {
        var(key)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(default))
            .join("threadline")
    };
    let state = directory("XDG_STATE_HOME", ".local/state");
    ApplicationPaths {
        data: directory("XDG_DATA_HOME", ".local/share"),
        config: directory("XDG_CONFIG_HOME", ".config"),
        cache: directory("XDG_CACHE_HOME", ".cache"),
        logs: state.join("logs"),
        state,
        install: home.join(".local/lib/threadline"),
    }
}

#[cfg(target_os = "macos")]
fn resolve() -> Result<ApplicationPaths, PlatformError> {
    let home = home_dir()?;
    let library = home.join("Library");
    let root = library.join("Application Support").join("Threadline");
    Ok(ApplicationPaths {
        data: root.join("Data"),
        config: root.join("Config"),
        state: root.join("State"),
        cache: library.join("Caches").join("Threadline"),
        logs: library.join("Logs").join("Threadline"),
        install: home.join("Applications").join("Threadline.app"),
    })
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn resolve() -> Result<ApplicationPaths, PlatformError> {
    Err(PlatformError::Unsupported)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn accepts_absolute_xdg_values_and_preserves_unicode() {
        let home = Path::new("/home/fixture-例");
        let paths = relative_to_xdg(home, |name| match name {
            "XDG_DATA_HOME" => Some(PathBuf::from("/storage/fixture-例")),
            "XDG_CACHE_HOME" => Some(PathBuf::from("invalid-relative")),
            _ => None,
        });
        assert_eq!(paths.data, Path::new("/storage/fixture-例/threadline"));
        assert_eq!(paths.cache, home.join(".cache/threadline"));
        assert_eq!(paths.config, home.join(".config/threadline"));
        assert_eq!(paths.logs, home.join(".local/state/threadline/logs"));
        assert_eq!(paths.install, home.join(".local/lib/threadline"));
    }
}
