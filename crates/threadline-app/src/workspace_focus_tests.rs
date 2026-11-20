use super::*;
use gpui_kit::{Point, TestAppContext, WindowBounds, WindowOptions, size, test::TestWindowExt};
use threadline_storage::Store;

#[gpui_kit::test]
fn ui_008_first_launch_opens_time_zone_onboarding_and_keeps_home_controls_hidden(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_profile(directory.path()).unwrap());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let handle = runtime.handle().clone();
    let (window, workspace) = cx.update(|cx| {
        gpui_kit::init(cx);
        bind_shortcuts(cx);
        crate::reader_workspace::bind_shortcuts(cx);
        cx.set_global(crate::ProfileStore(store));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(1120.), px(840.)),
            })),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Workspace::new(window, cx, 42, handle, false, false))
        })
        .unwrap()
    });

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while cx.read(|cx| workspace.read(cx).route != WorkspaceRoute::Onboarding) {
        cx.run_until_parked();
        assert!(std::time::Instant::now() < deadline, "Welcome did not load");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("onboarding-zone-continue").visible());
        assert!(window.try_find("home-refresh-all").is_none());
        assert!(window.try_find("account-open").is_none());
        assert!(window.try_find("account-token").is_none());
        window.press("secondary-shift-a", cx);
        assert_eq!(workspace.read(cx).route, WorkspaceRoute::Onboarding);
    })
    .unwrap();
}

#[gpui_kit::test]
fn ui_008_synchronization_opens_from_settings_and_returns_to_home(cx: &mut TestAppContext) {
    use std::time::{Duration, Instant};
    use threadline_storage::fixtures::{Scale, populate_reader};

    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_profile(directory.path()).unwrap());
    let fixture = populate_reader(&store, 42, Scale::Small).unwrap();
    let initially_offline = store
        .sync_settings(fixture.account)
        .unwrap()
        .controls
        .work_offline;
    let profile = store.github_access_profiles().unwrap().pop().unwrap();
    let retired = store
        .observe_github_repository(
            &profile,
            &threadline_storage::access::RepositoryObservation {
                provider_id: std::num::NonZeroU64::new(3_400_112).unwrap(),
                owner: &profile.owner_scope,
                name: "removed-before-start",
                private: true,
                source: threadline_storage::access::RepositoryObservationSource::Discovery,
            },
            2,
        )
        .unwrap();
    store
        .mark_repository_access(
            &profile,
            retired,
            threadline_storage::access::AccessState::NotFoundOrDenied,
            3,
        )
        .unwrap();
    let views = store.github_repository_views(fixture.account).unwrap();
    let retained = views.len() - 1;
    let monitored = views.iter().filter(|r| r.selected).count();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let handle = runtime.handle().clone();
    let (window, workspace) = cx.update(|cx| {
        gpui_kit::init(cx);
        bind_shortcuts(cx);
        crate::reader_workspace::bind_shortcuts(cx);
        cx.set_global(crate::ProfileStore(store));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(1200.), px(900.)),
            })),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Workspace::new(window, cx, 42, handle, false, false))
        })
        .unwrap()
    });
    let step = |cx: &mut TestAppContext, condition: &mut dyn FnMut(&mut TestAppContext) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            cx.run_until_parked();
            if condition(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("The native synchronization/account transition did not complete");
    };
    step(cx, &mut |cx| {
        cx.update_window(window, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
            window
                .try_find("home-repository-summary")
                .is_some_and(|control| control.visible())
        })
        .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        assert_eq!(workspace.read(cx).route, WorkspaceRoute::Home);
        assert!(window.find("home-refresh-all").visible());
        let summary = window.find("home-repository-summary");
        assert!(summary.visible());
        assert!(
            summary
                .value()
                .unwrap()
                .starts_with(&format!("Monitoring {monitored} of {retained} "))
        );
        assert!(window.find("account-open").visible());
        assert!(window.find("account-close").visible());
        assert!(window.try_find("account-token").is_none());
        assert!(window.try_find("reader-sync-settings").is_none());
        assert!(window.try_find("settings-synchronization").is_none());
        assert!(window.try_find("sync-work-offline").is_none());
        window.click("account-open", cx);
    })
    .unwrap();
    step(cx, &mut |cx| {
        cx.read(|cx| workspace.read(cx).route == WorkspaceRoute::Settings)
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("home-refresh-all").is_none());
        window.click("settings-synchronization", cx);
    })
    .unwrap();
    step(cx, &mut |cx| {
        cx.update_window(window, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
            window
                .try_find("sync-work-offline")
                .is_some_and(|control| control.visible())
        })
        .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        assert!(window.find("sync-pause-background").visible());
        window.click("settings-sync-back", cx);
        window.render_frame(cx);
        assert!(window.find("settings-synchronization").visible());
        assert!(window.try_find("sync-work-offline").is_none());
        window.click("account-offline", cx);
    })
    .unwrap();
    step(cx, &mut |cx| {
        cx.read(|cx| {
            workspace
                .read(cx)
                .accounts
                .as_ref()
                .unwrap()
                .read(cx)
                .local_reader_state()
                .0
                .offline
                != initially_offline
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.click("settings-synchronization", cx);
    })
    .unwrap();
    step(cx, &mut |cx| {
        cx.update_window(window, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
            window
                .try_find("sync-work-offline")
                .is_some_and(|control| !control.disabled().unwrap_or(false))
                && window.find("sync-check-now").disabled().unwrap_or(false) != initially_offline
        })
        .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        window.press("secondary-shift-a", cx);
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert_eq!(workspace.read(cx).route, WorkspaceRoute::Home);
        assert!(window.find("home-refresh-all").visible());
        window.click("account-open", cx);
    })
    .unwrap();
    step(cx, &mut |cx| {
        cx.read(|cx| workspace.read(cx).route == WorkspaceRoute::Settings)
    });
    cx.update_window(window, |_, window, cx| {
        window.click("account-close", cx);
        assert_eq!(workspace.read(cx).route, WorkspaceRoute::Home);
    })
    .unwrap();
}
