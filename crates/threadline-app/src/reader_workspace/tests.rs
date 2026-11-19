use super::*;
use gpui_kit::{
    AppContext, Bounds, Pixels, Point, Size, TestAppContext, WindowBounds, WindowOptions, size,
    test::TestWindowExt,
};
use std::num::NonZeroU64;
use threadline_storage::{
    IssueClassification, IssueRecord, ItemLocator, ItemRepresentation, ItemUpdate, UtcMillis,
    fixtures::{Scale, populate_reader},
    queue::DraftKind,
};

#[gpui_kit::test]
fn ui_008_inactive_tab_labels_restore_and_follow_cached_title_updates(cx: &mut TestAppContext) {
    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    let number = NonZeroU64::new(94821).unwrap();
    let update_title = |title: &str, timestamp| {
        setup
            .store
            .ingest(
                account,
                ItemLocator {
                    repository: setup.fixture.repository,
                    number: number.get(),
                },
                ItemUpdate {
                    representation: ItemRepresentation::Issue(IssueRecord {
                        id: threadline_core::domain::IssueApiId::new(number),
                        node_id: None,
                        classification: IssueClassification::Issue,
                    }),
                    remote_updated_at: UtcMillis(timestamp),
                    title: Change::Value(title.into()),
                    body: Change::Missing,
                    closed: Change::Missing,
                },
            )
            .unwrap()
    };
    let item = update_title("Saved title", 1_900_000_000_000);
    let draft = "inactive-draft";
    setup
        .store
        .create_draft(
            account,
            draft,
            item,
            DraftKind::Comment,
            &threadline_storage::queue::DraftContent {
                body: "Retain this draft".into(),
                base: String::new(),
                anchor: String::new(),
            },
        )
        .unwrap();
    let mut layout = setup.store.workspace(account).unwrap().unwrap();
    for (index, pane) in layout.panes.iter_mut().enumerate() {
        pane.active = Some(0);
        pane.tabs[1].item = item;
        pane.tabs[1].view = TabView {
            draft: (index == 0).then(|| draft.to_string()),
            ..Default::default()
        };
    }
    setup.store.save_workspace(account, &layout).unwrap();
    let other = setup
        .store
        .ensure_account("github.com", 98_765_432)
        .unwrap();
    assert!(
        setup
            .store
            .tab_summaries(other, &[item])
            .unwrap()
            .is_empty()
    );
    let (window, reader) = setup.open(cx);
    let check = |cx: &mut TestAppContext, title: &str| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let reader = reader.read(cx);
            for (index, pane) in reader.layout.panes.iter().enumerate() {
                assert!(!reader.tab_views.contains_key(&pane.tabs[1].id));
                let label = format!(
                    "Issue fixture/relay #94821 {title}{}",
                    if index == 0 { " · Draft" } else { "" }
                );
                assert_eq!(
                    window.find(format!("reader-tab-{index}-1")).label(),
                    Some(label.as_str())
                );
            }
        })
        .unwrap();
    };
    check(cx, "Saved title");
    update_title("Confirmed new title", 1_900_000_000_001);
    cx.update_window(window, |_, window, cx| {
        reader.update(cx, |reader, cx| {
            reader.update_sync(
                crate::sync_worker::SyncUpdate {
                    account: Some(account),
                    status: None,
                    connection: threadline_core::connectivity::ConnectivityObserver::new(1)
                        .snapshot(std::time::Duration::ZERO),
                    cache_generation: 1,
                    history_generation: 0,
                    eligible_outbox: 0,
                },
                window,
                cx,
            );
        });
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| {
            reader
                .read(cx)
                .tab_summaries
                .get(&item)
                .and_then(|row| row.title.as_deref())
                == Some("Confirmed new title")
        })
    });
    check(cx, "Confirmed new title");
}

#[gpui_kit::test]
fn ui_008_pasted_github_comments_reuse_their_cached_tab_and_keep_other_panes(
    cx: &mut TestAppContext,
) {
    use threadline_storage::{
        pr::{InlineComment, PrFamily, PrHeader, PrInputPage, PrRecord, PrSection},
        reader::{DiscussionComment, DiscussionInputPage},
        resources::{CommitOid, ResourceFamily, ResourceKey, ResourceTarget, WalkMode},
    };
    let setup = ReaderHarness::seeded();
    let store = &setup.store;
    let account = setup.fixture.account;
    let issue = setup.fixture.long_discussion;
    let pull = setup.fixture.reviewed_pr;
    let comments = ResourceKey {
        account,
        target: ResourceTarget::Item(issue),
        family: ResourceFamily::Comments,
    };
    let progress = store
        .start_resource_fetch(comments, WalkMode::Full)
        .unwrap();
    let comment = |id, body: &str| DiscussionComment {
        provider_id: id,
        node_id: None,
        author: None,
        created_at_ms: id as i64,
        updated_at_ms: id as i64,
        body: body.into(),
    };
    store
        .commit_discussion_page(
            account,
            issue,
            &progress,
            DiscussionInputPage {
                comments: vec![
                    comment(801, "Earlier issue comment"),
                    comment(802, "Linked issue comment"),
                ],
                next_cursor: None,
                completed_validators: None,
                fetched_at_ms: 900,
            },
        )
        .unwrap();
    let pull_comments = ResourceKey {
        target: ResourceTarget::Item(pull),
        ..comments
    };
    let progress = store
        .start_resource_fetch(pull_comments, WalkMode::Full)
        .unwrap();
    store
        .commit_discussion_page(
            account,
            pull,
            &progress,
            DiscussionInputPage {
                comments: vec![comment(803, "PrSearchAnchor ordinary PR discussion")],
                next_cursor: None,
                completed_validators: None,
                fetched_at_ms: 901,
            },
        )
        .unwrap();
    let oid = |c: &str| CommitOid::parse(&c.repeat(40)).unwrap();
    let revision = store
        .record_pr_revision(
            account,
            pull,
            &oid("a"),
            &oid("b"),
            Some(&oid("c")),
            &PrHeader::default(),
            1_900_000_000_000,
        )
        .unwrap();
    let progress = store
        .start_pr_fetch(&revision, PrFamily::Threads, WalkMode::Full)
        .unwrap();
    let review = |id, in_reply_to, body: &str| InlineComment {
        id,
        node_id: None,
        in_reply_to,
        author: Some("fixture-reader".into()),
        actor: Default::default(),
        body: body.into(),
        created_at_ms: None,
        updated_at_ms: None,
        anchor: None,
    };
    store
        .commit_pr_page(
            &revision,
            PrFamily::Threads,
            &progress,
            PrInputPage {
                records: vec![PrRecord::Thread {
                    node_id: "PRRT_linked".into(),
                    resolved: None,
                    outdated: None,
                    location: None,
                    comments: vec![
                        review(901, None, "Original review root"),
                        review(902, Some(901), "Exact downloaded review reply"),
                    ],
                    replies_complete: true,
                    reply_cursor: None,
                }],
                next_cursor: None,
                validators: None,
                partial: None,
                fetched_at_ms: 1_900_000_000_001,
            },
        )
        .unwrap();

    let pane = |id: &str, item| Pane {
        active: Some(0),
        tabs: vec![Tab {
            id: id.into(),
            item,
            view: TabView::default(),
        }],
    };
    store
        .save_workspace(
            account,
            &Workspace {
                focused_pane: 0,
                proportions: vec![0.4, 0.6],
                panes: vec![pane("original-issue", issue), pane("original-pull", pull)],
            },
        )
        .unwrap();
    let (window, reader) = setup.open_at(cx, size(px(1750.), px(1000.)));
    let url = |kind, item, fragment| {
        let number = store.item(account, item).unwrap().unwrap().locator.number;
        format!("https://github.com/fixture/relay/{kind}/{number}{fragment}")
    };
    let enter = |cx: &mut TestAppContext, url: &str| {
        cx.update_window(window, |_, window, cx| {
            reader.read(cx).url.focus_handle(cx).focus(window, cx);
            window.press("secondary-a", cx);
            window.input(url, cx);
            assert_eq!(reader.read(cx).url.read(cx).value().as_ref(), url);
        })
        .unwrap();
        click(cx, window, "reader-open-url");
    };
    enter(
        cx,
        &url("issues", issue, "?from=threadline#issuecomment-802"),
    );
    until(cx, |cx| {
        cx.read(|cx| {
            let state = reader.read(cx);
            !state.layout_busy
                && state.tab_views.get("original-issue").is_some_and(|view| {
                    !view.data.anchor_missing
                        && view
                            .data
                            .discussion
                            .records
                            .first()
                            .is_some_and(|comment| comment.body == "Linked issue comment")
                })
        })
    });
    enter(cx, &url("pull", pull, "#discussion_r902"));
    until(cx, |cx| {
        cx.read(|cx| {
            let state = reader.read(cx);
            !state.layout_busy
                && state
                    .active_tab()
                    .is_some_and(|tab| tab.id == "original-pull")
                && state
                    .tab_views
                    .get("original-pull")
                    .and_then(|view| view.pr.as_ref())
                    .and_then(|pr| pr.data.linked_review_comment.as_ref())
                    .is_some_and(|comment| {
                        comment.id == 902 && comment.body == "Exact downloaded review reply"
                    })
        })
    });
    let saved = store.workspace(account).unwrap().unwrap();
    assert_eq!(saved.proportions, [0.4, 0.6]);
    assert_eq!(
        saved
            .panes
            .iter()
            .map(|pane| pane.tabs.len())
            .collect::<Vec<_>>(),
        [1, 1]
    );
    assert_eq!(
        saved.panes[0].tabs[0].view.scroll_anchor.as_deref(),
        Some("00000000000000000802")
    );
    let pull_view = &saved.panes[1].tabs[0].view;
    assert_eq!(
        pull_view.scroll_anchor.as_deref(),
        Some("review-comment:902")
    );
    assert_eq!(
        pull_view.pr_reader.as_ref().unwrap().section,
        PrSection::Conversation
    );

    enter(cx, &url("pull", pull, "#discussion_r919"));
    until(cx, |cx| {
        cx.read(|cx| {
            let state = reader.read(cx);
            !state.layout_busy
                && state.active_tab().is_some_and(|tab| {
                    tab.view.scroll_anchor.as_deref() == Some("review-comment:919")
                })
                && state
                    .tab_views
                    .get("original-pull")
                    .and_then(|view| view.pr.as_ref())
                    .is_some_and(|pr| pr.data.linked_review_comment.is_none())
        })
    });
    cx.update_window(window, |_, window, cx| {
        reader.update(cx, |reader, cx| {
            reader.navigate_markdown_url(&url("issues", issue, "#issuecomment-801"), window, cx)
        })
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| {
            let state = reader.read(cx);
            !state.layout_busy
                && state.layout.focused_pane == 1
                && state.active_tab().is_some_and(|tab| {
                    tab.item == issue
                        && tab.id != "original-issue"
                        && tab.view.scroll_anchor.as_deref() == Some("00000000000000000801")
                })
        })
    });
    let saved = store.workspace(account).unwrap().unwrap();
    assert_eq!(
        saved.panes[0].tabs[0].view.scroll_anchor.as_deref(),
        Some("00000000000000000802")
    );
    assert_eq!(saved.panes[1].tabs[0].id, "original-pull");
    click(cx, window, "reader-tab-1-0");
    until(cx, |cx| {
        cx.read(|cx| {
            let state = reader.read(cx);
            !state.layout_busy
                && state
                    .active_tab()
                    .is_some_and(|tab| tab.id == "original-pull")
        })
    });
    click(cx, window, "pr-1-files");
    until(cx, |cx| {
        cx.read(|cx| {
            let state = reader.read(cx);
            !state.layout_busy
                && state
                    .active_tab()
                    .is_some_and(|tab| pr::tab_settings(tab).section == PrSection::Files)
        })
    });
    cx.update_window(window, |_, window, cx| {
        reader.read(cx).search.focus_handle(cx).focus(window, cx);
        window.input("PrSearchAnchor", cx);
    })
    .unwrap();
    click(cx, window, "reader-search-submit");
    until(cx, |cx| {
        cx.read(|cx| {
            reader
                .read(cx)
                .search_hits
                .as_ref()
                .is_some_and(|hits| hits.len() == 1)
        })
    });
    click(cx, window, "reader-result-0");
    until(cx, |cx| {
        cx.read(|cx| {
            let state = reader.read(cx);
            !state.layout_busy
                && state.active_tab().is_some_and(|tab| {
                    tab.id == "original-pull"
                        && pr::tab_settings(tab).section == PrSection::Conversation
                        && tab.view.scroll_anchor.as_deref()
                            == Some("issue-comment:00000000000000000803")
                })
                && state.tab_views.get("original-pull").is_some_and(|view| {
                    view.data.discussion.records.first().is_some_and(|record| {
                        record.body == "PrSearchAnchor ordinary PR discussion"
                    })
                })
        })
    });
    assert!(
        view::parse_item_url("https://other.example/fixture/relay/pull/1#discussion_r902")
            .is_none()
    );
    let diff = view::parse_item_url("https://github.com/fixture/relay/pull/1#diff-902").unwrap();
    assert!(diff.unsupported_fragment && diff.anchor.is_none());
}

#[gpui_kit::test]
fn cached_navigation_shows_an_existing_tab_immediately_and_saves_both_open_and_switch(
    cx: &mut TestAppContext,
) {
    let setup = ReaderHarness::seeded();
    let first = Tab {
        id: "cached-navigation-original".into(),
        item: setup.fixture.long_discussion,
        view: TabView::default(),
    };
    let next = setup.fixture.reviewed_pr;
    setup.one_pane(&first);
    let (window, reader) = setup.open(cx);
    let before = first.item;
    cx.update_window(window, |_, window, cx| {
        reader.update(cx, |reader, cx| {
            assert!(
                !reader.tab_views.values().any(|view| view
                    .data
                    .item
                    .as_ref()
                    .is_some_and(|item| item.id == next))
            );
            reader.open(next, None, OpenTab::CurrentPane, window, cx);
            assert!(reader.layout_busy);
        });
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| {
            let reader = reader.read(cx);
            reader.active_tab().is_some_and(|tab| {
                tab.item == next
                    && reader.tab_views.get(&tab.id).is_some_and(|view| {
                        view.data.item.as_ref().is_some_and(|item| item.id == next)
                    })
            }) && !reader.layout_busy
        })
    });
    let account = setup.fixture.account;
    let reopened = setup.store.reader_layout(account).unwrap().unwrap().0;
    assert_eq!(
        reopened.panes[0].tabs[reopened.panes[0].active.unwrap()].item,
        next
    );
    cx.update_window(window, |_, window, cx| {
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        window.click("reader-tab-0-0", cx);
        let visible = reader.read(cx);
        assert!(visible.layout_busy);
        assert_eq!(visible.active_tab().unwrap().item, before);
    })
    .unwrap();
    until(cx, |cx| cx.read(|cx| !reader.read(cx).layout_busy));
    let reopened = setup.store.reader_layout(account).unwrap().unwrap().0;
    assert_eq!(
        reopened.panes[0].tabs[reopened.panes[0].active.unwrap()].item,
        before
    );

    cx.update_window(window, |_, window, cx| {
        reader.update(cx, |reader, cx| {
            let mut invalid = reader.layout.clone();
            invalid.panes[0].active = Some(1);
            invalid.panes[0].tabs.push(Tab {
                id: "missing-cache".into(),
                item: ItemId(i64::MAX),
                view: TabView::default(),
            });
            let target = invalid.panes[0].tabs[1].clone();
            reader.save_layout_with_tab(invalid, Some(target), true, None, window, cx);
            assert_eq!(reader.active_tab().unwrap().item, next);
        });
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| {
            let reader = reader.read(cx);
            !reader.layout_busy && reader.active_tab().is_some_and(|tab| tab.item == before)
        })
    });
    let reopened = setup.store.reader_layout(account).unwrap().unwrap().0;
    assert_eq!(
        reopened.panes[0].tabs[reopened.panes[0].active.unwrap()].item,
        before
    );
}

#[gpui_kit::test]
fn cached_offline_account_adopts_worker_snapshot_published_before_hydration(
    cx: &mut TestAppContext,
) {
    use crate::{accounts::AccountService, sync_worker::SyncWorker};
    use threadline_core::connectivity::ConnectionStatus;

    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    setup
        .store
        .update_sync_settings(account, |settings| settings.controls.work_offline = true)
        .unwrap();
    let accounts = AccountService::new(Arc::clone(&setup.store)).unwrap();
    let worker = Arc::new(SyncWorker::start(accounts, setup.runtime.handle(), 42));
    let mut updates = worker.watch();
    setup.runtime.block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            updates.wait_for(|snapshot| {
                snapshot.account == Some(account)
                    && snapshot.connection.status == ConnectionStatus::Offline
            }),
        )
        .await
        .unwrap()
        .unwrap();
    });

    let (window, reader) = setup.open(cx);
    let profile = setup
        .store
        .github_access_profiles()
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let repositories = setup.store.github_repository_views(account).unwrap();
    cx.update_window(window, |_, window, cx| {
        reader.update(cx, |reader, cx| {
            reader.set_account(
                ReaderAccount {
                    account: None,
                    profile: None,
                    offline: true,
                    reachability: Reachability::NotAttempted,
                    repositories: Vec::new(),
                },
                window,
                cx,
            );
            reader.attach_sync(worker, window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(window, |_, window, cx| {
        reader.update(cx, |reader, cx| {
            assert!(reader.sync_update.is_none());
            reader.set_account(
                ReaderAccount {
                    account: Some(account),
                    profile: Some(profile),
                    offline: true,
                    reachability: Reachability::NotAttempted,
                    repositories,
                },
                window,
                cx,
            );
            assert_eq!(
                reader.shared_connection().unwrap().status,
                ConnectionStatus::Offline
            );
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn ui_008_cached_markdown_and_native_editor_survive_immediate_orderly_exit(
    cx: &mut TestAppContext,
) {
    use super::write_composer::WriteBase;
    use threadline_storage::{
        queue::DraftContent,
        resources::{
            ResourceFamily, ResourceKey, ResourcePage, ResourceTarget, Validators, WalkMode,
        },
    };
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
    let initial_title = "Initial fixture issue title";
    let provider_revision = "fixture-editor-base-17";
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
                title: Change::Value(initial_title.into()),
                body: Change::Value(initial_body.into()),
                closed: Change::Value(false),
            },
        )
        .unwrap();
    let metadata = ResourceKey {
        account,
        target: ResourceTarget::Item(item.id),
        family: ResourceFamily::Metadata,
    };
    let fetch = store
        .start_resource_fetch(metadata, WalkMode::Full)
        .unwrap();
    store
        .commit_resource_page(
            metadata,
            &fetch,
            ResourcePage {
                records: &[],
                next_cursor: None,
                completed_validators: Some(&Validators {
                    revision: Some(provider_revision.into()),
                    ..Default::default()
                }),
                fetched_at_ms: 1_900_000_000_001,
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
    click(cx, window, "reader-issue-issue-state-0");
    cx.update_window(window, |_, window, cx| {
        let draft = reader
            .read(cx)
            .drafts
            .get(&format!("issue-{}", item.id.0))
            .unwrap();
        let editor = draft.view.clone();
        let title = draft
            .issue_choices
            .as_ref()
            .unwrap()
            .title
            .as_ref()
            .unwrap()
            .clone();
        title.focus_handle(cx).focus(window, cx);
        window.render_frame(cx);
        window.press("secondary-a", cx);
        window.input("Changed fixture issue title", cx);
        assert_eq!(
            title.read(cx).value().as_ref(),
            "Changed fixture issue title"
        );
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
    let Some(WriteBase::Issue {
        fields,
        provider_revision: saved_revision,
    }) = WriteBase::decode(&draft.content, true)
    else {
        panic!("Expected the original issue edit base");
    };
    assert_eq!(
        fields.body.as_ref().and_then(|body| body.as_deref()),
        Some(initial_body)
    );
    assert_eq!(fields.title.as_deref(), Some(initial_title));
    assert_eq!(fields.closed, Some(false));
    assert_eq!(saved_revision, provider_revision);
    let desired: serde_json::Value = serde_json::from_str(&draft.content.anchor).unwrap();
    assert_eq!(desired["title"], "Changed fixture issue title");
    assert_eq!(desired["closed"], true);

    let legacy = DraftContent {
        body: draft.content.body,
        anchor: initial_body.into(),
        base: provider_revision.into(),
    };
    let Some(WriteBase::Issue {
        fields,
        provider_revision: legacy_revision,
    }) = WriteBase::decode(&legacy, true)
    else {
        panic!("Expected the legacy issue edit base");
    };
    assert_eq!(
        fields.body.as_ref().and_then(|body| body.as_deref()),
        Some(initial_body)
    );
    assert_eq!(legacy_revision, provider_revision);
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
        self.open_at(cx, size(px(1420.), px(1010.)))
    }

    fn open_at(
        &self,
        cx: &mut TestAppContext,
        window_size: Size<Pixels>,
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
                    size: window_size,
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
                    assert!(reader.initial_browse.is_some());
                    assert!(!reader.list_busy, "saved panes get first access to SQLite");
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

fn submit_then_focus_other_pane(
    setup: &ReaderHarness,
    cx: &mut TestAppContext,
    window: gpui_kit::AnyWindowHandle,
    reader: &Entity<ReaderWorkspace>,
    controls: (&str, &str),
    pending: impl Fn(&ReaderWorkspace) -> bool,
) {
    use rusqlite::{Connection, TransactionBehavior};

    let account = setup.fixture.account;
    let before = setup.store.queue_page(account, 0, 100).unwrap();
    let mut writer = Connection::open(setup._directory.path().join("s04-ui.sqlite")).unwrap();
    let held = writer
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let original_pane = reader.read(cx).layout.focused_pane;
        let original_item = reader.read(cx).active_tab().unwrap().item;
        window.click(controls.0.to_owned(), cx);
        assert!(pending(reader.read(cx)));
        window.click(controls.1.to_owned(), cx);
        let workspace = reader.read(cx);
        assert_ne!(workspace.layout.focused_pane, original_pane);
        assert_ne!(workspace.active_tab().unwrap().item, original_item);
        assert!(workspace.focus[workspace.layout.focused_pane].is_focused(window));
        assert!(pending(workspace));
        assert_eq!(setup.store.queue_page(account, 0, 100).unwrap(), before);

        // A second click from the original rendered frame must not queue a second operation.
        window.click(controls.0.to_owned(), cx);
    })
    .unwrap();
    drop(held);
}

fn fixture_seed() -> u64 {
    std::env::var("THREADLINE_TEST_SEED")
        .ok()
        .and_then(|seed| seed.parse().ok())
        .unwrap_or(12345)
}

#[gpui_kit::test]
fn ui_002_reused_duplicates_follow_activation_after_reorder_history_and_account_switch(
    cx: &mut TestAppContext,
) {
    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    let item = setup.fixture.long_discussion;
    let other = setup.fixture.reviewed_pr;
    let draft = format!("comment-{}", item.0);
    setup
        .store
        .create_draft(
            account,
            &draft,
            item,
            DraftKind::Comment,
            &threadline_storage::queue::DraftContent {
                body: "Preserve the original shared draft.".into(),
                base: String::new(),
                anchor: String::new(),
            },
        )
        .unwrap();
    let tab = |id: &str, item, draft| Tab {
        id: id.into(),
        item,
        view: TabView {
            draft,
            navigation: Some(id.into()),
            ..Default::default()
        },
    };
    let mut layout = Workspace {
        focused_pane: 0,
        proportions: vec![0.5, 0.5],
        panes: vec![
            Pane {
                active: Some(2),
                tabs: vec![
                    tab("unrelated", other, None),
                    tab("recent", item, Some(draft.clone())),
                    tab("rightmost", item, Some(draft.clone())),
                ],
            },
            Pane {
                active: Some(0),
                tabs: vec![tab("opposite-pane", item, Some(draft.clone()))],
            },
        ],
    };
    setup.store.save_workspace(account, &layout).unwrap();
    let alternate = setup.store.ensure_account("github.com", 43).unwrap();
    let alternate_repo = setup
        .store
        .ensure_repository(alternate, 72, "fixture", "separate")
        .unwrap();
    let alternate_item = setup
        .store
        .ingest(
            alternate,
            ItemLocator {
                repository: alternate_repo,
                number: 1,
            },
            ItemUpdate {
                representation: ItemRepresentation::Issue(IssueRecord {
                    id: 1.try_into().unwrap(),
                    node_id: None,
                    classification: IssueClassification::Issue,
                }),
                remote_updated_at: UtcMillis(1),
                title: Change::Value("Separate account issue".into()),
                body: Change::Missing,
                closed: Change::Missing,
            },
        )
        .unwrap();
    layout = Workspace {
        focused_pane: 0,
        proportions: vec![1.],
        panes: vec![Pane {
            active: Some(0),
            tabs: vec![tab("recent", alternate_item, None)],
        }],
    };
    setup.store.save_workspace(alternate, &layout).unwrap();

    let (window, reader) = setup.open_at(cx, size(px(1700.), px(1000.)));
    let wait_active = |cx: &mut TestAppContext, target: &str| {
        until(cx, |cx| {
            cx.read(|cx| {
                let view = reader.read(cx);
                !view.layout_busy && view.active_tab().is_some_and(|tab| tab.id == target)
            })
        })
    };
    let open_item = |cx: &mut TestAppContext, item| {
        cx.update_window(window, |_, window, cx| {
            reader.update(cx, |reader, cx| {
                reader.open(item, None, OpenTab::CurrentPane, window, cx)
            })
        })
        .unwrap()
    };
    click(cx, window, "reader-tab-0-1");
    wait_active(cx, "recent");
    click(cx, window, "reader-reorder-tab");
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).layout.panes[0].active == Some(0))
    });
    open_item(cx, other);
    wait_active(cx, "unrelated");
    open_item(cx, item);
    wait_active(cx, "recent");
    click(cx, window, "reader-back");
    wait_active(cx, "unrelated");
    click(cx, window, "reader-forward");
    wait_active(cx, "recent");

    let navigate_url = |cx: &mut TestAppContext, raw: &str| {
        cx.update_window(window, |_, window, cx| {
            reader.update(cx, |reader, cx| reader.navigate_url(raw, window, cx))
        })
        .unwrap();
    };
    let number = |item| {
        setup
            .store
            .item(account, item)
            .unwrap()
            .unwrap()
            .locator
            .number
    };
    let issue_url = format!("https://github.com/fixture/relay/issues/{}", number(item));
    let pr_url = format!("https://github.com/fixture/relay/pull/{}", number(other));
    click(cx, window, "reader-tab-1-0");
    wait_active(cx, "opposite-pane");
    navigate_url(cx, &pr_url);
    wait_active(cx, "unrelated");
    navigate_url(cx, &issue_url);
    wait_active(cx, "recent");

    for profile in [alternate, account] {
        cx.update_window(window, |_, window, cx| {
            reader.update(cx, |reader, cx| {
                reader.set_account(
                    ReaderAccount {
                        account: Some(profile),
                        profile: None,
                        offline: true,
                        reachability: Reachability::NotAttempted,
                        repositories: setup.store.github_repository_views(profile).unwrap(),
                    },
                    window,
                    cx,
                );
            })
        })
        .unwrap();
        wait_active(cx, "recent");
        cx.read(|cx| {
            let workspace = reader.read(cx);
            assert!(workspace.back.is_empty() && workspace.forward.is_empty());
            if profile == alternate {
                assert_eq!(workspace.active_tab().unwrap().item, alternate_item);
                assert!(workspace.drafts.is_empty());
            }
        });
    }
    open_item(cx, other);
    wait_active(cx, "unrelated");
    open_item(cx, item);
    wait_active(cx, "recent");
    let saved = setup.store.workspace(account).unwrap().unwrap();
    assert_eq!(saved.focused_pane, 0);
    assert_eq!(saved.panes[0].tabs[0].id, "recent");
    assert_eq!(saved.panes[0].tabs[2].id, "rightmost");
    assert_eq!(saved.panes[1].tabs[0].id, "opposite-pane");
    assert_eq!(
        saved.panes[0].tabs[0].view.draft.as_deref(),
        Some(draft.as_str())
    );
    assert_eq!(
        setup
            .store
            .draft(account, &draft)
            .unwrap()
            .unwrap()
            .content
            .body,
        "Preserve the original shared draft."
    );
}

#[gpui_kit::test]
fn ui_002_independent_panes_share_one_editor_and_reopen_same_draft(cx: &mut TestAppContext) {
    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    let item = setup.fixture.long_discussion;
    let anchored_comment = setup
        .store
        .discussion_page(account, item, None, 10)
        .unwrap()
        .records[0]
        .key
        .clone();
    let tab = Tab {
        id: "primary-note".into(),
        item,
        view: TabView {
            navigation: Some("issue-conversation".into()),
            scroll_anchor: Some(anchored_comment),
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
    let last_read_offset = 78;
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let workspace = reader.read(cx);
        let handle = &workspace.tab_views[&tab.id].scroll;
        assert!(handle.bounds().size.height > px(0.));
        handle.set_offset(point(px(0.), px(-(last_read_offset as f32))));
    })
    .unwrap();
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
    assert_eq!(reopened.view.scroll_anchor, tab.view.scroll_anchor);
    assert_eq!(reopened.view.scroll_offset, last_read_offset);
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).tab_views.contains_key(&tab.id))
    });
    cx.update_window(window, |_, window, cx| {
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert_eq!(
            reader.read(cx).tab_views[&tab.id].scroll.offset().y,
            px(-(last_read_offset as f32))
        );
    })
    .unwrap();
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

    let other = setup.fixture.reviewed_pr;
    cx.update_window(window, |_, window, cx| {
        reader.update(cx, |reader, cx| {
            reader.open(other, None, OpenTab::Beside, window, cx)
        });
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| {
            let workspace = reader.read(cx);
            workspace.layout.focused_pane == 0
                && !workspace.layout_busy
                && workspace.active_tab().is_some_and(|tab| {
                    tab.item == other && workspace.tab_views.contains_key(&tab.id)
                })
        })
    });
    click(cx, window, "reader-tab-1-1");
    until(cx, |cx| {
        cx.read(|cx| {
            let workspace = reader.read(cx);
            !workspace.layout_busy
                && workspace
                    .active_tab()
                    .is_some_and(|active| active.id == tab.id)
        })
    });
    click(cx, window, "reader-draft-comment-1");
    until(cx, |cx| {
        cx.read(|cx| {
            reader
                .read(cx)
                .drafts
                .get(&draft_id)
                .is_some_and(|editor| editor.saved() && editor.tab_id == tab.id)
        })
    });
    let original_draft = setup.store.draft(account, &draft_id).unwrap().unwrap();
    submit_then_focus_other_pane(
        &setup,
        cx,
        window,
        &reader,
        ("reader-submit-1", "reader-tab-0-0"),
        |workspace| workspace.drafts[&draft_id].submitting,
    );
    until(cx, |cx| {
        cx.read(|cx| {
            let workspace = reader.read(cx);
            !workspace.layout_busy
                && workspace.layout.focused_pane == 0
                && workspace.drafts[&draft_id].submitted.is_some()
        })
    });
    let after = setup.store.queue_page(account, 0, 100).unwrap();
    assert_eq!(&after[..queue_before.len()], queue_before.as_slice());
    assert_eq!(after.len(), queue_before.len() + 1);
    let queued = setup
        .store
        .operation(account, &after.last().unwrap().id)
        .unwrap()
        .unwrap();
    assert_eq!(queued.item, item);
    assert_eq!(queued.original_repository, setup.fixture.repository);
    assert_eq!(
        queued.draft,
        Some((draft_id.clone(), original_draft.revision))
    );
    assert_eq!(
        setup.store.write_intent(account, &queued.id).unwrap(),
        Some(threadline_storage::writes::WriteIntent::CreateComment {
            body: "Shared draft in one editor".into()
        })
    );
    let draft = setup.store.draft(account, &draft_id).unwrap().unwrap();
    assert_eq!(draft.content, original_draft.content);
    assert_eq!(draft.revision, original_draft.revision);
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
    let number = NonZeroU64::new(4281).unwrap();
    let title = "A downloaded title wider than the pane ".repeat(12);
    let title = title.trim_end();
    let account = setup.fixture.account;
    let long_title_item = setup
        .store
        .ingest(
            account,
            ItemLocator {
                repository: setup.fixture.repository,
                number: number.get(),
            },
            ItemUpdate {
                representation: ItemRepresentation::Issue(IssueRecord {
                    id: threadline_core::domain::IssueApiId::new(number),
                    node_id: None,
                    classification: IssueClassification::Issue,
                }),
                remote_updated_at: UtcMillis(1_900_000_000_000),
                title: Change::Value(title.into()),
                body: Change::Value("Downloaded content".into()),
                closed: Change::Value(false),
            },
        )
        .unwrap();
    let mut layout = setup.store.workspace(account).unwrap().unwrap();
    layout.focused_pane = 0;
    layout.panes[0].active = Some(0);
    layout.panes[0].tabs[1].item = long_title_item;
    setup.store.save_workspace(account, &layout).unwrap();
    let (window, reader) = setup.open(cx);
    until(cx, |cx| {
        cx.read(|cx| !reader.read(cx).list_busy && !reader.read(cx).rows.is_empty())
    });
    cx.read(|cx| {
        let reader = reader.read(cx);
        assert_eq!(reader.cached_active_panes(), 2);
        assert!(reader.initial_browse.is_none());
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
        "reader-tab-0-1",
        "reader-tab-1-0",
        "reader-close-tab",
    ] {
        let bounds = observation
            .test_control_bounds(control)
            .unwrap_or_else(|| panic!("missing visible {control}"));
        assert!(bounds[2] > 0. && bounds[3] > 0.);
    }
    let second_bounds = observation.test_control_bounds("reader-tab-0-1").unwrap();
    for (control, position) in [("reader-tab-0-1", 1), ("reader-tab-0-0", 0)] {
        click(cx, window, control);
        until(cx, |cx| {
            cx.read(|cx| {
                let reader = reader.read(cx);
                !reader.layout_busy && reader.layout.panes[0].active == Some(position)
            })
        });
        cx.update_window(window, |_, window, cx| {
            observation.begin_frame();
            window.render_frame(cx);
            assert_eq!(
                observation.test_control_bounds("reader-tab-0-1"),
                Some(second_bounds)
            );
            assert!(
                window
                    .find("reader-tab-0-1")
                    .label()
                    .unwrap()
                    .contains(title)
            );
        })
        .unwrap();
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
    assert!(observation.test_control_bounds("reader-result-0").is_none());
    assert!(
        observation
            .test_control_bounds("reader-maximize-pane")
            .is_some()
    );
    setup.runtime.block_on(writer.finish()).unwrap();
}

#[cfg(feature = "e2e")]
#[gpui_kit::test]
fn mixed_lifecycle_cache_loads_every_seeded_editor_in_the_original_panes(cx: &mut TestAppContext) {
    use threadline_storage::fixtures::populate_native_lifecycle;

    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_profile(directory.path()).unwrap());
    let fixture = populate_native_lifecycle(
        &store,
        12345,
        "00112233445566778899aabbccddeeff",
        1_900_000_000_000,
        || Ok(()),
    )
    .unwrap();
    super::lifecycle_fixture::seed_drafts(&store, fixture.account).unwrap();
    let layout = store.workspace(fixture.account).unwrap().unwrap();
    let review_id = layout.panes[1].tabs[0].id.clone();
    let setup = ReaderHarness {
        _directory: directory,
        store,
        fixture,
        runtime: tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .build()
            .unwrap(),
    };
    let (window, reader) = setup.open(cx);
    until(cx, |cx| {
        cx.read(|cx| {
            reader
                .read(cx)
                .review_editors
                .contains_key(&setup.fixture.reviewed_pr)
        })
    });
    cx.read(|cx| {
        let workspace = reader.read(cx);
        assert_eq!(
            workspace.review_editors[&setup.fixture.reviewed_pr].tab_id,
            review_id
        );
        assert_eq!(workspace.cached_active_panes(), 2);
        assert_eq!(workspace.layout.proportions, vec![0.43, 0.57]);
    });
    for (id, pane, index) in [
        ("reader-tab-0-0", 0, 0),
        ("reader-tab-0-1", 0, 1),
        ("reader-tab-1-1", 1, 1),
    ] {
        let tab = &layout.panes[pane].tabs[index];
        let draft = tab.view.draft.as_ref().unwrap();
        click(cx, window, id);
        until(cx, |cx| {
            cx.read(|cx| {
                let workspace = reader.read(cx);
                workspace
                    .active_tab()
                    .is_some_and(|active| active.id == tab.id)
                    && workspace
                        .drafts
                        .get(draft)
                        .is_some_and(|editor| editor.saved() && editor.submitted.is_none())
            })
        });
    }
    let drafts =
        super::lifecycle_fixture::draft_checkpoint(&setup.store, setup.fixture.account).unwrap();
    assert_eq!(drafts.as_array().unwrap().len(), 4);
    click(cx, window, "pr-1-checks");
    until(cx, |cx| {
        cx.read(|cx| {
            let workspace = reader.read(cx);
            let tab = workspace.active_tab().unwrap();
            workspace
                .tab_views
                .get(&tab.id)
                .and_then(|view| view.pr.as_ref())
                .is_some_and(|pr| {
                    pr.data.pages.iter().any(|(family, page)| {
                        *family == threadline_storage::pr::PrFamily::Checks
                            && page.state.coverage
                                == threadline_storage::resources::Coverage::Missing
                    })
                })
        })
    });
}

#[cfg(feature = "e2e")]
#[gpui_kit::test]
fn default_shared_pr_panes_keep_conversation_and_cached_thread_actions_reachable(
    cx: &mut TestAppContext,
) {
    use gpui_kit::{Modifiers, PlatformInput, ScrollDelta, ScrollEvent, ScrollSource, TouchPhase};
    use threadline_storage::{
        pr::{InlineComment, PrFamily, PrHeader, PrInputPage, PrRecord, PrSection, PrTabView},
        resources::{CommitOid, WalkMode},
    };

    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    let item = setup.fixture.reviewed_pr;
    let stamp = 1_900_000_000_000;
    let base = CommitOid::parse(&"a".repeat(40)).unwrap();
    let head = CommitOid::parse(&"b".repeat(40)).unwrap();
    let revision = setup
        .store
        .record_pr_revision(
            account,
            item,
            &base,
            &head,
            None,
            &PrHeader::default(),
            stamp,
        )
        .unwrap();
    let progress = setup
        .store
        .start_pr_fetch(&revision, PrFamily::Threads, WalkMode::Full)
        .unwrap();
    setup
        .store
        .commit_pr_page(
            &revision,
            PrFamily::Threads,
            &progress,
            PrInputPage {
                records: vec![PrRecord::Thread {
                    node_id: "PRRT_42".into(),
                    resolved: Some(false),
                    outdated: Some(false),
                    location: None,
                    comments: vec![InlineComment {
                        id: 771,
                        node_id: Some("PRRC_771".into()),
                        in_reply_to: None,
                        author: None,
                        actor: Default::default(),
                        body: "A long downloaded review note.\n".repeat(24),
                        created_at_ms: Some(stamp),
                        updated_at_ms: Some(stamp),
                        anchor: None,
                    }],
                    replies_complete: true,
                    reply_cursor: None,
                }],
                next_cursor: None,
                validators: None,
                partial: None,
                fetched_at_ms: stamp,
            },
        )
        .unwrap();
    let left = Tab {
        id: "cached-thread-conversation".into(),
        item,
        view: TabView {
            pr_reader: Some(PrTabView {
                section: PrSection::Conversation,
                ..Default::default()
            }),
            ..Default::default()
        },
    };
    let right = Tab {
        id: "other-pr-conversation".into(),
        ..left.clone()
    };
    setup
        .store
        .save_workspace(
            account,
            &Workspace {
                focused_pane: 0,
                proportions: vec![0.5, 0.5],
                panes: [left, right]
                    .into_iter()
                    .map(|tab| Pane {
                        active: Some(0),
                        tabs: vec![tab],
                    })
                    .collect(),
            },
        )
        .unwrap();
    let (window, reader) = setup.open_at(cx, size(px(1320.), px(820.)));
    until(cx, |cx| {
        cx.read(|cx| {
            reader
                .read(cx)
                .tab_views
                .contains_key("other-pr-conversation")
        })
    });
    let (observer, writer) = crate::observation::Observer::new(
        setup._directory.path().join("thread-bounds.json"),
        std::time::Instant::now(),
        false,
        setup.runtime.handle(),
    );
    cx.update_window(window, |_, window, cx| {
        reader.update(cx, |reader, cx| {
            reader.set_observer(Some(observer.clone()), cx)
        });
        observer.begin_frame();
        window.render_frame(cx);
        let [body_x, body_y, body_w, body_h] = observer
            .test_control_bounds("reader-conversation-0")
            .expect("The ordinary downloaded PR discussion must be visible");
        for index in 0..2 {
            let [_, _, _, discussion] = observer
                .test_control_bounds(&format!("reader-conversation-{index}"))
                .expect("The ordinary downloaded PR discussion must be visible");
            assert!(discussion > 24., "pane {index} has no readable body");
        }
        let (left_scroll, right_scroll) = {
            let workspace = reader.read(cx);
            (
                workspace.tab_views["cached-thread-conversation"]
                    .scroll
                    .clone(),
                workspace.tab_views["other-pr-conversation"].scroll.clone(),
            )
        };
        window.dispatch_event(
            PlatformInput::Scroll(ScrollEvent {
                position: Point::new(px(body_x + body_w / 2.), px(body_y + body_h / 2.)),
                delta: ScrollDelta::Lines(Point::new(0., -3.)),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
                source: ScrollSource::Pointer,
            }),
            cx,
        );
        observer.begin_frame();
        window.render_frame(cx);
        assert!(left_scroll.offset().y < px(0.));
        assert_eq!(right_scroll.offset().y, px(0.));
        let [x, y, w, h] = observer
            .test_control_bounds("pr-0-conversation-records")
            .unwrap();
        let position = Point::new(px(x + w / 2.), px(y + h / 2.));
        assert!(
            observer
                .test_control_bounds("pr-0-thread-0-reply")
                .is_none()
        );
        let controls = ["pr-0-thread-0-reply", "pr-0-thread-0-resolution"];
        let mut seen = [false; 2];
        for _ in 0..16 {
            window.dispatch_event(
                PlatformInput::Scroll(ScrollEvent {
                    position,
                    delta: ScrollDelta::Lines(Point::new(0., -3.)),
                    modifiers: Modifiers::default(),
                    touch_phase: TouchPhase::Moved,
                    source: ScrollSource::Pointer,
                }),
                cx,
            );
            observer.begin_frame();
            window.render_frame(cx);
            for (visible, id) in seen.iter_mut().zip(controls) {
                *visible |= observer.test_control_bounds(id).is_some();
            }
            if seen.iter().all(|visible| *visible) {
                break;
            }
        }
        assert_eq!(seen, [true, true], "A saved thread action remained clipped");
    })
    .unwrap();
    setup.runtime.block_on(writer.finish()).unwrap();
}

#[gpui_kit::test]
fn ui_001_pr_context_changes_keep_the_immutable_native_diff_and_each_tabs_state(
    cx: &mut TestAppContext,
) {
    exercise_pr_context(cx, false);
}

#[gpui_kit::test]
fn ui_004_anchored_review_draft_survives_pr_context_changes_and_queues_the_original_head(
    cx: &mut TestAppContext,
) {
    exercise_pr_context(cx, true);
}

#[gpui_kit::test]
fn ui_004_existing_threads_follow_verified_source_lines_in_both_diff_modes(
    cx: &mut TestAppContext,
) {
    use gpui_kit::component::diff::{DiffLineRange, DiffMode, DiffSide};
    use threadline_storage::{
        pr::{
            InlineComment, PatchEvidence, PrFamily, PrHeader, PrInputPage, PrRecord, PrTabView,
            ReviewAnchor, ReviewThreadLocation, SourceSide,
        },
        resources::{CommitOid, WalkMode},
    };

    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    let item = setup.fixture.reviewed_pr;
    let stamp = 1_900_000_000_000;
    let oid = |letter: &str| CommitOid::parse(&letter.repeat(40)).unwrap();
    let revision = setup
        .store
        .record_pr_revision(
            account,
            item,
            &oid("a"),
            &oid("b"),
            Some(&oid("c")),
            &PrHeader::default(),
            stamp,
        )
        .unwrap();
    let path = "current.rs";
    let line = |side, line| {
        Some(ReviewThreadLocation::Line {
            path: path.into(),
            side,
            line,
        })
    };
    let comment = |id, in_reply_to, body: &str, side| InlineComment {
        id,
        node_id: Some(format!("review-{id}")),
        in_reply_to,
        author: Some("Fixture reviewer".into()),
        actor: Default::default(),
        body: body.into(),
        created_at_ms: Some(stamp),
        updated_at_ms: Some(stamp),
        anchor: Some(ReviewAnchor {
            path: "original.rs".into(),
            side,
            line: 4,
            original_oid: oid("d").as_str().to_owned(),
        }),
    };
    let thread = |id: &str, location, outdated, comments| PrRecord::Thread {
        node_id: id.into(),
        resolved: Some(false),
        outdated,
        location,
        comments,
        replies_complete: true,
        reply_cursor: None,
    };
    let threads = vec![
        thread(
            "right",
            line(SourceSide::Right, 17),
            Some(false),
            vec![
                comment(610, None, "Moved root on modified line.", SourceSide::Right),
                comment(
                    611,
                    Some(610),
                    "Downloaded reply on the same thread.",
                    SourceSide::Right,
                ),
            ],
        ),
        thread(
            "left",
            line(SourceSide::Left, 9),
            Some(false),
            vec![comment(
                620,
                None,
                "Deletion-side thread.",
                SourceSide::Left,
            )],
        ),
        thread(
            "outdated",
            line(SourceSide::Right, 17),
            Some(true),
            vec![comment(630, None, "Outdated body.", SourceSide::Right)],
        ),
        thread(
            "file",
            Some(ReviewThreadLocation::File { path: path.into() }),
            Some(false),
            vec![comment(640, None, "File-level body.", SourceSide::Right)],
        ),
        thread(
            "unavailable",
            line(SourceSide::Right, 99),
            Some(false),
            vec![comment(
                650,
                None,
                "Undownloaded source line.",
                SourceSide::Right,
            )],
        ),
    ];
    let files = vec![PrRecord::File {
        path: path.into(),
        previous_path: Some("original.rs".into()),
        status: "renamed".into(),
        additions: Some(1),
        deletions: Some(1),
        patch: PatchEvidence::Text("@@ -9,2 +17,2 @@\n-old\n+new\n context".into()),
    }];
    for (family, records) in [(PrFamily::Files, files), (PrFamily::Threads, threads)] {
        let progress = setup
            .store
            .start_pr_fetch(&revision, family, WalkMode::Full)
            .unwrap();
        setup
            .store
            .commit_pr_page(
                &revision,
                family,
                &progress,
                PrInputPage {
                    records,
                    next_cursor: None,
                    validators: None,
                    partial: None,
                    fetched_at_ms: stamp,
                },
            )
            .unwrap();
    }
    setup.one_pane(&Tab {
        id: "verified-thread-source".into(),
        item,
        view: TabView {
            pr_reader: Some(PrTabView {
                context_collapsed: true,
                ..Default::default()
            }),
            ..Default::default()
        },
    });
    let (window, reader) = setup.open(cx);
    let check = |cx: &mut TestAppContext, mode| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let state = reader.read(cx);
            let tab = state.active_tab().unwrap();
            let pr = state.tab_views[&tab.id].pr.as_ref().unwrap();
            let diff = pr.diff.as_ref().unwrap().clone();
            assert_eq!(diff.read(cx).mode(), mode);
            let file = pr.data.files.as_ref().unwrap().selected.as_ref().unwrap();
            let annotations = state.review_annotations(tab, pr, file);
            assert!(annotations.unavailable);
            assert_eq!(annotations.rows.len(), 2);
            assert_eq!(
                window.find("pr-0-thread-placement").role(),
                Some(gpui_kit::Role::Status)
            );
            for (id, side, line, text) in [
                ("thread:left", DiffSide::Original, 9, "old"),
                ("thread:right", DiffSide::Modified, 17, "new"),
            ] {
                let marker = annotations
                    .rows
                    .iter()
                    .find(|row| row.id() == &id.into())
                    .unwrap();
                let source = marker.position().unwrap();
                assert_eq!(
                    (source.path().as_ref(), source.side(), source.line()),
                    (path, side, line)
                );
                diff.update(cx, |state, cx| {
                    state.set_selected_lines(Some(DiffLineRange::new(path, side, line, line)), cx);
                });
                assert_eq!(diff.read(cx).selected_text(cx).trim(), text);
            }
            let right = &annotations.labels[&"thread:right".into()];
            assert!(right.contains("Moved root on modified line."));
            assert!(right.contains("Downloaded reply on the same thread."));
        })
        .unwrap();
    };
    check(cx, DiffMode::Unified);
    click(cx, window, "pr-0-diff-split");
    until(cx, |cx| cx.read(|cx| !reader.read(cx).layout_busy));
    check(cx, DiffMode::Split);
}

fn exercise_pr_context(cx: &mut TestAppContext, review: bool) {
    use gpui_kit::component::diff::{DiffLineRange, DiffMode, DiffSide};
    use std::num::NonZeroU64;
    use threadline_storage::{
        ItemLocator,
        pr::{
            PatchEvidence, PrContextMode, PrFamily, PrHeader, PrInputPage, PrRecord, PrSection,
            ReferenceObservation, SourceSide,
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
                body: Change::Value(
                    "Review introduction\n\nFixes #101. Resolves #102. Closes #103.".into(),
                ),
                closed: Change::Value(false),
            },
        )
        .unwrap();
    let issue_ids = (101..=103)
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
    let verify = |index: usize| {
        let (item, issue_id) = issue_ids[index];
        store
            .record_pr_reference(
                account,
                pr,
                body_revision,
                candidates[index].position,
                ReferenceObservation::Item {
                    repository,
                    item,
                    issue_id,
                },
                stamp + 1,
            )
            .unwrap();
    };
    verify(0);
    verify(1);
    if review {
        store.sign_out_github_account(account, stamp + 2).unwrap();
    }
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
                        additions: Some(2),
                        deletions: Some(2),
                        patch: PatchEvidence::Text(
                            "@@ -10,2 +20,2 @@\n unchanged\n-old line\n+new line\n@@ -30 +40 @@\n-old second\n+new second".into(),
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
    let (window, reader) = if review {
        setup.open_at(cx, size(px(2200.), px(1450.)))
    } else {
        setup.open(cx)
    };
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
    until(cx, |cx| cx.read(|cx| !reader.read(cx).layout_busy));
    click(cx, window, "pr-0-next-change");
    until(cx, |cx| {
        cx.read(|cx| {
            let reader = reader.read(cx);
            !reader.layout_busy
                && pr::tab_settings(&reader.layout.panes[0].tabs[0])
                    .code_anchor
                    .is_some_and(|anchor| {
                        anchor.line == 21
                            && anchor.path == filenames[1]
                            && anchor.side == SourceSide::Right
                            && anchor.original_oid == revision.head.as_str()
                    })
        })
    });
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
            assert_eq!(diff.read(cx).mode(), DiffMode::Split);
            assert_eq!(diff.read(cx).selected_lines().unwrap().start(), 21);
            assert!(diff.read(cx).selected_text(cx).contains("new line"));
            diff
        })
        .unwrap();
    until(cx, |cx| cx.read(|cx| !reader.read(cx).layout_busy));
    cx.update_window(window, |_, _, cx| {
        diff.update(cx, |diff, cx| diff.set_selected_lines(None, cx));
    })
    .unwrap();
    for (action, line) in [("pr-0-next-change", 40), ("pr-0-previous-change", 21)] {
        click(cx, window, action);
        until(cx, |cx| {
            cx.read(|cx| {
                let reader = reader.read(cx);
                !reader.layout_busy
                    && pr::tab_settings(&reader.layout.panes[0].tabs[0])
                        .code_anchor
                        .is_some_and(|anchor| anchor.line == line)
            })
        });
    }
    let draft_id = format!("review-{}", pr.0);
    let original_review = if review {
        click(cx, window, "pr-0-review-add-inline");
        until(cx, |cx| {
            cx.read(|cx| {
                let reader = reader.read(cx);
                !reader.layout_busy
                    && reader
                        .review_editors
                        .get(&pr)
                        .is_some_and(|editor| editor.inline.len() == 1)
            })
        });
        cx.update_window(window, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
            let editor = &reader.read(cx).review_editors[&pr];
            assert!(editor.inline[0].input.focus_handle(cx).is_focused(window));
            window.input("One anchored review note", cx);
        })
        .unwrap();
        until(cx, |cx| {
            cx.read(|cx| reader.read(cx).review_editors[&pr].saved())
        });
        let saved = store.review_draft(account, &draft_id).unwrap().unwrap();
        assert_eq!(saved.revision_id, revision.id);
        let inline = &saved.draft.comments[0];
        assert_eq!(inline.body, "One anchored review note");
        assert_eq!(
            inline.anchor.side,
            threadline_storage::pr::SourceSide::Right
        );
        assert_eq!(inline.anchor.path, filenames[1]);
        assert_eq!(inline.anchor.first_line, 21);
        assert_eq!(inline.anchor.last_line, 21);
        assert_eq!(inline.anchor.source_oid, revision.head.as_str());

        click(cx, window, "pr-0-review-toggle-editor");
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(reader.read(cx).review_editors[&pr].collapsed);
            assert!(window.try_find("pr-0-review-inline").is_none());
            assert!(window.try_find("pr-0-next-change").is_some());
        })
        .unwrap();
        assert_eq!(store.review_draft(account, &draft_id).unwrap(), Some(saved));
        click(cx, window, "pr-0-next-change");
        until(cx, |cx| {
            cx.read(|cx| {
                let reader = reader.read(cx);
                !reader.layout_busy
                    && diff
                        .read(cx)
                        .selected_lines()
                        .is_some_and(|line| line.start() == 40)
            })
        });
        click(cx, window, "pr-0-review-add-inline");
        until(cx, |cx| {
            cx.read(|cx| {
                let editor = &reader.read(cx).review_editors[&pr];
                !editor.collapsed && editor.inline.len() == 2 && editor.focused_inline == Some(1)
            })
        });
        cx.update_window(window, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
            let editor = &reader.read(cx).review_editors[&pr];
            assert!(editor.inline[1].input.focus_handle(cx).is_focused(window));
            window.input("Second anchored review note", cx);
        })
        .unwrap();
        until(cx, |cx| {
            cx.read(|cx| reader.read(cx).review_editors[&pr].saved())
        });
        let saved = store.review_draft(account, &draft_id).unwrap().unwrap();
        assert_eq!(saved.draft.comments.len(), 2);
        assert_eq!(saved.draft.comments[0].body, "One anchored review note");
        assert_eq!(saved.draft.comments[1].body, "Second anchored review note");
        assert_eq!(saved.draft.comments[1].anchor.first_line, 40);
        assert_eq!(
            saved.draft.comments[1].anchor.source_oid,
            revision.head.as_str()
        );
        cx.update_window(window, |_, _, cx| {
            diff.update(cx, |state, cx| {
                state.set_selected_lines(
                    Some(DiffLineRange::new(filenames[1], DiffSide::Modified, 21, 21)),
                    cx,
                );
            });
            assert!(diff.read(cx).selected_text(cx).contains("new line"));
        })
        .unwrap();
        click(cx, window, "pr-0-context-more");
        until(cx, |cx| cx.read(|cx| !reader.read(cx).layout_busy));
        Some(saved)
    } else {
        None
    };
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
    if let Some(original_review) = original_review {
        assert_eq!(
            reopened.review_draft(account, &draft_id).unwrap(),
            Some(original_review.clone())
        );
        cx.read(|cx| {
            let reader = reader.read(cx);
            assert_eq!(reader.review_editors.len(), 1);
            assert_eq!(reader.review_editors[&pr].id(), draft_id);
        });
        click(cx, window, "pr-1-review-open");
        until(cx, |cx| {
            cx.read(|cx| {
                let reader = reader.read(cx);
                !reader.layout_busy && reader.layout.focused_pane == 0
            })
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                reader.read(cx).review_editors[&pr].tab_id,
                tab.id,
                "the other view never becomes the editor owner",
            );
        })
        .unwrap();
        click(cx, window, "reader-maximize-pane");
        until(cx, |cx| {
            cx.read(|cx| reader.read(cx).maximized && !reader.read(cx).layout_busy)
        });
        click(cx, window, "reader-maximize-pane");
        until(cx, |cx| {
            cx.read(|cx| !reader.read(cx).maximized && !reader.read(cx).layout_busy)
        });
        let other = issue_ids[0].0;
        cx.update_window(window, |_, window, cx| {
            reader.update(cx, |reader, cx| {
                reader.open(other, None, OpenTab::Beside, window, cx)
            });
        })
        .unwrap();
        until(cx, |cx| {
            cx.read(|cx| {
                let workspace = reader.read(cx);
                workspace.layout.focused_pane == 1
                    && !workspace.layout_busy
                    && workspace.active_tab().is_some_and(|tab| {
                        tab.item == other && workspace.tab_views.contains_key(&tab.id)
                    })
            })
        });
        click(cx, window, "reader-tab-0-0");
        until(cx, |cx| {
            cx.read(|cx| {
                let workspace = reader.read(cx);
                workspace.layout.focused_pane == 0
                    && !workspace.layout_busy
                    && workspace.review_editors[&pr].saved()
            })
        });
        let operations_before = store.queue_page(account, 0, 100).unwrap().len();
        submit_then_focus_other_pane(
            &setup,
            cx,
            window,
            &reader,
            ("pr-0-review-submit", "reader-tab-1-1"),
            |workspace| workspace.review_editors[&pr].sending,
        );
        until(cx, |cx| {
            cx.read(|cx| {
                let workspace = reader.read(cx);
                !workspace.layout_busy
                    && workspace.layout.focused_pane == 1
                    && workspace.review_editors[&pr].submitted.is_some()
            })
        });
        let operations = store.queue_page(account, 0, 100).unwrap();
        assert_eq!(operations.len(), operations_before + 1);
        let operation = store
            .operation(account, &operations.last().unwrap().id)
            .unwrap()
            .unwrap();
        assert_eq!(operation.item, pr);
        assert_eq!(operation.original_repository, repository);
        assert_eq!(
            operation.draft,
            Some((draft_id.clone(), original_review.revision))
        );
        assert_eq!(operation.state, threadline_storage::queue::State::Queued);
        let intent: threadline_storage::reviews::ReviewIntent =
            serde_json::from_str(&operation.snapshot.payload).unwrap();
        assert_eq!(
            intent,
            threadline_storage::reviews::ReviewIntent::Review(original_review.draft.clone())
        );
        assert_eq!(operation.snapshot.base, revision.head.as_str());
        assert_eq!(
            store.review_draft(account, &draft_id).unwrap(),
            Some(original_review)
        );
    }
    if !review {
        cx.update_window(window, |_, window, cx| {
            reader.update(cx, |reader, cx| {
                reader.open(issue_ids[0].0, None, OpenTab::CurrentPane, window, cx);
            });
        })
        .unwrap();
        until(cx, |cx| {
            cx.read(|cx| {
                reader
                    .read(cx)
                    .active_tab()
                    .is_some_and(|tab| tab.item == issue_ids[0].0)
            })
        });
        verify(2);
        cx.update_window(window, |_, window, cx| {
            reader.update(cx, |reader, cx| reader.reload_pr(pr, window, cx));
        })
        .unwrap();
        until(cx, |cx| {
            cx.read(|cx| {
                let reader = reader.read(cx);
                reader
                    .active_tab()
                    .is_some_and(|tab| tab.item == issue_ids[0].0)
                    && reader.tab_views[&tab.id]
                        .pr
                        .as_ref()
                        .and_then(|pr| pr.data.references.as_ref())
                        .is_some_and(|(_, rows)| rows[2].verified_item == Some(issue_ids[2].0))
            })
        });
        click(cx, window, "reader-tab-0-0");
        until(cx, |cx| {
            cx.read(|cx| {
                reader
                    .read(cx)
                    .active_tab()
                    .is_some_and(|tab| tab.item == pr)
            })
        });
    }
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
fn ui_006_failed_enqueue_keeps_the_saved_comment_and_reports_the_failure(cx: &mut TestAppContext) {
    use rusqlite::{Connection, TransactionBehavior};
    use threadline_storage::{queue::State, writes::WriteIntent};

    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    let item = setup.fixture.long_discussion;
    setup.one_pane(&Tab {
        id: "comment-queue-failure".into(),
        item,
        view: TabView::default(),
    });
    let previous = setup.store.queue_page(account, 0, 100).unwrap();
    let projections = setup.store.projections(account, item).unwrap();
    let (window, reader) = setup.open(cx);
    click(cx, window, "reader-draft-comment-0");
    let draft_id = format!("comment-{}", item.0);
    until(cx, |cx| {
        cx.read(|cx| {
            let reader = reader.read(cx);
            !reader.layout_busy && reader.drafts.contains_key(&draft_id)
        })
    });
    let body = "Keep this Markdown when the database cannot queue the comment.";
    cx.update_window(window, |_, window, cx| {
        let edit = reader.read(cx).drafts[&draft_id].view.clone();
        edit.focus_handle(cx).focus(window, cx);
        window.render_frame(cx);
        window.input(body, cx);
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| {
            let editor = &reader.read(cx).drafts[&draft_id];
            editor.saved() && editor.body == body
        })
    });
    let saved = setup.store.draft(account, &draft_id).unwrap().unwrap();
    assert_eq!(saved.content.body, body);

    let mut other_writer = Connection::open(setup._directory.path().join("s04-ui.sqlite")).unwrap();
    let reservation = other_writer
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    click(cx, window, "reader-submit-0");
    until(cx, |cx| {
        cx.read(|cx| {
            reader.read(cx).status
                == Some("The action could not be queued. Your local draft is still saved.")
        })
    });
    cx.update_window(window, |_, _, cx| {
        let reader = reader.read(cx);
        let editor = &reader.drafts[&draft_id];
        assert_eq!(editor.view.read(cx).value().as_ref(), body);
        assert!(editor.submitted.is_none());
        assert!(
            editor.saved(),
            "The lease heartbeat must not invalidate saved text"
        );
        assert!(!editor.submitting && !editor.lease_lost);
    })
    .unwrap();
    assert_eq!(setup.store.queue_page(account, 0, 100).unwrap(), previous);
    assert_eq!(setup.store.projections(account, item).unwrap(), projections);
    assert_eq!(
        setup
            .store
            .draft(account, &draft_id)
            .unwrap()
            .unwrap()
            .content,
        saved.content
    );
    drop(reservation);

    click(cx, window, "reader-submit-0");
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).drafts[&draft_id].submitted.is_some())
    });
    let operations = setup.store.queue_page(account, 0, 100).unwrap();
    assert_eq!(operations.len(), previous.len() + 1);
    let queued = operations.last().unwrap();
    assert_eq!(queued.item, item);
    assert_eq!(queued.state, State::Queued);
    assert_eq!(
        setup.store.write_intent(account, &queued.id).unwrap(),
        Some(WriteIntent::CreateComment { body: body.into() })
    );

    let tab = "comment-queue-failure";
    until(cx, |cx| {
        cx.read(|cx| {
            let reader = reader.read(cx);
            !reader.layout_busy
                && reader.tab_views[tab]
                    .data
                    .projections
                    .iter()
                    .any(|entry| entry.operation_id == queued.id)
        })
    });
    let index = cx.read(|cx| {
        reader.read(cx).tab_views[tab]
            .data
            .projections
            .iter()
            .position(|entry| entry.operation_id == queued.id)
            .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        use gpui_kit::{
            Modifiers, PlatformInput, ScrollDelta, ScrollEvent, ScrollSource, TouchPhase,
        };
        window.render_frame(cx);
        let viewport = window.find("reader-conversation-0").bounds();
        window.dispatch_event(
            PlatformInput::Scroll(ScrollEvent {
                position: viewport.center(),
                delta: ScrollDelta::Lines(Point::new(0., -150.)),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
                source: ScrollSource::Pointer,
            }),
            cx,
        );
        window.render_frame(cx);
        window.click(format!("reader-edit-pending-comment-0-{index}"), cx);
    })
    .unwrap();
    let followup = format!("comment-edit-{}-{}", item.0, queued.id);
    until(cx, |cx| {
        cx.read(|cx| {
            let reader = reader.read(cx);
            reader
                .active_tab()
                .is_some_and(|tab| tab.view.draft.as_deref() == Some(&followup))
                && reader
                    .drafts
                    .get(&followup)
                    .is_some_and(|editor| editor.comment_edit && editor.body == body)
        })
    });
}

#[gpui_kit::test]
fn ui_006_reopened_issue_draft_uses_the_durable_submission_for_its_exact_revision(
    cx: &mut TestAppContext,
) {
    use threadline_storage::queue::{CancelResult, DraftSubmissionStatus, State};

    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    let item = setup.fixture.long_discussion;
    let draft_id = format!("issue-{}", item.0);
    setup.one_pane(&Tab {
        id: "submitted-issue-draft".into(),
        item,
        view: TabView::default(),
    });
    let existing = setup.store.queue_page(account, 0, 100).unwrap().len();
    let (window, reader) = setup.open(cx);
    click(cx, window, "reader-edit-issue-0");
    until(cx, |cx| {
        cx.read(|cx| !reader.read(cx).layout_busy && reader.read(cx).drafts.contains_key(&draft_id))
    });

    let type_body =
        |cx: &mut TestAppContext, window, reader: &Entity<ReaderWorkspace>, text: &'static str| {
            cx.update_window(window, |_, window, cx| {
                let editor = reader.read(cx).drafts[&draft_id].view.clone();
                editor.focus_handle(cx).focus(window, cx);
                window.render_frame(cx);
                window.input(text, cx);
            })
            .unwrap();
            until(cx, |cx| {
                cx.read(|cx| {
                    let editor = &reader.read(cx).drafts[&draft_id];
                    editor.saved() && editor.body.contains(text)
                })
            });
        };
    type_body(cx, window, &reader, "Use the same issue after reopening.");
    click(cx, window, "reader-submit-0");
    until(cx, |cx| {
        cx.read(|cx| {
            reader.read(cx).drafts[&draft_id]
                .current_submission()
                .is_some()
        })
    });
    let operation = setup
        .store
        .queue_page(account, 0, 100)
        .unwrap()
        .remove(existing);
    let version = setup
        .store
        .draft(account, &draft_id)
        .unwrap()
        .unwrap()
        .revision;

    let (finished, wait) = std::sync::mpsc::channel();
    cx.update_window(window, |_, window, cx| {
        reader.update(cx, |reader, cx| {
            reader.request_departure(
                true,
                move |_, _| {
                    finished.send(()).unwrap();
                },
                window,
                cx,
            );
        });
    })
    .unwrap();
    until(cx, |_| wait.try_recv().is_ok());

    let (window, reader) = setup.open(cx);
    until(cx, |cx| {
        cx.read(|cx| {
            reader
                .read(cx)
                .drafts
                .get(&draft_id)
                .and_then(drafts::Editor::current_submission)
                == Some(DraftSubmissionStatus::queued(version))
        })
    });
    assert_eq!(
        setup.store.queue_page(account, 0, 100).unwrap().len(),
        existing + 1
    );

    assert_eq!(
        setup
            .store
            .cancel_operation(account, &operation.id)
            .unwrap(),
        CancelResult::Cancelled
    );
    cx.update_window(window, |_, window, cx| {
        reader.update(cx, |reader, cx| {
            reader.refresh_draft_submissions(window, cx)
        });
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| {
            reader.read(cx).drafts[&draft_id]
                .current_submission()
                .is_some_and(|status| outbox::draft_submission_label(status) == "Cancelled")
        })
    });
    type_body(cx, window, &reader, "A newer edit is still a draft.");
    cx.read(|cx| {
        let editor = &reader.read(cx).drafts[&draft_id];
        assert!(editor.durable_revision > version);
        assert!(editor.current_submission().is_none());
    });
    assert_eq!(
        setup
            .store
            .operation(account, &operation.id)
            .unwrap()
            .unwrap()
            .state,
        State::Cancelled
    );
    assert_eq!(
        outbox::draft_submission_label(DraftSubmissionStatus {
            revision: version,
            state: State::OutcomeUnknown,
            cancel_requested: true,
        }),
        "Outcome unknown · cancellation requested"
    );
}

#[cfg(feature = "e2e")]
#[gpui_kit::test]
fn issue_conversation_observation_tracks_the_viewport_without_moving_it(cx: &mut TestAppContext) {
    let setup = ReaderHarness::seeded();
    setup.one_pane(&Tab {
        id: "conversation-viewport".into(),
        item: setup.fixture.long_discussion,
        view: TabView::default(),
    });
    let (window, reader) = setup.open(cx);
    let (observer, _writer) = crate::observation::Observer::new(
        setup._directory.path().join("conversation-bounds.json"),
        std::time::Instant::now(),
        false,
        setup.runtime.handle(),
    );
    cx.update_window(window, |_, window, cx| {
        use gpui_kit::{
            Modifiers, PlatformInput, ScrollDelta, ScrollEvent, ScrollSource, TouchPhase,
        };

        let id = "reader-conversation-0";
        window.render_frame(cx);
        let before = window.find(id).bounds();
        let logical = [
            before.origin.x,
            before.origin.y,
            before.size.width,
            before.size.height,
        ]
        .map(|value| value.as_f32());
        assert!(logical[2] > 0. && logical[3] > 0.);
        reader.update(cx, |reader, cx| {
            reader.set_observer(Some(observer.clone()), cx)
        });
        observer.begin_frame();
        window.render_frame(cx);
        assert_eq!(window.find(id).bounds(), before);
        assert_eq!(observer.test_control_bounds(id), Some(logical));

        window.dispatch_event(
            PlatformInput::Scroll(ScrollEvent {
                position: before.center(),
                delta: ScrollDelta::Lines(Point::new(0., -40.)),
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Moved,
                source: ScrollSource::Pointer,
            }),
            cx,
        );
        observer.begin_frame();
        window.render_frame(cx);
        assert_eq!(observer.test_control_bounds(id), Some(logical));
    })
    .unwrap();
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

#[gpui_kit::test]
fn ui_005_offline_merge_targets_the_confirmed_pane_and_updates_the_outbox_badge(
    cx: &mut TestAppContext,
) {
    use threadline_storage::{
        access::{AccessState, DiscoveryState, RepositoryObservation, RepositoryObservationSource},
        merges::{Gate, MergeMethod, MergePolicy, QueueRequirement},
        pr::PrHeader,
        queue::{ActionKind, State},
        resources::CommitOid,
        writes::Permission,
    };
    let setup = ReaderHarness::seeded();
    let store = &setup.store;
    let fixture = &setup.fixture;
    let account = fixture.account;
    let pr = fixture.reviewed_pr;
    let now = now_ms();
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
            now,
        )
        .unwrap();
    let repository = store
        .github_repository_views(account)
        .unwrap()
        .into_iter()
        .find(|repo| repo.id == fixture.repository)
        .unwrap();
    store
        .observe_github_repository(
            &profile,
            &RepositoryObservation {
                provider_id: repository.provider_id,
                owner: &repository.owner,
                name: &repository.name,
                private: true,
                source: RepositoryObservationSource::Direct,
            },
            now,
        )
        .unwrap();
    let oid = |byte: &str| CommitOid::parse(&byte.repeat(40)).unwrap();
    let revision = store
        .record_pr_revision(
            account,
            pr,
            &oid("a"),
            &oid("b"),
            Some(&oid("c")),
            &PrHeader::default(),
            now,
        )
        .unwrap();
    let policy = MergePolicy {
        allowed_methods: Some([MergeMethod::Squash].into_iter().collect()),
        viewer_permission: Permission::Allowed,
        open: Some(true),
        draft: Some(false),
        merged: Some(false),
        mergeable: Some(true),
        queue: QueueRequirement::NotRequired,
        checks: Gate::Satisfied,
        reviews: Gate::Satisfied,
        branch_rules: Gate::Satisfied,
    };
    store.set_dispatch_controls(account, false, false).unwrap();
    let observed = store
        .github_access_profile(account, &profile.owner_scope)
        .unwrap()
        .unwrap();
    store
        .record_merge_policy(&observed, &revision, &policy, now)
        .unwrap();
    store.sign_out_github_account(account, now + 1).unwrap();
    store.set_dispatch_controls(account, true, false).unwrap();
    let tabs = [
        Tab {
            id: "merge-bound-pr".into(),
            item: pr,
            view: TabView::default(),
        },
        Tab {
            id: "other-pane-issue".into(),
            item: fixture.long_discussion,
            view: TabView::default(),
        },
    ];
    store
        .save_workspace(
            account,
            &Workspace {
                focused_pane: 0,
                proportions: vec![0.5, 0.5],
                panes: tabs
                    .iter()
                    .cloned()
                    .map(|tab| Pane {
                        active: Some(0),
                        tabs: vec![tab],
                    })
                    .collect(),
            },
        )
        .unwrap();

    let before = store
        .sidebar_counts(account, ReaderScope::Selected)
        .unwrap()
        .outbox;
    let (window, reader) = setup.open(cx);
    click(cx, window, "pr-0-merge-open");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("pr-0-merge-merge").is_none());
        assert!(window.try_find("pr-0-merge-rebase").is_none());
        assert!(window.try_find("pr-1-merge-open").is_none());
        assert!(window.try_find("pr-0-merge-confirm").is_none());
    })
    .unwrap();
    click(cx, window, "pr-0-merge-squash");
    click(cx, window, "reader-tab-1-0");
    until(cx, |cx| {
        cx.read(|cx| {
            let state = reader.read(cx);
            !state.layout_busy && state.layout.focused_pane == 1
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("pr-0-merge-confirm").is_some());
    })
    .unwrap();
    click(cx, window, "reader-tab-0-0");
    until(cx, |cx| {
        cx.read(|cx| {
            let workspace = reader.read(cx);
            !workspace.layout_busy && workspace.layout.focused_pane == 0
        })
    });
    submit_then_focus_other_pane(
        &setup,
        cx,
        window,
        &reader,
        ("pr-0-merge-confirm", "reader-tab-1-0"),
        |workspace| workspace.merge_views[&tabs[0].id].busy,
    );
    until(cx, |cx| {
        cx.read(|cx| {
            let workspace = reader.read(cx);
            !workspace.layout_busy
                && workspace.layout.focused_pane == 1
                && workspace
                    .counts
                    .is_some_and(|counts| counts.outbox == before + 1)
        })
    });
    let statuses = store.merge_statuses(account, pr).unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].state, State::Queued);
    assert_eq!(statuses[0].method, MergeMethod::Squash);
    assert_eq!(statuses[0].head, revision.head);
    assert_eq!(statuses[0].revision_id, revision.id);
    assert!(
        store
            .merge_statuses(account, fixture.long_discussion)
            .unwrap()
            .is_empty()
    );
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let bounds = window.find("reader-outbox-badge").bounds();
        assert_eq!(bounds.size.width, bounds.size.height);
        assert!(bounds.size.width > px(23.));
    })
    .unwrap();
    let page = store.queue_page(account, 0, 100).unwrap();
    let (index, row) = page
        .iter()
        .enumerate()
        .find(|(_, row)| row.kind == ActionKind::Merge && row.item == pr)
        .unwrap();
    let operation = store.operation(account, &row.id).unwrap().unwrap();
    assert_eq!(operation.original_repository, fixture.repository);
    assert_eq!(
        operation.original_number,
        store.item(account, pr).unwrap().unwrap().locator.number
    );
    assert_eq!(operation.snapshot.base, revision.head.as_str());
    click(cx, window, "reader-outbox");
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).queue.iter().any(|q| q.id == row.id))
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(format!("reader-outbox-action-{index}"), cx);
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).outbox.selected_state().as_deref() == Some("Queued"))
    });
    click(cx, window, "reader-outbox-cancel");
    until(cx, |cx| {
        cx.read(|cx| {
            reader
                .read(cx)
                .counts
                .is_some_and(|counts| counts.outbox == before)
        })
    });
    assert_eq!(
        store.merge_statuses(account, pr).unwrap()[0].state,
        State::Cancelled
    );
    assert_ne!(
        store.item(account, pr).unwrap().unwrap().closed,
        Change::Value(true)
    );
}

#[gpui_kit::test]
fn history_view_selects_exact_parents_and_restores_an_offline_comparison(cx: &mut TestAppContext) {
    use gpui_kit::component::diff::{DiffLineRange, DiffSide};
    use threadline_storage::{
        access::{AccessState, DiscoveryState, RepositoryObservation, RepositoryObservationSource},
        history::{
            ComparisonOptions, HistoryComparison, HistoryDiffInput, HistoryEntry, HistoryRef,
        },
        pr::{PatchEvidence, PrRecord},
        resources::{CommitOid, ComparisonMode, GraphCommit},
    };
    let setup = ReaderHarness::seeded();
    let store = &setup.store;
    let account = setup.fixture.account;
    let repository = setup.fixture.repository;
    let reference = HistoryRef::Branch("main".into());
    let now = now_ms();
    let original = store.github_access_profiles().unwrap().remove(0);
    let reserved = store
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
        .install_current_credential(&reserved, || Ok::<_, ()>(()))
        .unwrap()
        .unwrap();
    store
        .record_access_result(
            &reserved,
            original.kind,
            AccessState::Readable,
            DiscoveryState::Complete,
            now,
        )
        .unwrap();
    let repo = store
        .github_repository_views(account)
        .unwrap()
        .into_iter()
        .find(|repo| repo.id == repository)
        .unwrap();
    store
        .observe_github_repository(
            &reserved,
            &RepositoryObservation {
                provider_id: repo.provider_id,
                owner: &repo.owner,
                name: &repo.name,
                private: true,
                source: RepositoryObservationSource::Direct,
            },
            now,
        )
        .unwrap();
    store.set_dispatch_controls(account, false, false).unwrap();
    let profile = store
        .github_access_profile(account, &reserved.owner_scope)
        .unwrap()
        .unwrap();
    let oid = |digit: char| CommitOid::parse(&digit.to_string().repeat(40)).unwrap();
    let commit = |digit, parents: &[char]| HistoryEntry {
        commit: GraphCommit {
            oid: oid(digit),
            title: format!("Fixture commit {digit}"),
            committed_at_ms: now,
            parents: parents.iter().map(|&digit| oid(digit)).collect(),
        },
        message: format!("Fixture commit {digit}"),
        author: Some("Fixture author".into()),
        authored_at_ms: Some(now),
    };
    store
        .observe_history_ref(&profile, repository, &reference, Some(&oid('f')), now)
        .unwrap();
    let walk = store
        .begin_history_page(&profile, repository, &reference)
        .unwrap()
        .unwrap();
    store
        .commit_history_page(
            &walk,
            &[commit('f', &['d', 'e']), commit('d', &[]), commit('e', &[])],
            None,
            true,
            now + 1,
        )
        .unwrap();
    let options = ComparisonOptions {
        ignore_whitespace: false,
        context_lines: 3,
    };
    let parents = store
        .comparison_for_parent(account, repository, &oid('f'), Some(&oid('e')), options)
        .unwrap();
    let pair = store
        .comparison_between(
            account,
            repository,
            &oid('d'),
            &oid('f'),
            ComparisonMode::Direct,
            options,
        )
        .unwrap();
    for (plan, text) in [
        (parents, "parent-specific text"),
        (pair, "pair-specific text"),
    ] {
        let walk = store.begin_history_diff(&profile, &plan).unwrap().unwrap();
        let identity = plan.diff_identity();
        store
            .commit_history_diff_page(
                &walk,
                HistoryDiffInput {
                    identity: &identity,
                    effective_base: identity.base.as_ref(),
                    files: vec![PrRecord::File {
                        path: "same.rs".into(),
                        previous_path: None,
                        status: "modified".into(),
                        additions: Some(1),
                        deletions: Some(1),
                        patch: PatchEvidence::Text(format!("@@ -1 +1 @@\n-old\n+{text}")),
                    }],
                    next_cursor: None,
                    complete: true,
                    observed_at_ms: now + 2,
                },
            )
            .unwrap();
    }

    let (window, reader) = setup.open(cx);
    click(cx, window, "reader-history");
    let repository_index = store
        .github_repository_views(account)
        .unwrap()
        .iter()
        .filter(|repo| repo.selected)
        .position(|repo| repo.id == repository)
        .unwrap();
    if repository_index != 0 {
        until(cx, |cx| cx.read(|cx| !reader.read(cx).history.busy));
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click(format!("history-repository-{repository_index}"), cx);
        })
        .unwrap();
    }
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).history.cached_graph_rows() == 3)
    });
    click(cx, window, "history-commit-0");
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).history.plan().is_some())
    });
    click(cx, window, "history-parent-1");
    until(cx, |cx| {
        cx.read(|cx| {
            matches!(
                reader.read(cx).history.plan(),
                Some(HistoryComparison::Parent {
                    parent_index: 1,
                    ..
                })
            )
        })
    });
    assert_eq!(
        store
            .history_view(account, repository, "reader-commit-history")
            .unwrap()
            .unwrap()
            .state
            .parent,
        Some(oid('e'))
    );
    let check_file = |cx: &mut TestAppContext, window, reader: &Entity<ReaderWorkspace>, text| {
        until(cx, |cx| {
            cx.read(|cx| {
                !reader.read(cx).history.busy && reader.read(cx).history.selected_diff().is_some()
            })
        });
        cx.update_window(window, |_, _, cx| {
            let diff = reader.read(cx).history.selected_diff().unwrap().clone();
            diff.update(cx, |state, cx| {
                state.set_selected_lines(
                    Some(DiffLineRange::new("same.rs", DiffSide::Modified, 1, 1)),
                    cx,
                )
            });
            assert_eq!(diff.read(cx).selected_text(cx).trim(), text);
            diff
        })
        .unwrap()
    };
    click(cx, window, "history-file-0");
    let parent_diff = check_file(cx, window, &reader, "parent-specific text");
    let graph = store
        .history_page(account, repository, &oid('f'), None, 48)
        .unwrap();
    let base = graph
        .rows
        .iter()
        .find(|row| row.entry.commit.oid == oid('d'))
        .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(format!("history-compare-{}", base.ordinal), cx);
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| {
            matches!(reader.read(cx).history.plan(),
        Some(HistoryComparison::Pair { key }) if key.base == oid('d') && key.head == oid('f'))
        })
    });
    assert!(cx.read(|cx| reader.read(cx).history.selected_diff().is_none()));
    click(cx, window, "history-file-0");
    let pair_diff = check_file(cx, window, &reader, "pair-specific text");
    assert_ne!(pair_diff, parent_diff);
    click(cx, window, "history-file-0");
    assert_eq!(
        check_file(cx, window, &reader, "pair-specific text"),
        pair_diff
    );
    let state = store
        .history_view(account, repository, "reader-commit-history")
        .unwrap()
        .unwrap()
        .state;
    assert_eq!(state.selected, Some(oid('f')));
    assert_eq!(state.second, Some(oid('d')));
    assert!(state.parent.is_none());

    for index in 0..100 {
        store
            .observe_history_ref(
                &profile,
                repository,
                &HistoryRef::Branch(format!("first-{index:03}")),
                Some(&oid('d')),
                now + 2,
            )
            .unwrap();
    }
    assert!(
        store
            .history_refs(account, repository, None, 100)
            .unwrap()
            .iter()
            .all(|row| row.reference != reference)
    );
    // The selected ref remains present even after it falls outside the bounded alphabetic page.
    for (index, tip) in [Some(oid('a')), None].into_iter().enumerate() {
        store
            .observe_history_ref(
                &profile,
                repository,
                &reference,
                tip.as_ref(),
                now + 2 + index as i64,
            )
            .unwrap();
        let sequence = index as u64 + 1;
        let connection = threadline_core::connectivity::ConnectivityObserver::new(1)
            .snapshot(std::time::Duration::ZERO);
        cx.update_window(window, |_, window, cx| {
            reader.update(cx, |this, cx| {
                this.update_sync(
                    crate::sync_worker::SyncUpdate {
                        account: Some(account),
                        status: None,
                        connection,
                        cache_generation: sequence,
                        history_generation: sequence,
                        eligible_outbox: 0,
                    },
                    window,
                    cx,
                );
            })
        })
        .unwrap();
        until(cx, |cx| {
            cx.read(|cx| reader.read(cx).history.cached_ref_tip(&reference) == Some(tip.as_ref()))
        });
        cx.read(|cx| {
            let history = &reader.read(cx).history;
            assert_eq!(history.cached_graph_rows(), 3);
            assert!(
                matches!(history.plan(), Some(HistoryComparison::Pair { key })
                if key.base == oid('d') && key.head == oid('f'))
            );
        });
        assert_eq!(
            store
                .history_view(account, repository, "reader-commit-history")
                .unwrap()
                .unwrap()
                .state,
            state
        );
    }

    store.sign_out_github_account(account, now + 5).unwrap();
    store.set_dispatch_controls(account, true, false).unwrap();
    let (reopened, restored) = setup.open(cx);
    click(cx, reopened, "reader-history");
    if repository_index != 0 {
        until(cx, |cx| cx.read(|cx| !restored.read(cx).history.busy));
        cx.update_window(reopened, |_, window, cx| {
            window.render_frame(cx);
            window.click(format!("history-repository-{repository_index}"), cx);
        })
        .unwrap();
    }
    until(cx, |cx| {
        cx.read(|cx| {
            matches!(restored.read(cx).history.plan(),
        Some(HistoryComparison::Pair { key }) if key.base == oid('d') && key.head == oid('f'))
        })
    });
    check_file(cx, reopened, &restored, "pair-specific text");
}

#[gpui_kit::test]
fn ui_007_notification_categories_actor_rules_and_repository_settings_are_independent(
    cx: &mut TestAppContext,
) {
    use threadline_storage::notifications::{Category, DeliveryMode, RuleActor, RuleScope};
    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    let repo = setup.fixture.repository;
    let item = setup.fixture.reviewed_pr;
    let cached = setup.store.item(account, item).unwrap().unwrap().title;
    let (window, reader) = setup.open(cx);
    click(cx, window, "reader-activity");
    click(cx, window, "activity-toggle-settings");
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).activity.scope_is_ready(None))
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("notification-settings").role(),
            Some(gpui_kit::Role::Group)
        );
        assert_eq!(
            window.find("notification-settings").label(),
            Some("Notification rules")
        );
    })
    .unwrap();
    click(cx, window, "notifications-automated-reviews");
    until(cx, |_| {
        setup
            .store
            .notification_settings(account)
            .unwrap()
            .actor_rules
            .len()
            == 1
    });
    click(cx, window, "notifications-category-1-2");
    until(cx, |_| {
        setup
            .store
            .notification_settings(account)
            .unwrap()
            .categories
            .get(&Category::OrdinaryComment)
            == Some(&DeliveryMode::Popup)
    });
    let defaults = setup.store.notification_settings(account).unwrap();
    assert_eq!(defaults.actor_rules[0].actor, RuleActor::KnownAutomation);
    assert_eq!(defaults.actor_rules[0].scope, RuleScope::ReviewsAndInline);
    assert!(!defaults.sound);
    assert!(!defaults.categories.contains_key(&Category::InlineComment));
    let index = setup
        .store
        .github_repository_views(account)
        .unwrap()
        .into_iter()
        .filter(|candidate| candidate.selected)
        .position(|candidate| candidate.id == repo)
        .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(format!("notifications-repository-{index}"), cx);
    })
    .unwrap();
    until(cx, |cx| {
        cx.read(|cx| reader.read(cx).activity.scope_is_ready(Some(repo)))
    });
    click(cx, window, "notifications-mute");
    until(cx, |_| {
        setup
            .store
            .repository_notification_rules(account, repo)
            .unwrap()
            .muted
    });
    assert_eq!(
        setup.store.notification_settings(account).unwrap(),
        defaults
    );
    assert_eq!(
        setup.store.item(account, item).unwrap().unwrap().title,
        cached
    );
}

#[gpui_kit::test]
fn ui_007_notification_opens_the_exact_downloaded_provider_comment(cx: &mut TestAppContext) {
    use threadline_storage::{
        notifications::{Destination, NotificationTarget},
        reader::{DiscussionComment, DiscussionInputPage},
        resources::{ResourceFamily, ResourceKey, ResourceTarget, WalkMode},
    };

    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    let repository = setup.fixture.repository;
    let item = setup.fixture.long_discussion;
    let comments = ResourceKey {
        account,
        target: ResourceTarget::Item(item),
        family: ResourceFamily::Comments,
    };
    let fetch = setup
        .store
        .start_resource_fetch(comments, WalkMode::Full)
        .unwrap();
    setup
        .store
        .commit_discussion_page(
            account,
            item,
            &fetch,
            DiscussionInputPage {
                comments: (1..=30)
                    .map(|id| DiscussionComment {
                        provider_id: id,
                        node_id: None,
                        author: None,
                        created_at_ms: id as i64,
                        updated_at_ms: id as i64,
                        body: format!("Fixture conversation {id}"),
                    })
                    .collect(),
                next_cursor: None,
                completed_validators: None,
                fetched_at_ms: 1_900_000_000_001,
            },
        )
        .unwrap();
    let (window, reader) = setup.open(cx);
    let key = format!("{:020}", 29);
    for route in ["issue-comment:29", "comment:29"] {
        cx.update_window(window, |_, window, cx| {
            reader.update(cx, |reader, cx| {
                reader.open_notification_destination(
                    account,
                    Destination {
                        repository: Some(repository),
                        target: NotificationTarget::Item {
                            item,
                            thread: Some(route.into()),
                        },
                    },
                    window,
                    cx,
                )
            });
        })
        .unwrap();
        until(cx, |cx| {
            cx.read(|cx| {
                let reader = reader.read(cx);
                !reader.layout_busy
                    && reader.active_tab().is_some_and(|tab| {
                        tab.item == item
                            && tab.view.scroll_anchor.as_deref() == Some(&key)
                            && reader.tab_views.get(&tab.id).is_some_and(|view| {
                                !view.data.anchor_missing
                                    && view
                                        .data
                                        .discussion
                                        .records
                                        .first()
                                        .is_some_and(|record| record.key == key)
                            })
                    })
            })
        });
    }
}

mod badges;

mod review_mute;
