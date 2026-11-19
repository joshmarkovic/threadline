//! Automatic releases for the registered Windows installation.
use crate::Staged;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckError {
    /// GitHub rejected authentication or concealed the release repository.
    ReleaseAccessRequired,
    Other(&'static str),
}

impl From<&'static str> for CheckError {
    fn from(message: &'static str) -> Self {
        Self::Other(message)
    }
}

type CheckResult<T> = Result<T, CheckError>;

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

/// Whether the running program is registered for signed private prereleases.
pub fn private_channel() -> bool {
    #[cfg(windows)]
    {
        matches!(
            windows::verified_install(),
            Ok(Some((_, release::Trust::Private(_))))
        )
    }
    #[cfg(not(windows))]
    false
}

/// An optional release-only credential may be supplied by the account owner.
/// Dropping this future discards any partial download.
pub async fn check(release_token: Option<&str>) -> CheckResult<Option<Staged>> {
    #[cfg(windows)]
    return windows::check(release_token).await;
    #[cfg(not(windows))]
    {
        let _ = release_token;
        Ok(None)
    }
}
