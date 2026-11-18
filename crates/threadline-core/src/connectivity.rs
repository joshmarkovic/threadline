//! Shared API reachability evidence. UTC timestamps are for display; elapsed time controls freshness and retries.
use crate::github::{AccessKind, Reachability};
use std::time::Duration;

const FRESH: Duration = Duration::from_secs(120);
const PROBE_DELAYS: [u64; 5] = [5, 15, 30, 60, 120];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionStatus {
    Checking,
    Reachable,
    Unreachable,
    Offline,
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeTrigger {
    Startup,
    Wake,
    NetworkChanged,
    CheckNow,
    WaitingWork,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservationId {
    epoch: u64,
    sequence: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectionSnapshot {
    pub status: ConnectionStatus,
    pub reason: Option<AccessKind>,
    pub last_checked_ms: Option<i64>,
    pub last_contact_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug)]
struct Evidence {
    status: ConnectionStatus,
    reason: Option<AccessKind>,
    elapsed: Duration,
}

pub struct ConnectivityObserver {
    seed: u64,
    offline: bool,
    epoch: u64,
    next_id: u64,
    accepted: u64,
    in_probe: Option<ObservationId>,
    last: Option<Evidence>,
    last_checked: Option<i64>,
    last_contact: Option<i64>,
    failures: usize,
    retry_after: Duration,
}

impl ConnectivityObserver {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            offline: false,
            epoch: 0,
            next_id: 0,
            accepted: 0,
            in_probe: None,
            last: None,
            last_checked: None,
            last_contact: None,
            failures: 0,
            retry_after: Duration::ZERO,
        }
    }

    fn invalidate_requests(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.in_probe = None;
        self.accepted = self.next_id;
    }

    /// Switch the active account without allowing old requests or contact details into its display.
    pub fn reset_account(&mut self, offline: bool) {
        self.invalidate_requests();
        self.offline = offline;
        self.last = None;
        self.last_checked = None;
        self.last_contact = None;
        self.failures = 0;
        self.retry_after = Duration::ZERO;
    }

    pub fn set_offline(&mut self, offline: bool) {
        if self.offline != offline {
            self.invalidate_requests();
            self.offline = offline;
            self.last = None;
            self.failures = 0;
            self.retry_after = Duration::ZERO;
        }
    }

    pub fn begin_request(&mut self) -> Option<ObservationId> {
        if self.offline {
            return None;
        }
        self.next_id = self.next_id.checked_add(1)?;
        Some(ObservationId {
            epoch: self.epoch,
            sequence: self.next_id,
        })
    }

    pub fn snapshot(&self, now: Duration) -> ConnectionSnapshot {
        let status = match (self.offline, self.last) {
            (true, _) => ConnectionStatus::Offline,
            (false, None) => ConnectionStatus::Checking,
            (false, Some(e)) if now.saturating_sub(e.elapsed) >= FRESH => ConnectionStatus::Stale,
            (false, Some(e)) => e.status,
        };
        ConnectionSnapshot {
            status,
            reason: self.last.and_then(|e| e.reason),
            last_checked_ms: self.last_checked,
            last_contact_ms: self.last_contact,
        }
    }

    pub fn next_stale_at(&self, now: Duration) -> Option<Duration> {
        if self.offline {
            return None;
        }
        self.last
            .map(|e| e.elapsed.saturating_add(FRESH))
            .filter(|deadline| *deadline > now)
    }

    /// Returns true on a transition to recently verified reachability.
    pub fn record(
        &mut self,
        id: ObservationId,
        reachability: Reachability,
        reason: Option<AccessKind>,
        wall_ms: i64,
        monotonic: Duration,
    ) -> bool {
        if self.in_probe == Some(id) {
            self.in_probe = None;
        }
        if self.offline
            || self.epoch != id.epoch
            || id.sequence <= self.accepted
            || reachability == Reachability::NotAttempted
            || reason == Some(AccessKind::Cancelled)
        {
            return false;
        }
        self.accepted = id.sequence;
        self.last_checked = Some(wall_ms);
        let previous = self.snapshot(monotonic).status;
        let status = match reachability {
            Reachability::ReceivedResponse => {
                self.last_contact = Some(wall_ms);
                self.failures = 0;
                self.retry_after = Duration::ZERO;
                ConnectionStatus::Reachable
            }
            Reachability::NoResponse => {
                self.failures = self.failures.saturating_add(1);
                self.retry_after = monotonic.saturating_add(self.backoff());
                ConnectionStatus::Unreachable
            }
            Reachability::NotAttempted => unreachable!(),
        };
        self.last = Some(Evidence {
            status,
            reason,
            elapsed: monotonic,
        });
        status == ConnectionStatus::Reachable && previous != ConnectionStatus::Reachable
    }

    fn backoff(&self) -> Duration {
        let base = PROBE_DELAYS[self.failures.saturating_sub(1).min(PROBE_DELAYS.len() - 1)] * 1000;
        let width = base / 10;
        let jitter = self
            .seed
            .wrapping_add(self.next_id.wrapping_mul(2_654_435_761))
            % (2 * width + 1);
        Duration::from_millis((base - width + jitter).min(120_000))
    }

    /// All arguments are deadlines or readings of the same monotonic process clock.
    pub fn next_probe_due(
        &self,
        now: Duration,
        server_not_before: Duration,
        waiting: bool,
    ) -> Option<Duration> {
        if self.offline || !waiting || self.in_probe.is_some() {
            return None;
        }
        let due = match self.snapshot(now).status {
            ConnectionStatus::Reachable => return None,
            ConnectionStatus::Checking => now,
            _ => self.retry_after.max(now),
        };
        Some(due.max(server_not_before))
    }

    pub fn begin_probe(
        &mut self,
        trigger: ProbeTrigger,
        now: Duration,
        server_not_before: Duration,
        waiting: bool,
    ) -> Option<ObservationId> {
        if self.offline || self.in_probe.is_some() || now < server_not_before {
            return None;
        }
        if trigger == ProbeTrigger::WaitingWork
            && self.next_probe_due(now, server_not_before, waiting)? > now
        {
            return None;
        }
        if matches!(trigger, ProbeTrigger::Wake | ProbeTrigger::NetworkChanged) {
            self.invalidate_requests();
        }
        let id = self.begin_request()?;
        self.in_probe = Some(id);
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sec(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn ut_008_stale_receipts_cannot_override_current_http_contact_or_explicit_offline() {
        let mut observer = ConnectivityObserver::new(345);
        assert_eq!(observer.snapshot(sec(0)).status, ConnectionStatus::Checking);
        let slow = observer.begin_request().unwrap();
        let fast = observer.begin_request().unwrap();
        assert!(observer.record(
            fast,
            Reachability::ReceivedResponse,
            Some(AccessKind::Unauthorized),
            100,
            sec(2)
        ));
        assert!(!observer.record(
            slow,
            Reachability::NoResponse,
            Some(AccessKind::Timeout),
            110,
            sec(3)
        ));
        assert_eq!(
            observer.snapshot(sec(3)).reason,
            Some(AccessKind::Unauthorized)
        );
        let cancelled = observer.begin_request().unwrap();
        assert!(!observer.record(
            cancelled,
            Reachability::NoResponse,
            Some(AccessKind::Cancelled),
            120,
            sec(6)
        ));
        assert_eq!(
            observer.snapshot(sec(121)).status,
            ConnectionStatus::Reachable
        );
        assert_eq!(observer.snapshot(sec(122)).status, ConnectionStatus::Stale);
        let current = observer.begin_request().unwrap();
        assert!(observer.record(
            current,
            Reachability::ReceivedResponse,
            Some(AccessKind::RateLimited),
            90,
            sec(124)
        ));
        assert_eq!(observer.snapshot(sec(124)).last_contact_ms, Some(90));
        let previous = observer.begin_request().unwrap();
        observer.set_offline(true);
        assert!(
            observer
                .begin_probe(ProbeTrigger::CheckNow, sec(125), Duration::ZERO, true)
                .is_none()
        );
        observer.set_offline(false);
        assert!(!observer.record(
            previous,
            Reachability::ReceivedResponse,
            None,
            130,
            sec(126)
        ));
        assert_eq!(
            observer.snapshot(sec(126)).status,
            ConnectionStatus::Checking
        );
    }

    #[test]
    fn ut_008_pending_work_coalesces_probes_and_respects_jittered_retry_and_server_floors() {
        let mut observer = ConnectivityObserver::new(2);
        let probe = observer
            .begin_probe(ProbeTrigger::Startup, sec(0), Duration::ZERO, false)
            .unwrap();
        assert!(
            observer
                .begin_probe(ProbeTrigger::NetworkChanged, sec(0), Duration::ZERO, false)
                .is_none()
        );
        assert!(!observer.record(
            probe,
            Reachability::NoResponse,
            Some(AccessKind::Transport),
            1,
            sec(0)
        ));
        let wait = observer
            .next_probe_due(sec(0), Duration::ZERO, true)
            .unwrap();
        assert!((Duration::from_millis(4500)..=Duration::from_millis(5500)).contains(&wait));
        assert!(
            observer
                .next_probe_due(sec(0), Duration::ZERO, false)
                .is_none()
        );
        assert_eq!(
            observer.next_probe_due(sec(0), sec(120), true),
            Some(sec(120))
        );
        assert!(
            observer
                .begin_probe(ProbeTrigger::WaitingWork, sec(119), sec(120), true)
                .is_none()
        );
        let probe = observer
            .begin_probe(ProbeTrigger::WaitingWork, sec(120), sec(120), true)
            .unwrap();
        assert!(observer.record(
            probe,
            Reachability::ReceivedResponse,
            Some(AccessKind::SsoRequired),
            200,
            sec(121)
        ));
        let later = observer.begin_request().unwrap();
        assert!(!observer.record(later, Reachability::ReceivedResponse, None, 210, sec(122)));
        assert!(
            observer
                .next_probe_due(sec(123), Duration::ZERO, true)
                .is_none()
        );
    }
}
