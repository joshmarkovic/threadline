use super::*;
use crate::queue::dispatch as outbox;
use crate::writes::Permission;

pub use crate::mutations::PrMutationRoute as ReviewRoute;
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReviewStep {
    CreatePending(ReviewDraft),
    SubmitPending {
        review_id: NonZeroU64,
        event: ReviewEvent,
        body: String,
    },
    Reply {
        thread_id: String,
        root_comment_id: NonZeroU64,
        body: String,
    },
    SetThreadResolved {
        thread_id: String,
        resolved: bool,
    },
}
impl ReviewStep {
    fn key(&self) -> &'static str {
        match self {
            Self::CreatePending(_) => "create_pending_review",
            Self::SubmitPending { .. } => "submit_review",
            Self::Reply { .. } => "reply_to_review_thread",
            Self::SetThreadResolved { resolved: true, .. } => "resolve_review_thread",
            Self::SetThreadResolved {
                resolved: false, ..
            } => "reopen_review_thread",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RemotePending {
    Unknown,
    Absent,
    Present {
        id: NonZeroU64,
        actor: NonZeroU64,
        head: String,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ThreadObservation {
    pub id: String,
    pub root_comment_id: Option<NonZeroU64>,
    pub outdated: Option<bool>,
    pub resolved: Option<bool>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReviewObservation {
    pub actor: NonZeroU64,
    pub repository_id: NonZeroU64,
    pub pr_id: NonZeroU64,
    pub number: NonZeroU64,
    pub credential_generation: i64,
    pub observed_at_ms: i64,
    pub base: String,
    pub head: String,
    pub permission: Permission,
    pub pr_open: Option<bool>,
    pub pr_author: Option<NonZeroU64>,
    pub pending: RemotePending,
    pub thread: Option<ThreadObservation>,
}

#[derive(Clone, Debug)]
pub struct ReviewClaim {
    claim: Claim,
    pub route: ReviewRoute,
    pub step: ReviewStep,
}
impl ReviewClaim {
    pub fn operation_id(&self) -> &str {
        &self.claim.operation.id
    }
    pub fn attempt(&self) -> i64 {
        self.claim.number
    }
}
pub enum ReviewAdmission {
    Send(ReviewTicket),
    Held(&'static str),
}
pub struct ReviewTicket(Box<ReviewClaim>);
impl ReviewTicket {
    pub fn claim(&self) -> &ReviewClaim {
        &self.0
    }
}

pub struct ReviewSubmission<'a> {
    pub id: &'a str,
    pub intent: &'a ReviewIntent,
    pub draft: Option<DraftSubmission<'a>>,
    pub dependencies: &'a [&'a str],
    pub submitted_at_ms: i64,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReviewResult {
    Pending {
        id: NonZeroU64,
        author: NonZeroU64,
        head: String,
    },
    Submitted {
        id: NonZeroU64,
        author: NonZeroU64,
        head: String,
        event: ReviewEvent,
    },
    Reply {
        id: NonZeroU64,
        author: NonZeroU64,
        root_comment_id: NonZeroU64,
        body: String,
    },
    Thread {
        id: String,
        resolved: bool,
    },
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewResponse {
    pub actor: NonZeroU64,
    pub repository_id: NonZeroU64,
    pub pr_id: NonZeroU64,
    pub number: NonZeroU64,
    pub result: ReviewResult,
}
pub enum ReviewProof<'a> {
    MutationResponse { request_id: &'a str },
    AcceptedObjectRead,
}
pub enum ReviewFailure {
    Unsent { retry_at_ms: i64 },
    Rejected { reason: &'static str },
    Throttled { not_before_ms: i64 },
    Ambiguous,
}

pub(super) use crate::mutations::pr_route as route;

fn step(db: &Connection, claim: &Claim) -> Result<ReviewStep> {
    let intent: ReviewIntent = decode(&claim.operation.snapshot.payload)?;
    Ok(match intent {
        ReviewIntent::Review(draft) => {
            let previous:Option<String>=db.query_row("SELECT remote_id FROM step_receipts WHERE account_id=?
                AND operation_id=? AND step='create_pending_review' AND object_kind='pending_review'",
                params![claim.operation.account,claim.operation.id],|r|r.get(0)).optional()?;
            match previous {
                Some(id) => ReviewStep::SubmitPending {
                    review_id: numeric(&id)?,
                    event: draft.event,
                    body: draft.summary,
                },
                None => ReviewStep::CreatePending(draft),
            }
        }
        ReviewIntent::Reply {
            thread_id,
            root_comment_id,
            body,
        } => ReviewStep::Reply {
            thread_id,
            root_comment_id,
            body,
        },
        ReviewIntent::SetThreadResolved {
            thread_id,
            resolved,
        } => ReviewStep::SetThreadResolved {
            thread_id,
            resolved,
        },
    })
}

fn hold(claim: &ReviewClaim, o: &ReviewObservation, now: i64) -> Option<&'static str> {
    let r = &claim.route;
    if (
        o.actor,
        o.repository_id,
        o.pr_id,
        o.number,
        o.credential_generation,
    ) != (
        r.actor,
        r.repository_id,
        r.pr_id,
        r.number,
        r.credential_generation,
    ) {
        return Some("original_identity_changed");
    }
    if o.observed_at_ms > now || now.saturating_sub(o.observed_at_ms) > 60_000 {
        return Some("stale_review_preflight");
    }
    if o.base != r.base || o.head != r.head {
        return Some("review_revision_changed");
    }
    if o.permission != Permission::Allowed || o.pr_open != Some(true) {
        return Some("review_permission_or_state_unknown");
    }
    let event = match &claim.step {
        ReviewStep::CreatePending(d) => Some(d.event),
        ReviewStep::SubmitPending { event, .. } => Some(*event),
        _ => None,
    };
    if matches!(
        event,
        Some(ReviewEvent::Approve | ReviewEvent::RequestChanges)
    ) && o.pr_author.is_none_or(|id| id == r.actor)
    {
        return Some("cannot_review_own_or_unknown_author");
    }
    match &claim.step {
        ReviewStep::CreatePending(_) if o.pending != RemotePending::Absent => {
            Some("pending_review_not_known_absent")
        }
        ReviewStep::SubmitPending { review_id, .. } if !matches!(&o.pending,RemotePending::Present{id,actor,head} if id==review_id && *actor==r.actor && head==&r.head) => {
            Some("pending_review_identity_changed")
        }
        ReviewStep::Reply {
            thread_id,
            root_comment_id,
            ..
        } if !o.thread.as_ref().is_some_and(|t| {
            &t.id == thread_id
                && t.root_comment_id == Some(*root_comment_id)
                && t.outdated == Some(false)
        }) =>
        {
            Some("original_thread_changed")
        }
        ReviewStep::SetThreadResolved {
            thread_id,
            resolved,
        } if !o.thread.as_ref().is_some_and(|t| {
            &t.id == thread_id && t.resolved == Some(!resolved) && t.outdated == Some(false)
        }) =>
        {
            Some("original_thread_state_changed")
        }
        _ => None,
    }
}
fn eligible(db: &Connection, claim: &ReviewClaim, now: i64) -> Result<bool> {
    let op = &claim.claim.operation;
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM operations o JOIN operation_gates g USING(account_id,id)
        WHERE o.account_id=? AND o.id=? AND o.attempt=? AND o.state='validating' AND o.lease_until>?
        AND NOT o.cancel_requested AND g.hold IS NULL)",
        params![op.account, op.id, claim.attempt(), now],
        |r| r.get(0),
    )?)
}
fn finish_preflight(
    tx: &Transaction<'_>,
    claim: &ReviewClaim,
    state: &str,
    reason: &str,
    at: i64,
) -> Result<bool> {
    let op = &claim.claim.operation;
    let changed = tx.execute(
        "UPDATE operations SET state=?,reason=?,retry_at=?,lease_until=NULL WHERE account_id=?
        AND id=? AND attempt=? AND state='validating' AND NOT cancel_requested",
        params![state, reason, at, op.account, op.id, claim.attempt()],
    )?;
    if changed == 1 {
        outbox::end_attempt(tx, op.account, &op.id, claim.attempt(), state, reason)?;
    }
    Ok(changed == 1)
}

impl Store {
    pub fn submit_review(&self, rev: &PrRevision, s: ReviewSubmission<'_>) -> Result<String> {
        s.intent.validate()?;
        self.write(|tx| {
            pr::validate_revision(tx, rev)?;
            let pr_id:String=tx.query_row("SELECT d.pull_request_api_id FROM pull_request_details d JOIN items i ON i.id=d.item_id
                        JOIN repositories r ON r.id=i.repository_id JOIN github_access_profiles p ON p.account_id=i.account_id AND p.owner_scope=r.owner
                        WHERE i.id=? AND i.account_id=? AND i.repository_id=?",params![rev.pr,rev.account,rev.repository],|r|r.get(0))?;
            let already = if let Some(draft) = s.draft {
                tx.query_row(
                    "SELECT id FROM operations WHERE account_id=? AND draft_id=? AND draft_revision=?",
                    params![rev.account, draft.id, draft.revision],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
            } else {
                None
            };
            let id = already.as_deref().unwrap_or(s.id);
            let existing = queue::read_operation(tx, rev.account, id)?;
            if existing.is_none() {
                match s.intent {
                    ReviewIntent::Review(draft) => {
                        if let Some(lease) = s.draft
                            && !drafts::saved_matches(tx, rev, lease, draft)?
                        {
                            return Err(Error::Conflict(
                                "The submitted review differs from the saved draft revision",
                            ));
                        }
                        drafts::verify_anchors(tx, rev, s.draft.map(|d| d.id), draft)?;
                    }
                    ReviewIntent::Reply { thread_id, .. }
                    | ReviewIntent::SetThreadResolved { thread_id, .. } => {
                        let cached =
                            stored_record(tx, rev, PrFamily::Threads, &format!("thread:{thread_id}"))?;
                        let Some(PrRecord::Thread {
                            node_id, comments, ..
                        }) = cached.map(|r| r.record)
                        else {
                            return Err(Error::Conflict(
                                "Original thread is not cached for this review",
                            ));
                        };
                        if &node_id != thread_id
                            || matches!(s.intent,ReviewIntent::Reply{root_comment_id,..}
                                    if !comments.iter().any(|c|c.id==root_comment_id.get() && c.in_reply_to.is_none()))
                        {
                            return Err(Error::Conflict(
                                "Original reply target is not the root comment of this thread",
                            ));
                        }
                    }
                }
            }
            let raw = encode(s.intent)?;
            let projection = match s.intent {
                ReviewIntent::Review(d) => format!("{:?} · {} inline comments", d.event, d.comments.len()),
                ReviewIntent::Reply { body, .. } => body.clone(),
                ReviewIntent::SetThreadResolved { resolved: true, .. } => "Resolve thread".into(),
                ReviewIntent::SetThreadResolved {
                    resolved: false, ..
                } => "Reopen thread".into(),
            };
            queue::enqueue_tx(
                tx,
                rev.account,
                Submission {
                    id,
                    item: rev.pr,
                    kind: s.intent.kind(),
                    payload: &raw,
                    base: rev.head.as_str(),
                    anchor: &format!("pr-review-v1/{}", rev.id),
                    projection: &projection,
                    draft: s.draft,
                    dependencies: s.dependencies,
                },
            )?;
            if s.draft.is_some()
                && let ReviewIntent::Reply { body, .. } = s.intent
            {
                let saved = queue::read_operation(tx, rev.account, id)?.ok_or(Error::Corrupt)?;
                if saved.snapshot.draft.is_none_or(|d| d.body != *body) {
                    return Err(Error::Conflict(
                        "The submitted reply differs from the saved revision",
                    ));
                }
            }
            tx.execute("INSERT INTO review_intents VALUES(?,?,?,?,?,?) ON CONFLICT(account_id,operation_id) DO NOTHING",
                        params![rev.account,id,rev.pr,rev.id,pr_id,s.submitted_at_ms])?;
            Ok(id.into())
        })
    }

    pub fn claim_review(
        &self,
        account: AccountId,
        now: i64,
        until: i64,
    ) -> Result<Option<ReviewClaim>> {
        if until <= now {
            return Err(Error::InvalidInput("Invalid review preflight lease"));
        }
        self.write(|tx| {
            if crate::writes::deadline(tx, account)?.is_some_and(|t| t > now) {
                return Ok(None);
            }
            let Some(claim) = outbox::claim_in(tx, account, now, until, "review")? else {
                return Ok(None);
            };
            let route = match route(tx, account, &claim.operation.id) {
                Ok(route)=>route,
                Err(Error::Conflict(_))=>{
                    tx.execute("UPDATE operations SET state='attention',reason='original_review_route_changed',
                        lease_until=NULL WHERE account_id=? AND id=? AND attempt=?",
                        params![account,claim.operation.id,claim.number])?;
                    outbox::end_attempt(tx,account,&claim.operation.id,claim.number,"attention","original_review_route_changed")?;
                    return Ok(None);
                }
                Err(error)=>return Err(error),
            };
            let step = step(tx, &claim)?;
            Ok(Some(ReviewClaim { claim, route, step }))
        })
    }
    pub fn review_profile(&self, claim: &ReviewClaim, now: i64) -> Result<AccessProfile> {
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            if !eligible(&tx, claim, now)?
                || route(&tx, claim.route.account, claim.operation_id())? != claim.route
            {
                return Err(Error::Conflict(
                    "The original review scope or credential changed",
                ));
            }
            crate::access::profile(&tx, claim.route.account, &claim.route.owner)?
                .ok_or(Error::NotFound)
        })
    }
    pub fn authorize_review(
        &self,
        claim: &ReviewClaim,
        observed: &ReviewObservation,
        now: i64,
    ) -> Result<ReviewAdmission> {
        self.write(|tx| {
            let original = &claim.claim.operation;
            if !eligible(tx, claim, now)?
                || route(tx, original.account, &original.id)? != claim.route
                || step(tx, &claim.claim)? != claim.step
            {
                return Err(Error::Conflict("Review preflight changed or expired"));
            }
            let veto = hold(claim, observed, now);
            tx.execute(
                "INSERT INTO review_attempts VALUES(?,?,?,?,?,?,?, ?,?)",
                params![
                    original.account,
                    original.id,
                    claim.attempt(),
                    observed.observed_at_ms,
                    claim.route.credential_generation,
                    encode(&claim.route)?,
                    encode(&claim.step)?,
                    encode(observed)?,
                    veto.unwrap_or("validated")
                ],
            )?;
            if let Some(reason) = veto {
                finish_preflight(tx, claim, "attention", reason, 0)?;
                return Ok(ReviewAdmission::Held(reason));
            }
            Ok(ReviewAdmission::Send(ReviewTicket(Box::new(claim.clone()))))
        })
    }
    pub fn fail_review_preflight(
        &self,
        claim: &ReviewClaim,
        why: crate::writes::WritePreflightFailure,
        now: i64,
    ) -> Result<()> {
        use crate::writes::WritePreflightFailure as Failure;
        self.write(|tx|{
            let op=&claim.claim.operation;
            if !eligible(tx,claim,now)? || route(tx,op.account,&op.id)?!=claim.route{
                return Err(Error::Conflict("The review preflight changed or expired"));
            }
            let (state,reason,floor)=match why {
                Failure::Transient=>("queued","review_preflight_unreachable",None),
                Failure::Throttled{not_before_ms}=>{
                    crate::sync::server_floor_in(tx,op.account,not_before_ms)?;
                    ("queued","review_preflight_throttled",Some(not_before_ms))
                }
                Failure::Unauthorized=>("attention","review_authentication_required",None),
                Failure::Forbidden=>("attention","review_permission_denied",None),
                Failure::Missing=>("attention","original_review_target_unavailable",None),
                Failure::InvalidResponse=>("attention","invalid_review_preflight",None),
            };
            let attempt=u32::try_from(claim.attempt()).unwrap_or(u32::MAX);
            let delay=threadline_core::sync::policy::retry_delay(attempt,std::time::Duration::ZERO)
                .unwrap_or(std::time::Duration::from_secs(15*60));
            let retry=now.saturating_add(i64::try_from(delay.as_millis()).unwrap_or(i64::MAX)).max(floor.unwrap_or(now));
            tx.execute("INSERT INTO review_attempts(account_id,operation_id,attempt,observed_at,credential_generation,route,step,observed,outcome)
                VALUES(?,?,?,?,?,?,?,'null',?)",params![op.account,op.id,claim.attempt(),now,claim.route.credential_generation,
                encode(&claim.route)?,encode(&claim.step)?,reason])?;
            finish_preflight(tx,claim,state,reason,if state=="queued"{retry}else{0})?;
            Ok(())
        })
    }
    /// Recheck an access failure once a new credential for the original actor and owner is installed.
    pub fn resume_reviews(&self, account: AccountId, now: i64) -> Result<usize> {
        self.write(|tx|{
            let floor=crate::writes::deadline(tx,account)?.unwrap_or(now).max(now);
            Ok(tx.execute("UPDATE operations AS q SET state='queued',retry_at=?2,reason='review_access_repaired'
                WHERE q.account_id=?1 AND NOT q.cancel_requested AND q.state='attention'
                  AND q.reason IN ('review_authentication_required','review_permission_denied')
                  AND EXISTS(SELECT 1 FROM review_attempts e JOIN github_operation_scopes s
                      ON s.account_id=e.account_id AND s.operation_id=e.operation_id
                      JOIN github_access_profiles p ON p.account_id=s.account_id AND p.owner_scope=s.owner_scope
                      JOIN operation_gates g ON g.account_id=s.account_id AND g.id=s.operation_id
                      WHERE e.account_id=q.account_id AND e.operation_id=q.id AND e.attempt=q.attempt
                        AND p.credential_generation>e.credential_generation AND g.hold IS NULL)",
                params![account,floor])?)
        })
    }
    pub fn abandon_review_preflight(&self, claim: &ReviewClaim, now: i64) -> Result<bool> {
        self.write(|tx| finish_preflight(tx, claim, "queued", "review_preflight_interrupted", now))
    }
    pub fn start_review_transport(&self, ticket: &ReviewTicket, now: i64) -> Result<bool> {
        let claim = ticket.claim();
        self.write(|tx| {
            let op = &claim.claim.operation;
            if !eligible(tx, claim, now)? {
                return Ok(false);
            }
            if route(tx, op.account, &op.id)? != claim.route {
                finish_preflight(tx, claim, "queued", "review_profile_changed", now)?;
                return Ok(false);
            }
            let observed:String=tx.query_row("SELECT observed FROM review_attempts WHERE account_id=? AND operation_id=? AND attempt=? AND outcome='validated'",
                        params![op.account,op.id,claim.attempt()],|r|r.get(0))?;
            if hold(claim, &decode(&observed)?, now).is_some()
                || pr::current_in(tx, op.account, op.item)?
                    .is_none_or(|(r, _, _)| r.id != claim.route.revision_id)
            {
                finish_preflight(
                    tx,
                    claim,
                    "attention",
                    "review_revision_or_observation_changed",
                    0,
                )?;
                return Ok(false);
            }
            crate::mutations::reserve_transport(tx,&claim.claim,claim.step.key(),now)
        })
    }
}

pub(super) fn sent(
    db: &Connection,
    account: AccountId,
    id: &str,
    attempt: i64,
) -> Result<(ReviewRoute, ReviewStep)> {
    let (route,step):(String,String)=db.query_row(
        "SELECT e.route,e.step FROM review_attempts e JOIN operation_attempts a USING(account_id,operation_id,attempt)
         JOIN operations o ON o.account_id=e.account_id AND o.id=e.operation_id AND o.attempt=e.attempt
         WHERE e.account_id=? AND e.operation_id=? AND e.attempt=? AND e.outcome='validated'
          AND a.sending_at IS NOT NULL AND o.state IN ('sending','waiting','unknown')",
        params![account,id,attempt],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(Error::Conflict("No dispatched review step is awaiting evidence"))?;
    Ok((decode(&route)?, decode(&step)?))
}

fn validate_result(
    route: &ReviewRoute,
    step: &ReviewStep,
    response: &ReviewResponse,
) -> Result<(&'static str, String)> {
    if (
        response.actor,
        response.repository_id,
        response.pr_id,
        response.number,
    ) != (route.actor, route.repository_id, route.pr_id, route.number)
    {
        return Err(Error::Conflict(
            "Review response belongs to another actor or PR",
        ));
    }
    match (step, &response.result) {
        (ReviewStep::CreatePending(_), ReviewResult::Pending { id, author, head })
            if *author == route.actor && head == &route.head =>
        {
            Ok(("pending_review", id.to_string()))
        }
        (
            ReviewStep::SubmitPending {
                review_id, event, ..
            },
            ReviewResult::Submitted {
                id,
                author,
                head,
                event: actual,
            },
        ) if id == review_id
            && author == &route.actor
            && head == &route.head
            && event == actual =>
        {
            Ok(("submitted_review", id.to_string()))
        }
        (
            ReviewStep::Reply {
                root_comment_id,
                body,
                ..
            },
            ReviewResult::Reply {
                id,
                author,
                root_comment_id: root,
                body: actual,
            },
        ) if root_comment_id == root && author == &route.actor && id != root && body == actual => {
            Ok(("review_reply", id.to_string()))
        }
        (
            ReviewStep::SetThreadResolved {
                thread_id,
                resolved,
            },
            ReviewResult::Thread {
                id,
                resolved: actual,
            },
        ) if thread_id == id && resolved == actual => Ok(("review_thread", id.clone())),
        _ => Err(Error::Conflict(
            "Review response does not confirm this exact submitted step",
        )),
    }
}

impl Store {
    /// The create response identified the new review or reply, but its full payload was unavailable.
    pub fn accept_review_object(
        &self,
        account: AccountId,
        id: &str,
        attempt: i64,
        remote: NonZeroU64,
        request_id: &str,
    ) -> Result<()> {
        if request_id.trim().is_empty() {
            return Err(Error::InvalidInput("Missing review request identity"));
        }
        self.write(|tx| {
            let (_, step) = sent(tx, account, id, attempt)?;
            if !matches!(
                step,
                ReviewStep::CreatePending(_) | ReviewStep::Reply { .. }
            ) {
                return Err(Error::InvalidInput(
                    "The dispatched step does not create a new object",
                ));
            }
            let old:Option<String>=tx.query_row("SELECT remote_id FROM review_accepted_ids WHERE account_id=? AND operation_id=? AND attempt=?",
                        params![account,id,attempt],|r|r.get(0)).optional()?;
            if old.is_some_and(|n| n != remote.to_string()) {
                return Err(Error::Conflict(
                    "The attempt already acknowledged another identity",
                ));
            }
            tx.execute("INSERT INTO review_accepted_ids VALUES(?,?,?,?,?) ON CONFLICT(account_id,operation_id,attempt) DO NOTHING",
                        params![account,id,attempt,remote.to_string(),request_id])?;
            outbox::record_delivery_in(
                tx,
                account,
                id,
                attempt,
                Delivery::Waiting("Review object identity accepted; read back exact ID"),
            )
        })
    }

    pub fn confirm_review_step(
        &self,
        account: AccountId,
        id: &str,
        attempt: i64,
        proof: ReviewProof<'_>,
        response: &ReviewResponse,
        now: i64,
    ) -> Result<()> {
        self.write(|tx| {
            let (route, step) = sent(tx, account, id, attempt)?;
            let (kind, remote) = validate_result(&route, &step, response)?;
            let acknowledged=tx.query_row("SELECT remote_id FROM review_accepted_ids
                WHERE account_id=? AND operation_id=? AND attempt=?",params![account,id,attempt],|r|r.get::<_,String>(0)).optional()?;
            if acknowledged.as_ref().is_some_and(|ack|ack!=&remote){
                return Err(Error::Conflict("The review response contradicts the acknowledged remote identity"));
            }

            let evidence = match proof {
                ReviewProof::MutationResponse { request_id } if !request_id.trim().is_empty() => {
                    format!("GitHub request {request_id}")
                }
                ReviewProof::MutationResponse { .. } => {
                    return Err(Error::InvalidInput("Missing review request identity"));
                }
                ReviewProof::AcceptedObjectRead => {
                    let allowed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM operation_gates WHERE account_id=? AND id=? AND hold IS NULL)",
                                params![account,id],|r|r.get(0))?;
                    let acknowledged:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM review_accepted_ids WHERE account_id=? AND operation_id=?
                                AND attempt=? AND remote_id=?)",params![account,id,attempt,remote],|r|r.get(0))?;
                    let final_event = matches!(step, ReviewStep::SubmitPending { .. });
                    if !allowed
                        || (!acknowledged && !final_event)
                        || matches!(step, ReviewStep::SetThreadResolved { .. })
                    {
                        return Err(Error::Conflict(
                            "Current thread state or similar text cannot establish who performed the action",
                        ));
                    }
                    "Exact original review or reply ID, actor, target and state reread".into()
                }
            };
            let partial = matches!(step, ReviewStep::CreatePending(_));
            let receipt = Receipt {
                step: step.key().into(),
                object_kind: kind.into(),
                remote_id: remote.clone(),
                evidence,
            };
            outbox::record_delivery_in(
                tx,
                account,
                id,
                attempt,
                if partial {
                    Delivery::Step(&receipt)
                } else {
                    Delivery::Confirmed(&receipt)
                },
            )?;
            tx.execute(
                "INSERT INTO review_activity VALUES(?,?,?,?,?,?,?)",
                params![account, id, step.key(), route.item, route.head, remote, now],
            )?;
            Ok(())
        })
    }

    pub fn record_review_failure(
        &self,
        account: AccountId,
        id: &str,
        attempt: i64,
        why: ReviewFailure,
        now: i64,
    ) -> Result<()> {
        self.write(|tx| {
            sent(tx, account, id, attempt)?;
            let accepted:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM review_accepted_ids WHERE account_id=?
                AND operation_id=? AND attempt=?)",params![account,id,attempt],|r|r.get(0))?;
            if accepted && !matches!(why,ReviewFailure::Ambiguous){
                return Err(Error::Conflict("The accepted review or reply cannot be replayed"));
            }
            match why {
                ReviewFailure::Ambiguous => outbox::record_delivery_in(
                    tx,
                    account,
                    id,
                    attempt,
                    Delivery::Unknown("Review effect unknown; no automatic replay"),
                ),
                ReviewFailure::Unsent { retry_at_ms } => outbox::record_delivery_in(
                    tx,
                    account,
                    id,
                    attempt,
                    Delivery::NotApplied {
                        proof: "Transport proved no request was sent",
                        retry_at: retry_at_ms.max(now),
                    },
                ),
                ReviewFailure::Throttled { not_before_ms } => {
                    let at = not_before_ms.max(now.saturating_add(1000));
                    crate::sync::server_floor_in(tx, account, at)?;
                    outbox::record_delivery_in(
                        tx,
                        account,
                        id,
                        attempt,
                        Delivery::NotApplied {
                            proof: "GitHub rejected the review request with throttling",
                            retry_at: at,
                        },
                    )
                }
                ReviewFailure::Rejected { reason } => {
                    if reason.trim().is_empty() {
                        return Err(Error::InvalidInput("Missing authoritative rejection"));
                    }
                    outbox::record_delivery_in(
                        tx,
                        account,
                        id,
                        attempt,
                        Delivery::NotApplied {
                            proof: reason,
                            retry_at: 0,
                        },
                    )?;
                    tx.execute("UPDATE operations SET state='attention',reason=? WHERE account_id=? AND id=? AND state='queued' AND NOT cancel_requested",
                                params![reason,account,id])?;
                    Ok(())
                }
            }
        })
    }
}
