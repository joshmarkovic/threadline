use std::{fmt, io::Cursor, sync::Arc};

use threadline_core::{
    domain::{ItemKind as ProviderKind, ItemNumber, RepositoryApiId, RepositoryName},
    github::{
        AccessError, AccessKind, Reachability,
        attachments::{self, ImageError},
        reader::{
            BoundRepositoryReader, IssueSummary, PullRequestSummary, RemoteSearchPage,
            RemoteSearchQuery, Reported, SearchQueryError,
        },
    },
    sync::Cancellation,
};
use threadline_platform::PlatformError;
use threadline_storage::{
    AccountId, Change, IssueClassification, IssueRecord, ItemId, ItemKind, ItemLocator,
    ItemRepresentation, ItemUpdate, NodeId, PullRequestRecord, PullRequestUpdate, RepositoryId,
    Store, UtcMillis,
    access::{AccessProfile, AccessState},
    reader::{
        DiscussionComment, DiscussionInputPage, IssuePresentation, LinkedItem, ListFacets,
        ListSummary, ReactionCounts, RepositoryListPage, TimelineEntry, TimelineInputPage,
        repository_list_key,
    },
    resources::{
        Coverage, ResourceError, ResourceFamily, ResourceKey, ResourceProgress, ResourceTarget,
        Validators, WalkMode,
    },
    sync::ReadBudget,
};

use crate::accounts::AccountService;
use crate::sync_worker::{blocking, hold};

pub(crate) mod pr_reads;
pub(crate) use pr_reads::PrSourceRequest;
mod merge_policy;
mod write_choices;
pub(crate) use write_choices::values as write_values;

#[derive(Clone, Debug)]
pub(crate) enum ReaderError {
    Offline,
    ServerBackoff,
    ScopeChanged,
    CredentialUnavailable,
    Cancelled,
    InvalidIdentity,
    IncompleteAccess,
    SourceUnavailable,
    BadCursor,
    Remote(AccessError),
    Query(SearchQueryError),
    Storage(Arc<threadline_storage::Error>),
    ProtectedStore(PlatformError),
    Image(ImageError),
}

impl fmt::Display for ReaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Offline => {
                f.write_str("Work Offline is enabled. Downloaded content remains available.")
            }
            Self::ServerBackoff => {
                f.write_str("GitHub has deferred requests. Cached content remains available.")
            }
            Self::ScopeChanged => f.write_str("The selected repository or active account changed."),
            Self::CredentialUnavailable | Self::ProtectedStore(_) => f.write_str(
                "The saved credential is unavailable. Downloaded content remains available.",
            ),
            Self::Cancelled => f.write_str("The download was cancelled."),
            Self::InvalidIdentity => {
                f.write_str("The item does not match the selected repository.")
            }
            Self::IncompleteAccess => f.write_str("GitHub withheld some content because of SSO."),
            Self::SourceUnavailable => f.write_str("This source range cannot be previewed."),
            Self::BadCursor => {
                f.write_str("The saved page cursor is invalid. Retry to restart the download.")
            }
            Self::Remote(error) => fmt::Display::fmt(error, f),
            Self::Query(error) => fmt::Display::fmt(error, f),
            Self::Storage(error) => fmt::Display::fmt(error, f),
            Self::Image(error) => fmt::Display::fmt(error, f),
        }
    }
}

impl std::error::Error for ReaderError {}

impl From<AccessError> for ReaderError {
    fn from(error: AccessError) -> Self {
        Self::Remote(error)
    }
}

impl From<SearchQueryError> for ReaderError {
    fn from(error: SearchQueryError) -> Self {
        Self::Query(error)
    }
}

impl From<threadline_storage::Error> for ReaderError {
    fn from(error: threadline_storage::Error) -> Self {
        Self::Storage(Arc::new(error))
    }
}

impl From<PlatformError> for ReaderError {
    fn from(error: PlatformError) -> Self {
        Self::ProtectedStore(error)
    }
}

impl From<ImageError> for ReaderError {
    fn from(error: ImageError) -> Self {
        Self::Image(error)
    }
}

impl ReaderError {
    pub(crate) fn reachability(&self) -> Reachability {
        match self {
            Self::Remote(error) => error.reachability,
            _ => Reachability::NotAttempted,
        }
    }

    fn resource_error(&self) -> ResourceError {
        match self {
            Self::BadCursor => ResourceError::CursorExpired,
            Self::Offline => ResourceError::Transport,
            Self::ServerBackoff => ResourceError::RateLimited,
            Self::ProtectedStore(error) => match error {
                PlatformError::Denied => ResourceError::Forbidden,
                _ => ResourceError::Unauthorized,
            },
            Self::CredentialUnavailable => ResourceError::Unauthorized,
            Self::ScopeChanged | Self::IncompleteAccess => ResourceError::Forbidden,
            Self::Remote(error) => match error.kind {
                AccessKind::Unauthorized | AccessKind::InvalidCredential => {
                    ResourceError::Unauthorized
                }
                AccessKind::PermissionDenied
                | AccessKind::SsoRequired
                | AccessKind::ApprovalPending => ResourceError::Forbidden,
                AccessKind::MissingOrInaccessible => ResourceError::NotFound,
                AccessKind::RateLimited => ResourceError::RateLimited,
                AccessKind::Transport | AccessKind::Timeout | AccessKind::ServiceFailure => {
                    ResourceError::Transport
                }
                AccessKind::Cancelled => ResourceError::Unknown,
                _ => ResourceError::InvalidResponse,
            },
            Self::InvalidIdentity => ResourceError::InvalidResponse,
            _ => ResourceError::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PageStatus {
    pub(crate) has_more: bool,
    pub(crate) complete: bool,
    pub(crate) contacted: bool,
}

pub(crate) struct FetchedImage {
    pub(crate) bytes: Vec<u8>,
    pub(crate) mime: &'static str,
}

impl PageStatus {
    fn complete() -> Self {
        Self {
            has_more: false,
            complete: true,
            contacted: false,
        }
    }

    fn from_next(next: Option<u32>, limited: bool) -> Self {
        Self {
            has_more: !limited && next.is_some(),
            complete: !limited && next.is_none(),
            contacted: true,
        }
    }
}

/// Run on the background runtime. The reader publishes only to the verified account and stable item.
#[derive(Clone)]
pub(crate) struct ReaderService {
    accounts: AccountService,
    store: Arc<Store>,
    pr_reads: Arc<pr_reads::Flights>,
}

#[derive(Eq, PartialEq)]
struct Scope {
    account: AccountId,
    repository: RepositoryId,
    provider_id: RepositoryApiId,
    route: RepositoryName,
    profile: AccessProfile,
}

impl ReaderService {
    pub(crate) fn new(accounts: AccountService) -> Self {
        let store = accounts.reader_store();
        Self {
            accounts,
            store,
            pr_reads: Arc::default(),
        }
    }

    pub(crate) async fn fetch_list_page(
        &self,
        account: AccountId,
        repository: RepositoryId,
        cancel: &Cancellation,
    ) -> Result<PageStatus, ReaderError> {
        let scope = self.scope(account, repository, cancel)?;
        let key = repository_list_key(account, repository);
        let Some(progress) = self.progress(key)? else {
            return Ok(PageStatus::complete());
        };
        let result = async {
            let binding = self.binding(&scope, cancel).await?;
            self.admit(account, cancel).await?;
            let page = self
                .response(
                    account,
                    binding.reader().list(cursor(&progress)?, cancel).await,
                )
                .await?;
            self.check_scope(&scope, cancel)?;
            let next = page.next_page.map(|next| next.to_string());
            let list = RepositoryListPage {
                summaries: page
                    .records
                    .into_iter()
                    .map(issue)
                    .collect::<Result<_, _>>()?,
                next_cursor: if page.sso_limited {
                    None
                } else {
                    next.as_deref()
                },
                completed_validators: None,
                fetched_at_ms: UtcMillis::now()?.0,
            };
            if page.sso_limited {
                self.store.commit_repository_list_partial(
                    account,
                    repository,
                    &progress,
                    list,
                    ResourceError::Forbidden,
                )?;
            } else {
                self.store
                    .commit_repository_list_page(account, repository, &progress, list)?;
            }
            Ok(PageStatus::from_next(page.next_page, page.sso_limited))
        }
        .await;
        self.record_error(key, &progress, result)
    }

    pub(crate) async fn fetch_item(
        &self,
        account: AccountId,
        item: ItemId,
        cancel: &Cancellation,
    ) -> Result<(), ReaderError> {
        let original = self
            .store
            .item(account, item)?
            .ok_or(ReaderError::ScopeChanged)?;
        let (scope, number) = self.item_scope(account, original.locator, cancel)?;
        let binding = self.binding(&scope, cancel).await?;
        let reader = binding.reader();
        self.admit(account, cancel).await?;
        let summary = self
            .response(account, reader.issue(number, cancel).await)
            .await?;
        let is_pr =
            summary.kind == ProviderKind::PullRequest || original.kind == ItemKind::PullRequest;
        self.check_item(&scope, item, original.locator, cancel)?;
        self.commit_item(&scope, item, issue(summary)?)?;
        if is_pr {
            self.admit(account, cancel).await?;
            let details = self
                .response(account, reader.pull_request(number, cancel).await)
                .await?;
            self.check_item(&scope, item, original.locator, cancel)?;
            self.commit_item(&scope, item, pull_request(details)?)?;
        }
        Ok(())
    }

    pub(crate) async fn fetch_comments(
        &self,
        account: AccountId,
        item: ItemId,
        cancel: &Cancellation,
    ) -> Result<PageStatus, ReaderError> {
        let locator = self
            .store
            .item(account, item)?
            .ok_or(ReaderError::ScopeChanged)?
            .locator;
        let (scope, number) = self.item_scope(account, locator, cancel)?;
        let key = ResourceKey {
            account,
            target: ResourceTarget::Item(item),
            family: ResourceFamily::Comments,
        };
        let Some(progress) = self.progress(key)? else {
            return Ok(PageStatus::complete());
        };
        let result = async {
            let binding = self.binding(&scope, cancel).await?;
            self.admit(account, cancel).await?;
            let page = self
                .response(
                    account,
                    binding
                        .reader()
                        .comments(number, cursor(&progress)?, cancel)
                        .await,
                )
                .await?;
            self.check_item(&scope, item, locator, cancel)?;
            let next = page.next_page.map(|next| next.to_string());
            let observed_at = UtcMillis::now()?.0;
            let authors: Vec<_> = page
                .records
                .iter()
                .filter_map(|comment| {
                    comment.author_id.map(|author_id| {
                        threadline_storage::writes::CommentAuthorObservation {
                            comment_id: comment.id,
                            author_id,
                            observed_at_ms: observed_at,
                        }
                    })
                })
                .collect();
            let comments = DiscussionInputPage {
                comments: page
                    .records
                    .into_iter()
                    .map(|comment| DiscussionComment {
                        provider_id: comment.id.get(),
                        node_id: Some(comment.node_id),
                        author: comment.author,
                        created_at_ms: comment.created_at_ms,
                        updated_at_ms: comment.updated_at_ms,
                        body: comment.body,
                    })
                    .collect(),
                next_cursor: if page.sso_limited {
                    None
                } else {
                    next.as_deref()
                },
                completed_validators: None,
                fetched_at_ms: observed_at,
            };
            if page.sso_limited {
                self.store.commit_discussion_partial(
                    account,
                    item,
                    &progress,
                    comments,
                    ResourceError::Forbidden,
                )?;
            } else {
                self.store
                    .commit_discussion_page(account, item, &progress, comments)?;
            }
            if !authors.is_empty() {
                self.store.record_comment_authors(
                    &scope.profile,
                    scope.repository,
                    item,
                    &authors,
                )?;
            }
            Ok(PageStatus::from_next(page.next_page, page.sso_limited))
        }
        .await;
        self.record_error(key, &progress, result)
    }

    pub(crate) async fn fetch_timeline(
        &self,
        account: AccountId,
        item: ItemId,
        cancel: &Cancellation,
    ) -> Result<PageStatus, ReaderError> {
        let locator = self
            .store
            .item(account, item)?
            .ok_or(ReaderError::ScopeChanged)?
            .locator;
        let (scope, number) = self.item_scope(account, locator, cancel)?;
        let key = ResourceKey {
            account,
            target: ResourceTarget::Item(item),
            family: ResourceFamily::Timeline,
        };
        let Some(progress) = self.progress(key)? else {
            return Ok(PageStatus::complete());
        };
        let result = async {
            let binding = self.binding(&scope, cancel).await?;
            self.admit(account, cancel).await?;
            let page = self
                .response(
                    account,
                    binding
                        .reader()
                        .timeline(number, cursor(&progress)?, cancel)
                        .await,
                )
                .await?;
            self.check_item(&scope, item, locator, cancel)?;
            if page.sso_limited {
                return Err(ReaderError::IncompleteAccess);
            }
            let next = page.next_page.map(|next| next.to_string());
            self.store.commit_timeline_page(
                account,
                item,
                &progress,
                TimelineInputPage {
                    events: page
                        .records
                        .into_iter()
                        .map(|event| TimelineEntry {
                            remote_key: event.key,
                            event: event.kind,
                            actor: event.actor,
                            occurred_at_ms: event.created_at_ms,
                            linked_item: event.linked_item.map(|link| LinkedItem {
                                owner: link.route.owner().to_owned(),
                                repository: link.route.name().to_owned(),
                                number: link.number.get(),
                                pull_request: link.pull_request,
                                provider_repository_id: link
                                    .repository_id
                                    .map(RepositoryApiId::get),
                                provider_issue_id: link.issue_id.map(|id| id.get()),
                            }),
                        })
                        .collect(),
                    next_cursor: next.as_deref(),
                    completed_validators: None,
                    fetched_at_ms: UtcMillis::now()?.0,
                },
            )?;
            Ok(PageStatus::from_next(page.next_page, false))
        }
        .await;
        self.record_error(key, &progress, result)
    }

    pub(crate) async fn search_remote(
        &self,
        account: AccountId,
        repository: RepositoryId,
        query: &str,
        page: u32,
        cancel: &Cancellation,
    ) -> Result<RemoteSearchPage, ReaderError> {
        let query = RemoteSearchQuery::new(query)?;
        let scope = self.scope(account, repository, cancel)?;
        let binding = self.binding(&scope, cancel).await?;
        self.admit(account, cancel).await?;
        let result = self
            .response(account, binding.reader().search(&query, page, cancel).await)
            .await?;
        self.check_scope(&scope, cancel)?;
        Ok(result)
    }

    /// Explicit image loading uses the active scope and an unauthenticated media client.
    pub(crate) async fn fetch_image(
        &self,
        account: AccountId,
        repository: RepositoryId,
        url: &str,
        cancel: &Cancellation,
    ) -> Result<FetchedImage, ReaderError> {
        let scope = self.scope(account, repository, cancel)?;
        if self.accounts.fixture {
            return Err(ImageError::Unavailable.into());
        }
        let bytes = attachments::fetch_image(url, cancel).await?;
        self.check_scope(&scope, cancel)?;
        Ok(validate_image(bytes)?)
    }

    async fn binding(
        &self,
        scope: &Scope,
        cancel: &Cancellation,
    ) -> Result<BoundRepositoryReader, ReaderError> {
        let account = scope.account;
        self.admit(account, cancel).await?;
        let (session, rate) = match self
            .accounts
            .reader_session_with_guidance(&scope.profile, cancel)
            .await
        {
            Err(ReaderError::Remote(error)) => {
                return self
                    .response::<BoundRepositoryReader>(account, Err(error))
                    .await;
            }
            other => other?,
        };
        if self.accounts.sync_worker_enabled() {
            hold(&self.store, account, Some(rate), None).await;
        }
        self.check_scope(scope, cancel)?;
        self.admit(account, cancel).await?;
        let session = Arc::new(session);
        let (reader, rate) = self
            .response(
                account,
                session
                    .bind_reader(&scope.route, scope.provider_id, cancel)
                    .await,
            )
            .await?;
        if self.accounts.sync_worker_enabled() {
            hold(&self.store, account, Some(rate), None).await;
        }
        Ok(reader)
    }

    async fn response<T>(
        &self,
        account: AccountId,
        result: Result<T, AccessError>,
    ) -> Result<T, ReaderError> {
        if self.accounts.sync_worker_enabled()
            && let Err(error) = &result
        {
            hold(&self.store, account, error.rate, Some(error.kind)).await;
        }
        result.map_err(ReaderError::from)
    }

    async fn admit(&self, account: AccountId, cancel: &Cancellation) -> Result<(), ReaderError> {
        if !self.accounts.sync_worker_enabled() {
            return Ok(());
        }
        loop {
            if cancel.is_cancelled() {
                return Err(ReaderError::Cancelled);
            }
            let (now, budget) = blocking(&self.store, move |store| {
                if store.active_github_account()? != Some(account)
                    || store.dispatch_profile(account)?.offline
                {
                    return Ok((0, None));
                }
                let now = UtcMillis::now()?.0;
                Ok((now, Some(store.reserve_read_budget(account, now)?)))
            })
            .await?;
            match budget {
                None => return Err(ReaderError::Offline),
                Some(ReadBudget::Granted) => return Ok(()),
                Some(ReadBudget::Deferred { server: true, .. }) => {
                    return Err(ReaderError::ServerBackoff);
                }
                Some(ReadBudget::Deferred {
                    until_ms,
                    server: false,
                }) => {
                    let delay = std::time::Duration::from_millis(
                        until_ms.saturating_sub(now).clamp(1, 1000) as u64,
                    );
                    tokio::select! {
                        _ = cancel.cancelled() => return Err(ReaderError::Cancelled),
                        _ = tokio::time::sleep(delay) => {},
                    }
                }
            }
        }
    }

    fn scope(
        &self,
        account: AccountId,
        repository: RepositoryId,
        cancel: &Cancellation,
    ) -> Result<Scope, ReaderError> {
        if cancel.is_cancelled() {
            return Err(ReaderError::Cancelled);
        }
        if self.store.active_github_account()? != Some(account) {
            return Err(ReaderError::ScopeChanged);
        }
        if self.store.dispatch_profile(account)?.offline {
            return Err(ReaderError::Offline);
        }
        let repo = self
            .store
            .github_repository_views(account)?
            .into_iter()
            .find(|entry| entry.id == repository && entry.selected && entry.verified_current)
            .ok_or(ReaderError::ScopeChanged)?;
        let profile = self
            .store
            .github_access_profile(account, &repo.owner)?
            .filter(|profile| {
                !matches!(
                    profile.access,
                    AccessState::SignedOut
                        | AccessState::Unauthorized
                        | AccessState::CredentialUnavailable
                )
            })
            .ok_or(ReaderError::CredentialUnavailable)?;
        let route = RepositoryName::new(&repo.owner, &repo.name)
            .map_err(|_| ReaderError::InvalidIdentity)?;
        Ok(Scope {
            account,
            repository,
            provider_id: RepositoryApiId::new(repo.provider_id),
            route,
            profile,
        })
    }

    fn item_scope(
        &self,
        account: AccountId,
        locator: ItemLocator,
        cancel: &Cancellation,
    ) -> Result<(Scope, ItemNumber), ReaderError> {
        let number = locator
            .number
            .try_into()
            .map_err(|_| ReaderError::InvalidIdentity)?;
        let scope = self.scope(account, locator.repository, cancel)?;
        Ok((scope, number))
    }

    fn check_scope(&self, expected: &Scope, cancel: &Cancellation) -> Result<(), ReaderError> {
        if &self.scope(expected.account, expected.repository, cancel)? != expected {
            return Err(ReaderError::ScopeChanged);
        }
        Ok(())
    }

    fn check_item(
        &self,
        scope: &Scope,
        item: ItemId,
        expected: ItemLocator,
        cancel: &Cancellation,
    ) -> Result<(), ReaderError> {
        self.check_scope(scope, cancel)?;
        if self
            .store
            .item(scope.account, item)?
            .is_none_or(|item| item.locator != expected)
        {
            return Err(ReaderError::ScopeChanged);
        }
        Ok(())
    }

    fn progress(&self, key: ResourceKey) -> Result<Option<ResourceProgress>, ReaderError> {
        if let Some(progress) = self.store.resume_resource_fetch(key)? {
            return Ok(Some(progress));
        }
        if self.store.resource_state(key)?.coverage == Coverage::Complete {
            return Ok(None);
        }
        Ok(Some(self.store.start_resource_fetch(key, WalkMode::Full)?))
    }

    fn commit_item(
        &self,
        scope: &Scope,
        item: ItemId,
        summary: ListSummary,
    ) -> Result<(), ReaderError> {
        let id = self.store.commit_item_metadata(
            scope.account,
            scope.repository,
            summary,
            &Validators::default(),
            UtcMillis::now()?.0,
        )?;
        if id != item {
            return Err(ReaderError::InvalidIdentity);
        }
        Ok(())
    }

    fn record_error<T>(
        &self,
        key: ResourceKey,
        progress: &ResourceProgress,
        result: Result<T, ReaderError>,
    ) -> Result<T, ReaderError> {
        if let Err(error) = &result {
            // A competing view may already own a newer generation. Never change its checkpoint.
            let _ = self
                .store
                .record_resource_error(key, progress, error.resource_error());
        }
        result
    }
}

fn validate_image(bytes: Vec<u8>) -> Result<FetchedImage, ImageError> {
    let format = image::guess_format(&bytes).map_err(|_| ImageError::UnsupportedType)?;
    let mime = match format {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        _ => return Err(ImageError::UnsupportedType),
    };
    let (width, height) = image::ImageReader::with_format(Cursor::new(&bytes), format)
        .into_dimensions()
        .map_err(|_| ImageError::UnsupportedType)?;
    if width == 0 || height == 0 || width > 4096 || height > 4096 {
        return Err(ImageError::TooLarge);
    }
    Ok(FetchedImage { bytes, mime })
}

fn cursor(progress: &ResourceProgress) -> Result<u32, ReaderError> {
    progress.cursor.as_deref().map_or(Ok(1), |value| {
        value
            .parse::<u32>()
            .ok()
            .filter(|page| *page > 1)
            .ok_or(ReaderError::BadCursor)
    })
}

pub(crate) fn issue(summary: IssueSummary) -> Result<ListSummary, ReaderError> {
    Ok(ListSummary {
        number: summary.number.get(),
        update: ItemUpdate {
            representation: ItemRepresentation::Issue(IssueRecord {
                id: summary.issue_id,
                node_id: Some(NodeId::new(summary.node_id)?),
                classification: match summary.kind {
                    ProviderKind::Issue => IssueClassification::Issue,
                    ProviderKind::PullRequest => IssueClassification::PullRequest,
                },
            }),
            remote_updated_at: UtcMillis(summary.updated_at_ms),
            title: Change::Value(summary.title),
            body: field(summary.body),
            closed: Change::Value(summary.closed),
        },
        facets: ListFacets {
            author: summary.author,
            labels: summary.labels,
            assignees: summary.assignees,
            presentation: IssuePresentation {
                created_at_ms: summary.created_at_ms,
                milestone: field(summary.milestone),
                reactions: summary.reactions.map(|counts| ReactionCounts {
                    positive: counts.positive,
                    negative: counts.negative,
                    laugh: counts.laugh,
                    hooray: counts.hooray,
                    confused: counts.confused,
                    heart: counts.heart,
                    rocket: counts.rocket,
                    eyes: counts.eyes,
                    total: counts.total,
                }),
            },
            ..Default::default()
        },
    })
}

fn pull_request(details: PullRequestSummary) -> Result<ListSummary, ReaderError> {
    Ok(ListSummary {
        number: details.number.get(),
        update: ItemUpdate {
            representation: ItemRepresentation::PullRequest(PullRequestRecord {
                id: details.pull_request_id.get().try_into()?,
                node_id: Some(NodeId::new(details.node_id)?),
                details: PullRequestUpdate {
                    merged: Change::Value(details.merged),
                    base_oid: Change::Value(details.base_oid),
                    head_oid: Change::Value(details.head_oid),
                    merge_oid: field(details.merge_oid),
                },
            }),
            remote_updated_at: UtcMillis(details.updated_at_ms),
            title: Change::Value(details.title),
            body: field(details.body),
            closed: Change::Value(details.closed),
        },
        facets: ListFacets::default(),
    })
}

fn field<T>(snapshot: Reported<T>) -> Change<T> {
    match snapshot {
        Reported::Absent => Change::Missing,
        Reported::Null => Change::Null,
        Reported::Value(value) => Change::Value(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_preview_accepts_bounded_native_raster_formats() {
        let source = image::DynamicImage::ImageRgb8(image::RgbImage::new(1, 1));
        let mut output = Cursor::new(Vec::new());
        source
            .write_to(&mut output, image::ImageFormat::Png)
            .unwrap();
        let bytes = output.into_inner();
        let preview = validate_image(bytes.clone()).unwrap();
        assert_eq!(preview.mime, "image/png");
        assert_eq!(preview.bytes, bytes);
        assert!(matches!(
            validate_image(b"<svg xmlns='synthetic'/>".to_vec()),
            Err(ImageError::UnsupportedType)
        ));
    }
}
