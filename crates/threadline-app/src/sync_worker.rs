//! Runs the shared SQLite read schedule on the application's Tokio runtime.

mod reads;
mod writes;
pub(crate) use writes::translate_merge_policy;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use threadline_core::{
    connectivity::{
        ConnectionSnapshot, ConnectionStatus, ConnectivityObserver, ObservationId, ProbeTrigger,
    },
    github::{AccessKind, GitHubSession, RateLimit, Reachability, reader::BoundRepositoryReader},
    sync::{Cancellation, policy::system_timezone},
};
use threadline_storage::{
    AccountId, RepositoryId, Store, UtcMillis,
    access::{AccessProfile, AccessState},
    sync::{ReadBudget, ReadTarget, SyncSettings, SyncStatus},
};
use tokio::{
    runtime::Handle,
    sync::{Notify, watch},
};

use crate::{accounts::AccountService, reader::ReaderError};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SyncUpdate {
    pub(crate) account: Option<AccountId>,
    pub(crate) connection: ConnectionSnapshot,
    pub(crate) status: Option<SyncStatus>,
    pub(crate) cache_generation: u64,
    pub(crate) history_generation: u64,
    pub(crate) eligible_outbox: u64,
}

pub(crate) struct SyncWorker {
    core: Arc<Shared>,
    updates: watch::Receiver<SyncUpdate>,
}

struct Shared {
    observer: Mutex<ConnectivityObserver>,
    scope: Mutex<Scope>,
    changed: Notify,
    started: Instant,
    shutdown: Cancellation,
}

#[derive(Default)]
struct Scope {
    generation: u64,
    ready: bool,
    requested: Option<ProbeTrigger>,
    active: Option<Active>,
}

struct Active {
    target: Option<(AccountId, ReadTarget, i64)>,
    cancel: Cancellation,
}

impl SyncWorker {
    pub(crate) fn start(accounts: AccountService, runtime: &Handle, seed: u64) -> Self {
        let network = accounts.sync_worker_enabled();
        Self::start_with_network(accounts, runtime, seed, network)
    }

    #[cfg(test)]
    pub(crate) fn start_fixture(accounts: AccountService, runtime: &Handle) -> Self {
        Self::start_with_network(accounts, runtime, 42, true)
    }

    fn start_with_network(
        accounts: AccountService,
        runtime: &Handle,
        seed: u64,
        network: bool,
    ) -> Self {
        let observer = ConnectivityObserver::new(seed);
        let (sender, updates) = watch::channel(SyncUpdate {
            account: None,
            connection: observer.snapshot(Duration::ZERO),
            status: None,
            cache_generation: 0,
            history_generation: 0,
            eligible_outbox: 0,
        });
        let core = Arc::new(Shared {
            observer: Mutex::new(observer),
            scope: Mutex::new(Scope::default()),
            changed: Notify::new(),
            started: Instant::now(),
            shutdown: Cancellation::new(),
        });
        runtime.spawn(run(accounts, core.clone(), sender, network));
        Self { core, updates }
    }

    pub(crate) fn watch(&self) -> watch::Receiver<SyncUpdate> {
        self.updates.clone()
    }

    pub(crate) fn wake(&self, trigger: Option<ProbeTrigger>) {
        if let Some(trigger) = trigger {
            let mut scope = self.core.scope.lock().expect("sync scope");
            scope.ready |= trigger == ProbeTrigger::Startup;
            scope.requested = Some(trigger);
        }
        self.core.changed.notify_one();
    }

    pub(crate) fn interrupt(&self, trigger: Option<ProbeTrigger>) {
        let mut scope = self.core.scope.lock().expect("sync scope");
        scope.generation = scope.generation.wrapping_add(1);
        if let Some(active) = &scope.active {
            active.cancel.cancel();
        }
        drop(scope);
        self.wake(trigger);
    }

    pub(crate) fn cancel_read(&self, account: AccountId, target: &ReadTarget, generation: i64) {
        let scope = self.core.scope.lock().expect("sync scope");
        if let Some(active) = &scope.active
            && active
                .target
                .as_ref()
                .is_some_and(|(owner, read, running)| {
                    *owner == account && read == target && *running == generation
                })
        {
            active.cancel.cancel();
        }
        self.core.changed.notify_one();
    }

    pub(crate) fn begin_foreground(&self) -> Option<ObservationId> {
        self.core
            .observer
            .lock()
            .expect("connectivity observer")
            .begin_request()
    }

    pub(crate) fn record_foreground(
        &self,
        id: ObservationId,
        reachability: Reachability,
        reason: Option<AccessKind>,
    ) {
        self.core.record(id, reachability, reason);
        self.wake(None);
    }
}

impl Drop for SyncWorker {
    fn drop(&mut self) {
        self.core.shutdown.cancel();
    }
}

impl Shared {
    fn record(&self, id: ObservationId, reachability: Reachability, reason: Option<AccessKind>) {
        if let Ok(now) = UtcMillis::now() {
            self.observer.lock().expect("connectivity observer").record(
                id,
                reachability,
                reason,
                now.0,
                self.started.elapsed(),
            );
        }
    }

    fn start_read(
        &self,
        generation: u64,
        target: Option<(AccountId, ReadTarget, i64)>,
    ) -> Cancellation {
        let mut scope = self.scope.lock().expect("sync scope");
        let cancel = self.shutdown.child_token();
        if scope.generation != generation {
            cancel.cancel();
        }
        scope.active = Some(Active {
            target,
            cancel: cancel.clone(),
        });
        cancel
    }

    fn finish_read(&self) {
        self.scope.lock().expect("sync scope").active = None;
    }
}

#[derive(Default)]
struct BoundReaders {
    sessions: HashMap<String, Arc<GitHubSession>>,
    repositories: HashMap<RepositoryId, Bound>,
}

struct Bound {
    generation: i64,
    credential: String,
    reader: BoundRepositoryReader,
    pr_snapshots: HashMap<i64, threadline_core::github::reader::pr::PrSnapshot>,
}

impl BoundReaders {
    fn clear_repository(&mut self, repository: RepositoryId) {
        if let Some(bound) = self.repositories.remove(&repository) {
            self.sessions.remove(&bound.credential);
            self.repositories
                .retain(|_, entry| entry.credential != bound.credential);
        }
    }

    fn retain_profiles(&mut self, profiles: &[AccessProfile]) {
        let current = |credential: &str| {
            profiles.iter().any(|profile| {
                profile.credential_ref == credential && profile.access == AccessState::Readable
            })
        };
        self.sessions.retain(|credential, _| current(credential));
        self.repositories
            .retain(|_, bound| current(&bound.credential));
    }
}

struct Local {
    account: AccountId,
    settings: SyncSettings,
    status: SyncStatus,
    waiting: u64,
    profiles: Vec<AccessProfile>,
    next_read: Option<i64>,
    next_write: Option<i64>,
}

impl Local {
    fn load(
        store: &Store,
        now: i64,
        network: bool,
        enroll: bool,
    ) -> threadline_storage::Result<Option<Self>> {
        let Some(account) = store.active_github_account()? else {
            return Ok(None);
        };
        let settings = store.sync_settings(account)?;
        if network && enroll {
            store.schedule_selected_activity(account, now)?;
        }
        Ok(Some(Self {
            account,
            settings,
            status: store.sync_status(account)?,
            waiting: store.eligible_queued_actions(account, now)?,
            profiles: store
                .github_access_profiles()?
                .into_iter()
                .filter(|profile| profile.account == account)
                .collect(),
            next_read: if network {
                store.next_sync_due(account, now)?
            } else {
                None
            },
            next_write: if network {
                writes::due(store, account, now)?
            } else {
                None
            },
        }))
    }

    fn probe_profile(&self) -> Option<&AccessProfile> {
        self.profiles
            .iter()
            .find(|profile| profile.access == AccessState::Readable)
            .or_else(|| {
                self.profiles.iter().find(|profile| {
                    !matches!(
                        profile.access,
                        AccessState::SignedOut | AccessState::CredentialUnavailable
                    )
                })
            })
    }
}

async fn run(
    accounts: AccountService,
    core: Arc<Shared>,
    sender: watch::Sender<SyncUpdate>,
    background_enabled: bool,
) {
    let store = accounts.reader_store();
    let mut active_account = None;
    let mut bound = BoundReaders::default();
    let mut digest = 0u64;
    let mut history_digest = 0u64;
    let mut requested = None;
    let mut enroll = true;
    let mut probe_retry = Duration::ZERO;
    let mut prefer_write = true;
    let mut next_write_family = 0;
    let mut was_reachable = false;
    while !core.shutdown.is_cancelled() {
        let Ok(now) = UtcMillis::now() else {
            break;
        };
        let now = now.0;
        let elapsed = core.started.elapsed();
        let (generation, network) = {
            let mut scope = core.scope.lock().expect("sync scope");
            requested = scope.requested.take().or(requested);
            (scope.generation, background_enabled && scope.ready)
        };
        let local = blocking(&store, move |store| {
            Local::load(store, now, network, enroll)
        })
        .await;
        let Ok(Some(local)) = local else {
            if active_account.take().is_some() {
                core.observer
                    .lock()
                    .expect("connectivity observer")
                    .reset_account(false);
                bound = BoundReaders::default();
            }
            publish(&core, &sender, None, digest, history_digest);
            let retry = local.is_err().then_some(Duration::from_secs(15));
            wait(&core, retry).await;
            enroll = true;
            continue;
        };
        let offline = local.settings.controls.work_offline;
        bound.retain_profiles(&local.profiles);
        let account_changed = active_account.replace(local.account) != Some(local.account);
        {
            let mut observer = core.observer.lock().expect("connectivity observer");
            if account_changed {
                observer.reset_account(offline);
                bound = BoundReaders::default();
                requested = Some(ProbeTrigger::Startup);
                probe_retry = Duration::ZERO;
                was_reachable = false;
                prefer_write = true;
                next_write_family = 0;
            } else {
                observer.set_offline(offline);
            }
        }
        publish(&core, &sender, Some(&local), digest, history_digest);
        let deadline = core
            .observer
            .lock()
            .expect("connectivity observer")
            .next_stale_at(elapsed);
        let mut next = deadline.map(|due| due.saturating_sub(elapsed));
        if network && !offline {
            let status = core
                .observer
                .lock()
                .expect("connectivity observer")
                .snapshot(elapsed)
                .status;
            let reachable = status == ConnectionStatus::Reachable;
            if reachable && !was_reachable {
                let account = local.account;
                let _ = blocking(&store, move |store| writes::resume(store, account, now)).await;
            }
            was_reachable = reachable;
            let floor = elapsed
                .saturating_add(until(now, local.status.budget_not_before_ms.unwrap_or(now)));
            if let Some(profile) = local.probe_profile() {
                let (chosen, due) = {
                    let mut observer = core.observer.lock().expect("connectivity observer");
                    let waiting = local.waiting != 0 || local.next_write.is_some();
                    let due = if requested.is_some() {
                        Some(floor)
                    } else {
                        observer
                            .next_probe_due(elapsed, floor, waiting)
                            .map(|due| due.max(probe_retry))
                    };
                    let chosen = due.filter(|due| *due <= elapsed).and_then(|_| {
                        observer.begin_probe(
                            requested.unwrap_or(ProbeTrigger::WaitingWork),
                            elapsed,
                            floor,
                            waiting,
                        )
                    });
                    (chosen, due)
                };
                if let Some(id) = chosen {
                    let cancel = core.start_read(generation, None);
                    let attempted =
                        probe(&accounts, &store, &core, &mut bound, profile, id, &cancel).await;
                    core.finish_read();
                    if attempted == ProbeResult::Authenticated {
                        let account = local.account;
                        let _ = blocking(&store, move |store| writes::resume(store, account, now))
                            .await;
                    }
                    if attempted != ProbeResult::Deferred {
                        requested = None;
                    }
                    if attempted == ProbeResult::Unavailable {
                        probe_retry = core
                            .started
                            .elapsed()
                            .saturating_add(Duration::from_secs(120));
                    }
                    enroll = false;
                    continue;
                }
                if let Some(due) = due {
                    next = earliest(next, due.saturating_sub(elapsed));
                }
            }
            let write_allowed = status != ConnectionStatus::Unreachable;
            let write_ready = write_allowed && local.next_write.is_some_and(|due| due <= now);
            if write_ready && (prefer_write || local.next_read.is_none_or(|due| due > now)) {
                if writes::execute(
                    &accounts,
                    &store,
                    &core,
                    local.account,
                    generation,
                    next_write_family,
                )
                .await
                {
                    digest = digest.wrapping_add(1);
                }
                next_write_family = (next_write_family + 1) % writes::ACTION_FAMILIES;
                prefer_write = false;
                enroll = wait(&core, Some(Duration::from_millis(250))).await;
                continue;
            }
            if write_allowed && let Some(due) = local.next_write {
                next = earliest(next, until(now, due));
            }
            if let Some(due) = local.next_read {
                let delay = until(now, due);
                if delay.is_zero() {
                    if let Ok(Some(claim)) = blocking(&store, move |store| {
                        store.claim_sync_read(local.account, now)
                    })
                    .await
                    {
                        let history =
                            threadline_storage::sync::HistoryRead::from_target(&claim.target)
                                .is_some();
                        if reads::execute(
                            &accounts, &store, &core, &mut bound, &local, claim, generation,
                        )
                        .await
                        {
                            digest = digest.wrapping_add(1);
                            if history {
                                history_digest = history_digest.wrapping_add(1);
                            }
                        }
                    }
                    prefer_write = true;
                    // A competing write can change eligibility between the due check and claim.
                    enroll = wait(&core, Some(Duration::from_millis(250))).await;
                    continue;
                }
                next = earliest(next, delay);
            }
        }
        enroll = wait(&core, next).await;
    }
}

#[derive(PartialEq, Eq)]
enum ProbeResult {
    Authenticated,
    Attempted,
    Deferred,
    Unavailable,
}

async fn probe(
    accounts: &AccountService,
    store: &Arc<Store>,
    core: &Shared,
    bound: &mut BoundReaders,
    profile: &AccessProfile,
    id: ObservationId,
    cancel: &Cancellation,
) -> ProbeResult {
    if cancel.is_cancelled() {
        core.record(id, Reachability::NotAttempted, Some(AccessKind::Cancelled));
        return ProbeResult::Unavailable;
    }
    let account = profile.account;
    let admitted = blocking(store, move |store| {
        store.reserve_read_budget(account, UtcMillis::now()?.0)
    })
    .await;
    match admitted {
        Ok(ReadBudget::Granted) => {}
        Ok(ReadBudget::Deferred { .. }) => {
            core.record(id, Reachability::NotAttempted, None);
            return ProbeResult::Deferred;
        }
        Err(_) => {
            core.record(id, Reachability::NotAttempted, None);
            return ProbeResult::Unavailable;
        }
    }
    let result = accounts.reader_session_with_guidance(profile, cancel).await;
    if cancel.is_cancelled() {
        core.record(id, Reachability::NotAttempted, Some(AccessKind::Cancelled));
        return ProbeResult::Unavailable;
    }
    match result {
        Ok((session, rate)) => {
            core.record(id, Reachability::ReceivedResponse, None);
            bound
                .sessions
                .insert(profile.credential_ref.clone(), Arc::new(session));
            hold(store, account, Some(rate), None).await;
            ProbeResult::Authenticated
        }
        Err(error) => {
            let (reachability, reason, rate) = evidence(&error);
            core.record(id, reachability, reason);
            hold(store, account, rate, reason).await;
            if reachability == Reachability::NotAttempted {
                ProbeResult::Unavailable
            } else {
                ProbeResult::Attempted
            }
        }
    }
}

fn publish(
    core: &Shared,
    sender: &watch::Sender<SyncUpdate>,
    local: Option<&Local>,
    cache_generation: u64,
    history_generation: u64,
) {
    let connection = core
        .observer
        .lock()
        .expect("connectivity observer")
        .snapshot(core.started.elapsed());
    let eligible = connection.status == ConnectionStatus::Reachable && connection.reason.is_none();
    let reading = SyncUpdate {
        account: local.map(|local| local.account),
        status: local.map(|local| local.status.clone()),
        connection,
        cache_generation,
        history_generation,
        eligible_outbox: if eligible {
            local.map_or(0, |local| local.waiting)
        } else {
            0
        },
    };
    sender.send_if_modified(|old| {
        if *old == reading {
            false
        } else {
            *old = reading;
            true
        }
    });
}

fn evidence(error: &ReaderError) -> (Reachability, Option<AccessKind>, Option<RateLimit>) {
    match error {
        ReaderError::Remote(error) => (error.reachability, Some(error.kind), error.rate),
        _ => (Reachability::NotAttempted, None, None),
    }
}

pub(super) fn floor(rate: Option<RateLimit>, reason: Option<AccessKind>, now: i64) -> Option<i64> {
    rate.and_then(|rate| rate.not_before_ms(now))
        .or_else(|| (reason == Some(AccessKind::RateLimited)).then(|| now.saturating_add(60_000)))
}

pub(super) async fn hold(
    store: &Arc<Store>,
    account: AccountId,
    rate: Option<RateLimit>,
    reason: Option<AccessKind>,
) {
    if let Ok(now) = UtcMillis::now()
        && let Some(floor) = floor(rate, reason, now.0)
    {
        let _ = blocking(store, move |store| store.hold_sync_budget(account, floor)).await;
    }
}

pub(super) async fn blocking<T: Send + 'static>(
    store: &Arc<Store>,
    run: impl FnOnce(&Store) -> threadline_storage::Result<T> + Send + 'static,
) -> threadline_storage::Result<T> {
    let store = store.clone();
    tokio::task::spawn_blocking(move || run(&store))
        .await
        .unwrap_or(Err(threadline_storage::Error::Conflict(
            "Synchronization worker was interrupted",
        )))
}

fn until(now: i64, when: i64) -> Duration {
    Duration::from_millis(when.saturating_sub(now).max(0) as u64)
}

fn earliest(current: Option<Duration>, candidate: Duration) -> Option<Duration> {
    Some(current.map_or(candidate, |current| current.min(candidate)))
}

async fn wait(core: &Shared, delay: Option<Duration>) -> bool {
    let timer = async {
        match delay {
            Some(delay) => tokio::time::sleep(delay.min(Duration::from_secs(86_400))).await,
            None => std::future::pending::<()>().await,
        }
    };
    tokio::select! {
        _ = core.shutdown.cancelled() => false,
        _ = core.changed.notified() => true,
        _ = timer => false,
    }
}

fn zone(settings: &SyncSettings) -> threadline_storage::Result<String> {
    settings.schedule.timezone.clone().map_or_else(
        || system_timezone().map_err(threadline_storage::Error::InvalidInput),
        Ok,
    )
}
