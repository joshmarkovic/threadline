//! Automatic stable releases for the registered Windows installation.
use crate::{Result, Staged};

#[cfg(any(windows, test))]
mod release;
#[cfg(windows)]
pub(crate) use release::Installed;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::Restart;

/// An unconfigured build or a source/fixture executable stays offline.
pub fn configured() -> bool {
    #[cfg(windows)]
    {
        matches!(windows::verified_install(), Ok(Some(_)))
    }
    #[cfg(not(windows))]
    false
}

/// An optional release-only credential may be supplied by the account owner.
/// Dropping this future discards any partial download.
pub async fn check(release_token: Option<&str>) -> Result<Option<Staged>> {
    #[cfg(windows)]
    return windows::check(release_token).await;
    #[cfg(not(windows))]
    {
        let _ = release_token;
        Ok(None)
    }
}
