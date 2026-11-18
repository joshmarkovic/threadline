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
fn ui_008_cached_markdown_and_native_editor_survive_immediate_orderly_exit(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let profile = tempfile::tempdir().unwrap();
    let path = profile.path().join("reader.sqlite");
    let store = Arc::new(Store::open(&path).unwrap());
    let fixture = populate_reader(&store, fixture_seed(), Scale::Small).unwrap();
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

#[track_caller]
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

struct ReaderHarness {
    _directory: tempfile::TempDir,
    store: Arc<Store>,
    fixture: threadline_storage::fixtures::Fixture,
    runtime: tokio::runtime::Runtime,
}

impl ReaderHarness {
    fn seeded() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&directory.path().join("s04-ui.sqlite")).unwrap());
        let fixture = populate_reader(&store, fixture_seed(), Scale::Small).unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        Self {
            _directory: directory,
            store,
            fixture,
            runtime,
        }
    }

    fn one_pane(&self, tab: &Tab) {
        let mut layout = empty_workspace();
        layout.panes[0].active = Some(0);
        layout.panes[0].tabs.push(tab.clone());
        self.store
            .save_workspace(self.fixture.account, &layout)
            .unwrap();
    }

    fn open(
        &self,
        cx: &mut TestAppContext,
    ) -> (gpui_kit::AnyWindowHandle, Entity<ReaderWorkspace>) {
        cx.executor().allow_parking();
        let account = self.fixture.account;
        let repositories = self.store.github_repository_views(account).unwrap();
        let profiles = self.store.github_access_profiles().unwrap();
        let store = Arc::clone(&self.store);
        let handle = self.runtime.handle().clone();
        let (window, reader) = cx.update(|cx| {
            gpui_kit::init(cx);
            super::bind_shortcuts(cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(1420.), px(1010.)),
                })),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| {
                    let mut reader = ReaderWorkspace::new(store, handle, None, window, cx);
                    reader.set_account(
                        ReaderAccount {
                            account: Some(account),
                            profile: profiles.first().cloned(),
                            offline: true,
                            reachability: Reachability::NotAttempted,
                            repositories,
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
            cx.read(|cx| {
                let reader = reader.read(cx);
                !reader.layout_busy
                    && reader
                        .active_tab()
                        .is_some_and(|tab| reader.tab_views.contains_key(&tab.id))
            })
        });
        (window, reader)
    }
}

fn click(cx: &mut TestAppContext, window: gpui_kit::AnyWindowHandle, id: &'static str) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
}

fn fixture_seed() -> u64 {
    std::env::var("THREADLINE_TEST_SEED")
        .ok()
        .and_then(|seed| seed.parse().ok())
        .unwrap_or(12345)
}

#[gpui_kit::test]
fn ui_002_independent_panes_share_one_editor_and_reopen_same_draft(cx: &mut TestAppContext) {
    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    let item = setup.fixture.long_discussion;
    let tab = Tab {
        id: "primary-note".into(),
        item,
        view: TabView {
            navigation: Some("issue-conversation".into()),
            ..Default::default()
        },
    };
    setup.one_pane(&tab);
    let queue_before = setup.store.queue_page(account, 0, 100).unwrap();
    let (window, reader) = setup.open(cx);
    click(cx, window, "reader-duplicate-tab");
    until(cx, |cx| {
        cx.read(|cx| {
            let view = reader.read(cx);
            view.layout.panes.len() == 2 && view.tab_views.len() == 2 && !view.layout_busy
        })
    });
    let duplicate = cx.read(|cx| reader.read(cx).layout.panes[1].tabs[0].clone());
    assert_ne!(duplicate.id, tab.id);
    assert_eq!(duplicate.item, tab.item);

    click(cx, window, "reader-draft-comment-0");
    let draft_id = format!("comment-{}", item.0);
    until(cx, |cx| {
        cx.read(|cx| {
            let view = reader.read(cx);
            !view.layout_busy
                && view
                    .drafts
                    .get(&draft_id)
                    .is_some_and(|editor| editor.tab_id == tab.id)
        })
    });
    click(cx, window, "reader-draft-comment-1");
    until(cx, |cx| {
        cx.read(|cx| {
            let view = reader.read(cx);
            view.drafts.len() == 1 && !view.layout_busy && view.layout.focused_pane == 0
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert!(
            reader.read(cx).drafts[&draft_id]
                .view
                .focus_handle(cx)
                .is_focused(window)
        );
        window.input("Shared draft in one editor", cx);
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).drafts[&draft_id].saved())
    });
    click(cx, window, "reader-move-tab");
    until(cx, |cx| {
        cx.read(|cx| {
            let view = reader.read(cx);
            !view.layout_busy
                && view.layout.focused_pane == 1
                && view.layout.panes[1].tabs.len() == 2
        })
    });
    click(cx, window, "reader-close-tab");
    until(cx, |cx| {
        cx.read(|cx| {
            let view = reader.read(cx);
            !view.layout_busy && view.layout.panes[1].tabs.len() == 1
        })
    });
    click(cx, window, "reader-reopen-tab");
    until(cx, |cx| {
        cx.read(|cx| {
            let view = reader.read(cx);
            !view.layout_busy && view.active_tab().is_some_and(|active| active.id == tab.id)
        })
    });
    let persisted = setup.store.workspace(account).unwrap().unwrap();
    let reopened = persisted.panes[1]
        .tabs
        .iter()
        .find(|candidate| candidate.id == tab.id)
        .unwrap();
    assert_eq!(reopened.view.navigation, tab.view.navigation);
    assert_eq!(reopened.view.draft.as_deref(), Some(draft_id.as_str()));
    assert_eq!(
        setup
            .store
            .draft(account, &draft_id)
            .unwrap()
            .unwrap()
            .content
            .body,
        "Shared draft in one editor"
    );
    assert_eq!(
        setup.store.queue_page(account, 0, 100).unwrap(),
        queue_before
    );
}

#[gpui_kit::test]
fn ui_003_partial_error_and_unseen_conversation_keep_cached_panes_visible(cx: &mut TestAppContext) {
    use threadline_storage::{
        ItemLocator,
        resources::{
            Coverage, ResourceError, ResourceFamily, ResourceKey, ResourcePage, ResourceRecord,
            ResourceTarget, WalkMode,
        },
    };
    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    let create = |number, title: &str| {
        setup
            .store
            .ingest(
                account,
                ItemLocator {
                    repository: setup.fixture.repository,
                    number,
                },
                ItemUpdate {
                    representation: ItemRepresentation::Issue(IssueRecord {
                        id: threadline_core::domain::IssueApiId::new(
                            std::num::NonZeroU64::new(number).unwrap(),
                        ),
                        node_id: None,
                        classification: IssueClassification::Issue,
                    }),
                    remote_updated_at: UtcMillis(1_900_000_000_000),
                    title: Change::Value(title.into()),
                    body: Change::Value(format!("{title} keeps its downloaded body.")),
                    closed: Change::Value(false),
                },
            )
            .unwrap()
    };
    let partial = create(4981, "Partial issue");
    let missing = create(4982, "Unseen issue");
    let key = ResourceKey {
        account,
        target: ResourceTarget::Item(partial),
        family: ResourceFamily::Comments,
    };
    let progress = setup
        .store
        .start_resource_fetch(key, WalkMode::Full)
        .unwrap();
    let checkpoint = setup
        .store
        .commit_resource_page(
            key,
            &progress,
            ResourcePage {
                records: &[ResourceRecord {
                    key: "1".into(),
                    body: "Already downloaded comment.".into(),
                }],
                next_cursor: Some("2"),
                completed_validators: None,
                fetched_at_ms: 1_900_000_000_001,
            },
        )
        .unwrap()
        .unwrap();
    setup
        .store
        .record_resource_error(key, &checkpoint, ResourceError::Transport)
        .unwrap();
    let pane = |id: &str, item| Pane {
        active: Some(0),
        tabs: vec![Tab {
            id: id.into(),
            item,
            view: TabView::default(),
        }],
    };
    setup
        .store
        .save_workspace(
            account,
            &Workspace {
                focused_pane: 0,
                proportions: vec![0.5; 2],
                panes: vec![pane("partial", partial), pane("missing", missing)],
            },
        )
        .unwrap();
    let (window, reader) = setup.open(cx);
    until(cx, |cx| cx.read(|cx| reader.read(cx).tab_views.len() == 2));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let connectivity = window.find("reader-connection");
        assert_eq!(connectivity.role(), Some(gpui_kit::Role::Status));
        assert!(connectivity.label().unwrap().contains("Working offline"));
        let error = window.find("Comments-resource-problem");
        assert_eq!(error.role(), Some(gpui_kit::Role::Status));
        assert!(
            error
                .label()
                .unwrap()
                .contains("Download interrupted by a connection error")
        );
        assert_eq!(window.find_all("Comments-resource-problem").len(), 1);
        for pane in [0, 1] {
            assert!(window.find(format!("reader-conversation-{pane}")).visible());
        }
    })
    .unwrap();
    cx.read(|cx| {
        let reader = reader.read(cx);
        assert_eq!(
            reader.tab_views["partial"].data.discussion.state.coverage,
            Coverage::Partial
        );
        assert_eq!(reader.tab_views["partial"].notes.len(), 1);
        assert_eq!(
            reader.tab_views["missing"].data.discussion.state.coverage,
            Coverage::Missing
        );
        assert!(reader.tab_views["missing"].notes.is_empty());
        assert!(reader.tab_views.values().all(|view| view.body.is_some()));
    });
}

#[gpui_kit::test]
fn ui_008_keyboard_find_panes_and_multiline_editor_have_accessible_identity(
    cx: &mut TestAppContext,
) {
    let setup = ReaderHarness::seeded();
    let item = setup.fixture.long_discussion;
    setup.one_pane(&Tab {
        id: "keyboard-note".into(),
        item,
        view: TabView::default(),
    });
    let (window, reader) = setup.open(cx);

    click(cx, window, "reader-tab-0-0");
    until(cx, |cx| cx.read(|cx| !reader.read(cx).layout_busy));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("reader-pane-0").role(),
            Some(gpui_kit::Role::Group)
        );
        assert_eq!(window.find("reader-pane-0").label(), Some("Pane 1"));
        assert_eq!(
            window.find("reader-search-submit").label(),
            Some("Search downloaded")
        );
        window.press("secondary-shift-f", cx);
        assert!(reader.read(cx).search.focus_handle(cx).is_focused(window));
        window.input("synthetic", cx);
        window.press("enter", cx);
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).search_reply.is_some())
    });

    cx.update_window(window, |_, window, cx| {
        window.press("secondary-alt-d", cx);
    })
    .unwrap();
    let id = format!("comment-{}", item.0);
    until(cx, |cx| {
        cx.read(|cx| {
            let reader = reader.read(cx);
            !reader.layout_busy && reader.drafts.contains_key(&id)
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert!(
            reader.read(cx).drafts[&id]
                .view
                .focus_handle(cx)
                .is_focused(window)
        );
        window.input("Keyboard draft", cx);
        window.press("enter", cx);
        window.input("Line two", cx);
        assert_eq!(
            reader.read(cx).drafts[&id].view.read(cx).value().as_ref(),
            "Keyboard draft\nLine two"
        );
    })
    .unwrap();
    until(cx, |cx| cx.read(|cx| reader.read(cx).drafts[&id].saved()));
    assert_eq!(
        setup
            .store
            .draft(setup.fixture.account, &id)
            .unwrap()
            .unwrap()
            .content
            .body,
        "Keyboard draft\nLine two"
    );
}

#[gpui_kit::test]
fn ui_008_cache_restores_a_tab_selected_during_a_pane_focus_save(cx: &mut TestAppContext) {
    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    let mut layout = setup.store.workspace(account).unwrap().unwrap();
    layout.focused_pane = 1;
    layout.panes[0].active = Some(0);
    let selected = layout.panes[0].tabs[1].clone();
    setup.store.save_workspace(account, &layout).unwrap();
    let (window, reader) = setup.open(cx);

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        reader.update(cx, |reader, cx| reader.remember_pane_focus(0, window, cx));
        assert!(
            reader.read(cx).layout_busy,
            "the focus-only save is still pending"
        );
        window.click("reader-tab-0-1", cx);
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| {
            let reader = reader.read(cx);
            !reader.layout_busy && reader.active_tab().is_some_and(|tab| tab.id == selected.id)
        })
    });
    let persisted = setup.store.workspace(account).unwrap().unwrap();
    assert_eq!(persisted.focused_pane, 0);
    assert_eq!(persisted.panes[0].active, Some(1));
    assert_eq!(persisted.panes[0].tabs[1].id, selected.id);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .find("reader-tab-0-1")
                .label()
                .unwrap()
                .ends_with(" · Active tab")
        );
        assert!(
            !window
                .find("reader-tab-0-0")
                .label()
                .unwrap()
                .ends_with(" · Active tab")
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn ui_008_observed_reader_controls_follow_the_visible_panes(cx: &mut TestAppContext) {
    let setup = ReaderHarness::seeded();
    let (window, reader) = setup.open(cx);
    until(cx, |cx| {
        cx.read(|cx| !reader.read(cx).list_busy && !reader.read(cx).rows.is_empty())
    });
    let (observation, writer) = crate::observation::Observer::new(
        setup._directory.path().join("bounds.json"),
        std::time::Instant::now(),
        false,
        setup.runtime.handle(),
    );
    cx.update_window(window, |_, window, cx| {
        reader.update(cx, |reader, cx| {
            reader.set_observer(Some(observation.clone()), cx)
        });
        observation.begin_frame();
        window.render_frame(cx);
    })
    .unwrap();
    for control in [
        "reader-search",
        "reader-search-submit",
        "reader-result-0",
        "reader-tab-0-0",
        "reader-tab-1-0",
        "reader-close-tab",
    ] {
        let bounds = observation
            .test_control_bounds(control)
            .unwrap_or_else(|| panic!("missing visible {control}"));
        assert!(bounds[2] > 0. && bounds[3] > 0.);
    }
    click(cx, window, "reader-maximize-pane");
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).maximized && !reader.read(cx).layout_busy)
    });
    cx.update_window(window, |_, window, cx| {
        observation.begin_frame();
        window.render_frame(cx);
    })
    .unwrap();
    let hidden = cx.read(|cx| 1 - reader.read(cx).layout.focused_pane);
    assert!(
        observation
            .test_control_bounds(&format!("reader-tab-{hidden}-0"))
            .is_none()
    );
    setup.runtime.block_on(writer.finish()).unwrap();
}

#[gpui_kit::test]
fn ui_001_pr_context_changes_keep_the_immutable_native_diff_and_each_tabs_state(
    cx: &mut TestAppContext,
) {
    use gpui_kit::component::diff::{DiffLineRange, DiffMode, DiffSide};
    use std::num::NonZeroU64;
    use threadline_storage::{
        ItemLocator,
        pr::{
            PatchEvidence, PrContextMode, PrFamily, PrHeader, PrInputPage, PrRecord, PrSection,
            ReferenceObservation,
        },
        resources::{CommitOid, WalkMode},
    };
    let setup = ReaderHarness::seeded();
    let store = &setup.store;
    let account = setup.fixture.account;
    let repository = setup.fixture.repository;
    let pr = setup.fixture.reviewed_pr;
    let pr_item = store.item(account, pr).unwrap().unwrap();
    let stamp = 1_900_000_000_000;
    store
        .ingest(
            account,
            pr_item.locator,
            ItemUpdate {
                representation: ItemRepresentation::Issue(IssueRecord {
                    id: pr_item.issue_id.unwrap(),
                    node_id: pr_item.issue_node_id,
                    classification: IssueClassification::PullRequest,
                }),
                remote_updated_at: UtcMillis(stamp),
                title: Change::Value("Offline review fixture".into()),
                body: Change::Value("Review introduction\n\nFixes #101. Resolves #102.".into()),
                closed: Change::Value(false),
            },
        )
        .unwrap();
    let issue_ids = (101..=102)
        .map(|number| {
            let issue_id =
                threadline_storage::IssueApiId::new(NonZeroU64::new(6000 + number).unwrap());
            let item = store
                .ingest(
                    account,
                    ItemLocator { repository, number },
                    ItemUpdate {
                        representation: ItemRepresentation::Issue(IssueRecord {
                            id: issue_id,
                            node_id: None,
                            classification: IssueClassification::Issue,
                        }),
                        remote_updated_at: UtcMillis(stamp),
                        title: Change::Value(format!("Linked fixture {number}")),
                        body: Change::Value(format!("**Cached issue {number} body**")),
                        closed: Change::Value(false),
                    },
                )
                .unwrap();
            (item, issue_id)
        })
        .collect::<Vec<_>>();
    use threadline_storage::access::{
        AccessState, DiscoveryState, RepositoryObservation, RepositoryObservationSource,
    };
    let original = store.github_access_profiles().unwrap().remove(0);
    let profile = store
        .reserve_access_profile_credential(
            account,
            original.user_id,
            &original.login,
            &original.owner_scope,
            original.kind,
            Some(&original),
        )
        .unwrap();
    store
        .install_current_credential(&profile, || Ok::<_, ()>(()))
        .unwrap()
        .unwrap();
    store
        .record_access_result(
            &profile,
            original.kind,
            AccessState::Readable,
            DiscoveryState::Complete,
            stamp,
        )
        .unwrap();
    let repo = store
        .github_repository_views(account)
        .unwrap()
        .into_iter()
        .find(|repo| repo.id == repository)
        .unwrap();
    assert_eq!(
        store
            .observe_github_repository(
                &profile,
                &RepositoryObservation {
                    provider_id: repo.provider_id,
                    owner: &repo.owner,
                    name: &repo.name,
                    private: true,
                    source: RepositoryObservationSource::Direct,
                },
                stamp
            )
            .unwrap(),
        repository
    );
    let body_revision = "fixture-pr-body-1";
    let prior = store
        .pr_reference_set(account, pr)
        .unwrap()
        .map(|(revision, _)| revision);
    let candidates = store
        .index_pr_body(account, pr, prior.as_deref(), body_revision, stamp)
        .unwrap();
    for (candidate, (item, issue_id)) in candidates.into_iter().zip(&issue_ids) {
        store
            .record_pr_reference(
                account,
                pr,
                body_revision,
                candidate.position,
                ReferenceObservation::Item {
                    repository,
                    item: *item,
                    issue_id: *issue_id,
                },
                stamp + 1,
            )
            .unwrap();
    }
    store.sign_out_github_account(account, stamp + 2).unwrap();
    let commit = |byte: char| CommitOid::parse(&byte.to_string().repeat(40)).unwrap();
    let revision = store
        .record_pr_revision(
            account,
            pr,
            &commit('a'),
            &commit('b'),
            Some(&commit('c')),
            &PrHeader::default(),
            stamp,
        )
        .unwrap();
    let write = store
        .start_pr_fetch(&revision, PrFamily::Files, WalkMode::Full)
        .unwrap();
    let filenames = ["src/main.rs", "src/renamed module.rs"];
    store
        .commit_pr_page(
            &revision,
            PrFamily::Files,
            &write,
            PrInputPage {
                records: filenames
                    .iter()
                    .map(|path| PrRecord::File {
                        path: (*path).to_owned(),
                        previous_path: Some("src/old.rs".into()),
                        status: "renamed".into(),
                        additions: Some(1),
                        deletions: Some(1),
                        patch: PatchEvidence::Text(
                            "@@ -10,2 +20,2 @@\n unchanged\n-old line\n+new line".into(),
                        ),
                    })
                    .collect(),
                next_cursor: None,
                validators: None,
                partial: None,
                fetched_at_ms: stamp + 1,
            },
        )
        .unwrap();
    let tab = Tab {
        id: "review-pane-one".into(),
        item: pr,
        view: TabView::default(),
    };
    setup.one_pane(&tab);
    let (window, reader) = setup.open(cx);
    click(cx, window, "pr-0-file-1");
    until(cx, |cx| {
        cx.read(|cx| {
            reader.read(cx).tab_views[&tab.id]
                .pr
                .as_ref()
                .and_then(|pr| pr.data.files.as_ref())
                .and_then(|files| files.selected.as_ref())
                .is_some_and(|file| file.summary.path == filenames[1])
        })
    });
    click(cx, window, "pr-0-diff-split");
    let diff = cx
        .update_window(window, |_, _, cx| {
            let diff = reader.read(cx).tab_views[&tab.id]
                .pr
                .as_ref()
                .unwrap()
                .diff
                .as_ref()
                .unwrap()
                .clone();
            diff.update(cx, |state, cx| {
                state.set_selected_lines(
                    Some(DiffLineRange::new(filenames[1], DiffSide::Modified, 21, 21)),
                    cx,
                )
            });
            assert_eq!(diff.read(cx).mode(), DiffMode::Split);
            assert!(diff.read(cx).selected_text(cx).contains("new line"));
            diff
        })
        .unwrap();
    until(cx, |cx| cx.read(|cx| !reader.read(cx).layout_busy));
    click(cx, window, "pr-0-context-issue");
    until(cx, |cx| {
        cx.read(|cx| {
            let reader = reader.read(cx);
            !reader.layout_busy
                && reader.tab_views[&tab.id]
                    .pr
                    .as_ref()
                    .and_then(|pr| pr.data.linked.as_ref())
                    .is_some_and(|link| link.item.id == issue_ids[0].0)
        })
    });
    click(cx, window, "pr-0-linked-1");
    until(cx, |cx| {
        cx.read(|cx| {
            let reader = reader.read(cx);
            !reader.layout_busy
                && reader.tab_views[&tab.id]
                    .pr
                    .as_ref()
                    .and_then(|pr| pr.data.linked.as_ref())
                    .is_some_and(|link| link.item.id == issue_ids[1].0)
        })
    });
    click(cx, window, "pr-0-context-body");
    until(cx, |cx| cx.read(|cx| !reader.read(cx).layout_busy));
    click(cx, window, "pr-0-context-collapse");
    until(cx, |cx| cx.read(|cx| !reader.read(cx).layout_busy));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("pr-0-context-issue").is_none());
        let pr = reader.read(cx).tab_views[&tab.id].pr.as_ref().unwrap();
        assert_eq!(pr.data.revision, revision);
        assert_eq!(pr.diff.as_ref(), Some(&diff));
        assert!(diff.read(cx).selected_text(cx).contains("new line"));
    })
    .unwrap();
    click(cx, window, "reader-duplicate-tab");
    until(cx, |cx| {
        cx.read(|cx| {
            let reader = reader.read(cx);
            reader.layout.panes.len() == 2 && !reader.layout_busy && reader.tab_views.len() == 2
        })
    });
    click(cx, window, "pr-1-conversation");
    until(cx, |cx| cx.read(|cx| !reader.read(cx).layout_busy));
    let reopened = Store::open(&setup._directory.path().join("s04-ui.sqlite")).unwrap();
    let saved = reopened.reader_layout(account).unwrap().unwrap().0;
    let original = &saved.panes[0].tabs[0].view;
    let duplicate = &saved.panes[1].tabs[0].view;
    let settings = original.pr_reader.as_ref().unwrap();
    assert_eq!(settings.section, PrSection::Files);
    assert_eq!(settings.context_mode, PrContextMode::Body);
    assert!(settings.context_collapsed);
    assert_eq!(settings.selected_path.as_deref(), Some(filenames[1]));
    assert_eq!(
        original.context.as_ref().unwrap().selected_issue,
        Some(issue_ids[1].0)
    );
    assert_eq!(
        duplicate.pr_reader.as_ref().unwrap().section,
        PrSection::Conversation
    );
}

#[gpui_kit::test]
fn ui_006_native_offline_edit_keeps_saved_text_and_cancels_the_original_queue_target(
    cx: &mut TestAppContext,
) {
    use threadline_storage::{queue::State, writes::WriteIntent};
    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    let item = setup.fixture.long_discussion;
    let tab = Tab {
        id: "existing-issue-write".into(),
        item,
        view: TabView::default(),
    };
    setup.one_pane(&tab);
    let previous = setup.store.queue_page(account, 0, 100).unwrap();
    let previous_title = setup.store.item(account, item).unwrap().unwrap().title;
    let (window, reader) = setup.open(cx);
    click(cx, window, "reader-edit-issue-0");
    let draft_id = format!("issue-{}", item.0);
    until(cx, |cx| {
        cx.read(|cx| {
            let reader = reader.read(cx);
            !reader.layout_busy && reader.drafts.contains_key(&draft_id)
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        window.click("reader-issue-title-0", cx);
        window.press("secondary-a", cx);
        window.input("Revised title saved offline", cx);
        let body = reader.read(cx).drafts[&draft_id].view.clone();
        body.focus_handle(cx).focus(window, cx);
        window.render_frame(cx);
        window.press("secondary-a", cx);
        window.input("Preserve this local Markdown after cancellation", cx);
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).drafts[&draft_id].saved())
    });
    assert_eq!(setup.store.queue_page(account, 0, 100).unwrap(), previous);
    click(cx, window, "reader-submit-0");
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).drafts[&draft_id].submitted.is_some())
    });
    let page = setup.store.queue_page(account, 0, 100).unwrap();
    assert_eq!(page.len(), previous.len() + 1);
    let queued = page.last().unwrap();
    assert_eq!(queued.item, item);
    assert_eq!(queued.state, State::Queued);
    let Some(WriteIntent::EditIssue(changes)) =
        setup.store.write_intent(account, &queued.id).unwrap()
    else {
        panic!("expected existing issue write")
    };
    assert_eq!(changes.title.unwrap().wanted, "Revised title saved offline");
    assert_eq!(
        changes.body.unwrap().wanted.as_deref(),
        Some("Preserve this local Markdown after cancellation")
    );
    assert_eq!(
        setup.store.item(account, item).unwrap().unwrap().title,
        previous_title
    );

    click(cx, window, "reader-outbox");
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).queue.iter().any(|row| row.id == queued.id))
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(format!("reader-outbox-action-{}", page.len() - 1), cx);
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).outbox.selected_state().as_deref() == Some("Queued"))
    });
    click(cx, window, "reader-outbox-cancel");
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).outbox.selected_state().as_deref() == Some("Cancelled"))
    });
    click(cx, window, "reader-outbox-filter-history");
    click(cx, window, "reader-outbox-pause");
    until(cx, |cx| cx.read(|cx| reader.read(cx).outbox.is_paused()));
    let reopened = Store::open(&setup._directory.path().join("s04-ui.sqlite")).unwrap();
    let saved = reopened.operation(account, &queued.id).unwrap().unwrap();
    assert_eq!(saved.state, State::Cancelled);
    assert_eq!(saved.item, item);
    assert_eq!(
        saved.snapshot.draft.unwrap().body,
        "Preserve this local Markdown after cancellation"
    );
    assert!(
        reopened
            .sync_settings(account)
            .unwrap()
            .controls
            .pause_queued_actions
    );
    assert_eq!(
        reopened
            .draft(account, &draft_id)
            .unwrap()
            .unwrap()
            .content
            .body,
        "Preserve this local Markdown after cancellation"
    );
}

#[gpui_kit::test]
fn ui_006_registered_native_commands_offer_edits_and_comments_without_issue_creation(
    cx: &mut TestAppContext,
) {
    use threadline_storage::{queue::ActionKind, writes::WriteIntent};
    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    let item = setup.fixture.long_discussion;
    let tab = Tab {
        id: "issue-command-inventory".into(),
        item,
        view: TabView::default(),
    };
    setup.one_pane(&tab);
    let previous = setup.store.queue_page(account, 0, 100).unwrap();
    let (window, reader) = setup.open(cx);
    let prohibit_issue_creation = |label: &str| {
        let key: String = label
            .chars()
            .filter(char::is_ascii_alphabetic)
            .map(|character| character.to_ascii_lowercase())
            .collect();
        assert!(
            !key.contains("newissue") && !key.contains("createissue"),
            "unexpected issue-creation command {label}"
        );
    };
    cx.update_window(window, |_, window, cx| {
        crate::workspace::bind_shortcuts(cx);
        window.render_frame(cx);
        assert!(window.find("reader-edit-issue-0").visible());
        assert!(window.find("reader-draft-comment-0").visible());
        for command in cx.all_action_names() {
            prohibit_issue_creation(command);
        }
        for element in gpui_kit::base::test_support::snapshots(window) {
            if let Some(label) = element.label() {
                prohibit_issue_creation(label);
            }
        }
        assert!(cx.build_action("threadline::NewIssue", None).is_err());
        window.press("secondary-n", cx);
        assert_eq!(reader.read(cx).layout.panes[0].tabs.len(), 1);
    })
    .unwrap();
    assert_eq!(setup.store.queue_page(account, 0, 100).unwrap(), previous);
    click(cx, window, "reader-draft-comment-0");
    let draft_id = format!("comment-{}", item.0);
    until(cx, |cx| {
        cx.read(|cx| {
            let reader = reader.read(cx);
            !reader.layout_busy && reader.drafts.contains_key(&draft_id)
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        window.input("A regular comment on an existing issue", cx);
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).drafts[&draft_id].saved())
    });
    assert_eq!(setup.store.queue_page(account, 0, 100).unwrap(), previous);
    click(cx, window, "reader-submit-0");
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).drafts[&draft_id].submitted.is_some())
    });
    let queued = setup.store.queue_page(account, 0, 100).unwrap();
    assert_eq!(queued.len(), previous.len() + 1);
    let queued = queued.last().unwrap();
    assert_eq!((queued.kind, queued.item), (ActionKind::Comment, item));
    assert_eq!(
        setup.store.write_intent(account, &queued.id).unwrap(),
        Some(WriteIntent::CreateComment {
            body: "A regular comment on an existing issue".into()
        })
    );
}
