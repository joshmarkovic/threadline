use super::{
    view::{control, time_label},
    *,
};
use crate::palette::Palette;
use gpui_kit::{
    AnyElement, InteractiveElement, IntoElement, ParentElement, Role, StatefulInteractiveElement,
    Styled, TestSupportExt,
    component::{Disableable, Selectable, input::Input},
    div,
};
use std::{collections::BTreeSet, num::NonZeroU64};
use threadline_storage::{
    RepositoryId,
    notifications::{
        ActorRule, Category, DeliveryMode, Destination, FeedEntry, NotificationSettings,
        NotificationTarget, QuietHours, Relevance, RepositoryRules, RuleActor, RuleScope,
    },
};

mod mute;

const FEED_PAGE: u16 = 40;

#[derive(Default)]
pub(super) struct ActivityState {
    settings: Option<NotificationSettings>,
    repository: Option<RepositoryId>,
    overrides: Option<RepositoryRules>,
    feed: Vec<FeedEntry>,
    before: Option<i64>,
    pub(super) busy: bool,
    inbox_busy: bool,
    pending_open: Option<(String, Destination)>,
    rule_form: Option<RuleForm>,
    mute_preview: Option<mute::MutePreview>,
}

#[cfg(feature = "e2e")]
#[derive(serde::Serialize)]
pub(crate) struct NotificationObservation {
    preferences: bool,
    saved: bool,
    quiet_hours: Option<QuietHours>,
    popup_default: bool,
    isolated_quiet_event_feed_only: bool,
}

fn notification_tab_anchor(anchor: &str) -> std::borrow::Cow<'_, str> {
    let provider_comment = anchor
        .strip_prefix("issue-comment:")
        .or_else(|| anchor.strip_prefix("comment:"))
        .and_then(|id| id.parse::<NonZeroU64>().ok());
    provider_comment.map_or_else(
        || std::borrow::Cow::Borrowed(anchor),
        |id| std::borrow::Cow::Owned(format!("{id:020}")),
    )
}

#[cfg(all(test, feature = "e2e"))]
impl ActivityState {
    pub(super) fn scope_is_ready(&self, scope: Option<RepositoryId>) -> bool {
        !self.busy && self.settings.is_some() && self.repository == scope
    }
}

struct RuleForm {
    actor_id: Entity<InputState>,
    start: Entity<InputState>,
    end: Entity<InputState>,
    timezone: Entity<InputState>,
}

#[derive(Clone)]
enum Setting {
    Enabled,
    Sound,
    SuppressOwn,
    Mentions,
    QuietHours,
    Mute,
    Fallback(DeliveryMode),
    Category(Category, Option<DeliveryMode>),
    Relevance(Option<Relevance>),
    IgnoreAutomatedReviews,
    IgnoreAutomatedComments,
    Actor(ActorRule),
    AddReviewMute(ActorRule),
    QuietDetails(QuietHours),
    QuietDay(u8),
}

fn delivery(mode: DeliveryMode) -> &'static str {
    match mode {
        DeliveryMode::Off => "Off",
        DeliveryMode::FeedOnly => "Feed only",
        DeliveryMode::Popup => "Feed and popup",
    }
}

fn category_name(category: Category) -> &'static str {
    match category {
        Category::BranchUpdate => "Branch updates",
        Category::OrdinaryComment => "Conversation comments",
        Category::InlineComment => "Inline comments",
        Category::ReviewRequest => "Review requests",
        Category::Approval => "Approvals",
        Category::ChangesRequested => "Changes requested",
        Category::CommentOnlyReview => "Comment-only reviews",
        Category::PrOpened => "Pull requests opened",
        Category::PrReady => "Pull requests ready",
        Category::PrMerged => "Pull requests merged",
        Category::IssueAssignment => "Issue assignments",
        Category::IssueState => "Issue state changes",
        Category::DirectMention => "Direct mentions",
        Category::CheckStatus => "Check status changes",
    }
}

fn format_minute(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

fn read_minute(value: &str) -> Option<u16> {
    let (hours, minutes) = value.trim().split_once(':')?;
    let (hours, minutes) = (hours.parse::<u16>().ok()?, minutes.parse::<u16>().ok()?);
    (hours < 24 && minutes < 60).then_some(hours.saturating_mul(60).saturating_add(minutes))
}

fn actor_scope(automated_reviews: bool) -> RuleScope {
    if automated_reviews {
        RuleScope::ReviewsAndInline
    } else {
        RuleScope::Categories(BTreeSet::from([Category::OrdinaryComment]))
    }
}

fn automated_enabled(rules: &[ActorRule], reviews: bool) -> bool {
    let scope = actor_scope(reviews);
    rules
        .iter()
        .any(|rule| rule.actor == RuleActor::KnownAutomation && rule.scope == scope)
}

fn toggle_automated(rules: &mut Vec<ActorRule>, reviews: bool) {
    let rule = ActorRule {
        actor: RuleActor::KnownAutomation,
        scope: actor_scope(reviews),
    };
    toggle_actor_rule(rules, rule);
}

fn toggle_actor_rule(rules: &mut Vec<ActorRule>, rule: ActorRule) {
    if let Some(index) = rules.iter().position(|candidate| candidate == &rule) {
        rules.remove(index);
    } else {
        rules.push(rule);
    }
}

fn update_global(settings: &mut NotificationSettings, change: Setting) {
    match change {
        Setting::Enabled => settings.enabled = !settings.enabled,
        Setting::Sound => settings.sound = !settings.sound,
        Setting::SuppressOwn => settings.suppress_own = !settings.suppress_own,
        Setting::Mentions => settings.mention_exception = !settings.mention_exception,
        Setting::QuietHours => {
            settings.quiet_hours = if settings.quiet_hours.is_some() {
                None
            } else {
                Some(QuietHours {
                    weekdays: 0x7f,
                    start_minute: 22 * 60,
                    end_minute: 7 * 60,
                    timezone: None,
                })
            };
        }
        Setting::Fallback(mode) => settings.fallback = mode,
        Setting::Category(category, mode) => match mode {
            Some(mode) => {
                settings.categories.insert(category, mode);
            }
            None => {
                settings.categories.remove(&category);
            }
        },
        Setting::Relevance(Some(value)) => settings.relevance = value,
        Setting::IgnoreAutomatedReviews => toggle_automated(&mut settings.actor_rules, true),
        Setting::IgnoreAutomatedComments => toggle_automated(&mut settings.actor_rules, false),
        Setting::Actor(rule) => toggle_actor_rule(&mut settings.actor_rules, rule),
        Setting::QuietDetails(value) => settings.quiet_hours = Some(value),
        Setting::QuietDay(day) => {
            if let Some(schedule) = settings.quiet_hours.as_mut() {
                schedule.weekdays ^= day;
            }
        }
        Setting::Mute | Setting::Relevance(None) | Setting::AddReviewMute(_) => {}
    }
}

fn update_override(rules: &mut RepositoryRules, change: Setting) {
    match change {
        Setting::Mute => rules.muted = !rules.muted,
        Setting::Category(category, mode) => match mode {
            Some(mode) => {
                rules.categories.insert(category, mode);
            }
            None => {
                rules.categories.remove(&category);
            }
        },
        Setting::Relevance(value) => rules.relevance = value,
        Setting::IgnoreAutomatedReviews => toggle_automated(&mut rules.actor_rules, true),
        Setting::IgnoreAutomatedComments => toggle_automated(&mut rules.actor_rules, false),
        Setting::Actor(rule) => toggle_actor_rule(&mut rules.actor_rules, rule),
        Setting::AddReviewMute(rule) if !rules.actor_rules.contains(&rule) => {
            rules.actor_rules.push(rule);
        }
        _ => {}
    }
}

impl ReaderWorkspace {
    #[cfg(feature = "e2e")]
    pub(super) fn notification_observation(&self) -> NotificationObservation {
        NotificationObservation {
            preferences: self.surface == Surface::NotificationSettings,
            saved: !self.activity.busy && self.activity.settings.is_some(),
            quiet_hours: self
                .activity
                .settings
                .as_ref()
                .and_then(|saved| saved.quiet_hours.clone()),
            popup_default: self
                .activity
                .settings
                .as_ref()
                .is_some_and(|settings| settings.fallback == DeliveryMode::Popup),
            isolated_quiet_event_feed_only: self.surface == Surface::Activity
                && self.activity.feed.iter().any(|entry| {
                    entry.event_key == "synthetic-quiet-activity-v1"
                        && entry.decision.mode == DeliveryMode::FeedOnly
                        && entry.decision.reasons.contains(
                            &threadline_storage::notifications::DecisionReason::QuietHours,
                        )
                        && !entry.decision.sound
                }),
        }
    }

    pub(super) fn drain_notification_inbox(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        if self.layout_busy
            || self.history.busy
            || self.departure.is_some()
            || self.activity.inbox_busy
            || self.activity.pending_open.is_some()
        {
            return;
        }
        self.activity.inbox_busy = true;
        self.launch(
            window,
            cx,
            move |store| store.pending_notification_open(account),
            move |this, result, window, cx| {
                this.activity.inbox_busy = false;
                if !this.layout_busy
                    && !this.history.busy
                    && let Ok(Some((id, destination))) = result
                {
                    this.activity.pending_open = Some((id, destination.clone()));
                    this.open_notification_destination(account, destination, window, cx);
                }
            },
        );
    }

    pub(super) fn finish_notification_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let complete = self.activity.pending_open.as_ref().is_some_and(|(_, destination)| {
            matches!(&destination.target, NotificationTarget::Item { item, thread }
                if self.active_tab().is_some_and(|tab| tab.item == *item
                    && thread.as_ref().is_none_or(|anchor| {
                        tab.view.scroll_anchor.as_deref() == Some(notification_tab_anchor(anchor).as_ref())
                    })))
        });
        if complete {
            self.complete_notification_open(window, cx);
        } else {
            self.drain_notification_inbox(window, cx);
        }
    }

    pub(super) fn complete_notification_open(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.account else { return };
        let Some((id, _)) = self.activity.pending_open.take() else {
            return;
        };
        self.activity.inbox_busy = true;
        self.launch(
            window,
            cx,
            move |store| store.complete_notification_open(account, &id),
            |this, result, window, cx| {
                this.activity.inbox_busy = false;
                if result.is_ok() {
                    this.drain_notification_inbox(window, cx);
                }
            },
        );
    }

    pub(super) fn show_activity(
        &mut self,
        preferences: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.surface = if preferences {
            Surface::NotificationSettings
        } else {
            Surface::Activity
        };
        self.load_activity(self.activity.repository, None, window, cx);
    }

    fn load_activity(
        &mut self,
        repository: Option<RepositoryId>,
        before: Option<i64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.account else { return };
        if self.activity.busy {
            return;
        }
        self.activity.mute_preview = None;
        self.activity.busy = true;
        self.launch(
            window,
            cx,
            move |store| {
                let settings = store.notification_settings(account)?;
                let rules = repository
                    .map(|repo| store.repository_notification_rules(account, repo))
                    .transpose()?;
                let feed = store.notification_feed(account, before, FEED_PAGE)?;
                Ok((settings, rules, feed))
            },
            move |this, result, window, cx| {
                this.activity.busy = false;
                match result {
                    Ok((settings, rules, feed)) => {
                        if this.activity.rule_form.is_none() {
                            let start = settings
                                .quiet_hours
                                .as_ref()
                                .map_or(22 * 60, |quiet| quiet.start_minute);
                            let end = settings
                                .quiet_hours
                                .as_ref()
                                .map_or(7 * 60, |quiet| quiet.end_minute);
                            let timezone = settings
                                .quiet_hours
                                .as_ref()
                                .and_then(|quiet| quiet.timezone.clone())
                                .unwrap_or_default();
                            this.activity.rule_form = Some(RuleForm {
                                actor_id: cx.new(|cx| InputState::new(window, cx)),
                                start: cx.new(|cx| {
                                    InputState::new(window, cx).default_value(format_minute(start))
                                }),
                                end: cx.new(|cx| {
                                    InputState::new(window, cx).default_value(format_minute(end))
                                }),
                                timezone: cx
                                    .new(|cx| InputState::new(window, cx).default_value(timezone)),
                            });
                        }
                        this.activity.settings = Some(settings);
                        this.activity.repository = repository;
                        this.activity.overrides = rules;
                        this.activity.feed = feed;
                        this.activity.before = before;
                    }
                    Err(_) => {
                        this.status =
                            Some("The saved notification rules or feed could not be loaded.")
                    }
                }
                this.settle_departure(window, cx);
            },
        );
    }

    fn save_notification_preference(
        &mut self,
        repository: Option<RepositoryId>,
        change: Setting,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.account else { return };
        if self.activity.busy || self.activity.repository != repository {
            return;
        }
        let confirms_preview = matches!(change, Setting::AddReviewMute(_));
        self.activity.busy = true;
        self.launch(
            window,
            cx,
            move |store| match repository {
                Some(repo) => store
                    .update_repository_notification_rules(account, repo, move |rules| {
                        update_override(rules, change)
                    })
                    .map(|rules| (None, Some(rules))),
                None => store
                    .update_notification_settings(account, move |settings| {
                        update_global(settings, change)
                    })
                    .map(|settings| (Some(settings), None)),
            },
            move |this, result, window, cx| {
                this.activity.busy = false;
                match result {
                    Ok((settings, rules)) => {
                        if confirms_preview {
                            this.activity.mute_preview = None;
                        }
                        if settings.is_some() {
                            this.activity.settings = settings;
                        }
                        if rules.is_some() {
                            this.activity.overrides = rules;
                        }
                        if let Some(worker) = this.sync_worker.as_ref() {
                            worker.wake(None);
                        }
                    }
                    Err(_) => this.status = Some("The notification preference could not be saved."),
                }
                this.settle_departure(window, cx);
            },
        );
        cx.notify();
    }

    pub(super) fn activity_surface(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let preferences = self.surface == Surface::NotificationSettings;
        let heading = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(
                control("activity-back", "Back to workspace").on_click(cx.listener(
                    |this, _, window, cx| {
                        this.browse(false, window, cx);
                    },
                )),
            )
            .child(
                control(
                    "activity-toggle-settings",
                    if preferences {
                        "Activity feed"
                    } else {
                        "Notification settings"
                    },
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.show_activity(!preferences, window, cx)
                })),
            );
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .flex_1()
            .min_h_0()
            .child(heading)
            .child(if preferences {
                self.notification_controls(palette, cx)
            } else {
                self.feed_controls(palette, cx)
            })
            .into_any_element()
    }

    fn notification_controls(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(global) = &self.activity.settings else {
            return div()
                .child("Loading notification preferences…")
                .into_any_element();
        };
        let repository = self.activity.repository;
        let overrides = self.activity.overrides.as_ref();
        let mut scopes = div().flex().flex_wrap().gap_1().child(
            control("notifications-global", "Account defaults")
                .selected(repository.is_none())
                .disabled(self.activity.busy)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.load_activity(None, None, window, cx);
                })),
        );
        for (index, repo) in self
            .repositories
            .iter()
            .filter(|repo| repo.selected)
            .enumerate()
        {
            let scope = repo.id;
            scopes = scopes.child(
                control(
                    format!("notifications-repository-{index}"),
                    format!("{}/{}", repo.owner, repo.name),
                )
                .selected(repository == Some(scope))
                .disabled(self.activity.busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.load_activity(Some(scope), None, window, cx);
                })),
            );
        }
        let mut panel = div()
            .id("notification-settings")
            .test_support()
            .role(Role::Group)
            .aria_label("Notification rules")
            .overflow_y_scroll()
            .min_h_0()
            .flex_1()
            .flex()
            .flex_col()
            .gap_2()
            .child(scopes)
            .children(self.review_mute_preview(cx));
        let flags = if let Some(overrides) = overrides {
            vec![("mute", "Mute repository", overrides.muted, Setting::Mute)]
        } else {
            vec![
                (
                    "enabled",
                    "Notifications enabled",
                    global.enabled,
                    Setting::Enabled,
                ),
                (
                    "sound",
                    "Play notification sounds",
                    global.sound,
                    Setting::Sound,
                ),
                (
                    "own",
                    "Suppress my own actions",
                    global.suppress_own,
                    Setting::SuppressOwn,
                ),
                (
                    "mentions",
                    "Allow direct-mention exceptions",
                    global.mention_exception,
                    Setting::Mentions,
                ),
                (
                    "quiet",
                    "Enable quiet hours",
                    global.quiet_hours.is_some(),
                    Setting::QuietHours,
                ),
            ]
        };
        let mut flag_controls = div().flex().flex_wrap().gap_1();
        for (key, label, selected, action) in flags {
            flag_controls = flag_controls.child(self.preference_button(
                (format!("notifications-{key}"), label),
                selected,
                action,
                cx,
            ));
        }
        panel = panel.child(flag_controls);
        let actor_rules = overrides.map_or(global.actor_rules.as_slice(), |rules| {
            rules.actor_rules.as_slice()
        });
        panel = panel.children(self.activity.rule_form.as_ref().map(|form| {
            self.rule_editor(
                form,
                actor_rules,
                repository
                    .is_none()
                    .then_some(global.quiet_hours.as_ref())
                    .flatten(),
                cx,
            )
        }));
        panel = panel.child(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(self.preference_button(
                    (
                        "notifications-automated-reviews".into(),
                        "Suppress automated reviews and inline comments",
                    ),
                    automated_enabled(actor_rules, true),
                    Setting::IgnoreAutomatedReviews,
                    cx,
                ))
                .child(self.preference_button(
                    (
                        "notifications-automated-comments".into(),
                        "Suppress automated conversation comments",
                    ),
                    automated_enabled(actor_rules, false),
                    Setting::IgnoreAutomatedComments,
                    cx,
                )),
        );

        let mut relevant = div().flex().flex_wrap().gap_1();
        for (key, label, mode) in [
            ("all", "All activity", Relevance::All),
            ("mine", "Activity involving me", Relevance::InvolvingMe),
            ("followed", "Followed activity", Relevance::Followed),
        ] {
            let selected =
                overrides.map_or(Some(global.relevance), |rules| rules.relevance) == Some(mode);
            relevant = relevant.child(self.preference_button(
                (format!("notifications-relevance-{key}"), label),
                selected,
                Setting::Relevance(Some(mode)),
                cx,
            ));
        }
        if let Some(overrides) = overrides {
            relevant = relevant.child(self.preference_button(
                (
                    "notifications-relevance-default".into(),
                    "Use account relevance",
                ),
                overrides.relevance.is_none(),
                Setting::Relevance(None),
                cx,
            ));
        }
        panel = panel.child(relevant);
        let modes = [
            DeliveryMode::Off,
            DeliveryMode::FeedOnly,
            DeliveryMode::Popup,
        ];
        if overrides.is_none() {
            let mut defaults = div().flex().flex_wrap().gap_1().child("Default delivery");
            for (index, mode) in modes.into_iter().enumerate() {
                defaults = defaults.child(self.preference_button(
                    (format!("notifications-default-{index}"), delivery(mode)),
                    global.fallback == mode,
                    Setting::Fallback(mode),
                    cx,
                ));
            }
            panel = panel.child(defaults);
        }
        for (index, category) in Category::ALL.iter().copied().enumerate() {
            let explicit = overrides
                .map_or_else(
                    || global.categories.get(&category),
                    |rules| rules.categories.get(&category),
                )
                .copied();
            let mut row = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .border_b_1()
                .border_color(palette.border)
                .child(div().text_sm().child(category_name(category)));
            row = row.child(self.preference_button(
                (
                    format!("notifications-category-{index}-default"),
                    "Use default",
                ),
                explicit.is_none(),
                Setting::Category(category, None),
                cx,
            ));
            for (mode_index, mode) in modes.into_iter().enumerate() {
                row = row.child(self.preference_button(
                    (
                        format!("notifications-category-{index}-{mode_index}"),
                        delivery(mode),
                    ),
                    explicit == Some(mode),
                    Setting::Category(category, Some(mode)),
                    cx,
                ));
            }
            panel = panel.child(row);
        }
        panel.into_any_element()
    }

    fn preference_button(
        &self,
        (id, label): (String, &'static str),
        selected: bool,
        action: Setting,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let repository = self.activity.repository;
        let button = control(id.clone(), label)
            .selected(selected)
            .accessibility_label(format!(
                "{label} · {}",
                if selected { "Selected" } else { "Unselected" }
            ))
            .disabled(self.activity.busy)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.save_notification_preference(repository, action.clone(), window, cx)
            }));
        self.observed_control(id, button, false)
    }

    fn rule_editor(
        &self,
        form: &RuleForm,
        rules: &[ActorRule],
        quiet: Option<&QuietHours>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut content = div().flex().flex_col().gap_1();
        let repo = self.activity.repository;
        let mut input = div().flex().flex_wrap().items_center().gap_1().child(
            div().w(px(145.)).child(
                Input::new(&form.actor_id)
                    .accessibility_id("notification-actor-id")
                    .aria_label("Numeric GitHub user or app ID"),
            ),
        );
        for (id, label, application, reviews) in [
            (
                "notifications-user-reviews",
                "Suppress user reviews",
                false,
                true,
            ),
            (
                "notifications-user-comments",
                "Suppress user conversation comments",
                false,
                false,
            ),
            (
                "notifications-app-reviews",
                "Suppress app reviews",
                true,
                true,
            ),
            (
                "notifications-app-comments",
                "Suppress app conversation comments",
                true,
                false,
            ),
        ] {
            let field = form.actor_id.clone();
            input = input.child(control(id, label).disabled(self.activity.busy).on_click(
                cx.listener(move |this, _, window, cx| {
                    let value = field
                        .read(cx)
                        .value()
                        .trim()
                        .parse::<std::num::NonZeroU64>();
                    if let Ok(id) = value {
                        let actor = if application {
                            RuleActor::App(id)
                        } else {
                            RuleActor::User(id)
                        };
                        this.save_notification_preference(
                            repo,
                            Setting::Actor(ActorRule {
                                actor,
                                scope: actor_scope(reviews),
                            }),
                            window,
                            cx,
                        );
                    } else {
                        this.status = Some("Enter a positive numeric GitHub user or app ID.");
                        cx.notify();
                    }
                }),
            ));
        }
        let mut listed = div().flex().flex_wrap().gap_1();
        for (index, rule) in rules.iter().enumerate() {
            let actor = match rule.actor {
                RuleActor::User(id) => format!("user {id}"),
                RuleActor::App(id) => format!("app {id}"),
                RuleActor::KnownAutomation => continue,
            };
            let scope = match &rule.scope {
                RuleScope::ReviewsAndInline => "reviews and inline comments".into(),
                RuleScope::Categories(categories) => categories
                    .iter()
                    .copied()
                    .map(category_name)
                    .collect::<Vec<_>>()
                    .join(", "),
            };
            let rule = rule.clone();
            listed = listed.child(
                control(
                    format!("notifications-remove-rule-{index}"),
                    format!("Remove {actor} rule · {scope}"),
                )
                .disabled(self.activity.busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.save_notification_preference(
                        repo,
                        Setting::Actor(rule.clone()),
                        window,
                        cx,
                    )
                })),
            );
        }
        content = content.child(input).child(listed);
        if let Some(quiet) = quiet {
            let (start, end, zone) = (form.start.clone(), form.end.clone(), form.timezone.clone());
            let mut hours = div()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(
                    div().w(px(112.)).child(
                        Input::new(&form.start)
                            .accessibility_id("notifications-quiet-start")
                            .aria_label("Quiet hours start, HH:MM"),
                    ),
                )
                .child(
                    div().w(px(112.)).child(
                        Input::new(&form.end)
                            .accessibility_id("notifications-quiet-end")
                            .aria_label("Quiet hours end, HH:MM"),
                    ),
                )
                .child(
                    div().w(px(175.)).child(
                        Input::new(&form.timezone)
                            .accessibility_id("notifications-quiet-zone")
                            .aria_label("IANA time zone, blank for local"),
                    ),
                );
            let weekdays = quiet.weekdays;
            hours = hours.child(
                control("notifications-save-quiet", "Save quiet hours")
                    .disabled(self.activity.busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let start_value = start.read(cx).value();
                        let end_value = end.read(cx).value();
                        let (Some(start_minute), Some(end_minute)) =
                            (read_minute(&start_value), read_minute(&end_value))
                        else {
                            this.status = Some("Enter quiet hours in 24-hour HH:MM format.");
                            cx.notify();
                            return;
                        };
                        let zone = zone.read(cx).value();
                        let timezone = (!zone.trim().is_empty()).then(|| zone.trim().to_owned());
                        this.save_notification_preference(
                            None,
                            Setting::QuietDetails(QuietHours {
                                weekdays,
                                start_minute,
                                end_minute,
                                timezone,
                            }),
                            window,
                            cx,
                        );
                    })),
            );
            for (index, label) in [
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
            {
                let day = 1 << index;
                hours = hours.child(self.preference_button(
                    (format!("notifications-quiet-day-{index}"), label),
                    quiet.weekdays & day != 0,
                    Setting::QuietDay(day),
                    cx,
                ));
            }
            content = content.child(hours);
        }
        content.into_any_element()
    }

    fn feed_controls(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut panel = div()
            .id("activity-feed")
            .role(Role::Group)
            .aria_label("Notification activity")
            .overflow_y_scroll()
            .min_h_0()
            .flex_1()
            .flex()
            .flex_col()
            .gap_2();
        if self.activity.feed.is_empty() {
            return panel
                .child(if self.activity.busy {
                    "Loading activity…"
                } else {
                    "No matching activity is available."
                })
                .into_any_element();
        }
        let mut page = div().flex().gap_1();
        if self.activity.before.is_some() {
            page = page.child(
                control("activity-newest", "Newest activity")
                    .disabled(self.activity.busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.load_activity(this.activity.repository, None, window, cx)
                    })),
            );
        }
        if self.activity.feed.len() == usize::from(FEED_PAGE)
            && let Some(before) = self.activity.feed.last().map(|event| event.id)
        {
            page = page.child(
                control("activity-older", "Older activity")
                    .disabled(self.activity.busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.load_activity(this.activity.repository, Some(before), window, cx)
                    })),
            );
        }
        panel = panel.child(page);
        for event in &self.activity.feed {
            let labels = event
                .categories
                .iter()
                .copied()
                .map(category_name)
                .collect::<Vec<_>>()
                .join(" · ");
            let source = self
                .repositories
                .iter()
                .find(|repo| repo.id == event.repository)
                .map_or_else(
                    || "Repository unavailable".into(),
                    |repo| format!("{}/{}", repo.owner, repo.name),
                );
            let reason = event
                .decision
                .reasons
                .iter()
                .map(|reason| reason.description())
                .collect::<Vec<_>>()
                .join(" ");
            let target = Destination {
                repository: Some(event.repository),
                target: event.target.clone(),
            };
            let event_id = event.id;
            let read_target = target.clone();
            let mut row = div()
                .p_2()
                .border_b_1()
                .border_color(palette.border)
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_sm().child(format!("{labels} · {source}")))
                .child(div().text_xs().text_color(palette.muted).child(format!(
                    "{} · {} · {}",
                    if event.read_at_ms.is_some() {
                        "Read"
                    } else {
                        "Unread"
                    },
                    time_label(event.occurred_at_ms.or(Some(event.observed_at_ms))),
                    reason,
                )));
            let mut buttons = div().flex().flex_wrap().gap_1().child(
                control(format!("activity-open-{event_id}"), "Open").on_click(cx.listener(
                    move |this, _, window, cx| {
                        if let Some(account) = this.account {
                            this.open_notification_destination(account, target.clone(), window, cx);
                        }
                    },
                )),
            );
            if let (Some(account), Some(actor)) = (self.account, event.review_actor.clone())
                && self
                    .repositories
                    .iter()
                    .any(|repo| repo.id == event.repository && repo.selected)
            {
                buttons = buttons.child(self.review_mute_control(
                    format!("activity-mute-review-{event_id}"),
                    account,
                    event.repository,
                    actor,
                    cx,
                ));
            }
            if event.read_at_ms.is_none() {
                buttons = buttons.child(
                    control(format!("activity-read-{event_id}"), "Mark read")
                        .disabled(self.activity.busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.update_feed_event(event_id, None, window, cx)
                        })),
                );
            }
            if matches!(
                event.target,
                NotificationTarget::Item { .. } | NotificationTarget::Comparison { .. }
            ) {
                buttons = buttons.child(
                    control(format!("activity-snooze-{event_id}"), "Snooze for one hour")
                        .disabled(self.activity.busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.update_feed_event(event_id, Some(read_target.clone()), window, cx)
                        })),
                );
            }
            row = row.child(buttons);
            panel = panel.child(row);
        }
        panel.into_any_element()
    }

    fn update_feed_event(
        &mut self,
        event_id: i64,
        snooze: Option<Destination>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.account else { return };
        if self.activity.busy {
            return;
        }
        self.activity.busy = true;
        self.launch(
            window,
            cx,
            move |store| {
                let now = now_ms();
                if let Some(destination) = snooze {
                    let repo = destination
                        .repository
                        .ok_or(threadline_storage::Error::NotFound)?;
                    store.snooze_notification_target(
                        account,
                        repo,
                        &destination.target,
                        Some(now.saturating_add(3_600_000)),
                    )?;
                } else {
                    store.mark_notification_read(account, event_id, now)?;
                }
                Ok(())
            },
            |this, result, window, cx| {
                this.activity.busy = false;
                if result.is_ok() {
                    this.load_activity(this.activity.repository, this.activity.before, window, cx);
                } else {
                    this.status = Some("The activity update could not be saved.");
                }
            },
        );
    }

    pub(crate) fn open_notification_destination(
        &mut self,
        account: AccountId,
        destination: Destination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.account != Some(account) || self.layout_busy || self.departure.is_some() {
            return;
        }
        if self
            .activity
            .pending_open
            .as_ref()
            .is_some_and(|(_, pending)| pending != &destination)
        {
            return;
        }
        let repository = destination.repository;
        match destination.target {
            NotificationTarget::ActivityFeed => {
                self.show_activity(false, window, cx);
                self.complete_notification_open(window, cx);
            }
            NotificationTarget::Repository => {
                if let Some(repo) = repository.filter(|id| {
                    self.repositories
                        .iter()
                        .any(|entry| entry.id == *id && entry.selected)
                }) {
                    self.scope = ReaderScope::Repository(repo);
                    self.browse(false, window, cx);
                } else {
                    self.show_activity(false, window, cx);
                }
                self.complete_notification_open(window, cx);
            }
            NotificationTarget::Item { item, thread } => {
                let thread = thread.map(|anchor| notification_tab_anchor(&anchor).into_owned());
                self.launch(
                    window, cx,
                    move |store| store.item(account, item),
                    move |this, result, window, cx| {
                        match result {
                            Ok(Some(record)) if repository.is_none_or(|repo| repo == record.locator.repository)
                                && this.repositories.iter().any(|repo| repo.id == record.locator.repository && repo.selected) =>
                                if this.layout_busy {
                                    this.activity.pending_open = None;
                                } else {
                                    this.open(item, thread, OpenTab::Activation, window, cx);
                                },
                            _ => {
                                this.show_activity(false, window, cx);
                                this.status = Some("The activity item is no longer available in the selected repositories.");
                                this.complete_notification_open(window, cx);
                            }
                        }
                    },
                );
            }
            NotificationTarget::Comparison {
                reference,
                base,
                head,
            } => {
                if !repository.is_some_and(|repository| {
                    self.open_activity_comparison(repository, reference, base, head, window, cx)
                }) {
                    self.show_activity(false, window, cx);
                    self.complete_notification_open(window, cx);
                }
            }
        }
    }
}
