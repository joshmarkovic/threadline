use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MergeFailure {
    DefinitelyUnsent,
    Throttled { not_before_ms: i64 },
    RequiredMergeQueue,
    ChangedHead,
    PermissionDenied,
    RulesRejected,
    InvalidInput,
    Ambiguous,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeReadback {
    pub operation_id: String,
    pub attempt: i64,
    pub route: MergeRoute,
    pub method: MergeMethod,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergedPr {
    pub actor: NonZeroU64,
    pub repository_id: NonZeroU64,
    pub pr_id: NonZeroU64,
    pub number: NonZeroU64,
    pub expected_head: String,
    pub merge_oid: CommitOid,
    pub merged_by: Option<NonZeroU64>,
    /// Timestamp from a PR response when available.
    pub remote_updated_at_ms: Option<i64>,
}
pub enum MergeProof<'a> {
    MutationResponse { request_id: &'a str },
    MergedPrRead,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfirmedMerge {
    pub item: ItemId,
    pub repository: RepositoryId,
    pub expected_head: String,
    pub merge_oid: CommitOid,
    pub own_action: bool,
    pub confirmed_at_ms: i64,
}

fn sent(db: &Connection, a: AccountId, id: &str, n: i64) -> Result<(MergeRoute, MergeMethod)> {
    let raw=db.query_row(
        "SELECT e.route,json_extract(q.snapshot,'$.payload') FROM merge_attempts e
         JOIN operation_attempts s USING(account_id,operation_id,attempt)
         JOIN operations q ON q.account_id=e.account_id AND q.id=e.operation_id AND q.attempt=e.attempt
         WHERE e.account_id=? AND e.operation_id=? AND e.attempt=? AND e.decision='validated'
           AND s.sending_at IS NOT NULL AND s.step='direct_merge' AND q.state IN ('sending','unknown','waiting')",
        params![a,id,n],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).optional()?
        .ok_or(Error::Conflict("No sent merge attempt is awaiting evidence"))?;
    Ok((decode(&raw.0)?, decode(&raw.1)?))
}

fn readback(db: &Connection, a: AccountId, until: i64) -> Result<Option<(MergeReadback, i64)>> {
    let row=db.query_row("SELECT q.id,q.attempt,q.retry_at FROM operations q JOIN merge_intents m
        ON m.account_id=q.account_id AND m.operation_id=q.id JOIN operation_gates g USING(account_id,id)
        WHERE q.account_id=? AND q.state IN ('unknown','waiting') AND g.hold IS NULL AND q.retry_at<=?
          AND EXISTS(SELECT 1 FROM merge_attempts e WHERE e.account_id=q.account_id AND e.operation_id=q.id
              AND e.attempt=q.attempt AND e.decision='validated')
        ORDER BY q.retry_at,q.sequence LIMIT 1",params![a,until],|r|Ok((r.get::<_,String>(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let Some((operation_id, attempt, at)) = row else {
        return Ok(None);
    };
    let (route, method) = sent(db, a, &operation_id, attempt)?;
    Ok(Some((
        MergeReadback {
            operation_id,
            attempt,
            route,
            method,
        },
        at,
    )))
}
fn verify_readback(db: &Connection, target: &MergeReadback) -> Result<()> {
    let r = &target.route;
    let (original, method) = sent(db, r.account, &target.operation_id, target.attempt)?;
    let current = mutations::pr_route(db, r.account, &target.operation_id)?;
    let mut renewed = original.clone();
    renewed.credential_generation = current.credential_generation;
    let allowed: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM operation_gates WHERE account_id=? AND id=? AND hold IS NULL)",
        params![r.account, target.operation_id],
        |row| row.get(0),
    )?;
    if !allowed || original != *r || current != renewed || method != target.method {
        return Err(Error::Conflict(
            "The original merge account or PR scope changed",
        ));
    }
    Ok(())
}

fn publish(tx: &Transaction<'_>, r: &MergeRoute, observed: &MergedPr) -> Result<()> {
    // A late positive result remains valid for its original Outbox operation, even after a local transfer.
    let live:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM items i JOIN pull_request_details d ON d.item_id=i.id
        WHERE i.id=? AND i.account_id=? AND i.repository_id=? AND i.number=? AND d.pull_request_api_id=?)",
        params![r.item,r.account,r.repository,r.number.to_string(),r.pr_id.to_string()],|row|row.get(0))?;
    if !live {
        return Ok(());
    }
    let old: Option<String> = tx
        .query_row(
            "SELECT merge_oid FROM pull_request_details WHERE item_id=? AND merged=1 AND merge_oid_at>=merged_at",
            [r.item],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    if old
        .as_deref()
        .is_some_and(|s| s != observed.merge_oid.as_str())
    {
        return Err(Error::Conflict(
            "The original pull request already has a different confirmed merge commit",
        ));
    }
    if let Some(remote) = observed.remote_updated_at_ms {
        crate::identity::ingest_in(
            tx,
            r.account,
            crate::ItemLocator {
                repository: r.repository,
                number: r.number.get(),
            },
            crate::ItemUpdate {
                representation: crate::ItemRepresentation::PullRequest(crate::PullRequestRecord {
                    id: crate::PullRequestApiId::try_from(r.pr_id.get())?,
                    node_id: None,
                    details: crate::PullRequestUpdate {
                        merged: crate::Change::Value(true),
                        merge_oid: crate::Change::Value(observed.merge_oid.as_str().into()),
                        ..Default::default()
                    },
                }),
                remote_updated_at: crate::UtcMillis(remote),
                title: crate::Change::Missing,
                body: crate::Change::Missing,
                closed: crate::Change::Value(true),
            },
        )?;
    }
    // The direct endpoint may omit provider time. A confirmed merge is irreversible and supplies its final OID.
    tx.execute(
        "UPDATE pull_request_details SET merged=1,merge_oid=? WHERE item_id=? AND account_id=?",
        params![observed.merge_oid.as_str(), r.item, r.account],
    )?;
    Ok(())
}

impl Store {
    pub fn next_merge_due(&self, a: AccountId, now: i64) -> Result<Option<i64>> {
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            let eligible=tx.query_row("SELECT MIN(q.retry_at) FROM operations q JOIN merge_intents m ON m.account_id=q.account_id AND m.operation_id=q.id
                        JOIN operation_gates g USING(account_id,id) WHERE q.account_id=? AND q.state='queued' AND NOT q.cancel_requested AND g.hold IS NULL",
                        [a],|r|r.get::<_,Option<i64>>(0))?;
            let lease=tx.query_row("SELECT MIN(q.lease_until) FROM operations q JOIN merge_intents m ON m.account_id=q.account_id AND m.operation_id=q.id
                        WHERE q.account_id=? AND q.state IN ('validating','sending')",[a],|r|r.get::<_,Option<i64>>(0))?;
            let floor = writes::deadline(&tx, a)?.unwrap_or(now).max(now);
            Ok(eligible
                .into_iter()
                .chain(readback(&tx, a, i64::MAX)?.map(|(_, due)| due))
                .map(|t| t.max(floor))
                .chain(lease.map(|t| t.max(now)))
                .min())
        })
    }
    pub fn claim_merge_readback(
        &self,
        a: AccountId,
        now: i64,
        until: i64,
    ) -> Result<Option<MergeReadback>> {
        if until <= now {
            return Err(Error::InvalidInput("Invalid merge readback reservation"));
        }
        self.write(|tx| {
            if writes::deadline(tx, a)?.is_some_and(|t| t > now) {
                return Ok(None);
            }
            let Some((target, _)) = readback(tx, a, now)? else {
                return Ok(None);
            };
            tx.execute(
                "UPDATE operations SET retry_at=? WHERE account_id=? AND id=? AND attempt=?",
                params![until, a, target.operation_id, target.attempt],
            )?;
            Ok(Some(target))
        })
    }
    pub fn merge_readback_profile(&self, target: &MergeReadback) -> Result<AccessProfile> {
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            verify_readback(&tx, target)?;
            crate::access::profile(&tx, target.route.account, &target.route.owner)?
                .ok_or(Error::NotFound)
        })
    }
    pub fn defer_merge_readback(&self, target: &MergeReadback, not_before: i64) -> Result<()> {
        self.write(|tx| {
            verify_readback(tx, target)?;
            tx.execute(
                "UPDATE operations SET retry_at=MAX(retry_at,?) WHERE account_id=? AND id=? AND attempt=?",
                params![
                    not_before,
                    target.route.account,
                    target.operation_id,
                    target.attempt
                ],
            )?;
            Ok(())
        })
    }
    /// An acknowledgement can carry an OID, but never marks the PR merged without positive evidence.
    pub fn accept_merge(
        &self,
        a: AccountId,
        id: &str,
        attempt: i64,
        request: &str,
        merge_oid: Option<&CommitOid>,
    ) -> Result<()> {
        if request.trim().is_empty() {
            return Err(Error::InvalidInput("Missing merge request identity"));
        }
        self.write(|tx| {
            sent(tx, a, id, attempt)?;
            let oid = merge_oid.map(CommitOid::as_str);
            let previous=tx.query_row("SELECT request_id,merge_oid FROM merge_acceptances WHERE account_id=? AND operation_id=? AND attempt=?",
                        params![a,id,attempt],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?))).optional()?;
            if previous.as_ref().is_some_and(|(rid, old)| {
                rid != request || old.as_deref().zip(oid).is_some_and(|(a, b)| a != b)
            }) {
                return Err(Error::Conflict(
                    "The accepted merge request already has another identity",
                ));
            }
            tx.execute("INSERT INTO merge_acceptances VALUES(?,?,?,?,?) ON CONFLICT(account_id,operation_id,attempt)
                        DO UPDATE SET merge_oid=COALESCE(merge_acceptances.merge_oid,excluded.merge_oid)",params![a,id,attempt,request,oid])?;
            dispatch::record_delivery_in(
                tx,
                a,
                id,
                attempt,
                Delivery::Waiting("A direct merge is pending authoritative confirmation"),
            )
        })
    }
    pub fn record_merge_failure(
        &self,
        a: AccountId,
        id: &str,
        attempt: i64,
        why: MergeFailure,
        now: i64,
    ) -> Result<()> {
        self.write(|tx| {
            sent(tx, a, id, attempt)?;
            let accepted:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM merge_acceptances WHERE account_id=? AND operation_id=? AND attempt=?)",
                        params![a,id,attempt],|r|r.get(0))?;
            if why == MergeFailure::Ambiguous {
                if accepted {
                    return Ok(());
                }
                return dispatch::record_delivery_in(
                    tx,
                    a,
                    id,
                    attempt,
                    Delivery::Unknown("The direct merge may have reached GitHub"),
                );
            }
            if accepted {
                return Err(Error::Conflict(
                    "An acknowledged merge cannot be retried on contradictory evidence",
                ));
            }
            let (reason, attention, floor) = match why {
                MergeFailure::DefinitelyUnsent => ("merge_transport_proved_unsent", false, now),
                MergeFailure::Throttled { not_before_ms } => {
                    crate::sync::server_floor_in(tx, a, not_before_ms)?;
                    ("direct_merge_throttled", false, not_before_ms)
                }
                MergeFailure::RequiredMergeQueue => ("required_merge_queue_unsupported", true, now),
                MergeFailure::ChangedHead => ("expected_merge_revision_changed", true, now),
                MergeFailure::PermissionDenied => ("merge_permission_denied", true, now),
                MergeFailure::RulesRejected => ("branch_rules_rejected_direct_merge", true, now),
                MergeFailure::InvalidInput => ("invalid_direct_merge", true, now),
                MergeFailure::Ambiguous => unreachable!(),
            };
            let delay = threadline_core::sync::policy::retry_delay(
                u32::try_from(attempt).unwrap_or(u32::MAX),
                std::time::Duration::ZERO,
            )
            .unwrap_or(std::time::Duration::from_secs(15 * 60));
            let retry = now
                .saturating_add(i64::try_from(delay.as_millis()).unwrap_or(i64::MAX))
                .max(floor);
            dispatch::record_delivery_in(
                tx,
                a,
                id,
                attempt,
                Delivery::NotApplied {
                    proof: reason,
                    retry_at: retry,
                },
            )?;
            if attention {
                tx.execute("UPDATE operations SET state='attention',reason=? WHERE account_id=? AND id=? AND state='queued' AND NOT cancel_requested",
                        params![reason,a,id])?;
            }
            Ok(())
        })
    }
    pub fn confirm_merge(
        &self,
        a: AccountId,
        id: &str,
        attempt: i64,
        proof: MergeProof<'_>,
        merged: &MergedPr,
        now: i64,
    ) -> Result<()> {
        self.write(|tx| {
            let (route, method) = sent(tx, a, id, attempt)?;
            if (
                merged.actor,
                merged.repository_id,
                merged.pr_id,
                merged.number,
                merged.expected_head.as_str(),
            ) != (
                route.actor,
                route.repository_id,
                route.pr_id,
                route.number,
                route.head.as_str(),
            ) {
                return Err(Error::Conflict(
                    "This merge result belongs to another PR, actor or expected head",
                ));
            }
            let accepted:Option<String>=tx.query_row("SELECT merge_oid FROM merge_acceptances WHERE account_id=? AND operation_id=? AND attempt=?",
                        params![a,id,attempt],|r|r.get(0)).optional()?.flatten();
            if accepted
                .as_deref()
                .is_some_and(|oid| oid != merged.merge_oid.as_str())
            {
                return Err(Error::Conflict(
                    "The merged commit differs from the acknowledged merge",
                ));
            }
            let (own, evidence) = match proof {
                MergeProof::MutationResponse { request_id }
                    if !request_id.trim().is_empty()
                        && merged.merged_by.is_none_or(|user| user == route.actor) =>
                {
                    (
                        true,
                        format!("Direct merge response to GitHub request {request_id}"),
                    )
                }
                MergeProof::MutationResponse { .. } => {
                    return Err(Error::InvalidInput(
                        "Missing or contradictory direct merge receipt",
                    ));
                }
                MergeProof::MergedPrRead => {
                    let target = MergeReadback {
                        operation_id: id.into(),
                        attempt,
                        route: route.clone(),
                        method,
                    };
                    verify_readback(tx, &target)?;
                    let attributed = accepted.is_some() && merged.merged_by == Some(route.actor);
                    (
                        attributed,
                        if attributed {
                            "Merged PR confirms the request's acknowledged commit and original actor".into()
                        } else {
                            "Merged PR confirms this expected head; origin of the merge is unproved".into()
                        },
                    )
                }
            };
            let receipt = Receipt {
                step: "direct_merge".into(),
                object_kind: "merged_commit".into(),
                remote_id: merged.merge_oid.as_str().into(),
                evidence,
            };
            dispatch::record_delivery_in(tx, a, id, attempt, Delivery::Confirmed(&receipt))?;
            if !own {
                tx.execute("UPDATE operations SET reason='merged_observed_origin_unproved' WHERE account_id=? AND id=?",params![a,id])?;
            }
            publish(tx, &route, merged)?;
            tx.execute(
                "INSERT INTO confirmed_merges VALUES(?,?,?,?,?,?,?,?)",
                params![
                    a,
                    id,
                    route.item,
                    route.repository,
                    route.head,
                    merged.merge_oid.as_str(),
                    own,
                    now
                ],
            )?;
            Ok(())
        })
    }
    pub fn confirmed_merge(&self, a: AccountId, id: &str) -> Result<Option<ConfirmedMerge>> {
        self.read(|db| {
            let row=db.query_row("SELECT item_id,repository_id,head,merge_oid,own_action,confirmed_at FROM confirmed_merges
                        WHERE account_id=? AND operation_id=?",params![a,id],
                        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get::<_,String>(3)?,r.get(4)?,r.get(5)?))).optional()?;
            row.map(
                |(item, repository, expected_head, oid, own_action, confirmed_at_ms)| {
                    Ok(ConfirmedMerge {
                        item,
                        repository,
                        expected_head,
                        merge_oid: CommitOid::parse(&oid)?,
                        own_action,
                        confirmed_at_ms,
                    })
                },
            )
            .transpose()
        })
    }
}

impl Store {
    pub fn fail_merge_preflight(
        &self,
        c: &MergeClaim,
        why: writes::WritePreflightFailure,
        now: i64,
    ) -> Result<()> {
        use writes::WritePreflightFailure as Failure;
        self.write(|tx| {
            let op = &c.claim.operation;
            if !eligible(tx, c, now)? || mutations::pr_route(tx, op.account, &op.id)? != c.route {
                return Err(Error::Conflict("The merge preflight changed or expired"));
            }
            let (state, reason, floor) = match why {
                Failure::Transient => ("queued", "merge_preflight_unreachable", now),
                Failure::Throttled { not_before_ms } => {
                    crate::sync::server_floor_in(tx, op.account, not_before_ms)?;
                    ("queued", "merge_preflight_throttled", not_before_ms)
                }
                Failure::Unauthorized | Failure::Forbidden => {
                    ("attention", "merge_permission_denied", now)
                }
                Failure::Missing => ("attention", "original_merge_target_unavailable", now),
                Failure::InvalidResponse => ("attention", "invalid_merge_preflight", now),
            };
            let delay = threadline_core::sync::policy::retry_delay(
                u32::try_from(c.attempt()).unwrap_or(u32::MAX),
                std::time::Duration::ZERO,
            )
            .unwrap_or(std::time::Duration::from_secs(15 * 60));
            let retry = now
                .saturating_add(i64::try_from(delay.as_millis()).unwrap_or(i64::MAX))
                .max(floor);
            tx.execute(
                "INSERT INTO merge_attempts VALUES(?,?,?,?,'null',?,?)",
                params![
                    op.account,
                    op.id,
                    c.attempt(),
                    encode(&c.route)?,
                    reason,
                    now
                ],
            )?;
            finish(
                tx,
                &c.claim,
                state,
                reason,
                if state == "queued" { retry } else { 0 },
            )?;
            Ok(())
        })
    }

    pub fn resume_merges(&self, a: AccountId, now: i64) -> Result<usize> {
        self.write(|tx| {
            let due = writes::deadline(tx, a)?.unwrap_or(now).max(now);
            Ok(tx.execute("UPDATE operations AS q SET state='queued',reason='merge_access_repaired',retry_at=?2
                        WHERE q.account_id=?1 AND q.state='attention' AND NOT q.cancel_requested AND q.reason='merge_permission_denied'
                        AND EXISTS(SELECT 1 FROM merge_attempts e JOIN github_operation_scopes scope
                            ON scope.account_id=e.account_id AND scope.operation_id=e.operation_id
                            JOIN github_access_profiles p ON p.account_id=scope.account_id AND p.owner_scope=scope.owner_scope
                            JOIN operation_gates g ON g.account_id=q.account_id AND g.id=q.id
                            WHERE e.account_id=q.account_id AND e.operation_id=q.id AND e.attempt=q.attempt
                              AND p.credential_generation>json_extract(e.route,'$.credential_generation') AND g.hold IS NULL)",
                        params![a,due])?)
        })
    }
}
