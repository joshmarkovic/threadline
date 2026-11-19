use std::{future::Future, pin::Pin, sync::Arc};

use gpui_kit::{
    Context, EventEmitter, InteractiveElement, IntoElement, ParentElement, Render, Role,
    StatefulInteractiveElement, Styled, Window,
    component::{Disableable, button::Button},
    div,
};
use tokio::{runtime::Handle, task::AbortHandle};

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
    fn configured(&self) -> bool;
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
    fn configured(&self) -> bool {
        #[cfg(windows)]
        {
            threadline_installer::github_update::configured()
        }
        #[cfg(not(windows))]
        {
            false
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
        let work = runtime.spawn_blocking(move || probe.configured());
        cx.spawn(async move |view, cx| {
            let configured = work.await.unwrap_or_default();
            let _ = view.update(cx, |model, cx| {
                model.state = if configured {
                    State::Waiting
                } else {
                    State::Unconfigured
                };
                model.start(cx);
                cx.notify();
            });
        })
        .detach();
        Self {
            runtime,
            source,
            state: State::Inspecting,
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
                }
                cx.notify();
            });
        })
        .detach();
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
        let ready = matches!(self.state, State::Ready(_));
        let status = match &self.state {
            State::Inspecting => "Checking this installation…".into(),
            State::Unconfigured => "This installation does not receive automatic updates.".into(),
            State::Waiting if self.permitted == Some(false) => {
                "Update checks are paused by your sync settings.".into()
            }
            State::Waiting => "Waiting for the workspace settings.".into(),
            State::Checking => "Checking for updates…".into(),
            State::Current => "No newer update is available.".into(),
            State::Failed(error) => error.to_string(),
            State::Ready(_) if self.launching => "Starting the verified installer…".into(),
            State::Ready(setup) => format!("Threadline {} is ready to install.", setup.version()),
        };
        if !self.open && !ready {
            return div();
        }
        let mut panel = div().flex().flex_col().gap_2().p_3();
        if self.open {
            panel = panel.child("App updates");
        }
        let mut summary = div().flex().items_center().gap_2().child(
            div()
                .id("update-status")
                .accessibility_id("update-status")
                .role(Role::Status)
                .aria_label(status.clone())
                .child(status),
        );
        if ready {
            summary = summary.child(
                Button::new("updates-restart")
                    .label("Restart to update")
                    .accessibility_id("updates-restart")
                    .disabled(self.launching)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(RestartRequested::User))),
            );
        }
        if self.open && matches!(self.state, State::Failed(_)) {
            summary = summary.child(
                Button::new("updates-check-again")
                    .label("Check again")
                    .disabled(self.permitted != Some(true))
                    .on_click(cx.listener(|model, _, _, cx| {
                        model.state = State::Waiting;
                        model.start(cx);
                        cx.notify();
                    })),
            );
        }
        panel = panel.child(summary);
        if let Some(note) = self.note {
            panel = panel.child(
                div()
                    .id("update-note")
                    .accessibility_id("update-note")
                    .role(Role::Status)
                    .aria_label(note)
                    .child(note),
            );
        }
        panel
    }
}

#[cfg(test)]
mod tests;
