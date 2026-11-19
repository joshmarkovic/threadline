use super::*;
use threadline_storage::{
    ItemId, pr::PrDownloadClaim, reader::IssueDownloadClaim, resources::ResourceError,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DownloadKey {
    Issue(AccountId, ItemId),
    PullRequest(i64),
}

#[derive(Clone, Copy)]
pub(super) enum DownloadKind {
    Issue,
    PullRequest,
}

enum Claim {
    Issue(IssueDownloadClaim),
    PullRequest(PrDownloadClaim),
}

impl Claim {
    fn identity(&self) -> (DownloadKey, i64) {
        match self {
            Self::Issue(claim) => (
                DownloadKey::Issue(claim.account, claim.item),
                claim.generation,
            ),
            Self::PullRequest(claim) => (
                DownloadKey::PullRequest(claim.revision.id),
                claim.attempt.generation,
            ),
        }
    }
}

pub(super) struct ActiveDownload {
    pub(super) key: DownloadKey,
    pub(super) generation: i64,
    pub(super) cancel: Cancellation,
}

pub(super) async fn start(
    core: &Arc<Shared>,
    store: &Arc<Store>,
    reader: &ReaderService,
    account: AccountId,
    kind: DownloadKind,
    scope_generation: u64,
    now: i64,
) -> bool {
    let Ok(Some(claim)) = blocking(store, move |store| match kind {
        DownloadKind::Issue => store
            .claim_issue_download(account, now)
            .map(|claim| claim.map(Claim::Issue)),
        DownloadKind::PullRequest => store
            .claim_pr_download(account, now)
            .map(|claim| claim.map(Claim::PullRequest)),
    })
    .await
    else {
        return false;
    };
    let (key, generation) = claim.identity();
    let cancel = core.shutdown.child_token();
    {
        let mut scope = core.scope.lock().expect("sync scope");
        if scope.generation != scope_generation {
            cancel.cancel();
        }
        scope.download = Some(ActiveDownload {
            key,
            generation,
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
            match &claim {
                Claim::Issue(claim) => reader.run_issue_download_step(claim, &cancel).await,
                Claim::PullRequest(claim) => reader
                    .run_claimed_pr_download(claim, &cancel, publish)
                    .await
                    .map(|_| true),
            }
        };
        if let Some(observed) = observed {
            let (reachability, reason) = match &result {
                Ok(true) => (Reachability::ReceivedResponse, None),
                Ok(false) => (Reachability::NotAttempted, None),
                Err(ReaderError::IncompleteIssueDownload) => (Reachability::ReceivedResponse, None),
                Err(ReaderError::IncompleteAccess) => (
                    Reachability::ReceivedResponse,
                    Some(AccessKind::SsoRequired),
                ),
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
        if let Err(error) = result {
            let now = UtcMillis::now().map_or(now, |at| at.0);
            let interrupted = cancel.is_cancelled()
                || matches!(error, ReaderError::Offline | ReaderError::Cancelled);
            let _ = blocking(&store, move |store| match claim {
                Claim::PullRequest(claim) => {
                    let due = now.saturating_add(if interrupted { 0 } else { 30_000 });
                    store.defer_pr_download(&claim, due)
                }
                Claim::Issue(claim) => {
                    let scope_changed = matches!(error, ReaderError::ScopeChanged);
                    let reason =
                        (!interrupted && !scope_changed).then(|| error.resource_error(false));
                    let due = match reason {
                        Some(
                            ResourceError::Forbidden
                            | ResourceError::NotFound
                            | ResourceError::InvalidResponse,
                        ) => None,
                        Some(_) => Some(now.saturating_add(30_000)),
                        None if scope_changed => Some(now.saturating_add(1_000)),
                        None => Some(now),
                    };
                    store.defer_issue_download(&claim, due, reason)
                }
            })
            .await;
        }
        core.scope.lock().expect("sync scope").download = None;
        publish();
    });
    true
}
