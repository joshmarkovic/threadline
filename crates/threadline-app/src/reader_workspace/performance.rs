//! Correlation for owned synthetic Section 16 actions and their first useful paint.

use serde::Serialize;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "target", rename_all = "snake_case")]
pub(crate) enum Action {
    Search(u64),
    Open(i64),
    Tab(i64),
}

#[derive(Clone, Copy, Serialize)]
pub(crate) struct PaintTiming {
    #[serde(flatten)]
    pub(crate) action: Action,
    pub(crate) handler_qpc: i64,
    pub(crate) painted_qpc: Option<i64>,
    pub(crate) frequency: i64,
}

pub(super) struct ReaderTiming {
    frequency: Option<i64>,
    pending: Option<PaintTiming>,
}

impl ReaderTiming {
    pub(super) fn new() -> Self {
        let frequency = if std::env::var_os("THREADLINE_E2E_READER_TIMING").as_deref()
            == Some(std::ffi::OsStr::new("1"))
        {
            frequency()
        } else {
            None
        };
        Self {
            frequency,
            pending: None,
        }
    }

    pub(super) fn start(&mut self, action: Action) {
        self.pending = self.frequency.and_then(|frequency| {
            counter().map(|handler_qpc| PaintTiming {
                action,
                handler_qpc,
                painted_qpc: None,
                frequency,
            })
        });
    }

    pub(super) fn ready(
        &self,
        active_item: Option<i64>,
        content_ready: bool,
        search_generation: Option<u64>,
    ) -> Option<PaintTiming> {
        self.pending.filter(|timing| match timing.action {
            Action::Search(expected) => search_generation == Some(expected),
            Action::Open(expected) | Action::Tab(expected) => {
                content_ready && active_item == Some(expected)
            }
        })
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn QueryPerformanceCounter(value: *mut i64) -> i32;
    fn QueryPerformanceFrequency(value: *mut i64) -> i32;
}

fn frequency() -> Option<i64> {
    #[cfg(windows)]
    {
        let mut value = 0;
        // Windows exposes a common high-resolution counter to every process on this host.
        (unsafe { QueryPerformanceFrequency(&mut value) } != 0 && value > 0).then_some(value)
    }
    #[cfg(not(windows))]
    None
}

pub(crate) fn counter() -> Option<i64> {
    #[cfg(windows)]
    {
        let mut value = 0;
        (unsafe { QueryPerformanceCounter(&mut value) } != 0 && value > 0).then_some(value)
    }
    #[cfg(not(windows))]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn performance_action_is_ready_only_for_the_expected_search_or_cached_item() {
        for action in [Action::Search(3), Action::Open(12), Action::Tab(12)] {
            let timing = ReaderTiming {
                frequency: Some(1_000),
                pending: Some(PaintTiming {
                    action,
                    handler_qpc: 10,
                    painted_qpc: None,
                    frequency: 1_000,
                }),
            };
            assert!(timing.ready(Some(11), true, Some(2)).is_none());
            assert!(timing.ready(Some(12), false, Some(2)).is_none());
            let ready = timing.ready(Some(12), true, Some(3)).unwrap();
            assert!(ready.action == action);
            assert_eq!(ready.handler_qpc, 10);
            assert_eq!(ready.painted_qpc, None);
            let serialized = serde_json::to_value(ready).unwrap();
            assert!(serialized["kind"].is_string());
            assert!(serialized["target"].is_number());
        }
    }
}
