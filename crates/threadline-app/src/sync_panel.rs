use std::sync::Arc;

use crate::display_time::DisplayClock;
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement,
    ParentElement, Render, Role, StatefulInteractiveElement, Styled, Window,
    component::{
        Disableable, Selectable, Sizable,
        button::Button,
        input::{Input, InputState},
    },
    div, px,
};
use threadline_storage::{
    AccountId, Store,
    sync::{SyncControls, SyncSettings, SyncStatus, WorkSchedule},
};

use crate::palette::Palette;

pub(crate) struct SettingsChanged {
    pub(crate) previous: SyncControls,
    pub(crate) current: SyncControls,
}

pub(crate) struct SyncPanel {
    store: Arc<Store>,
    runtime: tokio::runtime::Handle,
    account: AccountId,
    details: Option<SyncStatus>,
    saved: Option<SyncSettings>,
    weekdays: u8,
    start: Entity<InputState>,
    end: Entity<InputState>,
    timezone: Entity<InputState>,
    working: Entity<InputState>,
    off_hours: Entity<InputState>,
    visible: Entity<InputState>,
    busy: bool,
    message: Option<&'static str>,
    #[cfg(feature = "e2e")]
    observer: Option<crate::observation::Observer>,
}

impl EventEmitter<SettingsChanged> for SyncPanel {}

#[derive(Clone, Copy)]
enum Control {
    Offline,
    PauseReads,
    PauseWrites,
    Quiet,
}

impl Control {
    fn flag(self, controls: &mut SyncControls) -> &mut bool {
        match self {
            Self::Offline => &mut controls.work_offline,
            Self::PauseReads => &mut controls.pause_background,
            Self::PauseWrites => &mut controls.pause_queued_actions,
            Self::Quiet => &mut controls.notification_quiet,
        }
    }
}

impl SyncPanel {
    #[cfg(feature = "e2e")]
    pub(crate) fn set_observer(&mut self, observer: Option<crate::observation::Observer>) {
        self.observer = observer;
    }

    #[cfg(feature = "e2e")]
    pub(crate) fn saved_settings(&self) -> Option<&SyncSettings> {
        self.saved.as_ref()
    }

    fn observed_control(&self, id: &str, enabled: bool, element: impl IntoElement) -> AnyElement {
        #[cfg(feature = "e2e")]
        if enabled && let Some(observer) = self.observer.as_ref() {
            return div()
                .relative()
                .child(element)
                .child(observer.visible_control_bounds(id.to_owned()))
                .into_any_element();
        }
        #[cfg(not(feature = "e2e"))]
        let _ = (id, enabled);
        element.into_any_element()
    }
    pub(crate) fn set_status(&mut self, status: Option<SyncStatus>, cx: &mut Context<Self>) {
        if self.details != status {
            self.details = status;
            cx.notify();
        }
    }
    pub(crate) fn new(
        store: Arc<Store>,
        runtime: tokio::runtime::Handle,
        account: AccountId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut input = |label| cx.new(|cx| InputState::new(window, cx).placeholder(label));
        let mut panel = Self {
            store,
            runtime,
            account,
            details: None,
            saved: None,
            weekdays: 0,
            start: input("09:00"),
            end: input("17:00"),
            timezone: input("User time zone"),
            working: input("120"),
            off_hours: input("1800"),
            visible: input("60"),
            busy: false,
            message: None,
            #[cfg(feature = "e2e")]
            observer: None,
        };
        panel.load(window, cx, true);
        panel
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>, load_form: bool) {
        let account = self.account;
        let store = Arc::clone(&self.store);
        self.busy = true;
        let task = self.runtime.spawn_blocking(move || {
            Ok::<_, threadline_storage::Error>((
                store.sync_settings(account)?,
                store.sync_status(account)?,
            ))
        });
        cx.spawn_in(window, async move |panel, cx| {
            let result = task.await;
            let _ = panel.update_in(cx, |panel, window, cx| {
                panel.busy = false;
                match result {
                    Ok(Ok((settings, status))) => {
                        if load_form || panel.saved.is_none() {
                            panel.set_inputs(&settings.schedule, window, cx);
                        }
                        panel.saved = Some(settings);
                        panel.details = Some(status);
                        panel.message = None;
                    }
                    _ => panel.message = Some("Saved synchronization settings are unavailable."),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn set_inputs(&mut self, schedule: &WorkSchedule, window: &mut Window, cx: &mut Context<Self>) {
        self.weekdays = schedule.weekdays;
        let fields = [
            (&self.start, format_time(schedule.start_minute)),
            (&self.end, format_time(schedule.end_minute)),
            (
                &self.timezone,
                schedule.timezone.clone().unwrap_or_default(),
            ),
            (&self.working, schedule.work_interval_secs.to_string()),
            (
                &self.off_hours,
                schedule.off_hours_interval_secs.to_string(),
            ),
            (&self.visible, schedule.visible_interval_secs.to_string()),
        ];
        for (input, value) in fields {
            input.update(cx, |input, cx| input.set_value(value, window, cx));
        }
    }

    fn input_schedule(&self, cx: &Context<Self>) -> Result<WorkSchedule, &'static str> {
        let zone = self.timezone.read(cx).value().trim().to_string();
        let timezone = (!zone.is_empty()).then_some(zone);
        let start_minute = parse_time(self.start.read(cx).value().as_ref(), false)?;
        let end_minute = parse_time(self.end.read(cx).value().as_ref(), true)?;
        let schedule = WorkSchedule {
            weekdays: self.weekdays,
            start_minute,
            end_minute,
            timezone,
            work_interval_secs: seconds(self.working.read(cx).value().as_ref())?,
            off_hours_interval_secs: seconds(self.off_hours.read(cx).value().as_ref())?,
            visible_interval_secs: seconds(self.visible.read(cx).value().as_ref())?,
        };
        schedule.validate()?;
        Ok(schedule)
    }

    fn save_schedule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saved.is_none() {
            return;
        }
        match self.input_schedule(cx) {
            Ok(schedule) => {
                self.persist(move |settings| settings.schedule = schedule, window, cx);
            }
            Err(issue) => {
                self.message = Some(issue);
                cx.notify();
            }
        }
    }

    fn toggle(&mut self, control: Control, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut shown) = self.saved.as_ref().map(|settings| settings.controls) else {
            return;
        };
        let enabled = !*control.flag(&mut shown);
        self.persist(
            move |settings| *control.flag(&mut settings.controls) = enabled,
            window,
            cx,
        );
    }

    fn persist(
        &mut self,
        change: impl FnOnce(&mut SyncSettings) + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message = None;
        let store = Arc::clone(&self.store);
        let account = self.account;
        let task = self.runtime.spawn_blocking(move || {
            let (prior, settings) = store.update_sync_settings(account, change)?;
            let status = store.sync_status(account).ok();
            Ok::<_, threadline_storage::Error>((prior.controls, settings, status))
        });
        cx.spawn_in(window, async move |panel, cx| {
            let result = task.await;
            let _ = panel.update_in(cx, |panel, _, cx| {
                panel.busy = false;
                match result {
                    Ok(Ok((previous, settings, status))) => {
                        let current = settings.controls;
                        panel.saved = Some(settings);
                        panel.details = status;
                        panel.message = Some("Synchronization settings saved.");
                        cx.emit(SettingsChanged { previous, current });
                    }
                    _ => panel.message = Some("Synchronization settings could not be saved."),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn check_now(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.dispatch_action(Box::new(crate::workspace::CheckAccountAccess), cx);
    }
}

impl Render for SyncPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let zone = DisplayClock::current(cx);
        let palette = Palette::current(cx);
        let controls = self
            .saved
            .as_ref()
            .map_or(SyncControls::default(), |settings| settings.controls);
        let unavailable = self.saved.is_none() || self.busy;
        let dials = [
            (
                "sync-work-offline",
                "Work offline",
                Control::Offline,
                controls.work_offline,
            ),
            (
                "sync-pause-background",
                "Pause background refresh",
                Control::PauseReads,
                controls.pause_background,
            ),
            (
                "sync-pause-outbox",
                "Pause queued actions",
                Control::PauseWrites,
                controls.pause_queued_actions,
            ),
            (
                "sync-quiet-notifications",
                "Quiet notifications",
                Control::Quiet,
                controls.notification_quiet,
            ),
        ];
        let fields = [
            ("sync-start", "Work starts (HH:MM)", &self.start),
            ("sync-end", "Work ends (HH:MM)", &self.end),
            (
                "sync-timezone",
                "Work schedule time zone override",
                &self.timezone,
            ),
            (
                "sync-work-interval",
                "Work-hours interval in seconds",
                &self.working,
            ),
            (
                "sync-off-interval",
                "Off-hours interval in seconds",
                &self.off_hours,
            ),
            (
                "sync-visible-interval",
                "Visible item interval in seconds",
                &self.visible,
            ),
        ];
        div()
            .id("sync-settings-panel")
            .role(Role::Group)
            .aria_label("Synchronization settings")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .bg(palette.panel)
            .child(div().text_lg().child("Synchronization"))
            .child(
                div().flex().flex_wrap().gap_2().children(
                    dials
                        .into_iter()
                        .map(|(id, label, target, active)| {
                            let toggle = button(
                                id,
                                format!("{label} · {}", if active { "On" } else { "Off" }),
                            )
                            .selected(active)
                            .disabled(unavailable)
                            .on_click(cx.listener(
                                move |panel, _, window, cx| panel.toggle(target, window, cx),
                            ));
                            self.observed_control(id, !unavailable, toggle)
                        })
                        .collect::<Vec<_>>(),
                ),
            )
            .child(div().text_sm().child("Workdays"))
            .child(
                div().flex().flex_wrap().gap_2().children(
                    [
                        "Monday",
                        "Tuesday",
                        "Wednesday",
                        "Thursday",
                        "Friday",
                        "Saturday",
                        "Sunday",
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(index, label)| {
                        let id = format!("sync-day-{index}");
                        let toggle = button(id.clone(), label)
                            .selected(self.weekdays & (1 << index) != 0)
                            .disabled(unavailable)
                            .on_click(cx.listener(move |panel, _, _, cx| {
                                panel.weekdays ^= 1 << index;
                                cx.notify();
                            }));
                        self.observed_control(&id, !unavailable, toggle)
                    })
                    .collect::<Vec<_>>(),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_3()
                    .children(fields.into_iter().map(|(id, label, field)| {
                        div()
                            .w(px(231.))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_xs().child(label))
                            .child(
                                self.observed_control(
                                    id,
                                    !unavailable,
                                    Input::new(field)
                                        .small()
                                        .disabled(unavailable)
                                        .accessibility_id(id)
                                        .aria_label(label),
                                ),
                            )
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(palette.muted)
                    .child(format!("Default time zone · {}", zone.name())),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        self.observed_control(
                            "sync-save-schedule",
                            !unavailable,
                            button("sync-save-schedule", "Save schedule")
                                .disabled(unavailable)
                                .on_click(cx.listener(|panel, _, window, cx| {
                                    panel.save_schedule(window, cx)
                                })),
                        ),
                    )
                    .child(
                        self.observed_control(
                            "sync-check-now",
                            !unavailable && !controls.work_offline,
                            button("sync-check-now", "Check now")
                                .disabled(unavailable || controls.work_offline)
                                .on_click(
                                    cx.listener(|panel, _, window, cx| panel.check_now(window, cx)),
                                ),
                        ),
                    )
                    .child(
                        button("sync-refresh-status", "Refresh status")
                            .disabled(self.busy)
                            .on_click(
                                cx.listener(|panel, _, window, cx| panel.load(window, cx, false)),
                            ),
                    ),
            )
            .children(self.message.map(|message| {
                div()
                    .id("sync-message")
                    .role(Role::Status)
                    .text_sm()
                    .child(message)
            }))
            .children(self.details.as_ref().map(|details| {
                div()
                    .id("sync-pending-status")
                    .role(Role::Status)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(format!("Pending reads · {}", details.pending))
                    .child(format!(
                        "Last successful sync · {}",
                        zone.label(details.last_success_ms, "none recorded")
                    ))
                    .children(
                        details
                            .last_failure
                            .as_ref()
                            .map(|reason| format!("Most recent read failure · {reason}")),
                    )
                    .children(details.budget_not_before_ms.map(|time| {
                        format!(
                            "Server budget resumes · {}",
                            zone.label(Some(time), "none recorded")
                        )
                    }))
            }))
    }
}

fn button(
    id: impl Into<gpui_kit::SharedString>,
    label: impl Into<gpui_kit::SharedString>,
) -> Button {
    let id = id.into();
    Button::new(id.clone())
        .small()
        .label(label)
        .accessibility_id(id)
}

fn parse_time(value: &str, end: bool) -> Result<u16, &'static str> {
    let parse = || {
        let (hour, minute) = value.trim().split_once(':')?;
        if hour.len() != 2 || minute.len() != 2 {
            return None;
        }
        let (hour, minute) = (hour.parse::<u16>().ok()?, minute.parse::<u16>().ok()?);
        ((hour < 24 && minute < 60) || (end && hour == 24 && minute == 0))
            .then_some(hour * 60 + minute)
    };
    parse().ok_or("Enter work hours as HH:MM.")
}

fn format_time(minute: u16) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

fn seconds(value: &str) -> Result<u32, &'static str> {
    value
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or("Refresh intervals must be positive seconds.")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn work_windows_parse_clock_boundaries_and_positive_intervals() {
        assert_eq!(parse_time("09:00", false), Ok(540));
        assert_eq!(parse_time("24:00", true), Ok(1440));
        assert!(parse_time("24:00", false).is_err());
        assert!(parse_time("25:00", true).is_err());
        assert!(parse_time("12:60", true).is_err());
        assert!(parse_time("9:00", false).is_err());
        assert_eq!(format_time(180), "03:00");
        assert_eq!(seconds("1800"), Ok(1800));
        assert!(seconds("0").is_err());
    }
}

#[cfg(all(test, feature = "e2e"))]
mod ui_tests {
    use super::*;
    use gpui_kit::{
        AnyWindowHandle, Bounds, Focusable, Point, TestAppContext, WindowBounds, WindowOptions,
        size, test::TestWindowExt,
    };

    #[gpui_kit::test]
    fn settings_controls_persist_separate_pause_flags_and_work_window(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let profile = tempfile::tempdir().unwrap();
        let path = profile.path().join("sync-profile.sqlite");
        let store = Arc::new(Store::open(&path).unwrap());
        let account = store.ensure_account("github.com", 81).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        let (window, panel) = cx.update(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1090.), px(820.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| SyncPanel::new(store, handle, account, window, cx)),
            )
            .unwrap()
        });
        let ready = |cx: &TestAppContext| {
            cx.read(|cx| !panel.read(cx).busy && panel.read(cx).saved.is_some())
        };
        wait_for(cx, ready);

        click(cx, window, "sync-day-5");
        cx.update_window(window, |_, window, cx| {
            let input = panel.read(cx).timezone.clone();
            input.focus_handle(cx).focus(window, cx);
            window.render_frame(cx);
            window.input("America/New_York", cx);
            window.click("sync-save-schedule", cx);
        })
        .unwrap();
        wait_for(cx, ready);
        Store::open(&path)
            .unwrap()
            .update_sync_settings(account, |settings| settings.controls.work_offline = true)
            .unwrap();
        for id in [
            "sync-pause-background",
            "sync-pause-outbox",
            "sync-quiet-notifications",
        ] {
            click(cx, window, id);
            wait_for(cx, ready);
        }
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find("sync-check-now").disabled(), Some(true));
        })
        .unwrap();
        let saved = Store::open(&path).unwrap().sync_settings(account).unwrap();
        assert_eq!(saved.schedule.timezone.as_deref(), Some("America/New_York"));
        assert_eq!(saved.schedule.weekdays, 0b0111111);
        assert_eq!(saved.schedule.work_interval_secs, 120);
        assert_eq!(
            saved.controls,
            SyncControls {
                work_offline: true,
                pause_background: true,
                pause_queued_actions: true,
                notification_quiet: true,
            }
        );
        click(cx, window, "sync-pause-background");
        wait_for(cx, ready);
        let cleared = Store::open(&path).unwrap().sync_settings(account).unwrap();
        assert!(!cleared.controls.pause_background);
        assert!(
            cleared.controls.pause_queued_actions
                && cleared.controls.work_offline
                && cleared.controls.notification_quiet
        );
        click(cx, window, "sync-work-offline");
        wait_for(cx, ready);
        let online = Store::open(&path).unwrap().sync_settings(account).unwrap();
        assert!(!online.controls.work_offline);
        assert!(online.controls.pause_queued_actions && online.controls.notification_quiet);
    }

    fn click(cx: &mut TestAppContext, window: AnyWindowHandle, id: &'static str) {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        })
        .unwrap();
    }

    fn wait_for(cx: &mut TestAppContext, ready: impl Fn(&TestAppContext) -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            cx.run_until_parked();
            if ready(cx) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("synchronization settings did not finish persisting");
    }
}
