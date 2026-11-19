use super::*;
use gpui_kit::{TestAppContext, WindowOptions, test::TestWindowExt};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use threadline_platform::PlatformError;

#[derive(Default)]
struct Fixture {
    configured: AtomicBool,
    private_channel: AtomicBool,
    credential: Mutex<Option<Vec<u8>>>,
    reads: AtomicUsize,
    checks: AtomicUsize,
    with_token: AtomicBool,
    handoffs: AtomicUsize,
    acknowledge_handoff: AtomicBool,
    release_gate: tokio::sync::Notify,
}
impl CredentialStore for Fixture {
    fn read(&self, key: &CredentialRef) -> Result<Option<Credential>, PlatformError> {
        assert_eq!(key.scope, "updates/github.com/joshmarkovic/threadline");
        assert_eq!(key.name, "release-contents-read");
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(self.credential.lock().unwrap().clone().map(Credential))
    }
    fn save(&self, key: &CredentialRef, credential: &Credential) -> Result<(), PlatformError> {
        assert_eq!(key.scope, "updates/github.com/joshmarkovic/threadline");
        *self.credential.lock().unwrap() = Some(credential.0.clone());
        Ok(())
    }
    fn remove(&self, key: &CredentialRef) -> Result<(), PlatformError> {
        assert_eq!(key.name, "release-contents-read");
        self.credential.lock().unwrap().take();
        Ok(())
    }
}

struct TestSource(Arc<Fixture>);
impl Source for TestSource {
    fn configured(&self) -> bool {
        self.0.configured.load(Ordering::SeqCst)
    }
    fn private_channel(&self) -> bool {
        self.0.private_channel.load(Ordering::SeqCst)
    }
    fn check<'a>(&'a self, token: Option<&'a str>) -> CheckFuture<'a> {
        Box::pin(async move {
            self.0
                .with_token
                .store(token == Some("fixture-only"), Ordering::SeqCst);
            self.0.checks.fetch_add(1, Ordering::SeqCst);
            self.0.release_gate.notified().await;
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
    fixture
        .save(
            &CredentialRef::github_releases(),
            &Credential(b"fixture-only".to_vec()),
        )
        .unwrap();
    let (window, panel) = cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| {
                Updates::with_source(
                    runtime.handle().clone(),
                    Arc::new(TestSource(fixture.clone())),
                    fixture.clone(),
                    window,
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
    assert!(fixture.with_token.load(Ordering::SeqCst));
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |model, cx| {
            model.credential_note = Some("Private release token saved.");
            model.show_settings(true, window, cx);
        });
        window.render_frame(cx);
        assert!(window.find("update-credential-note").visible());
        panel.update(cx, |model, cx| model.show_settings(false, window, cx));
        window.render_frame(cx);
        assert!(window.try_find("update-credential-note").is_none());
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
fn startup_updates_skip_credential_store_and_network_when_unconfigured(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let fixture = Arc::new(Fixture::default());
    let (_, panel) = cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| {
                Updates::with_source(
                    runtime.handle().clone(),
                    Arc::new(TestSource(fixture.clone())),
                    fixture.clone(),
                    window,
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
    assert_eq!(fixture.reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.checks.load(Ordering::SeqCst), 0);
}

#[gpui_kit::test]
fn private_updates_request_release_access_once_in_settings(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let fixture = Arc::new(Fixture::default());
    fixture.configured.store(true, Ordering::SeqCst);
    fixture.private_channel.store(true, Ordering::SeqCst);
    let (window, panel) = cx.update(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| {
                Updates::with_source(
                    runtime.handle().clone(),
                    Arc::new(TestSource(fixture.clone())),
                    fixture.clone(),
                    window,
                    cx,
                )
            })
        })
        .unwrap()
    });
    cx.update(|cx| panel.update(cx, |model, cx| model.set_policy(Some(true), cx)));
    await_model(cx, |cx| {
        cx.read(|cx| matches!(panel.read(cx).state, State::NeedsCredential))
    });
    assert_eq!(fixture.checks.load(Ordering::SeqCst), 0);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("updates-open-settings").visible());
        assert!(window.try_find("updates-token-save").is_none());
        panel.update(cx, |model, cx| model.show_settings(true, window, cx));
        window.render_frame(cx);
        assert!(window.find("updates-token-save").visible());
        panel
            .read(cx)
            .input
            .clone()
            .update(cx, |input, cx| input.set_value("fixture-only", window, cx));
        window.click("updates-token-save", cx);
    })
    .unwrap();
    await_model(cx, |_| fixture.checks.load(Ordering::SeqCst) == 1);
    assert!(fixture.with_token.load(Ordering::SeqCst));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("updates-token-save").is_none());
        assert!(window.find("updates-manage-token").visible());
    })
    .unwrap();
}

#[gpui_kit::test]
fn startup_installs_the_verified_update_automatically(cx: &mut TestAppContext) {
    check_workspace_startup(cx, false);
}

#[gpui_kit::test]
fn starting_work_during_the_check_defers_installation(cx: &mut TestAppContext) {
    check_workspace_startup(cx, true);
}

fn check_workspace_startup(cx: &mut TestAppContext, interact: bool) {
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let fixture = Arc::new(Fixture::default());
    fixture.configured.store(true, Ordering::SeqCst);
    fixture.acknowledge_handoff.store(true, Ordering::SeqCst);
    let ((window, _workspace), panel) = cx.update(|cx| {
        gpui_kit::init(cx);
        let mut panel = None;
        let window = gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            let updates = cx.new(|cx| {
                Updates::with_source(
                    runtime.handle().clone(),
                    Arc::new(TestSource(fixture.clone())),
                    fixture.clone(),
                    window,
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
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click("updates-restart", cx);
        })
        .unwrap();
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
                    fixture.clone(),
                    window,
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
