use gpui_kit::component::button::ButtonVariants;
use threadline_core::github::GitHubUser;
use threadline_platform::github_cli::{CliError, GitHubCli};

use super::*;

#[derive(Default)]
pub(super) enum Phase {
    #[default]
    Idle,
    SigningIn,
    LookingUp,
    Confirm {
        user: GitHubUser,
        repository: Option<String>,
    },
    Verifying,
    Failed(CliError),
}

#[derive(Default)]
pub(super) struct BrowserSignIn {
    pub(super) phase: Phase,
    pub(super) cancel: Cancellation,
}

impl BrowserSignIn {
    pub(super) fn in_progress(&self) -> bool {
        !matches!(self.phase, Phase::Idle | Phase::Failed(_))
    }

    pub(super) fn needs_repository(&self) -> bool {
        matches!(
            self.phase,
            Phase::Confirm {
                repository: None,
                ..
            }
        )
    }
}

#[cfg(all(test, feature = "e2e"))]
pub(crate) type CliAttempt = (
    bool,
    tokio::sync::oneshot::Sender<Result<GitHubUser, CliError>>,
);
#[cfg(all(test, feature = "e2e"))]
pub(crate) struct CliFixture(pub(crate) tokio::sync::mpsc::UnboundedSender<CliAttempt>);
#[cfg(all(test, feature = "e2e"))]
impl gpui_kit::Global for CliFixture {}

impl AccountPanel {
    pub(super) fn reset_browser(&mut self) {
        self.browser.cancel.cancel();
        self.browser.cancel = Cancellation::new();
        self.browser.phase = Phase::Idle;
    }

    fn start_browser(&mut self, interactive: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.view.offline || self.browser.in_progress() {
            return;
        }
        let raw = self.route.read(cx).value();
        let repository = (!raw.trim().is_empty()).then(|| raw.trim().to_owned());
        if let Some(repository) = &repository
            && let Err(error) = self.validated_repository(repository)
        {
            self.show_repository_problem(error, window, cx);
            return;
        }
        self.reset_browser();
        self.clear_token_input(window, cx);
        self.message = None;
        self.github_error = None;
        self.focus.focus(window, cx);
        self.browser.phase = if interactive {
            Phase::SigningIn
        } else {
            Phase::LookingUp
        };
        let cancel = self.browser.cancel.clone();
        let cli_cancel = cancel.clone();
        let isolated = self.service.fixture;
        #[cfg(all(test, feature = "e2e"))]
        let mock = cx
            .try_global::<CliFixture>()
            .map(|fixture| fixture.0.clone());
        let work = self.runtime.spawn(async move {
            if isolated {
                #[cfg(all(test, feature = "e2e"))]
                if let Some(mock) = mock {
                    let (reply, response) = tokio::sync::oneshot::channel();
                    mock.send((interactive, reply))
                        .map_err(|_| CliError::Unavailable)?;
                    return response.await.map_err(|_| CliError::Unavailable)?;
                }
                return Err(CliError::Unavailable);
            }
            let cli = GitHubCli::discover()?;
            if interactive {
                cli.login(&cli_cancel).await?;
            }
            cli.active_user(&cli_cancel).await
        });
        cx.spawn(async move |panel, cx| {
            let result = work.await.unwrap_or(Err(CliError::Unavailable));
            if cancel.is_cancelled() {
                return;
            }
            let _ = panel.update(cx, |panel, cx| {
                panel.browser.phase = match result {
                    Ok(user) => {
                        panel.focus_sign_in = repository.is_none();
                        Phase::Confirm { user, repository }
                    }
                    Err(error) => Phase::Failed(error),
                };
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn confirm_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.view.offline {
            return;
        }
        let Phase::Confirm { user, repository } = &self.browser.phase else {
            return;
        };
        let field = self.route.read(cx).value();
        let repository = repository.as_deref().unwrap_or_else(|| field.trim());
        if let Err(error) = self.validated_repository(repository) {
            self.show_repository_problem(error, window, cx);
            return;
        }
        let command = AccountCommand::ConnectGitHubCli {
            repository: repository.to_owned(),
            login: user.login.clone(),
            user_id: user.id,
            renew: self.renew.clone(),
        };
        self.browser.phase = Phase::Verifying;
        self.launch(command, Some(window), cx);
    }

    fn cancel_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reset_browser();
        self.message = Some("GitHub sign-in cancelled.");
        self.focus_form(window, cx);
        cx.notify();
    }

    fn sign_in_choices(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let disabled = self.busy || self.view.offline;
        let sign_in = Button::new("github-browser-sign-in")
            .small()
            .primary()
            .accessibility_id("github-browser-sign-in")
            .label("Sign in with GitHub")
            .disabled(disabled)
            .on_click(cx.listener(|panel, _, window, cx| panel.start_browser(true, window, cx)));
        let use_cli = Button::new("github-use-cli")
            .small()
            .label("Use signed-in GitHub CLI account")
            .accessibility_id("github-use-cli")
            .disabled(disabled)
            .on_click(cx.listener(|panel, _, window, cx| panel.start_browser(false, window, cx)));
        let use_pat = Button::new("account-use-pat")
            .small()
            .accessibility_id("account-use-pat")
            .label("Use a personal access token")
            .disabled(self.busy)
            .on_click(cx.listener(|panel, _, window, cx| {
                panel.reset_browser();
                panel.use_pat = true;
                panel.message = None;
                panel.github_error = None;
                panel.focus_form(window, cx);
                cx.notify();
            }));
        let alternatives = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(self.control("account-use-pat", use_pat))
            .when(self.page == AccountPanelPage::Settings, |row| {
                row.child(
                    Button::new("account-close-browser")
                        .small()
                        .label("Cancel")
                        .disabled(self.busy)
                        .on_click(
                            cx.listener(|panel, _, window, cx| panel.clear_token(window, cx)),
                        ),
                )
            });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .child("Use the official GitHub CLI to sign in through your browser."),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(sign_in)
                    .child(use_cli),
            )
            .child(alternatives)
    }

    pub(super) fn browser_controls(
        &self,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let section = div().flex().flex_col().gap_2();
        let actions = div().flex().flex_wrap().gap_2();
        let cancel = Button::new("github-cancel-sign-in")
            .small()
            .label("Cancel sign-in")
            .accessibility_id("github-cancel-sign-in")
            .on_click(cx.listener(|panel, _, window, cx| panel.cancel_browser(window, cx)));
        match &self.browser.phase {
            Phase::Idle => section.child(self.sign_in_choices(cx)),
            Phase::Failed(error) => {
                let help = if cfg!(windows) && *error == CliError::Missing {
                    "Install GitHub CLI in PowerShell with winget install --id GitHub.cli --exact, then reopen Threadline."
                } else {
                    error.message()
                };
                section
                    .child(super::pages::status("github-sign-in-status", help))
                    .child(self.sign_in_choices(cx))
            }
            Phase::Confirm { user, repository } => {
                let field = self.route.read(cx).value();
                let route = repository.as_deref().unwrap_or_else(|| field.trim());
                let question = if self.validated_repository(route).is_ok() {
                    format!("Connect @{} to {}?", user.login, route)
                } else {
                    format!(
                        "Signed in as @{}. Enter a repository to connect.",
                        user.login
                    )
                };
                let confirm = Button::new("github-confirm-account")
                    .small()
                    .primary()
                    .label("Connect account")
                    .accessibility_id("github-confirm-account")
                    .on_click(
                        cx.listener(|panel, _, window, cx| panel.confirm_browser(window, cx)),
                    );
                section.child(super::pages::status("github-sign-in-status", question))
                    .child(div().text_sm().text_color(palette.muted).child(
                        "GitHub CLI permissions can include other repositories. Your organization may require single sign-on or approval for GitHub CLI.",
                    ))
                    .child(actions.child(confirm).child(cancel))
            }
            Phase::Verifying => section.child(super::pages::status(
                "github-sign-in-status",
                "Checking repository access…",
            )),
            phase => {
                let message = match phase {
                    Phase::SigningIn => {
                        "Follow the GitHub CLI prompts to finish signing in through your browser."
                    }
                    _ => "Checking the account signed in to GitHub CLI…",
                };
                section
                    .child(super::pages::status("github-sign-in-status", message))
                    .child(actions.child(cancel))
            }
        }
    }
}
