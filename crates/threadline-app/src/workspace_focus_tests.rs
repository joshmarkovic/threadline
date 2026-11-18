use super::*;
use gpui_kit::{Point, TestAppContext, WindowBounds, WindowOptions, size, test::TestWindowExt};
use threadline_storage::Store;

#[gpui_kit::test]
fn ui_008_native_account_shortcut_opens_from_welcome_and_returns_focus(cx: &mut TestAppContext) {
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

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let shell = workspace.read(cx).shell_focus.clone();
        assert!(shell.is_focused(window));
        assert!(!workspace.read(cx).accounts_open);

        window.press("secondary-shift-a", cx);
        assert!(workspace.read(cx).accounts_open);
        assert!(window.find("account-close").visible());

        window.press("secondary-shift-a", cx);
        assert!(!workspace.read(cx).accounts_open);
        assert!(shell.is_focused(window));

        window.press("secondary-shift-a", cx);
        assert!(workspace.read(cx).accounts_open);
    })
    .unwrap();
}
