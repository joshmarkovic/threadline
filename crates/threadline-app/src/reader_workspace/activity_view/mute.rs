use super::*;

#[derive(Clone)]
pub(super) struct MutePreview {
    account: AccountId,
    repository: RepositoryId,
    actor: RuleActor,
}

impl ReaderWorkspace {
    pub(in crate::reader_workspace) fn review_mute_control(
        &self,
        id: String,
        account: AccountId,
        repository: RepositoryId,
        actor: RuleActor,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let available = self.account == Some(account)
            && self
                .repositories
                .iter()
                .any(|r| r.id == repository && r.selected);
        control(id, "Mute review activity from this account")
            .disabled(self.activity.busy || !available)
            .on_click(cx.listener(move |this, _, window, cx| {
                if this.activity.busy
                    || this.account != Some(account)
                    || !this
                        .repositories
                        .iter()
                        .any(|repo| repo.id == repository && repo.selected)
                {
                    return;
                }
                this.surface = Surface::NotificationSettings;
                this.load_activity(Some(repository), None, window, cx);
                this.activity.mute_preview = Some(MutePreview {
                    account,
                    repository,
                    actor: actor.clone(),
                });
                cx.notify();
            }))
    }

    pub(super) fn review_mute_preview(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let preview = self.activity.mute_preview.as_ref()?;
        let repo = self.repositories.iter().find(|repo| {
            repo.selected && repo.id == preview.repository && self.account == Some(preview.account)
        })?;
        if self.activity.repository != Some(repo.id) {
            return None;
        }
        let settings = self.activity.settings.as_ref()?;
        let overrides = self.activity.overrides.as_ref()?;
        let actor = match preview.actor {
            RuleActor::User(id) => format!("GitHub user #{id}"),
            RuleActor::App(id) => format!("GitHub App #{id}"),
            RuleActor::KnownAutomation => return None,
        };
        let rule = ActorRule {
            actor: preview.actor.clone(),
            scope: RuleScope::ReviewsAndInline,
        };
        let exists = settings.actor_rules.contains(&rule) || overrides.actor_rules.contains(&rule);
        let mut description = format!(
            "Mute notifications from {actor} in {}/{} for approvals, requested changes, comment-only reviews and inline comments. The PR still shows the reviews and comments.",
            repo.owner, repo.name
        );
        if settings.mention_exception {
            description.push_str(" Enabled direct mentions can still notify you.");
        }
        if exists {
            description.push_str(" This rule already applies.");
        }
        let account = preview.account;
        let repository = preview.repository;
        let mut panel = div()
            .id("notifications-mute-preview")
            .test_support()
            .role(Role::Group)
            .aria_label(description.clone())
            .flex()
            .flex_col()
            .gap_1()
            .child(description);
        if !exists {
            panel = panel.child(
                control("notifications-confirm-mute-review", "Apply review mute")
                    .disabled(self.activity.busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.account == Some(account)
                            && this.activity.mute_preview.as_ref().is_some_and(|current| {
                                current.account == account
                                    && current.repository == repository
                                    && current.actor == rule.actor
                            })
                        {
                            this.save_notification_preference(
                                Some(repository),
                                Setting::AddReviewMute(rule.clone()),
                                window,
                                cx,
                            );
                        }
                    })),
            );
        }
        Some(
            panel
                .child(
                    control("notifications-dismiss-mute-review", "Dismiss preview")
                        .disabled(self.activity.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.activity.mute_preview = None;
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }
}
