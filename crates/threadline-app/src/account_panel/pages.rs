use gpui_kit::component::{
    Disableable, Selectable, Sizable,
    button::{Button, ButtonVariants},
    input::Input,
};
use gpui_kit::{
    AnyElement, Context, Focusable, InteractiveElement, IntoElement, ParentElement, Role,
    StatefulInteractiveElement, Styled, TestSupportExt, div, prelude::FluentBuilder, px,
};
use threadline_core::github::AccessError;
use threadline_storage::access::DiscoveryState;

use super::{AccountPanel, AccountPanelEvent, OnboardingStep, sign_in_label};
use crate::{accounts::AccessIssueScope, palette::Palette};

impl AccountPanel {
    pub(super) fn onboarding_page(
        &self,
        step: OnboardingStep,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (title, detail, content) = match step {
            OnboardingStep::TimeZone => (
                "Welcome to Threadline",
                "Start by choosing how dates and times appear in your workspace.",
                self.time_zone_settings(true, palette, cx)
                    .into_any_element(),
            ),
            OnboardingStep::GitHub => (
                "Sign in to GitHub",
                "Connect your GitHub account and check a repository to get started.",
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(self.credential_form(palette, cx))
                    .child(self.reply(cx))
                    .into_any_element(),
            ),
            OnboardingStep::Repository => (
                "Choose a repository",
                "Select a repository to monitor. You can change your selection in Settings.",
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(self.identity())
                    .child(self.repositories(palette, cx))
                    .child(
                        div().flex().child(
                            self.control(
                                "onboarding-continue",
                                Button::new("onboarding-continue")
                                    .primary()
                                    .accessibility_id("onboarding-continue")
                                    .label(if self.view.offline {
                                        "Continue offline"
                                    } else if self
                                        .view
                                        .repositories
                                        .iter()
                                        .any(|repo| repo.selected)
                                    {
                                        "Continue to Home"
                                    } else {
                                        "Continue without monitoring"
                                    })
                                    .disabled(self.busy)
                                    .on_click(cx.listener(|panel, _, _, cx| {
                                        panel.onboarding = None;
                                        cx.emit(AccountPanelEvent::OnboardingFinished);
                                        cx.notify();
                                    })),
                            ),
                        ),
                    )
                    .into_any_element(),
            ),
        };
        div()
            .flex()
            .flex_col()
            .gap_4()
            .w_full()
            .max_w(px(760.))
            .mx_auto()
            .p_4()
            .child(div().text_xl().child(title))
            .child(div().text_sm().text_color(palette.muted).child(detail))
            .child(content)
            .into_any_element()
    }

    pub(super) fn settings_page(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        if let Some(sync) = &self.synchronization {
            return div()
                .flex()
                .flex_col()
                .size_full()
                .max_w(px(920.))
                .mx_auto()
                .child(
                    div().flex().child(
                        self.control(
                            "settings-sync-back",
                            Button::new("settings-sync-back")
                                .accessibility_id("settings-sync-back")
                                .label("Back to Settings")
                                .small()
                                .on_click(cx.listener(|panel, _, window, cx| {
                                    panel.set_page(super::AccountPanelPage::Settings, window, cx);
                                })),
                        ),
                    ),
                )
                .child(sync.clone())
                .into_any_element();
        }
        let form_title = if self.renew.is_some() {
            "Reconnect this GitHub account"
        } else {
            "Add a GitHub account"
        };
        div()
            .flex()
            .flex_col()
            .gap_5()
            .max_w(px(920.))
            .w_full()
            .mx_auto()
            .child(div().text_xl().child("Settings"))
            .when(self.view.active.is_some(), |settings| {
                settings.child(
                    div().flex().child(
                        self.control(
                            "settings-synchronization",
                            Button::new("settings-synchronization")
                                .accessibility_id("settings-synchronization")
                                .label("Synchronization")
                                .small()
                                .on_click(cx.listener(|_, _, _, cx| {
                                    cx.emit(AccountPanelEvent::OpenSynchronization)
                                })),
                        ),
                    ),
                )
            })
            .child(
                section("GitHub account", palette)
                    .child(self.identity())
                    .child(self.profile_controls(cx))
                    .child(
                        div().flex().child(
                            self.control(
                                "account-add",
                                Button::new("account-add")
                                    .accessibility_id("account-add")
                                    .label("Add account")
                                    .small()
                                    .disabled(self.busy)
                                    .on_click(cx.listener(|panel, _, window, cx| {
                                        panel.start_credential_form(None, window, cx);
                                    })),
                            ),
                        ),
                    )
                    .when(self.credential_form_open, |group| {
                        group
                            .child(div().text_sm().child(form_title))
                            .child(self.credential_form(palette, cx))
                            .child(self.reply(cx))
                    }),
            )
            .child(section("Display", palette).child(self.time_zone_settings(false, palette, cx)))
            .children(self.updates.clone())
            .child(section("Repositories", palette).child(self.repositories(palette, cx)))
            .into_any_element()
    }

    fn identity(&self) -> impl IntoElement {
        let text = self.current_profile().map_or_else(
            || "No GitHub account connected".to_owned(),
            |profile| format!("{} · {}", profile.login, sign_in_label(profile.access)),
        );
        status("account-identity", text)
    }

    fn repositories(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.view.active;
        let profiles = || {
            self.view
                .profiles
                .iter()
                .filter(|profile| Some(profile.account) == active)
        };
        let discovery = if profiles()
            .any(|profile| profile.discovery == DiscoveryState::RestrictedBySso)
        {
            "GitHub withheld some repositories because of SSO. You can still select repositories that are listed."
        } else if profiles().any(|profile| profile.discovery == DiscoveryState::Partial) {
            "The repository list is incomplete."
        } else if profiles().any(|profile| profile.discovery == DiscoveryState::Complete) {
            "Repository list checked."
        } else {
            "Repository list not checked yet."
        };
        let rows = self
            .view
            .visible_repositories()
            .enumerate()
            .map(|(index, repo)| self.repository_row(index, repo, palette, cx))
            .collect::<Vec<_>>();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .when(active.is_some() && !self.credential_form_open, |group| {
                group
                    .child(div().max_w(px(520.)).child(self.repository_field(palette)))
                    .child(
                        div().flex().child(
                            self.control(
                                "account-check-repository",
                                Button::new("account-check-repository")
                                    .accessibility_id("account-check-repository")
                                    .label("Check repository")
                                    .small()
                                    .disabled(self.busy || self.view.offline)
                                    .on_click(cx.listener(|panel, _, window, cx| {
                                        panel.check_repository(window, cx);
                                    })),
                            ),
                        ),
                    )
            })
            .when(!self.credential_form_open, |group| {
                group.child(self.reply(cx))
            })
            .when(active.is_some(), |group| {
                group.child(status("account-discovery-status", discovery))
            })
            .children(rows)
    }

    pub(super) fn authorization_error(&self) -> Option<&AccessError> {
        self.github_error
            .as_ref()
            .filter(|error| !self.cli_reply || error.sso_action.is_some())
    }

    fn reply(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let text = if self.browser.in_progress() {
            None
        } else if self.busy {
            Some("Checking GitHub…".into())
        } else if let Some(error) = self.github_error.as_ref() {
            let label = match self.issue_scope {
                Some(AccessIssueScope::Repository) => "Repository access",
                Some(AccessIssueScope::Discovery) => "Repository list",
                _ => "GitHub sign-in",
            };
            Some(format!("{label} · {error}"))
        } else if self.view.offline {
            Some("Working offline".into())
        } else {
            self.message.map(str::to_owned)
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(text.map(|text| status("account-status", text)))
            .when(self.cli_reply && self.github_error.as_ref().is_some_and(|error| matches!(
                error.kind, threadline_core::github::AccessKind::SsoRequired
                | threadline_core::github::AccessKind::ApprovalPending
                | threadline_core::github::AccessKind::MissingOrInaccessible
                | threadline_core::github::AccessKind::PermissionDenied
            )), |group| group.child(div().text_sm().child(
                "For an organization repository, check that this account has access, complete organization single sign-on, and request GitHub CLI approval if required.",
            )))
            .children(
                self.authorization_error()
                    .and_then(AccessError::authorization_url)
                    .map(|url| {
                        Button::new("account-open-authorization")
                            .label("Open GitHub authorization")
                            .small()
                            .accessibility_id("account-open-authorization")
                            .on_click(cx.listener(move |_, _, _, cx| cx.open_url(url.as_str())))
                    }),
            )
    }

    fn time_zone_settings(
        &self,
        onboarding: bool,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let effective = format!(
            "Display time zone · {}",
            crate::display_time::DisplayClock::current(cx).name()
        );
        let (id, label) = if onboarding {
            ("onboarding-zone-continue", "Continue to GitHub")
        } else {
            ("display-timezone-save", "Save time zone")
        };
        let device_label = cx
            .try_global::<crate::display_time::DisplayClock>()
            .map_or_else(
                || "Use device time zone".to_owned(),
                |clock| format!("Use device time zone ({})", clock.device_name()),
            );
        let choices = [
            ("display-timezone-system", device_label, "system"),
            (
                "display-timezone-new-york",
                "New York (Eastern Time)".into(),
                "America/New_York",
            ),
            (
                "display-timezone-utc",
                "Coordinated Universal Time (UTC)".into(),
                "UTC",
            ),
        ];
        let selected = self.display_zone.read(cx).value();
        div()
            .id("display-timezone-settings")
            .accessibility_id("display-timezone-settings")
            .role(Role::Group)
            .aria_label("Display time zone")
            .flex()
            .flex_col()
            .gap_2()
            .max_w(px(520.))
            .child(status("display-timezone-effective", effective))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .flex_wrap()
                    .children(choices.into_iter().map(|(id, label, value)| {
                        self.control(
                            id,
                            Button::new(id)
                                .accessibility_id(id)
                                .label(label)
                                .small()
                                .selected(selected.as_ref() == value)
                                .disabled(self.saving_display_zone)
                                .on_click(cx.listener(move |panel, _, window, cx| {
                                    panel.custom_display_zone = false;
                                    panel.display_zone.update(cx, |editor, cx| {
                                        editor.set_value(value, window, cx);
                                    });
                                    cx.notify();
                                })),
                        )
                    }))
                    .when(onboarding, |options| {
                        options.child(
                            Button::new("display-timezone-other")
                                .accessibility_id("display-timezone-other")
                                .label("Other time zone…")
                                .small()
                                .selected(self.custom_display_zone)
                                .disabled(self.saving_display_zone)
                                .on_click(cx.listener(|panel, _, window, cx| {
                                    panel.custom_display_zone = true;
                                    panel.display_zone.focus_handle(cx).focus(window, cx);
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .when(!onboarding || self.custom_display_zone, |group| {
                group
                    .child(
                        div()
                            .text_sm()
                            .text_color(palette.muted)
                            .child("Time zone identifier (for example, Europe/Paris)"),
                    )
                    .child(
                        Input::new(&self.display_zone)
                            .accessibility_id("display-timezone")
                            .aria_label("Display time zone"),
                    )
            })
            .child(
                div().flex().child(
                    self.control(
                        id,
                        Button::new(id)
                            .label(label)
                            .accessibility_id(id)
                            .when(onboarding, |button| button.primary())
                            .disabled(self.saving_display_zone)
                            .on_click(cx.listener(move |panel, _, _, cx| {
                                panel.save_display_zone(onboarding, cx);
                            })),
                    ),
                ),
            )
            .children(
                self.display_zone_message
                    .map(|message| status("display-timezone-message", message)),
            )
    }
}

fn section(title: &'static str, palette: Palette) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_3()
        .border_1()
        .border_color(palette.border)
        .rounded_md()
        .child(div().text_lg().child(title))
}

pub(super) fn status(
    id: &'static str,
    text: impl Into<gpui_kit::SharedString>,
) -> impl IntoElement {
    let text = text.into();
    div()
        .id(id)
        .accessibility_id(id)
        .test_support()
        .role(Role::Status)
        .aria_label(text.clone())
        .text_sm()
        .child(text)
}
