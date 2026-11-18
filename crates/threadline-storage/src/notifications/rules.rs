use crate::{Error, ItemId, RepositoryId, Result, resources::CommitOid};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU64;
use threadline_core::sync::policy::WorkSchedule;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum Category {
    BranchUpdate,
    OrdinaryComment,
    InlineComment,
    ReviewRequest,
    Approval,
    ChangesRequested,
    CommentOnlyReview,
    PrOpened,
    PrReady,
    PrMerged,
    IssueAssignment,
    IssueState,
    DirectMention,
    CheckStatus,
}
impl Category {
    pub const ALL: [Self; 14] = [
        Self::BranchUpdate,
        Self::OrdinaryComment,
        Self::InlineComment,
        Self::ReviewRequest,
        Self::Approval,
        Self::ChangesRequested,
        Self::CommentOnlyReview,
        Self::PrOpened,
        Self::PrReady,
        Self::PrMerged,
        Self::IssueAssignment,
        Self::IssueState,
        Self::DirectMention,
        Self::CheckStatus,
    ];
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum DeliveryMode {
    Off,
    #[default]
    FeedOnly,
    Popup,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum Relevance {
    #[default]
    All,
    InvolvingMe,
    Followed,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RuleActor {
    User(NonZeroU64),
    App(NonZeroU64),
    KnownAutomation,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RuleScope {
    Categories(BTreeSet<Category>),
    ReviewsAndInline,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActorRule {
    pub actor: RuleActor,
    pub scope: RuleScope,
}
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepositoryRules {
    pub muted: bool,
    pub categories: BTreeMap<Category, DeliveryMode>,
    pub relevance: Option<Relevance>,
    pub actor_rules: Vec<ActorRule>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct QuietHours {
    pub weekdays: u8,
    pub start_minute: u16,
    pub end_minute: u16,
    pub timezone: Option<String>,
}
impl QuietHours {
    pub(super) fn schedule(&self) -> WorkSchedule {
        WorkSchedule {
            weekdays: self.weekdays,
            start_minute: self.start_minute,
            end_minute: self.end_minute,
            timezone: self.timezone.clone(),
            ..Default::default()
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub fallback: DeliveryMode,
    pub categories: BTreeMap<Category, DeliveryMode>,
    pub relevance: Relevance,
    pub actor_rules: Vec<ActorRule>,
    pub mention_exception: bool,
    pub suppress_own: bool,
    pub sound: bool,
    pub quiet_hours: Option<QuietHours>,
}
impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            fallback: DeliveryMode::FeedOnly,
            categories: BTreeMap::new(),
            relevance: Relevance::All,
            actor_rules: vec![],
            mention_exception: false,
            suppress_own: true,
            sound: false,
            quiet_hours: None,
        }
    }
}
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventActor {
    pub user_id: Option<NonZeroU64>,
    pub app_id: Option<NonZeroU64>,
    pub known_automation: bool,
}
impl RuleActor {
    fn matches(&self, who: &EventActor) -> bool {
        match self {
            Self::User(id) => who.user_id == Some(*id),
            Self::App(id) => who.app_id == Some(*id),
            Self::KnownAutomation => who.known_automation || who.app_id.is_some(),
        }
    }
}
impl RuleScope {
    fn matches(&self, paths: &BTreeSet<Category>) -> bool {
        match self {
            Self::Categories(spec) => !spec.is_disjoint(paths),
            Self::ReviewsAndInline => paths.iter().any(|c| {
                matches!(
                    c,
                    Category::Approval
                        | Category::ChangesRequested
                        | Category::CommentOnlyReview
                        | Category::InlineComment
                )
            }),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum NotificationTarget {
    Item {
        item: ItemId,
        thread: Option<String>,
    },
    Comparison {
        reference: String,
        base: CommitOid,
        head: CommitOid,
    },
    Repository,
    ActivityFeed,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventFacts {
    pub target: NotificationTarget,
    pub categories: BTreeSet<Category>,
    pub actor: EventActor,
    pub involves_me: bool,
    pub receipt: Option<ReceiptMatch>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReceiptMatch {
    Ordinary {
        action: String,
        remote_id: String,
        revision: String,
    },
    Review {
        step: String,
        remote_id: String,
        head: CommitOid,
    },
    Merge {
        head: CommitOid,
        merge_oid: CommitOid,
    },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeliveryContext {
    pub own: bool,
    pub followed: bool,
    pub snoozed: bool,
    pub quiet: bool,
    pub popup: PopupAvailability,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PopupAvailability {
    Enabled,
    SenderSettingUnknown,
    Disabled,
    RegistrationMissing,
    Unavailable,
}

impl From<bool> for PopupAvailability {
    fn from(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DecisionReason {
    GlobalDisabled,
    RepositoryMuted,
    RepositoryUnavailable,
    Irrelevant,
    OwnAction,
    ActorSuppressed,
    AllCategoriesOff,
    MentionException,
    QuietHours,
    Snoozed,
    OperatingSystem,
    SenderSettingUnknown,
    RegistrationMissing,
    AdapterUnavailable,
    Enabled,
}
impl DecisionReason {
    pub fn description(self) -> &'static str {
        match self {
            Self::GlobalDisabled => "Notifications are turned off.",
            Self::RepositoryUnavailable => {
                "The repository is outside the authorized monitoring scope."
            }
            Self::RepositoryMuted => "The repository is muted.",
            Self::Irrelevant => "The activity is outside the selected relevance filter.",
            Self::OwnAction => "Your own action is suppressed.",
            Self::ActorSuppressed => "An actor rule suppresses this activity.",
            Self::AllCategoriesOff => "All matching notification categories are off.",
            Self::MentionException => {
                "The enabled direct mention is allowed by an explicit actor-rule exception."
            }
            Self::QuietHours => "Quiet hours keep this activity in the feed.",
            Self::Snoozed => "This target is snoozed.",
            Self::OperatingSystem => "Desktop notifications are disabled in the operating system.",
            Self::SenderSettingUnknown => {
                "Windows returned no preference for this registered sender. Windows evaluates the submitted notification under its current policy."
            }
            Self::RegistrationMissing => {
                "Windows could not match this application to its registered Start shortcut and notification activator."
            }
            Self::AdapterUnavailable => "The desktop notification service was unavailable.",
            Self::Enabled => "An enabled category permits this delivery.",
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub mode: DeliveryMode,
    pub eligible: BTreeSet<Category>,
    pub reasons: Vec<DecisionReason>,
    pub sound: bool,
}
pub fn preview(
    settings: &NotificationSettings,
    repo: &RepositoryRules,
    event: &EventFacts,
    ctx: DeliveryContext,
) -> Decision {
    use DecisionReason::*;
    let off = |r| Decision {
        mode: DeliveryMode::Off,
        eligible: BTreeSet::new(),
        reasons: vec![r],
        sound: false,
    };
    if !settings.enabled {
        return off(GlobalDisabled);
    }
    if repo.muted {
        return off(RepositoryMuted);
    }
    let relevance = repo.relevance.unwrap_or(settings.relevance);
    if match relevance {
        Relevance::All => false,
        Relevance::Followed => !ctx.followed,
        Relevance::InvolvingMe => {
            !event.involves_me && !event.categories.contains(&Category::DirectMention)
        }
    } {
        return off(Irrelevant);
    }
    if settings.suppress_own && ctx.own {
        return off(OwnAction);
    }
    let mode = |c: &Category| {
        *repo
            .categories
            .get(c)
            .or_else(|| settings.categories.get(c))
            .unwrap_or(&settings.fallback)
    };
    let mut eligible: BTreeSet<_> = event
        .categories
        .iter()
        .copied()
        .filter(|c| mode(c) != DeliveryMode::Off)
        .collect();
    if eligible.is_empty() {
        return off(AllCategoriesOff);
    }
    let matches = settings
        .actor_rules
        .iter()
        .chain(&repo.actor_rules)
        .any(|r| r.actor.matches(&event.actor) && r.scope.matches(&event.categories));
    let mut reasons = vec![];
    if matches {
        if settings.mention_exception && eligible.contains(&Category::DirectMention) {
            eligible.retain(|c| *c == Category::DirectMention);
            reasons.push(MentionException);
        } else {
            return off(ActorSuppressed);
        }
    }
    let mut result = eligible.iter().map(mode).max().unwrap_or_default();
    if result == DeliveryMode::Popup {
        if ctx.snoozed {
            reasons.push(Snoozed)
        }
        if ctx.quiet {
            reasons.push(QuietHours)
        }
        match ctx.popup {
            PopupAvailability::Enabled => (),
            PopupAvailability::SenderSettingUnknown => reasons.push(SenderSettingUnknown),
            PopupAvailability::Disabled => reasons.push(OperatingSystem),
            PopupAvailability::RegistrationMissing => reasons.push(RegistrationMissing),
            PopupAvailability::Unavailable => reasons.push(AdapterUnavailable),
        }
        let can_submit = matches!(
            ctx.popup,
            PopupAvailability::Enabled | PopupAvailability::SenderSettingUnknown
        );
        if ctx.snoozed || ctx.quiet || !can_submit {
            result = DeliveryMode::FeedOnly
        }
    }
    if reasons.is_empty() {
        reasons.push(Enabled)
    }
    Decision {
        mode: result,
        eligible,
        reasons,
        sound: result == DeliveryMode::Popup && settings.sound,
    }
}

pub(super) fn group_key(repository: RepositoryId, target: &NotificationTarget) -> Result<String> {
    match target {
        NotificationTarget::Item { item, .. } => {
            Ok(format!("repo/{}/item/{}", repository.0, item.0))
        }
        NotificationTarget::Comparison { reference, .. } => {
            Ok(format!("repo/{}/ref/{reference}", repository.0))
        }
        NotificationTarget::Repository => Ok(format!("repo/{}", repository.0)),
        NotificationTarget::ActivityFeed => Err(Error::InvalidInput(
            "A provider event requires a repository target",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ut_014_categories_actors_mentions_own_actions_and_desktop_restrictions_apply_in_order() {
        let actor = NonZeroU64::new(62).unwrap();
        let mut s = NotificationSettings {
            fallback: DeliveryMode::Off,
            ..Default::default()
        };
        s.categories
            .insert(Category::DirectMention, DeliveryMode::Popup);
        let mut r = RepositoryRules::default();
        let mut ctx = DeliveryContext {
            own: false,
            followed: false,
            quiet: false,
            snoozed: false,
            popup: PopupAvailability::Enabled,
        };
        let mut e = EventFacts {
            target: NotificationTarget::Repository,
            categories: [Category::InlineComment, Category::DirectMention].into(),
            actor: EventActor {
                user_id: Some(actor),
                ..Default::default()
            },
            involves_me: true,
            receipt: None,
        };
        assert_eq!(preview(&s, &r, &e, ctx).mode, DeliveryMode::Popup);
        let uninitialized = preview(
            &s,
            &r,
            &e,
            DeliveryContext {
                popup: PopupAvailability::SenderSettingUnknown,
                ..ctx
            },
        );
        assert_eq!(uninitialized.mode, DeliveryMode::Popup);
        assert_eq!(
            uninitialized.reasons,
            [DecisionReason::SenderSettingUnknown]
        );
        for (popup, reason) in [
            (PopupAvailability::Disabled, DecisionReason::OperatingSystem),
            (
                PopupAvailability::RegistrationMissing,
                DecisionReason::RegistrationMissing,
            ),
            (
                PopupAvailability::Unavailable,
                DecisionReason::AdapterUnavailable,
            ),
        ] {
            let decision = preview(&s, &r, &e, DeliveryContext { popup, ..ctx });
            assert_eq!(decision.mode, DeliveryMode::FeedOnly);
            assert_eq!(decision.reasons, [reason]);
            assert!(!decision.sound);
        }
        r.actor_rules.push(ActorRule {
            actor: RuleActor::User(actor),
            scope: RuleScope::ReviewsAndInline,
        });
        assert_eq!(
            preview(&s, &r, &e, ctx).reasons,
            [DecisionReason::ActorSuppressed]
        );
        s.mention_exception = true;
        assert_eq!(
            preview(&s, &r, &e, ctx).eligible,
            [Category::DirectMention].into()
        );
        ctx.quiet = true;
        ctx.snoozed = true;
        let quiet = preview(&s, &r, &e, ctx);
        assert_eq!(quiet.mode, DeliveryMode::FeedOnly);
        assert!(!quiet.sound);
        assert_eq!(
            quiet.reasons,
            [
                DecisionReason::MentionException,
                DecisionReason::Snoozed,
                DecisionReason::QuietHours
            ]
        );
        ctx.own = true;
        assert_eq!(
            preview(&s, &r, &e, ctx).reasons,
            [DecisionReason::OwnAction]
        );
        r.muted = true;
        assert_eq!(
            preview(&s, &r, &e, ctx).reasons,
            [DecisionReason::RepositoryMuted]
        );
        s.enabled = false;
        assert_eq!(
            preview(&s, &r, &e, ctx).reasons,
            [DecisionReason::GlobalDisabled]
        );
        s.enabled = true;
        r.muted = false;
        ctx.own = false;
        ctx.quiet = false;
        ctx.snoozed = false;
        r.categories
            .insert(Category::DirectMention, DeliveryMode::Off);
        assert_eq!(preview(&s, &r, &e, ctx).mode, DeliveryMode::Off);
        for cat in [
            Category::Approval,
            Category::ChangesRequested,
            Category::CommentOnlyReview,
            Category::InlineComment,
            Category::OrdinaryComment,
        ] {
            e.categories = [cat].into();
            r.categories.insert(cat, DeliveryMode::Popup);
            let outcome = preview(&s, &r, &e, ctx);
            assert_eq!(
                outcome.mode,
                if cat == Category::OrdinaryComment {
                    DeliveryMode::Popup
                } else {
                    DeliveryMode::Off
                }
            );
            let mut human = e.clone();
            human.actor.user_id = NonZeroU64::new(63);
            assert_eq!(preview(&s, &r, &human, ctx).mode, DeliveryMode::Popup);
        }
    }
}
