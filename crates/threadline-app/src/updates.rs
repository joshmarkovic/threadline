use std::{future::Future, pin::Pin, sync::Arc};

use gpui_kit::{
    AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement,
    Render, Role, StatefulInteractiveElement, Styled, TestSupportExt, Window,
    component::{
        Disableable, Sizable,
        button::Button,
        input::{Input, InputState},
    },
    div,
};
use threadline_platform::{Credential, CredentialRef, CredentialStore, NativeCredentialStore};
use tokio::{runtime::Handle, task::AbortHandle};
use zeroize::Zeroize;

#[derive(Clone, Copy)]
pub(crate) enum RestartRequested {
    Startup,
    User,
}
impl EventEmitter<RestartRequested> for Updates {}
pub(crate) struct OpenUpdateSettings;
impl EventEmitter<OpenUpdateSettings> for Updates {}
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
    fn private_channel(&self) -> bool;
    fn check<'a>(&'a self, token: Option<&'a str>) -> CheckFuture<'a>;
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
    fn private_channel(&self) -> bool {
        #[cfg(windows)]
        {
            threadline_installer::github_update::private_channel()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
    fn check<'a>(&'a self, token: Option<&'a str>) -> CheckFuture<'a> {
        #[cfg(windows)]
        {
            Box::pin(async move {
                Ok(threadline_installer::github_update::check(token)
                    .await?
                    .map(|setup| Box::new(setup) as Box<dyn VerifiedSetup>))
            })
        }
        #[cfg(not(windows))]
        {
            let _ = token;
            Box::pin(async { Ok(None) })
        }
    }
}

struct ReleaseSecret(Credential);
impl Drop for ReleaseSecret {
    fn drop(&mut self) {
        self.0.0.zeroize();
    }
}

enum State {
    Inspecting,
    Unconfigured,
    Waiting,
    Checking,
    NeedsCredential,
    Current,
    Failed(&'static str),
    Ready(Arc<dyn VerifiedSetup>),
}

type Vault = Arc<dyn CredentialStore + Send + Sync>;
pub(crate) struct Updates {
    runtime: Handle,
    source: Arc<dyn Source>,
    vault: Vault,
    state: State,
    private_channel: bool,
    permitted: Option<bool>,
    generation: u64,
    check_task: Option<AbortHandle>,
    open: bool,
    editing_credential: bool,
    input: Entity<InputState>,
    saving: bool,
    launching: bool,
    note: Option<&'static str>,
    credential_note: Option<&'static str>,
}

impl Updates {
    pub(crate) fn new(runtime: Handle, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_source(
            runtime,
            Arc::new(Releases),
            Arc::new(NativeCredentialStore),
            window,
            cx,
        )
    }

    fn with_source(
        runtime: Handle,
        source: Arc<dyn Source>,
        vault: Vault,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let probe = source.clone();
        let work = runtime.spawn_blocking(move || {
            let configured = probe.configured();
            (configured, configured && probe.private_channel())
        });
        cx.spawn(async move |view, cx| {
            let (configured, private_channel) = work.await.unwrap_or_default();
            let _ = view.update(cx, |model, cx| {
                model.private_channel = private_channel;
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
            vault,
            state: State::Inspecting,
            private_channel: false,
            permitted: None,
            generation: 0,
            check_task: None,
            open: false,
            editing_credential: false,
            input: Self::new_input(window, cx),
            saving: false,
            launching: false,
            note: None,
            credential_note: None,
        }
    }

    fn new_input(window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("GitHub token for release downloads")
                .masked(true)
        })
    }

    pub(crate) fn show_settings(
        &mut self,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open = open;
        if !self.open {
            self.editing_credential = false;
            self.input = Self::new_input(window, cx);
        }
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
        if self.permitted != Some(true) || self.saving || !matches!(self.state, State::Waiting) {
            return;
        }
        self.state = State::Checking;
        self.generation += 1;
        let generation = self.generation;
        let vault = self.vault.clone();
        let source = self.source.clone();
        let private_channel = self.private_channel;
        let work = self.runtime.spawn(async move {
            let token =
                tokio::task::spawn_blocking(move || vault.read(&CredentialRef::github_releases()))
                    .await
                    .map_err(|_| "Could not read the release credential.")?
                    .map_err(|_| "Could not read the release credential.")?
                    .map(ReleaseSecret);
            let token = token
                .as_ref()
                .map(|s| str::from_utf8(&s.0.0))
                .transpose()
                .map_err(|_| "Replace the saved release token in Settings.")?;
            if private_channel && token.is_none() {
                return Ok(State::NeedsCredential);
            }
            Ok(match source.check(token).await? {
                Some(setup) => State::Ready(setup.into()),
                None => State::Current,
            })
        });
        self.check_task = Some(work.abort_handle());
        cx.spawn(async move |view, cx| {
            let result = work.await.unwrap_or(Err("The update check stopped."));
            let _ = view.update(cx, |model, cx| {
                if model.generation != generation {
                    return;
                }
                model.check_task = None;
                model.state = result.unwrap_or_else(State::Failed);
                if matches!(model.state, State::Ready(_)) {
                    cx.emit(RestartRequested::Startup);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn edit_credential(&mut self, save: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving
            || self.launching
            || matches!(self.state, State::Inspecting | State::Unconfigured)
        {
            return;
        }
        let value = save.then(|| {
            ReleaseSecret(Credential(
                self.input.read(cx).text().to_string().into_bytes(),
            ))
        });
        if value.as_ref().is_some_and(|s| {
            s.0.0.is_empty() || s.0.0.len() > 2048 || !s.0.0.iter().all(u8::is_ascii_graphic)
        }) {
            self.credential_note = Some("Enter a release token without spaces or line breaks.");
            cx.notify();
            return;
        }
        self.input = Self::new_input(window, cx);
        self.saving = true;
        self.cancel_check();
        self.state = State::Waiting;
        let vault = self.vault.clone();
        let work = self.runtime.spawn_blocking(move || {
            let key = CredentialRef::github_releases();
            match value {
                Some(s) => vault.save(&key, &s.0),
                None => vault.remove(&key),
            }
        });
        cx.spawn(async move |view, cx| {
            let ok = matches!(work.await, Ok(Ok(())));
            let _ = view.update(cx, |model, cx| {
                model.saving = false;
                model.editing_credential = !ok;
                model.credential_note = Some(match (ok, save) {
                    (true, true) => "Private release token saved.",
                    (true, false) => "Private release token removed.",
                    (false, _) => "Windows could not change the release credential.",
                });
                if ok {
                    model.start(cx);
                } else {
                    model.state = State::Failed("Update credential settings could not be saved.");
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
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
        let configured = !matches!(self.state, State::Inspecting | State::Unconfigured);
        let status = match &self.state {
            State::Inspecting => "Checking this installation…".into(),
            State::Unconfigured => "This installation does not receive automatic updates.".into(),
            State::Waiting if self.saving => "Saving release credential settings…".into(),
            State::Waiting if self.permitted == Some(false) => {
                "Update checks are paused by your sync settings.".into()
            }
            State::Waiting => "Waiting for the workspace settings.".into(),
            State::Checking => "Checking for updates…".into(),
            State::NeedsCredential => "Connect GitHub to download private app updates.".into(),
            State::Current => "No newer update is available.".into(),
            State::Failed(error) => error.to_string(),
            State::Ready(_) if self.launching => "Starting the verified installer…".into(),
            State::Ready(setup) => format!("Threadline {} is ready to install.", setup.version()),
        };
        let needs_access = matches!(self.state, State::NeedsCredential);
        if !self.open && !ready && !needs_access {
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
        if needs_access && !self.open {
            summary = summary.child(
                Button::new("updates-open-settings")
                    .label("Set up updates")
                    .accessibility_id("updates-open-settings")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(OpenUpdateSettings))),
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
        if self.open && configured && !self.editing_credential && !needs_access {
            panel = panel.child(
                Button::new("updates-manage-token")
                    .label("Manage release access")
                    .accessibility_id("updates-manage-token")
                    .small()
                    .on_click(cx.listener(|model, _, _, cx| {
                        model.editing_credential = true;
                        cx.notify();
                    })),
            );
        }
        if self.open && configured && (self.editing_credential || needs_access) {
            let token = Input::new(&self.input)
                .small()
                .aria_label("GitHub token for release downloads")
                .accessibility_id("updates-release-token");
            let save = Button::new("updates-token-save")
                .label("Save release token")
                .small()
                .accessibility_id("updates-token-save")
                .disabled(self.saving || self.launching)
                .on_click(
                    cx.listener(|model, _, window, cx| model.edit_credential(true, window, cx)),
                );
            let remove = Button::new("updates-token-remove")
                .label("Remove release token")
                .small()
                .accessibility_id("updates-token-remove")
                .disabled(self.saving || self.launching)
                .on_click(
                    cx.listener(|model, _, window, cx| model.edit_credential(false, window, cx)),
                );
            panel = panel
                .child("For private downloads, save a GitHub token here once. Use an existing or dedicated token with Contents read access to joshmarkovic/threadline. Windows Credential Manager stores it separately from your workspace login.")
                .child(div().flex().items_center().gap_2().child(token).child(save).children((!needs_access).then_some(remove)));
        }
        if self.open
            && let Some(note) = self.credential_note
        {
            panel = panel.child(
                div()
                    .id("update-credential-note")
                    .test_support()
                    .accessibility_id("update-credential-note")
                    .role(Role::Status)
                    .aria_label(note)
                    .child(note),
            );
        }
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
