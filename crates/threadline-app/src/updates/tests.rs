use super::*;
use gpui_kit::{AppContext, TestAppContext, WindowOptions, test::TestWindowExt};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Default)]
struct Fixture {
    configured: AtomicBool,
    checks: AtomicUsize,
    handoffs: AtomicUsize,
    acknowledge_handoff: AtomicBool,
    nothing_newer: AtomicBool,
    release_gate: tokio::sync::Notify,
    next_error: Mutex<Option<&'static str>>,
}

struct TestSource(Arc<Fixture>);
impl Source for TestSource {
    fn installed(&self) -> Option<String> {
        self.0
            .configured
            .load(Ordering::SeqCst)
            .then(|| "1.2.2".to_owned())
    }
    fn check(&self) -> CheckFuture<'_> {
        Box::pin(async move {
            self.0.checks.fetch_add(1, Ordering::SeqCst);
            self.0.release_gate.notified().await;
            if let Some(error) = self.0.next_error.lock().unwrap().take() {
                return Err(error);
            }
            if self.0.nothing_newer.load(Ordering::SeqCst) {
                return Ok(None);
            }
            Ok(Some(
                Box::new(TestSetup(self.0.clone())) as Box<dyn VerifiedSetup>
            ))
        })
    }
}
struct TestSetup(Arc<Fixture>);
impl VerifiedSetup for TestSetup {
    fn version(&self) -> &str {
        "1.2.3"
    }
    fn launch(&self) -> Result<(), &'static str> {
        self.0.handoffs.fetch_add(1, Ordering::SeqCst);
        if self.0.acknowledge_handoff.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err("Synthetic installer failed. Threadline is still open.")
        }
    }
}
fn await_model(cx: &mut TestAppContext, mut ready: impl FnMut(&mut TestAppContext) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        cx.run_until_parked();
        if ready(cx) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Update state timed out."
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[gpui_kit::test]
fn startup_updates_wait_for_policy_cancel_and_allow_manual_retry(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let fixture = Arc::new(Fixture::default());
    fixture.configured.store(true, Ordering::SeqCst);
    let (window, panel) = cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(|cx| {
                Updates::with_source(
                    runtime.handle().clone(),
                    Arc::new(TestSource(fixture.clone())),
                    cx,
                )
            })
        })
        .unwrap()
    });
    await_model(cx, |cx| {
        cx.update(|cx| matches!(panel.read(cx).state, State::Waiting))
    });
    cx.update(|cx| panel.update(cx, |model, cx| model.set_policy(Some(false), cx)));
    assert_eq!(fixture.checks.load(Ordering::SeqCst), 0);
    cx.update(|cx| panel.update(cx, |model, cx| model.set_policy(Some(true), cx)));
    await_model(cx, |_| fixture.checks.load(Ordering::SeqCst) == 1);
    cx.update(|cx| panel.update(cx, |model, cx| model.set_policy(Some(false), cx)));
    assert!(cx.update(|cx| matches!(panel.read(cx).state, State::Waiting)));
    cx.update(|cx| panel.update(cx, |model, cx| model.set_policy(Some(true), cx)));
    await_model(cx, |_| fixture.checks.load(Ordering::SeqCst) == 2);
    fixture.release_gate.notify_one();
    await_model(cx, |cx| {
        cx.update(|cx| matches!(panel.read(cx).state, State::Ready(_)))
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("updates-restart").visible());
    })
    .unwrap();
    cx.update(|cx| panel.update(cx, |model, cx| model.set_policy(Some(true), cx)));
    assert_eq!(fixture.checks.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.handoffs.load(Ordering::SeqCst), 0);
    let acknowledged = Arc::new(AtomicBool::new(false));
    let signal = acknowledged.clone();
    let (_click, _acknowledgement) = cx.update(|cx| {
        let click = cx.subscribe(&panel, |model, request: &RestartRequested, cx| {
            assert!(matches!(request, RestartRequested::User));
            model.update(cx, |model, cx| model.launch(*request, cx));
        });
        let acknowledgement = cx.subscribe(&panel, move |_, _: &SetupStarted, _| {
            signal.store(true, Ordering::SeqCst);
        });
        (click, acknowledgement)
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("updates-restart", cx);
    })
    .unwrap();
    await_model(cx, |cx| cx.update(|cx| panel.read(cx).note.is_some()));
    assert_eq!(fixture.handoffs.load(Ordering::SeqCst), 1);
    assert!(cx.update(|cx| matches!(panel.read(cx).state, State::Ready(_))));
    assert!(!acknowledged.load(Ordering::SeqCst));
    fixture.acknowledge_handoff.store(true, Ordering::SeqCst);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("updates-restart", cx);
    })
    .unwrap();
    await_model(cx, |_| acknowledged.load(Ordering::SeqCst));
    assert_eq!(fixture.handoffs.load(Ordering::SeqCst), 2);
}

#[gpui_kit::test]
fn startup_updates_skip_network_when_unconfigured(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let fixture = Arc::new(Fixture::default());
    let (_, panel) = cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(|cx| {
                Updates::with_source(
                    runtime.handle().clone(),
                    Arc::new(TestSource(fixture.clone())),
                    cx,
                )
            })
        })
        .unwrap()
    });
    cx.update(|cx| panel.update(cx, |model, cx| model.set_policy(Some(true), cx)));
    await_model(cx, |cx| {
        cx.update(|cx| matches!(panel.read(cx).state, State::Unconfigured))
    });
    assert_eq!(fixture.checks.load(Ordering::SeqCst), 0);
}

#[gpui_kit::test]
fn public_updates_offer_retry_without_a_token_prompt(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let fixture = Arc::new(Fixture::default());
    fixture.configured.store(true, Ordering::SeqCst);
    *fixture.next_error.lock().unwrap() = Some("The public GitHub releases are unavailable.");
    let (window, panel) = cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(|cx| {
                Updates::with_source(
                    runtime.handle().clone(),
                    Arc::new(TestSource(fixture.clone())),
                    cx,
                )
            })
        })
        .unwrap()
    });
    cx.update(|cx| panel.update(cx, |model, cx| model.set_policy(Some(true), cx)));
    await_model(cx, |_| fixture.checks.load(Ordering::SeqCst) == 1);
    fixture.release_gate.notify_one();
    await_model(cx, |cx| {
        cx.read(|cx| matches!(panel.read(cx).state, State::Failed(_)))
    });
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |model, cx| model.show_settings(true, cx));
        window.render_frame(cx);
        assert!(window.try_find("updates-release-token").is_none());
        assert!(window.try_find("updates-manage-token").is_none());
        window.click("updates-check-again", cx);
    })
    .unwrap();
    await_model(cx, |_| fixture.checks.load(Ordering::SeqCst) == 2);
    fixture.release_gate.notify_one();
    await_model(cx, |cx| {
        cx.read(|cx| matches!(panel.read(cx).state, State::Ready(_)))
    });
}

#[gpui_kit::test]
fn an_open_app_checks_again_later_and_on_request(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let fixture = Arc::new(Fixture::default());
    fixture.configured.store(true, Ordering::SeqCst);
    fixture.nothing_newer.store(true, Ordering::SeqCst);
    let (window, panel) = cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(|cx| {
                Updates::with_source(
                    runtime.handle().clone(),
                    Arc::new(TestSource(fixture.clone())),
                    cx,
                )
            })
        })
        .unwrap()
    });
    let current = |cx: &mut TestAppContext| {
        fixture.release_gate.notify_one();
        await_model(cx, |cx| {
            cx.read(|cx| matches!(panel.read(cx).state, State::Current))
        });
    };
    cx.update(|cx| panel.update(cx, |model, cx| model.set_policy(Some(true), cx)));
    await_model(cx, |_| fixture.checks.load(Ordering::SeqCst) == 1);
    current(cx);
    cx.executor()
        .advance_clock(RECHECK_AFTER_CURRENT - Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(fixture.checks.load(Ordering::SeqCst), 1);
    cx.executor().advance_clock(Duration::from_secs(1));
    await_model(cx, |_| fixture.checks.load(Ordering::SeqCst) == 2);
    current(cx);
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |model, cx| model.show_settings(true, cx));
        window.render_frame(cx);
        assert!(window.find("update-version").visible());
        window.click("updates-check-again", cx);
    })
    .unwrap();
    await_model(cx, |_| fixture.checks.load(Ordering::SeqCst) == 3);
}

#[gpui_kit::test]
fn startup_installs_the_verified_update_automatically(cx: &mut TestAppContext) {
    check_workspace_startup(cx, Start::Untouched);
}

#[gpui_kit::test]
fn starting_work_during_the_check_defers_installation(cx: &mut TestAppContext) {
    check_workspace_startup(cx, Start::Working);
}

#[gpui_kit::test]
fn a_deferred_update_installs_once_the_app_is_idle(cx: &mut TestAppContext) {
    check_workspace_startup(cx, Start::WorkingThenIdle);
}

#[derive(Clone, Copy, PartialEq)]
enum Start {
    Untouched,
    Working,
    WorkingThenIdle,
}

fn check_workspace_startup(cx: &mut TestAppContext, start: Start) {
    use crate::workspace::{IDLE_POLL, IDLE_WHILE_ACTIVE};

    let interact = start != Start::Untouched;
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let fixture = Arc::new(Fixture::default());
    fixture.configured.store(true, Ordering::SeqCst);
    fixture.acknowledge_handoff.store(true, Ordering::SeqCst);
    let ((window, workspace), panel) = cx.update(|cx| {
        gpui_kit::init(cx);
        let mut panel = None;
        let window = gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            let updates = cx.new(|cx| {
                Updates::with_source(
                    runtime.handle().clone(),
                    Arc::new(TestSource(fixture.clone())),
                    cx,
                )
            });
            panel = Some(updates.clone());
            cx.new(|cx| {
                let mut workspace = crate::workspace::Workspace::new(
                    window,
                    cx,
                    42,
                    runtime.handle().clone(),
                    false,
                    false,
                );
                workspace.attach_updates(updates, window, cx);
                workspace
            })
        })
        .unwrap();
        (window, panel.unwrap())
    });
    cx.update(|cx| panel.update(cx, |model, cx| model.set_policy(Some(true), cx)));
    await_model(cx, |_| fixture.checks.load(Ordering::SeqCst) == 1);
    if interact {
        cx.simulate_keystrokes(window, "a");
    }
    fixture.release_gate.notify_one();
    if interact {
        await_model(cx, |cx| {
            cx.read(|cx| matches!(panel.read(cx).state, State::Ready(_)))
        });
        assert_eq!(fixture.handoffs.load(Ordering::SeqCst), 0);
    }
    match start {
        Start::Untouched => {}
        Start::Working => {
            cx.update_window(window, |_, window, cx| {
                window.render_frame(cx);
                window.click("updates-restart", cx);
            })
            .unwrap();
        }
        Start::WorkingThenIdle => {
            cx.executor().advance_clock(IDLE_POLL);
            cx.run_until_parked();
            assert_eq!(fixture.handoffs.load(Ordering::SeqCst), 0);
            cx.update(|cx| {
                workspace.update(cx, |workspace, _| {
                    workspace.rewind_last_interaction(IDLE_WHILE_ACTIVE)
                })
            });
            cx.executor().advance_clock(IDLE_POLL);
        }
    }
    await_model(cx, |_| fixture.handoffs.load(Ordering::SeqCst) == 1);
}

#[cfg(feature = "e2e")]
#[gpui_kit::test]
fn startup_update_retries_after_workspace_save_and_installer_failures(cx: &mut TestAppContext) {
    use crate::workspace::Workspace;
    use threadline_storage::{
        Store,
        fixtures::{Scale, populate_reader},
    };

    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("update-restart.sqlite3");
    let store = Arc::new(Store::open(&db).unwrap());
    let account = populate_reader(&store, 42, Scale::Small).unwrap().account;
    store
        .update_sync_settings(account, |settings| {
            settings.controls.work_offline = true;
            settings.controls.pause_background = true;
        })
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let fixture = Arc::new(Fixture::default());
    fixture.configured.store(true, Ordering::SeqCst);
    let ((window, _workspace), model) = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(crate::ProfileStore(store));
        let mut model = None;
        let window = gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            let updates = cx.new(|cx| {
                Updates::with_source(
                    runtime.handle().clone(),
                    Arc::new(TestSource(fixture.clone())),
                    cx,
                )
            });
            model = Some(updates.clone());
            cx.new(|cx| {
                let mut view =
                    Workspace::new(window, cx, 42, runtime.handle().clone(), false, false);
                view.attach_updates(updates, window, cx);
                view
            })
        })
        .unwrap();
        (window, model.unwrap())
    });
    await_model(cx, |cx| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            matches!(model.read(cx).state, State::Waiting)
                && window.try_find("reader-browse").is_some()
        })
        .unwrap()
    });
    let db = rusqlite::Connection::open(db).unwrap();
    db.execute_batch(
        "CREATE TRIGGER refuse_layout BEFORE UPDATE OF layout ON workspaces
        BEGIN SELECT RAISE(ABORT, 'synthetic layout failure'); END;",
    )
    .unwrap();
    let click = |cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("updates-restart", cx);
        })
        .unwrap()
    };
    cx.update(|cx| model.update(cx, |model, cx| model.set_policy(Some(true), cx)));
    fixture.release_gate.notify_one();
    await_model(cx, |cx| {
        cx.read(|cx| {
            model.read(cx).note
                == Some("Workspace state could not be saved. Your tabs remain open.")
        })
    });
    assert_eq!(fixture.handoffs.load(Ordering::SeqCst), 0);
    db.execute_batch("DROP TRIGGER refuse_layout;").unwrap();
    click(cx);
    await_model(cx, |cx| {
        cx.read(|cx| {
            model.read(cx).note == Some("Synthetic installer failed. Threadline is still open.")
        })
    });
    assert_eq!(fixture.handoffs.load(Ordering::SeqCst), 1);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("updates-restart").visible());
        assert!(window.find("reader-browse").visible());
    })
    .unwrap();
}
