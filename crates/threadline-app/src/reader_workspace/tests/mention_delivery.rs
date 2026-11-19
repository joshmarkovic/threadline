use super::*;
use threadline_storage::{
    access::{AccessState, DiscoveryState, RepositoryObservation, RepositoryObservationSource},
    notifications::{
        ActorRule, Category, DecisionReason, DeliveryMode, Destination, EventActor,
        NotificationTarget, RuleActor, RuleScope,
    },
    reader::DiscussionComment,
    resources::{DeliveryPhase, ResourceFamily, Validators},
    sync::{ObservationPage, ObservedComment, ReadPriority, ReadTarget},
};

const INTERVAL: i64 = 3_600_000;

fn enable_fixture_reader(setup: &ReaderHarness, start: i64) -> NonZeroU64 {
    let store = &setup.store;
    let previous = store.github_access_profiles().unwrap().pop().unwrap();
    let profile = store
        .reserve_access_profile_credential(
            previous.account,
            previous.user_id,
            "reader",
            &previous.owner_scope,
            previous.kind,
            Some(&previous),
        )
        .unwrap();
    store
        .install_current_credential(&profile, || Ok::<_, ()>(()))
        .unwrap()
        .unwrap();
    store
        .record_access_result(
            &profile,
            profile.kind,
            AccessState::Readable,
            DiscoveryState::Complete,
            start,
        )
        .unwrap();
    let repo = store
        .observe_github_repository(
            &profile,
            &RepositoryObservation {
                provider_id: NonZeroU64::new(1).unwrap(),
                owner: &profile.owner_scope,
                name: "relay",
                private: true,
                source: RepositoryObservationSource::Direct,
            },
            start,
        )
        .unwrap();
    assert_eq!(repo, setup.fixture.repository);
    store
        .set_dispatch_controls(profile.account, false, true)
        .unwrap();
    profile.user_id
}

#[gpui_kit::test]
fn ui_007_mention_delivery_respects_each_override_and_retains_the_comments(
    cx: &mut TestAppContext,
) {
    use DecisionReason::*;
    let setup = ReaderHarness::seeded();
    let store = &setup.store;
    let account = setup.fixture.account;
    let repo = setup.fixture.repository;
    let item = setup.fixture.long_discussion;
    let start = UtcMillis::now().unwrap().0 - 10 * INTERVAL;
    let me = enable_fixture_reader(&setup, start);
    let other = NonZeroU64::new(me.get().wrapping_add(1)).unwrap_or(NonZeroU64::MIN);
    let target = ReadTarget {
        repository: repo,
        item: None,
        family: ResourceFamily::Comments,
        revision: String::new(),
    };
    store
        .subscribe_read(account, &target, "activity", ReadPriority::Activity, start)
        .unwrap();
    let publish = |at, comments| {
        let claim = store
            .claim_sync_read(account, at)
            .unwrap()
            .expect("due comment feed");
        assert_eq!(claim.target, target);
        store
            .commit_observed_page(
                &claim,
                ObservationPage {
                    comments,
                    items: vec![],
                    next_cursor: None,
                    validators: Validators::default(),
                },
                at,
                "UTC",
            )
            .unwrap();
    };
    publish(start, vec![]);
    let capture = tempfile::NamedTempFile::new().unwrap();
    let mut expected_deliveries = Vec::new();
    let mut bodies = Vec::new();
    let item_target = NotificationTarget::Item { item, thread: None };
    let scenarios = [
        AllCategoriesOff,
        Enabled,
        ActorSuppressed,
        MentionException,
        OwnAction,
        RepositoryMuted,
        Snoozed,
        DecisionReason::QuietHours,
    ];

    for (index, reason) in scenarios.into_iter().enumerate() {
        let id = 8100 + index as u64;
        let at = start + (index as i64 + 1) * INTERVAL;
        let actor = if reason == OwnAction { me } else { other };
        let body = if reason == AllCategoriesOff {
            "Ordinary comment without a mention.".to_string()
        } else {
            format!("Comment {id} mentions @reader.")
        };
        bodies.push(body.clone());
        let muted_actor = !matches!(reason, AllCategoriesOff | Enabled);
        store
            .update_repository_notification_rules(account, repo, |rules| {
                rules.muted = reason == RepositoryMuted;
                rules.actor_rules = if muted_actor {
                    vec![ActorRule {
                        actor: RuleActor::User(actor),
                        scope: RuleScope::Categories([Category::OrdinaryComment].into()),
                    }]
                } else {
                    vec![]
                };
            })
            .unwrap();
        store
            .update_notification_settings(account, |settings| {
                settings.fallback = DeliveryMode::Off;
                settings
                    .categories
                    .insert(Category::OrdinaryComment, DeliveryMode::Off);
                settings
                    .categories
                    .insert(Category::DirectMention, DeliveryMode::Popup);
                settings.mention_exception = muted_actor && reason != ActorSuppressed;
                settings.sound = true;
                let minute = ((at / 60_000) % 1440) as u16;
                settings.quiet_hours = (reason == DecisionReason::QuietHours).then(|| {
                    threadline_storage::notifications::QuietHours {
                        weekdays: 127,
                        start_minute: minute,
                        end_minute: (minute + 10) % 1440,
                        timezone: Some("UTC".into()),
                    }
                });
            })
            .unwrap();
        store
            .snooze_notification_target(
                account,
                repo,
                &item_target,
                (reason == Snoozed).then_some(at + INTERVAL),
            )
            .unwrap();
        publish(
            at,
            vec![ObservedComment {
                number: 1,
                actor: EventActor {
                    user_id: Some(actor),
                    ..Default::default()
                },
                comment: DiscussionComment {
                    provider_id: id,
                    node_id: None,
                    author: Some("fixture".into()),
                    created_at_ms: at - 1,
                    updated_at_ms: at - 1,
                    body,
                },
            }],
        );
        assert_eq!(
            store
                .process_notification_events(account, at, "UTC", true, 128)
                .unwrap(),
            1
        );
        let key = format!("github:comments:{id}:comment_created:{}", at - 1);
        let entry = store
            .notification_feed(account, None, 20)
            .unwrap()
            .into_iter()
            .find(|row| row.event_key == key);
        let delivered = matches!(reason, Enabled | MentionException);
        if delivered || matches!(reason, Snoozed | DecisionReason::QuietHours) {
            let entry = entry.unwrap();
            assert_eq!(entry.decision.eligible, [Category::DirectMention].into());
            assert!(entry.decision.reasons.contains(&reason));
            assert_eq!(
                entry.decision.mode,
                if delivered {
                    DeliveryMode::Popup
                } else {
                    DeliveryMode::FeedOnly
                }
            );
            assert_eq!(entry.decision.sound, delivered);
        } else {
            assert!(entry.is_none());
        }
        // Route an already-adjudicated group through the app and its capture adapter after the collection deadline.
        crate::activity_delivery::deliver_to_test_capture(store, &capture);
        let state = store.load_delivery_state(account, &key).unwrap().unwrap();
        if delivered {
            assert_eq!(state.phase, DeliveryPhase::Delivered);
            expected_deliveries.push(serde_json::json!({
                "events": 1, "sound": true, "destination": Destination {
                    repository: Some(repo), target: NotificationTarget::Item {
                        item, thread: Some(format!("issue-comment:{id}")),
                    },
                },
            }));
        } else {
            assert_eq!(state.phase, DeliveryPhase::Suppressed);
            assert!(state.reason.unwrap().contains(reason.description()));
        }
    }
    crate::activity_delivery::deliver_to_test_capture(store, &capture);
    let captured = std::fs::read_to_string(capture.path()).unwrap();
    let deliveries: Vec<serde_json::Value> = captured
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(deliveries, expected_deliveries);
    assert_eq!(deliveries.len(), 2);

    setup.one_pane(&Tab {
        id: "mention-discussion".into(),
        item,
        view: TabView::default(),
    });
    let (window, reader) = setup.open(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let tab = &reader.read(cx).tab_views["mention-discussion"];
        for body in &bodies {
            let row = tab
                .data
                .discussion
                .records
                .iter()
                .find(|row| &row.body == body)
                .expect("downloaded comment");
            assert!(tab.notes.iter().any(|(key, _)| key == &row.key));
        }
    })
    .unwrap();
}
