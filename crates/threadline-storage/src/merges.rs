//! Direct pull request merging with the captured head, verified repository capabilities and durable receipts.
pub use crate::mutations::PrMutationRoute as MergeRoute;
use crate::{
    AccountId, Error, ItemId, RepositoryId, Result, Store,
    access::AccessProfile,
    mutations,
    pr::{self, PrRevision},
    queue::{self, ActionKind, Claim, Delivery, Receipt, Submission, dispatch},
    resources::CommitOid,
    writes::{self, Permission, decode, encode},
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::num::NonZeroU64;
mod recovery;
pub use recovery::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}
impl MergeMethod {
    pub const fn github_name(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Squash => "squash",
            Self::Rebase => "rebase",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Gate {
    Satisfied,
    NotRequired,
    Blocking,
    Unknown,
}
impl Gate {
    fn allows(self) -> bool {
        matches!(self, Self::Satisfied | Self::NotRequired)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum QueueRequirement {
    Required,
    NotRequired,
    Unknown,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MergePolicy {
    pub allowed_methods: Option<BTreeSet<MergeMethod>>,
    pub viewer_permission: Permission,
    pub open: Option<bool>,
    pub draft: Option<bool>,
    pub merged: Option<bool>,
    pub mergeable: Option<bool>,
    pub queue: QueueRequirement,
    pub checks: Gate,
    pub reviews: Gate,
    pub branch_rules: Gate,
}
impl MergePolicy {
    pub fn permits_method(&self, method: MergeMethod) -> bool {
        self.allowed_methods
            .as_ref()
            .is_some_and(|m| m.contains(&method))
    }
    /// The direct method never requests administrative bypass, a merge queue or auto-merge.
    pub fn direct_hold(&self, method: MergeMethod) -> Option<&'static str> {
        if self.queue == QueueRequirement::Required {
            return Some("required_merge_queue_unsupported");
        }
        if self.queue == QueueRequirement::Unknown {
            return Some("merge_queue_rule_unknown");
        }
        if self.viewer_permission != Permission::Allowed {
            return Some("merge_permission_unknown_or_denied");
        }
        if !self.permits_method(method) {
            return Some("direct_merge_method_unavailable");
        }
        if self.open != Some(true) || self.draft != Some(false) || self.merged != Some(false) {
            return Some("pr_state_prevents_merge");
        }
        if self.mergeable != Some(true) {
            return Some("mergeability_unknown_or_blocked");
        }
        if !self.checks.allows() {
            return Some("required_checks_unknown_or_blocked");
        }
        if !self.reviews.allows() {
            return Some("required_reviews_unknown_or_blocked");
        }
        if !self.branch_rules.allows() {
            return Some("branch_rule_unknown_or_blocked");
        }
        None
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CachedMergePolicy {
    pub observed_at_ms: i64,
    pub policy: MergePolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeStatus {
    pub revision_id: i64,
    pub head: CommitOid,
    pub method: MergeMethod,
    pub state: queue::State,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmittedMerge {
    pub revision_id: i64,
    pub head: CommitOid,
    pub method: MergeMethod,
    pub base_ref: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MergeObservation {
    pub actor: NonZeroU64,
    pub repository_id: NonZeroU64,
    pub pr_id: NonZeroU64,
    pub number: NonZeroU64,
    pub credential_generation: i64,
    pub base: String,
    #[serde(default)]
    pub base_ref: Option<String>,
    pub head: String,
    pub observed_at_ms: i64,
    pub policy: MergePolicy,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum StoredIntent {
    Bound {
        method: MergeMethod,
        base_ref: Option<String>,
    },
    Legacy(MergeMethod),
}
impl StoredIntent {
    fn parts(self) -> (MergeMethod, Option<String>) {
        match self {
            Self::Bound { method, base_ref } => (method, base_ref),
            Self::Legacy(method) => (method, None),
        }
    }
}
pub struct MergeSubmission<'a> {
    pub id: &'a str,
    pub method: MergeMethod,
    pub dependencies: &'a [&'a str],
    pub submitted_at_ms: i64,
}
#[derive(Clone, Debug)]
pub struct MergeClaim {
    claim: Claim,
    pub route: MergeRoute,
    pub method: MergeMethod,
    pub expected_base_ref: Option<String>,
}
impl MergeClaim {
    pub fn operation_id(&self) -> &str {
        &self.claim.operation.id
    }
    pub fn attempt(&self) -> i64 {
        self.claim.number
    }
}
pub enum MergeAdmission {
    Send(MergeTicket),
    Held(&'static str),
}
pub struct MergeTicket(Box<MergeClaim>);
impl MergeTicket {
    pub fn claim(&self) -> &MergeClaim {
        &self.0
    }
}

fn cached(db: &Connection, rev: &PrRevision) -> Result<Option<CachedMergePolicy>> {
    pr::validate_revision(db, rev)?;
    let row=db.query_row("SELECT observed_at,policy FROM merge_policies WHERE revision_id=? AND account_id=? AND item_id=?",
        params![rev.id,rev.account,rev.pr],|r|Ok((r.get(0)?,r.get::<_,String>(1)?))).optional()?;
    row.map(|(observed_at_ms, s)| {
        Ok(CachedMergePolicy {
            observed_at_ms,
            policy: decode(&s)?,
        })
    })
    .transpose()
}
fn store_policy(
    tx: &Transaction<'_>,
    rev: &PrRevision,
    policy: &MergePolicy,
    at: i64,
) -> Result<()> {
    tx.execute("INSERT INTO merge_policies(revision_id,account_id,item_id,observed_at,policy) VALUES(?,?,?,?,?)
        ON CONFLICT(revision_id) DO UPDATE SET observed_at=excluded.observed_at,policy=excluded.policy
        WHERE excluded.observed_at>merge_policies.observed_at",params![rev.id,rev.account,rev.pr,at,encode(policy)?])?;
    Ok(())
}
fn hold(
    r: &MergeRoute,
    method: MergeMethod,
    observed: &MergeObservation,
    now: i64,
) -> Option<&'static str> {
    if (
        observed.actor,
        observed.repository_id,
        observed.pr_id,
        observed.number,
        observed.credential_generation,
    ) != (
        r.actor,
        r.repository_id,
        r.pr_id,
        r.number,
        r.credential_generation,
    ) {
        return Some("original_merge_identity_changed");
    }
    if observed.observed_at_ms > now || now.saturating_sub(observed.observed_at_ms) > 60_000 {
        return Some("merge_preflight_stale");
    }
    if observed.head != r.head {
        return Some("expected_merge_revision_changed");
    }
    observed.policy.direct_hold(method)
}
fn target_branch_matches(
    original_oid: &str,
    original_ref: Option<&str>,
    actual_oid: &str,
    actual_ref: Option<&str>,
) -> bool {
    match (original_ref, actual_ref) {
        (Some(expected), Some(actual)) => !expected.is_empty() && expected == actual,
        _ => original_oid == actual_oid,
    }
}
fn admission_hold(c: &MergeClaim, o: &MergeObservation, now: i64) -> Option<&'static str> {
    hold(&c.route, c.method, o, now).or_else(|| {
        (!target_branch_matches(
            &c.route.base,
            c.expected_base_ref.as_deref(),
            &o.base,
            o.base_ref.as_deref(),
        ))
        .then_some("merge_target_branch_changed")
    })
}

fn eligible(db: &Connection, c: &MergeClaim, now: i64) -> Result<bool> {
    let op = &c.claim.operation;
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM operations o JOIN operation_gates g USING(account_id,id)
        WHERE o.account_id=? AND o.id=? AND o.attempt=? AND o.state='validating'
          AND NOT o.cancel_requested AND o.lease_until>? AND g.hold IS NULL)",
        params![op.account, op.id, c.attempt(), now],
        |r| r.get(0),
    )?)
}
fn finish(tx: &Transaction<'_>, c: &Claim, state: &str, reason: &str, due: i64) -> Result<bool> {
    let op = &c.operation;
    let changed = tx.execute(
        "UPDATE operations SET state=?,reason=?,retry_at=?,lease_until=NULL WHERE account_id=?
        AND id=? AND attempt=? AND state='validating' AND NOT cancel_requested",
        params![state, reason, due, op.account, op.id, c.number],
    )?;
    if changed == 1 {
        dispatch::end_attempt(tx, op.account, &op.id, c.number, state, reason)?;
    }
    Ok(changed == 1)
}
impl Store {
    pub fn merge_policy(&self, rev: &PrRevision) -> Result<Option<CachedMergePolicy>> {
        self.read(|db| cached(db, rev))
    }

    /// Read an immutable merge submission even if its downloaded PR data was cleared.
    pub fn submitted_merge(&self, account: AccountId, id: &str) -> Result<Option<SubmittedMerge>> {
        self.read(|db| {
            let revision_id = db
                .query_row(
                    "SELECT revision_id FROM merge_intents WHERE account_id=? AND operation_id=?",
                    params![account, id],
                    |row| row.get(0),
                )
                .optional()?;
            revision_id
                .map(|revision_id| {
                    let operation =
                        queue::read_operation(db, account, id)?.ok_or(Error::Corrupt)?;
                    let (method, base_ref) =
                        decode::<StoredIntent>(&operation.snapshot.payload)?.parts();
                    Ok(SubmittedMerge {
                        revision_id,
                        head: CommitOid::parse(&operation.snapshot.base)?,
                        method,
                        base_ref,
                    })
                })
                .transpose()
        })
    }

    pub fn merge_statuses(&self, account: AccountId, pr: ItemId) -> Result<Vec<MergeStatus>> {
        self.read(|db| {
            let mut statement = db.prepare(
                "SELECT m.operation_id,m.revision_id FROM merge_intents m
                 JOIN operations o ON o.account_id=m.account_id AND o.id=m.operation_id
                 WHERE m.account_id=? AND m.item_id=? ORDER BY o.sequence DESC LIMIT 8",
            )?;
            let identities = statement.query_map(params![account, pr], |row| {
                Ok((row.get::<_, String>(0)?, row.get(1)?))
            })?;
            identities
                .map(|identity| {
                    let (id, revision_id) = identity?;
                    let operation =
                        queue::read_operation(db, account, &id)?.ok_or(Error::Corrupt)?;
                    let (method, _) = decode::<StoredIntent>(&operation.snapshot.payload)?.parts();
                    Ok(MergeStatus {
                        revision_id,
                        head: CommitOid::parse(&operation.snapshot.base)?,
                        method,
                        state: operation.state,
                        reason: operation.reason,
                    })
                })
                .collect()
        })
    }
    pub fn record_merge_policy(
        &self,
        p: &AccessProfile,
        rev: &PrRevision,
        policy: &MergePolicy,
        at: i64,
    ) -> Result<()> {
        if p.account != rev.account {
            return Err(Error::Conflict("Policy belongs to another account"));
        }
        self.write(|tx| {
            writes::require_observation_profile(tx, p, rev.repository)?;
            pr::validate_revision(tx, rev)?;
            store_policy(tx, rev, policy, at)
        })
    }
    pub fn submit_merge(&self, rev: &PrRevision, s: MergeSubmission<'_>) -> Result<String> {
        self.write(|tx| {
            pr::validate_revision(tx, rev)?;
            let existing = queue::read_operation(tx, rev.account, s.id)?;
            if existing.is_none() {
                let known = cached(tx, rev)?.ok_or(Error::Conflict(
                    "Available direct merge methods have not been downloaded",
                ))?;
                if !known.policy.permits_method(s.method)
                    || known.policy.queue == QueueRequirement::Required
                {
                    return Err(Error::Conflict(
                        "Direct merge is unavailable for the selected method or repository rules",
                    ));
                }
                if pr::current_in(tx, rev.account, rev.pr)?.is_none_or(|(now, _, _)| now.id != rev.id) {
                    return Err(Error::Conflict(
                        "The displayed head has changed; choose the current revision explicitly",
                    ));
                }
            }
            let duplicate = if existing.is_none() {
                tx.query_row("SELECT q.id FROM operations q JOIN merge_intents m ON m.account_id=q.account_id AND m.operation_id=q.id
                            WHERE q.account_id=? AND q.item_id=? AND m.revision_id=? AND q.state NOT IN ('confirmed','cancelled')
                            ORDER BY q.sequence LIMIT 1",params![rev.account,rev.pr,rev.id],|r|r.get::<_,String>(0)).optional()?
            } else {
                None
            };
            let id = duplicate.as_deref().unwrap_or(s.id);
            let payload=match queue::read_operation(tx,rev.account,id)? {
                Some(op) if op.kind==ActionKind::Merge && decode::<StoredIntent>(&op.snapshot.payload)?.parts().0==s.method=>op.snapshot.payload,
                _=>{
                    let (_,header)=pr::revision_in(tx,rev.account,rev.pr,rev.id)?.ok_or(Error::NotFound)?;
                    encode(&StoredIntent::Bound{method:s.method,base_ref:header.base_ref})?
                }
            };
            let pr_id:String=tx.query_row("SELECT d.pull_request_api_id FROM pull_request_details d JOIN items i ON i.id=d.item_id
                        JOIN repositories r ON r.id=i.repository_id JOIN github_access_profiles p ON p.account_id=i.account_id AND p.owner_scope=r.owner
                        WHERE i.id=? AND i.account_id=? AND i.repository_id=?",params![rev.pr,rev.account,rev.repository],|r|r.get(0))?;
            queue::enqueue_tx(
                tx,
                rev.account,
                Submission {
                    id,
                    item: rev.pr,
                    kind: ActionKind::Merge,
                    payload: &payload,
                    base: rev.head.as_str(),
                    anchor: &format!("pr-merge-v1/{}", rev.id),
                    projection: &format!("{} at {}", s.method.github_name(), rev.head.as_str()),
                    draft: None,
                    dependencies: s.dependencies,
                },
            )?;
            tx.execute("INSERT INTO merge_intents VALUES(?,?,?,?,?,?) ON CONFLICT(account_id,operation_id) DO NOTHING",
                        params![rev.account,id,rev.pr,rev.id,pr_id,s.submitted_at_ms])?;
            Ok(id.into())
        })
    }
    pub fn claim_merge(
        &self,
        account: AccountId,
        now: i64,
        until: i64,
    ) -> Result<Option<MergeClaim>> {
        if until <= now {
            return Err(Error::InvalidInput("Invalid merge claim deadline"));
        }
        self.write(|tx| {
            if writes::deadline(tx, account)?.is_some_and(|t| t > now) {
                return Ok(None);
            }
            let Some(claim) = dispatch::claim_in(tx, account, now, until, "merge")? else {
                return Ok(None);
            };
            let route = match mutations::pr_route(tx, account, &claim.operation.id) {
                Ok(route) => route,
                Err(Error::Conflict(_)) => {
                    finish(tx, &claim, "attention", "original_merge_route_changed", 0)?;
                    return Ok(None);
                }
                Err(error) => return Err(error),
            };
            let (method, expected_base_ref) =
                decode::<StoredIntent>(&claim.operation.snapshot.payload)?.parts();
            Ok(Some(MergeClaim {
                claim,
                route,
                method,
                expected_base_ref,
            }))
        })
    }
    pub fn merge_profile(&self, c: &MergeClaim, now: i64) -> Result<AccessProfile> {
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            if !eligible(&tx, c, now)?
                || mutations::pr_route(&tx, c.claim.operation.account, c.operation_id())? != c.route
            {
                return Err(Error::Conflict(
                    "The original merge lease or credential changed",
                ));
            }
            crate::access::profile(&tx, c.route.account, &c.route.owner)?.ok_or(Error::NotFound)
        })
    }
    pub fn authorize_merge(
        &self,
        c: &MergeClaim,
        o: &MergeObservation,
        now: i64,
    ) -> Result<MergeAdmission> {
        self.write(|tx| {
            let op = &c.claim.operation;
            let route = mutations::pr_route(tx, op.account, &op.id)?;
            if !eligible(tx, c, now)?
                || route != c.route
                || decode::<StoredIntent>(&op.snapshot.payload)?.parts()
                    != (c.method, c.expected_base_ref.clone())
            {
                return Err(Error::Conflict(
                    "The immutable merge target or attempt changed",
                ));
            }
            let reason = admission_hold(c, o, now);
            tx.execute(
                "INSERT INTO merge_attempts VALUES(?,?,?,?,?,?,?)",
                params![
                    op.account,
                    op.id,
                    c.attempt(),
                    encode(&route)?,
                    encode(o)?,
                    reason.unwrap_or("validated"),
                    o.observed_at_ms
                ],
            )?;
            if (
                o.actor,
                o.repository_id,
                o.pr_id,
                o.number,
                o.base.as_str(),
                o.head.as_str(),
                o.credential_generation,
            ) == (
                route.actor,
                route.repository_id,
                route.pr_id,
                route.number,
                route.base.as_str(),
                route.head.as_str(),
                route.credential_generation,
            ) && o.observed_at_ms <= now
            {
                let (rev, _) = pr::revision_in(tx, op.account, op.item, route.revision_id)?
                    .ok_or(Error::NotFound)?;
                store_policy(tx, &rev, &o.policy, o.observed_at_ms)?;
            }
            if let Some(reason) = reason {
                finish(tx, &c.claim, "attention", reason, 0)?;
                return Ok(MergeAdmission::Held(reason));
            }
            Ok(MergeAdmission::Send(MergeTicket(Box::new(c.clone()))))
        })
    }
    pub fn start_merge_transport(&self, ticket: &MergeTicket, now: i64) -> Result<bool> {
        let c = ticket.claim();
        self.write(|tx| {
            let op = &c.claim.operation;
            if !eligible(tx, c, now)? {
                return Ok(false);
            }
            if mutations::pr_route(tx, op.account, &op.id)? != c.route {
                finish(tx, &c.claim, "queued", "merge_profile_changed", now)?;
                return Ok(false);
            }
            let raw:String=tx.query_row("SELECT observation FROM merge_attempts WHERE account_id=? AND operation_id=? AND attempt=? AND decision='validated'",
                        params![op.account,op.id,c.attempt()],|r|r.get(0))?;
            let observed: MergeObservation = decode(&raw)?;
            let current = pr::current_in(tx, op.account, op.item)?;
            let Some((rev, _, _))=current.filter(|(rev,header,_)| rev.head.as_str()==c.route.head
                && target_branch_matches(&c.route.base,c.expected_base_ref.as_deref(),rev.base.as_str(),header.base_ref.as_deref())) else {
                finish(
                    tx,
                    &c.claim,
                    "attention",
                    "expected_merge_revision_changed",
                    0,
                )?;
                return Ok(false);
            };
            if rev.id!=c.route.revision_id && rev.base.as_str()!=observed.base {
                finish(tx,&c.claim,"queued","refresh_current_branch_policy",now)?;
                return Ok(false)
            }
            let veto = admission_hold(c,&observed,now)
                .or(cached(tx, &rev)?.and_then(|latest| latest.policy.direct_hold(c.method)));
            if let Some(reason) = veto {
                finish(tx, &c.claim, "attention", reason, 0)?;
                return Ok(false);
            }
            mutations::reserve_transport(tx, &c.claim, "direct_merge", now)
        })
    }
    pub fn abandon_merge_preflight(&self, c: &MergeClaim, now: i64) -> Result<bool> {
        self.write(|tx| finish(tx, &c.claim, "queued", "merge_preflight_interrupted", now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ut_017_direct_merge_requires_the_exact_method_expected_head_and_known_branch_policy() {
        let policy = MergePolicy {
            allowed_methods: Some(
                [MergeMethod::Merge, MergeMethod::Squash]
                    .into_iter()
                    .collect(),
            ),
            viewer_permission: Permission::Allowed,
            open: Some(true),
            draft: Some(false),
            merged: Some(false),
            mergeable: Some(true),
            queue: QueueRequirement::NotRequired,
            checks: Gate::NotRequired,
            reviews: Gate::Satisfied,
            branch_rules: Gate::Satisfied,
        };
        let n = |v| NonZeroU64::new(v).unwrap();
        let route = MergeRoute {
            account: AccountId(1),
            repository: RepositoryId(2),
            item: ItemId(3),
            actor: n(4),
            repository_id: n(5),
            pr_id: n(6),
            number: n(7),
            owner: "work".into(),
            name: "repository".into(),
            base: "a".repeat(40),
            head: "b".repeat(40),
            credential_generation: 2,
            revision_id: 8,
        };
        let o = MergeObservation {
            actor: n(4),
            repository_id: n(5),
            pr_id: n(6),
            number: n(7),
            base: route.base.clone(),
            base_ref: None,
            head: route.head.clone(),
            credential_generation: 2,
            observed_at_ms: 100,
            policy: policy.clone(),
        };
        assert_eq!(hold(&route, MergeMethod::Squash, &o, 102), None);
        assert_eq!(
            hold(&route, MergeMethod::Rebase, &o, 102),
            Some("direct_merge_method_unavailable")
        );
        for (policy, reason) in [
            (
                MergePolicy {
                    queue: QueueRequirement::Required,
                    ..policy.clone()
                },
                "required_merge_queue_unsupported",
            ),
            (
                MergePolicy {
                    queue: QueueRequirement::Unknown,
                    ..policy.clone()
                },
                "merge_queue_rule_unknown",
            ),
            (
                MergePolicy {
                    checks: Gate::Unknown,
                    ..policy.clone()
                },
                "required_checks_unknown_or_blocked",
            ),
            (
                MergePolicy {
                    reviews: Gate::Blocking,
                    ..policy.clone()
                },
                "required_reviews_unknown_or_blocked",
            ),
            (
                MergePolicy {
                    branch_rules: Gate::Blocking,
                    ..policy.clone()
                },
                "branch_rule_unknown_or_blocked",
            ),
            (
                MergePolicy {
                    viewer_permission: Permission::Denied,
                    ..policy.clone()
                },
                "merge_permission_unknown_or_denied",
            ),
            (
                MergePolicy {
                    draft: Some(true),
                    ..policy
                },
                "pr_state_prevents_merge",
            ),
        ] {
            assert_eq!(
                hold(
                    &route,
                    MergeMethod::Merge,
                    &MergeObservation {
                        policy,
                        ..o.clone()
                    },
                    103
                ),
                Some(reason)
            )
        }
        assert_eq!(
            hold(
                &route,
                MergeMethod::Merge,
                &MergeObservation {
                    head: "c".repeat(40),
                    ..o.clone()
                },
                103
            ),
            Some("expected_merge_revision_changed")
        );
        assert_eq!(
            hold(
                &route,
                MergeMethod::Merge,
                &MergeObservation {
                    actor: n(99),
                    ..o.clone()
                },
                103
            ),
            Some("original_merge_identity_changed")
        );
        assert_eq!(
            hold(&route, MergeMethod::Merge, &o, 100_000),
            Some("merge_preflight_stale")
        );
    }
}
