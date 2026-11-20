use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::UnboundedSender;
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{
    NotificationSetting, ToastActivatedEventArgs, ToastFailedEventArgs, ToastNotification,
    ToastNotificationManager, ToastNotifier,
};
use windows::Win32::Foundation::{CLASS_E_NOAGGREGATION, E_INVALIDARG, E_POINTER, ERROR_NOT_FOUND};
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, CoCreateGuid, CoRegisterClassObject, CoRevokeClassObject, IClassFactory,
    IClassFactory_Impl, REGCLS_MULTIPLEUSE,
};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
use windows::Win32::UI::Notifications::{
    INotificationActivationCallback, INotificationActivationCallback_Impl,
    NOTIFICATION_USER_INPUT_DATA,
};
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::core::{
    BOOL, Error, GUID, HSTRING, IInspectable, IUnknown, Interface, PCWSTR, Ref, implement,
};
use windows_sys::Win32::System::Registry::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER};

use super::toast_registration::{installed_link_matches, registry_matches};
use super::{
    Activation, ActivityActivation, ActivityNotification, ActivityPermission, PlatformError,
    PlatformEvent,
};

const BODY: &str = "Open the sample pull request in Threadline.";
const GROUP: &str = "ThreadlineReview";
const INSTALLED_APP_ID: &str = "Threadline.Desktop";
const INSTALLED_CLASS: GUID = GUID::from_u128(0x5a20cea8_a5d8_4b5e_9be3_4121ba0aa753);

pub(super) struct Identity {
    app_id: String,
    class: GUID,
    title: String,
    action: String,
}

impl Identity {
    pub(super) fn installed() -> Option<Self> {
        let executable = std::env::current_exe().ok()?;
        let command = format!("\"{}\" --notification-com-server", executable.display());
        let local_class = GUID::from_u128(0xdf3bb9a2_0794_44a6_b130_a35197093afe);
        let identities = [
            (
                INSTALLED_APP_ID,
                INSTALLED_CLASS,
                "Threadline.lnk",
                r"CLSID\{5a20cea8-a5d8-4b5e-9be3-4121ba0aa753}\LocalServer32",
            ),
            (
                "Threadline.InstallTest",
                local_class,
                "Threadline Preview.lnk",
                r"CLSID\{df3bb9a2-0794-44a6-b130-a35197093afe}\LocalServer32",
            ),
        ];
        for (app_id, class, shortcut, server) in identities {
            if registry_matches(HKEY_CLASSES_ROOT, server, None, &command)
                && installed_link_matches(&executable, app_id, class, shortcut)
            {
                return Some(Self {
                    app_id: app_id.into(),
                    class,
                    title: "Threadline review".into(),
                    action: "Open review".into(),
                });
            }
        }
        None
    }

    fn fixture(token: &str) -> Result<Self, PlatformError> {
        if token.len() != 32
            || !token
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(PlatformError::ActivationRejected);
        }
        let class =
            u128::from_str_radix(token, 16).map_err(|_| PlatformError::ActivationRejected)?;
        if class == 0 || GUID::from_u128(class) == INSTALLED_CLASS {
            return Err(PlatformError::ActivationRejected);
        }
        Ok(Self {
            app_id: format!("Threadline.Feasibility.{token}"),
            class: GUID::from_u128(class),
            title: format!("Threadline review {}", &token[..8]),
            action: format!("Open review {}", &token[..8]),
        })
    }

    pub(super) fn registered_fixture(token: &str) -> Result<Self, PlatformError> {
        let identity = Self::fixture(token)?;
        let subkey = format!(
            r"Software\Classes\CLSID\{{{}-{}-{}-{}-{}}}",
            &token[..8],
            &token[8..12],
            &token[12..16],
            &token[16..20],
            &token[20..]
        );
        let executable = std::env::current_exe().map_err(|_| PlatformError::Unavailable)?;
        let command = format!("\"{}\" --notification-com-server", executable.display());
        if registry_matches(
            HKEY_CURRENT_USER,
            &subkey,
            Some("ThreadlineFeasibilityOwner"),
            token,
        ) && registry_matches(
            HKEY_CURRENT_USER,
            &format!(r"{subkey}\LocalServer32"),
            None,
            &command,
        ) {
            Ok(identity)
        } else {
            Err(PlatformError::ActivationRejected)
        }
    }

    pub(super) fn set_process(&self) -> Result<(), PlatformError> {
        let id = HSTRING::from(self.app_id.as_str());
        // The wide string is NUL-terminated and remains in scope for the shell call.
        unsafe { SetCurrentProcessExplicitAppUserModelID(PCWSTR(id.as_ptr())) }
            .map_err(|error| native_error("Assign the Threadline notification identity", error))
    }
}

pub(super) fn clear_fixture_history(token: &str) -> Result<u32, PlatformError> {
    let identity = Identity::registered_fixture(token)?;
    identity.set_process()?;
    // Cleanup runs as the same executable and uses only this per-run fixture identity.
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
        .map_err(|error| native_error("Initialize fixture notification cleanup", error))?;
    let _apartment = Apartment;
    let app_id = HSTRING::from(identity.app_id.as_str());
    let group = HSTRING::from(GROUP);
    let cleanup = || -> windows::core::Result<u32> {
        let history = ToastNotificationManager::History()?;
        let entries = history.GetHistoryWithId(&app_id)?;
        let mut removed = 0;
        for index in 0..entries.Size()? {
            let item = entries.GetAt(index)?;
            let tag = item.Tag()?;
            let valid_tag = tag.to_string_lossy();
            if item.Group()? == group
                && valid_tag.len() == 12
                && valid_tag
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                history.RemoveGroupedTagWithId(&tag, &group, &app_id)?;
                removed += 1;
            }
        }
        for _ in 0..3 {
            if history.GetHistoryWithId(&app_id)?.Size()? == 0 {
                return Ok(removed);
            }
            std::thread::sleep(std::time::Duration::from_millis(80));
        }
        Err(E_INVALIDARG.into())
    };
    cleanup().map_err(|error| {
        native_error(
            "Clear and verify the owned Threadline fixture notification history",
            error,
        )
    })
}

struct Route {
    app_id: String,
    current: Mutex<Option<String>>,
    events: UnboundedSender<PlatformEvent>,
}

#[derive(Clone, Copy)]
enum Source {
    Banner,
    Center,
}

impl Route {
    fn activate(&self, app_id: &str, payload: &str, source: Source) -> bool {
        if app_id != self.app_id {
            return false;
        }
        if let Ok(activation) = ActivityActivation::parse(payload) {
            // The account's persistent journal verifies this opaque ID again before routing.
            return self
                .events
                .send(PlatformEvent::ActivityActivated(activation))
                .is_ok();
        }
        let Ok(mut current) = self.current.lock() else {
            return false;
        };
        if app_id != self.app_id || current.as_deref() != Some(payload) {
            return false;
        }
        current.take();
        if matches!(source, Source::Center) {
            let _ = self.events.send(PlatformEvent::NotificationComActivated);
        }
        self.events
            .send(PlatformEvent::Activate(Activation::OpenReview))
            .is_ok()
    }
}

#[implement(INotificationActivationCallback)]
struct ActivationCallback(Arc<Route>);

impl INotificationActivationCallback_Impl for ActivationCallback_Impl {
    fn Activate(
        &self,
        app_id: &PCWSTR,
        payload: &PCWSTR,
        _data: *const NOTIFICATION_USER_INPUT_DATA,
        _count: u32,
    ) -> windows::core::Result<()> {
        if app_id.is_null() || payload.is_null() {
            return Err(E_INVALIDARG.into());
        }
        // COM marshals the two non-null, NUL-terminated strings for this invocation.
        let app_id = unsafe { app_id.to_string()? };
        let payload = unsafe { payload.to_string()? };
        if !self.0.activate(&app_id, &payload, Source::Center) {
            return Err(E_INVALIDARG.into());
        }
        Ok(())
    }
}

#[implement(IClassFactory)]
struct ActivationFactory(Arc<Route>);

impl IClassFactory_Impl for ActivationFactory_Impl {
    fn CreateInstance(
        &self,
        outer: Ref<IUnknown>,
        iid: *const GUID,
        output: *mut *mut c_void,
    ) -> windows::core::Result<()> {
        if iid.is_null() || output.is_null() {
            return Err(E_POINTER.into());
        }
        // COM owns the caller's output pointer for the duration of this method.
        unsafe { *output = std::ptr::null_mut() };
        if outer.as_ref().is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        let handler: INotificationActivationCallback =
            ActivationCallback(Arc::clone(&self.0)).into();
        unsafe { handler.query(iid, output).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> windows::core::Result<()> {
        Ok(())
    }
}

struct Apartment;

impl Drop for Apartment {
    fn drop(&mut self) {
        // This guard is created after RoInitialize succeeds, and is owned by that worker thread.
        unsafe { RoUninitialize() };
    }
}

pub(super) struct Toasts {
    identity: Identity,
    route: Arc<Route>,
    registration: u32,
    pending: Option<PendingToast>,
    _apartment: Apartment,
}

impl Toasts {
    pub(super) fn register(
        identity: Identity,
        events: UnboundedSender<PlatformEvent>,
    ) -> Result<Self, PlatformError> {
        // The window worker initializes and releases the apartment and registration on this thread.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
            .map_err(|error| native_error("Initialize Windows notifications", error))?;
        let apartment = Apartment;
        let route = Arc::new(Route {
            app_id: identity.app_id.clone(),
            current: Mutex::new(None),
            events,
        });
        let factory: IClassFactory = ActivationFactory(Arc::clone(&route)).into();
        let registration = unsafe {
            CoRegisterClassObject(
                &identity.class,
                &factory,
                CLSCTX_LOCAL_SERVER,
                REGCLS_MULTIPLEUSE,
            )
        }
        .map_err(|error| native_error("Register the Threadline notification activator", error))?;
        Ok(Self {
            identity,
            route,
            registration,
            pending: None,
            _apartment: apartment,
        })
    }

    fn activity_notifier(&self) -> Result<ToastNotifier, PlatformError> {
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(
            self.identity.app_id.as_str(),
        ))
        .map_err(|error| native_error("Find the Threadline activity sender", error))
    }
    pub(super) fn activity_permission(&self) -> Result<ActivityPermission, PlatformError> {
        sender_permission(&self.activity_notifier()?)
    }
    pub(super) fn submit_activity(
        &self,
        notice: ActivityNotification,
    ) -> Result<(), PlatformError> {
        let notifier = self.activity_notifier()?;
        let permission = sender_permission(&notifier)?;
        if permission == ActivityPermission::Disabled {
            return Err(PlatformError::Denied);
        }
        let document =
            XmlDocument::new().map_err(|e| native_error("Create the activity card", e))?;
        document
            .LoadXml(&HSTRING::from(notice.xml()))
            .map_err(|e| native_error("Load the activity card", e))?;
        let toast = ToastNotification::CreateToastNotification(&document)
            .map_err(|e| native_error("Create the activity notification", e))?;
        let key = notice.id.key();
        toast
            .SetTag(&HSTRING::from(&key[..16]))
            .map_err(|e| native_error("Name the activity notification", e))?;
        toast
            .SetGroup(&HSTRING::from(&key[16..]))
            .map_err(|e| native_error("Group the activity notification", e))?;
        // Windows retains the card across orderly application exit. The registered COM class handles all three actions.
        notifier
            .Show(&toast)
            .map_err(|e| native_error("Submit activity to Windows", e))?;
        if permission == ActivityPermission::SenderSettingUnknown {
            corroborate_unknown_sender(&self.identity.app_id, &key)?;
        }
        Ok(())
    }

    pub(super) fn show_review(&mut self) -> Result<(), PlatformError> {
        let app_id = HSTRING::from(self.identity.app_id.as_str());
        let notifier = ToastNotificationManager::CreateToastNotifierWithId(&app_id)
            .map_err(|error| native_error("Find the Threadline notification sender", error))?;
        if let Ok(setting) = notifier.Setting()
            && setting != NotificationSetting::Enabled
        {
            return Err(PlatformError::Denied);
        }
        // The launch token gates both the transient event and the COM callback from a retained card.
        let token = format!(
            "{:032x}",
            unsafe { CoCreateGuid() }
                .map_err(|error| native_error("Create the Threadline notification token", error))?
                .to_u128()
        );
        let tag = HSTRING::from(&token[..12]);
        let group = HSTRING::from(GROUP);
        let document = XmlDocument::new()
            .map_err(|error| native_error("Create the Threadline notification body", error))?;
        let xml = format!(
            "<toast launch='{token}' activationType='foreground'><visual><binding template='ToastGeneric'><text>{}</text><text>{BODY}</text></binding></visual><actions><action content='{}' arguments='{token}' activationType='foreground'/></actions></toast>",
            self.identity.title, self.identity.action
        );
        document
            .LoadXml(&HSTRING::from(xml))
            .map_err(|error| native_error("Load the Threadline notification body", error))?;
        let toast = ToastNotification::CreateToastNotification(&document)
            .map_err(|error| native_error("Create the Threadline notification", error))?;
        toast
            .SetTag(&tag)
            .map_err(|error| native_error("Tag the Threadline notification", error))?;
        toast
            .SetGroup(&group)
            .map_err(|error| native_error("Group the Threadline notification", error))?;

        let mut pending = PendingToast {
            notifier,
            toast,
            app_id,
            tag,
            group,
            activated: None,
            failed: None,
            show_attempted: false,
        };
        let route = Arc::clone(&self.route);
        pending.activated = Some(
            pending
                .toast
                .Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(
                    move |_, args| {
                        if let Some(payload) = args
                            .as_ref()
                            .and_then(|value| value.cast::<ToastActivatedEventArgs>().ok())
                            .and_then(|value| value.Arguments().ok())
                        {
                            route.activate(
                                &route.app_id,
                                &payload.to_string_lossy(),
                                Source::Banner,
                            );
                        }
                        Ok(())
                    },
                ))
                .map_err(|error| native_error("Listen for the Threadline notification", error))?,
        );
        let route = Arc::clone(&self.route);
        let expected = token.clone();
        pending.failed = Some(
            pending
                .toast
                .Failed(
                    &TypedEventHandler::<ToastNotification, ToastFailedEventArgs>::new(
                        move |_, args| {
                            let code = args
                                .as_ref()
                                .and_then(|event| event.ErrorCode().ok())
                                .map_or(0, |value| value.0 as u32);
                            if let Ok(mut current) = route.current.lock()
                                && current.as_deref() == Some(&expected)
                            {
                                current.take();
                                let _ = route.events.send(PlatformEvent::Error(
                                    PlatformError::Native {
                                        operation: "Deliver the Threadline notification",
                                        code,
                                    },
                                ));
                            }
                            Ok(())
                        },
                    ),
                )
                .map_err(|error| {
                    native_error("Listen for Threadline notification errors", error)
                })?,
        );

        *self
            .route
            .current
            .lock()
            .map_err(|_| PlatformError::Unavailable)? = Some(token.clone());
        self.pending.take();
        pending.show_attempted = true;
        if let Err(error) = pending.notifier.Show(&pending.toast) {
            if let Ok(mut current) = self.route.current.lock()
                && current.as_deref() == Some(&token)
            {
                current.take();
            }
            return Err(native_error("Submit the Threadline notification", error));
        }
        self.pending = Some(pending);
        let _ = self.route.events.send(PlatformEvent::NotificationSubmitted);
        Ok(())
    }
}

impl Drop for Toasts {
    fn drop(&mut self) {
        if let Ok(mut current) = self.route.current.lock() {
            current.take();
        }
        self.pending.take();
        // This cookie belongs to this worker's class factory registration.
        let _ = unsafe { CoRevokeClassObject(self.registration) };
    }
}

struct PendingToast {
    notifier: ToastNotifier,
    toast: ToastNotification,
    app_id: HSTRING,
    tag: HSTRING,
    group: HSTRING,
    activated: Option<i64>,
    failed: Option<i64>,
    show_attempted: bool,
}

impl Drop for PendingToast {
    fn drop(&mut self) {
        if let Some(token) = self.activated {
            let _ = self.toast.RemoveActivated(token);
        }
        if let Some(token) = self.failed {
            let _ = self.toast.RemoveFailed(token);
        }
        if self.show_attempted {
            let _ = self.notifier.Hide(&self.toast);
            if let Ok(history) = ToastNotificationManager::History() {
                let _ = history.RemoveGroupedTagWithId(&self.tag, &self.group, &self.app_id);
            }
        }
    }
}

fn native_error(operation: &'static str, error: Error) -> PlatformError {
    PlatformError::Native {
        operation,
        code: error.code().0 as u32,
    }
}

fn sender_permission(notifier: &ToastNotifier) -> Result<ActivityPermission, PlatformError> {
    match notifier.Setting() {
        Ok(value) if value == NotificationSetting::Enabled => Ok(ActivityPermission::Enabled),
        Ok(_) => Ok(ActivityPermission::Disabled),
        // A registered unpackaged sender may have no preference record before its first submission.
        // Windows still applies its current policy when the application calls Show.
        Err(error) if error.code() == windows::core::HRESULT::from_win32(ERROR_NOT_FOUND.0) => {
            Ok(ActivityPermission::SenderSettingUnknown)
        }
        Err(error) => Err(native_error("Read Windows notification preferences", error)),
    }
}

fn corroborate_unknown_sender(app_id: &str, key: &str) -> Result<(), PlatformError> {
    let history = ToastNotificationManager::History()
        .map_err(|e| native_error("Open Windows notification history", e))?;
    let sender = HSTRING::from(app_id);
    let tag = HSTRING::from(&key[..16]);
    let group = HSTRING::from(&key[16..]);
    for attempt in 0..6 {
        let contains = || -> windows::core::Result<bool> {
            let entries = history.GetHistoryWithId(&sender)?;
            for index in 0..entries.Size()? {
                let entry = entries.GetAt(index)?;
                if entry.Tag()? == tag && entry.Group()? == group {
                    return Ok(true);
                }
            }
            Ok(false)
        };
        match contains() {
            Ok(true) => return Ok(()),
            Ok(false) => (),
            Err(error) if error.code() == windows::core::HRESULT::from_win32(ERROR_NOT_FOUND.0) => {
            }
            Err(error) => {
                return Err(native_error(
                    "Read the submitted Windows activity group",
                    error,
                ));
            }
        }
        if attempt < 5 {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    Err(PlatformError::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_accepts_only_the_expected_app_and_single_live_token() {
        let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let route = Route {
            app_id: "Threadline.Desktop".into(),
            current: Mutex::new(Some("live-token".into())),
            events,
        };
        assert!(!route.activate("Different.App", "live-token", Source::Center));
        assert!(!route.activate("Threadline.Desktop", "wrong-token", Source::Center));
        assert!(receiver.try_recv().is_err());
        assert!(route.activate("Threadline.Desktop", "live-token", Source::Center));
        assert!(!route.activate("Threadline.Desktop", "live-token", Source::Banner));
        assert_eq!(
            receiver.try_recv().unwrap(),
            PlatformEvent::NotificationComActivated
        );
        assert_eq!(
            receiver.try_recv().unwrap(),
            PlatformEvent::Activate(Activation::OpenReview)
        );
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn fixture_registration_matches_its_scoped_token_only() {
        let identity = Identity::fixture("0123456789abcdef0123456789abcdef").unwrap();
        assert_eq!(
            identity.app_id,
            "Threadline.Feasibility.0123456789abcdef0123456789abcdef"
        );
        assert_eq!(
            identity.class.to_u128(),
            0x01234567_89ab_cdef_0123_456789abcdef
        );
        assert_eq!(identity.title, "Threadline review 01234567");
        assert_eq!(identity.action, "Open review 01234567");
        assert!(Identity::fixture("00000000000000000000000000000000").is_err());
        assert!(Identity::fixture("5a20cea8a5d84b5e9be34121ba0aa753").is_err());
    }
}
