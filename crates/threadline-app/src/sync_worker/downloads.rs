use super::*;

pub(super) struct ActiveDownload {
    pub(super) revision: i64,
    pub(super) generation: i64,
    pub(super) cancel: Cancellation,
}

pub(super) async fn start(
    core: &Arc<Shared>,
    store: &Arc<Store>,
    reader: &ReaderService,
    account: AccountId,
    scope_generation: u64,
    now: i64,
) -> bool {
    let Ok(Some(claim)) = blocking(store, move |store| store.claim_pr_download(account, now)).await
    else {
        return false;
    };
    let cancel = core.shutdown.child_token();
    {
        let mut scope = core.scope.lock().expect("sync scope");
        if scope.generation != scope_generation {
            cancel.cancel();
        }
        scope.download = Some(ActiveDownload {
            revision: claim.revision.id,
            generation: claim.attempt.generation,
            cancel: cancel.clone(),
        });
    }
    let core = Arc::clone(core);
    let store = Arc::clone(store);
    let reader = reader.clone();
    tokio::spawn(async move {
        let observed = core
            .observer
            .lock()
            .expect("connectivity observer")
            .begin_request();
        let publish = || {
            core.download_updates.fetch_add(1, Ordering::Release);
            core.changed.notify_one();
        };
        publish();
        let result = if cancel.is_cancelled() {
            Err(ReaderError::Cancelled)
        } else {
            reader
                .run_claimed_pr_download(&claim, &cancel, publish)
                .await
        };
        if let Some(observed) = observed {
            let (reachability, reason) = match &result {
                Ok(_) => (Reachability::ReceivedResponse, None),
                Err(error) => (
                    error.reachability(),
                    match error {
                        ReaderError::Remote(remote) => Some(remote.kind),
                        _ => None,
                    },
                ),
            };
            core.record(observed, reachability, reason);
        }
        if result.is_err() {
            let backoff = if cancel.is_cancelled() { 0 } else { 30_000 };
            let due = UtcMillis::now()
                .map_or(now, |at| at.0)
                .saturating_add(backoff);
            let _ = blocking(&store, move |store| store.defer_pr_download(&claim, due)).await;
        }
        core.scope.lock().expect("sync scope").download = None;
        publish();
    });
    true
}
