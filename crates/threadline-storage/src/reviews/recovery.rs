use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewReadback {
    pub operation_id: String,
    pub attempt: i64,
    pub route: ReviewRoute,
    pub step: ReviewStep,
    pub remote_id: NonZeroU64,
}

fn next_readback(
    db: &Connection,
    account: AccountId,
    through: i64,
) -> Result<Option<(ReviewReadback, i64)>> {
    let raw=db.query_row(
        "SELECT q.id,q.attempt,e.route,e.step,a.remote_id,q.retry_at
         FROM operations q JOIN review_attempts e ON e.account_id=q.account_id AND e.operation_id=q.id AND e.attempt=q.attempt
         JOIN operation_attempts sent ON sent.account_id=e.account_id AND sent.operation_id=e.operation_id AND sent.attempt=e.attempt
         LEFT JOIN review_accepted_ids a ON a.account_id=e.account_id AND a.operation_id=e.operation_id AND a.attempt=e.attempt
         JOIN operation_gates g ON g.account_id=q.account_id AND g.id=q.id
         WHERE q.account_id=? AND q.state IN ('unknown','waiting') AND e.outcome='validated'
           AND sent.sending_at IS NOT NULL AND (a.remote_id IS NOT NULL OR sent.step='submit_review')
           AND g.hold IS NULL AND q.retry_at<=? ORDER BY q.retry_at,q.sequence LIMIT 1",
        params![account,through],|r|Ok((r.get(0)?,r.get(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,
            r.get::<_,Option<String>>(4)?,r.get(5)?))).optional()?;
    let Some((operation_id, attempt, raw_route, raw_step, remote, due)) = raw else {
        return Ok(None);
    };
    let step: ReviewStep = decode(&raw_step)?;
    let remote_id = match (&step, remote) {
        (ReviewStep::SubmitPending { review_id, .. }, None) => *review_id,
        (ReviewStep::CreatePending(_) | ReviewStep::Reply { .. }, Some(id)) => numeric(&id)?,
        _ => return Err(Error::Corrupt),
    };
    Ok(Some((
        ReviewReadback {
            operation_id,
            attempt,
            route: decode(&raw_route)?,
            step,
            remote_id,
        },
        due,
    )))
}

fn check_readback(db: &Connection, target: &ReviewReadback) -> Result<()> {
    let (original, step) = dispatch::sent(
        db,
        target.route.account,
        &target.operation_id,
        target.attempt,
    )?;
    let current = dispatch::route(db, original.account, &target.operation_id)?;
    let mut renewed = original.clone();
    renewed.credential_generation = current.credential_generation;
    let identity=match &step {
        ReviewStep::SubmitPending{review_id,..}=>*review_id==target.remote_id,
        ReviewStep::CreatePending(_)|ReviewStep::Reply{..}=>db.query_row(
            "SELECT EXISTS(SELECT 1 FROM review_accepted_ids WHERE account_id=? AND operation_id=? AND attempt=? AND remote_id=?)",
            params![original.account,target.operation_id,target.attempt,target.remote_id.to_string()],|r|r.get(0))?,
        _=>false,
    };
    let authorized: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM operation_gates WHERE account_id=? AND id=? AND hold IS NULL)",
        params![original.account, target.operation_id],
        |r| r.get(0),
    )?;
    if !authorized
        || !identity
        || original != target.route
        || current != renewed
        || step != target.step
    {
        return Err(Error::Conflict(
            "The accepted object, review scope or actor changed",
        ));
    }
    Ok(())
}

impl Store {
    pub fn next_review_due(&self, account: AccountId, now: i64) -> Result<Option<i64>> {
        self.read(|db|{
            let tx=db.unchecked_transaction()?;
            let eligible=tx.query_row("SELECT MIN(q.retry_at) FROM operations q JOIN review_intents w ON w.account_id=q.account_id AND w.operation_id=q.id
                JOIN operation_gates g USING(account_id,id) WHERE q.account_id=? AND q.state='queued' AND NOT q.cancel_requested AND g.hold IS NULL",
                [account],|r|r.get::<_,Option<i64>>(0))?;
            let lease=tx.query_row("SELECT MIN(q.lease_until) FROM operations q JOIN review_intents w ON w.account_id=q.account_id AND w.operation_id=q.id
                WHERE q.account_id=? AND q.state IN ('validating','sending')",[account],|r|r.get::<_,Option<i64>>(0))?;
            let floor=crate::writes::deadline(&tx,account)?.unwrap_or(now).max(now);
            let readback=next_readback(&tx,account,i64::MAX)?.map(|(_,at)|at);
            Ok(eligible.into_iter().chain(readback).map(|at|at.max(floor))
                .chain(lease.map(|at|at.max(now))).min())
        })
    }
    /// Reserve an exact existing object for GET. No readback candidates are generated from matching text or a thread's current boolean alone.
    pub fn claim_review_readback(
        &self,
        account: AccountId,
        now: i64,
        until: i64,
    ) -> Result<Option<ReviewReadback>> {
        if until <= now {
            return Err(Error::InvalidInput("Invalid review readback reservation"));
        }
        self.write(|tx| {
            if crate::writes::deadline(tx, account)?.is_some_and(|floor| floor > now) {
                return Ok(None);
            }
            let Some((target, _)) = next_readback(tx, account, now)? else {
                return Ok(None);
            };
            tx.execute(
                "UPDATE operations SET retry_at=? WHERE account_id=? AND id=? AND attempt=?",
                params![until, account, target.operation_id, target.attempt],
            )?;
            Ok(Some(target))
        })
    }
    pub fn review_readback_profile(&self, target: &ReviewReadback) -> Result<AccessProfile> {
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            check_readback(&tx, target)?;
            crate::access::profile(&tx, target.route.account, &target.route.owner)?
                .ok_or(Error::NotFound)
        })
    }
    pub fn defer_review_readback(&self, target: &ReviewReadback, not_before: i64) -> Result<()> {
        self.write(|tx|{
            check_readback(tx,target)?;
            tx.execute("UPDATE operations SET retry_at=MAX(retry_at,?) WHERE account_id=? AND id=? AND attempt=?",
                params![not_before,target.route.account,target.operation_id,target.attempt])?;
            Ok(())
        })
    }
}
