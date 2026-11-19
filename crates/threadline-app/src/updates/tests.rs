use super::*;
use gpui_kit::{TestAppContext, WindowOptions};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use threadline_platform::PlatformError;

#[derive(Default)]
struct Fixture {
    configured: AtomicBool,
    credential: Mutex<Option<Vec<u8>>>,
    reads: AtomicUsize,
    checks: AtomicUsize,
    with_token: AtomicBool,
    handoffs: AtomicUsize,
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
        Err("Synthetic installer failed. Threadline is still open.")
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
fn startup_updates_wait_for_policy_cancel_and_require_explicit_restart(cx: &mut TestAppContext) {
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
    cx.update(|cx| panel.update(cx, |model, cx| model.set_policy(Some(true), cx)));
    assert_eq!(fixture.checks.load(Ordering::SeqCst), 2);
    assert_eq!(fixture.handoffs.load(Ordering::SeqCst), 0);
    cx.update_window(window, |_, _, cx| {
        panel.update(cx, |model, cx| model.launch(cx))
    })
    .unwrap();
    await_model(cx, |cx| cx.update(|cx| panel.read(cx).note.is_some()));
    assert_eq!(fixture.handoffs.load(Ordering::SeqCst), 1);
    assert!(cx.update(|cx| matches!(panel.read(cx).state, State::Ready(_))));
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
