use super::*;
use crate::account_panel::{AccountPanel, CliFixture};
use gpui_kit::{AppContext, TestAppContext, WindowOptions, test::TestWindowExt};
use std::num::NonZeroU64;
use threadline_core::github::GitHubUser;
use threadline_platform::github_cli::CliError;

#[gpui_kit::test]
fn git_hub_cli_shows_the_chosen_account_and_discards_abandoned_sign_ins(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let simulator = Simulator::start();
    let profile = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(&profile.path().join("cli.sqlite")).unwrap());
    let service = AccountService {
        store: store.clone(),
        credentials: Arc::new(FixtureVault::default()),
        github: GitHubClient::loopback(simulator.address).unwrap(),
        fixture: true,
    };
    let (mock, mut attempts) = tokio::sync::mpsc::unbounded_channel();
    let (window, panel) = cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(CliFixture(mock));
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| AccountPanel::new(service, runtime.handle().clone(), window, cx))
        })
        .unwrap()
    });
    wait_form(cx, |cx| cx.read(|cx| panel.read(cx).loaded()));
    let click = |id: &'static str, cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        })
        .unwrap();
    };
    let visible = |id: &'static str, cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.try_find(id).is_some()
        })
        .unwrap()
    };
    let user = || GitHubUser {
        id: NonZeroU64::new(41).unwrap(),
        login: "test-user".into(),
    };
    let enter_route = |route: &'static str, cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            panel.update(cx, |panel, cx| panel.focus_form(window, cx));
            window.press("secondary-a", cx);
            window.input(route, cx);
        })
        .unwrap();
    };
    let next_attempt = |attempts: &mut tokio::sync::mpsc::UnboundedReceiver<_>,
                        cx: &mut TestAppContext| {
        let mut result = None;
        wait_form_for(cx, "a request to the isolated CLI", |_| {
            result = attempts.try_recv().ok();
            result.is_some()
        });
        result.unwrap()
    };
    click("account-add", cx);
    click("github-browser-sign-in", cx);
    let (interactive, empty_route) = next_attempt(&mut attempts, cx);
    assert!(interactive);
    empty_route.send(Ok(user())).ok().unwrap();
    wait_form(cx, |cx| visible("github-confirm-account", cx));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("github-sign-in-status").label(),
            Some("Signed in as @test-user. Enter a repository to connect.")
        );
    })
    .unwrap();
    click("github-confirm-account", cx);
    assert!(visible("account-repository-error", cx));
    assert!(store.active_github_account().unwrap().is_none());
    enter_route("fixture-org/private", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("github-sign-in-status").label(),
            Some("Connect @test-user to fixture-org/private?")
        );
    })
    .unwrap();
    click("github-cancel-sign-in", cx);
    enter_route("owner-only", cx);
    click("github-browser-sign-in", cx);
    assert!(visible("account-repository-error", cx));
    assert!(attempts.try_recv().is_err());
    enter_route("fixture-org/private", cx);

    click("github-browser-sign-in", cx);
    let (interactive, delayed) = next_attempt(&mut attempts, cx);
    assert!(interactive);
    click("github-cancel-sign-in", cx);
    click("github-use-cli", cx);
    let (interactive, missing) = next_attempt(&mut attempts, cx);
    assert!(!interactive);
    missing.send(Err(CliError::Missing)).ok().unwrap();
    wait_form(cx, |cx| visible("github-browser-sign-in", cx));
    assert!(visible("github-sign-in-status", cx));

    click("github-use-cli", cx);
    let (_, success) = next_attempt(&mut attempts, cx);
    success.send(Ok(user())).ok().unwrap();
    wait_form(cx, |cx| visible("github-confirm-account", cx));
    delayed.send(Err(CliError::LoginFailed)).ok().unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("github-sign-in-status").label(),
            Some("Connect @test-user to fixture-org/private?")
        );
    })
    .unwrap();
    assert!(store.active_github_account().unwrap().is_none());
    assert_eq!(simulator.requests.load(Ordering::SeqCst), 0);
    click("github-cancel-sign-in", cx);

    click("github-browser-sign-in", cx);
    let (_, departed) = next_attempt(&mut attempts, cx);
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |panel, cx| panel.clear_token(window, cx));
    })
    .unwrap();
    departed.send(Ok(user())).ok().unwrap();
    click("account-add", cx);
    assert!(!visible("github-confirm-account", cx));
    assert!(visible("account-use-pat", cx));
    assert!(store.active_github_account().unwrap().is_none());
}
