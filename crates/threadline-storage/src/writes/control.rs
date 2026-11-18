use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationTarget {
    pub operation_id: String,
    pub attempt: i64,
    pub route: WriteRoute,
    pub comment_id: NonZeroU64,
}

fn reconciliation_candidates(
    db: &Connection,
    account: AccountId,
    through: i64,
    limit: u16,
) -> Result<Vec<(ReconciliationTarget, i64)>> {
    let mut query=db.prepare(
        "SELECT a.operation_id,a.attempt,a.remote_id,e.route,o.retry_at
         FROM write_accepted_objects a
         JOIN operations o ON o.account_id=a.account_id AND o.id=a.operation_id AND o.attempt=a.attempt
         JOIN write_evidence e USING(account_id,operation_id,attempt)
         JOIN operation_gates g ON g.account_id=o.account_id AND g.id=o.id
         WHERE a.account_id=? AND a.kind='comment' AND o.state IN ('waiting','unknown')
           AND g.hold IS NULL AND o.retry_at<=?
         ORDER BY o.retry_at,o.sequence LIMIT ?")?;
    let rows = query.query_map(params![account, through, i64::from(limit)], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get(4)?,
        ))
    })?;
    rows.map(|row| {
        let (operation_id, attempt, remote, raw, retry_at) = row?;
        Ok((
            ReconciliationTarget {
                operation_id,
                attempt,
                route: decode(&raw)?,
                comment_id: numeric(&remote)?,
            },
            retry_at,
        ))
    })
    .collect()
}

impl Store {
    /// Earliest queued attempt or expiring lease for the worker. Paused and unselected work supplies no demand.
    pub fn next_write_due(&self, account: AccountId, now: i64) -> Result<Option<i64>> {
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            let earliest=tx.query_row("SELECT MIN(q.retry_at) FROM operations q JOIN write_intents w
                        ON w.account_id=q.account_id AND w.operation_id=q.id JOIN operation_gates g USING(account_id,id)
                        WHERE q.account_id=? AND q.state='queued' AND NOT q.cancel_requested AND g.hold IS NULL",
                        [account],|r|r.get::<_,Option<i64>>(0))?;
            let floor = deadline(&tx, account)?.unwrap_or(now).max(now);
            let work = earliest.map(|t| t.max(floor));
            let lease=tx.query_row("SELECT MIN(q.lease_until) FROM operations q JOIN write_intents w
                        ON w.account_id=q.account_id AND w.operation_id=q.id WHERE q.account_id=? AND q.state IN ('validating','sending')",
                        [account],|r|r.get::<_,Option<i64>>(0))?.map(|t|t.max(now));
            let readback = reconciliation_candidates(&tx,account,i64::MAX,1)?
                .first().map(|(_,at)|(*at).max(floor));
            Ok(work.into_iter().chain(lease).chain(readback).min())
        })
    }

    /// User retry affects only a queued, definitely unsent action. Shared server floors still apply.
    pub fn retry_write_now(&self, account: AccountId, id: &str, now: i64) -> Result<bool> {
        self.write(|tx| {
            let floor = deadline(tx, account)?.unwrap_or(now).max(now);
            Ok(tx.execute("UPDATE operations SET retry_at=?3,reason='explicit_retry' WHERE account_id=?1 AND id=?2
                        AND state='queued' AND cancel_requested=0 AND EXISTS(SELECT 1 FROM write_intents w
                            WHERE w.account_id=?1 AND w.operation_id=?2)
                        AND EXISTS(SELECT 1 FROM operation_gates g WHERE g.account_id=?1 AND g.id=?2 AND g.hold IS NULL)",
                        params![account,id,floor])?==1)
        })
    }

    /// Coalesce a reconnect/wake signal. A changed owner or user cannot release held work.
    pub fn resume_writes(&self, account: AccountId, now: i64) -> Result<usize> {
        self.write(|tx| {
            let floor = deadline(tx, account)?.unwrap_or(now).max(now);
            let changed=tx.execute("UPDATE operations AS q SET state='queued',retry_at=?2,reason='eligible_after_recovery'
                        WHERE q.account_id=?1 AND q.cancel_requested=0 AND EXISTS(
                            SELECT 1 FROM write_intents w JOIN operation_gates g ON g.account_id=w.account_id AND g.id=w.operation_id
                            WHERE w.account_id=q.account_id AND w.operation_id=q.id AND g.hold IS NULL)
                        AND ((q.state='queued' AND q.reason='retry_burst_exhausted') OR
                             (q.state='attention' AND q.reason IN ('mutation_access_rejected','write_permission_unavailable') AND EXISTS(
                                SELECT 1 FROM write_evidence e JOIN github_operation_scopes sc USING(account_id,operation_id)
                                JOIN github_access_profiles p ON p.account_id=sc.account_id AND p.owner_scope=sc.owner_scope
                                WHERE e.account_id=q.account_id AND e.operation_id=q.id AND e.attempt=q.attempt
                                AND p.credential_generation>e.credential_generation)))",params![account,floor])?;
            if changed > 0 {
                tx.execute("INSERT INTO write_timing(account_id,not_before,retry_epoch) VALUES(?,0,?)
                            ON CONFLICT(account_id) DO UPDATE SET retry_epoch=MAX(write_timing.retry_epoch,excluded.retry_epoch)",params![account,now])?;
            }
            Ok(changed)
        })
    }

    /// Read-only reconciliation can resume an accepted create after restart, including after cancellation.
    pub fn write_reconciliations(
        &self,
        account: AccountId,
        limit: u16,
    ) -> Result<Vec<ReconciliationTarget>> {
        if !(1..=100).contains(&limit) {
            return Err(Error::InvalidInput("Invalid reconciliation page size"));
        }
        self.read(|db| {
            Ok(reconciliation_candidates(db, account, i64::MAX, limit)?
                .into_iter()
                .map(|(target, _)| target)
                .collect())
        })
    }

    /// Reserve one eligible exact-object GET. Its per-operation deadline survives process exit.
    pub fn claim_write_reconciliation(
        &self,
        account: AccountId,
        now: i64,
        lease_until: i64,
    ) -> Result<Option<ReconciliationTarget>> {
        if lease_until <= now {
            return Err(Error::InvalidInput("Invalid readback reservation"));
        }
        self.write(|tx| {
            if deadline(tx, account)?.is_some_and(|floor| floor > now) {
                return Ok(None);
            }
            let Some((target, _)) = reconciliation_candidates(tx, account, now, 1)?.pop() else {
                return Ok(None);
            };
            tx.execute(
                "UPDATE operations SET retry_at=? WHERE account_id=? AND id=? AND attempt=?",
                params![lease_until, account, target.operation_id, target.attempt],
            )?;
            Ok(Some(target))
        })
    }

    /// A missing or inaccessible accepted object retains its exact identity. Other items remain eligible.
    pub fn defer_write_reconciliation(
        &self,
        target: &ReconciliationTarget,
        not_before: i64,
    ) -> Result<bool> {
        self.write(|tx|{
            let changed=tx.execute("UPDATE operations SET retry_at=MAX(retry_at,?1)
                WHERE account_id=?2 AND id=?3 AND attempt=?4 AND state IN ('waiting','unknown')
                  AND EXISTS(SELECT 1 FROM write_accepted_objects a JOIN write_evidence e USING(account_id,operation_id,attempt)
                     WHERE a.account_id=?2 AND a.operation_id=?3 AND a.attempt=?4
                       AND a.kind='comment' AND a.remote_id=?5 AND e.route=?6)",
                params![not_before,target.route.account,target.operation_id,target.attempt,
                    target.comment_id.to_string(),encode(&target.route)?])?;
            Ok(changed==1)
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WritePreflightFailure {
    Transient,
    Throttled { not_before_ms: i64 },
    Unauthorized,
    Forbidden,
    Missing,
    InvalidResponse,
}

fn validating(db: &Connection, claim: &WriteClaim, now: i64) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM operations o JOIN operation_gates g USING(account_id,id)
           WHERE o.account_id=? AND o.id=? AND o.attempt=? AND o.state='validating'
             AND o.lease_until>? AND o.cancel_requested=0 AND g.hold IS NULL)",
        params![
            claim.route.account,
            claim.operation_id(),
            claim.attempt(),
            now
        ],
        |r| r.get(0),
    )?)
}

impl Store {
    /// Bind the live credential lookup to this eligible, original preflight and owner.
    pub fn write_profile(
        &self,
        claim: &WriteClaim,
        now: i64,
    ) -> Result<crate::access::AccessProfile> {
        self.read(|db| {
            let snapshot = db.unchecked_transaction()?;
            if !validating(&snapshot, claim, now)?
                || load_route(&snapshot, claim.route.account, claim.operation_id())? != claim.route
            {
                return Err(Error::Conflict(
                    "The write target or credential generation changed",
                ));
            }
            crate::access::profile(&snapshot, claim.route.account, &claim.route.owner)?
                .ok_or(Error::NotFound)
        })
    }

    /// Release only a still-unsent preflight, including when the account has just gone offline.
    pub fn abandon_write_preflight(&self, claim: &WriteClaim, now: i64) -> Result<bool> {
        self.write(|tx| {
            let original = &claim.claim.operation;
            let changed = tx.execute(
                "UPDATE operations SET state='queued', reason='preflight_interrupted',
                       retry_at=?4, lease_until=NULL
                 WHERE account_id=?1 AND id=?2 AND attempt=?3 AND state='validating' AND cancel_requested=0
                   AND EXISTS(SELECT 1 FROM operation_attempts a WHERE a.account_id=?1
                     AND a.operation_id=?2 AND a.attempt=?3 AND a.sending_at IS NULL)",
                params![original.account,original.id,claim.attempt(),now],
            )?;
            if changed == 1 {
                dispatch::end_attempt(tx, original.account, &original.id, claim.attempt(), "queued", "preflight_interrupted")?;
            }
            Ok(changed == 1)
        })
    }

    /// Bind a read-only lookup to the same accepted comment and original actor, including after credential renewal.
    pub fn reconciliation_profile(
        &self,
        target: &ReconciliationTarget,
    ) -> Result<crate::access::AccessProfile> {
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            let raw: Option<String> = tx.query_row(
                "SELECT e.route FROM write_accepted_objects a JOIN write_evidence e USING(account_id,operation_id,attempt)
                 JOIN operations o ON o.account_id=a.account_id AND o.id=a.operation_id AND o.attempt=a.attempt
                 JOIN operation_gates g ON g.account_id=o.account_id AND g.id=o.id
                 WHERE a.account_id=?1 AND o.id=?2 AND a.attempt=?3 AND a.remote_id=?4 AND a.kind='comment'
                   AND o.state IN ('waiting','unknown') AND g.hold IS NULL",
                params![target.route.account,target.operation_id,target.attempt,target.comment_id.to_string()],
                |r|r.get(0),
            ).optional()?;
            let original: WriteRoute = decode(&raw.ok_or(Error::Conflict("No eligible accepted comment for this attempt"))?)?;
            let current=load_route(&tx,target.route.account,&target.operation_id)?;
            let mut expected=original.clone();
            expected.credential_generation=current.credential_generation;
            if original!=target.route || current!=expected {
                return Err(Error::Conflict("The accepted comment no longer belongs to this original route"));
            }
            crate::access::profile(&tx,current.account,&current.owner)?.ok_or(Error::NotFound)
        })
    }

    pub fn fail_write_preflight(
        &self,
        claim: &WriteClaim,
        why: WritePreflightFailure,
        now: i64,
    ) -> Result<()> {
        self.write(|tx| {
            if !validating(tx, claim, now)?
                || load_route(tx, claim.route.account, claim.operation_id())? != claim.route
            {
                return Err(Error::Conflict(
                    "The preflight result no longer belongs to the active credential or target",
                ));
            }
            let (state, mut reason, floor) = match why {
                WritePreflightFailure::Transient => ("queued", "temporary_preflight", None),
                WritePreflightFailure::Throttled { not_before_ms } => {
                    crate::sync::server_floor_in(tx, claim.route.account, not_before_ms)?;
                    ("queued", "preflight_throttled", Some(not_before_ms))
                }
                WritePreflightFailure::Unauthorized | WritePreflightFailure::Forbidden => {
                    ("attention", "write_permission_unavailable", None)
                }
                WritePreflightFailure::Missing => ("attention", "verified_target_unavailable", None),
                WritePreflightFailure::InvalidResponse => ("attention", "invalid_preflight_response", None),
            };
            let due = if state == "queued" {
                let burst:u32=tx.query_row("SELECT COUNT(*) FROM operation_attempts WHERE account_id=?1 AND operation_id=?2
                            AND claimed_at >= COALESCE((SELECT retry_epoch FROM write_timing WHERE account_id=?1),0)",
                            params![claim.route.account,claim.operation_id()],|r|r.get(0))?;
                let step = threadline_core::sync::policy::retry_delay(burst, std::time::Duration::ZERO);
                if step.is_none() {
                    reason = "retry_burst_exhausted";
                }
                let jitter = claim
                    .operation_id()
                    .bytes()
                    .fold(claim.attempt() as u64, |v, b| {
                        v.wrapping_mul(31).wrapping_add(b.into())
                    })
                    % 501;
                let delay = step
                    .unwrap_or(std::time::Duration::from_secs(15 * 60))
                    .as_millis() as i64
                    + jitter as i64;
                now.saturating_add(delay).max(floor.unwrap_or(now))
            } else {
                0
            };
            tx.execute("INSERT INTO write_evidence(account_id,operation_id,attempt,credential_generation,observed_at,current_fields,result,route)
                        VALUES(?1,?2,?3,?4,?5,'null',?6,?7)",params![claim.route.account,claim.operation_id(),claim.attempt(),
                            claim.route.credential_generation,now,reason,encode(&claim.route)?])?;
            end_preflight(tx, claim, state, reason, due)
        })
    }

    /// Atomically reserve this mutation once. The provider starts HTTP only after this returns true.
    pub fn start_write_transport(&self, ticket: &MutationTicket, now: i64) -> Result<bool> {
        let claim = ticket.claim();
        self.write(|tx| {
            if !validating(tx, claim, now)? {
                return Ok(false);
            }
            if load_route(tx, claim.route.account, claim.operation_id())? != claim.route {
                end_preflight(
                    tx,
                    claim,
                    "queued",
                    "credential_changed_before_transport",
                    now,
                )?;
                return Ok(false);
            }
            let accepted:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM write_evidence WHERE account_id=?
                        AND operation_id=? AND attempt=? AND result='validated' AND credential_generation=?)",
                        params![claim.route.account,claim.operation_id(),claim.attempt(),claim.route.credential_generation],|r|r.get(0))?;
            if !accepted {
                return Err(Error::Conflict("The mutation has no accepted preflight"));
            }
            let floor = deadline(tx, claim.route.account)?;
            let sending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM operations o JOIN mutation_families w
                        ON w.account_id=o.account_id AND w.operation_id=o.id WHERE o.account_id=? AND o.state='sending')",
                        [claim.route.account],|r|r.get(0))?;
            if sending || floor.is_some_and(|t| t > now) {
                end_preflight(
                    tx,
                    claim,
                    "queued",
                    "mutation_pacing",
                    floor.unwrap_or(now).max(now.saturating_add(1_000)),
                )?;
                return Ok(false);
            }
            dispatch::begin_send_in(tx, &claim.claim, claim.intent.action(), now)?;
            let next = now.saturating_add(1_000);
            crate::sync::budget_in(tx, claim.route.account, next)?;
            tx.execute("INSERT INTO write_timing(account_id,not_before) VALUES(?,?) ON CONFLICT(account_id) DO UPDATE SET
                        not_before=MAX(write_timing.not_before,excluded.not_before)",params![claim.route.account,next])?;
            Ok(true)
        })
    }
}
