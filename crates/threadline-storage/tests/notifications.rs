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
