//! Portable desktop service contracts and native implementations.

use std::error::Error;
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};

use tokio::sync::mpsc::{self, UnboundedReceiver};

mod activity;
pub use activity::{
    ActivityAction, ActivityActivation, ActivityId, ActivityNotification, ActivityPermission,
};
mod credentials;
mod paths;
#[cfg(windows)]
mod platform_windows;
#[cfg(windows)]
pub mod publisher;
#[cfg(windows)]
mod toast_registration;
#[cfg(windows)]
mod toast_windows;

pub use credentials::NativeCredentialStore;
pub use paths::NativePaths;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Activation {
    Focus,
    OpenReview,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformEvent {
    Activate(Activation),
    TrayChanged(bool),
    NotificationShown,
    NotificationSubmitted,
    NotificationComActivated,
    ActivityActivated(ActivityActivation),
    Quit,
    Wake,
    NetworkChanged,
    Error(PlatformError),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Capabilities {
    pub notifications: bool,
    pub tray: bool,
    pub single_instance: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformError {
    Unsupported,
    NotificationRegistrationMissing,
    Unavailable,
    Locked,
    Denied,
    InvalidCredentialReference,
    InvalidCredentialValue,
    InvalidProfileDirectory,
    InstanceWindowNotFound,
    ActivationRejected,
    Native { operation: &'static str, code: u32 },
}

impl Display for PlatformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Unsupported => "This desktop service is unavailable on this platform",
            Self::NotificationRegistrationMissing => {
                "Windows did not recognize this executable's notification registration"
            }
            Self::Unavailable => "Desktop service is temporarily unavailable",
            Self::Locked => "The desktop service is locked",
            Self::Denied => "The desktop service denied this operation",
            Self::InvalidCredentialReference => "The protected credential reference is invalid",
            Self::InvalidCredentialValue => "The protected credential has an invalid size",
            Self::InvalidProfileDirectory => "The isolated profile must be an existing directory",
            Self::InstanceWindowNotFound => {
                "The running Threadline instance did not open its activation window"
            }
            Self::ActivationRejected => {
                "The running Threadline instance could not accept activation"
            }
            Self::Native { operation, code } => {
                return write!(formatter, "{operation} failed (Windows code {code})");
            }
        };
        formatter.write_str(message)
    }
}

impl Error for PlatformError {}

#[derive(Clone)]
pub struct PlatformControls {
    #[cfg(windows)]
    desktop: platform_windows::Desktop,
}

impl PlatformControls {
    pub fn capabilities(&self) -> Capabilities {
        #[cfg(windows)]
        return Capabilities {
            notifications: true,
            tray: true,
            single_instance: true,
        };
        #[cfg(not(windows))]
        return Capabilities::default();
    }

    /// Wait for `TrayChanged` or `Error` before relying on tray availability.
    pub fn set_tray_enabled(&self, enabled: bool) -> Result<(), PlatformError> {
        #[cfg(windows)]
        return self.desktop.set_tray_enabled(enabled);
        #[cfg(not(windows))]
        {
            let _ = enabled;
            Err(PlatformError::Unsupported)
        }
    }

    /// Submit a durable activity group on a worker. Windows may accept a card without showing a banner.
    pub fn submit_activity(&self, notice: ActivityNotification) -> Result<(), PlatformError> {
        #[cfg(windows)]
        return self.desktop.submit_activity(notice);
        #[cfg(not(windows))]
        {
            let _ = notice;
            Err(PlatformError::Unsupported)
        }
    }

    /// Read the sender's current setting before taking a native delivery reservation.
    pub fn activity_permission(&self) -> Result<ActivityPermission, PlatformError> {
        #[cfg(windows)]
        return self.desktop.activity_permission();
        #[cfg(not(windows))]
        Err(PlatformError::Unsupported)
    }

    pub fn activity_notifications_enabled(&self) -> Result<bool, PlatformError> {
        match self.activity_permission()? {
            ActivityPermission::Enabled => Ok(true),
            ActivityPermission::Disabled => Ok(false),
            ActivityPermission::SenderSettingUnknown => Err(PlatformError::Unavailable),
        }
    }

    /// Queue a native feasibility notification. WinRT reports submission before any activation.
    pub fn notify_open_review(&self) -> Result<(), PlatformError> {
        #[cfg(windows)]
        return self.desktop.show_notification();
        #[cfg(not(windows))]
        return Err(PlatformError::Unsupported);
    }
}

pub enum Launch {
    Primary {
        controls: PlatformControls,
        events: UnboundedReceiver<PlatformEvent>,
    },
    Forwarded,
}

pub fn start(activation: Activation) -> Result<Launch, PlatformError> {
    start_platform(activation, None)
}

pub fn start_isolated(activation: Activation, profile: &Path) -> Result<Launch, PlatformError> {
    let profile = canonical_profile(profile)?;
    start_platform(activation, Some(&profile))
}

#[cfg(windows)]
pub fn start_isolated_with_test_toast(
    activation: Activation,
    profile: &Path,
    token: &str,
) -> Result<Launch, PlatformError> {
    let profile = canonical_profile(profile)?;
    let identity = toast_windows::Identity::registered_fixture(token)?;
    start_windows(activation, Some(&profile), Some(identity))
}

#[cfg(windows)]
pub fn installed_activity_identity() -> bool {
    toast_windows::Identity::installed().is_some()
}

#[cfg(windows)]
pub fn clear_test_toast_history(token: &str) -> Result<u32, PlatformError> {
    toast_windows::clear_fixture_history(token)
}

fn canonical_profile(profile: &Path) -> Result<PathBuf, PlatformError> {
    if profile.as_os_str().is_empty() {
        return Err(PlatformError::InvalidProfileDirectory);
    }
    let profile = profile
        .canonicalize()
        .map_err(|_| PlatformError::InvalidProfileDirectory)?;
    if !profile.is_dir() {
        return Err(PlatformError::InvalidProfileDirectory);
    }
    Ok(profile)
}

#[cfg(windows)]
fn start_platform(activation: Activation, profile: Option<&Path>) -> Result<Launch, PlatformError> {
    let identity = profile
        .is_none()
        .then(toast_windows::Identity::installed)
        .flatten();
    start_windows(activation, profile, identity)
}

#[cfg(windows)]
fn start_windows(
    activation: Activation,
    profile: Option<&Path>,
    identity: Option<toast_windows::Identity>,
) -> Result<Launch, PlatformError> {
    let (sender, events) = mpsc::unbounded_channel();
    match platform_windows::start(activation, profile, sender, identity)? {
        Some(desktop) => Ok(Launch::Primary {
            controls: PlatformControls { desktop },
            events,
        }),
        None => Ok(Launch::Forwarded),
    }
}

#[cfg(not(windows))]
fn start_platform(
    _activation: Activation,
    _profile: Option<&Path>,
) -> Result<Launch, PlatformError> {
    let (_sender, events) = mpsc::unbounded_channel();
    Ok(Launch::Primary {
        controls: PlatformControls {},
        events,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationPaths {
    pub data: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
    pub install: PathBuf,
}

pub trait PathResolver {
    fn application_paths(&self) -> Result<ApplicationPaths, PlatformError>;
}

pub struct CredentialRef {
    pub scope: String,
    pub name: String,
}

/// The type intentionally has no Debug or Display representation.
pub struct Credential(pub Vec<u8>);

pub trait CredentialStore {
    fn read(&self, credential: &CredentialRef) -> Result<Option<Credential>, PlatformError>;
    fn save(&self, credential: &CredentialRef, value: &Credential) -> Result<(), PlatformError>;
    fn remove(&self, credential: &CredentialRef) -> Result<(), PlatformError>;
}

pub trait StartupRegistration {
    fn enabled(&self) -> Result<bool, PlatformError>;
    fn set_enabled(&self, enabled: bool) -> Result<(), PlatformError>;
}

pub trait SystemTimeZone {
    fn identifier(&self) -> Result<String, PlatformError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_profile_directory_without_starting_desktop_services() {
        assert!(matches!(
            start_isolated(Activation::Focus, Path::new("")),
            Err(PlatformError::InvalidProfileDirectory)
        ));
    }

    #[cfg(not(windows))]
    #[test]
    fn foreground_start_remains_available_without_tray_or_notifications() {
        let Launch::Primary {
            controls,
            mut events,
        } = start(Activation::OpenReview).unwrap()
        else {
            panic!("expected foreground session");
        };
        assert_eq!(controls.capabilities(), Capabilities::default());
        assert_eq!(
            controls.set_tray_enabled(true),
            Err(PlatformError::Unsupported)
        );
        assert_eq!(
            controls.notify_open_review(),
            Err(PlatformError::Unsupported)
        );
        assert!(events.try_recv().is_err());
    }
}
