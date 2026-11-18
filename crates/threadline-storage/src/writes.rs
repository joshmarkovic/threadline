//! Typed issue and ordinary-comment intents, preflight and receipts. Networking is performed by the provider.
use crate::{
    AccountId, Error, ItemId, RepositoryId, Result, Store,
    queue::{
        ActionKind, Claim, DraftSubmission, State, Submission, dispatch, enqueue_tx, read_operation,
    },
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Serialize, de::DeserializeOwned};
use std::num::NonZeroU64;

mod model;
#[cfg(test)]
mod tests;
pub use model::*;
mod choices;
pub use choices::*;
mod control;
pub use control::*;
mod outcomes;
pub use outcomes::*;

pub struct WriteSubmission<'a> {
    pub id: &'a str,
    pub item: ItemId,
    pub intent: &'a WriteIntent,
    pub draft: Option<DraftSubmission<'a>>,
    pub dependencies: &'a [&'a str],
    pub submitted_at_ms: i64,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WriteRoute {
    pub account: AccountId,
    pub actor: NonZeroU64,
    pub owner: String,
    pub repository_name: String,
    pub repository: RepositoryId,
    pub repository_id: NonZeroU64,
    pub number: NonZeroU64,
    pub issue_id: NonZeroU64,
    pub item: ItemId,
    pub pull_request: bool,
    pub credential_generation: i64,
}
#[derive(Clone, Debug)]
pub struct WriteClaim {
    claim: Claim,
    pub route: WriteRoute,
    pub intent: WriteIntent,
    pub comment_id: Option<NonZeroU64>,
}
impl WriteClaim {
    pub fn operation_id(&self) -> &str {
        &self.claim.operation.id
    }
    pub fn attempt(&self) -> i64 {
        self.claim.number
    }
}
#[derive(Clone, Debug)]
pub struct MutationTicket {
    inner: Box<WriteClaim>,
}
impl MutationTicket {
    pub fn claim(&self) -> &WriteClaim {
        &self.inner
    }
}
#[derive(Clone, Debug)]
pub enum WriteAdmission {
    Send(MutationTicket),
    NeedsAttention(&'static str),
    Deferred(i64),
}

pub(super) fn encode<T: Serialize>(data: &T) -> Result<String> {
    serde_json::to_string(data).map_err(|_| Error::InvalidInput("Invalid write payload"))
}
pub(super) fn decode<T: DeserializeOwned>(raw: &str) -> Result<T> {
    serde_json::from_str(raw).map_err(|_| Error::Corrupt)
}
fn numeric(raw: &str) -> Result<NonZeroU64> {
    raw.parse().map_err(|_| Error::Corrupt)
}

fn load_route(tx: &Connection, account: AccountId, id: &str) -> Result<WriteRoute> {
    let raw=tx.query_row(
        "SELECT a.provider_user_id,r.owner,r.name,r.id,r.provider_repository_id,i.number,w.issue_api_id,i.id,
         i.kind='pull_request',p.credential_generation FROM write_intents w
         JOIN operations o ON o.account_id=w.account_id AND o.id=w.operation_id
         JOIN items i ON i.id=o.item_id AND i.account_id=o.account_id
         JOIN accounts a ON a.id=w.account_id
         JOIN repositories r ON r.id=i.repository_id AND r.account_id=i.account_id
         JOIN github_operation_scopes scope ON scope.account_id=o.account_id AND scope.operation_id=o.id
         JOIN github_access_profiles p ON p.account_id=scope.account_id AND p.owner_scope=scope.owner_scope
         WHERE w.account_id=? AND w.operation_id=? AND i.issue_api_id=w.issue_api_id",
        params![account,id],
        |r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,RepositoryId>(3)?,
            r.get::<_,String>(4)?,r.get::<_,i64>(5)?,r.get::<_,String>(6)?,r.get::<_,ItemId>(7)?,
            r.get::<_,bool>(8)?,r.get::<_,i64>(9)?)),
    ).optional()?.ok_or(Error::Conflict("The stored GitHub item or owner profile changed"))?;
    Ok(WriteRoute {
        account,
        actor: numeric(&raw.0)?,
        owner: raw.1,
        repository_name: raw.2,
        repository: raw.3,
        repository_id: numeric(&raw.4)?,
        number: u64::try_from(raw.5)
            .ok()
            .and_then(NonZeroU64::new)
            .ok_or(Error::Corrupt)?,
        issue_id: numeric(&raw.6)?,
        item: raw.7,
        pull_request: raw.8,
        credential_generation: raw.9,
    })
}
fn resolved_comment(
    tx: &Connection,
    claim: &Claim,
    intent: &WriteIntent,
) -> Result<Option<NonZeroU64>> {
    match intent {
        WriteIntent::EditComment {
            target: CommentTarget::Remote(id),
            ..
        } => Ok(Some(*id)),
        WriteIntent::EditComment {
            target: CommentTarget::CreatedBy(predecessor),
            ..
        } => {
            let id = tx
                .query_row(
                    "SELECT remote_id FROM own_write_actions WHERE account_id=? AND item_id=?
                AND operation_id=? AND action='comment_created'",
                    params![claim.operation.account, claim.operation.item, predecessor],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
                .ok_or(Error::Conflict(
                    "The confirmed predecessor has no comment receipt",
                ))?;
            Ok(Some(numeric(&id)?))
        }
        _ => Ok(None),
    }
}

fn deadline(db: &Connection, account: AccountId) -> Result<Option<i64>> {
    let write: Option<i64> = db
        .query_row(
            "SELECT not_before FROM write_timing WHERE account_id=?",
            [account],
            |r| r.get(0),
        )
        .optional()?;
    Ok(write.max(crate::sync::server_floor(db, account)?))
}

impl Store {
    pub fn submit_write(&self, account: AccountId, s: WriteSubmission<'_>) -> Result<String> {
        s.intent.validate()?;
        self.write(|tx| {
            let issue=tx.query_row(
                "SELECT i.issue_api_id FROM items i JOIN repositories r ON r.id=i.repository_id AND r.account_id=i.account_id
                 JOIN github_access_profiles p ON p.account_id=i.account_id AND p.owner_scope=r.owner
                 WHERE i.account_id=? AND i.id=?", params![account,s.item], |row|row.get::<_,Option<String>>(0),
            ).optional()?.flatten().ok_or(Error::Conflict("An established GitHub item and owner profile are required"))?;
            let mut dependencies=s.dependencies.to_vec();
            if let WriteIntent::EditComment{target:CommentTarget::CreatedBy(predecessor),body} = s.intent {
                let previous=read_operation(tx,account,predecessor)?.ok_or(Error::NotFound)?;
                let typed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM write_intents WHERE account_id=? AND operation_id=?)",
                    params![account,predecessor],|r|r.get(0))?;
                if !typed || previous.item!=s.item || !matches!(decode(&previous.snapshot.payload)?,
                    WriteIntent::CreateComment{body:original} if original==body.base) {
                    return Err(Error::Conflict("The earlier comment belongs to a different intent or target"));
                }
                if !dependencies.contains(&predecessor.as_str()){dependencies.push(predecessor)}
            }
            let raw=encode(s.intent)?;
            let kind=match s.intent {WriteIntent::EditIssue(_)=>ActionKind::EditIssue,_=>ActionKind::Comment};
            let already = match s.draft {
                Some(draft)=>tx.query_row("SELECT id FROM operations WHERE account_id=? AND draft_id=? AND draft_revision=?",
                    params![account,draft.id,draft.revision],|r|r.get::<_,String>(0)).optional()?,
                None=>None,
            };
            let operation_id=already.as_deref().unwrap_or(s.id);
            if let WriteIntent::EditComment{target:CommentTarget::Remote(id),..}=s.intent
                && read_operation(tx,account,operation_id)?.is_none() {
                let key=format!("{:020}",id.get());
                let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM cached_resources c
                    JOIN reader_comment_metadata m ON m.resource_id=c.id WHERE c.account_id=? AND c.item_id=?
                    AND c.family='comments' AND m.record_key=?)",params![account,s.item,key],|r|r.get(0))?;
                if !valid {return Err(Error::Conflict("Comment identity is not established for this original item"))}
            }
            enqueue_tx(tx,account,Submission{id:operation_id,item:s.item,kind,payload:&raw,base:"github-write-intent-v1",
                anchor:&issue,projection:&raw,draft:s.draft,dependencies:&dependencies})?;
            let saved=read_operation(tx,account,operation_id)?.ok_or(Error::Corrupt)?;
            if let Some(draft)=saved.snapshot.draft {
                let expected=match s.intent {
                    WriteIntent::CreateComment{body}=>Some(body.as_str()),
                    WriteIntent::EditComment{body,..}=>Some(body.wanted.as_str()),
                    WriteIntent::EditIssue(changes)=>changes.body.as_ref().map(|field|field.wanted.as_deref().unwrap_or_default()),
                };
                if expected.is_some_and(|body|body!=draft.body){return Err(Error::Conflict("Submitted text differs from the saved editor revision"))}
            }
            tx.execute("INSERT INTO write_intents(account_id,operation_id,issue_api_id,submitted_at) VALUES(?,?,?,?)
                ON CONFLICT(account_id,operation_id) DO NOTHING",params![account,operation_id,issue,s.submitted_at_ms])?;
            Ok(operation_id.to_owned())
        })
    }

    pub fn write_intent(&self, account: AccountId, id: &str) -> Result<Option<WriteIntent>> {
        self.read(|db| {
            let raw=db.query_row("SELECT json_extract(o.snapshot,'$.payload') FROM operations o JOIN write_intents w
                        ON w.account_id=o.account_id AND w.operation_id=o.id WHERE o.account_id=? AND o.id=?",
                        params![account,id],|r|r.get::<_,String>(0)).optional()?;
            raw.as_deref().map(decode).transpose()
        })
    }
    pub fn claim_write(
        &self,
        account: AccountId,
        now: i64,
        lease_until: i64,
    ) -> Result<Option<WriteClaim>> {
        if lease_until <= now {
            return Err(Error::InvalidInput(
                "Write preflight lease must expire after it starts",
            ));
        }
        self.write(|tx| {
            if deadline(tx, account)?.is_some_and(|floor| floor > now) {
                return Ok(None);
            }
            let Some(claim) = dispatch::claim_in(tx, account, now, lease_until, true)? else {
                return Ok(None);
            };
            let route = load_route(tx, account, &claim.operation.id)?;
            let intent = decode(&claim.operation.snapshot.payload)?;
            let comment_id = resolved_comment(tx, &claim, &intent)?;
            Ok(Some(WriteClaim {
                claim,
                route,
                intent,
                comment_id,
            }))
        })
    }
    pub fn authorize_write(
        &self,
        claim: &WriteClaim,
        observed: &WriteObservation,
        now: i64,
    ) -> Result<WriteAdmission> {
        self.write(|tx| {
            let op = &claim.claim.operation;
            let at = claim.attempt();
            let started=tx.query_row("SELECT a.claimed_at FROM operation_attempts a JOIN operations o ON
                        o.account_id=a.account_id AND o.id=a.operation_id AND o.attempt=a.attempt
                        JOIN operation_gates g ON g.account_id=o.account_id AND g.id=o.id
                        WHERE o.account_id=? AND o.id=? AND o.attempt=? AND o.state='validating' AND o.lease_until>?
                        AND NOT o.cancel_requested AND g.hold IS NULL",params![op.account,op.id,at,now],|r|r.get::<_,i64>(0))
                        .optional()?.ok_or(Error::Conflict("Write claim expired or lost its permission or target"))?;
            if observed.observed_at_ms < started
                || observed.observed_at_ms > now
                || load_route(tx, op.account, &op.id)? != claim.route
                || observed.credential_generation != claim.route.credential_generation
            {
                return Err(Error::Conflict(
                    "Preflight belongs to a different credential or attempt",
                ));
            }
            if let Some(floor) = deadline(tx, op.account)?.filter(|t| *t > now) {
                end_preflight(tx, claim, "queued", "mutation_pacing", floor)?;
                return Ok(WriteAdmission::Deferred(floor));
            }
            if decode::<WriteIntent>(&op.snapshot.payload)? != claim.intent
                || resolved_comment(tx, &claim.claim, &claim.intent)? != claim.comment_id
            {
                return Err(Error::Conflict(
                    "The claimed operation differs from its immutable submission",
                ));
            }
            let other_sending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM operations o JOIN write_intents w
                        ON w.account_id=o.account_id AND w.operation_id=o.id WHERE o.account_id=? AND o.id<>? AND o.state='sending')",
                        params![op.account,op.id],|r|r.get(0))?;
            if other_sending {
                let due = now.saturating_add(1_000);
                end_preflight(tx, claim, "queued", "mutation_in_progress", due)?;
                return Ok(WriteAdmission::Deferred(due));
            }
            let reason = veto(claim, observed);
            let current = if observed.permission != Permission::Allowed
                || reason == Some("github_target_or_account_changed")
            {
                None
            } else {
                Some(observed)
            };
            tx.execute("INSERT INTO write_evidence(account_id,operation_id,attempt,credential_generation,observed_at,current_fields,result,route)
                        VALUES(?,?,?,?,?,?,?,?)",params![op.account,op.id,at,observed.credential_generation,observed.observed_at_ms,
                            encode(&current)?,reason.unwrap_or("validated"),encode(&claim.route)?])?;
            if let Some(reason) = reason {
                end_preflight(tx, claim, "attention", reason, 0)?;
                return Ok(WriteAdmission::NeedsAttention(reason));
            }
            Ok(WriteAdmission::Send(MutationTicket {
                inner: Box::new(claim.clone()),
            }))
        })
    }
}
fn end_preflight(
    tx: &Transaction<'_>,
    claim: &WriteClaim,
    state: &str,
    reason: &str,
    at: i64,
) -> Result<()> {
    let op = &claim.claim.operation;
    let changed = tx.execute(
        "UPDATE operations SET state=?4,reason=?5,retry_at=?6,lease_until=NULL
        WHERE account_id=?1 AND id=?2 AND attempt=?3 AND state='validating'",
        params![op.account, op.id, claim.attempt(), state, reason, at],
    )?;
    if changed != 1 {
        return Err(Error::Conflict("Write claim no longer holds"));
    }
    dispatch::end_attempt(tx, op.account, &op.id, claim.attempt(), state, reason)
}
fn veto(claim: &WriteClaim, now: &WriteObservation) -> Option<&'static str> {
    let route = &claim.route;
    if now.actor != route.actor
        || now.repository != route.repository_id
        || now.issue != route.issue_id
        || now.number != route.number
        || now.pull_request != route.pull_request
    {
        return Some("github_target_or_account_changed");
    }
    if now.permission != Permission::Allowed {
        return Some("write_permission_unavailable");
    }
    match &claim.intent {
        WriteIntent::EditIssue(fields) if !matches_fields(fields, &now.fields, false) => {
            Some("issue_fields_conflict")
        }
        WriteIntent::EditIssue(fields) if !available_choices(fields, &now.choices) => {
            Some("repository_choice_unavailable")
        }
        WriteIntent::EditComment { body, .. } => match &now.comment {
            Some(comment) if Some(comment.id) == claim.comment_id && comment.body == body.base => {
                None
            }
            _ => Some("comment_identity_or_body_conflict"),
        },
        _ => None,
    }
}
