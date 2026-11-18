use std::num::NonZeroU32;

use threadline_core::{
    domain::RepositoryName,
    github::writer::{
        ChoiceValues, DispatchEvidence, GitHubWriter, IssueChoices, IssueMutation, IssuePatch,
        IssueState, IssueWriteTarget, MutationObject, VerifiedComment, WritePermission,
    },
};
use threadline_storage::writes::{
    ConfirmationProof, ConfirmedObject, ConfirmedWrite, IssueChanges, Permission,
    ReconciliationTarget, RemoteComment, RepositoryChoices, WriteAdmission, WriteClaim,
    WriteFailure, WriteIntent, WriteObservation, WritePreflightFailure,
};

use crate::reader::write_values as values;

use super::*;

const LEASE_MS: i64 = 180_000;

pub(super) async fn execute(
    accounts: &AccountService,
    store: &Arc<Store>,
    core: &Shared,
    account: AccountId,
    generation: u64,
) -> bool {
    let Ok(Some(claim)) = blocking(store, move |store| {
        let now = UtcMillis::now()?.0;
        store.claim_write(account, now, now.saturating_add(LEASE_MS))
    })
    .await
    else {
        return false;
    };
    let cancel = core.start_read(generation, None);
    let dispatch = Dispatch {
        accounts,
        store,
        core,
        account,
        cancel: &cancel,
    };
    if let Err(error) = dispatch.run(&claim).await {
        let (reachability, reason, rate) = evidence(&error);
        dispatch.record(reachability, reason, rate).await;
        let _ = blocking(store, move |store| {
            let now = UtcMillis::now()?.0;
            let reason = preflight_failure(&error, now, store.server_read_budget(account)?);
            if matches!(error, ReaderError::Cancelled)
                || store.fail_write_preflight(&claim, reason, now).is_err()
            {
                store.abandon_write_preflight(&claim, now)?;
            }
            Ok(())
        })
        .await;
    }
    core.finish_read();
    true
}

pub(super) async fn reconcile(
    accounts: &AccountService,
    store: &Arc<Store>,
    core: &Shared,
    target: ReconciliationTarget,
    generation: u64,
) -> bool {
    let cancel = core.start_read(generation, None);
    let dispatch = Dispatch {
        accounts,
        store,
        core,
        account: target.route.account,
        cancel: &cancel,
    };
    let result = dispatch.reconcile(target).await;
    if let Err(error) = &result {
        let (reachability, reason, rate) = evidence(error);
        dispatch.record(reachability, reason, rate).await;
    }
    core.finish_read();
    result.is_ok()
}

struct Dispatch<'a> {
    accounts: &'a AccountService,
    store: &'a Arc<Store>,
    core: &'a Shared,
    account: AccountId,
    cancel: &'a Cancellation,
}

impl Dispatch<'_> {
    async fn run(&self, claim: &WriteClaim) -> Result<(), ReaderError> {
        let route = &claim.route;
        let profile = self.admit(claim).await?;
        let (session, rate) = self
            .accounts
            .reader_session_with_guidance(&profile, self.cancel)
            .await?;
        self.record(Reachability::ReceivedResponse, None, Some(rate))
            .await;
        let repo = RepositoryName::new(&route.owner, &route.repository_name)
            .map_err(|_| ReaderError::InvalidIdentity)?;
        self.admit(claim).await?;
        let (writer, rate) = session
            .issue_writer(
                &repo,
                route
                    .repository_id
                    .get()
                    .try_into()
                    .map_err(|_| ReaderError::InvalidIdentity)?,
                self.cancel,
            )
            .await?;
        self.record(Reachability::ReceivedResponse, None, Some(rate))
            .await;
        self.admit(claim).await?;
        let target = IssueWriteTarget {
            number: route
                .number
                .get()
                .try_into()
                .map_err(|_| ReaderError::InvalidIdentity)?,
            issue_id: route.issue_id,
            pull_request: route.pull_request,
        };
        let (issue, rate) = writer.observe_issue(target, self.cancel).await?;
        self.record(Reachability::ReceivedResponse, None, Some(rate))
            .await;

        let existing_comment = if let Some(id) = claim.comment_id {
            self.admit(claim).await?;
            let (comment, rate) = writer.observe_comment(&issue, id, self.cancel).await?;
            self.record(Reachability::ReceivedResponse, None, Some(rate))
                .await;
            Some(comment)
        } else {
            None
        };
        let patch = match &claim.intent {
            WriteIntent::EditIssue(fields) => Some(patch(fields)),
            _ => None,
        };
        let mutation = match &claim.intent {
            WriteIntent::EditIssue(_) => {
                IssueMutation::EditIssue(patch.as_ref().expect("issue patch"))
            }
            WriteIntent::CreateComment { body } => IssueMutation::CreateComment { body },
            WriteIntent::EditComment { body, .. } => IssueMutation::EditComment {
                comment: existing_comment
                    .as_ref()
                    .ok_or(ReaderError::InvalidIdentity)?,
                body: &body.wanted,
            },
        };
        let permission = match writer.permission(&issue, &mutation) {
            WritePermission::Allowed => Permission::Allowed,
            WritePermission::Denied => Permission::Denied,
            WritePermission::Unknown => Permission::Unknown,
        };
        let choices = if let (WriteIntent::EditIssue(fields), Permission::Allowed) =
            (&claim.intent, permission)
        {
            self.choices(claim, &writer, fields).await?
        } else {
            RepositoryChoices::default()
        };
        let observed = WriteObservation {
            credential_generation: route.credential_generation,
            actor: writer.actor(),
            repository: writer
                .repository()
                .get()
                .try_into()
                .map_err(|_| ReaderError::InvalidIdentity)?,
            issue: target.issue_id,
            number: route.number,
            pull_request: target.pull_request,
            observed_at_ms: UtcMillis::now()?.0,
            permission,
            fields: values(&issue.observed),
            comment: existing_comment.as_ref().map(remote_comment),
            choices,
        };
        if permission == Permission::Allowed {
            self.admit(claim).await?;
        }
        let leased = claim.clone();
        let admission = blocking(self.store, move |store| {
            store.authorize_write(&leased, &observed, UtcMillis::now()?.0)
        })
        .await?;
        let WriteAdmission::Send(ticket) = admission else {
            return Ok(());
        };
        let dispatch = ticket.clone();
        if self.cancel.is_cancelled() {
            return Err(ReaderError::Cancelled);
        }
        if !blocking(self.store, move |store| {
            store.start_write_transport(&dispatch, UtcMillis::now()?.0)
        })
        .await?
        {
            return Ok(());
        }

        let response = writer.mutate(&issue, mutation, self.cancel).await;
        let operation = claim.operation_id().to_owned();
        let attempt = claim.attempt();
        let account = self.account;
        let route = route.clone();
        let (reachable, reason, rate) = match &response {
            Ok(receipt) => (Reachability::ReceivedResponse, None, Some(receipt.rate)),
            Err(error) => (
                error.access.reachability,
                Some(error.access.kind),
                error.access.rate,
            ),
        };
        self.record(reachable, reason, rate).await;
        // After the send boundary every failure must leave explicit delivery evidence when storage allows it.
        let _ = blocking(self.store, move |store| {
            let now = UtcMillis::now()?.0;
            match response {
                Ok(response) => {
                    let object = match response.object {
                        MutationObject::Issue(issue) => ConfirmedObject::Issue {
                            fields: values(&issue.observed),
                            milestone_title: issue.milestone_title,
                            updated_at_ms: issue.updated_at_ms,
                        },
                        MutationObject::Comment(comment) => {
                            ConfirmedObject::Comment(remote_comment(&comment))
                        }
                    };
                    let confirmed = ConfirmedWrite {
                        actor: route.actor,
                        repository: route.repository_id,
                        issue: route.issue_id,
                        number: route.number,
                        object,
                    };
                    if store
                        .confirm_write(
                            account,
                            &operation,
                            attempt,
                            ConfirmationProof::MutationResponse {
                                request_id: &response.request_id,
                            },
                            &confirmed,
                            now,
                        )
                        .is_err()
                    {
                        if let ConfirmedObject::Comment(comment) = &confirmed.object
                            && matches!(ticket.claim().intent, WriteIntent::CreateComment { .. })
                            && store
                                .accept_created_comment(&ticket, comment.id, &response.request_id)
                                .is_ok()
                        {
                            return Ok(());
                        }
                        store.record_write_failure(
                            account,
                            &operation,
                            attempt,
                            WriteFailure::Ambiguous,
                            now,
                        )?;
                    }
                }
                Err(error) => {
                    let failure = match error.evidence {
                        DispatchEvidence::DefinitelyUnsent => WriteFailure::DefinitelyUnsent,
                        DispatchEvidence::AccessRejected => WriteFailure::AccessRejected,
                        DispatchEvidence::InvalidInput => WriteFailure::InvalidInput,
                        DispatchEvidence::Throttled => WriteFailure::Throttled {
                            not_before_ms: floor(rate, reason, now)
                                .unwrap_or_else(|| now.saturating_add(60_000)),
                        },
                        DispatchEvidence::Ambiguous => WriteFailure::Ambiguous,
                    };
                    store.record_write_failure(account, &operation, attempt, failure, now)?;
                }
            }
            Ok(())
        })
        .await;
        Ok(())
    }

    async fn admit(&self, claim: &WriteClaim) -> Result<AccessProfile, ReaderError> {
        let claim = claim.clone();
        self.budget(move |store, now| store.write_profile(&claim, now))
            .await
    }

    async fn budget(
        &self,
        profile: impl Fn(&Store, i64) -> threadline_storage::Result<AccessProfile>
        + Clone
        + Send
        + 'static,
    ) -> Result<AccessProfile, ReaderError> {
        while !self.cancel.is_cancelled() {
            let profile = profile.clone();
            let account = self.account;
            let (now, budget, profile) = blocking(self.store, move |store| {
                let now = UtcMillis::now()?.0;
                let profile = profile(store, now)?;
                let budget = store.reserve_read_budget(account, now)?;
                Ok((now, budget, profile))
            })
            .await?;
            match budget {
                ReadBudget::Granted => return Ok(profile),
                ReadBudget::Deferred { server: true, .. } => {
                    return Err(ReaderError::ServerBackoff);
                }
                ReadBudget::Deferred { until_ms, .. } => tokio::select! {
                    biased;
                    _=self.cancel.cancelled()=>break,
                    _=tokio::time::sleep(until(now,until_ms))=>{},
                },
            }
        }
        Err(ReaderError::Cancelled)
    }

    async fn reconcile(&self, target: ReconciliationTarget) -> Result<(), ReaderError> {
        let accepted = target.clone();
        let profile = move |store: &Store, _now| store.reconciliation_profile(&accepted);
        let original = self.budget(profile.clone()).await?;
        let (session, rate) = self
            .accounts
            .reader_session_with_guidance(&original, self.cancel)
            .await?;
        self.record(Reachability::ReceivedResponse, None, Some(rate))
            .await;
        let route = &target.route;
        let repo = RepositoryName::new(&route.owner, &route.repository_name)
            .map_err(|_| ReaderError::InvalidIdentity)?;
        self.admit_reconciliation(&original, profile.clone())
            .await?;
        let provider_id = route
            .repository_id
            .get()
            .try_into()
            .map_err(|_| ReaderError::InvalidIdentity)?;
        let (writer, rate) = session
            .issue_writer(&repo, provider_id, self.cancel)
            .await?;
        self.record(Reachability::ReceivedResponse, None, Some(rate))
            .await;
        self.admit_reconciliation(&original, profile.clone())
            .await?;
        let (issue, rate) = writer
            .observe_issue(
                IssueWriteTarget {
                    number: route
                        .number
                        .get()
                        .try_into()
                        .map_err(|_| ReaderError::InvalidIdentity)?,
                    issue_id: route.issue_id,
                    pull_request: route.pull_request,
                },
                self.cancel,
            )
            .await?;
        self.record(Reachability::ReceivedResponse, None, Some(rate))
            .await;
        self.admit_reconciliation(&original, profile).await?;
        let (comment, rate) = writer
            .observe_comment(&issue, target.comment_id, self.cancel)
            .await?;
        self.record(Reachability::ReceivedResponse, None, Some(rate))
            .await;
        if self.cancel.is_cancelled() {
            return Err(ReaderError::Cancelled);
        }
        let confirmed = ConfirmedWrite {
            actor: writer.actor(),
            repository: route.repository_id,
            issue: route.issue_id,
            number: route.number,
            object: ConfirmedObject::Comment(remote_comment(&comment)),
        };
        blocking(self.store, move |store| {
            if store.reconciliation_profile(&target)? != original {
                return Err(threadline_storage::Error::Conflict(
                    "The reconciliation credential changed",
                ));
            }
            store.confirm_write(
                target.route.account,
                &target.operation_id,
                target.attempt,
                ConfirmationProof::AcceptedCommentRead,
                &confirmed,
                UtcMillis::now()?.0,
            )
        })
        .await?;
        Ok(())
    }

    async fn admit_reconciliation(
        &self,
        original: &AccessProfile,
        profile: impl Fn(&Store, i64) -> threadline_storage::Result<AccessProfile>
        + Clone
        + Send
        + 'static,
    ) -> Result<(), ReaderError> {
        if &self.budget(profile).await? != original {
            return Err(ReaderError::ScopeChanged);
        }
        Ok(())
    }

    async fn record(
        &self,
        reachability: Reachability,
        reason: Option<AccessKind>,
        rate: Option<RateLimit>,
    ) {
        if !self.cancel.is_cancelled() {
            let id = self
                .core
                .observer
                .lock()
                .expect("connectivity observer")
                .begin_request();
            if let Some(id) = id {
                self.core.record(id, reachability, reason);
            }
        }
        hold(self.store, self.account, rate, reason).await;
    }

    async fn choices(
        &self,
        claim: &WriteClaim,
        writer: &GitHubWriter<'_>,
        fields: &IssueChanges,
    ) -> Result<RepositoryChoices, ReaderError> {
        let mut choices = RepositoryChoices::default();
        let label_needed = fields
            .labels
            .as_ref()
            .is_some_and(|e| !e.wanted.is_subset(&e.base));
        let assignee_needed = fields
            .assignees
            .as_ref()
            .is_some_and(|e| !e.wanted.is_subset(&e.base));
        let milestone_needed = fields
            .milestone
            .as_ref()
            .is_some_and(|e| e.wanted.is_some() && e.wanted != e.base);
        for kind in [
            label_needed.then_some(IssueChoices::Labels),
            assignee_needed.then_some(IssueChoices::Assignees),
            milestone_needed.then_some(IssueChoices::Milestones),
        ]
        .into_iter()
        .flatten()
        {
            let mut page = NonZeroU32::MIN;
            for _ in 0..5 {
                self.admit(claim).await?;
                let response = writer.issue_choices(kind, page, self.cancel).await?;
                self.record(Reachability::ReceivedResponse, None, Some(response.rate))
                    .await;
                match response.values {
                    ChoiceValues::Labels(rows) => {
                        choices.labels.get_or_insert_default().extend(rows)
                    }
                    ChoiceValues::Assignees(rows) => {
                        choices.assignees.get_or_insert_default().extend(rows)
                    }
                    ChoiceValues::Milestones(rows) => choices
                        .milestones
                        .get_or_insert_default()
                        .extend(rows.into_keys()),
                }
                let Some(next) = response.next else {
                    break;
                };
                page = next;
            }
        }
        Ok(choices)
    }
}

fn patch(fields: &IssueChanges) -> IssuePatch {
    IssuePatch {
        title: fields.title.as_ref().map(|v| v.wanted.clone()),
        body: fields.body.as_ref().map(|v| v.wanted.clone()),
        labels: fields.labels.as_ref().map(|v| v.wanted.clone()),
        assignees: fields.assignees.as_ref().map(|v| v.wanted.clone()),
        milestone: fields.milestone.as_ref().map(|v| v.wanted),
        state: fields.closed.as_ref().map(|v| {
            if v.wanted {
                IssueState::Closed
            } else {
                IssueState::Open
            }
        }),
    }
}

fn remote_comment(comment: &VerifiedComment) -> RemoteComment {
    RemoteComment {
        id: comment.id(),
        author_id: comment.author_id(),
        author: comment.author.clone(),
        body: comment.body.clone(),
        created_at_ms: comment.created_at_ms,
        updated_at_ms: comment.updated_at_ms,
    }
}

fn preflight_failure(error: &ReaderError, now: i64, server: Option<i64>) -> WritePreflightFailure {
    use WritePreflightFailure as Failure;
    match error {
        ReaderError::Remote(error) => match error.kind {
            AccessKind::RateLimited => Failure::Throttled {
                not_before_ms: floor(error.rate, Some(error.kind), now)
                    .unwrap_or_else(|| now.saturating_add(60_000)),
            },
            AccessKind::Unauthorized | AccessKind::InvalidCredential => Failure::Unauthorized,
            AccessKind::PermissionDenied
            | AccessKind::SsoRequired
            | AccessKind::ApprovalPending => Failure::Forbidden,
            AccessKind::MissingOrInaccessible
            | AccessKind::IdentityMismatch
            | AccessKind::WrongScope => Failure::Missing,
            AccessKind::MalformedResponse
            | AccessKind::RedirectRejected
            | AccessKind::ResponseTooLarge => Failure::InvalidResponse,
            _ => Failure::Transient,
        },
        ReaderError::CredentialUnavailable | ReaderError::ProtectedStore(_) => {
            Failure::Unauthorized
        }
        ReaderError::InvalidIdentity | ReaderError::ScopeChanged => Failure::Missing,
        ReaderError::ServerBackoff => Failure::Throttled {
            not_before_ms: server.unwrap_or_else(|| now.saturating_add(60_000)),
        },
        _ => Failure::Transient,
    }
}
