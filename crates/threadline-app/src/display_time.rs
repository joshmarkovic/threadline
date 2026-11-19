use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use gpui_kit::{App, Global};
use threadline_core::{display_time::DisplayZone, sync::policy::system_timezone};
use threadline_platform::{NativePaths, PathResolver};
use tokio::sync::watch;

#[derive(Clone)]
pub(crate) struct DisplayClock {
    file: PathBuf,
    system: DisplayZone,
    device: bool,
    selected: Option<DisplayZone>,
    changes: watch::Sender<Option<DisplayZone>>,
    pub(crate) warning: Option<&'static str>,
}

impl Global for DisplayClock {}

pub(crate) type CalendarZone = watch::Receiver<Option<DisplayZone>>;

fn device_zone() -> DisplayZone {
    system_timezone()
        .ok()
        .and_then(|name| DisplayZone::named(&name).ok())
        .unwrap_or(DisplayZone::UTC)
}

/// Workers consult the OS at the point a calendar rule is evaluated in device mode.
pub(crate) fn calendar_zone(receiver: &CalendarZone) -> DisplayZone {
    receiver.borrow().unwrap_or_else(device_zone)
}

impl DisplayClock {
    pub(crate) fn open(profile: Option<&Path>) -> Result<Self, String> {
        let (config, system) = match profile {
            Some(root) => (root.join("Config"), DisplayZone::UTC),
            None => (
                NativePaths
                    .application_paths()
                    .map_err(|e| e.to_string())?
                    .config,
                device_zone(),
            ),
        };
        Ok(Self::load(&config, system, profile.is_none()))
    }

    fn load(config: &Path, system: DisplayZone, device: bool) -> Self {
        let file = config.join("display-timezone");
        let result = match fs::read_to_string(&file) {
            Ok(text) => Self::parse(text.trim()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err("The saved display time zone could not be read."),
        };
        let selected = result.unwrap_or_default();
        Self {
            file,
            system,
            device,
            selected,
            changes: watch::channel(selected.or_else(|| (!device).then_some(system))).0,
            warning: result.err(),
        }
    }

    fn parse(value: &str) -> Result<Option<DisplayZone>, &'static str> {
        if value.is_empty() || value == "system" {
            return Ok(None);
        }
        DisplayZone::named(value).map(Some)
    }

    pub(crate) fn current(cx: &App) -> DisplayZone {
        cx.try_global::<Self>()
            .map_or(DisplayZone::UTC, Self::effective)
    }

    pub(crate) fn subscribe(cx: &App) -> CalendarZone {
        cx.try_global::<Self>().map_or_else(
            || watch::channel(Some(DisplayZone::UTC)).1,
            |clock| clock.changes.subscribe(),
        )
    }

    pub(crate) fn effective(&self) -> DisplayZone {
        self.selected.unwrap_or(self.system)
    }

    pub(crate) fn device_name(&self) -> &'static str {
        self.system.name()
    }

    pub(crate) fn install(self, cx: &mut App) {
        let device = self.device;
        cx.set_global(self);
        if device {
            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_secs(60))
                        .await;
                    cx.update(Self::refresh_device);
                }
            })
            .detach();
        }
    }

    fn refreshed(&self, lookup: impl FnOnce() -> DisplayZone) -> Option<Self> {
        if !self.device || self.selected.is_some() {
            return None;
        }
        let zone = lookup();
        (zone != self.system).then(|| Self {
            system: zone,
            ..self.clone()
        })
    }

    pub(crate) fn refresh_device(cx: &mut App) {
        if let Some(clock) = cx
            .try_global::<Self>()
            .and_then(|clock| clock.refreshed(device_zone))
        {
            cx.set_global(clock);
            cx.refresh_windows();
        }
    }

    pub(crate) fn setting(&self) -> &'static str {
        self.selected.map_or("system", DisplayZone::name)
    }

    pub(crate) fn save(mut self, value: &str) -> Result<Self, &'static str> {
        self.selected = Self::parse(value.trim())?;
        if self.device && self.selected.is_none() {
            self.system = device_zone();
        }
        self.warning = None;
        let dir = self
            .file
            .parent()
            .ok_or("The configuration directory is unavailable.")?;
        let write = || -> io::Result<()> {
            fs::create_dir_all(dir)?;
            let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
            writeln!(tmp, "{}", self.setting())?;
            tmp.as_file().sync_all()?;
            tmp.persist(&self.file)?;
            Ok(())
        };
        write().map_err(|_| "The display time zone could not be saved.")?;
        self.changes.send_replace(
            self.selected
                .or_else(|| (!self.device).then_some(self.system)),
        );
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_timezone_refreshes_system_mode_without_changing_user_or_isolated_modes() {
        let dir = tempfile::tempdir().unwrap();
        let ny = DisplayZone::named("America/New_York").unwrap();
        let device = DisplayClock::load(dir.path(), DisplayZone::UTC, true);
        assert_eq!(*device.changes.borrow(), None);
        let updated = device.refreshed(|| ny).unwrap();
        assert_eq!(updated.effective(), ny);
        assert!(updated.refreshed(|| ny).is_none());
        let worker = updated.changes.subscribe();
        let selected = updated.save("UTC").unwrap();
        assert_eq!(calendar_zone(&worker), DisplayZone::UTC);
        assert!(
            selected
                .refreshed(|| panic!("explicit mode consulted the OS"))
                .is_none()
        );
        let reset = selected.save("system").unwrap();
        assert!(reset.changes.borrow().is_none());
        let isolated = DisplayClock::load(dir.path(), DisplayZone::UTC, false);
        assert!(
            isolated
                .refreshed(|| panic!("isolated mode consulted the OS"))
                .is_none()
        );
        assert_eq!(
            calendar_zone(&isolated.changes.subscribe()),
            DisplayZone::UTC
        );
    }
    #[test]
    fn display_timezone_persists_separately_from_the_database_and_isolated_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let system = DisplayZone::named("Europe/Berlin").unwrap();
        let clock = DisplayClock::load(dir.path(), system, false);
        assert_eq!(clock.effective(), system);
        clock.save("America/New_York").unwrap();
        let saved = DisplayClock::load(dir.path(), system, false);
        assert_eq!(saved.setting(), "America/New_York");
        assert!(saved.clone().save("../not-a-zone").is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join("display-timezone")).unwrap(),
            "America/New_York\n"
        );
        let isolated = DisplayClock::open(Some(dir.path())).unwrap();
        assert_eq!(isolated.effective(), DisplayZone::UTC);
        saved.save("system").unwrap();
        assert_eq!(
            DisplayClock::load(dir.path(), system, false).effective(),
            system
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
