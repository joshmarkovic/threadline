//! One bounded, passive frame sample for the isolated large-reader journey.

use std::{
    path::Path,
    time::{Duration, Instant},
};

use gpui_kit::{
    WindowId,
    profiler::{FrameEvent, FrameTimingCollector, set_trace_enabled},
};
use serde::Serialize;
use tokio::{runtime::Handle, sync::oneshot, time::sleep};

const SETTLE: Duration = Duration::from_secs(2);
const SECONDS: usize = 3;
const SAMPLE: Duration = Duration::from_secs(SECONDS as u64);

#[derive(Serialize)]
struct IdleSample {
    pid: u32,
    settle_ms: u128,
    sample_ms: u128,
    draws_per_second: [u64; SECONDS],
    presents_per_second: [u64; SECONDS],
}

pub(super) fn start(path: &Path, runtime: &Handle) -> Option<oneshot::Sender<WindowId>> {
    if std::env::var_os("THREADLINE_E2E_READER_IDLE").as_deref() != Some(std::ffi::OsStr::new("1"))
    {
        return None;
    }
    set_trace_enabled(true);
    let (trigger, ready) = oneshot::channel();
    let destination = path.with_extension("idle.json");
    runtime.spawn(async move {
        let Ok(window) = ready.await else { return };
        sleep(SETTLE).await;
        let mut collector = FrameTimingCollector::new();
        let started = Instant::now();
        sleep(SAMPLE).await;
        let mut result = IdleSample {
            pid: std::process::id(),
            settle_ms: SETTLE.as_millis(),
            sample_ms: SAMPLE.as_millis(),
            draws_per_second: [0; SECONDS],
            presents_per_second: [0; SECONDS],
        };
        for event in collector.collect_unseen() {
            let (when, counts) = match event {
                FrameEvent::Draw(frame) if frame.window_id == window => {
                    (frame.draw_start, &mut result.draws_per_second)
                }
                FrameEvent::Present(frame) if frame.window_id == window => {
                    (frame.present_end, &mut result.presents_per_second)
                }
                _ => continue,
            };
            if let Some(bin) = sample_bin(started, when) {
                counts[bin] += 1;
            }
        }
        let Ok(bytes) = serde_json::to_vec(&result) else {
            return;
        };
        let saved =
            tokio::task::spawn_blocking(move || super::save_report(&destination, &bytes)).await;
        if !matches!(saved, Ok(Ok(()))) {
            eprintln!("Could not write native idle observations.");
        }
    });
    Some(trigger)
}

fn sample_bin(started: Instant, when: Instant) -> Option<usize> {
    when.checked_duration_since(started)
        .filter(|elapsed| *elapsed < SAMPLE)
        .map(|elapsed| elapsed.as_secs() as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_sample_excludes_settling_and_counts_only_the_bounded_window() {
        let start = Instant::now() + Duration::from_secs(1);
        assert_eq!(sample_bin(start, start - Duration::from_nanos(1)), None);
        assert_eq!(sample_bin(start, start), Some(0));
        assert_eq!(
            sample_bin(start, start + Duration::from_millis(1600)),
            Some(1)
        );
        assert_eq!(
            sample_bin(start, start + Duration::from_millis(2999)),
            Some(2)
        );
        assert_eq!(sample_bin(start, start + SAMPLE), None);
    }
}
