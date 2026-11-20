mod account_panel;
mod accounts;
mod activity_delivery;
#[cfg(feature = "e2e")]
mod activity_fixture;
mod brand;
mod display_time;
mod fixtures;
#[cfg(feature = "e2e")]
mod lifecycle_fixture;
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
mod ui;
mod updates;
mod workspace;

#[cfg(feature = "e2e")]
pub use activity_fixture::native_activity_command;
#[cfg(feature = "e2e")]
pub use lifecycle_fixture::native_lifecycle_command;
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
    pub display_timezone: Option<String>,
    pub seed: u64,
    pub open_review: bool,
    pub notification_com_server: bool,
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
    let clock = display_time::DisplayClock::open(options.profile.as_deref())?;
    if let Some(name) = options.display_timezone.as_deref() {
        clock.save(name)?;
        return Ok(());
    }
    #[cfg(feature = "e2e")]
    let mut startup = options
        .e2e_report
        .as_ref()
        .and_then(|_| observation::StartupTiming::enabled(started));
    #[cfg(feature = "e2e")]
    if let Some(startup) = startup.as_mut() {
        startup.mark(observation::StartupStage::OptionsParsed);
    }
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
    let (controls, mut events) = match launch {
        Launch::Forwarded => return Ok(()),
        Launch::Primary { controls, events } => (controls, events),
    };
    #[cfg(feature = "e2e")]
    if let Some(startup) = startup.as_mut() {
        startup.mark(observation::StartupStage::PlatformReady);
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    // Resolve and migrate the database on a worker before exposing any workspace
    // state. Close the failed launch without showing a substitute workspace.
    let ordinary_profile = options.profile.is_none() && !cfg!(feature = "e2e");
    let profile = options.profile;
    #[cfg(feature = "e2e")]
    if let Some(startup) = startup.as_mut() {
        startup.mark(observation::StartupStage::ProfileOpening);
    }
    let storage = runtime
        .block_on(runtime.spawn_blocking(move || open_profile(profile)))
        .unwrap_or_else(|_| Err("The profile database worker stopped unexpectedly".to_owned()));
    #[cfg(feature = "e2e")]
    if storage.is_ok()
        && let Some(startup) = startup.as_mut()
    {
        startup.mark(observation::StartupStage::ProfileOpened);
    }
    if options.notification_com_server {
        let event = runtime.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(30), async {
                while let Some(event) = events.recv().await {
                    if let threadline_platform::PlatformEvent::ActivityActivated(activity) = event {
                        return Some(activity);
                    }
                }
                None
            })
            .await
            .ok()
            .flatten()
        });
        let Some(event) = event else {
            return Err("Windows did not supply a notification action".into());
        };
        let profile = storage.as_ref().map_err(|e| e.to_owned())?;
        if !activity_delivery::activate(profile, event)? {
            return Ok(());
        }
    }
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
        .map(|path| {
            let (observer, writer) =
                observation::Observer::new(path, started, options.measure, &handle);
            observer.set_startup(startup);
            (observer, writer)
        })
        .unzip();
    #[cfg(not(feature = "e2e"))]
    let _ = started;
    #[cfg(feature = "e2e")]
    let mark_startup = {
        let observer = observer.clone();
        move |stage| {
            if let Some(observer) = &observer {
                observer.mark_startup(stage);
            }
        }
    };
    #[cfg(feature = "e2e")]
    mark_startup(observation::StartupStage::ApplicationConstructionStarted);
    let application = gpui_kit::application();
    #[cfg(feature = "e2e")]
    mark_startup(observation::StartupStage::ApplicationConstructed);
    application
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            #[cfg(feature = "e2e")]
            mark_startup(observation::StartupStage::ApplicationCallbackEntered);
            cx.set_global(activity_delivery::ActivityInbox);
            clock.install(cx);
            cx.set_quit_mode(QuitMode::LastWindowClosed);
            gpui_kit::init(cx);
            palette::install_theme(cx);
            #[cfg(feature = "e2e")]
            mark_startup(observation::StartupStage::ComponentsInitialized);
            bind_shortcuts(cx);
            reader_workspace::bind_shortcuts(cx);
            #[cfg(feature = "e2e")]
            mark_startup(observation::StartupStage::ApplicationActivationRequested);
            cx.activate(true);
            #[cfg(feature = "e2e")]
            mark_startup(observation::StartupStage::ToolkitReady);
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
                #[cfg(feature = "e2e")]
                if let Some(observer) = observer.as_ref() {
                    observer.mark_startup(observation::StartupStage::WindowCreated);
                }
                cx.new(|cx| {
                    let mut workspace =
                        Workspace::new(window, cx, seed, handle, open_review, feasibility_fixture);
                    if ordinary_profile {
                        workspace.enable_updates(window, cx);
                    }
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
