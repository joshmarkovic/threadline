//! Automatic releases for the registered Windows installation.
use crate::{Result, Staged};

#[cfg(any(windows, test))]
mod release;
#[cfg(windows)]
pub(crate) use release::Installed;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::Restart;

/// The installed version, when the running program is installed and configured
/// for verified updates.
pub fn installed_version() -> Option<String> {
    #[cfg(windows)]
    {
        windows::verified_install()
            .ok()
            .flatten()
            .map(|(installed, _)| installed.version.to_string())
    }
    #[cfg(not(windows))]
    None
}

/// Check the public release feed and stage a verified newer installer.
/// Dropping this future discards any partial download.
pub async fn check() -> Result<Option<Staged>> {
    #[cfg(windows)]
    return windows::check().await;
    #[cfg(not(windows))]
    Ok(None)
}
