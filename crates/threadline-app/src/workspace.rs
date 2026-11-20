use std::num::NonZeroUsize;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::base::text::TextViewState;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::diff::{DiffFile, DiffMode, DiffState};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Disableable, Selectable, Sizable};
use gpui_kit::{
    Action, AnyElement, App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyBinding, KeyBindingContextPredicate, MouseButton,
    ParentElement, PathBuilder, Pixels, Role, ScrollHandle, StatefulInteractiveElement, Styled,
    Subscription, TestSupportExt, Window, actions, canvas, div, point, prelude::FluentBuilder, px,
    relative,
};
use threadline_core::{
    fixtures::FeasibilityFixture,
    github::{ScriptedProvider, ScriptedReply},
    sync::{Cancellation, Scheduler, TokioClock},
};
use threadline_platform::{Activation, PlatformControls, PlatformEvent};

#[cfg(feature = "e2e")]
use crate::observation::{Observer, Snapshot};
use crate::palette::Palette;
use crate::pane_content::{DiscussionPane, ReviewDiff, markdown, pane_size};
use crate::{
    account_panel::AccountPanel,
    accounts::AccountService,
    fixtures::{HISTORY, LINKED_BODIES, PR_BODIES, PRIMARY_PR, SECONDARY_PR},
    reader_workspace::{ReaderAccount, ReaderWorkspace},
};

const GUTTER: f32 = 8.;
const WORKSPACE_INSET: f32 = 12.;
const COMPACT_REVIEW_WIDTH: f32 = 960.;

actions!(
    threadline,
    [
        FocusLeft,
        FocusRight,
        FocusDraft,
        CycleTab,
        ToggleContext,
        ToggleAccounts,
        CheckAccountAccess,
        ReloadLocalAccount,
        GrowLeft,
        ShrinkLeft,
        GrowContext,
        ShrinkContext,
        LoadPreview,
        Quit,
    ]
);

#[cfg(all(windows, feature = "e2e"))]
actions!(threadline, [SendTestNotification]);

pub fn bind_shortcuts(cx: &mut App) {
    let context =
        Rc::new(KeyBindingContextPredicate::parse("Threadline").expect("valid workspace context"));
    let demo = Rc::new(
        KeyBindingContextPredicate::parse("ThreadlineFixture").expect("valid fixture context"),
    );
    let shortcuts: [(&str, Box<dyn Action>); 12] = [
        ("secondary-1", Box::new(FocusLeft)),
        ("secondary-2", Box::new(FocusRight)),
        ("secondary-f", Box::new(FocusDraft)),
        ("ctrl-tab", Box::new(CycleTab)),
        ("secondary-shift-l", Box::new(ToggleContext)),
        ("secondary-shift-a", Box::new(ToggleAccounts)),
        ("secondary-left", Box::new(ShrinkLeft)),
        ("secondary-right", Box::new(GrowLeft)),
        ("secondary-shift-left", Box::new(ShrinkContext)),
        ("secondary-shift-right", Box::new(GrowContext)),
        ("secondary-r", Box::new(LoadPreview)),
        ("secondary-q", Box::new(Quit)),
    ];
    let bindings: Vec<_> = shortcuts
        .into_iter()
        .map(|(keys, action)| {
            let predicate = match keys {
                "secondary-shift-a" | "secondary-q" => context.clone(),
                _ => demo.clone(),
            };
            KeyBinding::load(keys, action, Some(predicate), false, None, cx)
                .expect("valid workspace shortcut")
        })
        .collect();
    cx.bind_keys(bindings);
    #[cfg(all(windows, feature = "e2e"))]
    {
        let notification_test = KeyBinding::load(
            "ctrl-shift-f9",
            Box::new(SendTestNotification),
            Some(demo),
            false,
            None,
            cx,
        )
        .expect("valid native notification test shortcut");
        cx.bind_keys([notification_test]);
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Pane {
    Left,
    Right,
}

impl Pane {
    fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ContextMode {
    Body,
    Issue,
}

#[derive(Clone, Copy)]
enum Divider {
    Workspace,
    Review,
}

struct Drag {
    divider: Divider,
    origin: Pixels,
    start_ratio: f32,
    width: f32,
}

enum Preview {
    Idle,
    Loading,
    Ready(String),
    Failed(String),
}

impl Preview {
    fn label(&self) -> &str {
        match self {
            Self::Idle => "Local fixture",
            Self::Loading => "Loading preview…",
            Self::Ready(title) => title,
            Self::Failed(message) => message,
        }
    }
}

struct PrTab {
    body: Entity<TextViewState>,
    issue: Entity<TextViewState>,
    body_scroll: ScrollHandle,
    issue_scroll: ScrollHandle,
    context: ContextMode,
    context_ratio: f32,
    show_compact_context: bool,
    diff: Entity<DiffState>,
    diff_view: Entity<ReviewDiff>,
}

pub struct Workspace {
    seed: u64,
    runtime: tokio::runtime::Handle,
    accounts: Option<Entity<AccountPanel>>,
    updates: Option<Entity<crate::updates::Updates>>,
    reader: Option<Entity<ReaderWorkspace>>,
    route: WorkspaceRoute,
    user_interacted: bool,
    restarting: bool,
    initial_route_chosen: bool,
    feasibility_fixture: bool,
    platform: Option<PlatformControls>,
    activity_delivery: Option<crate::activity_delivery::ActivityDelivery>,
    tray_enabled: bool,
    platform_status: Option<String>,
    notifications_shown: u32,
    notifications_submitted: u32,
    notification_com_activations: u32,
    activations: u32,
    focused: Pane,
    shell_focus: FocusHandle,
    focus: [FocusHandle; 2],
    divider_focus: [FocusHandle; 2],
    selected_tab: [usize; 2],
    inputs: [[Entity<InputState>; 2]; 2],
    outer_ratio: f32,
    drag: Option<Drag>,
    discussion: Entity<DiscussionPane>,
    reviews: [PrTab; 2],
    diff_loaded: bool,
    preview: Preview,
    _subscriptions: Vec<Subscription>,
    #[cfg(feature = "e2e")]
    pub(crate) observer: Option<Observer>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkspaceRoute {
    Home,
    Onboarding,
    Settings,
}

impl Workspace {
    #[cfg(feature = "e2e")]
    pub(crate) fn set_observer(&mut self, observer: Option<Observer>, cx: &mut Context<Self>) {
        if let Some(reader) = self.reader.as_ref()
            && self
                .accounts
                .as_ref()
                .is_some_and(|panel| panel.read(cx).uses_isolated_fixture())
        {
            reader.update(cx, |reader, cx| reader.set_observer(observer.clone(), cx));
        }
        self.observer = observer;
    }
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        seed: u64,
        runtime: tokio::runtime::Handle,
        open_review: bool,
        feasibility_fixture: bool,
    ) -> Self {
        let account_service = crate::profile_store(cx).and_then(AccountService::new);
        Self::new_with_service(
            window,
            cx,
            seed,
            runtime,
            open_review,
            feasibility_fixture,
            account_service,
        )
    }

    pub(crate) fn new_with_service(
        window: &mut Window,
        cx: &mut Context<Self>,
        seed: u64,
        runtime: tokio::runtime::Handle,
        open_review: bool,
        feasibility_fixture: bool,
        account_service: Option<AccountService>,
    ) -> Self {
        let focus = [cx.focus_handle(), cx.focus_handle()];
        let shell_focus = cx.focus_handle();
        let inputs = std::array::from_fn(|_| {
            std::array::from_fn(|_| {
                cx.new(|cx| InputState::new(window, cx).placeholder("Write a local draft…"))
            })
        });
        let mut subscriptions = Vec::new();
        subscriptions.push(cx.observe_window_activation(window, |_, window, cx| {
            if window.is_window_active() {
                crate::display_time::DisplayClock::refresh_device(cx);
            }
        }));
        subscriptions.push(cx.observe_global::<crate::display_time::DisplayClock>(
            |workspace, cx| {
                if let Some(delivery) = &workspace.activity_delivery {
                    delivery.wake();
                }
                cx.notify();
            },
        ));
        for pane in [Pane::Left, Pane::Right] {
            subscriptions.push(
                cx.on_focus_in(&focus[pane.index()], window, move |this, _, cx| {
                    this.focused = pane;
                    cx.notify();
                }),
            );
        }
        #[cfg(feature = "e2e")]
        for draft in inputs.iter().flatten() {
            subscriptions.push(cx.observe(draft, |this, _, cx| {
                if this.observer.is_some() {
                    cx.notify();
                }
            }));
        }
        let focused = if open_review { Pane::Right } else { Pane::Left };
        let store = crate::profile_store(cx);
        let reader = store.as_ref().map(|store| {
            let service = account_service
                .clone()
                .map(crate::reader::ReaderService::new);
            let worker = account_service
                .clone()
                .filter(|_| !feasibility_fixture)
                .map(|accounts| {
                    std::sync::Arc::new(crate::sync_worker::SyncWorker::start(
                        accounts,
                        &runtime,
                        seed,
                        crate::display_time::DisplayClock::subscribe(cx),
                    ))
                });
            cx.new(|cx| {
                let mut reader =
                    ReaderWorkspace::new(store.clone(), runtime.clone(), service, window, cx);
                if let Some(worker) = worker {
                    reader.attach_sync(worker, window, cx);
                }
                reader
            })
        });
        let accounts = account_service
            .map(|service| cx.new(|cx| AccountPanel::new(service, runtime.clone(), window, cx)));
        #[cfg(feature = "e2e")]
        if let Some(reader) = reader.as_ref() {
            subscriptions.push(cx.observe(reader, |workspace, _, cx| {
                if workspace.observer.is_some() {
                    cx.notify();
                }
            }));
        }
        if let Some(panel) = accounts.as_ref() {
            subscriptions.push(cx.subscribe_in(
                panel,
                window,
                |this, panel, event: &crate::account_panel::AccountPanelEvent, window, cx| {
                    match event {
                        crate::account_panel::AccountPanelEvent::OnboardingFinished => {
                            this.navigate(WorkspaceRoute::Home, window, cx)
                        }
                        crate::account_panel::AccountPanelEvent::OpenSynchronization => {
                            if this.route == WorkspaceRoute::Settings
                                && let Some(sync) = this.reader.as_ref().and_then(|reader| {
                                    reader.update(cx, |reader, cx| {
                                        reader.sync_settings_panel(window, cx)
                                    })
                                })
                            {
                                panel.update(cx, |panel, cx| {
                                    panel.open_synchronization(sync, window, cx)
                                });
                            }
                        }
                        crate::account_panel::AccountPanelEvent::RefreshFinished { account } => {
                            if let Some(reader) = &this.reader {
                                reader.update(cx, |reader, cx| {
                                    reader.refresh_home(*account, window, cx)
                                });
                            }
                            cx.notify();
                        }
                    }
                },
            ));
            subscriptions.push(cx.observe_in(panel, window, |this, panel, window, cx| {
                if let Some(reader) = &this.reader {
                    let (view, profile, reachability) = panel.read(cx).local_reader_state();
                    let state = ReaderAccount {
                        account: view.active,
                        offline: view.offline,
                        repositories: view.repositories.clone(),
                        profile: profile.cloned(),
                        reachability,
                    };
                    reader.update(cx, |reader, cx| reader.set_account(state, window, cx));
                }
                if !this.feasibility_fixture
                    && !this.initial_route_chosen
                    && panel.read(cx).loaded()
                {
                    let panel = panel.read(cx);
                    if panel
                        .local_reader_state()
                        .0
                        .background_updates_allowed
                        .is_some()
                    {
                        this.initial_route_chosen = true;
                        if panel.needs_onboarding() && this.route == WorkspaceRoute::Home {
                            this.navigate(WorkspaceRoute::Onboarding, window, cx);
                        }
                    }
                }
                cx.notify();
            }));
        }
        let initial_focus = if feasibility_fixture {
            focus[focused.index()].clone()
        } else {
            shell_focus.clone()
        };
        window.defer(cx, move |window, cx| initial_focus.focus(window, cx));
        let reviews = std::array::from_fn(|index| {
            let diff = cx.new(|cx| DiffState::new([], cx).with_context_lines(None));
            let diff_view = cx.new(|cx| ReviewDiff::new(diff.clone(), index == 0, cx));
            PrTab {
                body: cx.new(|cx| TextViewState::markdown(PR_BODIES[index], cx)),
                issue: cx.new(|cx| TextViewState::markdown(LINKED_BODIES[index], cx)),
                body_scroll: ScrollHandle::new(),
                issue_scroll: ScrollHandle::new(),
                context: ContextMode::Body,
                context_ratio: 0.33,
                show_compact_context: false,
                diff,
                diff_view,
            }
        });
        Self {
            seed,
            runtime,
            accounts,
            updates: None,
            reader,
            route: WorkspaceRoute::Home,
            user_interacted: false,
            restarting: false,
            initial_route_chosen: false,
            feasibility_fixture,
            platform: None,
            activity_delivery: None,
            tray_enabled: false,
            platform_status: None,
            notifications_shown: 0,
            notifications_submitted: 0,
            notification_com_activations: 0,
            activations: 0,
            focused,
            shell_focus,
            focus,
            divider_focus: [cx.focus_handle(), cx.focus_handle()],
            selected_tab: [0, 0],
            inputs,
            outer_ratio: 0.39,
            drag: None,
            discussion: cx.new(DiscussionPane::new),
            reviews,
            diff_loaded: false,
            preview: Preview::Idle,
            _subscriptions: subscriptions,
            #[cfg(feature = "e2e")]
            observer: None,
        }
    }

    pub fn load_patch(&mut self, patches: [Vec<DiffFile>; 2], cx: &mut Context<Self>) {
        for (review, files) in self.reviews.iter().zip(patches) {
            review.diff.update(cx, |diff, cx| diff.set_files(files, cx));
        }
        self.diff_loaded = true;
        cx.notify();
    }

    fn toggle_accounts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.user_interacted = true;
        if self.route == WorkspaceRoute::Onboarding {
            return;
        }
        if self.route == WorkspaceRoute::Home
            && let Some(reader) = self
                .reader
                .as_ref()
                .filter(|reader| reader.read(cx).has_account())
        {
            let this = cx.weak_entity();
            reader.update(cx, |reader, cx| {
                reader.request_departure(
                    false,
                    move |window, cx| {
                        let _ = this.update(cx, |workspace, cx| {
                            workspace.set_accounts_open(true, window, cx)
                        });
                    },
                    window,
                    cx,
                )
            });
            return;
        }
        self.set_accounts_open(self.route == WorkspaceRoute::Home, window, cx);
    }

    fn set_accounts_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        let route = if open {
            WorkspaceRoute::Settings
        } else {
            WorkspaceRoute::Home
        };
        self.navigate(route, window, cx);
    }

    fn navigate(&mut self, route: WorkspaceRoute, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.accounts.as_ref() else {
            return;
        };
        self.route = route;
        let open = route != WorkspaceRoute::Home;
        if let Some(updates) = &self.updates {
            updates.update(cx, |updates, cx| {
                updates.show_settings(route == WorkspaceRoute::Settings, cx)
            });
        }
        if let Some(reader) = self.reader.as_ref() {
            reader.update(cx, |reader, cx| {
                reader.set_workspace_visible(!open);
                cx.notify();
            });
        }
        if self.route != WorkspaceRoute::Home {
            panel.update(cx, |this, cx| {
                #[cfg(feature = "e2e")]
                this.observe_bounds(self.observer.clone());
                let page = if route == WorkspaceRoute::Onboarding {
                    crate::account_panel::AccountPanelPage::Onboarding
                } else {
                    crate::account_panel::AccountPanelPage::Settings
                };
                this.set_page(page, window, cx);
            });
        } else {
            panel.update(cx, |panel, cx| panel.clear_token(window, cx));
            if let Some(reader) = self
                .reader
                .as_ref()
                .filter(|reader| reader.read(cx).has_account())
            {
                reader.read(cx).focus_active(window);
            } else if self.feasibility_fixture {
                self.focus[self.focused.index()].focus(window, cx);
            } else {
                self.shell_focus.focus(window, cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn enable_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let updates = cx.new(|cx| crate::updates::Updates::new(self.runtime.clone(), cx));
        self.attach_updates(updates, window, cx);
    }

    pub(crate) fn attach_updates(
        &mut self,
        updates: Entity<crate::updates::Updates>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._subscriptions.push(cx.subscribe_in(
            &updates,
            window,
            |this, updates, request: &crate::updates::RestartRequested, window, cx| {
                let request = *request;
                if this.restarting
                    || (matches!(request, crate::updates::RestartRequested::Startup)
                        && this.user_interacted)
                {
                    return;
                }
                if this
                    .reader
                    .as_ref()
                    .is_some_and(|reader| reader.read(cx).departing())
                {
                    updates.update(cx, |model, cx| {
                        model.restart_problem(
                            "Finish the workspace change, then choose Restart to update.",
                            cx,
                        )
                    });
                    return;
                }
                this.restarting = true;
                this.shell_focus.focus(window, cx);
                cx.notify();
                let updates = updates.clone();
                if let Some(reader) = this
                    .reader
                    .as_ref()
                    .filter(|reader| reader.read(cx).has_account())
                {
                    reader.update(cx, |reader, cx| {
                        reader.request_departure(
                            false,
                            move |_, cx| updates.update(cx, |model, cx| model.launch(request, cx)),
                            window,
                            cx,
                        );
                    });
                } else {
                    updates.update(cx, |model, cx| model.launch(request, cx));
                }
            },
        ));
        self._subscriptions.push(cx.subscribe_in(
            &updates,
            window,
            |this, _, _: &crate::updates::SetupStarted, window, cx| {
                if let Some(reader) = this
                    .reader
                    .as_ref()
                    .filter(|reader| reader.read(cx).has_account())
                {
                    reader.update(cx, |reader, cx| {
                        reader.finish_saved_departure(window, cx, |_, cx| cx.quit())
                    });
                } else {
                    cx.quit();
                }
            },
        ));
        self._subscriptions.push(cx.subscribe_in(
            &updates,
            window,
            |this, _, _: &crate::updates::SetupStartFailed, window, cx| {
                this.restore_after_update_failure(window, cx);
            },
        ));
        if let Some(reader) = &self.reader {
            let saved_updates = updates.clone();
            self._subscriptions.push(cx.subscribe_in(
                reader,
                window,
                move |this, _, event: &crate::reader_workspace::DepartureRefused, window, cx| {
                    if this.restarting {
                        saved_updates.update(cx, |model, cx| model.restart_problem(event.0, cx));
                        this.restore_after_update_failure(window, cx);
                    }
                },
            ));
            let updates = updates.clone();
            self._subscriptions.push(cx.subscribe(
                reader,
                move |_, _, event: &crate::reader_workspace::UpdatePolicyChanged, cx| {
                    updates.update(cx, |model, cx| model.set_policy(Some(event.0), cx));
                },
            ));
        }
        if let Some(accounts) = &self.accounts {
            accounts.update(cx, |panel, cx| panel.set_updates(updates.clone(), cx));
            let updates = updates.clone();
            let mut last = None;
            self._subscriptions
                .push(cx.observe(accounts, move |_, panel, cx| {
                    let panel = panel.read(cx);
                    if panel.loaded() {
                        let view = panel.local_reader_state().0;
                        let scope = (view.active, view.background_updates_allowed);
                        if last != Some(scope) {
                            last = Some(scope);
                            updates.update(cx, |model, cx| model.set_policy(scope.1, cx));
                        }
                    }
                }));
        }
        self.updates = Some(updates);
    }

    fn restore_after_update_failure(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.restarting = false;
        self.shell_focus.focus(window, cx);
        cx.notify();
    }

    fn request_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.restarting {
            return;
        }
        if let Some(reader) = self
            .reader
            .as_ref()
            .filter(|reader| reader.read(cx).has_account())
        {
            reader.update(cx, |reader, cx| {
                reader.request_departure(true, |_, cx| cx.quit(), window, cx)
            });
        } else {
            cx.quit();
        }
    }

    pub fn connect_platform(
        &mut self,
        controls: PlatformControls,
        mut events: tokio::sync::mpsc::UnboundedReceiver<PlatformEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.feasibility_fixture
            && let Some(store) = crate::profile_store(cx)
        {
            self.activity_delivery = crate::activity_delivery::ActivityDelivery::start(
                &store,
                controls.clone(),
                crate::display_time::DisplayClock::subscribe(cx),
            );
        }
        self.platform = Some(controls);
        let view = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            let Some(view) = view.upgrade() else {
                return true;
            };
            if view.read(cx).restarting {
                return false;
            }
            if view.read(cx).tray_enabled {
                window.minimize_window();
                return false;
            }
            if view
                .read(cx)
                .reader
                .as_ref()
                .is_some_and(|reader| reader.read(cx).has_account())
            {
                view.update(cx, |workspace, cx| workspace.request_quit(window, cx));
                return false;
            }
            true
        });
        cx.spawn_in(window, async move |view, cx| {
            while let Some(event) = events.recv().await {
                let result = cx.update(|window, cx| {
                    view.update(cx, |this, cx| this.platform_event(event, window, cx))
                });
                if !matches!(result, Ok(Ok(()))) {
                    break;
                }
            }
        })
        .detach();
    }

    fn platform_event(
        &mut self,
        event: PlatformEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            PlatformEvent::Activate(activation) => {
                self.activations += 1;
                window.activate_window();
                if matches!(activation, Activation::OpenReview) {
                    self.select_tab(Pane::Right, 0, window, cx);
                }
                self.platform_status = Some("Opened from a native activation".into());
            }
            PlatformEvent::ActivityActivated(event) => {
                if let Some(store) = crate::profile_store(cx) {
                    let work = self
                        .runtime
                        .spawn_blocking(move || crate::activity_delivery::activate(&store, event));
                    cx.spawn_in(window, async move |_, cx| {
                        if matches!(work.await, Ok(Ok(true))) {
                            let _ = cx.update(|window, cx| {
                                window.activate_window();
                                cx.set_global(crate::activity_delivery::ActivityInbox);
                            });
                        }
                    })
                    .detach();
                }
            }
            PlatformEvent::TrayChanged(enabled) => {
                self.tray_enabled = enabled;
                self.platform_status = Some(if enabled {
                    "Closing the window minimizes to the taskbar. Use Quit to exit.".into()
                } else {
                    "Closing the window quits Threadline.".into()
                });
            }
            PlatformEvent::NotificationShown => {
                self.notifications_shown += 1;
                self.platform_status = Some("Windows accepted the test notification.".into());
            }
            PlatformEvent::NotificationSubmitted => {
                self.notifications_submitted += 1;
                self.platform_status = Some("Submitted the test notification to Windows.".into());
            }
            PlatformEvent::NotificationComActivated => self.notification_com_activations += 1,
            PlatformEvent::Quit => self.request_quit(window, cx),
            PlatformEvent::Wake | PlatformEvent::NetworkChanged => {
                crate::display_time::DisplayClock::refresh_device(cx);
                if let Some(reader) = self.reader.as_ref() {
                    let trigger = if event == PlatformEvent::Wake {
                        threadline_core::connectivity::ProbeTrigger::Wake
                    } else {
                        threadline_core::connectivity::ProbeTrigger::NetworkChanged
                    };
                    reader.read(cx).wake_sync(trigger);
                }
            }
            PlatformEvent::Error(error) => self.platform_status = Some(error.to_string()),
        }
        cx.notify();
    }

    fn request_preview(&mut self, window: &Window, cx: &mut Context<Self>) {
        if matches!(self.preview, Preview::Loading) {
            return;
        }
        self.preview = Preview::Loading;
        cx.notify();
        let fixture = FeasibilityFixture::new(self.seed);
        let provider = Arc::new(ScriptedProvider::new(
            Arc::new(TokioClock::new()),
            [ScriptedReply::success(
                fixture.request.item(),
                Duration::from_millis(850),
                fixture.summary,
            )],
        ));
        let scheduler = Scheduler::new(
            self.runtime.clone(),
            provider,
            NonZeroUsize::new(2).expect("nonzero limit"),
        );
        let work = scheduler.spawn(fixture.request, None, Cancellation::new());
        cx.spawn_in(window, async move |view, cx| {
            let preview = match work.await {
                Ok(outcome) => match outcome.result {
                    Ok(summary) => Preview::Ready(summary.title().to_owned()),
                    Err(error) => Preview::Failed(error.to_string()),
                },
                Err(error) => Preview::Failed(error.to_string()),
            };
            let _ = view.update(cx, |this, cx| {
                this.preview = preview;
                cx.notify();
            });
        })
        .detach();
    }

    fn send_test_notification(&mut self, cx: &mut Context<Self>) {
        if let Some(platform) = &self.platform
            && let Err(error) = platform.notify_open_review()
        {
            self.platform_status = Some(error.to_string());
            cx.notify();
        }
    }

    fn focus_pane(&mut self, pane: Pane, window: &mut Window, cx: &mut Context<Self>) {
        self.focus[pane.index()].focus(window, cx);
        self.focused = pane;
        cx.notify();
    }

    fn select_tab(
        &mut self,
        pane: Pane,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_tab[pane.index()] = index;
        self.focus_pane(pane, window, cx);
    }

    fn draft(&self, pane: Pane) -> &Entity<InputState> {
        &self.inputs[pane.index()][self.selected_tab[pane.index()]]
    }

    fn current_pane(&self, window: &Window, cx: &App) -> Pane {
        [Pane::Left, Pane::Right]
            .into_iter()
            .find(|pane| self.focus[pane.index()].contains_focused(window, cx))
            .unwrap_or(self.focused)
    }

    fn set_context(&mut self, mode: ContextMode, cx: &mut Context<Self>) {
        let tab = &mut self.reviews[self.selected_tab[1]];
        tab.context = mode;
        tab.show_compact_context = true;
        cx.notify();
    }

    fn ratio(&self, divider: Divider) -> f32 {
        match divider {
            Divider::Workspace => self.outer_ratio,
            Divider::Review => self.reviews[self.selected_tab[1]].context_ratio,
        }
    }

    fn resize(&mut self, divider: Divider, ratio: f32, cx: &mut Context<Self>) {
        match divider {
            Divider::Workspace => self.outer_ratio = ratio.clamp(0.29, 0.55),
            Divider::Review => {
                self.reviews[self.selected_tab[1]].context_ratio = ratio.clamp(0.25, 0.48)
            }
        }
        cx.notify();
    }

    fn nudge(&mut self, divider: Divider, delta: f32, cx: &mut Context<Self>) {
        self.resize(divider, self.ratio(divider) + delta, cx);
    }

    fn start_drag(&mut self, divider: Divider, origin: Pixels, window: &Window) {
        let workspace_width = window.viewport_size().width.as_f32() - 2. * WORKSPACE_INSET - GUTTER;
        let width = match divider {
            Divider::Workspace => workspace_width,
            Divider::Review => workspace_width * (1. - self.outer_ratio) - GUTTER - 2.,
        };
        self.drag = Some(Drag {
            divider,
            origin,
            start_ratio: self.ratio(divider),
            width: width.max(1.),
        });
    }

    fn drag_to(&mut self, x: Pixels, cx: &mut Context<Self>) {
        if let Some(drag) = &self.drag {
            self.resize(
                drag.divider,
                drag.start_ratio + (x - drag.origin).as_f32() / drag.width,
                cx,
            );
        }
    }

    fn divider(
        &self,
        divider: Divider,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (index, id, label, min, max) = match divider {
            Divider::Workspace => (0, "outer_divider", "Resize workspace panes", 29., 55.),
            Divider::Review => (1, "context_divider", "Resize review context", 25., 48.),
        };
        div()
            .id(id)
            .relative()
            .accessibility_id(id)
            .test_support()
            .track_focus(&self.divider_focus[index])
            .tab_stop(true)
            .role(Role::Splitter)
            .aria_label(label)
            .aria_orientation(gpui_kit::Orientation::Vertical)
            .aria_numeric_value(f64::from(self.ratio(divider) * 100.))
            .aria_min_numeric_value(min)
            .aria_max_numeric_value(max)
            .w(px(GUTTER))
            .h_full()
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .cursor_col_resize()
            .hover(|style| style.bg(palette.accent_bg))
            .focus(|style| style.bg(palette.accent_bg))
            .child(div().w(px(2.)).h(px(28.)).rounded_full().bg(palette.border))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &gpui_kit::MouseDownEvent, window, _| {
                    this.start_drag(divider, event.position.x, window);
                }),
            )
            .on_key_down(
                cx.listener(move |this, event: &gpui_kit::KeyDownEvent, _, cx| {
                    match event.keystroke.key.as_str() {
                        "left" => this.nudge(divider, -0.03, cx),
                        "right" => this.nudge(divider, 0.03, cx),
                        _ => return,
                    }
                    cx.stop_propagation();
                }),
            )
            .children(self.observed_bounds(id))
    }

    fn observed_bounds(&self, name: &'static str) -> Option<AnyElement> {
        #[cfg(feature = "e2e")]
        return self
            .observer
            .as_ref()
            .map(|observer| observer.observed_bounds(name).into_any_element());
        #[cfg(not(feature = "e2e"))]
        {
            let _ = name;
            None
        }
    }

    fn toolbar(&self, palette: Palette, ordinary: bool, cx: &mut Context<Self>) -> AnyElement {
        if ordinary {
            let loaded = self
                .accounts
                .as_ref()
                .is_some_and(|panel| panel.read(cx).loaded());
            let show_navigation = loaded && self.route != WorkspaceRoute::Onboarding;
            let on_home = self.route == WorkspaceRoute::Home;
            let mut navigation = div().flex().items_center().gap_2().child(
                div()
                    .text_lg()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child("Threadline"),
            );
            if show_navigation {
                for (id, label, destination) in [
                    ("account-close", "Home", WorkspaceRoute::Home),
                    ("account-open", "Settings", WorkspaceRoute::Settings),
                ] {
                    let tab = button(id, label)
                        .accessibility_id(id)
                        .selected(self.route == destination)
                        .toggled(self.route == destination)
                        .when(self.route == destination, |tab| {
                            tab.bg(palette.accent_bg).text_color(palette.accent)
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if this.route != destination {
                                this.toggle_accounts(window, cx);
                            }
                        }));
                    navigation = navigation.child(
                        div()
                            .relative()
                            .child(tab)
                            .children(self.observed_bounds(id)),
                    );
                }
            }
            let mut actions = div().flex().items_center().gap_3();
            if show_navigation && on_home {
                let busy = self
                    .accounts
                    .as_ref()
                    .is_some_and(|panel| panel.read(cx).home_refresh_busy());
                let refresh = button("home-refresh-all", "Refresh all")
                    .accessibility_id("home-refresh-all")
                    .disabled(busy)
                    .on_click(cx.listener(|workspace, _, window, cx| {
                        if let Some(panel) = &workspace.accounts {
                            panel.update(cx, |panel, cx| panel.refresh_all(Some(window), cx));
                        }
                    }));
                actions = actions.child(
                    div()
                        .relative()
                        .child(refresh)
                        .children(self.observed_bounds("home-refresh-all")),
                );
            }
            actions = actions.child(
                button("quit", "Quit")
                    .accessibility_id("quit")
                    .on_click(cx.listener(|this, _, window, cx| this.request_quit(window, cx))),
            );
            return div()
                .flex()
                .items_center()
                .justify_between()
                .px_4()
                .h(px(54.))
                .flex_shrink_0()
                .border_b_1()
                .border_color(palette.border)
                .bg(palette.panel)
                .child(navigation)
                .child(actions)
                .into_any_element();
        }
        let capabilities = self.platform.as_ref().map(PlatformControls::capabilities);
        let can_notify = capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.notifications);
        let can_tray = capabilities
            .as_ref()
            .is_some_and(|capabilities| capabilities.tray);
        div()
            .h(px(58.))
            .px_4()
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .border_b_1()
            .border_color(palette.border)
            .bg(palette.panel)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .flex_shrink_0()
                    .child(
                        div()
                            .size(px(26.))
                            .rounded_md()
                            .bg(palette.accent_bg)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().size(px(10.)).rounded_full().bg(palette.accent)),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("Threadline"),
                    )
                    .child(div().h(px(21.)).w(px(1.)).bg(palette.border))
                    .child(
                        div()
                            .text_sm()
                            .text_color(palette.muted)
                            .child("station-labs / relay"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .relative()
                            .child(
                                if self.route != WorkspaceRoute::Home {
                                    button("account-close", "Close accounts")
                                } else {
                                    button("account-open", "Accounts")
                                }
                                .accessibility_id(if self.route != WorkspaceRoute::Home {
                                    "account-close"
                                } else {
                                    "account-open"
                                })
                                .disabled(self.accounts.is_none())
                                .on_click(cx.listener(
                                    |this, _, window, cx| this.toggle_accounts(window, cx),
                                )),
                            )
                            .children(self.observed_bounds(
                                if self.route != WorkspaceRoute::Home {
                                    "account-close"
                                } else {
                                    "account-open"
                                },
                            )),
                    )
                    .child(
                        button("load-preview", "Load preview")
                            .accessibility_id("load-preview")
                            .disabled(matches!(self.preview, Preview::Loading))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.request_preview(window, cx)),
                            ),
                    )
                    .child(
                        div()
                            .relative()
                            .child(
                                button("test-notification", "Test notification")
                                    .disabled(!can_notify)
                                    .accessibility_id("test-notification")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.send_test_notification(cx)
                                    })),
                            )
                            .children(self.observed_bounds("test-notification")),
                    )
                    .child(
                        div()
                            .relative()
                            .child(
                                button("tray-toggle", "Keep in tray")
                                    .disabled(!can_tray)
                                    .accessibility_id("tray-toggle")
                                    .toggled(self.tray_enabled)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(platform) = &this.platform
                                            && let Err(error) =
                                                platform.set_tray_enabled(!this.tray_enabled)
                                        {
                                            this.platform_status = Some(error.to_string());
                                            cx.notify();
                                        }
                                    })),
                            )
                            .children(self.observed_bounds("tray-toggle")),
                    )
                    .child(button("quit", "Quit").accessibility_id("quit").on_click(
                        cx.listener(|this, _, window, cx| this.request_quit(window, cx)),
                    )),
            )
            .into_any_element()
    }

    fn status_bar(&self, palette: Palette) -> impl IntoElement {
        let status = self
            .platform_status
            .as_deref()
            .unwrap_or_else(|| self.preview.label());
        div()
            .h(px(31.))
            .px_4()
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(palette.border)
            .bg(palette.panel)
            .text_xs()
            .text_color(palette.muted)
            .child(
                div()
                    .id("workspace-status")
                    .role(Role::Status)
                    .aria_label(status.to_owned())
                    .test_support()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(7.)).rounded_full().bg(palette.good))
                    .child(status.to_owned()),
            )
            .child(if self.focused == Pane::Left {
                "Discussion pane focused"
            } else {
                "Review pane focused"
            })
    }

    fn pane(
        &self,
        pane: Pane,
        compact: bool,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (id, labels, input_id, input_label) = match pane {
            Pane::Left => (
                "left-pane",
                [("issue-tab", "Issue #41"), ("history-tab", "History")],
                "left_input",
                "Discussion draft",
            ),
            Pane::Right => (
                "right-pane",
                [
                    ("first-pr-tab", "Review #72"),
                    ("second-pr-tab", "Review #68"),
                ],
                "right_input",
                "Review draft",
            ),
        };
        let index = pane.index();
        let current = self.selected_tab[index];
        div()
            .id(id)
            .accessibility_id(id)
            .role(Role::Group)
            .aria_label(if pane == Pane::Left {
                "Discussion pane"
            } else {
                "Review pane"
            })
            .test_support()
            .track_focus(&self.focus[index])
            .tab_stop(true)
            .h_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .rounded_md()
            .border_1()
            .border_color(if self.focused == pane {
                palette.accent
            } else {
                palette.border
            })
            .bg(palette.panel)
            .map(|this| match pane {
                Pane::Left => this.flex_none().w(relative(self.outer_ratio)),
                Pane::Right => this.flex_1(),
            })
            .child(
                div()
                    .id(("pane-tabs", index))
                    .role(Role::TabList)
                    .flex_none()
                    .h(px(48.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_3()
                    .border_b_1()
                    .border_color(palette.border)
                    .bg(palette.surface)
                    .children(
                        labels
                            .into_iter()
                            .enumerate()
                            .map(|(position, (id, label))| {
                                button(id, label)
                                    .accessibility_id(id)
                                    .selected(current == position)
                                    .toggled(current == position)
                                    .when(current == position, |button| {
                                        button.bg(palette.accent_bg).text_color(palette.accent)
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.select_tab(pane, position, window, cx)
                                    }))
                            }),
                    ),
            )
            .child(match pane {
                Pane::Left if current == 0 => self.discussion().into_any_element(),
                Pane::Left => history(palette).into_any_element(),
                Pane::Right => self.review(compact, palette, cx).into_any_element(),
            })
            .child(
                div()
                    .relative()
                    .flex_none()
                    .px_3()
                    .py_2()
                    .border_t_1()
                    .border_color(palette.border)
                    .child(
                        Input::new(self.draft(pane))
                            .id(input_id)
                            .accessibility_id(input_id)
                            .aria_label(input_label)
                            .w_full(),
                    )
                    .children(self.observed_bounds(input_id)),
            )
            .into_any_element()
    }

    /// Bounds of the rendered issue lead when visible in the active pane.
    pub fn discussion_opening_bounds(&self, cx: &App) -> Option<Bounds<Pixels>> {
        (self.selected_tab[0] == 0)
            .then(|| self.discussion.read(cx).opening_bounds(cx))
            .flatten()
    }

    fn discussion(&self) -> impl IntoElement {
        div()
            .id("issue-discussion")
            .test_support()
            .relative()
            .flex_1()
            .min_h_0()
            .child(self.discussion.clone().cached(pane_size()))
    }

    fn review(&self, compact: bool, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let index = self.selected_tab[1];
        let review = &self.reviews[index];
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(palette.border)
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette.good)
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(if index == 0 {
                                "Open pull request · #72"
                            } else {
                                "Open pull request · #68"
                            }),
                    )
                    .child(
                        div()
                            .mt_1()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child(if index == 0 { PRIMARY_PR } else { SECONDARY_PR }),
                    )
                    .when(compact, |this| {
                        let label = if review.show_compact_context {
                            "Show code"
                        } else {
                            "Show context"
                        };
                        this.child(button("compact-surface", label).on_click(cx.listener(
                            |state, _, _, cx| {
                                let review = &mut state.reviews[state.selected_tab[1]];
                                review.show_compact_context = !review.show_compact_context;
                                cx.notify();
                            },
                        )))
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .when(!compact || review.show_compact_context, |this| {
                        this.child(self.context_panel(compact, palette, cx))
                    })
                    .when(!compact, |this| {
                        this.child(self.divider(Divider::Review, palette, cx))
                    })
                    .when(!compact || !review.show_compact_context, |this| {
                        this.child(self.diff_panel(palette, cx))
                    }),
            )
    }

    fn context_panel(
        &self,
        compact: bool,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let index = self.selected_tab[1];
        let review = &self.reviews[index];
        let (content, scroll, heading) = match review.context {
            ContextMode::Body => (
                &review.body,
                &review.body_scroll,
                "Pull request description",
            ),
            ContextMode::Issue => (
                &review.issue,
                &review.issue_scroll,
                if index == 0 {
                    "station-labs / relay #41"
                } else {
                    "station-labs / relay #42"
                },
            ),
        };
        div()
            .id("review-context")
            .test_support()
            .h_full()
            .min_h_0()
            .min_w_0()
            .flex_none()
            .w(relative(if compact { 1. } else { review.context_ratio }))
            .flex()
            .flex_col()
            .bg(palette.surface)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .flex_wrap()
                    .gap_1()
                    .px_2()
                    .py_2()
                    .border_b_1()
                    .border_color(palette.border)
                    .children(
                        [
                            ("context-pr", "PR body", ContextMode::Body),
                            ("context-issue", "Linked issue", ContextMode::Issue),
                        ]
                        .into_iter()
                        .map(|(id, label, mode)| {
                            button(id, label)
                                .accessibility_id(id)
                                .toggled(review.context == mode)
                                .selected(review.context == mode)
                                .when(review.context == mode, |button| {
                                    button.bg(palette.accent_bg).text_color(palette.accent)
                                })
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.set_context(mode, cx)),
                                )
                        }),
                    ),
            )
            .child(
                div()
                    .id((
                        "context-body",
                        index * 2 + usize::from(review.context == ContextMode::Issue),
                    ))
                    .accessibility_id("active-review-context")
                    .role(Role::Group)
                    .aria_label(heading)
                    .test_support()
                    .track_scroll(scroll)
                    .overflow_y_scroll()
                    .min_h_0()
                    .flex_1()
                    .p_3()
                    .text_sm()
                    .child(
                        div()
                            .mb_3()
                            .text_xs()
                            .text_color(palette.muted)
                            .child(heading),
                    )
                    .child(markdown(content, palette)),
            )
    }

    fn diff_panel(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let index = self.selected_tab[1];
        let review = &self.reviews[index];
        let diff = review.diff.clone();
        let split = diff.read(cx).mode() == DiffMode::Split;
        let display = div()
            .id("diff-panel")
            .accessibility_id("review-diff")
            .role(Role::Group)
            .aria_label("Changed files and review diff")
            .test_support()
            .min_w_0()
            .min_h_0()
            .h_full()
            .flex_1()
            .flex()
            .flex_col()
            .child(
                div()
                    .px_3()
                    .py_2()
                    .flex()
                    .flex_none()
                    .justify_between()
                    .items_center()
                    .border_b_1()
                    .border_color(palette.border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("Changed files"),
                    )
                    .child(
                        button(
                            "diff-mode",
                            if split { "Unified view" } else { "Split view" },
                        )
                        .on_click(cx.listener(move |_, _, _, cx| {
                            diff.update(cx, |state, cx| {
                                state.set_mode(
                                    if split {
                                        DiffMode::Unified
                                    } else {
                                        DiffMode::Split
                                    },
                                    cx,
                                )
                            });
                            cx.notify();
                        })),
                    ),
            );
        display
            .when(self.diff_loaded, |display| {
                display.child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .child(review.diff_view.clone().cached(pane_size()))
                        .children(self.observed_bounds("diff")),
                )
            })
            .when(!self.diff_loaded, |display| {
                display.child(div().p_4().child("Preparing diff…"))
            })
    }
}

impl gpui_kit::Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::current(cx);
        if self.restarting {
            return div()
                .id("update-handoff")
                .size_full()
                .track_focus(&self.shell_focus)
                .flex()
                .items_center()
                .justify_center()
                .bg(palette.canvas)
                .text_color(palette.ink)
                .role(Role::Status)
                .aria_label("Preparing to restart Threadline")
                .child("Preparing to restart Threadline…");
        }
        let cached_reader = self
            .reader
            .as_ref()
            .filter(|reader| reader.read(cx).has_account())
            .cloned();
        let cached = cached_reader.is_some();
        let compact = window.viewport_size().width.as_f32() < COMPACT_REVIEW_WIDTH;
        #[cfg(feature = "e2e")]
        if let Some(observer) = &self.observer {
            observer.begin_frame();
        }
        let window_content = div()
            .id("threadline")
            .capture_any_mouse_down(cx.listener(|this, _, _, _| this.user_interacted = true))
            .capture_key_down(cx.listener(|this, _, _, _| this.user_interacted = true))
            .on_scroll(cx.listener(|this, _, _, _| this.user_interacted = true))
            .track_focus(&self.shell_focus)
            .tab_stop(!self.feasibility_fixture)
            .key_context(if self.feasibility_fixture {
                "Threadline ThreadlineFixture"
            } else {
                "Threadline"
            })
            .size_full()
            .min_w_0()
            .min_h_0()
            .relative()
            .flex()
            .flex_col()
            .bg(palette.canvas)
            .text_color(palette.ink)
            .on_action(cx.listener(|this, _: &FocusLeft, window, cx| {
                this.focus_pane(Pane::Left, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusRight, window, cx| {
                this.focus_pane(Pane::Right, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FocusDraft, window, cx| {
                this.draft(this.current_pane(window, cx))
                    .focus_handle(cx)
                    .focus(window, cx);
            }))
            .on_action(cx.listener(|this, _: &CycleTab, window, cx| {
                let pane = this.current_pane(window, cx);
                let index = 1 - this.selected_tab[pane.index()];
                this.select_tab(pane, index, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleContext, window, cx| {
                if this.current_pane(window, cx) == Pane::Right {
                    let context = this.reviews[this.selected_tab[1]].context;
                    this.set_context(
                        if context == ContextMode::Body {
                            ContextMode::Issue
                        } else {
                            ContextMode::Body
                        },
                        cx,
                    );
                }
            }))
            .on_action(
                cx.listener(|this, _: &ToggleAccounts, window, cx| {
                    this.toggle_accounts(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ReloadLocalAccount, _, cx| {
                if let Some(panel) = this.accounts.as_ref() {
                    panel.update(cx, |panel, cx| panel.reload_local_profile(cx));
                }
            }))
            .on_action(cx.listener(|this, _: &CheckAccountAccess, window, cx| {
                if this.reader.as_ref().is_some_and(|reader| {
                    reader
                        .read(cx)
                        .wake_sync(threadline_core::connectivity::ProbeTrigger::CheckNow)
                }) {
                    return;
                }
                if let Some(panel) = this.accounts.as_ref() {
                    panel.update(cx, |panel, cx| {
                        panel.refresh((this.route != WorkspaceRoute::Home).then_some(window), cx)
                    });
                }
            }))
            .on_action(
                cx.listener(|this, _: &ShrinkLeft, _, cx| {
                    this.nudge(Divider::Workspace, -0.03, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &GrowLeft, _, cx| this.nudge(Divider::Workspace, 0.03, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ShrinkContext, _, cx| {
                    this.nudge(Divider::Review, -0.03, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &GrowContext, _, cx| this.nudge(Divider::Review, 0.03, cx)),
            )
            .on_action(
                cx.listener(|this, _: &LoadPreview, window, cx| this.request_preview(window, cx)),
            )
            .on_action(cx.listener(|this, _: &Quit, window, cx| this.request_quit(window, cx)))
            .on_mouse_move(
                cx.listener(|this, event: &gpui_kit::MouseMoveEvent, _, cx| {
                    if event.pressed_button == Some(MouseButton::Left) {
                        this.drag_to(event.position.x, cx);
                    } else {
                        this.drag = None;
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.drag = None),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.drag = None),
            );
        // Register before any descendant scroller can claim the capture phase.
        #[cfg(all(windows, feature = "e2e"))]
        let window_content = window_content.on_action(
            cx.listener(|this, _: &SendTestNotification, _, cx| this.send_test_notification(cx)),
        );
        #[cfg(feature = "e2e")]
        let window_content = window_content
            .when_some(self.observer.as_ref(), |content, observer| {
                content.child(observer.scroll_input())
            });
        let loading = self
            .accounts
            .as_ref()
            .is_some_and(|panel| !panel.read(cx).loaded());
        let window_content = window_content
            .child(self.toolbar(palette, !self.feasibility_fixture, cx))
            .when(self.route == WorkspaceRoute::Home, |content| {
                content.children(self.updates.clone())
            })
            .when(
                self.route == WorkspaceRoute::Home && cached && !self.feasibility_fixture,
                |content| {
                    let Some(panel) = &self.accounts else {
                        return content;
                    };
                    let panel = panel.read(cx);
                    let account = panel.local_reader_state().0;
                    let selected = account
                        .visible_repositories()
                        .filter(|repo| repo.selected)
                        .count();
                    let total = account.visible_repositories().count();
                    let mut summary = match (selected, total) {
                        (0, 0) => "No repositories discovered yet. Refresh all to check GitHub."
                            .to_owned(),
                        (0, _) => {
                            format!("No repositories monitored. Choose from {total} in Settings.")
                        }
                        _ => format!("Monitoring {selected} of {total} repositories"),
                    };
                    if let Some(checked) = account
                        .profiles
                        .iter()
                        .filter(|profile| Some(profile.account) == account.active)
                        .filter_map(|profile| profile.checked_at_ms)
                        .max()
                    {
                        let time = crate::display_time::DisplayClock::current(cx)
                            .label(Some(checked), "unknown");
                        summary.push_str(&format!(" · Last GitHub check {time}"));
                    }
                    let mut strip = div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_x_3()
                        .px_4()
                        .py_1()
                        .child(
                            div()
                                .id("home-repository-summary")
                                .test_support()
                                .accessibility_id("home-repository-summary")
                                .role(Role::Label)
                                .aria_value(summary.clone())
                                .child(summary),
                        );
                    if let Some(message) = panel.home_refresh_status() {
                        let status = message.to_owned();
                        strip = strip.child(
                            div()
                                .id("home-refresh-status")
                                .test_support()
                                .accessibility_id("home-refresh-status")
                                .role(Role::Status)
                                .aria_label(status.clone())
                                .child(status),
                        );
                    }
                    content.child(strip)
                },
            )
            .when(self.route != WorkspaceRoute::Home, |content| {
                content.child(div().flex_1().min_h_0().children(self.accounts.clone()))
            })
            .when(self.route == WorkspaceRoute::Home && cached, |content| {
                content.child(div().flex_1().min_h_0().min_w_0().children(cached_reader))
            })
            .when(
                self.route == WorkspaceRoute::Home && !cached && self.feasibility_fixture,
                |content| {
                    content.child(
                        div()
                            .flex_1()
                            .flex()
                            .min_h_0()
                            .min_w_0()
                            .p(px(WORKSPACE_INSET))
                            .child(self.pane(Pane::Left, compact, palette, cx))
                            .child(self.divider(Divider::Workspace, palette, cx))
                            .child(self.pane(Pane::Right, compact, palette, cx)),
                    )
                },
            )
            .when(
                self.route == WorkspaceRoute::Home && !cached && !self.feasibility_fixture,
                |content| {
                    content.child(
                        div()
                            .id("reader-welcome")
                            .role(Role::Status)
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap_3()
                            .child(div().text_lg().child(if loading {
                                "Opening local profile…"
                            } else {
                                "No cached workspace yet"
                            }))
                            .children((!loading && self.accounts.is_some()).then(|| {
                                button("reader-connect-account", "Open Settings")
                                    .accessibility_id("reader-connect-account")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.set_accounts_open(true, window, cx)
                                    }))
                            })),
                    )
                },
            )
            .when(self.feasibility_fixture, |content| {
                content.child(self.status_bar(palette))
            });
        #[cfg(feature = "e2e")]
        let window_content =
            window_content.when_some(self.observer.clone(), |content, observer| {
                let discussion = (self.feasibility_fixture && self.selected_tab[0] == 0)
                    .then(|| self.discussion.clone());
                content.child(observer.frame_report(self.snapshot(window, cx), discussion))
            });
        window_content
    }
}

#[cfg(feature = "e2e")]
impl Workspace {
    fn snapshot(&self, window: &Window, cx: &App) -> Snapshot {
        let tab = &self.reviews[self.selected_tab[1]];
        Snapshot {
            ready: if self.feasibility_fixture {
                self.diff_loaded
            } else {
                self.accounts
                    .as_ref()
                    .is_some_and(|panel| panel.read(cx).loaded())
            },
            seed: self.seed,
            fixture_revision: crate::fixtures::REVISION,
            focused_pane: if self.focused == Pane::Left {
                "left"
            } else {
                "right"
            },
            left_tab: [41, 0][self.selected_tab[0]],
            right_tab: [72, 68][self.selected_tab[1]],
            outer_ratio: self.outer_ratio,
            context_ratio: tab.context_ratio,
            context_mode: if tab.context == ContextMode::Body {
                "pr"
            } else {
                "issue"
            },
            left_input: self.draft(Pane::Left).read(cx).value().to_string(),
            right_input: self.draft(Pane::Right).read(cx).value().to_string(),
            preview_state: match &self.preview {
                Preview::Idle => "idle",
                Preview::Loading => "loading",
                Preview::Ready(_) => "ready",
                Preview::Failed(_) => "failed",
            },
            tray_enabled: self.tray_enabled,
            notifications_shown: self.notifications_shown,
            notifications_submitted: self.notifications_submitted,
            notification_com_activations: self.notification_com_activations,
            activations: self.activations,
            account_access: self.accounts.as_ref().and_then(|panel| {
                panel
                    .read(cx)
                    .observation(self.route != WorkspaceRoute::Home, window, cx)
            }),
            cached_reader: self
                .accounts
                .as_ref()
                .filter(|panel| panel.read(cx).uses_isolated_fixture())
                .and_then(|panel| {
                    let sync_open = self.route == WorkspaceRoute::Settings
                        && panel.read(cx).synchronization_open();
                    self.reader
                        .as_ref()
                        .map(|reader| reader.read(cx).observation(sync_open, window, cx))
                }),
        }
    }
}

fn button(id: &'static str, label: &'static str) -> Button {
    Button::new(id).label(label).ghost().small()
}

fn history(palette: Palette) -> impl IntoElement {
    const ROW_HEIGHT: f32 = 60.;
    let graph = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let anchor = |row: usize, lane: usize| {
                point(
                    bounds.left() + px(14. + 21. * lane as f32),
                    bounds.top() + px(ROW_HEIGHT * (row as f32 + 0.5)),
                )
            };
            for (row, commit) in HISTORY.iter().enumerate() {
                let start = anchor(row, commit.lane);
                for &parent in commit.parents {
                    let end = anchor(parent, HISTORY[parent].lane);
                    let mut path = PathBuilder::stroke(px(2.));
                    path.move_to(start);
                    let bend = start.y + (end.y - start.y) * 0.5;
                    path.cubic_bezier_to(end, point(start.x, bend), point(end.x, bend));
                    if let Ok(line) = path.build() {
                        window.paint_path(
                            line,
                            if commit.lane == 0 && HISTORY[parent].lane == 0 {
                                palette.good
                            } else {
                                palette.accent
                            },
                        );
                    }
                }
            }
        },
    )
    .absolute()
    .left_0()
    .top_0()
    .h_full()
    .w(px(58.));
    div()
        .id("commit-history")
        .accessibility_id("commit-history")
        .role(Role::Group)
        .aria_label("Branch history")
        .test_support()
        .overflow_y_scroll()
        .flex_1()
        .min_h_0()
        .p_4()
        .child(
            div()
                .text_lg()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .child("Branch history"),
        )
        .child(
            div()
                .relative()
                .mt_3()
                .child(graph)
                .children(HISTORY.iter().map(|commit| {
                    div()
                        .h(px(ROW_HEIGHT))
                        .pl(px(58.))
                        .relative()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .border_b_1()
                        .border_color(palette.border)
                        .child(
                            div()
                                .absolute()
                                .top(px(ROW_HEIGHT / 2. - 6.))
                                .left(px(8. + 21. * commit.lane as f32))
                                .size(px(12.))
                                .rounded_full()
                                .border_2()
                                .border_color(palette.panel)
                                .bg(if commit.lane == 0 {
                                    palette.good
                                } else {
                                    palette.accent
                                }),
                        )
                        .child(div().text_sm().child(commit.title))
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .text_xs()
                                .text_color(palette.muted)
                                .child(commit.sha)
                                .child(commit.author)
                                .when_some(commit.branch, |row, branch| {
                                    row.child(div().text_color(palette.accent).child(branch))
                                }),
                        )
                })),
        )
}

#[cfg(all(test, feature = "e2e"))]
#[path = "workspace_focus_tests.rs"]
mod focus_tests;
