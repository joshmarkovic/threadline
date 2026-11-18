use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::watch;

use crate::domain::ItemKey;
use crate::sync::Clock;

use super::{ItemSummary, Provider, ProviderError, ProviderFuture, ReadSummary};

pub struct ScriptedReply {
    item: ItemKey,
    delay: Duration,
    result: Result<ItemSummary, ProviderError>,
}

impl ScriptedReply {
    pub fn success(item: &ItemKey, delay: Duration, value: ItemSummary) -> Self {
        Self {
            item: item.clone(),
            delay,
            result: Ok(value),
        }
    }

    pub fn failure(item: &ItemKey, delay: Duration, error: ProviderError) -> Self {
        Self {
            item: item.clone(),
            delay,
            result: Err(error),
        }
    }
}

#[derive(Default)]
struct Script {
    replies: HashMap<ItemKey, VecDeque<ScriptedReply>>,
    requests: Vec<ItemKey>,
}

pub struct ScriptedProvider {
    clock: Arc<dyn Clock>,
    script: Mutex<Script>,
    request_count: watch::Sender<usize>,
}

impl ScriptedProvider {
    pub fn new(clock: Arc<dyn Clock>, replies: impl IntoIterator<Item = ScriptedReply>) -> Self {
        let (request_count, _) = watch::channel(0);
        let provider = Self {
            clock,
            script: Mutex::default(),
            request_count,
        };
        for reply in replies {
            provider.push(reply);
        }
        provider
    }

    pub fn push(&self, reply: ScriptedReply) {
        let mut script = self.script.lock().expect("fixture lock unpoisoned");
        script
            .replies
            .entry(reply.item.clone())
            .or_default()
            .push_back(reply);
    }

    pub fn requests(&self) -> Vec<ItemKey> {
        self.script
            .lock()
            .expect("fixture lock unpoisoned")
            .requests
            .clone()
    }

    pub async fn wait_for_requests(&self, minimum: usize) {
        let _ = self
            .request_count
            .subscribe()
            .wait_for(|count| *count >= minimum)
            .await;
    }

    fn take_reply(&self, item: &ItemKey) -> (Duration, Option<ScriptedReply>) {
        let mut script = self.script.lock().expect("fixture lock unpoisoned");
        let start = self.clock.now();
        script.requests.push(item.clone());
        let reply = script.replies.get_mut(item).and_then(VecDeque::pop_front);
        self.request_count.send_replace(script.requests.len());
        (start, reply)
    }
}

impl Provider for ScriptedProvider {
    fn read_summary<'a>(&'a self, request: &'a ReadSummary) -> ProviderFuture<'a> {
        Box::pin(async move {
            let (start, reply) = self.take_reply(request.item());
            let reply = reply.ok_or(ProviderError::NotScripted)?;
            self.clock
                .sleep_until(start.saturating_add(reply.delay))
                .await;
            reply.result
        })
    }
}
