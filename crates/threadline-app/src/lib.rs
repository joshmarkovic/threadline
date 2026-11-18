mod fixtures;
#[cfg(feature = "e2e")]
mod observation;
mod palette;
mod pane_content;
mod workspace;

pub use workspace::{Workspace, bind_shortcuts};

use std::path::{Path, PathBuf};
use std::time::Instant;

use gpui_kit::{
    AppContext, Bounds, QuitMode, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use threadline_platform::{Activation, Launch};

#[derive(Debug, Default)]
pub struct Options {
    pub seed: u64,
    pub open_review: bool,
    pub e2e_report: Option<PathBuf>,
    pub profile: Option<PathBuf>,
    pub measure: bool,
}

fn start_platform(
    activation: Activation,
    profile: Option<&Path>,
) -> Result<Launch, threadline_platform::PlatformError> {
    let Some(profile) = profile else {
        return threadline_platform::start(activation);
    };
    #[cfg(all(windows, feature = "e2e"))]
    if let Ok(token) = std::env::var("THREADLINE_TEST_TOAST_TOKEN") {
        return threadline_platform::start_isolated_with_test_toast(activation, profile, &token);
    }
    threadline_platform::start_isolated(activation, profile)
}

pub fn run(options: Options, started: Instant) -> Result<(), Box<dyn std::error::Error>> {
    let activation = if options.open_review {
        Activation::OpenReview
    } else {
        Activation::Focus
    };
    let launch = start_platform(activation, options.profile.as_deref())?;
    let (controls, events) = match launch {
        Launch::Forwarded => return Ok(()),
        Launch::Primary { controls, events } => (controls, events),
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()?;
    let seed = options.seed;
    let large = options.measure;
    let open_review = options.open_review;
    let patches = runtime.spawn_blocking(move || fixtures::patches(seed, large));
    let handle = runtime.handle().clone();
    #[cfg(feature = "e2e")]
    let (observer, report_writer) = options
        .e2e_report
        .map(|path| observation::Observer::new(path, started, options.measure, &handle))
        .unzip();
    #[cfg(not(feature = "e2e"))]
    let _ = started;
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            cx.set_quit_mode(QuitMode::LastWindowClosed);
            gpui_kit::init(cx);
            bind_shortcuts(cx);
            cx.activate(true);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1320.), px(820.)),
                    cx,
                ))),
                window_min_size: Some(size(px(720.), px(540.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Threadline".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            if let Err(error) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| {
                    let mut workspace = Workspace::new(window, cx, seed, handle, open_review);
                    #[cfg(feature = "e2e")]
                    {
                        workspace.observer = observer;
                    }
                    workspace.connect_platform(controls, events, window, cx);
                    cx.spawn_in(window, async move |view, cx| match patches.await {
                        Ok(patches) => {
                            let _ = view.update(cx, |view, cx| view.load_patch(patches, cx));
                        }
                        Err(error) => {
                            eprintln!("Could not prepare the native diff: {error}");
                        }
                    })
                    .detach();
                    workspace
                })
            }) {
                eprintln!("Could not start the Threadline window: {error}");
                std::process::exit(1);
            }
        });
    #[cfg(feature = "e2e")]
    if let Some(report_writer) = report_writer {
        runtime.block_on(report_writer.finish())?;
    }
    Ok(())
}
