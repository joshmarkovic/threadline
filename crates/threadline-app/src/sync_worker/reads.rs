use super::*;
mod pr;
use threadline_core::{
    domain::ItemNumber,
    github::reader::{
        ConversationComment, GitHubReader, IssueSummary, Page,
        refresh::{ReadValidators, Refresh, UpdatedComment},
    },
};
use threadline_storage::{
    Error, ItemId,
    reader::{DiscussionComment, ListSummary},
    resources::{ResourceFamily, Validators},
    sync::{
        ObservationPage, ObservedComment, ObservedItem, ReadClaim, ReadCompletion, ReadFailure,
    },
};

/// An authentication, repository verification, or resource page each occupies its own claim.
pub(super) async fn execute(
    accounts: &AccountService,
    store: &Arc<Store>,
    core: &Shared,
    bound: &mut BoundReaders,
    local: &Local,
    claim: ReadClaim,
    generation: u64,
) -> bool {
    let cancel = core.start_read(
        generation,
        Some((claim.account, claim.target.clone(), claim.generation)),
    );
    let id = core
        .observer
        .lock()
        .expect("connectivity observer")
        .begin_request();
    let result = if cancel.is_cancelled() {
        Err(ReaderError::Cancelled)
    } else {
        read(accounts, store, bound, &claim, &cancel).await
    };
    let result = if cancel.is_cancelled() {
        Err(ReaderError::Cancelled)
    } else {
        result
    };
    let Ok(now) = UtcMillis::now() else {
        core.finish_read();
        return false;
    };
    let changed = match result {
        Ok((outcome, rate)) => {
            if let Some(id) = id {
                core.record(id, Reachability::ReceivedResponse, None);
            }
            hold(store, claim.account, Some(rate), None).await;
            let system_zone = zone(&local.settings);
            let committed = blocking(store, move |store| {
                let changed = match &outcome {
                    Outcome::Feed { .. } | Outcome::Metadata { .. } => true,
                    Outcome::Pr(page) => page.changed(),
                    _ => false,
                };
                let write = match outcome {
                    Outcome::Prepared => {
                        store.fail_sync_read(&claim, ReadFailure::Cancelled, now.0, None)
                    }
                    Outcome::Unmodified(validators) => store.finish_sync_read(
                        &claim,
                        ReadCompletion::NotModified(validators),
                        now.0,
                        &system_zone?,
                    ),
                    Outcome::Metadata {
                        summary,
                        validators,
                    } => store
                        .commit_sync_item_metadata(
                            &claim,
                            *summary,
                            &validators,
                            now.0,
                            &system_zone?,
                        )
                        .map(|_| ()),
                    Outcome::Pr(page) => page.commit(store, &claim, now.0, &system_zone?),
                    Outcome::Feed {
                        items,
                        comments,
                        next,
                        validators,
                        limited,
                    } => {
                        let page = ObservationPage {
                            items,
                            comments,
                            next_cursor: next.as_deref(),
                            validators,
                        };
                        if limited {
                            store.commit_observed_partial(&claim, page, now.0)
                        } else {
                            store.commit_observed_page(&claim, page, now.0, &system_zone?)
                        }
                    }
                };
                write?;
                Ok(changed)
            })
            .await;
            committed.unwrap_or(false)
        }
        Err(error) => {
            let (reachability, reason, rate) = evidence(&error);
            if let Some(id) = id {
                core.record(id, reachability, reason);
            }
            let failure = failure(&error, &claim);
            let floor = floor(rate, reason, now.0);
            if matches!(reason, Some(AccessKind::Unauthorized)) {
                bound.clear_repository(claim.target.repository);
            }
            let _ = blocking(store, move |store| {
                store.fail_sync_read(&claim, failure, now.0, floor)
            })
            .await;
            false
        }
    };
    core.finish_read();
    changed
}

enum Outcome {
    Prepared,
    Unmodified(Validators),
    Pr(Box<pr::ClaimedPage>),
    Metadata {
        summary: Box<ListSummary>,
        validators: Validators,
    },
    Feed {
        items: Vec<ObservedItem>,
        comments: Vec<ObservedComment>,
        next: Option<String>,
        validators: Validators,
        limited: bool,
    },
}

async fn read(
    accounts: &AccountService,
    store: &Arc<Store>,
    bound: &mut BoundReaders,
    claim: &ReadClaim,
    cancel: &Cancellation,
) -> Result<(Outcome, RateLimit), ReaderError> {
    if let Some(binding) = bound.repositories.get_mut(&claim.target.repository)
        && binding.generation == claim.credential_generation
    {
        if claim.target.revision.starts_with("pr:") {
            return pr::read(store, binding, claim, cancel).await;
        }
        return read_bound(store, binding.reader.reader(), claim, cancel).await;
    }
    let repository = claim.target.repository;
    let identity = claim.clone();
    let entry = blocking(store, move |store| {
        store.read_sync_repository(&identity, UtcMillis::now()?.0)
    })
    .await?;
    let (route, provider_id, profile) = entry;
    let rate = if let Some(session) = bound.sessions.get(&profile.credential_ref) {
        let (reader, rate) = session.bind_reader(&route, provider_id, cancel).await?;
        bound.repositories.insert(
            repository,
            Bound {
                generation: claim.credential_generation,
                credential: profile.credential_ref.clone(),
                reader,
                pr_snapshots: HashMap::new(),
            },
        );
        rate
    } else {
        let (session, rate) = accounts
            .reader_session_with_guidance(&profile, cancel)
            .await?;
        bound
            .sessions
            .insert(profile.credential_ref.clone(), Arc::new(session));
        rate
    };
    Ok((Outcome::Prepared, rate))
}

async fn read_bound(
    store: &Arc<Store>,
    reader: GitHubReader<'_>,
    claim: &ReadClaim,
    cancel: &Cancellation,
) -> Result<(Outcome, RateLimit), ReaderError> {
    let validator = ReadValidators {
        etag: claim.validators.etag.clone(),
        last_modified: claim.validators.last_modified.clone(),
    };
    match (claim.target.family, claim.target.item) {
        (ResourceFamily::RepositoryItems, None) => {
            let response = reader
                .updated_issues(window(claim)?, page(claim)?, &validator, cancel)
                .await?;
            feed(response, |issues| {
                Ok((
                    issues
                        .into_iter()
                        .map(observed_issue)
                        .collect::<Result<_, _>>()?,
                    Vec::new(),
                ))
            })
        }
        (ResourceFamily::Comments, None) => {
            let response = reader
                .updated_comments(window(claim)?, page(claim)?, &validator, cancel)
                .await?;
            feed(response, |comments| {
                Ok((
                    Vec::new(),
                    comments.into_iter().map(observed_comment).collect(),
                ))
            })
        }
        (ResourceFamily::Metadata, Some(item)) => {
            let number = item_number(store, claim.account, item, claim.target.repository).await?;
            let response = reader.refresh_issue(number, &validator, cancel).await?;
            let validators = validators(response.validators);
            let outcome = match response.value {
                Some(summary) => Outcome::Metadata {
                    summary: Box::new(crate::reader::issue(summary)?),
                    validators,
                },
                None => Outcome::Unmodified(validators),
            };
            Ok((outcome, response.rate))
        }
        _ => Err(ReaderError::InvalidIdentity),
    }
}

async fn item_number(
    store: &Arc<Store>,
    account: AccountId,
    item: ItemId,
    repository: RepositoryId,
) -> Result<ItemNumber, ReaderError> {
    let number = blocking(store, move |store| {
        let current = store.item(account, item)?.ok_or(Error::NotFound)?;
        if current.locator.repository != repository {
            return Err(Error::Conflict("Item moved during refresh"));
        }
        Ok(current.locator.number)
    })
    .await?;
    number.try_into().map_err(|_| ReaderError::InvalidIdentity)
}

fn feed<T>(
    response: Refresh<Page<T>>,
    convert: impl FnOnce(Vec<T>) -> Result<(Vec<ObservedItem>, Vec<ObservedComment>), ReaderError>,
) -> Result<(Outcome, RateLimit), ReaderError> {
    let validators = validators(response.validators);
    let outcome = if let Some(page) = response.value {
        let (items, comments) = convert(page.records)?;
        Outcome::Feed {
            items,
            comments,
            next: page.next_page.map(|next| next.to_string()),
            validators,
            limited: page.sso_limited,
        }
    } else {
        Outcome::Unmodified(validators)
    };
    Ok((outcome, response.rate))
}

fn validators(values: ReadValidators) -> Validators {
    Validators {
        etag: values.etag,
        last_modified: values.last_modified,
        revision: None,
    }
}

fn window(claim: &ReadClaim) -> Result<i64, ReaderError> {
    claim.since_ms.ok_or(ReaderError::BadCursor)
}

fn page(claim: &ReadClaim) -> Result<u32, ReaderError> {
    claim.cursor.as_deref().map_or(Ok(1), |value| {
        value
            .parse::<u32>()
            .ok()
            .filter(|page| *page > 1)
            .ok_or(ReaderError::BadCursor)
    })
}

fn observed_issue(summary: IssueSummary) -> Result<ObservedItem, ReaderError> {
    let provider_id = summary.issue_id.get();
    let created_at_ms = summary.created_at_ms;
    let parsed = crate::reader::issue(summary)?;
    Ok(ObservedItem {
        number: parsed.number,
        provider_id,
        created_at_ms,
        update: parsed.update,
    })
}

fn observed_comment(row: UpdatedComment) -> ObservedComment {
    let ConversationComment {
        id,
        node_id,
        author,
        created_at_ms,
        updated_at_ms,
        body,
    } = row.comment;
    ObservedComment {
        number: row.issue.get(),
        comment: DiscussionComment {
            provider_id: id.get(),
            node_id: Some(node_id),
            author,
            created_at_ms,
            updated_at_ms,
            body,
        },
    }
}

fn failure(error: &ReaderError, claim: &ReadClaim) -> ReadFailure {
    match error {
        ReaderError::Cancelled => ReadFailure::Cancelled,
        ReaderError::BadCursor => ReadFailure::ExpiredCursor,
        ReaderError::ScopeChanged
        | ReaderError::CredentialUnavailable
        | ReaderError::ProtectedStore(_) => ReadFailure::Authorization,
        ReaderError::Remote(error) => match error.kind {
            AccessKind::Cancelled => ReadFailure::Cancelled,
            AccessKind::RateLimited => ReadFailure::Throttled,
            AccessKind::Unauthorized
            | AccessKind::SsoRequired
            | AccessKind::ApprovalPending
            | AccessKind::PermissionDenied
            | AccessKind::MissingOrInaccessible
            | AccessKind::IdentityMismatch => ReadFailure::Authorization,
            AccessKind::InvalidPagination if claim.cursor.is_some() => ReadFailure::ExpiredCursor,
            AccessKind::UnexpectedStatus
                if claim.cursor.is_some() && matches!(error.http_status, Some(410 | 422)) =>
            {
                ReadFailure::ExpiredCursor
            }
            _ => ReadFailure::Transient,
        },
        _ => ReadFailure::Transient,
    }
}
