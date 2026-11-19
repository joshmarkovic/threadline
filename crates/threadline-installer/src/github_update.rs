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

/// Whether the running program is installed and configured for verified updates.
pub fn configured() -> bool {
    #[cfg(windows)]
    {
        matches!(windows::verified_install(), Ok(Some(_)))
    }
    #[cfg(not(windows))]
    false
}

/// Check the public release feed and stage a verified newer installer.
/// Dropping this future discards any partial download.
pub async fn check() -> Result<Option<Staged>> {
    #[cfg(windows)]
    return windows::check().await;
    #[cfg(not(windows))]
    Ok(None)
}
