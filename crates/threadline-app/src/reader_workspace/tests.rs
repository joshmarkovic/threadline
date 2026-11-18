use super::*;
use gpui_kit::{
    AppContext, Bounds, Point, TestAppContext, WindowBounds, WindowOptions, size,
    test::TestWindowExt,
};
use threadline_storage::{
    IssueClassification, IssueRecord, ItemRepresentation, ItemUpdate, UtcMillis,
    fixtures::{Scale, populate_reader},
    queue::DraftKind,
};

#[gpui_kit::test]
fn cached_markdown_and_native_editor_survive_immediate_orderly_exit(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let profile = tempfile::tempdir().unwrap();
    let path = profile.path().join("reader.sqlite");
    let store = Arc::new(Store::open(&path).unwrap());
    let fixture = populate_reader(&store, 12345, Scale::Small).unwrap();
    let account = fixture.account;
    let item = store
        .item(account, fixture.long_discussion)
        .unwrap()
        .unwrap();
    let initial_body = "Issue text. ![Fixture attachment](https://github.com/user-attachments/assets/missing-fixture)";
    store
        .ingest(
            account,
            item.locator,
            ItemUpdate {
                representation: ItemRepresentation::Issue(IssueRecord {
                    id: item.issue_id.unwrap(),
                    node_id: item.issue_node_id,
                    classification: IssueClassification::Issue,
                }),
                remote_updated_at: UtcMillis(1_900_000_000_000),
                title: Change::Missing,
                body: Change::Value(initial_body.into()),
                closed: Change::Missing,
            },
        )
        .unwrap();
    let tab = Tab {
        id: "reader-test-editor".into(),
        item: item.id,
        view: TabView::default(),
    };
    let original_projections = store.projections(account, item.id).unwrap();
    let mut layout = empty_workspace();
    layout.panes[0].active = Some(0);
    layout.panes[0].tabs.push(tab.clone());
    store.save_workspace(account, &layout).unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let handle = runtime.handle().clone();
    let repos = store.github_repository_views(account).unwrap();
    let profiles = store.github_access_profiles().unwrap();
    let (window, reader) = cx.update(|cx| {
        gpui_kit::init(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(1360.), px(1040.)),
            })),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| {
                let mut reader = ReaderWorkspace::new(store.clone(), handle, None, window, cx);
                reader.set_account(
                    ReaderAccount {
                        account: Some(account),
                        profile: profiles.first().cloned(),
                        offline: true,
                        reachability: Reachability::NotAttempted,
                        repositories: repos,
                    },
                    window,
                    cx,
                );
                reader
            })
        })
        .unwrap()
    });
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).tab_views.contains_key(&tab.id))
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("reader-attachment-0").is_some());
        window.click("reader-attachment-0", cx);
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| {
            reader
                .read(cx)
                .images
                .values()
                .any(|state| matches!(state, media::ImageState::Missing))
        })
    });
    cx.update_window(window, |_, window, cx| {
        reader.update(cx, |reader, cx| {
            reader.edit(tab.clone(), DraftKind::IssueEdit, window, cx)
        });
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| {
            let reader = reader.read(cx);
            !reader.layout_busy && reader.drafts.contains_key(&format!("issue-{}", item.id.0))
        })
    });
    let (departed, wait) = std::sync::mpsc::channel();
    cx.update_window(window, |_, window, cx| {
        let editor = reader
            .read(cx)
            .drafts
            .get(&format!("issue-{}", item.id.0))
            .unwrap()
            .view
            .clone();
        editor.focus_handle(cx).focus(window, cx);
        window.render_frame(cx);
        window.press("secondary-a", cx);
        window.input("Latest Markdown **before exit**", cx);
        assert_eq!(
            editor.read(cx).value().as_ref(),
            "Latest Markdown **before exit**"
        );
        reader.update(cx, |reader, cx| {
            reader.request_departure(
                true,
                move |_, _| {
                    departed.send(()).unwrap();
                },
                window,
                cx,
            )
        });
    })
    .unwrap();
    until(cx, |_| wait.try_recv().is_ok());

    let reopened = Store::open(&path).unwrap();
    let persisted = reopened.workspace(account).unwrap().unwrap();
    let draft_id = persisted.panes[0].tabs[0].view.draft.as_deref().unwrap();
    let draft = reopened.draft(account, draft_id).unwrap().unwrap();
    assert_eq!(draft.content.body, "Latest Markdown **before exit**");
    assert_eq!(draft.content.anchor, initial_body);
    assert!(
        reopened
            .acquire_editor(account, draft_id, "restart", now_ms(), now_ms() + 10_000)
            .unwrap()
    );
    assert!(
        reopened
            .release_editor(account, draft_id, "restart")
            .unwrap()
    );
    assert_eq!(
        reopened.projections(account, item.id).unwrap(),
        original_projections
    );
}

fn until(cx: &mut TestAppContext, mut check: impl FnMut(&TestAppContext) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        cx.run_until_parked();
        if check(cx) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the cached reader operation did not finish");
}
