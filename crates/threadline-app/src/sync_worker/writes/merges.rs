use super::*;
use threadline_core::github::writer::merge::{
    DirectMergeFailure, DirectMergeRejection, MergeEligibility, MergeGate, MergeMethod as Method,
    QueueRequirement as Queue,
};
use threadline_storage::{
    merges::{
        Gate, MergeAdmission, MergeClaim, MergeFailure, MergeMethod, MergeObservation, MergePolicy,
        MergeProof, MergeReadback, MergeRoute, MergedPr, QueueRequirement,
    },
    resources::CommitOid,
};

pub(super) enum Job {
    Send(Box<MergeClaim>),
    Readback(Box<MergeReadback>),
}

pub(super) fn claim(
    store: &Store,
    account: AccountId,
    now: i64,
) -> threadline_storage::Result<Option<Job>> {
    if let Some(target) = store.claim_merge_readback(account, now, now.saturating_add(90_000))? {
        return Ok(Some(Job::Readback(Box::new(target))));
    }
    Ok(store
        .claim_merge(account, now, now.saturating_add(LEASE_MS))?
        .map(|claim| Job::Send(Box::new(claim))))
}

pub(super) async fn execute(dispatch: &Dispatch<'_>, job: Job) {
    match job {
        Job::Send(claim) => {
            if let Err(error) = run(dispatch, &claim).await {
                let (reachability, reason, rate) = evidence(&error);
                dispatch.record(reachability, reason, rate).await;
                let _ = blocking(dispatch.store, move |store| {
                    let now = UtcMillis::now()?.0;
                    let failure = preflight_failure(
                        &error,
                        now,
                        store.server_read_budget(claim.route.account)?,
                    );
                    if matches!(error, ReaderError::Cancelled)
                        || store.fail_merge_preflight(&claim, failure, now).is_err()
                    {
                        store.abandon_merge_preflight(&claim, now)?;
                    }
                    Ok(())
                })
                .await;
            }
        }
        Job::Readback(target) => {
            if let Err(error) = readback(dispatch, &target).await {
                let (reachability, reason, rate) = evidence(&error);
                dispatch.record(reachability, reason, rate).await;
                let _ = blocking(dispatch.store, move |store| {
                    let now = UtcMillis::now()?.0;
                    let due = now
                        .saturating_add(60_000)
                        .max(floor(rate, reason, now).unwrap_or(now));
                    store.defer_merge_readback(&target, due)
                })
                .await;
            }
        }
    }
}

async fn run(dispatch: &Dispatch<'_>, claim: &MergeClaim) -> Result<(), ReaderError> {
    let route = &claim.route;
    let saved = claim.clone();
    let admit = move |store: &Store, now| store.merge_profile(&saved, now);
    let profile = dispatch.budget(admit.clone()).await?;
    let (session, rate) = dispatch
        .accounts
        .reader_session_with_guidance(&profile, dispatch.cancel)
        .await?;
    dispatch
        .record(Reachability::ReceivedResponse, None, Some(rate))
        .await;
    dispatch.budget(admit.clone()).await?;
    let writer = bind(dispatch, &session, route).await?;
    dispatch.budget(admit.clone()).await?;
    let (pull, rate) = writer
        .observe_pull_for_review(
            route
                .number
                .get()
                .try_into()
                .map_err(|_| ReaderError::InvalidIdentity)?,
            route.pr_id,
            dispatch.cancel,
        )
        .await?;
    dispatch
        .record(Reachability::ReceivedResponse, None, Some(rate))
        .await;
    // Do not ask for policy on an unrelated comparison or retargeted branch.
    let exact = pull.head() == route.head
        && claim.expected_base_ref.as_deref().map_or_else(
            || pull.base() == route.base,
            |branch| pull.base_ref() == Some(branch),
        );
    let policy = if exact {
        dispatch.budget(admit.clone()).await?;
        let (observed, rate) = writer.observe_merge_policy(&pull, dispatch.cancel).await?;
        dispatch
            .record(Reachability::ReceivedResponse, None, Some(rate))
            .await;
        Some(observed)
    } else {
        None
    };
    let observation = MergeObservation {
        actor: writer.actor(),
        repository_id: route.repository_id,
        pr_id: route.pr_id,
        number: route.number,
        credential_generation: route.credential_generation,
        base: pull.base().to_owned(),
        base_ref: pull.base_ref().map(str::to_owned),
        head: pull.head().to_owned(),
        observed_at_ms: UtcMillis::now()?.0,
        policy: policy
            .as_ref()
            .map_or_else(unknown_policy, |policy| translate(policy.eligibility())),
    };
    if observation.policy.direct_hold(claim.method).is_none() {
        dispatch.budget(admit).await?;
    }
    let saved = claim.clone();
    let accepted = blocking(dispatch.store, move |store| {
        store.authorize_merge(&saved, &observation, UtcMillis::now()?.0)
    })
    .await?;
    let MergeAdmission::Send(ticket) = accepted else {
        return Ok(());
    };
    let policy = policy.ok_or(ReaderError::ScopeChanged)?;
    if dispatch.cancel.is_cancelled() {
        return Err(ReaderError::Cancelled);
    }
    let saved = claim.clone();
    if !blocking(dispatch.store, move |store| {
        let now = UtcMillis::now()?.0;
        if store.start_merge_transport(&ticket, now)? {
            return Ok(true);
        }
        store.abandon_merge_preflight(&saved, now)?;
        Ok(false)
    })
    .await?
    {
        return Ok(());
    }
    let outcome = writer
        .merge_at_head(
            &pull,
            &policy,
            method(claim.method),
            &route.head,
            dispatch.cancel,
        )
        .await;
    let (reachable, reason, rate) = match &outcome {
        Ok(receipt) => (Reachability::ReceivedResponse, None, Some(receipt.rate)),
        Err(error) => (
            error.transport.access.reachability,
            Some(error.transport.access.kind),
            error.transport.access.rate,
        ),
    };
    dispatch.record(reachable, reason, rate).await;
    let claim = claim.clone();
    let _ = blocking(dispatch.store, move |store| {
        let route = &claim.route;
        let now = UtcMillis::now()?.0;
        let operation = claim.operation_id();
        let attempt = claim.attempt();
        match outcome {
            Ok(receipt) => {
                let merge_oid = CommitOid::parse(&receipt.merge_oid)?;
                let confirmed = merged(route, merge_oid.clone(), None);
                if store
                    .confirm_merge(
                        route.account,
                        operation,
                        attempt,
                        MergeProof::MutationResponse {
                            request_id: &receipt.request_id,
                        },
                        &confirmed,
                        now,
                    )
                    .is_err()
                {
                    store.accept_merge(
                        route.account,
                        operation,
                        attempt,
                        &receipt.request_id,
                        Some(&merge_oid),
                    )?;
                }
            }
            Err(error) => {
                store.record_merge_failure(
                    route.account,
                    operation,
                    attempt,
                    failure(&error, now),
                    now,
                )?;
            }
        }
        Ok(())
    })
    .await;
    Ok(())
}

async fn readback(dispatch: &Dispatch<'_>, target: &MergeReadback) -> Result<(), ReaderError> {
    let saved = target.clone();
    let admit = move |store: &Store, _now| store.merge_readback_profile(&saved);
    let profile = dispatch.budget(admit.clone()).await?;
    let (session, rate) = dispatch
        .accounts
        .reader_session_with_guidance(&profile, dispatch.cancel)
        .await?;
    dispatch
        .record(Reachability::ReceivedResponse, None, Some(rate))
        .await;
    dispatch
        .admit_reconciliation(&profile, admit.clone())
        .await?;
    let route = &target.route;
    let writer = bind(dispatch, &session, route).await?;
    dispatch.admit_reconciliation(&profile, admit).await?;
    let (pull, rate) = writer
        .observe_pull_for_review(
            route
                .number
                .get()
                .try_into()
                .map_err(|_| ReaderError::InvalidIdentity)?,
            route.pr_id,
            dispatch.cancel,
        )
        .await?;
    dispatch
        .record(Reachability::ReceivedResponse, None, Some(rate))
        .await;
    if dispatch.cancel.is_cancelled() {
        return Err(ReaderError::Cancelled);
    }
    if !pull.is_merged() || pull.head() != route.head {
        return Err(ReaderError::ScopeChanged);
    }
    let oid = CommitOid::parse(pull.merge_oid().ok_or(ReaderError::InvalidIdentity)?)?;
    let observed = merged(route, oid, pull.merged_by());
    let target = target.clone();
    blocking(dispatch.store, move |store| {
        if store.merge_readback_profile(&target)? != profile {
            return Err(threadline_storage::Error::Conflict(
                "The merge readback credential changed",
            ));
        }
        store.confirm_merge(
            target.route.account,
            &target.operation_id,
            target.attempt,
            MergeProof::MergedPrRead,
            &observed,
            UtcMillis::now()?.0,
        )
    })
    .await?;
    Ok(())
}

async fn bind<'a>(
    dispatch: &Dispatch<'_>,
    session: &'a GitHubSession,
    route: &MergeRoute,
) -> Result<GitHubWriter<'a>, ReaderError> {
    let name =
        RepositoryName::new(&route.owner, &route.name).map_err(|_| ReaderError::InvalidIdentity)?;
    let id = route
        .repository_id
        .get()
        .try_into()
        .map_err(|_| ReaderError::InvalidIdentity)?;
    let (writer, rate) = session.issue_writer(&name, id, dispatch.cancel).await?;
    dispatch
        .record(Reachability::ReceivedResponse, None, Some(rate))
        .await;
    Ok(writer)
}

fn merged(
    route: &MergeRoute,
    merge_oid: CommitOid,
    merged_by: Option<std::num::NonZeroU64>,
) -> MergedPr {
    MergedPr {
        actor: route.actor,
        repository_id: route.repository_id,
        pr_id: route.pr_id,
        number: route.number,
        expected_head: route.head.clone(),
        merge_oid,
        merged_by,
        remote_updated_at_ms: None,
    }
}

fn failure(error: &DirectMergeFailure, now: i64) -> MergeFailure {
    match error.reason {
        DirectMergeRejection::HeadChanged => return MergeFailure::ChangedHead,
        DirectMergeRejection::NotPermitted => return MergeFailure::RulesRejected,
        DirectMergeRejection::Other => {}
    }
    let access = &error.transport.access;
    match error.transport.evidence {
        DispatchEvidence::DefinitelyUnsent => MergeFailure::DefinitelyUnsent,
        DispatchEvidence::AccessRejected => MergeFailure::PermissionDenied,
        DispatchEvidence::InvalidInput => MergeFailure::InvalidInput,
        DispatchEvidence::Throttled => MergeFailure::Throttled {
            not_before_ms: floor(access.rate, Some(access.kind), now)
                .unwrap_or_else(|| now.saturating_add(60_000)),
        },
        DispatchEvidence::Ambiguous => MergeFailure::Ambiguous,
    }
}

fn unknown_policy() -> MergePolicy {
    MergePolicy {
        allowed_methods: None,
        viewer_permission: Permission::Unknown,
        open: None,
        draft: None,
        merged: None,
        mergeable: None,
        queue: QueueRequirement::Unknown,
        checks: Gate::Unknown,
        reviews: Gate::Unknown,
        branch_rules: Gate::Unknown,
    }
}

pub(crate) fn translate(policy: &MergeEligibility) -> MergePolicy {
    let gate = |value| match value {
        MergeGate::Satisfied => Gate::Satisfied,
        MergeGate::Blocking => Gate::Blocking,
        MergeGate::Unknown => Gate::Unknown,
    };
    MergePolicy {
        allowed_methods: policy.methods.as_ref().map(|methods| {
            methods
                .iter()
                .map(|method| match method {
                    Method::Merge => MergeMethod::Merge,
                    Method::Squash => MergeMethod::Squash,
                    Method::Rebase => MergeMethod::Rebase,
                })
                .collect()
        }),
        viewer_permission: match policy.permission {
            WritePermission::Allowed => Permission::Allowed,
            WritePermission::Denied => Permission::Denied,
            WritePermission::Unknown => Permission::Unknown,
        },
        open: policy.open,
        draft: policy.draft,
        merged: policy.merged,
        mergeable: policy.mergeable,
        queue: match policy.queue {
            Queue::Required => QueueRequirement::Required,
            Queue::NotRequired => QueueRequirement::NotRequired,
            Queue::Unknown => QueueRequirement::Unknown,
        },
        checks: gate(policy.checks),
        reviews: gate(policy.reviews),
        branch_rules: gate(policy.branch_rules),
    }
}

fn method(value: MergeMethod) -> Method {
    match value {
        MergeMethod::Merge => Method::Merge,
        MergeMethod::Squash => Method::Squash,
        MergeMethod::Rebase => Method::Rebase,
    }
}
