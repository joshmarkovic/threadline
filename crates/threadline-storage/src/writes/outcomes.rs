use super::*;
use crate::{
    Change, IssueApiId, IssueClassification, IssueRecord, ItemRepresentation, ItemUpdate,
    UtcMillis,
    queue::{Delivery, Receipt},
    reader::{
        DiscussionComment, IssuePresentation, ListFacets, ListSummary, commit_item_metadata_in,
    },
    resources::{ResourceFamily, ResourceKey, ResourceTarget, Validators, ensure_resource},
};
use std::time::Duration;
use threadline_core::sync::policy::retry_delay;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfirmedObject {
    Issue {
        fields: IssueValues,
        milestone_title: Option<String>,
        updated_at_ms: i64,
    },
    Comment(RemoteComment),
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfirmedWrite {
    pub actor: NonZeroU64,
    pub repository: NonZeroU64,
    pub issue: NonZeroU64,
    pub number: NonZeroU64,
    pub object: ConfirmedObject,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteFailure {
    /// The transport could not establish a connection and can prove no mutation bytes were sent.
    DefinitelyUnsent,
    /// An authoritative HTTP rejection. Retain the existing intent and its original scope.
    AccessRejected,
    InvalidInput,
    /// An authoritative throttling response without a mutation effect.
    Throttled {
        not_before_ms: i64,
    },
    /// Includes a timeout, connection loss after send, and ambiguous 5xx.
    Ambiguous,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfirmationProof<'a> {
    /// Positive mutation response to this exact recorded HTTP attempt.
    MutationResponse { request_id: &'a str },
    /// Read-only lookup of the exact created comment ID previously acknowledged for this same attempt.
    AcceptedCommentRead,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnWrite {
    pub action: String,
    pub remote_id: NonZeroU64,
    pub revision: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteConflict {
    pub reason: String,
    pub intended: WriteIntent,
    pub current: Option<WriteObservation>,
}

fn request_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 160
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'-' | b'_'))
    {
        return Err(Error::InvalidInput("Invalid HTTP request receipt"));
    }
    Ok(())
}
fn typed_claim(
    tx: &Transaction<'_>,
    account: AccountId,
    id: &str,
    attempt: i64,
) -> Result<WriteClaim> {
    let op = read_operation(tx, account, id)?.ok_or(Error::NotFound)?;
    if op.attempt != attempt
        || !matches!(
            op.state,
            State::Sending | State::Waiting | State::OutcomeUnknown
        )
    {
        return Err(Error::Conflict("No such dispatched write"));
    }
    let sending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM write_evidence w JOIN operation_attempts a
        USING(account_id,operation_id,attempt) WHERE w.account_id=? AND w.operation_id=? AND w.attempt=?
        AND w.result='validated' AND a.sending_at IS NOT NULL)",params![account,id,attempt],|r|r.get(0))?;
    if !sending {
        return Err(Error::Conflict("Missing validated send boundary"));
    }
    let route: WriteRoute = decode(&tx.query_row(
        "SELECT route FROM write_evidence WHERE account_id=? AND operation_id=? AND attempt=?",
        params![account, id, attempt],
        |r| r.get::<_, String>(0),
    )?)?;
    let intent = decode(&op.snapshot.payload)?;
    let claim = Claim {
        operation: op,
        number: attempt,
    };
    let comment_id = resolved_comment(tx, &claim, &intent)?;
    Ok(WriteClaim {
        claim,
        route,
        intent,
        comment_id,
    })
}

impl Store {
    pub fn write_conflict(&self, account: AccountId, id: &str) -> Result<Option<WriteConflict>> {
        self.read(|db| {
            let row = db
                .query_row(
                    "SELECT o.snapshot,e.current_fields,e.result FROM operations o JOIN write_evidence e
                        ON e.account_id=o.account_id AND e.operation_id=o.id AND e.attempt=o.attempt
                        WHERE o.account_id=? AND o.id=? AND e.result<>'validated'",
                    params![account, id],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?;
            row.map(|(raw, stored, reason)| {
                let snapshot: crate::queue::Snapshot = decode(&raw)?;
                Ok(WriteConflict {
                    reason,
                    intended: decode(&snapshot.payload)?,
                    current: decode(&stored)?,
                })
            })
            .transpose()
        })
    }

    pub fn record_write_failure(
        &self,
        account: AccountId,
        id: &str,
        attempt: i64,
        reason: WriteFailure,
        now: i64,
    ) -> Result<()> {
        self.write(|tx| {
            let claim = typed_claim(tx, account, id, attempt)?;
            if matches!(reason, WriteFailure::Ambiguous) {
                return dispatch::record_delivery_in(
                    tx,
                    account,
                    id,
                    attempt,
                    Delivery::Unknown("Mutation delivery remains unproved"),
                );
            }
            let accepted: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM write_accepted_objects
                WHERE account_id=? AND operation_id=? AND attempt=?)",params![account,id,attempt],|r|r.get(0))?;
            if accepted {
                return Err(Error::Conflict("The transport already acknowledged creation; the attempt cannot be replayed"));
            }
            // Rejection/no-send proof must come from the same transport attempt; cached content is never such proof.
            let (reason, needs_attention, deadline) = match reason {
                WriteFailure::DefinitelyUnsent => ("transport_proved_unsent", false, None),
                WriteFailure::Throttled { not_before_ms } => {
                    crate::sync::server_floor_in(tx, account, not_before_ms)?;
                    ("mutation_throttled", false, Some(not_before_ms))
                }
                WriteFailure::AccessRejected => ("mutation_access_rejected", true, None),
                WriteFailure::InvalidInput => ("mutation_invalid_input", true, None),
                WriteFailure::Ambiguous => unreachable!(),
            };
            let burst = super::control::attempts_in_burst(tx, &claim)?;
            let entropy = claim.operation_id().bytes().fold(attempt as u64, |v, b| {
                v.wrapping_mul(31).wrapping_add(u64::from(b))
            });
            let step = retry_delay(burst.max(1), Duration::ZERO);
            let delay = step.unwrap_or(Duration::from_secs(15 * 60)) + Duration::from_millis(entropy % 501);
            let next = now
                .saturating_add(i64::try_from(delay.as_millis()).unwrap_or(i64::MAX))
                .max(deadline.unwrap_or(now));
            dispatch::record_delivery_in(
                tx,
                account,
                id,
                attempt,
                Delivery::NotApplied {
                    proof: reason,
                    retry_at: next,
                },
            )?;
            if step.is_none() && !needs_attention {
                tx.execute("UPDATE operations SET reason='retry_burst_exhausted' WHERE account_id=? AND id=? AND state='queued'",
                            params![account,id])?;
            }
            if needs_attention {
                tx.execute("UPDATE operations SET state='attention',reason=?3 WHERE account_id=?1 AND id=?2 AND state='queued'",
                            params![account,id,reason])?;
            }
            Ok(())
        })
    }

    pub fn accept_created_comment(
        &self,
        ticket: &MutationTicket,
        remote_id: NonZeroU64,
        request: &str,
    ) -> Result<()> {
        request_id(request)?;
        let claim = ticket.claim();
        self.write(|tx| {
            if !matches!(claim.intent, WriteIntent::CreateComment { .. }) {
                return Err(Error::InvalidInput(
                    "Only a create-comment attempt can accept a new remote ID",
                ));
            }
            tx.execute("INSERT INTO write_accepted_objects(account_id,operation_id,attempt,kind,remote_id,request_id)
                        VALUES(?,?,?,'comment',?,?)",params![claim.route.account,claim.operation_id(),claim.attempt(),remote_id.to_string(),request])?;
            dispatch::record_delivery_in(
                tx,
                claim.route.account,
                claim.operation_id(),
                claim.attempt(),
                Delivery::Waiting("GitHub acknowledged this exact created-comment ID"),
            )
        })
    }

    pub fn confirm_write(
        &self,
        account: AccountId,
        id: &str,
        attempt: i64,
        proof: ConfirmationProof<'_>,
        response: &ConfirmedWrite,
        now: i64,
    ) -> Result<()> {
        self.write(|tx| {
            let c = typed_claim(tx, account, id, attempt)?;
            let route = &c.route;
            if response.actor != route.actor
                || response.repository != route.repository_id
                || response.issue != route.issue_id
                || response.number != route.number
            {
                return Err(Error::Conflict(
                    "Confirmed write target differs from the original dispatched GitHub identity",
                ));
            }
            let (object, remote_id, revision) = match (&c.intent, &response.object) {
                (
                    WriteIntent::EditIssue(fields),
                    ConfirmedObject::Issue {
                        fields: current,
                        updated_at_ms,
                        ..
                    },
                ) if matches_fields(fields, current, true) => {
                    ("issue", route.issue_id, updated_at_ms.to_string())
                }
                (WriteIntent::CreateComment { body }, ConfirmedObject::Comment(comment))
                    if &comment.body == body && comment.author_id == Some(route.actor) =>
                {
                    ("comment", comment.id, comment.updated_at_ms.to_string())
                }
                (WriteIntent::EditComment { body, .. }, ConfirmedObject::Comment(comment))
                    if comment.body == body.wanted && c.comment_id == Some(comment.id) =>
                {
                    ("comment", comment.id, comment.updated_at_ms.to_string())
                }
                _ => {
                    return Err(Error::Conflict(
                        "The returned object does not identify the submitted intent",
                    ));
                }
            };
            let accepted=tx.query_row("SELECT remote_id FROM write_accepted_objects
                WHERE account_id=? AND operation_id=? AND attempt=?",params![account,id,attempt],
                |r|r.get::<_,String>(0)).optional()?;
            if accepted.is_some_and(|ack|ack!=remote_id.to_string()) {
                return Err(Error::Conflict("The response contradicts the acknowledged comment identity"));
            }
            let evidence = match proof {
                ConfirmationProof::MutationResponse { request_id: code } => {
                    request_id(code)?;
                    format!("GitHub mutation response {code}")
                }
                ConfirmationProof::AcceptedCommentRead => {
                    let authorized:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM operation_gates WHERE account_id=? AND id=? AND hold IS NULL)",
                                params![account,id],|r|r.get(0))?;
                    if !authorized {
                        return Err(Error::Conflict(
                            "Read-only reconciliation is held for the original credential scope",
                        ));
                    }
                    let matched:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM write_accepted_objects
                                WHERE account_id=? AND operation_id=? AND attempt=? AND kind='comment' AND remote_id=?)",
                                params![account,id,attempt,remote_id.to_string()],|r|r.get(0))?;
                    if !matches!(c.intent, WriteIntent::CreateComment { .. }) || !matched {
                        return Err(Error::Conflict(
                            "A matching body or an unbound remote object cannot prove delivery",
                        ));
                    }
                    "Read back the exact comment ID acknowledged for this creation".into()
                }
            };
            let action = c.intent.action();
            let receipt = Receipt {
                step: action.into(),
                object_kind: object.into(),
                remote_id: remote_id.to_string(),
                evidence,
            };
            dispatch::record_delivery_in(tx, account, id, attempt, Delivery::Confirmed(&receipt))?;
            let still_at_route:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM items WHERE id=? AND account_id=? AND repository_id=? AND number=?
                        AND issue_api_id=?)",params![route.item,account,route.repository,route.number.get().to_string(),route.issue_id.to_string()],|r|r.get(0))?;
            if still_at_route {
                publish(tx, CacheTarget::from(route), &response.object, now)?;
            }
            tx.execute("INSERT INTO own_write_actions(account_id,operation_id,repository_id,item_id,action,remote_id,remote_revision,confirmed_at)
                        VALUES(?,?,?,?,?,?,?,?)",params![account,id,route.repository,route.item,action,remote_id.to_string(),revision,now])?;
            Ok(())
        })
    }

    pub fn own_write(&self, account: AccountId, operation: &str) -> Result<Option<OwnWrite>> {
        self.read(|db| {
            let row=db.query_row("SELECT action,remote_id,remote_revision FROM own_write_actions WHERE account_id=? AND operation_id=?",
                        params![account,operation],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).optional()?;
            row.map(|(action, id, revision)| {
                Ok(OwnWrite {
                    action,
                    remote_id: numeric(&id)?,
                    revision,
                })
            })
            .transpose()
        })
    }
}
pub(super) struct CacheTarget {
    pub account: AccountId,
    pub repository: RepositoryId,
    pub item: ItemId,
    pub issue_id: NonZeroU64,
    pub number: NonZeroU64,
}
impl From<&WriteRoute> for CacheTarget {
    fn from(r: &WriteRoute) -> Self {
        Self {
            account: r.account,
            repository: r.repository,
            item: r.item,
            issue_id: r.issue_id,
            number: r.number,
        }
    }
}
pub(super) fn publish(
    tx: &Transaction<'_>,
    route: CacheTarget,
    object: &ConfirmedObject,
    at: i64,
) -> Result<()> {
    match object {
        ConfirmedObject::Comment(c) => {
            if c.updated_at_ms < c.created_at_ms {
                return Err(Error::InvalidInput("Invalid comment revision"));
            }
            let id = ensure_resource(
                tx,
                ResourceKey {
                    account: route.account,
                    target: ResourceTarget::Item(route.item),
                    family: ResourceFamily::Comments,
                },
            )?;
            tx.execute(
                "UPDATE cached_resources SET coverage=1 WHERE id=? AND coverage=0",
                [id],
            )?;
            if let Some(author_id) = c.author_id {
                super::choices::comment_author_in(
                    tx,
                    route.account,
                    route.item,
                    CommentAuthorObservation {
                        comment_id: c.id,
                        author_id,
                        observed_at_ms: c.updated_at_ms,
                    },
                )?;
            }
            crate::sync::apply_downloaded_comment(
                tx,
                route.item,
                &DiscussionComment {
                    provider_id: c.id.get(),
                    node_id: None,
                    author: c.author.clone(),
                    created_at_ms: c.created_at_ms,
                    updated_at_ms: c.updated_at_ms,
                    body: c.body.clone(),
                },
            )
        }
        ConfirmedObject::Issue {
            fields,
            milestone_title,
            updated_at_ms,
        } => {
            let change = |v: &Option<String>| v.clone().map_or(Change::Missing, Change::Value);
            let body = match &fields.body {
                None => Change::Missing,
                Some(None) => Change::Null,
                Some(Some(v)) => Change::Value(v.clone()),
            };
            let milestone = match (fields.milestone, milestone_title) {
                (Some(None), _) => Change::Null,
                (Some(Some(_)), Some(title)) => Change::Value(title.clone()),
                _ => Change::Missing,
            };
            let summary = ListSummary {
                number: route.number.get(),
                update: ItemUpdate {
                    representation: ItemRepresentation::Issue(IssueRecord {
                        id: IssueApiId::new(route.issue_id),
                        node_id: None,
                        classification: IssueClassification::Issue,
                    }),
                    remote_updated_at: UtcMillis(*updated_at_ms),
                    title: change(&fields.title),
                    body,
                    closed: fields.closed.map_or(Change::Missing, Change::Value),
                },
                facets: ListFacets {
                    labels: fields.labels.as_ref().map(|s| s.iter().cloned().collect()),
                    assignees: fields
                        .assignees
                        .as_ref()
                        .map(|s| s.iter().cloned().collect()),
                    presentation: IssuePresentation {
                        milestone,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            };
            if let Some(number) = fields.milestone {
                super::choices::milestone_in(
                    tx,
                    route.account,
                    route.item,
                    number,
                    *updated_at_ms,
                )?;
            }
            let item = commit_item_metadata_in(
                tx,
                route.account,
                route.repository,
                summary,
                &Validators::default(),
                at,
            )?;
            if item != route.item {
                return Err(Error::Conflict(
                    "Confirmed issue no longer has its original locator",
                ));
            }
            Ok(())
        }
    }
}
