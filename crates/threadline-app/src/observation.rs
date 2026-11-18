use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::profiler::{FrameEvent, FrameTiming, FrameTimingCollector, set_trace_enabled};
use gpui_kit::{
    Bounds, Entity, GpuSpecs, IntoElement, Pixels, ScrollEvent, Styled, Window, canvas,
};
use serde::Serialize;
use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::pane_content::DiscussionPane;

#[derive(Clone)]
pub(crate) struct Observer(Rc<RefCell<Recorder>>);

struct Recorder {
    started: Instant,
    startup_ms: Option<f64>,
    bounds: BTreeMap<Cow<'static, str>, [f32; 4]>,
    measurement: Option<Measurement>,
    reports: watch::Sender<PendingReport>,
}

#[derive(Clone, Default)]
struct PendingReport {
    bytes: Option<Arc<Vec<u8>>>,
    closing: bool,
}

pub(crate) struct ReportWriter {
    reports: watch::Sender<PendingReport>,
    task: JoinHandle<io::Result<()>>,
}

struct Measurement {
    collector: FrameTimingCollector,
    last_scroll: Option<Instant>,
    pending_frame: Option<FrameTiming>,
    frame_times: Vec<f64>,
    frame_phases: Vec<FramePhases>,
    gpu: Option<Option<GpuSpecs>>,
}

#[derive(Serialize)]
struct FramePhases {
    before_draw_ms: f64,
    draw_ms: f64,
    request_layout_ms: f64,
    prepaint_ms: f64,
    paint_ms: f64,
    before_present_ms: f64,
    platform_present_ms: f64,
}

#[derive(Serialize)]
pub(crate) struct Snapshot {
    pub ready: bool,
    pub seed: u64,
    pub fixture_revision: &'static str,
    pub focused_pane: &'static str,
    pub left_tab: u16,
    pub right_tab: u16,
    pub outer_ratio: f32,
    pub context_ratio: f32,
    pub context_mode: &'static str,
    pub left_input: String,
    pub right_input: String,
    pub preview_state: &'static str,
    pub tray_enabled: bool,
    pub notifications_shown: u32,
    pub notifications_submitted: u32,
    pub notification_com_activations: u32,
    pub activations: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_access: Option<crate::account_panel::AccountObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_reader: Option<crate::reader_workspace::ReaderObservation>,
}

#[derive(Serialize)]
struct Report<'a> {
    #[serde(flatten)]
    snapshot: Snapshot,
    pid: u32,
    scale_factor: f32,
    startup_ms: f64,
    bounds: &'a BTreeMap<Cow<'static, str>, [f32; 4]>,
    scroll_frame_times_ms: &'a [f64],
    #[serde(skip_serializing_if = "Option::is_none")]
    scroll_frame_phases: Option<&'a [FramePhases]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    renderer_adapter: Option<&'a GpuSpecs>,
}

impl Observer {
    pub(crate) fn new(
        path: PathBuf,
        started: Instant,
        measure: bool,
        runtime: &Handle,
    ) -> (Self, ReportWriter) {
        let measurement = measure.then(|| {
            set_trace_enabled(true);
            Measurement {
                collector: FrameTimingCollector::new(),
                last_scroll: None,
                pending_frame: None,
                frame_times: Vec::new(),
                frame_phases: Vec::new(),
                gpu: None,
            }
        });
        let writer = ReportWriter::new(path, runtime);
        let observer = Self(Rc::new(RefCell::new(Recorder {
            started,
            startup_ms: None,
            bounds: BTreeMap::new(),
            measurement,
            reports: writer.reports.clone(),
        })));
        (observer, writer)
    }

    pub(crate) fn begin_frame(&self) {
        self.0.borrow_mut().bounds.clear();
    }

    #[cfg(test)]
    pub(crate) fn test_control_bounds(&self, name: &str) -> Option<[f32; 4]> {
        self.0.borrow().bounds.get(name).copied()
    }

    pub(crate) fn observed_bounds(&self, name: &'static str) -> impl IntoElement {
        let observer = self.clone();
        canvas(
            move |bounds, _, _| {
                observer
                    .0
                    .borrow_mut()
                    .bounds
                    .insert(Cow::Borrowed(name), logical_bounds(bounds));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
    }

    pub(crate) fn visible_control_bounds(&self, name: String) -> impl IntoElement {
        let observer = self.clone();
        canvas(
            move |bounds, window, _| {
                let mask = window.content_mask();
                if bounds.size.width.as_f32() > 0.
                    && bounds.size.height.as_f32() > 0.
                    && mask.bounds.contains(&bounds.center())
                {
                    observer
                        .0
                        .borrow_mut()
                        .bounds
                        .insert(Cow::Owned(name), logical_bounds(bounds));
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
    }

    fn scrolled(&self) {
        let mut recorder = self.0.borrow_mut();
        if let Some(measurement) = &mut recorder.measurement {
            measurement.collect();
            measurement.last_scroll = Some(Instant::now());
        }
    }

    pub(crate) fn scroll_input(&self) -> impl IntoElement {
        let observer = self.clone();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                if observer.0.borrow().measurement.is_some() {
                    window.on_mouse_event(move |_: &ScrollEvent, phase, _, _| {
                        if phase.capture() {
                            observer.scrolled();
                        }
                    });
                }
            },
        )
        .absolute()
        .size_0()
    }

    pub(crate) fn frame_report(
        &self,
        snapshot: Snapshot,
        discussion: Option<Entity<DiscussionPane>>,
    ) -> impl IntoElement {
        let observer = self.clone();
        canvas(
            |_, _, _| {},
            move |_, _, window, cx| {
                let opening = discussion.and_then(|pane| pane.read(cx).opening_bounds(cx));
                observer.write(snapshot, opening, window);
            },
        )
        .absolute()
        .size_0()
    }

    fn write(&self, snapshot: Snapshot, opening: Option<Bounds<Pixels>>, window: &Window) {
        let mut recorder = self.0.borrow_mut();
        let elapsed_ms = recorder.started.elapsed().as_secs_f64() * 1_000.;
        let startup_ms = if snapshot.ready {
            *recorder.startup_ms.get_or_insert(elapsed_ms)
        } else {
            elapsed_ms
        };
        let Recorder {
            bounds,
            measurement,
            ..
        } = &mut *recorder;
        if let Some(opening) = opening {
            bounds.insert(Cow::Borrowed("markdown_copy"), logical_bounds(opening));
        }
        if let Some(measurement) = measurement {
            measurement.collect();
            if snapshot.ready && measurement.gpu.is_none() {
                measurement.gpu = Some(window.gpu_specs());
            }
        }
        let measurement = measurement.as_ref();
        let report = Report {
            snapshot,
            pid: std::process::id(),
            scale_factor: window.scale_factor(),
            startup_ms,
            bounds,
            scroll_frame_times_ms: measurement.map_or(&[], |data| &data.frame_times),
            scroll_frame_phases: measurement.map(|data| data.frame_phases.as_slice()),
            renderer_adapter: measurement.and_then(|data| data.gpu.as_ref()?.as_ref()),
        };
        let Ok(json) = serde_json::to_vec(&report) else {
            return;
        };
        recorder.reports.send_if_modified(move |pending| {
            if pending.closing || pending.bytes.as_deref() == Some(&json) {
                return false;
            }
            pending.bytes = Some(Arc::new(json));
            true
        });
    }
}

fn logical_bounds(bounds: Bounds<Pixels>) -> [f32; 4] {
    [
        bounds.origin.x.as_f32(),
        bounds.origin.y.as_f32(),
        bounds.size.width.as_f32(),
        bounds.size.height.as_f32(),
    ]
}

impl ReportWriter {
    fn new(path: PathBuf, runtime: &Handle) -> Self {
        let (reports, receiver) = watch::channel(PendingReport::default());
        Self {
            reports,
            task: runtime.spawn(write_reports(path, receiver)),
        }
    }

    pub(crate) async fn finish(self) -> io::Result<()> {
        self.reports.send_modify(|pending| pending.closing = true);
        self.task.await?
    }
}

async fn write_reports(
    path: PathBuf,
    mut receiver: watch::Receiver<PendingReport>,
) -> io::Result<()> {
    let path: Arc<Path> = path.into();
    let mut written = None;
    while receiver.changed().await.is_ok() {
        let PendingReport { bytes, closing } = receiver.borrow_and_update().clone();
        if let Some(bytes) = bytes
            && !written
                .as_ref()
                .is_some_and(|last| Arc::ptr_eq(last, &bytes))
        {
            let contents = Arc::clone(&bytes);
            let destination = Arc::clone(&path);
            match tokio::task::spawn_blocking(move || save_report(&destination, &contents)).await? {
                Ok(()) => written = Some(bytes),
                Err(error) if closing => return Err(error),
                Err(error) => eprintln!("Could not write native test observations: {error}"),
            }
        }
        if closing {
            break;
        }
    }
    Ok(())
}

fn save_report(path: &Path, contents: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension("json.pending");
    std::fs::write(&temporary, contents).and_then(|()| std::fs::rename(temporary, path))
}

impl Measurement {
    fn collect(&mut self) {
        for event in self.collector.collect_unseen() {
            match event {
                FrameEvent::Draw(frame) => {
                    self.pending_frame = self
                        .last_scroll
                        .filter(|scroll| {
                            frame.draw_start.saturating_duration_since(*scroll)
                                <= Duration::from_millis(200)
                                && frame.draw_start >= *scroll
                        })
                        .map(|_| frame);
                }
                FrameEvent::Present(present) => {
                    if let Some(frame) = self.pending_frame.take()
                        && frame.window_id == present.window_id
                        && self.frame_times.len() < 4_096
                    {
                        let start = frame.dirty_at.unwrap_or(frame.draw_start);
                        self.frame_times.push(milliseconds(
                            present.present_end.saturating_duration_since(start),
                        ));
                        self.frame_phases.push(FramePhases {
                            before_draw_ms: milliseconds(
                                frame.draw_start.saturating_duration_since(start),
                            ),
                            draw_ms: milliseconds(frame.draw_duration()),
                            request_layout_ms: milliseconds(frame.phases.request_layout),
                            prepaint_ms: milliseconds(frame.phases.prepaint),
                            paint_ms: milliseconds(frame.phases.paint),
                            before_present_ms: milliseconds(
                                present
                                    .present_start
                                    .saturating_duration_since(frame.draw_end),
                            ),
                            platform_present_ms: milliseconds(present.present_duration()),
                        });
                    }
                }
            }
        }
    }
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finishing_the_writer_persists_the_latest_pending_snapshot() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "threadline-observer-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("observation.json");
        std::fs::write(&path, b"{}").unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let writer = ReportWriter::new(path.clone(), runtime.handle());
        writer.reports.send_modify(|pending| {
            pending.bytes = Some(Arc::new(br#"{"seed":42,"ready":false}"#.to_vec()));
        });
        let latest = br#"{"seed":42,"ready":true}"#;
        writer.reports.send_modify(|pending| {
            pending.bytes = Some(Arc::new(latest.to_vec()));
        });
        let result = runtime.block_on(writer.finish());
        let written = std::fs::read(&path);
        let temporary = path.with_extension("json.pending");
        let temporary_exists = temporary.exists();
        if temporary_exists {
            std::fs::remove_file(temporary).unwrap();
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
        result.unwrap();
        assert_eq!(written.unwrap(), latest);
        assert!(!temporary_exists);
    }
}
