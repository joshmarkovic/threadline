use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use gpui_kit::{
    Context, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render, Role,
    SharedString, StatefulInteractiveElement, Styled, TestSupportExt, Window,
    component::{
        Disableable, Sizable,
        button::{Button, ButtonVariants},
    },
    div,
};
use tokio::{runtime::Handle, task::AbortHandle};

use crate::palette::Palette;

/// How long to wait before looking again after finding nothing newer.
const RECHECK_AFTER_CURRENT: Duration = Duration::from_secs(4 * 60 * 60);
/// How long to wait before repeating a check that failed.
const RECHECK_AFTER_FAILURE: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Copy)]
pub(crate) enum RestartRequested {
    Startup,
    User,
}
impl EventEmitter<RestartRequested> for Updates {}
pub(crate) struct SetupStarted;
impl EventEmitter<SetupStarted> for Updates {}
pub(crate) struct SetupStartFailed;
impl EventEmitter<SetupStartFailed> for Updates {}

trait VerifiedSetup: Send + Sync {
    fn version(&self) -> &str;
    fn launch(&self) -> Result<(), &'static str>;
}

type CheckResult = Result<Option<Box<dyn VerifiedSetup>>, &'static str>;
type CheckFuture<'a> = Pin<Box<dyn Future<Output = CheckResult> + Send + 'a>>;
trait Source: Send + Sync {
    /// The installed version, when this copy can update itself.
    fn installed(&self) -> Option<String>;
    fn check(&self) -> CheckFuture<'_>;
}

struct Releases;
#[cfg(windows)]
impl VerifiedSetup for threadline_installer::Staged {
    fn version(&self) -> &str {
        &self.manifest.version
    }
    fn launch(&self) -> Result<(), &'static str> {
        self.launch_for_restart()
            .map_err(|_| "Setup could not start. Threadline is still open.")
    }
}
impl Source for Releases {
    fn installed(&self) -> Option<String> {
        #[cfg(windows)]
        {
            threadline_installer::github_update::installed_version()
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
    fn check(&self) -> CheckFuture<'_> {
        #[cfg(windows)]
        {
            Box::pin(async move {
                threadline_installer::github_update::check()
                    .await
                    .map(|setup| setup.map(|setup| Box::new(setup) as Box<dyn VerifiedSetup>))
            })
        }
        #[cfg(not(windows))]
        {
            Box::pin(async { Ok(None) })
        }
    }
}

enum State {
    Inspecting,
    Unconfigured,
    Waiting,
    Checking,
    Current,
    Failed(&'static str),
    Ready(Arc<dyn VerifiedSetup>),
}

pub(crate) struct Updates {
    runtime: Handle,
    source: Arc<dyn Source>,
    state: State,
    installed: Option<String>,
    permitted: Option<bool>,
    generation: u64,
    check_task: Option<AbortHandle>,
    open: bool,
    launching: bool,
    note: Option<&'static str>,
}

impl Updates {
    pub(crate) fn new(runtime: Handle, cx: &mut Context<Self>) -> Self {
        Self::with_source(runtime, Arc::new(Releases), cx)
    }

    fn with_source(runtime: Handle, source: Arc<dyn Source>, cx: &mut Context<Self>) -> Self {
        let probe = source.clone();
        let work = runtime.spawn_blocking(move || probe.installed());
        cx.spawn(async move |view, cx| {
            let installed = work.await.unwrap_or_default();
            let _ = view.update(cx, |model, cx| {
                model.state = if installed.is_some() {
                    State::Waiting
                } else {
                    State::Unconfigured
                };
                model.installed = installed;
                model.start(cx);
                cx.notify();
            });
        })
        .detach();
        Self {
            runtime,
            source,
            state: State::Inspecting,
            installed: None,
            permitted: None,
            generation: 0,
            check_task: None,
            open: false,
            launching: false,
            note: None,
        }
    }

    pub(crate) fn show_settings(&mut self, open: bool, cx: &mut Context<Self>) {
        self.open = open;
        cx.notify();
    }

    pub(crate) fn set_policy(&mut self, allowed: Option<bool>, cx: &mut Context<Self>) {
        self.permitted = allowed;
        if allowed != Some(true) && matches!(self.state, State::Checking) {
            self.cancel_check();
            self.state = State::Waiting;
        }
        self.start(cx);
        cx.notify();
    }

    /// Whether a verified update is downloaded and may be installed without asking.
    pub(crate) fn ready_to_install(&self) -> bool {
        matches!(self.state, State::Ready(_)) && !self.launching && self.permitted == Some(true)
    }

    fn cancel_check(&mut self) {
        self.generation += 1;
        if let Some(task) = self.check_task.take() {
            task.abort();
        }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        if self.permitted != Some(true) || !matches!(self.state, State::Waiting) {
            return;
        }
        self.state = State::Checking;
        self.generation += 1;
        let generation = self.generation;
        let source = self.source.clone();
        let work = self.runtime.spawn(async move { source.check().await });
        self.check_task = Some(work.abort_handle());
        cx.spawn(async move |view, cx| {
            let result = work.await.unwrap_or(Err("The update check stopped."));
            let _ = view.update(cx, |model, cx| {
                if model.generation != generation {
                    return;
                }
                model.check_task = None;
                model.state = match result {
                    Ok(Some(setup)) => State::Ready(setup.into()),
                    Ok(None) => State::Current,
                    Err(error) => State::Failed(error),
                };
                if matches!(model.state, State::Ready(_)) {
                    cx.emit(RestartRequested::Startup);
                } else {
                    model.check_again_later(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Looks for a newer release again while the app stays open.
    fn check_again_later(&mut self, cx: &mut Context<Self>) {
        let delay = match self.state {
            State::Current => RECHECK_AFTER_CURRENT,
            State::Failed(_) => RECHECK_AFTER_FAILURE,
            _ => return,
        };
        let generation = self.generation;
        let timer = cx.background_executor().timer(delay);
        cx.spawn(async move |view, cx| {
            timer.await;
            let _ = view.update(cx, |model, cx| {
                if model.generation == generation {
                    model.check_now(cx);
                }
            });
        })
        .detach();
    }

    fn check_now(&mut self, cx: &mut Context<Self>) {
        if matches!(self.state, State::Current | State::Failed(_)) {
            self.state = State::Waiting;
            self.start(cx);
            cx.notify();
        }
    }

    pub(crate) fn launch(&mut self, request: RestartRequested, cx: &mut Context<Self>) {
        if matches!(request, RestartRequested::Startup) && self.permitted != Some(true) {
            self.restart_problem(
                "Automatic installation is paused by your sync settings.",
                cx,
            );
            cx.emit(SetupStartFailed);
            return;
        }
        let State::Ready(setup) = &self.state else {
            cx.emit(SetupStartFailed);
            return;
        };
        if self.launching {
            return;
        }
        self.launching = true;
        self.note = None;
        let setup = Arc::clone(setup);
        let work = self.runtime.spawn_blocking(move || setup.launch());
        cx.spawn(async move |view, cx| {
            let result = work.await.unwrap_or(Err(
                "The installer handoff stopped. Threadline is still open.",
            ));
            let _ = view.update(cx, |model, cx| {
                if let Err(error) = result {
                    model.restart_problem(error, cx);
                    cx.emit(SetupStartFailed);
                } else {
                    cx.emit(SetupStarted);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn restart_problem(&mut self, message: &'static str, cx: &mut Context<Self>) {
        self.note = Some(message);
        self.launching = false;
        cx.notify();
    }
}

impl Drop for Updates {
    fn drop(&mut self) {
        if let Some(task) = self.check_task.take() {
            task.abort();
        }
    }
}

impl Render for Updates {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::current(cx);
        let ready = matches!(self.state, State::Ready(_));
        let paused = matches!(self.state, State::Waiting) && self.permitted == Some(false);
        let status: SharedString = match &self.state {
            State::Inspecting => "Checking this installation…".into(),
            State::Unconfigured => "This installation does not receive automatic updates.".into(),
            State::Waiting if paused => "Update checks are paused by your sync settings.".into(),
            State::Waiting => "Waiting for the workspace settings.".into(),
            State::Checking => "Checking for updates…".into(),
            State::Current => "Threadline is up to date.".into(),
            State::Failed(error) => (*error).into(),
            State::Ready(_) if self.launching => "Starting the verified installer…".into(),
            State::Ready(setup) => {
                format!("Threadline {} is ready to install.", setup.version()).into()
            }
        };
        if !self.open && !ready {
            return div();
        }
        let status_color = match self.state {
            State::Failed(_) => palette.bad,
            State::Waiting if paused => palette.warn,
            _ => palette.ink,
        };
        let status = div()
            .id("update-status")
            .accessibility_id("update-status")
            .role(Role::Status)
            .aria_label(status.clone())
            .text_color(status_color)
            .child(status);
        let restart = ready.then(|| {
            Button::new("updates-restart")
                .label("Restart to update")
                .primary()
                .small()
                .accessibility_id("updates-restart")
                .disabled(self.launching)
                .on_click(cx.listener(|_, _, _, cx| cx.emit(RestartRequested::User)))
        });
        let note = self.note.map(|note| {
            div()
                .id("update-note")
                .accessibility_id("update-note")
                .role(Role::Status)
                .aria_label(note)
                .text_color(palette.bad)
                .child(note)
        });
        if !self.open {
            return div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_3()
                .px_3()
                .py_1p5()
                .border_b_1()
                .border_color(palette.border)
                .bg(palette.accent_bg)
                .text_sm()
                .child(status)
                .child(
                    div()
                        .text_color(palette.muted)
                        .child("It installs when you step away."),
                )
                .children(restart)
                .children(note);
        }
        let version = match &self.installed {
            Some(version) => format!("Version {version}"),
            None => format!(
                "Version {} · not installed by setup",
                env!("CARGO_PKG_VERSION")
            ),
        };
        let check = matches!(self.state, State::Current | State::Failed(_)).then(|| {
            Button::new("updates-check-again")
                .label(if matches!(self.state, State::Failed(_)) {
                    "Check again"
                } else {
                    "Check for updates"
                })
                .small()
                .accessibility_id("updates-check-again")
                .disabled(self.permitted != Some(true))
                .on_click(cx.listener(|model, _, _, cx| model.check_now(cx)))
        });
        crate::ui::card(
            "Updates",
            "Threadline checks for updates when it starts and every few hours. \
             It installs an update when you step away, then reopens.",
            palette,
        )
        .child(
            div()
                .id("update-version")
                .test_support()
                .text_sm()
                .text_color(palette.muted)
                .child(version),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_3()
                .text_sm()
                .child(status)
                .children(restart)
                .children(check),
        )
        .children(note.map(|note| note.text_sm()))
    }
}

#[cfg(test)]
mod tests;
