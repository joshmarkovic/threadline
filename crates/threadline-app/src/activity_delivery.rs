use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;
use threadline_core::display_time::DisplayZone;
use threadline_platform::{ActivityId, ActivityNotification, PlatformControls, PlatformError};
use threadline_storage::{
    Store, UtcMillis,
    notifications::{PopupAvailability, PopupGroup, PopupOutcome},
};

enum DeliveryTarget {
    Native(PlatformControls),
    #[cfg(feature = "e2e")]
    Capture(crate::activity_fixture::Capture),
}

#[cfg(all(test, feature = "e2e"))]
pub(crate) fn deliver_to_test_capture(store: &Store, output: &tempfile::NamedTempFile) {
    let target = DeliveryTarget::Capture(crate::activity_fixture::Capture::for_test(output));
    deliver(store, &target, &AtomicBool::new(false), DisplayZone::UTC).unwrap();
}

impl DeliveryTarget {
    fn availability(&self) -> PopupAvailability {
        use threadline_platform::ActivityPermission;
        let native = match self {
            Self::Native(native) => native,
            #[cfg(feature = "e2e")]
            Self::Capture(_) => return PopupAvailability::Enabled,
        };
        match native.activity_permission() {
            Ok(ActivityPermission::Enabled) => PopupAvailability::Enabled,
            Ok(ActivityPermission::Disabled) => PopupAvailability::Disabled,
            Ok(ActivityPermission::SenderSettingUnknown) => PopupAvailability::SenderSettingUnknown,
            Err(PlatformError::Denied) => PopupAvailability::Disabled,
            Err(PlatformError::NotificationRegistrationMissing) => {
                PopupAvailability::RegistrationMissing
            }
            Err(error) => {
                eprintln!("Threadline could not query the desktop notification service: {error}");
                PopupAvailability::Unavailable
            }
        }
    }

    fn submit(&self, group: &PopupGroup) -> threadline_storage::Result<PopupOutcome> {
        let native = match self {
            Self::Native(native) => native,
            #[cfg(feature = "e2e")]
            Self::Capture(capture) => {
                capture.record(group)?;
                return Ok(PopupOutcome::Delivered);
            }
        };
        let id =
            ActivityId::parse(&group.native_id).map_err(|_| threadline_storage::Error::Corrupt)?;
        Ok(
            match native.submit_activity(ActivityNotification {
                id,
                events: group.event_count,
                sound: group.sound,
            }) {
                Ok(()) => PopupOutcome::Delivered,
                Err(
                    PlatformError::Unsupported
                    | PlatformError::Denied
                    | PlatformError::NotificationRegistrationMissing,
                ) => PopupOutcome::Rejected,
                Err(_) => PopupOutcome::Unknown,
            },
        )
    }
}

pub(crate) struct ActivityDelivery {
    stop: Arc<AtomicBool>,
    wake: mpsc::SyncSender<()>,
}
impl ActivityDelivery {
    pub(crate) fn wake(&self) {
        let _ = self.wake.try_send(());
    }
    pub(crate) fn start(
        store: &Arc<Store>,
        native: PlatformControls,
        zone: crate::display_time::CalendarZone,
    ) -> Option<Self> {
        let target = DeliveryTarget::Native(native);
        #[cfg(feature = "e2e")]
        let target = if std::env::var_os("THREADLINE_E2E_NATIVE_ACTIVITY").is_some() {
            DeliveryTarget::Capture(crate::activity_fixture::Capture::open()?)
        } else {
            target
        };
        let (wake, rx) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        store.watch_commits(wake.clone()).ok()?;
        let worker_stop = stop.clone();
        let weak = Arc::downgrade(store);
        std::thread::Builder::new()
            .name("threadline-activity".into())
            .spawn(move || run(weak, target, rx, worker_stop, zone))
            .ok()?;
        let _ = wake.try_send(());
        Some(Self { stop, wake })
    }
}
impl Drop for ActivityDelivery {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
    }
}
fn run(
    store: Weak<Store>,
    target: DeliveryTarget,
    wake: mpsc::Receiver<()>,
    stop: Arc<AtomicBool>,
    zone: crate::display_time::CalendarZone,
) {
    let mut delay = None;
    loop {
        let awake = match delay {
            Some(delay) => !matches!(
                wake.recv_timeout(delay),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ),
            None => wake.recv().is_ok(),
        };
        if !awake || stop.load(Ordering::Acquire) {
            break;
        }
        let Some(store) = store.upgrade() else { break };
        let current_zone = crate::display_time::calendar_zone(&zone);
        delay = match deliver(&store, &target, &stop, current_zone) {
            Ok(next) => next,
            Err(error) => {
                eprintln!("Threadline activity delivery: {error}");
                None
            }
        };
    }
}
fn deliver(
    store: &Store,
    target: &DeliveryTarget,
    stop: &AtomicBool,
    zone: DisplayZone,
) -> threadline_storage::Result<Option<Duration>> {
    let Some(account) = store.active_github_account()? else {
        return Ok(None);
    };
    if !store.notification_work_pending(account)? {
        return Ok(None);
    }
    let availability = target.availability();
    while !stop.load(Ordering::Acquire)
        && store.process_notification_events(
            account,
            UtcMillis::now()?.0,
            zone.name(),
            availability,
            128,
        )? == 128
    {}
    while !stop.load(Ordering::Acquire) {
        let Some(group) = store.claim_ready_notification_popup(
            account,
            UtcMillis::now()?.0,
            zone.name(),
            availability,
        )?
        else {
            break;
        };
        let outcome = target.submit(&group)?;
        store.record_notification_outcome(
            account,
            &group.native_id,
            outcome,
            UtcMillis::now()?.0,
        )?;
    }
    let now = UtcMillis::now()?.0;
    Ok(store
        .next_notification_popup_at(account)?
        .map(|at| Duration::from_millis(at.saturating_sub(now).clamp(0, 30_000) as u64)))
}

/// A wake-up for cached readers. SQLite retains Open destinations until the matching workspace acknowledges them.
#[derive(Default)]
pub(crate) struct ActivityInbox;
impl gpui_kit::Global for ActivityInbox {}

pub(crate) fn activate(
    store: &Store,
    event: threadline_platform::ActivityActivation,
) -> threadline_storage::Result<bool> {
    use threadline_platform::ActivityAction;
    use threadline_storage::notifications::NotificationAction;
    let Some(account) = store.active_github_account()? else {
        return Ok(false);
    };
    let now = UtcMillis::now()?.0;
    let action = match event.action {
        ActivityAction::Open => NotificationAction::Open,
        ActivityAction::MarkRead => NotificationAction::MarkRead,
        ActivityAction::Snooze => NotificationAction::SnoozeUntil(now.saturating_add(3_600_000)),
    };
    let accepted = store
        .activate_notification(account, &event.id.key(), action, now)?
        .is_some();
    Ok(accepted && event.action == ActivityAction::Open)
}
