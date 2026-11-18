use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
};

use tokio::sync::watch;

use super::{AccountId, Cancellation, ItemId, PageStatus, ReaderError};

pub(super) type Key = (AccountId, ItemId, &'static str);
type Result = std::result::Result<PageStatus, ReaderError>;

#[derive(Default)]
pub(crate) struct Flights(Mutex<HashMap<Key, Weak<Flight>>>);

struct Flight {
    cancellation: Cancellation,
    subscribers: AtomicUsize,
    result: watch::Sender<Option<Result>>,
}

struct Subscription(Arc<Flight>);

struct Producer(Arc<Flight>);

impl Drop for Producer {
    fn drop(&mut self) {
        self.0.result.send_if_modified(|result| {
            if result.is_some() {
                return false;
            }
            *result = Some(Err(ReaderError::Cancelled));
            true
        });
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if self.0.subscribers.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.0.cancellation.cancel();
        }
    }
}

impl Flights {
    pub(super) async fn read<F>(
        &self,
        key: Key,
        subscriber: &Cancellation,
        work: impl FnOnce(Cancellation) -> F + Send + 'static,
    ) -> Result
    where
        F: Future<Output = Result> + Send + 'static,
    {
        if subscriber.is_cancelled() {
            return Err(ReaderError::Cancelled);
        }
        let (subscription, first) = self.subscribe(key);
        let flight = &subscription.0;
        let mut results = flight.result.subscribe();
        if first {
            let worker = Producer(flight.clone());
            tokio::spawn(async move {
                let finished = work(worker.0.cancellation.clone()).await;
                worker.0.result.send_replace(Some(finished));
            });
        }
        tokio::select! {
            biased;
            _ = subscriber.cancelled() => Err(ReaderError::Cancelled),
            output = results.wait_for(Option::is_some) => {
                output.ok().and_then(|value| value.as_ref().cloned()).ok_or(ReaderError::Cancelled)?
            },
        }
    }

    fn subscribe(&self, key: Key) -> (Subscription, bool) {
        let mut live = self.0.lock().expect("PR request subscriptions");
        live.retain(|_, flight| flight.strong_count() != 0);
        if let Some(flight) = live.get(&key).and_then(Weak::upgrade)
            && flight
                .subscribers
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    (count > 0).then(|| count.saturating_add(1))
                })
                .is_ok()
        {
            return (Subscription(flight), false);
        }
        let (result, _) = watch::channel(None);
        let flight = Arc::new(Flight {
            cancellation: Cancellation::new(),
            subscribers: AtomicUsize::new(1),
            result,
        });
        live.insert(key, Arc::downgrade(&flight));
        (Subscription(flight), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_service_one_departing_subscriber_cannot_cancel_the_other() {
        let flights = Flights::default();
        let key = (AccountId(1), ItemId(2), "files");
        let (first, spawned) = flights.subscribe(key);
        assert!(spawned);
        let (second, spawned) = flights.subscribe(key);
        assert!(!spawned);
        let transport = first.0.cancellation.clone();
        drop(first);
        assert!(!transport.is_cancelled());
        drop(second);
        assert!(transport.is_cancelled());
        let (_, spawned) = flights.subscribe(key);
        assert!(spawned);
    }
}
