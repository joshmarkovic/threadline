use gpui_kit::base::Root;
use gpui_kit::component::diff::DiffFile;
use gpui_kit::test::{TestQueryExt, TestWindowExt};
use gpui_kit::{
    AppContext, Bounds, ClipboardItem, Point, TestAppContext, WindowBounds, WindowHandle,
    WindowOptions, point, px, size,
};
use threadline_app::{Workspace, bind_shortcuts};

fn open(cx: &mut TestAppContext) -> (WindowHandle<Root>, tokio::runtime::Runtime) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let (window, _) = cx.update(|cx| {
        gpui_kit::init(cx);
        bind_shortcuts(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(1320.), px(820.)),
            })),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| {
                let mut workspace = Workspace::new(window, cx, 42, runtime.handle().clone(), false);
                workspace.load_patch(
                    [
                        DiffFile::parse(
                            "--- a/src/cache.rs\n+++ b/src/cache.rs\n@@ -1,4 +1,4 @@\n a\n b\n-old\n+cache\n d\n",
                        )
                        .unwrap(),
                        DiffFile::parse("--- a/src/status.rs\n+++ b/src/status.rs\n@@ -1 +1 @@\n-old\n+new\n")
                            .unwrap(),
                    ],
                    cx,
                );
                workspace
            })
        })
        .unwrap()
    });
    cx.run_until_parked();
    (window.downcast().unwrap(), runtime)
}

#[gpui_kit::test]
fn focus_tabs_and_context_keep_the_active_draft(cx: &mut TestAppContext) {
    let (handle, _runtime) = open(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("cache-review-note").is_some());
        window.click("left_input", cx);
        window.input("local discussion draft", cx);
        window.click("right_input", cx);
        window.input("cache review draft", cx);
        assert_eq!(window.find("right_input").focused(), Some(true));
        window.press("secondary-shift-l", cx);
        assert!(window.try_find("cache-review-note").is_some());
        assert!(
            window
                .try_find_by_label("station-labs / relay #41")
                .is_some()
        );
        assert_eq!(window.find("right_input").focused(), Some(true));
        assert_eq!(
            window.find("right_input").value(),
            Some("cache review draft")
        );

        window.click("second-pr-tab", cx);
        assert_eq!(window.find("right_input").value(), Some(""));
        assert!(
            window
                .try_find_by_label("Pull request description")
                .is_some()
        );
        window.click("first-pr-tab", cx);
        assert!(
            window
                .try_find_by_label("station-labs / relay #41")
                .is_some()
        );
        assert_eq!(
            window.find("right_input").value(),
            Some("cache review draft")
        );
        assert_eq!(
            window.find("left_input").value(),
            Some("local discussion draft")
        );

        window.press("secondary-1", cx);
        window.press("secondary-f", cx);
        assert_eq!(window.find("left_input").focused(), Some(true));
        assert_eq!(window.find("right_input").focused(), Some(false));
        window.press("ctrl-tab", cx);
        assert!(window.try_find("commit-history").is_some());
        window.press("ctrl-tab", cx);
        assert!(window.try_find("markdown-copy").is_some());
        assert_eq!(
            window.find("left_input").value(),
            Some("local discussion draft")
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn resize_drag_geometry_survives_scale_change(cx: &mut TestAppContext) {
    let (handle, _runtime) = open(cx);
    let (outer_width, context_width) = cx
        .update_window(handle.into(), |_, window, cx| {
            window.render_frame(cx);
            let context = window.find("review-context").bounds().size.width;
            let code = window.find("diff-panel").bounds().size.width;
            let fraction = context.as_f32() / (context + code).as_f32();
            assert!(
                (0.30..=0.35).contains(&fraction),
                "context fraction {fraction}"
            );
            let initial_left = window.find("left-pane").bounds().size.width;
            let splitter = window.find("outer_divider").bounds().center();
            window.drag(splitter, splitter + point(px(38.), px(0.)), cx);
            let left = window.find("left-pane").bounds().size.width;
            assert!(left - initial_left >= px(36.));

            let initial_context = window.find("review-context").bounds().size.width;
            let splitter = window.find("context_divider").bounds().center();
            window.drag(splitter, splitter + point(px(32.), px(0.)), cx);
            let context = window.find("review-context").bounds().size.width;
            assert!(context - initial_context >= px(29.));
            assert!(window.find("diff-panel").bounds().size.width > context);
            (left, context)
        })
        .unwrap();

    cx.simulate_window_scale_factor_change(handle.into(), 2.);
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.scale_factor(), 2.);
        assert_eq!(window.find("left-pane").bounds().size.width, outer_width);
        assert_eq!(
            window.find("review-context").bounds().size.width,
            context_width
        );
        window.click("context-issue", cx);
        assert!(
            window
                .try_find_by_label("station-labs / relay #41")
                .is_some()
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn markdown_drag_then_copy_uses_the_toolkit_selection(cx: &mut TestAppContext) {
    let (handle, _runtime) = open(cx);
    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);
        let bounds = window.find("markdown-copy").bounds();
        assert!(bounds.size.height > px(0.));
        let left = point(bounds.left() + px(1.), bounds.center().y);
        let right = point(
            (bounds.left() + px(320.)).min(bounds.right() - px(1.)),
            bounds.center().y,
        );
        cx.write_to_clipboard(ClipboardItem::new_string("pre-existing clipboard".into()));
        window.drag(left, right, cx);
        window.press("secondary-c", cx);
        let copied = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap();
        assert!(copied.contains("Offline review"), "clipboard: {copied:?}");
    })
    .unwrap();
}
