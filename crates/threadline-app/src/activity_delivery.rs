use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use threadline_core::sync::policy::system_timezone;
use threadline_platform::{ActivityId, ActivityNotification, PlatformControls, PlatformError};
use threadline_storage::{
    Store, UtcMillis,
    notifications::{PopupAvailability, PopupOutcome},
};

pub(crate) struct ActivityDelivery {
    stop: Arc<AtomicBool>,
    wake: mpsc::SyncSender<()>,
}
impl ActivityDelivery {
    pub(crate) fn start(store: &Arc<Store>, native: PlatformControls) -> Option<Self> {
        let (wake, rx) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        store.watch_commits(wake.clone()).ok()?;
        let worker_stop = stop.clone();
        let weak = Arc::downgrade(store);
        std::thread::Builder::new()
            .name("threadline-activity".into())
            .spawn(move || run(weak, native, rx, worker_stop))
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
    native: PlatformControls,
    wake: mpsc::Receiver<()>,
    stop: Arc<AtomicBool>,
) {
    while wake.recv().is_ok() && !stop.load(Ordering::Acquire) {
        let Some(store) = store.upgrade() else { break };
        if let Err(error) = deliver(&store, &native, &stop) {
            eprintln!("Threadline activity delivery: {error}");
        }
    }
}
fn deliver(
    store: &Store,
    native: &PlatformControls,
    stop: &AtomicBool,
) -> threadline_storage::Result<()> {
    let Some(account) = store.active_github_account()? else {
        return Ok(());
    };
    if !store.notification_work_pending(account)? {
        return Ok(());
    }
    let zone = system_timezone().map_err(threadline_storage::Error::InvalidInput)?;
    use threadline_platform::ActivityPermission;
    let availability = match native.activity_permission() {
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
    };
    while !stop.load(Ordering::Acquire)
        && store.process_notification_events(
            account,
            UtcMillis::now()?.0,
            &zone,
            availability,
            128,
        )? == 128
    {}
    while !stop.load(Ordering::Acquire) {
        let Some(group) =
            store.claim_notification_popup(account, UtcMillis::now()?.0, &zone, availability)?
        else {
            break;
        };
        let id =
            ActivityId::parse(&group.native_id).map_err(|_| threadline_storage::Error::Corrupt)?;
        let result = native.submit_activity(ActivityNotification {
            id,
            events: group.event_count,
            sound: group.sound,
        });
        let outcome = match result {
            Ok(()) => PopupOutcome::Delivered,
            Err(
                PlatformError::Unsupported
                | PlatformError::Denied
                | PlatformError::NotificationRegistrationMissing,
            ) => PopupOutcome::Rejected,
            Err(_) => PopupOutcome::Unknown,
        };
        store.record_notification_outcome(
            account,
            &group.native_id,
            outcome,
            UtcMillis::now()?.0,
        )?;
    }
    Ok(())
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
