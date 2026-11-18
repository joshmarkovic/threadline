mod account_panel;
mod accounts;
mod fixtures;
#[cfg(windows)]
mod notification_commands;
#[cfg(feature = "e2e")]
mod observation;
mod palette;
mod pane_content;
mod reader;
mod reader_workspace;
mod sync_panel;
mod sync_worker;
mod workspace;

#[cfg(windows)]
pub use notification_commands::handle_notification_command;
pub use workspace::{Workspace, bind_shortcuts};

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use gpui_kit::{
    App, AppContext, Bounds, Context, Global, IntoElement, ParentElement, QuitMode, Render, Styled,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, px, size,
};
use threadline_platform::{Activation, Launch, NativePaths, PathResolver};
use threadline_storage::Store;

#[derive(Debug, Default)]
pub struct Options {
    pub seed: u64,
    pub open_review: bool,
    pub e2e_report: Option<PathBuf>,
    pub profile: Option<PathBuf>,
    pub measure: bool,
    pub feasibility_fixture: bool,
}

struct ProfileStore(Arc<Store>);
impl Global for ProfileStore {}

/// The initialized profile store on a real app launch. Call SQLite from a worker.
pub fn profile_store(cx: &App) -> Option<Arc<Store>> {
    cx.try_global::<ProfileStore>().map(|store| store.0.clone())
}

fn open_profile(profile: Option<PathBuf>) -> Result<Store, String> {
    let root = match profile {
        Some(profile) => profile,
        None => {
            let root = NativePaths
                .application_paths()
                .map_err(|error| error.to_string())?
                .data;
            let mut directory = std::fs::DirBuilder::new();
            directory.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt as _;
                directory.mode(0o700);
            }
            directory
                .create(&root)
                .map_err(|_| "Could not create the application data directory".to_owned())?;
            root
        }
    };
    Store::open_profile(&root).map_err(|error| error.to_string())
}

struct ProfileFailure(String);

impl Render for ProfileFailure {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = palette::Palette::current(cx);
        div()
            .size_full()
            .bg(palette.canvas)
            .text_color(palette.ink)
            .flex()
            .flex_col()
            .p(px(24.))
            .gap(px(12.))
            .child(
                div()
                    .text_size(px(20.))
                    .child("Could not open the Threadline profile"),
            )
            .child(self.0.clone())
    }
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
    if options.e2e_report.is_some() && options.profile.is_none() {
        return Err("Native test reports require an isolated profile directory".into());
    }
    if options.feasibility_fixture
        && (!cfg!(feature = "e2e") || options.e2e_report.is_none() || options.profile.is_none())
    {
        return Err(
            "The Sprint 01 fixture requires an instrumented app and an isolated native report"
                .into(),
        );
    }
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
        .enable_all()
        .build()?;
    // Resolve and migrate the database on a worker before exposing any workspace
    // state. Close the failed launch without showing a substitute workspace.
    let profile = options.profile;
    let storage = runtime
        .block_on(runtime.spawn_blocking(move || open_profile(profile)))
        .unwrap_or_else(|_| Err("The profile database worker stopped unexpectedly".to_owned()));
    let startup_error = storage.as_ref().err().cloned();
    let seed = options.seed;
    let large = options.measure;
    let feasibility_fixture = options.feasibility_fixture;
    let open_review = options.open_review;
    let patches =
        feasibility_fixture.then(|| runtime.spawn_blocking(move || fixtures::patches(seed, large)));
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
            reader_workspace::bind_shortcuts(cx);
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
            match storage {
                Ok(store) => cx.set_global(ProfileStore(Arc::new(store))),
                Err(error) => {
                    if gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| ProfileFailure(error)))
                        .is_err()
                    {
                        cx.quit();
                    }
                    return;
                }
            }
            if let Err(error) = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| {
                    let mut workspace =
                        Workspace::new(window, cx, seed, handle, open_review, feasibility_fixture);
                    #[cfg(feature = "e2e")]
                    {
                        workspace.set_observer(observer, cx);
                    }
                    workspace.connect_platform(controls, events, window, cx);
                    if let Some(patches) = patches {
                        cx.spawn_in(window, async move |view, cx| match patches.await {
                            Ok(patches) => {
                                let _ = view.update(cx, |view, cx| view.load_patch(patches, cx));
                            }
                            Err(error) => {
                                eprintln!("Could not prepare the native diff: {error}");
                            }
                        })
                        .detach();
                    }
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
    if let Some(error) = startup_error {
        return Err(format!("Could not open the Threadline profile. {error}").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use threadline_storage::DATABASE_FILENAME;

    #[test]
    fn bootstrap_uses_only_the_explicit_unicode_fixture_and_preserves_an_invalid_database() {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("fixture-例");
        std::fs::create_dir(&profile).unwrap();
        let store = open_profile(Some(profile.clone())).unwrap();
        drop(store);
        assert!(profile.join(DATABASE_FILENAME).is_file());

        let other = temp.path().join("fixture-corrupt");
        std::fs::create_dir(&other).unwrap();
        let invalid_db = other.join(DATABASE_FILENAME);
        std::fs::write(&invalid_db, b"invalid synthetic database").unwrap();
        assert!(open_profile(Some(other)).is_err());
        assert_eq!(
            std::fs::read(&invalid_db).unwrap(),
            b"invalid synthetic database"
        );
    }

    #[test]
    fn reporting_without_an_isolated_profile_fails_before_startup() {
        let report = PathBuf::from("unused-output");
        let result = run(
            Options {
                e2e_report: Some(report),
                ..Options::default()
            },
            Instant::now(),
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "Native test reports require an isolated profile directory"
        );
    }
}
