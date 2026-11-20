use std::collections::HashSet;

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Disableable, Sizable};
use gpui_kit::{
    App, AppContext, Context, Entity, EventEmitter, Focusable, InteractiveElement, IntoElement,
    ParentElement, Render, Role, StatefulInteractiveElement, Styled, Subscription, TestSupportExt,
    Window, div, prelude::FluentBuilder, px,
};
use threadline_core::{
    domain::RepositoryName,
    github::{AccessError, Reachability},
    sync::Cancellation,
};
use threadline_storage::{
    AccountId,
    access::{AccessProfile, AccessState, CredentialKind, DiscoveryState, RepositoryView},
};

use crate::{
    accounts::{
        AccessIssueScope, AccountCommand, AccountReply, AccountService, AccountView, Pat,
        REPOSITORY_FORMAT, parse_repository,
    },
    palette::Palette,
};

mod browser;
mod pages;
#[cfg(all(test, feature = "e2e"))]
pub(crate) use browser::CliFixture;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccountPanelPage {
    Onboarding,
    Settings,
}

pub(crate) enum AccountPanelEvent {
    OnboardingFinished,
    OpenSynchronization,
    RefreshFinished { account: AccountId },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OnboardingStep {
    TimeZone,
    GitHub,
    Repository,
}

#[cfg(feature = "e2e")]
#[derive(serde::Serialize)]
pub(crate) struct AccountObservation {
    panel_open: bool,
    busy: bool,
    active_local_id: Option<i64>,
    active_user_id: Option<u64>,
    active_login: Option<String>,
    repository_count: usize,
    selected_count: usize,
    profile_access: Option<String>,
    discovery: Option<String>,
    reachability: String,
    authorization_available: bool,
    fixture_route_focused: bool,
    fixture_token_focused: bool,
    fixture_route_entered: bool,
    fixture_token_entered: bool,
    fixture_replacement_active: bool,
    fixture_identity_rejected: bool,
    input_issue: Option<crate::accounts::AccountInputIssue>,
}

pub(crate) struct AccountPanel {
    service: AccountService,
    runtime: tokio::runtime::Handle,
    focus: gpui_kit::FocusHandle,
    scroll: gpui_kit::ScrollHandle,
    page: AccountPanelPage,
    onboarding: Option<OnboardingStep>,
    focus_sign_in: bool,
    credential_form_open: bool,
    use_pat: bool,
    browser: browser::BrowserSignIn,
    route: Entity<InputState>,
    route_observation: Subscription,
    repository_error: Option<&'static str>,
    display_zone: Entity<InputState>,
    custom_display_zone: bool,
    display_zone_message: Option<&'static str>,
    saving_display_zone: bool,
    token: Entity<InputState>,
    updates: Option<Entity<crate::updates::Updates>>,
    synchronization: Option<Entity<crate::sync_panel::SyncPanel>>,
    renew: Option<AccessProfile>,
    view: AccountView,
    busy: bool,
    loaded: bool,
    message: Option<&'static str>,
    github_error: Option<AccessError>,
    cli_reply: bool,
    issue_scope: Option<AccessIssueScope>,
    home_refresh_status: Option<String>,
    reachability: Reachability,
    cancel: Cancellation,
    #[cfg(feature = "e2e")]
    observer: Option<crate::observation::Observer>,
    #[cfg(feature = "e2e")]
    input_issue: Option<crate::accounts::AccountInputIssue>,
    #[cfg(feature = "e2e")]
    token_observation: Option<gpui_kit::Subscription>,
}

impl AccountPanel {
    pub(crate) fn local_reader_state(
        &self,
    ) -> (&AccountView, Option<&AccessProfile>, Reachability) {
        (&self.view, self.current_profile(), self.reachability)
    }

    pub(crate) fn new(
        service: AccountService,
        runtime: tokio::runtime::Handle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let route = cx.new(|cx| InputState::new(window, cx).placeholder("owner/name"));
        let clock = cx.try_global::<crate::display_time::DisplayClock>();
        let value = clock.map_or("system", |clock| clock.setting());
        let display_zone_message = clock.and_then(|clock| clock.warning);
        let display_zone = cx.new(|cx| InputState::new(window, cx).default_value(value));
        let token = password_input(window, cx);
        let route_observation = cx.observe(&route, Self::route_changed);
        #[cfg(feature = "e2e")]
        let token_observation = service
            .fixture
            .then(|| cx.observe(&token, |_, _, cx| cx.notify()));
        let mut panel = Self {
            service,
            runtime,
            focus: cx.focus_handle(),
            scroll: gpui_kit::ScrollHandle::new(),
            page: AccountPanelPage::Settings,
            onboarding: None,
            focus_sign_in: false,
            credential_form_open: false,
            use_pat: false,
            browser: browser::BrowserSignIn::default(),
            route,
            route_observation,
            repository_error: None,
            display_zone,
            custom_display_zone: !matches!(value, "system" | "America/New_York" | "UTC"),
            display_zone_message,
            saving_display_zone: false,
            token,
            updates: None,
            synchronization: None,
            renew: None,
            view: AccountView::default(),
            busy: false,
            loaded: false,
            message: None,
            github_error: None,
            cli_reply: false,
            issue_scope: None,
            home_refresh_status: None,
            reachability: Reachability::NotAttempted,
            cancel: Cancellation::new(),
            #[cfg(feature = "e2e")]
            observer: None,
            #[cfg(feature = "e2e")]
            input_issue: None,
            #[cfg(feature = "e2e")]
            token_observation,
        };
        panel.launch(panel.service.initial_command(), None, cx);
        panel
    }

    pub(crate) fn focus_form(&self, window: &mut Window, cx: &mut App) {
        self.route.focus_handle(cx).focus(window, cx);
    }

    pub(crate) fn needs_onboarding(&self) -> bool {
        self.onboarding.is_some()
    }

    pub(crate) fn set_page(
        &mut self,
        page: AccountPanelPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.page != page {
            self.clear_token(window, cx);
        }
        self.page = page;
        self.synchronization = None;
        match (page, self.onboarding) {
            (AccountPanelPage::Onboarding, Some(OnboardingStep::GitHub)) => {
                self.focus_form(window, cx);
            }
            _ => self.focus.focus(window, cx),
        }
        cx.notify();
    }

    pub(crate) fn loaded(&self) -> bool {
        self.loaded
    }

    pub(crate) fn open_synchronization(
        &mut self,
        panel: Entity<crate::sync_panel::SyncPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_token(window, cx);
        self.synchronization = Some(panel);
        self.focus.focus(window, cx);
        cx.notify();
    }

    #[cfg(feature = "e2e")]
    pub(crate) fn synchronization_open(&self) -> bool {
        self.synchronization.is_some()
    }

    pub(crate) fn set_updates(
        &mut self,
        updates: Entity<crate::updates::Updates>,
        cx: &mut Context<Self>,
    ) {
        self.updates = Some(updates);
        cx.notify();
    }

    fn save_display_zone(&mut self, advance: bool, cx: &mut Context<Self>) {
        let Some(clock) = cx
            .try_global::<crate::display_time::DisplayClock>()
            .cloned()
        else {
            return;
        };
        if self.saving_display_zone {
            return;
        }
        self.saving_display_zone = true;
        let value = self.display_zone.read(cx).value().to_string();
        let write = self.runtime.spawn_blocking(move || clock.save(&value));
        cx.spawn(async move |panel, cx| {
            let result = write.await;
            let _ = panel.update(cx, |panel, cx| {
                panel.saving_display_zone = false;
                panel.display_zone_message = Some(match result {
                    Ok(Ok(clock)) => {
                        cx.set_global(clock);
                        if advance && panel.onboarding == Some(OnboardingStep::TimeZone) {
                            panel.onboarding = Some(OnboardingStep::GitHub);
                            panel.focus_sign_in = true;
                        }
                        cx.refresh_windows();
                        "Display time zone saved."
                    }
                    Ok(Err(error)) => error,
                    Err(_) => "The display time zone could not be saved.",
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn reload_local_profile(&mut self, cx: &mut Context<Self>) {
        self.launch(AccountCommand::Load, None, cx);
    }

    pub(crate) fn home_refresh_status(&self) -> Option<&str> {
        self.home_refresh_status.as_deref()
    }

    pub(crate) fn home_refresh_busy(&self) -> bool {
        self.busy
    }

    pub(crate) fn refresh_all(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if self.view.offline {
            self.home_refresh_status = Some("Working offline. GitHub was not checked.".into());
        } else if let Some(account) = self.view.active {
            self.home_refresh_status = Some("Checking GitHub repositories…".into());
            self.launch(AccountCommand::RefreshAll(account), window, cx);
        } else {
            self.home_refresh_status = Some("Sign in to refresh GitHub repositories.".into());
        }
        cx.notify();
    }

    pub(crate) fn clear_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reset_browser();
        self.use_pat = false;
        self.credential_form_open = false;
        self.renew = None;
        self.clear_token_input(window, cx);
    }

    fn clear_token_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.repository_error = None;
        self.token = password_input(window, cx);
        #[cfg(feature = "e2e")]
        if self.service.fixture {
            self.token_observation = Some(cx.observe(&self.token, |_, _, cx| cx.notify()));
        }
        cx.notify();
    }

    #[cfg(feature = "e2e")]
    pub(crate) fn observe_bounds(&mut self, observer: Option<crate::observation::Observer>) {
        self.observer = observer;
    }

    fn control(&self, name: &'static str, element: impl IntoElement) -> gpui_kit::Div {
        let control = div().relative().child(element);
        #[cfg(feature = "e2e")]
        let control = control.children(
            self.observer
                .as_ref()
                .map(|observer| observer.observed_bounds(name)),
        );
        #[cfg(not(feature = "e2e"))]
        let _ = name;
        control
    }

    #[cfg(feature = "e2e")]
    pub(crate) fn observation(
        &self,
        panel_open: bool,
        window: &Window,
        cx: &App,
    ) -> Option<AccountObservation> {
        if !self.service.fixture {
            return None;
        }
        let profile = self.current_profile();
        Some(AccountObservation {
            panel_open,
            busy: self.busy,
            active_local_id: self.view.active.map(|account| account.0),
            active_user_id: profile.map(|profile| profile.user_id.get()),
            active_login: profile.map(|profile| profile.login.clone()),
            repository_count: self.view.repositories.len(),
            selected_count: self
                .view
                .repositories
                .iter()
                .filter(|repo| repo.selected)
                .count(),
            profile_access: profile.map(|profile| format!("{:?}", profile.access)),
            discovery: profile.map(|profile| format!("{:?}", profile.discovery)),
            reachability: format!("{:?}", self.reachability),
            authorization_available: self
                .authorization_error()
                .is_some_and(|error| error.authorization_url().is_some()),
            fixture_route_focused: panel_open && self.route.focus_handle(cx).is_focused(window),
            fixture_token_focused: panel_open && self.token.focus_handle(cx).is_focused(window),
            fixture_route_entered: panel_open
                && self.route.read(cx).value().as_ref() == "fixture-org/private",
            fixture_token_entered: panel_open
                && self
                    .token
                    .read(cx)
                    .text()
                    .chars()
                    .eq(crate::accounts::FIXTURE_PAT.chars()),
            fixture_replacement_active: panel_open && self.renew.is_some(),
            fixture_identity_rejected: self.github_error.as_ref().is_some_and(|error| {
                error.kind == threadline_core::github::AccessKind::IdentityMismatch
            }),
            input_issue: if self.repository_error.is_some() {
                Some(crate::accounts::AccountInputIssue::InvalidRepository)
            } else {
                self.input_issue
            },
        })
    }

    #[cfg(feature = "e2e")]
    pub(crate) fn uses_isolated_fixture(&self) -> bool {
        self.service.fixture
    }

    fn launch(
        &mut self,
        command: AccountCommand,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        let renewing = match &command {
            AccountCommand::Connect { renew, .. }
            | AccountCommand::ConnectGitHubCli { renew, .. } => Some(renew.is_some()),
            _ => None,
        };
        let connecting = renewing.is_some();
        let replacing = renewing == Some(true);
        let browser_connect = matches!(&command, AccountCommand::ConnectGitHubCli { .. });
        self.cli_reply = match &command {
            AccountCommand::ConnectGitHubCli { .. } => true,
            AccountCommand::Refresh { profile, .. } | AccountCommand::Select { profile, .. } => {
                profile.kind == CredentialKind::GitHubCli
            }
            AccountCommand::RefreshAll(account) => self
                .view
                .profiles
                .iter()
                .any(|p| p.account == *account && p.kind == CredentialKind::GitHubCli),
            _ => false,
        };
        if !browser_connect {
            self.reset_browser();
        }
        let refresh_account = match &command {
            AccountCommand::RefreshAll(account) => Some(*account),
            _ => None,
        };
        if let Some(window) = window {
            self.focus.focus(window, cx);
        }
        self.busy = true;
        self.message = None;
        self.github_error = None;
        self.issue_scope = None;
        #[cfg(feature = "e2e")]
        {
            self.input_issue = None;
        }
        let cancel = self.cancel.clone();
        let service = self.service.clone();
        let work = self
            .runtime
            .spawn(async move { service.execute(command, &cancel).await });
        cx.spawn(async move |panel, cx| {
            let result = work.await;
            let _ = panel.update(cx, |panel, cx| {
                panel.busy = false;
                let initial_load = !panel.loaded;
                panel.loaded = true;
                let cancelled = panel.cancel.is_cancelled();
                panel.cancel = Cancellation::new();
                if browser_connect {
                    let still_on_form = matches!(panel.browser.phase, browser::Phase::Verifying);
                    panel.browser.phase = browser::Phase::Idle;
                    if cancelled || !still_on_form {
                        panel.reload_local_profile(cx);
                        return;
                    }
                }
                match result {
                    Ok(AccountReply {
                        view,
                        message,
                        github_error,
                        reachability,
                        succeeded,
                        issue_scope,
                        retry_at_ms,
                        #[cfg(feature = "e2e")]
                        input_issue,
                    }) => {
                        if panel.view.active != view.active {
                            panel.synchronization = None;
                        }
                        panel.view = view;
                        if initial_load && succeeded && panel.view.active.is_none() {
                            panel.onboarding = Some(OnboardingStep::TimeZone);
                        }
                        let credential_accepted = succeeded
                            || matches!(
                                issue_scope,
                                Some(AccessIssueScope::Repository | AccessIssueScope::Discovery)
                            );
                        if connecting && credential_accepted && panel.view.active.is_some() {
                            if panel.page == AccountPanelPage::Onboarding {
                                panel.onboarding = Some(OnboardingStep::Repository);
                            }
                            panel.credential_form_open = false;
                            panel.use_pat = false;
                        }
                        if replacing
                            && let Some(renew) = panel.renew.take()
                            && !credential_accepted
                        {
                            panel.renew = panel
                                .view
                                .profiles
                                .iter()
                                .find(|profile| {
                                    Some(profile.account) == panel.view.active
                                        && profile.account == renew.account
                                        && profile.user_id == renew.user_id
                                        && profile.owner_scope == renew.owner_scope
                                })
                                .cloned();
                        }
                        panel.message = message;
                        panel.github_error = github_error;
                        panel.issue_scope = issue_scope;
                        #[cfg(feature = "e2e")]
                        {
                            panel.input_issue = input_issue;
                        }
                        if reachability != Reachability::NotAttempted {
                            panel.reachability = reachability;
                        }
                        if let Some(account) = refresh_account {
                            let retry = retry_at_ms.map(|at| {
                                crate::display_time::DisplayClock::current(cx)
                                    .label(Some(at), "after the server cooldown")
                            });
                            panel.finish_home_refresh(succeeded, retry);
                            if !cancelled && panel.view.active == Some(account) {
                                cx.emit(AccountPanelEvent::RefreshFinished { account });
                            }
                        }
                    }
                    Err(_) => {
                        panel.message = Some("The account check did not finish.");
                        if refresh_account.is_some() {
                            panel.finish_home_refresh(false, None);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_home_refresh(&mut self, succeeded: bool, retry: Option<String>) {
        let active_profiles = || {
            self.view
                .profiles
                .iter()
                .filter(|profile| Some(profile.account) == self.view.active)
        };
        let sso =
            active_profiles().any(|profile| profile.discovery == DiscoveryState::RestrictedBySso);
        let incomplete =
            active_profiles().any(|profile| profile.discovery == DiscoveryState::Partial);
        self.home_refresh_status = Some(match (succeeded, retry) {
            (_, Some(at)) => {
                format!("GitHub rate-limited repository refresh. Try again after {at}.")
            }
            (true, None) if sso => {
                "Some repositories are hidden by GitHub SSO. See Settings for details.".into()
            }
            (true, None) if incomplete => {
                "Only part of the GitHub repository list was refreshed.".into()
            }
            (true, None) => "GitHub repositories refreshed.".into(),
            (false, None) => self
                .github_error
                .as_ref()
                .map(|error| format!("GitHub refresh is incomplete. {error}."))
                .or_else(|| self.message.map(str::to_owned))
                .unwrap_or_else(|| "GitHub refresh did not finish.".into()),
        });
    }

    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let repository = self.route.read(cx).value();
        if let Err(error) = self.validated_repository(&repository) {
            self.show_repository_problem(error, window, cx);
            return;
        }
        let pat = Pat::from_input(self.token.read(cx).text().to_string());
        let renew = self.renew.clone();
        self.clear_token_input(window, cx);
        self.launch(
            AccountCommand::Connect {
                repository: repository.to_string(),
                pat,
                renew,
            },
            Some(window),
            cx,
        );
    }

    fn check_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.view.offline {
            return;
        }
        let requested = self.route.read(cx).value();
        let route = match self.validated_repository(&requested) {
            Ok(route) => route,
            Err(error) => {
                self.show_repository_problem(error, window, cx);
                return;
            }
        };
        let Some(profile) = self.profile(route.owner()).cloned() else {
            return;
        };
        self.repository_error = None;
        self.launch(
            AccountCommand::Refresh {
                profile,
                repository: None,
                requested: Some(requested.to_string()),
            },
            Some(window),
            cx,
        );
    }

    fn show_repository_problem(
        &mut self,
        error: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.repository_error = Some(error);
        self.focus_form(window, cx);
        cx.notify();
    }

    fn validated_repository(&self, raw: &str) -> Result<RepositoryName, &'static str> {
        let route = parse_repository(raw).ok_or(REPOSITORY_FORMAT)?;
        if self
            .renew
            .as_ref()
            .is_some_and(|profile| !profile.owner_scope.eq_ignore_ascii_case(route.owner()))
        {
            return Err("Enter a repository owned by the account you are reconnecting.");
        }
        let credential_entry = self.credential_form_open
            || (self.page == AccountPanelPage::Onboarding
                && self.onboarding == Some(OnboardingStep::GitHub));
        if !credential_entry && self.profile(route.owner()).is_none() {
            return Err("Add an account for this repository owner in Settings to check access.");
        }
        Ok(route)
    }

    fn route_changed(&mut self, route: Entity<InputState>, cx: &mut Context<Self>) {
        let prior = self.repository_error;
        if prior.is_some() {
            self.repository_error = self.validated_repository(&route.read(cx).value()).err();
        }
        if self.repository_error != prior || self.browser.needs_repository() || self.service.fixture
        {
            cx.notify();
        }
    }

    fn profile(&self, owner: &str) -> Option<&AccessProfile> {
        self.view.profiles.iter().find(|profile| {
            Some(profile.account) == self.view.active
                && profile.owner_scope.eq_ignore_ascii_case(owner)
        })
    }

    fn current_profile(&self) -> Option<&AccessProfile> {
        self.view
            .profiles
            .iter()
            .filter(|profile| Some(profile.account) == self.view.active)
            .min_by_key(|profile| match profile.access {
                AccessState::Readable => 0,
                AccessState::SignedOut => 2,
                _ => 1,
            })
    }

    pub(crate) fn refresh(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if self
            .view
            .profiles
            .iter()
            .filter(|p| Some(p.account) == self.view.active && p.access != AccessState::SignedOut)
            .count()
            > 1
        {
            self.refresh_all(window, cx);
            return;
        }
        let Some(profile) = self.current_profile().cloned() else {
            return;
        };
        let repository = self
            .view
            .repositories
            .iter()
            .filter(|repo| repo.owner.eq_ignore_ascii_case(&profile.owner_scope))
            .max_by_key(|repo| repo.selected)
            .cloned();
        let requested = self.route.read(cx).value().to_string();
        let requested = (!requested.trim().is_empty()).then_some(requested);
        self.launch(
            AccountCommand::Refresh {
                profile,
                repository,
                requested,
            },
            window,
            cx,
        );
    }

    fn repository_field(&self, palette: Palette) -> impl IntoElement {
        let hint = if !self.use_pat
            && (self.credential_form_open || self.onboarding == Some(OnboardingStep::GitHub))
        {
            "Enter owner/repository, for example octocat/hello-world. You can sign in first."
        } else {
            "Use owner/repository, for example octocat/hello-world."
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_sm().child("Repository"))
            .child(div().text_xs().text_color(palette.muted).child(hint))
            .child(
                self.control(
                    "account-repository",
                    Input::new(&self.route)
                        .small()
                        .disabled(
                            self.busy
                                || (self.browser.in_progress() && !self.browser.needs_repository()),
                        )
                        .accessibility_id("account-repository")
                        .aria_label("GitHub repository"),
                ),
            )
            .children(self.repository_error.map(|error| {
                div()
                    .id("account-repository-error")
                    .accessibility_id("account-repository-error")
                    .test_support()
                    .role(Role::Status)
                    .aria_label(error)
                    .text_sm()
                    .text_color(palette.accent)
                    .child(error)
            }))
    }

    fn credential_form(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let form = div()
            .id("account-credential-form")
            .accessibility_id("account-credential-form")
            .test_support()
            .role(Role::Group)
            .aria_label("GitHub sign-in")
            .max_w(px(520.))
            .flex()
            .flex_col()
            .gap_3()
            .child(self.repository_field(palette));
        if !self.use_pat {
            return form.child(self.browser_controls(palette, cx));
        }
        let form = form
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_sm().child("Personal access token"))
                    .child(
                        self.control(
                            "account-token",
                            Input::new(&self.token)
                                .small()
                                .disabled(self.busy)
                                .accessibility_id("account-token")
                                .aria_label("GitHub personal access token"),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div().text_xs().text_color(palette.muted).child(
                            "For organization repositories, sign in with SSO if required, then use a token authorized for that organization.",
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                Button::new("account-org-sso")
                                    .accessibility_id("account-org-sso")
                                    .label("Sign in with organization SSO")
                                    .small()
                                    .disabled(self.busy || self.view.offline)
                                    .on_click(cx.listener(|panel, _, window, cx| {
                                        panel.open_setup_page(window, cx, |owner| {
                                            format!("https://github.com/orgs/{owner}/sso")
                                        });
                                    })),
                            )
                            .child(
                                Button::new("account-create-read-token")
                                    .accessibility_id("account-create-read-token")
                                    .label("Create read-only token")
                                    .small()
                                    .disabled(self.busy || self.view.offline)
                                    .on_click(cx.listener(|panel, _, window, cx| {
                                        panel.open_setup_page(window, cx, |owner| {
                                            format!("https://github.com/settings/personal-access-tokens/new?target_name={owner}&name=Threadline&issues=read&pull_requests=read")
                                        });
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(
                        self.control(
                            "account-connect",
                            Button::new("account-connect")
                                .label("Check access")
                                .small()
                                .accessibility_id("account-connect")
                                .disabled(self.busy || self.view.offline)
                                .on_click(
                                    cx.listener(|panel, _, window, cx| panel.connect(window, cx)),
                                ),
                        ),
                    )
                    .when(self.page == AccountPanelPage::Settings, |row| {
                        row.child(
                            Button::new("account-close-credential")
                                .label("Cancel")
                                .small()
                                .disabled(self.busy)
                                .on_click(cx.listener(|panel, _, window, cx| {
                                    panel.clear_token(window, cx);
                                })),
                        )
                    })
                    .when(self.busy, |row| {
                        row.child(
                            Button::new("account-cancel")
                                .label("Cancel check")
                                .small()
                                .on_click(cx.listener(|panel, _, window, cx| {
                                    panel.focus.focus(window, cx);
                                    panel.cancel.cancel();
                                })),
                        )
                    }),
            );
        let form = form.child(
            div().flex().child(
                Button::new("account-use-browser")
                    .accessibility_id("account-use-browser")
                    .label("Use browser sign-in")
                    .small()
                    .disabled(self.busy)
                    .on_click(cx.listener(|panel, _, window, cx| {
                        panel.clear_token_input(window, cx);
                        panel.use_pat = false;
                        panel.message = None;
                        panel.github_error = None;
                        panel.focus_form(window, cx);
                        cx.notify();
                    })),
            ),
        );
        #[cfg(feature = "e2e")]
        let form = form.when(
            self.service.fixture
                && std::env::var_os("THREADLINE_E2E_ACCOUNT_FORM")
                    .is_some_and(|value| value == "1"),
            |form| {
                form.child(
                    Button::new("account-fill-synthetic-token")
                        .label("Fill synthetic test credential")
                        .accessibility_id("account-fill-synthetic-token")
                        .small()
                        .disabled(
                            self.busy
                                || self.route.read(cx).value().as_ref() != "fixture-org/private",
                        )
                        .on_click(cx.listener(|panel, _, window, cx| {
                            if panel.service.fixture
                                && !panel.busy
                                && panel.route.read(cx).value().as_ref() == "fixture-org/private"
                            {
                                panel.token.update(cx, |token, cx| {
                                    token.set_value(crate::accounts::FIXTURE_PAT, window, cx);
                                });
                            }
                        })),
                )
            },
        );
        form
    }

    fn open_setup_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        url: fn(&str) -> String,
    ) {
        match self.validated_repository(&self.route.read(cx).value()) {
            Ok(route) => cx.open_url(&url(route.owner())),
            Err(error) => self.show_repository_problem(error, window, cx),
        }
    }

    fn start_credential_form(
        &mut self,
        replacement: Option<AccessProfile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_token(window, cx);
        self.renew = replacement;
        self.credential_form_open = true;
        self.focus_sign_in = true;
        cx.notify();
    }

    fn profile_controls(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let mut shown = HashSet::new();
        let rows: Vec<_> = self
            .view
            .profiles
            .iter()
            .filter_map(|profile| {
                if !shown.insert(profile.account) {
                    return None;
                }
                let account = profile.account;
                if Some(account) == self.view.active {
                    return None;
                }
                Some(
                    Button::new(format!("account-switch-{}", account.0))
                        .accessibility_id(format!("account-switch-{}", account.0))
                        .label(profile.login.clone())
                        .small()
                        .disabled(self.busy)
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            panel.route =
                                cx.new(|cx| InputState::new(window, cx).placeholder("owner/name"));
                            panel.route_observation = cx.observe(&panel.route, Self::route_changed);
                            panel.clear_token(window, cx);
                            panel.launch(AccountCommand::Switch(account), Some(window), cx)
                        })),
                )
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children((!rows.is_empty()).then(|| div().flex().gap_2().flex_wrap().children(rows)))
            .when_some(self.view.active, |section, account| {
                section.child(
                    div()
                        .flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            self.control(
                                "account-refresh",
                                Button::new("account-refresh")
                                    .label("Refresh access")
                                    .small()
                                    .accessibility_id("account-refresh")
                                    .disabled(self.busy || self.view.offline)
                                    .on_click(cx.listener(|panel, _, window, cx| {
                                        panel.refresh(Some(window), cx)
                                    })),
                            ),
                        )
                        .child(
                            self.control(
                                "account-offline",
                                Button::new("account-offline")
                                    .label(if self.view.offline {
                                        "Go online"
                                    } else {
                                        "Work offline"
                                    })
                                    .small()
                                    .accessibility_id("account-offline")
                                    .disabled(self.busy)
                                    .on_click(cx.listener(move |panel, _, window, cx| {
                                        panel.launch(
                                            AccountCommand::SetOffline {
                                                account,
                                                offline: !panel.view.offline,
                                            },
                                            Some(window),
                                            cx,
                                        )
                                    })),
                            ),
                        )
                        .when(
                            self.view.profiles.iter().any(|profile| {
                                profile.account == account
                                    && profile.access != AccessState::SignedOut
                            }),
                            |row| {
                                row.child(
                                    self.control(
                                        "account-sign-out",
                                        Button::new("account-sign-out")
                                            .label("Sign out")
                                            .small()
                                            .accessibility_id("account-sign-out")
                                            .disabled(self.busy)
                                            .on_click(cx.listener(move |panel, _, window, cx| {
                                                panel.clear_token(window, cx);
                                                panel.launch(
                                                    AccountCommand::SignOut(account),
                                                    Some(window),
                                                    cx,
                                                )
                                            })),
                                    ),
                                )
                            },
                        ),
                )
            })
            .children(
                self.view
                    .profiles
                    .iter()
                    .filter(|profile| Some(profile.account) == self.view.active)
                    .map(|profile| {
                        let target = profile.clone();
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .pt_2()
                            .border_t_1()
                            .border_color(palette.border)
                            .child(div().text_sm().child(format!(
                                "{} · {}",
                                profile.owner_scope,
                                sign_in_label(profile.access)
                            )))
                            .child(
                                Button::new(format!(
                                    "account-renew-{}-{}",
                                    profile.account.0, profile.owner_scope
                                ))
                                .accessibility_id(format!(
                                    "account-renew-{}-{}",
                                    profile.account.0, profile.owner_scope
                                ))
                                .label("Reconnect")
                                .small()
                                .disabled(self.busy)
                                .on_click(cx.listener(
                                    move |panel, _, window, cx| {
                                        panel.start_credential_form(
                                            Some(target.clone()),
                                            window,
                                            cx,
                                        );
                                    },
                                )),
                            )
                    })
                    .collect::<Vec<_>>(),
            )
    }

    fn repository_row(
        &self,
        index: usize,
        repo: &RepositoryView,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let account = self.view.active;
        let profile = self.profile(&repo.owner).cloned();
        let repo = repo.clone();
        let id = if repo.selected {
            format!("account-remove-{index}")
        } else {
            format!("account-select-{index}")
        };
        let can_refresh = profile.is_some() && !self.view.offline;
        let access = match repo.access {
            AccessState::Readable if !repo.verified_current => {
                "Check access with the current token"
            }
            _ => access_label(repo.access),
        };
        let status = if repo.selected {
            "Monitoring enabled"
        } else {
            "Monitoring disabled"
        };
        let checked = repo
            .last_success_at_ms
            .map(date_label)
            .unwrap_or_else(|| "No successful check recorded".into());
        div()
            .flex()
            .justify_between()
            .gap_3()
            .items_center()
            .py_2()
            .border_t_1()
            .border_color(palette.border)
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .child(format!("{}/{}", repo.owner, repo.name))
                            .when(repo.selected, |name| {
                                name.child(crate::ui::tag("Monitored", palette.good))
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette.muted)
                            .child(format!("{access} · {status}")),
                    )
                    .child(div().text_xs().text_color(palette.muted).child(format!(
                        "{checked} · {} cached items · {} cached text bytes",
                        repo.cached_items, repo.cached_text_bytes
                    ))),
            )
            .child(
                self.control(
                    if index == 0 && repo.selected {
                        "account-remove-0"
                    } else if index == 0 {
                        "account-select-0"
                    } else {
                        "account-repository-action"
                    },
                    Button::new(id.clone())
                        .label(if repo.selected { "Remove" } else { "Select" })
                        .small()
                        .accessibility_id(id)
                        .disabled(self.busy || (!repo.selected && !can_refresh))
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            if repo.selected
                                && let Some(account) = account
                            {
                                panel.launch(
                                    AccountCommand::Remove {
                                        account,
                                        repository: repo.id,
                                    },
                                    Some(window),
                                    cx,
                                );
                            } else if let Some(profile) = profile.clone() {
                                panel.launch(
                                    AccountCommand::Select {
                                        profile,
                                        repository: repo.clone(),
                                    },
                                    Some(window),
                                    cx,
                                );
                            }
                        })),
                ),
            )
    }
}

impl Drop for AccountPanel {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.browser.cancel.cancel();
    }
}

impl EventEmitter<AccountPanelEvent> for AccountPanel {}

impl Render for AccountPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.focus_sign_in) {
            self.focus_form(window, cx);
        }
        let palette = Palette::current(cx);
        let onboarding = match (self.page, self.onboarding) {
            (AccountPanelPage::Onboarding, Some(step)) => Some(step),
            _ => None,
        };
        let sections = match onboarding {
            Some(step) => vec![self.onboarding_page(step, palette, cx)],
            None => self.settings_sections(palette, cx),
        };
        let navigation = onboarding
            .is_none()
            .then(|| self.settings_nav(palette, cx).into_any_element());
        div()
            .size_full()
            .flex()
            .bg(palette.canvas)
            .text_color(palette.ink)
            .children(navigation)
            .child(
                div()
                    .id("account-panel")
                    .track_focus(&self.focus)
                    .tab_stop(true)
                    .role(Role::Group)
                    .aria_label("GitHub settings and setup")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .p_6()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .children(sections),
            )
    }
}

fn password_input(window: &mut Window, cx: &mut Context<AccountPanel>) -> Entity<InputState> {
    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("Personal access token")
            .masked(true)
    })
}

fn date_label(at_ms: i64) -> String {
    let now = threadline_storage::UtcMillis::now().map_or(at_ms, |time| time.0);
    let minutes = now.saturating_sub(at_ms).max(0) / 60_000;
    match minutes {
        0 => "Checked just now".into(),
        1 => "Checked 1 minute ago".into(),
        2..60 => format!("Checked {minutes} minutes ago"),
        60..120 => "Checked 1 hour ago".into(),
        120..1440 => format!("Checked {} hours ago", minutes / 60),
        1440..2880 => "Checked 1 day ago".into(),
        _ => format!("Checked {} days ago", minutes / 1440),
    }
}

fn access_label(state: AccessState) -> &'static str {
    match state {
        AccessState::Unchecked => "Not checked yet",
        AccessState::Readable => "Read access verified",
        AccessState::SignedOut => "Signed out",
        AccessState::CredentialUnavailable => "Sign-in unavailable",
        AccessState::Unauthorized => "Sign-in expired or revoked",
        AccessState::SsoRequired => "SSO authorization required",
        AccessState::ApprovalPending => "Organization approval pending",
        AccessState::InsufficientPermission => "Insufficient permission",
        AccessState::NotFoundOrDenied => "Missing or inaccessible",
        AccessState::TransportUnavailable => "Cannot reach GitHub securely",
        AccessState::RateLimited => "Rate limited",
        AccessState::ServiceUnavailable => "GitHub access check failed",
    }
}

fn sign_in_label(state: AccessState) -> &'static str {
    match state {
        AccessState::SignedOut => "Signed out",
        AccessState::Unauthorized => "Sign-in rejected by GitHub",
        AccessState::CredentialUnavailable => "Sign-in unavailable",
        _ => "Signed in",
    }
}
