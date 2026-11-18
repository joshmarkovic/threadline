use std::collections::HashSet;

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Disableable, Sizable};
use gpui_kit::{
    App, AppContext, Context, Entity, Focusable, InteractiveElement, IntoElement, ParentElement,
    Render, Role, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder,
};
use threadline_core::{
    github::{AccessError, Reachability},
    sync::Cancellation,
};
use threadline_storage::access::{AccessProfile, AccessState, DiscoveryState, RepositoryView};

use crate::{
    accounts::{AccountCommand, AccountReply, AccountService, AccountView, Pat},
    palette::Palette,
};

#[cfg(feature = "e2e")]
#[derive(serde::Serialize)]
pub(crate) struct AccountObservation {
    panel_open: bool,
    busy: bool,
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
    input_issue: Option<crate::accounts::AccountInputIssue>,
}

pub(crate) struct AccountPanel {
    service: AccountService,
    runtime: tokio::runtime::Handle,
    focus: gpui_kit::FocusHandle,
    route: Entity<InputState>,
    token: Entity<InputState>,
    renew: Option<AccessProfile>,
    view: AccountView,
    busy: bool,
    loaded: bool,
    message: Option<&'static str>,
    github_error: Option<AccessError>,
    reachability: Reachability,
    cancel: Cancellation,
    #[cfg(feature = "e2e")]
    observer: Option<crate::observation::Observer>,
    #[cfg(feature = "e2e")]
    input_issue: Option<crate::accounts::AccountInputIssue>,
    #[cfg(feature = "e2e")]
    route_observation: Option<gpui_kit::Subscription>,
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
        let token = password_input(window, cx);
        #[cfg(feature = "e2e")]
        let route_observation = service
            .fixture
            .then(|| cx.observe(&route, |_, _, cx| cx.notify()));
        #[cfg(feature = "e2e")]
        let token_observation = service
            .fixture
            .then(|| cx.observe(&token, |_, _, cx| cx.notify()));
        let mut panel = Self {
            service,
            runtime,
            focus: cx.focus_handle(),
            route,
            token,
            renew: None,
            view: AccountView::default(),
            busy: false,
            loaded: false,
            message: None,
            github_error: None,
            reachability: Reachability::NotAttempted,
            cancel: Cancellation::new(),
            #[cfg(feature = "e2e")]
            observer: None,
            #[cfg(feature = "e2e")]
            input_issue: None,
            #[cfg(feature = "e2e")]
            route_observation,
            #[cfg(feature = "e2e")]
            token_observation,
        };
        panel.launch(AccountCommand::Load, None, cx);
        panel
    }

    pub(crate) fn focus_form(&self, window: &mut Window, cx: &mut App) {
        self.route.focus_handle(cx).focus(window, cx);
    }

    pub(crate) fn loaded(&self) -> bool {
        self.loaded
    }

    pub(crate) fn reload_local_profile(&mut self, cx: &mut Context<Self>) {
        self.launch(AccountCommand::Load, None, cx);
    }

    pub(crate) fn clear_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                .github_error
                .as_ref()
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
            input_issue: self.input_issue,
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
        if let Some(window) = window {
            self.focus.focus(window, cx);
        }
        self.busy = true;
        self.message = None;
        self.github_error = None;
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
                panel.loaded = true;
                panel.cancel = Cancellation::new();
                match result {
                    Ok(AccountReply {
                        view,
                        message,
                        github_error,
                        reachability,
                        #[cfg(feature = "e2e")]
                        input_issue,
                    }) => {
                        panel.view = view;
                        panel.message = message;
                        panel.github_error = github_error;
                        #[cfg(feature = "e2e")]
                        {
                            panel.input_issue = input_issue;
                        }
                        if reachability != Reachability::NotAttempted {
                            panel.reachability = reachability;
                        }
                    }
                    Err(_) => panel.message = Some("The account check did not finish."),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let repository = self.route.read(cx).value().to_string();
        let pat = Pat::from_input(self.token.read(cx).text().to_string());
        let renew = self.renew.take();
        self.clear_token(window, cx);
        self.launch(
            AccountCommand::Connect {
                repository,
                pat,
                renew,
            },
            Some(window),
            cx,
        );
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
            .find(|profile| Some(profile.account) == self.view.active)
    }

    pub(crate) fn refresh(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
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

    fn forms(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_sm().child(if self.renew.is_some() {
                "Replace the token for this account"
            } else {
                "Connect a GitHub account"
            }))
            .child(
                self.control(
                    "account-repository",
                    Input::new(&self.route)
                        .small()
                        .disabled(self.busy)
                        .accessibility_id("account-repository")
                        .aria_label("GitHub repository"),
                ),
            )
            .child(
                self.control(
                    "account-token",
                    Input::new(&self.token)
                        .small()
                        .disabled(self.busy)
                        .accessibility_id("account-token")
                        .aria_label("GitHub personal access token"),
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
                    .when(self.renew.is_some(), |row| {
                        row.child(
                            Button::new("account-add")
                                .label("Add another account")
                                .small()
                                .on_click(cx.listener(|panel, _, _, cx| {
                                    panel.renew = None;
                                    cx.notify();
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
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(palette.muted)
                            .child(if self.busy { "Checking…" } else { "" }),
                    ),
            )
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
                let active = Some(account) == self.view.active;
                Some(
                    Button::new(format!("account-switch-{}", account.0))
                        .label(if active {
                            format!("{} (active)", profile.login)
                        } else {
                            profile.login.clone()
                        })
                        .small()
                        .disabled(self.busy || active)
                        .on_click(cx.listener(move |panel, _, window, cx| {
                            panel.route =
                                cx.new(|cx| InputState::new(window, cx).placeholder("owner/name"));
                            #[cfg(feature = "e2e")]
                            if panel.service.fixture {
                                panel.route_observation =
                                    Some(cx.observe(&panel.route, |_, _, cx| cx.notify()));
                            }
                            panel.clear_token(window, cx);
                            panel.renew = None;
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
                        .child(
                            self.control(
                                "account-sign-out",
                                Button::new("account-sign-out")
                                    .label("Sign out")
                                    .small()
                                    .accessibility_id("account-sign-out")
                                    .disabled(self.busy)
                                    .on_click(cx.listener(move |panel, _, window, cx| {
                                        panel.launch(
                                            AccountCommand::SignOut(account),
                                            Some(window),
                                            cx,
                                        )
                                    })),
                            ),
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
                            .gap_2()
                            .child(div().text_sm().child(format!(
                                "{} · {}",
                                profile.owner_scope,
                                access_label(profile.access)
                            )))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(palette.muted)
                                    .child(discovery_label(profile.discovery)),
                            )
                            .child(
                                Button::new(format!(
                                    "account-renew-{}-{}",
                                    profile.account.0, profile.owner_scope
                                ))
                                .label("Replace token")
                                .small()
                                .disabled(self.busy)
                                .on_click(cx.listener(
                                    move |panel, _, window, cx| {
                                        panel.renew = Some(target.clone());
                                        panel.focus_form(window, cx);
                                        cx.notify();
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
        let access = match profile.as_ref().map(|profile| profile.access) {
            Some(AccessState::Readable)
                if !repo.verified_current
                    && matches!(repo.access, AccessState::Readable | AccessState::Unchecked) =>
            {
                "Check access with the current token"
            }
            Some(state) if state != AccessState::Readable => access_label(state),
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
            .p_2()
            .border_b_1()
            .border_color(palette.border)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .child(format!("{}/{}", repo.owner, repo.name)),
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
    }
}

impl Render for AccountPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::current(cx);
        let status = self
            .github_error
            .as_ref()
            .map(ToString::to_string)
            .or_else(|| self.message.map(str::to_owned));
        let authorization = self
            .github_error
            .as_ref()
            .and_then(AccessError::authorization_url);
        let reachability = match self.reachability {
            _ if self.view.offline => "Working offline",
            Reachability::NotAttempted => "GitHub has not been checked in this session",
            Reachability::NoResponse => "No secure response from GitHub",
            Reachability::ReceivedResponse => "GitHub responded",
        };
        let repos = self
            .view
            .repositories
            .iter()
            .enumerate()
            .map(|(index, repo)| {
                self.repository_row(index, repo, palette, cx)
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        div()
            .id("account-panel")
            .track_focus(&self.focus)
            .tab_stop(true)
            .role(Role::Group)
            .aria_label("GitHub account and repositories")
            .w_full()
            .h_full()
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_4()
            .bg(palette.panel)
            .text_color(palette.ink)
            .child(div().text_lg().child("GitHub accounts"))
            .child(self.profile_controls(palette, cx))
            .child(self.forms(palette, cx))
            .children(status.map(|status| {
                div()
                    .id("account-status")
                    .role(Role::Status)
                    .text_sm()
                    .child(status)
            }))
            .children(authorization.map(|url| {
                Button::new("account-open-authorization")
                    .label("Open GitHub authorization")
                    .small()
                    .accessibility_id("account-open-authorization")
                    .on_click(move |_, _, cx| cx.open_url(url.as_str()))
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(palette.muted)
                    .child(reachability),
            )
            .children(repos)
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
        AccessState::Unchecked => "Access not checked",
        AccessState::Readable => "Read access verified",
        AccessState::SignedOut => "Signed out",
        AccessState::CredentialUnavailable => "Protected token unavailable",
        AccessState::Unauthorized => "Token expired or revoked",
        AccessState::SsoRequired => "SSO authorization required",
        AccessState::ApprovalPending => "Organization approval pending",
        AccessState::InsufficientPermission => "Insufficient permission",
        AccessState::NotFoundOrDenied => "Missing or inaccessible",
        AccessState::TransportUnavailable => "Cannot reach GitHub securely",
        AccessState::RateLimited => "Rate limited",
        AccessState::ServiceUnavailable => "GitHub access check failed",
    }
}

fn discovery_label(state: DiscoveryState) -> &'static str {
    match state {
        DiscoveryState::Unchecked => "Repositories unchecked",
        DiscoveryState::Partial => "Repository list incomplete",
        DiscoveryState::RestrictedBySso => "Repository list incomplete due to SSO",
        DiscoveryState::Complete => "Repository list checked",
    }
}
