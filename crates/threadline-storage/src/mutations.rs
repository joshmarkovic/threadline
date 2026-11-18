//! Shared original pull-request routes and serial mutation transport reservations.
use crate::{
    AccountId, Error, ItemId, RepositoryId, Result,
    queue::{Claim, dispatch},
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

fn numeric(s: &str) -> Result<NonZeroU64> {
    s.parse().map_err(|_| Error::Corrupt)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PrMutationRoute {
    pub account: AccountId,
    pub repository: RepositoryId,
    pub item: ItemId,
    pub actor: NonZeroU64,
    pub repository_id: NonZeroU64,
    pub pr_id: NonZeroU64,
    pub number: NonZeroU64,
    pub owner: String,
    pub name: String,
    pub credential_generation: i64,
    pub base: String,
    pub head: String,
    pub revision_id: i64,
}
pub(crate) fn pr_route(db: &Connection, account: AccountId, id: &str) -> Result<PrMutationRoute> {
    let raw = db.query_row(
        "SELECT q.item_id,r.id,a.provider_user_id,r.provider_repository_id,v.pr_api_id,i.number,
           r.owner,r.name,p.credential_generation,b.base_oid,b.head_oid,b.id
         FROM pr_mutation_targets v JOIN operations q ON q.account_id=v.account_id AND q.id=v.operation_id
         JOIN items i ON i.id=q.item_id AND i.account_id=q.account_id
         JOIN pull_request_details d ON d.item_id=i.id AND d.pull_request_api_id=v.pr_api_id
         JOIN pr_revisions b ON b.id=v.revision_id AND b.repository_id=i.repository_id
         JOIN repositories r ON r.id=i.repository_id AND r.account_id=i.account_id
         JOIN accounts a ON a.id=q.account_id
         JOIN github_operation_scopes s ON s.account_id=q.account_id AND s.operation_id=q.id
         JOIN github_access_profiles p ON p.account_id=s.account_id AND p.owner_scope=s.owner_scope
         WHERE q.account_id=? AND q.id=? AND p.owner_scope=r.owner",
        params![account,id],|row| Ok((
            row.get(0)?, row.get(1)?, row.get::<_,String>(2)?, row.get::<_,String>(3)?,
            row.get::<_,String>(4)?, row.get::<_,i64>(5)?, row.get(6)?, row.get(7)?,
            row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?,
        )))
        .optional()?.ok_or(Error::Conflict("The original PR identity or owner no longer matches"))?;
    let (
        item,
        repository,
        actor,
        repo,
        pr,
        number,
        owner,
        name,
        credential_generation,
        base,
        head,
        revision_id,
    ) = raw;
    Ok(PrMutationRoute {
        account,
        item,
        repository,
        actor: numeric(&actor)?,
        repository_id: numeric(&repo)?,
        pr_id: numeric(&pr)?,
        number: NonZeroU64::new(u64::try_from(number).map_err(|_| Error::Corrupt)?)
            .ok_or(Error::Corrupt)?,
        owner,
        name,
        credential_generation,
        base,
        head,
        revision_id,
    })
}
/// Called in the same transaction as the family-specific immutable intent, fresh-evidence and generation checks.
pub(crate) fn reserve_transport(
    tx: &Transaction<'_>,
    claim: &Claim,
    step: &str,
    now: i64,
) -> Result<bool> {
    let op = &claim.operation;
    let busy:bool=tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM operations o JOIN mutation_families f
           ON f.account_id=o.account_id AND f.operation_id=o.id WHERE o.account_id=? AND o.state='sending')",
        [op.account],|r|r.get(0))?;
    let floor = crate::writes::deadline(tx, op.account)?.unwrap_or(now);
    if busy || floor > now {
        let due = floor.max(now.saturating_add(1_000));
        let changed=tx.execute("UPDATE operations SET state='queued',reason='mutation_pacing',retry_at=?,
                lease_until=NULL WHERE account_id=? AND id=? AND attempt=? AND state='validating' AND NOT cancel_requested",
            params![due,op.account,op.id,claim.number])?;
        if changed == 1 {
            dispatch::end_attempt(
                tx,
                op.account,
                &op.id,
                claim.number,
                "queued",
                "mutation_pacing",
            )?;
        }
        return Ok(false);
    }
    dispatch::begin_send_in(tx, claim, step, now)?;
    let next = now.saturating_add(1_000);
    crate::sync::budget_in(tx, op.account, next)?;
    tx.execute("INSERT INTO write_timing(account_id,not_before) VALUES(?,?)
        ON CONFLICT(account_id) DO UPDATE SET not_before=MAX(write_timing.not_before,excluded.not_before)",params![op.account,next])?;
    Ok(true)
}
