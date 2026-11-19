use super::{Operation, State, read_operation};
use crate::{AccountId, Error, RepositoryId, Result, Store};
use rusqlite::{OptionalExtension, Transaction, params};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub operation: Operation,
    pub number: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub step: String,
    pub object_kind: String,
    pub remote_id: String,
    pub evidence: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptEvent {
    pub attempt: i64,
    pub outcome: String,
    pub evidence: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchProfile {
    pub active: bool,
    pub authorized: bool,
    pub offline: bool,
    pub outbox_paused: bool,
    pub restored: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelResult {
    Cancelled,
    MayAlreadyBeSent,
    AlreadyConfirmed,
}
#[derive(Debug, Clone, Copy)]
pub enum Delivery<'a> {
    /// GitHub confirmed this step. More steps may still be pending.
    Step(&'a Receipt),
    Confirmed(&'a Receipt),
    Waiting(&'a str),
    Unknown(&'a str),
    /// Authoritative evidence that this attempt had no effect, allowing a later retry.
    NotApplied {
        proof: &'a str,
        retry_at: i64,
    },
}
#[derive(Debug, Clone, Copy)]
pub enum Validation<'a> {
    Retry { reason: &'a str, at: i64 },
    NeedsAttention(&'a str),
}

impl Store {
    pub fn dispatch_profile(&self, account: AccountId) -> Result<DispatchProfile> {
        self.read(|db| {
            db.query_row(
                "SELECT EXISTS(SELECT 1 FROM active_profile WHERE account_id=a.id),
                 coalesce(d.authorized,0),coalesce(d.offline,0),coalesce(d.outbox_paused,0),
                 EXISTS(SELECT 1 FROM restored_profiles WHERE account_id=a.id)
                 FROM accounts a LEFT JOIN dispatch_profiles d ON d.account_id=a.id WHERE a.id=?",
                [account],
                |row| {
                    Ok(DispatchProfile {
                        active: row.get(0)?,
                        authorized: row.get(1)?,
                        offline: row.get(2)?,
                        outbox_paused: row.get(3)?,
                        restored: row.get(4)?,
                    })
                },
            )
            .optional()?
            .ok_or(Error::NotFound)
        })
    }
    /// Release the backup hold after the caller has reconciled this account's remote history.
    pub fn release_restored_profile(&self, account: AccountId) -> Result<()> {
        self.write(|tx| {
            tx.execute(
                "DELETE FROM restored_profiles WHERE account_id=?",
                [account],
            )?;
            Ok(())
        })
    }
    /// Persist an already verified provider identity. Credential material remains outside SQLite.
    pub fn authorize_profile(
        &self,
        account: AccountId,
        verified_host: &str,
        provider_user: u64,
    ) -> Result<()> {
        self.write(|tx| {
            let verified: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=? AND host=? AND provider_user_id=?)",
                params![account, verified_host, provider_user.to_string()],
                |r| r.get(0),
            )?;
            if !verified {
                return Err(Error::Conflict("verified account identity does not match"));
            }
            tx.execute("INSERT INTO dispatch_profiles(account_id,authorized) VALUES(?,1) ON CONFLICT(account_id) DO UPDATE SET authorized=1", [account])?;
            activate(tx, account)
        })
    }
    pub fn activate_local_profile(&self, account: AccountId) -> Result<()> {
        self.write(|tx| activate(tx, account))
    }
    pub fn sign_out(&self, account: AccountId) -> Result<()> {
        self.write(|tx| {
            tx.execute("INSERT INTO dispatch_profiles(account_id,authorized) VALUES(?,0) ON CONFLICT(account_id) DO UPDATE SET authorized=0", [account])?;
            Ok(())
        })
    }
    pub fn set_dispatch_controls(
        &self,
        account: AccountId,
        offline: bool,
        outbox_paused: bool,
    ) -> Result<()> {
        self.write(|tx| {
            tx.execute("INSERT INTO dispatch_profiles(account_id,offline,outbox_paused) VALUES(?,?,?) ON CONFLICT(account_id) DO UPDATE SET offline=excluded.offline,outbox_paused=excluded.outbox_paused", params![account, offline, outbox_paused])?;
            Ok(())
        })
    }
    pub fn set_repository_paused(
        &self,
        account: AccountId,
        repository: RepositoryId,
        paused: bool,
    ) -> Result<()> {
        self.write(|tx| {
            if paused {
                tx.execute(
                    "INSERT INTO paused_repositories VALUES(?,?) ON CONFLICT DO NOTHING",
                    params![account, repository],
                )?;
            } else {
                tx.execute(
                    "DELETE FROM paused_repositories WHERE account_id=? AND repository_id=?",
                    params![account, repository],
                )?;
            }
            Ok(())
        })
    }
    /// Additional dispatch holds such as sign-out, per-target ordering or an unresolved dependency.
    pub fn dispatch_hold(&self, account: AccountId, id: &str) -> Result<Option<String>> {
        self.read(|db| {
            Ok(db.query_row(
                "SELECT hold FROM operation_gates WHERE account_id=? AND id=?",
                params![account, id],
                |r| r.get(0),
            )?)
        })
    }
    pub fn claim_next(
        &self,
        account: AccountId,
        now: i64,
        lease_until: i64,
    ) -> Result<Option<Claim>> {
        if lease_until <= now {
            return Err(Error::InvalidInput("claim lease must end after start"));
        }
        self.write(|tx| claim_in(tx, account, now, lease_until, "legacy"))
    }
    pub fn finish_validation(&self, claim: &Claim, outcome: Validation<'_>) -> Result<()> {
        let (state, reason, at) = match outcome {
            Validation::Retry { reason, at } => ("queued", reason, at),
            Validation::NeedsAttention(reason) => ("attention", reason, 0),
        };
        if reason.is_empty() {
            return Err(Error::InvalidInput("missing validation reason"));
        }
        self.write(|tx| {
            let op = &claim.operation;
            let changed = tx.execute(
                "UPDATE operations SET state=?4,reason=?5,retry_at=?6,lease_until=NULL WHERE account_id=?1 AND id=?2 AND attempt=?3 AND state='validating'",
                params![op.account, op.id, claim.number, state, reason, at],
            )?;
            if changed != 1 {
                return Err(Error::Conflict("validation claim no longer holds"));
            }
            end_attempt(tx, op.account, &op.id, claim.number, state, reason)
        })
    }
    /// The caller must win this boundary after preflight and before sending the named remote step.
    pub fn begin_send(&self, claim: &Claim, step: &str, now: i64) -> Result<()> {
        if step.is_empty() {
            return Err(Error::InvalidInput("missing step identifier"));
        }
        self.write(|tx| {
            reject_typed(tx, claim.operation.account, &claim.operation.id)?;
            begin_send_in(tx, claim, step, now)
        })
    }
    pub fn cancel_operation(&self, account: AccountId, id: &str) -> Result<CancelResult> {
        self.write(|tx| {
            let op = read_operation(tx, account, id)?.ok_or(Error::NotFound)?;
            match cancel_decision(op.state) {
                CancelResult::AlreadyConfirmed => Ok(CancelResult::AlreadyConfirmed),
                CancelResult::MayAlreadyBeSent => {
                    tx.execute("UPDATE operations SET cancel_requested=1,reason='cancel_after_dispatch' WHERE account_id=? AND id=?", params![account, id])?;
                    Ok(CancelResult::MayAlreadyBeSent)
                }
                CancelResult::Cancelled => {
                    tx.execute("UPDATE operations SET state='cancelled',reason='cancel_remaining',cancel_requested=1,lease_until=NULL WHERE account_id=? AND id=?", params![account, id])?;
                    if op.state == State::Validating {
                        end_attempt(
                            tx,
                            account,
                            id,
                            op.attempt,
                            "cancelled",
                            "cancelled_before_dispatch",
                        )?;
                    }
                    Ok(CancelResult::Cancelled)
                }
            }
        })
    }
    /// Expired validating leases had no send authorization. Expired sending leases must reconcile.
    pub fn next_operation_recovery(&self, account: AccountId) -> Result<Option<i64>> {
        self.read(|db| {
            Ok(db.query_row(
                "SELECT MIN(lease_until) FROM operations
                 WHERE account_id=? AND state IN ('validating','sending')",
                [account],
                |row| row.get(0),
            )?)
        })
    }

    pub fn recover_expired(&self, account: AccountId, now: i64) -> Result<usize> {
        self.write(|tx| {
            tx.execute("UPDATE operation_attempts SET outcome=CASE WHEN sending_at IS NULL THEN 'expired_before_dispatch' ELSE 'outcome_unknown' END
                WHERE account_id=?1 AND outcome IS NULL AND EXISTS(SELECT 1 FROM operations q WHERE q.account_id=?1 AND q.id=operation_id
                    AND q.attempt=operation_attempts.attempt AND q.lease_until<=?2 AND q.state IN ('validating','sending'))", params![account, now])?;
            Ok(tx.execute("UPDATE operations SET state=CASE state WHEN 'sending' THEN 'unknown' ELSE 'queued' END,
                reason=CASE state WHEN 'sending' THEN 'interrupted_send' ELSE 'expired_preflight' END,lease_until=NULL
                WHERE account_id=? AND state IN ('validating','sending') AND lease_until<=?", params![account, now])?)
        })
    }
    /// Persist a response or a read-only reconciliation result for the exact attempt.
    pub fn record_delivery(
        &self,
        account: AccountId,
        id: &str,
        attempt: i64,
        outcome: Delivery<'_>,
    ) -> Result<()> {
        self.write(|tx| {
            reject_typed(tx, account, id)?;
            record_delivery_in(tx, account, id, attempt, outcome)
        })
    }
    pub fn receipts(&self, account: AccountId, id: &str) -> Result<Vec<Receipt>> {
        self.read(|db| {
            let mut q = db.prepare("SELECT step,object_kind,remote_id,evidence FROM step_receipts WHERE account_id=? AND operation_id=? ORDER BY step")?;
            Ok(q.query_map(params![account, id], |r| {
                Ok(Receipt {
                    step: r.get(0)?,
                    object_kind: r.get(1)?,
                    remote_id: r.get(2)?,
                    evidence: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
        })
    }
    pub fn attempt_events(&self, account: AccountId, id: &str) -> Result<Vec<AttemptEvent>> {
        self.read(|db| {
            let mut q = db.prepare("SELECT attempt,outcome,evidence FROM attempt_events WHERE account_id=? AND operation_id=? ORDER BY event_id")?;
            let result = q.query_map(params![account, id], |row| {
                Ok(AttemptEvent {
                    attempt: row.get(0)?,
                    outcome: row.get(1)?,
                    evidence: row.get(2)?,
                })
            })?;
            Ok(result.collect::<rusqlite::Result<_>>()?)
        })
    }
}
pub(crate) fn hold_restored_dispatch(tx: &Transaction<'_>) -> Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO restored_profiles SELECT id FROM accounts",
        [],
    )?;
    tx.execute("UPDATE operation_attempts SET outcome=CASE WHEN sending_at IS NULL THEN 'expired_before_dispatch' ELSE 'outcome_unknown' END
        WHERE outcome IS NULL AND EXISTS(SELECT 1 FROM operations q WHERE q.account_id=operation_attempts.account_id AND q.id=operation_id
        AND q.attempt=operation_attempts.attempt AND q.state IN ('validating','sending'))", [])?;
    tx.execute("INSERT OR IGNORE INTO restored_operations(account_id,operation_id,snapshot_state,snapshot_reason)
        SELECT account_id,id,state,reason FROM operations WHERE state NOT IN ('confirmed','cancelled')", [])?;
    tx.execute(
        "UPDATE operations SET state='unknown',reason='restored_backup',lease_until=NULL
        WHERE state NOT IN ('confirmed','cancelled')",
        [],
    )?;
    tx.execute("DELETE FROM reviewed_operation_locators", [])?;
    Ok(())
}
fn activate(tx: &Transaction<'_>, account: AccountId) -> Result<()> {
    tx.execute("INSERT INTO active_profile VALUES(1,?) ON CONFLICT(singleton) DO UPDATE SET account_id=excluded.account_id", [account])?;
    Ok(())
}
fn cancel_decision(state: State) -> CancelResult {
    match state {
        State::Confirmed => CancelResult::AlreadyConfirmed,
        State::Sending | State::OutcomeUnknown | State::Waiting => CancelResult::MayAlreadyBeSent,
        State::Queued | State::Validating | State::NeedsAttention | State::Cancelled => {
            CancelResult::Cancelled
        }
    }
}

pub(crate) fn end_attempt(
    tx: &Transaction<'_>,
    account: AccountId,
    id: &str,
    attempt: i64,
    outcome: &str,
    evidence: &str,
) -> Result<()> {
    tx.execute(
        "INSERT INTO attempt_events(account_id,operation_id,attempt,outcome,evidence) VALUES(?,?,?,?,?)",
        params![account, id, attempt, outcome, evidence],
    )?;
    tx.execute(
        "UPDATE operation_attempts SET outcome=?4,evidence=?5 WHERE account_id=?1 AND operation_id=?2 AND attempt=?3",
        params![account, id, attempt, outcome, evidence],
    )?;
    Ok(())
}

fn reject_typed(tx: &Transaction<'_>, account: AccountId, id: &str) -> Result<()> {
    let typed: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM mutation_families WHERE account_id=? AND operation_id=?)",
        params![account, id],
        |row| row.get(0),
    )?;
    if typed {
        return Err(Error::Conflict(
            "Submitted mutations require typed preflight and response evidence",
        ));
    }
    Ok(())
}
pub(crate) fn claim_in(
    tx: &Transaction<'_>,
    account: AccountId,
    now: i64,
    lease_until: i64,
    family: &str,
) -> Result<Option<Claim>> {
    let id: Option<String> = tx
                .query_row("SELECT q.id FROM operations q JOIN operation_gates g USING(account_id,id)
                WHERE q.account_id=? AND q.state='queued' AND q.cancel_requested=0 AND q.retry_at<=?2 AND g.hold IS NULL
                AND COALESCE((SELECT family FROM mutation_families w WHERE w.account_id=q.account_id AND w.operation_id=q.id),'legacy')=?3
                ORDER BY q.sequence LIMIT 1", params![account, now, family], |r| r.get(0))
                .optional()?;
    let Some(id) = id else { return Ok(None) };
    let number = tx.query_row("UPDATE operations SET state='validating',reason=NULL,attempt=attempt+1,lease_until=? WHERE account_id=? AND id=? RETURNING attempt", params![lease_until, account, id], |r| {
                r.get(0)
            })?;
    tx.execute("INSERT INTO operation_attempts(account_id,operation_id,attempt,claimed_at,claimed_repository,claimed_number)
                SELECT ?1,?2,?3,?4,i.repository_id,i.number FROM operations o JOIN items i ON i.id=o.item_id
                WHERE o.account_id=?1 AND o.id=?2", params![account, id, number, now])?;
    Ok(Some(Claim {
        operation: read_operation(tx, account, &id)?.ok_or(Error::NotFound)?,
        number,
    }))
}
pub(crate) fn begin_send_in(
    tx: &Transaction<'_>,
    claim: &Claim,
    step: &str,
    now: i64,
) -> Result<()> {
    let op = &claim.operation;
    let changed = tx.execute(
                "UPDATE operations SET state='sending' WHERE account_id=?1 AND id=?2 AND attempt=?3 AND state='validating'
                AND cancel_requested=0 AND lease_until>?5 AND EXISTS(SELECT 1 FROM operation_gates WHERE account_id=?1 AND id=?2 AND hold IS NULL)
                AND EXISTS(SELECT 1 FROM operation_attempts a JOIN items i ON i.id=operations.item_id AND i.account_id=a.account_id
                    WHERE a.account_id=?1 AND a.operation_id=?2 AND a.attempt=?3 AND a.claimed_repository=i.repository_id AND a.claimed_number=i.number)
                AND NOT EXISTS(SELECT 1 FROM step_receipts WHERE account_id=?1 AND operation_id=?2 AND step=?4)",
                params![op.account, op.id, claim.number, step, now],
            )?;
    if changed != 1 {
        return Err(Error::Conflict(
            "dispatch held, expired, cancelled or step already confirmed",
        ));
    }
    tx.execute(
                "UPDATE operation_attempts SET sending_at=?4,step=?5 WHERE account_id=?1 AND operation_id=?2 AND attempt=?3",
                params![op.account, op.id, claim.number, now, step],
            )?;
    Ok(())
}
pub(crate) fn record_delivery_in(
    tx: &Transaction<'_>,
    account: AccountId,
    id: &str,
    attempt: i64,
    outcome: Delivery<'_>,
) -> Result<()> {
    let op = read_operation(tx, account, id)?.ok_or(Error::NotFound)?;
    let quarantined: bool =
                tx.query_row("SELECT EXISTS(SELECT 1 FROM restored_operations WHERE account_id=? AND operation_id=? AND decision IS NULL)", params![account, id], |r| r.get(0))?;
    if quarantined
        || op.attempt != attempt
        || !matches!(
            op.state,
            State::Sending | State::OutcomeUnknown | State::Waiting
        )
    {
        return Err(Error::Conflict("no dispatched attempt awaiting evidence"));
    }
    let step: String =
                tx.query_row("SELECT step FROM operation_attempts WHERE account_id=? AND operation_id=? AND attempt=? AND sending_at IS NOT NULL", params![account, id, attempt], |r| r.get(0))?;
    let (state, reason, evidence, retry_at) = match outcome {
        Delivery::Step(r) | Delivery::Confirmed(r) => {
            if r.step != step
                || r.remote_id.is_empty()
                || r.object_kind.is_empty()
                || r.evidence.is_empty()
            {
                return Err(Error::InvalidInput("invalid remote step receipt"));
            }
            tx.execute(
                "INSERT INTO step_receipts VALUES(?,?,?,?,?,?)",
                params![account, id, step, r.object_kind, r.remote_id, r.evidence],
            )?;
            let next = if matches!(outcome, Delivery::Confirmed(_)) {
                "confirmed"
            } else if op.cancel_requested {
                "cancelled"
            } else {
                "queued"
            };
            (next, "confirmed_step", r.evidence.as_str(), 0)
        }
        Delivery::Waiting(proof) => ("waiting", "accepted_by_remote", proof, 0),
        Delivery::Unknown(proof) => ("unknown", "outcome_unknown", proof, 0),
        Delivery::NotApplied { proof, retry_at } => (
            if op.cancel_requested {
                "cancelled"
            } else {
                "queued"
            },
            "proven_not_applied",
            proof,
            retry_at,
        ),
    };
    if evidence.is_empty() {
        return Err(Error::InvalidInput("delivery evidence is required"));
    }
    tx.execute(
                "UPDATE operations SET state=?3,reason=?4,retry_at=?5,lease_until=NULL WHERE account_id=?1 AND id=?2",
                params![account, id, state, reason, retry_at],
            )?;
    end_attempt(tx, account, id, attempt, state, evidence)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ut_009_cancellation_preserves_remote_ambiguity_and_terminal_confirmation() {
        for state in [
            State::Queued,
            State::Validating,
            State::NeedsAttention,
            State::Cancelled,
        ] {
            assert_eq!(cancel_decision(state), CancelResult::Cancelled);
        }
        for state in [State::Sending, State::OutcomeUnknown, State::Waiting] {
            assert_eq!(cancel_decision(state), CancelResult::MayAlreadyBeSent);
        }
        assert_eq!(
            cancel_decision(State::Confirmed),
            CancelResult::AlreadyConfirmed
        );
    }
}
