use super::*;
use threadline_core::{
    github::writer::review::{
        InlineReview, ObservedReview, ObservedThread, PendingReview, ReviewEvent as Event,
        ReviewMutation, ReviewObject, ReviewSide, ReviewState, VerifiedPull,
    },
    sync::policy::retry_delay,
};
use threadline_storage::{
    pr::SourceSide,
    reviews::{
        RemotePending, ReviewAdmission, ReviewClaim, ReviewEvent, ReviewFailure, ReviewObservation,
        ReviewProof, ReviewReadback, ReviewResponse, ReviewResult, ReviewRoute, ReviewStep,
        ThreadObservation,
    },
};

pub(super) enum Job {
    Send(Box<ReviewClaim>),
    Readback(Box<ReviewReadback>),
}

pub(super) fn claim(
    store: &Store,
    account: AccountId,
    now: i64,
) -> threadline_storage::Result<Option<Job>> {
    if let Some(target) = store.claim_review_readback(account, now, now.saturating_add(90_000))? {
        return Ok(Some(Job::Readback(Box::new(target))));
    }
    Ok(store
        .claim_review(account, now, now.saturating_add(LEASE_MS))?
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
                    let account = claim.route.account;
                    let failure =
                        preflight_failure(&error, now, store.server_read_budget(account)?);
                    if matches!(error, ReaderError::Cancelled)
                        || store.fail_review_preflight(&claim, failure, now).is_err()
                    {
                        store.abandon_review_preflight(&claim, now)?;
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
                    let next = now
                        .saturating_add(60_000)
                        .max(floor(rate, reason, now).unwrap_or(now));
                    store.defer_review_readback(&target, next)
                })
                .await;
            }
        }
    }
}

enum Facts {
    Pending(PendingReview),
    Known(ObservedReview),
    Thread(ObservedThread),
    Changed,
}

async fn run(dispatch: &Dispatch<'_>, claim: &ReviewClaim) -> Result<(), ReaderError> {
    let route = &claim.route;
    let leased = claim.clone();
    let admit = move |store: &Store, now| store.review_profile(&leased, now);
    let profile = dispatch.budget(admit.clone()).await?;
    let (session, rate) = dispatch
        .accounts
        .reader_session_with_guidance(&profile, dispatch.cancel)
        .await?;
    dispatch
        .record(Reachability::ReceivedResponse, None, Some(rate))
        .await;
    let repo =
        RepositoryName::new(&route.owner, &route.name).map_err(|_| ReaderError::InvalidIdentity)?;
    dispatch.budget(admit.clone()).await?;
    let (writer, rate) = session
        .issue_writer(
            &repo,
            route
                .repository_id
                .get()
                .try_into()
                .map_err(|_| ReaderError::InvalidIdentity)?,
            dispatch.cancel,
        )
        .await?;
    dispatch
        .record(Reachability::ReceivedResponse, None, Some(rate))
        .await;
    dispatch.budget(admit.clone()).await?;
    let pull = observe_pull(dispatch, &writer, route).await?;
    let desired_event = match &claim.step {
        ReviewStep::CreatePending(draft) => Some(event(draft.event)),
        ReviewStep::SubmitPending { event: action, .. } => Some(event(*action)),
        _ => None,
    };
    let mut permission = writer.review_permission(&pull, desired_event);
    let unchanged = pull.base() == route.base && pull.head() == route.head;
    let facts = if !unchanged || !pull.is_open() {
        Facts::Changed
    } else {
        match &claim.step {
            ReviewStep::CreatePending(_) if permission == WritePermission::Allowed => {
                let mut page = NonZeroU32::MIN;
                let mut observed = PendingReview::Unknown;
                for _ in 0..5 {
                    dispatch.budget(admit.clone()).await?;
                    let ((pending, next), rate) = writer
                        .observe_own_pending(&pull, page, dispatch.cancel)
                        .await?;
                    dispatch
                        .record(Reachability::ReceivedResponse, None, Some(rate))
                        .await;
                    match (pending, next) {
                        (PendingReview::Absent, Some(next)) => page = next,
                        (last, _) => {
                            observed = last;
                            break;
                        }
                    }
                }
                Facts::Pending(observed)
            }
            ReviewStep::SubmitPending { review_id, .. }
                if permission == WritePermission::Allowed =>
            {
                dispatch.budget(admit.clone()).await?;
                let (observed, rate) = writer
                    .observe_review_object(&pull, *review_id, dispatch.cancel)
                    .await?;
                dispatch
                    .record(Reachability::ReceivedResponse, None, Some(rate))
                    .await;
                Facts::Known(observed)
            }
            ReviewStep::Reply { thread_id, .. }
            | ReviewStep::SetThreadResolved { thread_id, .. } => {
                dispatch.budget(admit.clone()).await?;
                let (thread, rate) = writer
                    .observe_review_thread(&pull, thread_id, dispatch.cancel)
                    .await?;
                dispatch
                    .record(Reachability::ReceivedResponse, None, Some(rate))
                    .await;
                if let ReviewStep::SetThreadResolved { resolved, .. } = &claim.step {
                    permission = thread.permission(*resolved);
                }
                Facts::Thread(thread)
            }
            _ => Facts::Changed,
        }
    };
    let pending = match &facts {
        Facts::Pending(PendingReview::Absent) => RemotePending::Absent,
        Facts::Pending(PendingReview::Present(review)) | Facts::Known(review)
            if review.state() == ReviewState::Pending =>
        {
            RemotePending::Present {
                id: review.id(),
                actor: review.author(),
                head: review.head().to_owned(),
            }
        }
        _ => RemotePending::Unknown,
    };
    let thread = match &facts {
        Facts::Thread(thread) => Some(ThreadObservation {
            id: thread.id().to_owned(),
            root_comment_id: thread.root(),
            outdated: thread.outdated(),
            resolved: thread.resolved(),
        }),
        _ => None,
    };
    let observation = ReviewObservation {
        actor: writer.actor(),
        repository_id: route.repository_id,
        pr_id: route.pr_id,
        number: route.number,
        base: pull.base().to_owned(),
        head: pull.head().to_owned(),
        credential_generation: route.credential_generation,
        observed_at_ms: UtcMillis::now()?.0,
        pr_open: Some(pull.is_open()),
        pr_author: pull.author_id(),
        pending,
        thread,
        permission: match permission {
            WritePermission::Allowed => Permission::Allowed,
            WritePermission::Denied => Permission::Denied,
            WritePermission::Unknown => Permission::Unknown,
        },
    };
    let inline = match &claim.step {
        ReviewStep::CreatePending(draft) => draft
            .comments
            .iter()
            .map(|comment| {
                let anchor = &comment.anchor;
                let side = match anchor.side {
                    SourceSide::Left => ReviewSide::Left,
                    SourceSide::Right => ReviewSide::Right,
                };
                let range = anchor.first_line != anchor.last_line;
                InlineReview {
                    path: &anchor.path,
                    body: &comment.body,
                    side,
                    line: anchor.last_line,
                    start_line: range.then_some(anchor.first_line),
                    start_side: range.then_some(side),
                }
            })
            .collect(),
        _ => Vec::new(),
    };
    let correlation = format!("{}:{}", claim.operation_id(), claim.attempt());
    let mutation = match (&claim.step, &facts) {
        (ReviewStep::CreatePending(draft), Facts::Pending(pending)) => {
            Some(ReviewMutation::CreatePending {
                pending,
                summary: &draft.summary,
                comments: &inline,
            })
        }
        (
            ReviewStep::SubmitPending {
                event: action,
                body,
                ..
            },
            Facts::Known(review),
        ) => Some(ReviewMutation::SubmitPending {
            review,
            event: event(*action),
            body,
        }),
        (
            ReviewStep::Reply {
                root_comment_id,
                body,
                ..
            },
            Facts::Thread(thread),
        ) => Some(ReviewMutation::Reply {
            thread,
            root: *root_comment_id,
            body,
        }),
        (ReviewStep::SetThreadResolved { resolved, .. }, Facts::Thread(thread)) => {
            Some(ReviewMutation::SetThreadResolved {
                thread,
                resolved: *resolved,
                correlation: &correlation,
            })
        }
        _ => None,
    };
    if mutation.is_some() && permission == WritePermission::Allowed {
        dispatch.budget(admit).await?;
    }
    let leased = claim.clone();
    let approved = blocking(dispatch.store, move |store| {
        store.authorize_review(&leased, &observation, UtcMillis::now()?.0)
    })
    .await?;
    let ReviewAdmission::Send(ticket) = approved else {
        return Ok(());
    };
    let mutation = mutation.ok_or(ReaderError::ScopeChanged)?;
    if dispatch.cancel.is_cancelled() {
        return Err(ReaderError::Cancelled);
    }
    let start = blocking(dispatch.store, move |store| {
        store.start_review_transport(&ticket, UtcMillis::now()?.0)
    })
    .await?;
    if !start {
        return Ok(());
    }

    let response = writer.mutate_review(&pull, mutation, dispatch.cancel).await;
    let (reachable, reason, rate) = match &response {
        Ok(response) => (Reachability::ReceivedResponse, None, Some(response.rate)),
        Err(error) => (
            error.access.reachability,
            Some(error.access.kind),
            error.access.rate,
        ),
    };
    dispatch.record(reachable, reason, rate).await;
    let current = claim.clone();
    let _ = blocking(dispatch.store, move |store| {
        let route = &current.route;
        let account = route.account;
        let operation = current.operation_id();
        let attempt = current.attempt();
        let now = UtcMillis::now()?.0;
        match response {
            Ok(response) => {
                let accepted = match (&current.step, &response.object) {
                    (ReviewStep::CreatePending(_), ReviewObject::Review(review)) => {
                        Some(review.id())
                    }
                    (ReviewStep::Reply { .. }, ReviewObject::Reply(reply)) => Some(reply.id),
                    _ => None,
                };
                let result = review_result(response.object).ok_or(
                    threadline_storage::Error::Conflict("Unconfirmed review outcome"),
                );
                if let Ok(result) = result
                    && store
                        .confirm_review_step(
                            account,
                            operation,
                            attempt,
                            ReviewProof::MutationResponse {
                                request_id: &response.request_id,
                            },
                            &review_response(route, result),
                            now,
                        )
                        .is_ok()
                {
                    return Ok(());
                }
                if let Some(accepted) = accepted
                    && store
                        .accept_review_object(
                            account,
                            operation,
                            attempt,
                            accepted,
                            &response.request_id,
                        )
                        .is_ok()
                {
                    return Ok(());
                }
                store.record_review_failure(
                    account,
                    operation,
                    attempt,
                    ReviewFailure::Ambiguous,
                    now,
                )?;
            }
            Err(error) => {
                let failure = match error.evidence {
                    DispatchEvidence::DefinitelyUnsent => ReviewFailure::Unsent {
                        retry_at_ms: now.saturating_add(
                            retry_delay(attempt.try_into().unwrap_or(u32::MAX), Duration::ZERO)
                                .unwrap_or(Duration::from_secs(900))
                                .as_millis() as i64,
                        ),
                    },
                    DispatchEvidence::Throttled => ReviewFailure::Throttled {
                        not_before_ms: floor(rate, reason, now)
                            .unwrap_or_else(|| now.saturating_add(60_000)),
                    },
                    DispatchEvidence::AccessRejected => ReviewFailure::Rejected {
                        reason: "review_access_rejected",
                    },
                    DispatchEvidence::InvalidInput => ReviewFailure::Rejected {
                        reason: "review_invalid_input",
                    },
                    DispatchEvidence::Ambiguous => ReviewFailure::Ambiguous,
                };
                store.record_review_failure(account, operation, attempt, failure, now)?;
            }
        }
        Ok(())
    })
    .await;
    Ok(())
}

async fn observe_pull(
    dispatch: &Dispatch<'_>,
    writer: &GitHubWriter<'_>,
    route: &ReviewRoute,
) -> Result<VerifiedPull, ReaderError> {
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
    Ok(pull)
}

async fn readback(dispatch: &Dispatch<'_>, target: &ReviewReadback) -> Result<(), ReaderError> {
    let saved = target.clone();
    let admit = move |store: &Store, _now| store.review_readback_profile(&saved);
    let profile = dispatch.budget(admit.clone()).await?;
    let (session, rate) = dispatch
        .accounts
        .reader_session_with_guidance(&profile, dispatch.cancel)
        .await?;
    dispatch
        .record(Reachability::ReceivedResponse, None, Some(rate))
        .await;
    let route = &target.route;
    dispatch
        .admit_reconciliation(&profile, admit.clone())
        .await?;
    let name =
        RepositoryName::new(&route.owner, &route.name).map_err(|_| ReaderError::InvalidIdentity)?;
    let (writer, rate) = session
        .issue_writer(
            &name,
            route
                .repository_id
                .get()
                .try_into()
                .map_err(|_| ReaderError::InvalidIdentity)?,
            dispatch.cancel,
        )
        .await?;
    dispatch
        .record(Reachability::ReceivedResponse, None, Some(rate))
        .await;
    dispatch
        .admit_reconciliation(&profile, admit.clone())
        .await?;
    let pull = observe_pull(dispatch, &writer, route).await?;
    dispatch.admit_reconciliation(&profile, admit).await?;
    let (object, rate) = if matches!(target.step, ReviewStep::Reply { .. }) {
        let (reply, rate) = writer
            .observe_review_reply(&pull, target.remote_id, dispatch.cancel)
            .await?;
        (ReviewObject::Reply(reply), rate)
    } else {
        let (review, rate) = writer
            .observe_review_object(&pull, target.remote_id, dispatch.cancel)
            .await?;
        (ReviewObject::Review(review), rate)
    };
    dispatch
        .record(Reachability::ReceivedResponse, None, Some(rate))
        .await;
    if dispatch.cancel.is_cancelled() {
        return Err(ReaderError::Cancelled);
    }
    let result = review_result(object).ok_or(ReaderError::InvalidIdentity)?;
    let target = target.clone();
    blocking(dispatch.store, move |store| {
        if store.review_readback_profile(&target)? != profile {
            return Err(threadline_storage::Error::Conflict(
                "Review readback credential changed",
            ));
        }
        store.confirm_review_step(
            target.route.account,
            &target.operation_id,
            target.attempt,
            ReviewProof::AcceptedObjectRead,
            &review_response(&target.route, result),
            UtcMillis::now()?.0,
        )
    })
    .await?;
    Ok(())
}

fn event(value: ReviewEvent) -> Event {
    match value {
        ReviewEvent::Comment => Event::Comment,
        ReviewEvent::Approve => Event::Approve,
        ReviewEvent::RequestChanges => Event::RequestChanges,
    }
}

fn review_response(route: &ReviewRoute, result: ReviewResult) -> ReviewResponse {
    ReviewResponse {
        actor: route.actor,
        repository_id: route.repository_id,
        pr_id: route.pr_id,
        number: route.number,
        result,
    }
}

fn review_result(value: ReviewObject) -> Option<ReviewResult> {
    Some(match value {
        ReviewObject::Review(review) => match review.state() {
            ReviewState::Pending => ReviewResult::Pending {
                id: review.id(),
                author: review.author(),
                head: review.head().to_owned(),
            },
            ReviewState::Submitted(event) => ReviewResult::Submitted {
                id: review.id(),
                author: review.author(),
                head: review.head().to_owned(),
                event: match event {
                    Event::Comment => ReviewEvent::Comment,
                    Event::Approve => ReviewEvent::Approve,
                    Event::RequestChanges => ReviewEvent::RequestChanges,
                },
            },
            ReviewState::Other => return None,
        },
        ReviewObject::Reply(reply) => ReviewResult::Reply {
            id: reply.id,
            author: reply.author,
            root_comment_id: reply.root,
            body: reply.body,
        },
        ReviewObject::Thread { id, resolved } => ReviewResult::Thread { id, resolved },
    })
}
