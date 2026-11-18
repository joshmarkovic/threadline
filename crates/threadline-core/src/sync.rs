use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::{Semaphore, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::domain::{ItemKey, ViewSelection};
use crate::github::{ItemSummary, Provider, ProviderError, ReadSummary};

pub use tokio_util::sync::CancellationToken as Cancellation;

pub type Wait<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// Monotonic elapsed time within a single process or test clock.
pub trait Clock: Send + Sync {
    fn now(&self) -> Duration;
    fn sleep_until(&self, deadline: Duration) -> Wait<'_>;
}

pub struct TokioClock {
    origin: Instant,
}

impl TokioClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for TokioClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for TokioClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    fn sleep_until(&self, deadline: Duration) -> Wait<'_> {
        Box::pin(tokio::time::sleep(deadline.saturating_sub(self.now())))
    }
}

pub struct ManualClock {
    time: watch::Sender<Duration>,
}

impl ManualClock {
    pub fn new() -> Self {
        let (time, _) = watch::channel(Duration::ZERO);
        Self { time }
    }

    pub fn advance(&self, elapsed: Duration) {
        self.time
            .send_modify(|time| *time = time.saturating_add(elapsed));
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        *self.time.borrow()
    }

    fn sleep_until(&self, deadline: Duration) -> Wait<'_> {
        let mut time = self.time.subscribe();
        Box::pin(async move {
            let _ = time.wait_for(|now| *now >= deadline).await;
        })
    }
}

#[derive(Debug)]
pub struct ReadOutcome {
    pub item: ItemKey,
    pub view: Option<ViewSelection>,
    pub result: Result<ItemSummary, ProviderError>,
}

impl ReadOutcome {
    /// A result may update a view only while that view still owns the same selection.
    pub fn matches_view(&self, item: &ItemKey, view: ViewSelection) -> bool {
        &self.item == item && self.view == Some(view)
    }
}

#[derive(Clone)]
pub struct Scheduler {
    runtime: Handle,
    provider: Arc<dyn Provider>,
    admitted: Arc<Semaphore>,
    in_flight: Arc<Semaphore>,
}

impl Scheduler {
    pub fn new(runtime: Handle, provider: Arc<dyn Provider>, parallelism: NonZeroUsize) -> Self {
        let parallelism = parallelism.get().min(Semaphore::MAX_PERMITS);
        let capacity = parallelism.saturating_mul(8).min(Semaphore::MAX_PERMITS);
        Self {
            runtime,
            provider,
            admitted: Arc::new(Semaphore::new(capacity)),
            in_flight: Arc::new(Semaphore::new(parallelism)),
        }
    }

    pub fn spawn(
        &self,
        request: ReadSummary,
        view: Option<ViewSelection>,
        cancellation: Cancellation,
    ) -> JoinHandle<ReadOutcome> {
        let provider = Arc::clone(&self.provider);
        let in_flight = Arc::clone(&self.in_flight);
        let admission = Arc::clone(&self.admitted).try_acquire_owned();
        self.runtime.spawn(async move {
            let result = tokio::select! {
                biased;
                _ = cancellation.cancelled() => Err(ProviderError::Cancelled),
                result = async {
                    let _admission = admission.map_err(|_| ProviderError::SchedulerBusy)?;
                    let _in_flight = in_flight.acquire().await.expect("scheduler permits stay open");
                    let summary = provider.read_summary(&request).await?;
                    if summary.item() != request.item() {
                        return Err(ProviderError::WrongResource);
                    }
                    Ok(summary)
                } => result,
            };
            ReadOutcome {
                item: request.item().clone(),
                view,
                result,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::FeasibilityFixture;
    use crate::github::{ScriptedProvider, ScriptedReply};

    #[tokio::test]
    async fn simulated_time_controls_completion_and_stale_view_rejection() {
        let fixture = FeasibilityFixture::new(42);
        let clock = Arc::new(ManualClock::new());
        let delay = Duration::from_secs(10);
        let provider = Arc::new(ScriptedProvider::new(
            clock.clone(),
            [ScriptedReply::success(
                fixture.request.item(),
                delay,
                fixture.summary.clone(),
            )],
        ));
        let scheduler = Scheduler::new(Handle::current(), provider.clone(), NonZeroUsize::MIN);
        let selection = ViewSelection {
            tab: 2.try_into().unwrap(),
            generation: 7,
        };
        let result = scheduler.spawn(
            fixture.request.clone(),
            Some(selection),
            Cancellation::new(),
        );

        provider.wait_for_requests(1).await;
        clock.advance(Duration::from_secs(9));
        tokio::task::yield_now().await;
        assert!(!result.is_finished());
        clock.advance(Duration::from_secs(1));
        let outcome = result.await.unwrap();

        assert!(outcome.matches_view(fixture.request.item(), selection));
        assert!(!outcome.matches_view(
            fixture.request.item(),
            ViewSelection {
                generation: 8,
                ..selection
            }
        ));
        assert!(!outcome.matches_view(
            fixture.request.item(),
            ViewSelection {
                tab: 3.try_into().unwrap(),
                ..selection
            }
        ));
        assert_eq!(outcome.result.unwrap(), fixture.summary);
    }

    #[tokio::test]
    async fn cancellation_and_queue_limit_do_not_dispatch_unwanted_requests() {
        let fixture = FeasibilityFixture::new(7);
        let clock = Arc::new(ManualClock::new());
        let provider = Arc::new(ScriptedProvider::new(
            clock.clone(),
            [ScriptedReply::success(
                fixture.request.item(),
                Duration::from_secs(30),
                fixture.summary.clone(),
            )],
        ));
        let scheduler = Scheduler::new(Handle::current(), provider.clone(), NonZeroUsize::MIN);
        let running_cancel = Cancellation::new();
        let running = scheduler.spawn(fixture.request.clone(), None, running_cancel.clone());
        provider.wait_for_requests(1).await;

        let queued_cancel = Cancellation::new();
        let queued = (0..7)
            .map(|_| scheduler.spawn(fixture.request.clone(), None, queued_cancel.clone()))
            .collect::<Vec<_>>();
        let excess = scheduler.spawn(fixture.request.clone(), None, Cancellation::new());
        assert_eq!(
            excess.await.unwrap().result,
            Err(ProviderError::SchedulerBusy)
        );

        queued_cancel.cancel();
        for request in queued {
            assert_eq!(request.await.unwrap().result, Err(ProviderError::Cancelled));
        }
        running_cancel.cancel();
        assert_eq!(running.await.unwrap().result, Err(ProviderError::Cancelled));
        assert_eq!(provider.requests(), [fixture.request.item().clone()]);
        assert_eq!(clock.now(), Duration::ZERO);

        provider.push(ScriptedReply::success(
            fixture.request.item(),
            Duration::ZERO,
            fixture.summary,
        ));
        assert!(
            scheduler
                .spawn(fixture.request, None, Cancellation::new())
                .await
                .unwrap()
                .result
                .is_ok()
        );
    }

    #[tokio::test]
    async fn mismatched_provider_item_is_rejected_even_for_a_current_selection() {
        let fixture = FeasibilityFixture::new(4);
        let clock = Arc::new(ManualClock::new());
        let mut wrong_item = fixture.request.item().clone();
        wrong_item.number = 43.try_into().unwrap();
        let wrong_summary = ItemSummary::new(
            wrong_item,
            fixture.summary.revision().clone(),
            fixture.summary.issue_api_id,
            fixture.summary.kind,
            "Wrong item".into(),
            None,
        );
        let provider = Arc::new(ScriptedProvider::new(
            clock,
            [ScriptedReply::success(
                fixture.request.item(),
                Duration::ZERO,
                wrong_summary,
            )],
        ));
        let scheduler = Scheduler::new(Handle::current(), provider.clone(), NonZeroUsize::MIN);
        let result = scheduler
            .spawn(fixture.request, None, Cancellation::new())
            .await
            .unwrap();
        assert_eq!(result.result, Err(ProviderError::WrongResource));
    }
}
