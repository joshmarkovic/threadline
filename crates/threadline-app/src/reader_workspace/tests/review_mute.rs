use super::*;
use threadline_storage::{
    notifications::{EventActor, RuleActor, RuleScope},
    pr::{PrFamily, PrHeader, PrInputPage, PrRecord, PrSection, PrTabView},
    resources::WalkMode,
};

#[gpui_kit::test]
fn ui_007_preview_review_author_mute_and_keep_the_downloaded_review(cx: &mut TestAppContext) {
    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    let repo = setup.fixture.repository;
    let item = setup.fixture.reviewed_pr;
    let stamp = 1_900_000_000_000;
    let oid =
        |digit: &str| threadline_storage::resources::CommitOid::parse(&digit.repeat(40)).unwrap();
    let rev = setup
        .store
        .record_pr_revision(
            account,
            item,
            &oid("a"),
            &oid("b"),
            None,
            &PrHeader::default(),
            stamp,
        )
        .unwrap();
    let actor_id = NonZeroU64::new(72).unwrap();
    let review = |id, actor| PrRecord::Review {
        id,
        node_id: None,
        author: Some("reviewer".into()),
        actor,
        body: "This downloaded review stays on the PR.".into(),
        state: "APPROVED".into(),
        submitted_at_ms: Some(stamp),
        commit_oid: None,
    };
    let records = vec![
        review(
            61,
            EventActor {
                user_id: Some(actor_id),
                ..Default::default()
            },
        ),
        review(62, EventActor::default()),
    ];
    let fetch = setup
        .store
        .start_pr_fetch(&rev, PrFamily::Reviews, WalkMode::Full)
        .unwrap();
    setup
        .store
        .commit_pr_page(
            &rev,
            PrFamily::Reviews,
            &fetch,
            PrInputPage {
                records: records.clone(),
                next_cursor: None,
                validators: None,
                partial: None,
                fetched_at_ms: stamp,
            },
        )
        .unwrap();
    setup.one_pane(&Tab {
        id: "review-account-mute".into(),
        item,
        view: TabView {
            pr_reader: Some(PrTabView {
                section: PrSection::Conversation,
                ..Default::default()
            }),
            ..Default::default()
        },
    });
    let (window, reader) = setup.open(cx);
    let control = "pr-0-mute-review-61";
    until(cx, |cx| {
        cx.read(|cx| {
            let current = reader.read(cx);
            current.surface == Surface::Browse
                && current
                    .tab_views
                    .get("review-account-mute")
                    .and_then(|tab| tab.pr.as_ref())
                    .is_some_and(|view| view.data.revision == rev)
        })
    });
    cx.update_window(window, |_, window, _| {
        assert!(window.try_find("pr-0-mute-review-62").is_none());
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let current = reader.read(cx);
        assert_eq!(current.account, Some(account));
        assert!(
            current
                .repositories
                .iter()
                .any(|r| r.selected && r.id == repo)
        );
        assert!(!current.activity.busy);
        let bounds = window.find(control).bounds();
        window.click(control, cx);
        assert!(
            reader.read(cx).surface == Surface::NotificationSettings,
            "review mute button at {bounds:?} did not open settings"
        );
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).activity.scope_is_ready(Some(repo)))
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let preview = window.find("notifications-mute-preview");
        let label = preview.label().unwrap();
        assert!(
            label.contains("GitHub user #72 in fixture/relay")
                && label.contains("inline comments")
                && label.contains("PR still shows")
        );
    })
    .unwrap();
    assert!(
        setup
            .store
            .repository_notification_rules(account, repo)
            .unwrap()
            .actor_rules
            .is_empty()
    );
    click(cx, window, "notifications-dismiss-mute-review");
    assert!(
        setup
            .store
            .repository_notification_rules(account, repo)
            .unwrap()
            .actor_rules
            .is_empty()
    );
    click(cx, window, "activity-back");
    until(cx, |cx| {
        cx.read(|cx| {
            let current = reader.read(cx);
            current.surface == Surface::Browse
                && current
                    .tab_views
                    .get("review-account-mute")
                    .and_then(|tab| tab.pr.as_ref())
                    .is_some_and(|view| view.data.revision == rev)
        })
    });
    click(cx, window, control);
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).activity.scope_is_ready(Some(repo)))
    });
    click(cx, window, "notifications-confirm-mute-review");
    until(cx, |_| {
        !setup
            .store
            .repository_notification_rules(account, repo)
            .unwrap()
            .actor_rules
            .is_empty()
    });
    let rules = setup
        .store
        .repository_notification_rules(account, repo)
        .unwrap()
        .actor_rules;
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].scope, RuleScope::ReviewsAndInline);
    assert_eq!(rules[0].actor, RuleActor::User(actor_id));
    assert!(
        setup
            .store
            .notification_settings(account)
            .unwrap()
            .actor_rules
            .is_empty()
    );
    let retained: Vec<_> = setup
        .store
        .pr_page(&rev, PrFamily::Reviews, None, 10)
        .unwrap()
        .rows
        .into_iter()
        .map(|(_, record)| record)
        .collect();
    assert_eq!(retained, records);
}
