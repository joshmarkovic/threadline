use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::profiler::{FrameEvent, FrameTimingCollector, set_trace_enabled};
use gpui_kit::{IntoElement, ScrollEvent, Styled, Window, canvas};
use serde::Serialize;

#[derive(Clone)]
pub(crate) struct Observer(Rc<RefCell<Recorder>>);

struct Recorder {
    started: Instant,
    startup_ms: Option<f64>,
    path: PathBuf,
    bounds: BTreeMap<&'static str, [f32; 4]>,
    previous_report: Vec<u8>,
    measurement: Option<Measurement>,
}

struct Measurement {
    collector: FrameTimingCollector,
    last_scroll: Option<Instant>,
    pending_frame: Option<Instant>,
    frame_times: Vec<f64>,
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
    pub activations: u32,
}

#[derive(Serialize)]
struct Report<'a> {
    #[serde(flatten)]
    snapshot: Snapshot,
    pid: u32,
    scale_factor: f32,
    startup_ms: f64,
    bounds: &'a BTreeMap<&'static str, [f32; 4]>,
    scroll_frame_times_ms: &'a [f64],
}

impl Observer {
    pub(crate) fn new(path: PathBuf, started: Instant, measure: bool) -> Self {
        let measurement = measure.then(|| {
            set_trace_enabled(true);
            Measurement {
                collector: FrameTimingCollector::new(),
                last_scroll: None,
                pending_frame: None,
                frame_times: Vec::new(),
            }
        });
        Self(Rc::new(RefCell::new(Recorder {
            started,
            startup_ms: None,
            path,
            bounds: BTreeMap::new(),
            previous_report: Vec::new(),
            measurement,
        })))
    }

    pub(crate) fn begin_frame(&self) {
        self.0.borrow_mut().bounds.clear();
    }

    pub(crate) fn observed_bounds(&self, name: &'static str) -> impl IntoElement {
        let observer = self.clone();
        canvas(
            move |bounds, _, _| {
                observer.0.borrow_mut().bounds.insert(
                    name,
                    [
                        bounds.origin.x.as_f32(),
                        bounds.origin.y.as_f32(),
                        bounds.size.width.as_f32(),
                        bounds.size.height.as_f32(),
                    ],
                );
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

    pub(crate) fn frame_report(&self, snapshot: Snapshot) -> impl IntoElement {
        let observer = self.clone();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                if observer.0.borrow().measurement.is_some() {
                    let measure = observer.clone();
                    window.on_mouse_event(move |_: &ScrollEvent, phase, _, _| {
                        if phase.capture() {
                            measure.scrolled();
                        }
                    });
                }
                observer.write(snapshot, window);
            },
        )
        .absolute()
        .size_0()
    }

    fn write(&self, snapshot: Snapshot, window: &Window) {
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
        let frame_times = if let Some(measurement) = measurement {
            measurement.collect();
            &measurement.frame_times[..]
        } else {
            &[]
        };
        let report = Report {
            snapshot,
            pid: std::process::id(),
            scale_factor: window.scale_factor(),
            startup_ms,
            bounds,
            scroll_frame_times_ms: frame_times,
        };
        let Ok(json) = serde_json::to_vec(&report) else {
            return;
        };
        if json == recorder.previous_report {
            return;
        }
        let temporary = recorder.path.with_extension("json.pending");
        let result = std::fs::write(&temporary, &json)
            .and_then(|()| std::fs::rename(temporary, &recorder.path));
        match result {
            Ok(()) => recorder.previous_report = json,
            Err(error) => eprintln!("Could not write native test observations: {error}"),
        }
    }
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
                        .map(|_| frame.dirty_at.unwrap_or(frame.draw_start));
                }
                FrameEvent::Present(present) => {
                    if let Some(start) = self.pending_frame.take()
                        && self.frame_times.len() < 4_096
                    {
                        self.frame_times.push(
                            present
                                .present_end
                                .saturating_duration_since(start)
                                .as_secs_f64()
                                * 1_000.,
                        );
                    }
                }
            }
        }
    }
}
