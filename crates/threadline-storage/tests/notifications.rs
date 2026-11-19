use std::num::NonZeroU64;
use threadline_storage as storage;
#[path = "support/write_profile.rs"]
mod write_profile;
use storage::{access::AccessProfile, notifications::*, resources::DeliveryPhase, *};
fn facts(item: ItemId, category: Category) -> EventFacts {
    EventFacts {
        target: NotificationTarget::Item { item, thread: None },
        categories: [category].into(),
        actor: Default::default(),
        involves_me: false,
        receipt: None,
    }
}
fn record(
    db: &Store,
    p: &AccessProfile,
    r: RepositoryId,
    key: &str,
    ts: i64,
    facts: &EventFacts,
) -> bool {
    db.observe_notification_change(
        p,
        ObservedNotification {
            repository: r,
            stream: "comments",
            key,
            kind: "observed",
            occurred_at_ms: ts,
            received_at_ms: ts + 1,
            revision: None,
            facts,
        },
    )
    .unwrap()
}
#[test]
fn it_018_baselines_many_paths_group_by_exact_target_and_unknown_after_crash_is_never_resent() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, r, item) = write_profile::setup(&db);
    let p = db.github_access_profile(a, "work").unwrap().unwrap();
    let mut f = facts(item, Category::InlineComment);
    f.categories.insert(Category::DirectMention);
    assert!(!record(&db, &p, r, "old", 100, &f));
    assert!(
        db.enroll_notification_stream(&p, r, "comments", 110)
            .unwrap()
    );
    assert!(
        !db.enroll_notification_stream(&p, r, "comments", 120)
            .unwrap()
    );
    assert!(!record(&db, &p, r, "old", 110, &f));
    db.update_notification_settings(a, |s| s.fallback = DeliveryMode::Popup)
        .unwrap();
    for (k, ts) in [("first", 111), ("second", 112)] {
        let mut f = f.clone();
        f.target = NotificationTarget::Item {
            item,
            thread: Some(k.into()),
        };
        assert!(record(&db, &p, r, k, ts, &f));
        assert!(!record(&db, &p, r, k, ts, &f));
    }
    assert_eq!(
        db.process_notification_events(a, 115, "UTC", true, 1)
            .unwrap(),
        1
    );
    assert!(
        db.claim_notification_popup(a, 115, "UTC", true)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.process_notification_events(a, 116, "UTC", true, 100)
            .unwrap(),
        1
    );
    let group = db
        .claim_notification_popup(a, 120, "UTC", true)
        .unwrap()
        .unwrap();
    assert_eq!(group.event_count, 2);
    assert!(!group.sound);
    assert_eq!(
        group.destination.target,
        NotificationTarget::Item { item, thread: None }
    );
    let outsider = db.ensure_account("github.com", 999).unwrap();
    assert!(
        db.activate_notification(outsider, &group.native_id, NotificationAction::Open, 123)
            .unwrap()
            .is_none()
    );
    drop(db);
    let db = Store::open_profile(dir.path()).unwrap();
    assert!(
        db.claim_notification_popup(a, 125, "UTC", true)
            .unwrap()
            .is_none()
    );
    let feed = db.notification_feed(a, None, 100).unwrap();
    assert_eq!(feed.len(), 2);
    assert_eq!(
        db.load_delivery_state(a, &feed[0].event_key)
            .unwrap()
            .unwrap()
            .phase,
        DeliveryPhase::Unknown
    );
    assert!(
        db.record_notification_outcome(a, &group.native_id, PopupOutcome::Delivered, 130)
            .unwrap()
    );
    assert!(
        !db.record_notification_outcome(a, &group.native_id, PopupOutcome::Delivered, 131)
            .unwrap()
    );
    let activated = db
        .activate_notification(a, &group.native_id, NotificationAction::Open, 132)
        .unwrap()
        .unwrap();
    assert_eq!(
        activated.target,
        NotificationTarget::Item { item, thread: None }
    );
    assert!(
        db.activate_notification(a, &group.native_id, NotificationAction::MarkRead, 140)
            .unwrap()
            .is_none()
    );
    drop(db);
    let db = Store::open_profile(dir.path()).unwrap();
    assert_eq!(
        db.pending_notification_open(a).unwrap(),
        Some((group.native_id.clone(), activated))
    );
    assert!(db.pending_notification_open(outsider).unwrap().is_none());
    assert!(
        !db.complete_notification_open(outsider, &group.native_id)
            .unwrap()
    );
    assert!(db.complete_notification_open(a, &group.native_id).unwrap());
    assert!(db.pending_notification_open(a).unwrap().is_none());
    assert!(
        db.notification_feed(a, None, 100)
            .unwrap()
            .iter()
            .all(|e| e.read_at_ms == Some(132))
    );
    assert_eq!(
        db.load_delivery_state(a, &feed[0].event_key)
            .unwrap()
            .unwrap()
            .phase,
        DeliveryPhase::Delivered
    );
}
#[test]
fn it_018_delivery_rechecks_current_preferences_before_native_boundary_and_summarizes_offline_catchup()
 {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (a, r, item) = write_profile::setup(&db);
    let p = db.github_access_profile(a, "work").unwrap().unwrap();
    db.enroll_notification_stream(&p, r, "comments", 1).unwrap();
    db.update_notification_settings(a, |s| s.fallback = DeliveryMode::Popup)
        .unwrap();
    let f = facts(item, Category::OrdinaryComment);
    assert!(record(&db, &p, r, "first", 2, &f));
    db.process_notification_events(a, 3, "UTC", true, 100)
        .unwrap();
    db.update_repository_notification_rules(a, r, |rules| rules.muted = true)
        .unwrap();
    assert!(
        db.claim_notification_popup(a, 4, "UTC", true)
            .unwrap()
            .is_none()
    );
    assert!(db.notification_feed(a, None, 100).unwrap().is_empty());
    db.update_repository_notification_rules(a, r, |rules| rules.muted = false)
        .unwrap();
    for (idx, c) in Category::ALL.iter().copied().enumerate() {
        let mut f = facts(
            if idx % 2 == 0 {
                item
            } else {
                write_profile::issue(&db, a, r, 42)
            },
            c,
        );
        if c == Category::OrdinaryComment {
            f.actor.user_id = NonZeroU64::new(81);
        }
        assert!(record(&db, &p, r, &format!("category-{idx}"), 15, &f));
    }
    db.process_notification_events(a, 1_000_000, "UTC", true, 100)
        .unwrap();
    let digest = db
        .claim_notification_popup(a, 1_000_001, "UTC", true)
        .unwrap()
        .unwrap();
    assert_eq!(digest.event_count, 13);
    assert_eq!(digest.destination.target, NotificationTarget::ActivityFeed);
    assert_eq!(db.notification_feed(a, None, 50).unwrap().len(), 13);
    db.record_notification_outcome(a, &digest.native_id, PopupOutcome::Delivered, 1_000_002)
        .unwrap();
    db.activate_notification(
        a,
        &digest.native_id,
        NotificationAction::SnoozeUntil(2_000_000),
        1_000_003,
    )
    .unwrap();
    assert!(record(&db, &p, r, "now", 1_100_000, &f));
    db.process_notification_events(a, 1_100_001, "UTC", true, 100)
        .unwrap();
    assert!(
        db.claim_notification_popup(a, 1_100_002, "UTC", true)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.notification_feed(a, None, 1).unwrap()[0]
            .decision
            .reasons,
        [DecisionReason::Snoozed]
    );
    let wrong_repo = db.ensure_repository(a, 400, "work", "other").unwrap();
    assert!(
        db.follow_notification_target(a, wrong_repo, &f.target, true)
            .is_err()
    );
}

#[test]
fn it_018_popup_waits_for_separately_committed_pages_and_survives_restart() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (account, repo, item) = write_profile::setup(&db);
    let profile = db.github_access_profile(account, "work").unwrap().unwrap();
    let popup = PopupAvailability::Enabled;
    let at = 1_700_000_000_000;
    db.enroll_notification_stream(&profile, repo, "comments", at - 100)
        .unwrap();
    db.update_notification_settings(account, |settings| settings.fallback = DeliveryMode::Popup)
        .unwrap();

    let review = facts(item, Category::CommentOnlyReview);
    assert!(record(&db, &profile, repo, "submitted-review", at, &review));
    assert_eq!(
        db.process_notification_events(account, at, "UTC", popup, 10)
            .unwrap(),
        1
    );
    let deadline = db.next_notification_popup_at(account).unwrap().unwrap();
    assert!(deadline > at + 1_000);
    assert!(
        db.claim_ready_notification_popup(account, at, "UTC", popup)
            .unwrap()
            .is_none()
    );

    let mut inline = facts(item, Category::InlineComment);
    inline.target = NotificationTarget::Item {
        item,
        thread: Some("fixture-thread".into()),
    };
    assert!(record(
        &db,
        &profile,
        repo,
        "review-inline-note",
        at + 1_000,
        &inline
    ));
    assert_eq!(
        db.process_notification_events(account, at + 1_000, "UTC", popup, 10)
            .unwrap(),
        1
    );
    drop(db);

    let db = Store::open_profile(dir.path()).unwrap();
    assert_eq!(
        db.next_notification_popup_at(account).unwrap(),
        Some(deadline)
    );
    assert!(
        db.claim_ready_notification_popup(account, deadline - 1, "UTC", popup)
            .unwrap()
            .is_none()
    );
    let grouped = db
        .claim_ready_notification_popup(account, deadline, "UTC", popup)
        .unwrap()
        .unwrap();
    assert_eq!(grouped.event_count, 2);
    assert_eq!(
        grouped.destination.target,
        NotificationTarget::Item { item, thread: None }
    );
    assert_eq!(db.next_notification_popup_at(account).unwrap(), None);
    assert!(
        db.claim_ready_notification_popup(account, deadline + 1, "UTC", popup)
            .unwrap()
            .is_none()
    );
}

#[test]
fn it_018_contextual_review_muting_uses_provider_identity_and_preserves_existing_activity() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (account, repository, item) = write_profile::setup(&db);
    let profile = db.github_access_profile(account, "work").unwrap().unwrap();
    let user = NonZeroU64::new(721).unwrap();
    let app = NonZeroU64::new(951).unwrap();
    db.enroll_notification_stream(&profile, repository, "comments", 100)
        .unwrap();
    let observe = |key, ts, category, actor| {
        let mut f = facts(item, category);
        f.actor = actor;
        assert!(record(&db, &profile, repository, key, ts, &f));
    };
    let identity = EventActor {
        user_id: Some(user),
        ..Default::default()
    };
    observe("review-first", 101, Category::Approval, identity.clone());
    observe(
        "ordinary-first",
        102,
        Category::OrdinaryComment,
        identity.clone(),
    );
    observe(
        "review-unidentified",
        103,
        Category::Approval,
        EventActor::default(),
    );
    observe(
        "review-app",
        104,
        Category::CommentOnlyReview,
        EventActor {
            app_id: Some(app),
            ..Default::default()
        },
    );
    assert_eq!(
        db.process_notification_events(account, 110, "UTC", false, 10)
            .unwrap(),
        4
    );
    let feed = db.notification_feed(account, None, 10).unwrap();
    let event_key = |key| format!("observed:{}:comments:{key}", repository.0);
    let review_actor = |key| {
        feed.iter()
            .find(|row| row.event_key == event_key(key))
            .unwrap()
            .review_actor
            .clone()
    };
    assert_eq!(review_actor("review-first"), Some(RuleActor::User(user)));
    assert_eq!(review_actor("ordinary-first"), None);
    assert_eq!(review_actor("review-unidentified"), None);
    assert_eq!(review_actor("review-app"), Some(RuleActor::App(app)));
    db.update_repository_notification_rules(account, repository, |policy| {
        policy.actor_rules.push(ActorRule {
            actor: review_actor("review-first").unwrap(),
            scope: RuleScope::ReviewsAndInline,
        })
    })
    .unwrap();
    observe("review-later", 120, Category::Approval, identity.clone());
    observe(
        "ordinary-later",
        121,
        Category::OrdinaryComment,
        identity.clone(),
    );
    observe(
        "other-review-later",
        122,
        Category::Approval,
        EventActor {
            user_id: NonZeroU64::new(723),
            ..Default::default()
        },
    );
    assert_eq!(
        db.process_notification_events(account, 130, "UTC", false, 10)
            .unwrap(),
        3
    );
    let latest = db.notification_feed(account, None, 10).unwrap();
    let contains = |key| latest.iter().any(|e| e.event_key == event_key(key));
    assert!(
        contains("review-first") && contains("ordinary-later") && contains("other-review-later")
    );
    assert!(!contains("review-later"));
    let outsider = db.ensure_account("github.com", 3129).unwrap();
    assert!(db.notification_feed(outsider, None, 10).unwrap().is_empty());
    assert!(
        db.repository_notification_rules(outsider, repository)
            .is_err()
    );
}
