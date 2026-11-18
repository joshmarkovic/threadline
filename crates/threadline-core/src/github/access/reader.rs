//! Read-only access to one verified repository. Page cursors never select an origin or target.

pub mod history;
pub mod pr;
pub mod refresh;
mod wire;

use reqwest::{Url, header::HeaderMap};
use serde::de::DeserializeOwned;
use std::{fmt, num::NonZeroU64, sync::Arc};

use crate::domain::{IssueApiId, ItemKind, ItemNumber, RepositoryApiId, RepositoryName};

use super::{
    AccessError, AccessKind, CancellationToken, GitHubSession, RateLimit, RepositoryAccess,
    RepositoryRead, next_page_for, partial_sso,
};

const PAGE_SIZE: u32 = 20;
pub const SEARCH_RESULT_CAP: u64 = 1_000;

#[derive(Clone, Default, Eq, PartialEq)]
pub enum Reported<T> {
    #[default]
    Absent,
    Null,
    Value(T),
}

pub struct Page<T> {
    pub records: Vec<T>,
    pub next_page: Option<u32>,
    pub sso_limited: bool,
}

#[derive(Clone, Eq, PartialEq)]
pub struct IssueSummary {
    pub number: ItemNumber,
    pub issue_id: IssueApiId,
    pub node_id: String,
    pub kind: ItemKind,
    pub title: String,
    pub body: Reported<String>,
    pub closed: bool,
    pub created_at_ms: Option<i64>,
    pub updated_at_ms: i64,
    pub author: Option<String>,
    pub labels: Option<Vec<String>>,
    pub assignees: Option<Vec<String>>,
    pub milestone: Reported<String>,
    pub reactions: Option<ReactionCounts>,
}

#[derive(Clone, Eq, PartialEq)]
pub struct ReactionCounts {
    pub positive: u64,
    pub negative: u64,
    pub laugh: u64,
    pub hooray: u64,
    pub confused: u64,
    pub heart: u64,
    pub rocket: u64,
    pub eyes: u64,
    pub total: Option<u64>,
}

#[derive(Clone, Eq, PartialEq)]
pub struct PullRequestSummary {
    pub number: ItemNumber,
    pub pull_request_id: NonZeroU64,
    pub node_id: String,
    pub title: String,
    pub body: Reported<String>,
    pub closed: bool,
    pub updated_at_ms: i64,
    pub merged: bool,
    pub base_oid: String,
    pub head_oid: String,
    pub merge_oid: Reported<String>,
    pub presentation: PullPresentation,
}

#[derive(Clone, Default, Eq, PartialEq)]
pub struct PullPresentation {
    pub author: Option<String>,
    pub author_id: Option<NonZeroU64>,
    pub locked: Option<bool>,
    pub merged_by: Option<NonZeroU64>,
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
    pub draft: Option<bool>,
    pub requested_reviewers: Option<Vec<String>>,
    pub created_at_ms: Option<i64>,
    pub merged_at_ms: Option<i64>,
    pub changed_files: Option<u64>,
    base_repository_id: Option<RepositoryApiId>,
    pub(super) head_repository_id: Option<RepositoryApiId>,
    base_repository_node: Option<String>,
}

#[derive(Clone, Eq, PartialEq)]
pub struct ConversationComment {
    pub id: NonZeroU64,
    pub node_id: String,
    pub author: Option<String>,
    pub author_id: Option<NonZeroU64>,
    pub body: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Eq, PartialEq)]
pub struct TimelineEvent {
    pub key: String,
    pub kind: String,
    pub actor: Option<String>,
    pub created_at_ms: Option<i64>,
    pub stable_identity: bool,
    pub linked_item: Option<LinkedItem>,
}

/// A structured source issue reported by GitHub for a cross-reference event.
#[derive(Clone, Eq, PartialEq)]
pub struct LinkedItem {
    pub route: RepositoryName,
    pub number: ItemNumber,
    pub pull_request: bool,
    pub repository_id: Option<RepositoryApiId>,
    pub issue_id: Option<IssueApiId>,
}

pub struct RemoteSearchPage {
    pub hits: Vec<IssueSummary>,
    pub next_page: Option<u32>,
    pub reported_total: u64,
    pub incomplete: bool,
    pub capped: bool,
}

/// Search terms are always sent as an exact literal phrase within one verified repository.
pub struct RemoteSearchQuery(String);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchQueryError {
    Empty,
    Unsupported,
}

impl fmt::Display for SearchQueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "Enter text to search GitHub.",
            Self::Unsupported => {
                "GitHub discovery currently supports a plain search phrase without qualifiers."
            }
        })
    }
}

impl std::error::Error for SearchQueryError {}

impl RemoteSearchQuery {
    pub fn new(text: &str) -> Result<Self, SearchQueryError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(SearchQueryError::Empty);
        }
        if text.len() > 160
            || text
                .chars()
                .any(|ch| ch.is_control() || matches!(ch, ':' | '"' | '\\'))
        {
            return Err(SearchQueryError::Unsupported);
        }
        Ok(Self(text.to_owned()))
    }
}

/// Built only after GitHub confirms the immutable repository ID and access context.
pub struct GitHubReader<'a> {
    session: &'a GitHubSession,
    route: RepositoryName,
    repository: RepositoryApiId,
}

impl<'a> GitHubReader<'a> {
    pub(super) fn from_access(session: &'a GitHubSession, repository: &RepositoryAccess) -> Self {
        Self {
            session,
            route: repository.route.clone(),
            repository: repository.id,
        }
    }
}

/// A repository binding owns the authenticated session that verified it.
pub struct BoundRepositoryReader {
    session: Arc<GitHubSession>,
    route: RepositoryName,
    repository: RepositoryApiId,
}

impl BoundRepositoryReader {
    pub fn reader(&self) -> GitHubReader<'_> {
        GitHubReader {
            session: &self.session,
            route: self.route.clone(),
            repository: self.repository,
        }
    }
}

impl GitHubSession {
    /// Verify the explicitly designated private repository and reuse its bound reader.
    pub async fn private_reader(
        &self,
        route: &RepositoryName,
        repository: RepositoryApiId,
        cancel: &CancellationToken,
    ) -> Result<(RepositoryAccess, GitHubReader<'_>), AccessError> {
        let verified = self
            .verify_private_repository(route, Some(repository), cancel)
            .await?;
        let reader = GitHubReader {
            session: self,
            route: verified.route.clone(),
            repository: verified.id,
        };
        Ok((verified, reader))
    }

    pub async fn bind_reader(
        self: &Arc<Self>,
        route: &RepositoryName,
        repository: RepositoryApiId,
        cancel: &CancellationToken,
    ) -> Result<(BoundRepositoryReader, RateLimit), AccessError> {
        let (
            RepositoryRead::Fresh {
                repository: checked,
                ..
            },
            rate,
        ) = self
            .repository_with_guidance(route, Some(repository), None, cancel)
            .await?
        else {
            return Err(AccessError::received(AccessKind::UnexpectedStatus, 304));
        };
        let bound = BoundRepositoryReader {
            session: self.clone(),
            route: checked.route,
            repository: checked.id,
        };
        Ok((bound, rate))
    }

    pub async fn reader(
        &self,
        route: &RepositoryName,
        repository: RepositoryApiId,
        cancel: &CancellationToken,
    ) -> Result<GitHubReader<'_>, AccessError> {
        let checked = self.repository(route, Some(repository), cancel).await?;
        Ok(GitHubReader {
            session: self,
            route: checked.route,
            repository: checked.id,
        })
    }
}

impl GitHubReader<'_> {
    pub async fn list(
        &self,
        page: u32,
        cancel: &CancellationToken,
    ) -> Result<Page<IssueSummary>, AccessError> {
        let listed = self
            .paginate::<wire::Issue>(
                "issues",
                page,
                &[("state", "all"), ("sort", "created"), ("direction", "desc")],
                cancel,
            )
            .await?;
        let records = listed
            .records
            .into_iter()
            .map(|value| value.into_summary(self, false))
            .collect::<Result<_, _>>()?;
        Ok(Page {
            records,
            next_page: listed.next_page,
            sso_limited: listed.sso_limited,
        })
    }

    pub async fn issue(
        &self,
        number: ItemNumber,
        cancel: &CancellationToken,
    ) -> Result<IssueSummary, AccessError> {
        let suffix = format!("issues/{}", number.get());
        let (wire, _): (wire::Issue, _) = self.session.get(&self.path(&suffix), cancel).await?;
        let issue = wire.into_summary(self, false)?;
        require_number(number, issue.number)?;
        Ok(issue)
    }

    pub async fn pull_request(
        &self,
        number: ItemNumber,
        cancel: &CancellationToken,
    ) -> Result<PullRequestSummary, AccessError> {
        let suffix = format!("pulls/{}", number.get());
        let (wire, _): (wire::PullRequest, _) =
            self.session.get(&self.path(&suffix), cancel).await?;
        let pr = wire.into_summary()?;
        require_number(number, pr.number)?;
        Ok(pr)
    }

    /// Shared ordinary issue and PR conversation comments.
    pub async fn comments(
        &self,
        number: ItemNumber,
        page: u32,
        cancel: &CancellationToken,
    ) -> Result<Page<ConversationComment>, AccessError> {
        let issue = format!("issues/{}", number.get());
        let comments = self
            .paginate::<wire::Comment>(&format!("{issue}/comments"), page, &[], cancel)
            .await?;
        let records = comments
            .records
            .into_iter()
            .map(|value| value.into_comment(self, &issue))
            .collect::<Result<_, _>>()?;
        Ok(Page {
            records,
            next_page: comments.next_page,
            sso_limited: comments.sso_limited,
        })
    }

    /// Timeline activity is separate from the ordinary comment collection.
    pub async fn timeline(
        &self,
        number: ItemNumber,
        page: u32,
        cancel: &CancellationToken,
    ) -> Result<Page<TimelineEvent>, AccessError> {
        let suffix = format!("issues/{}/timeline", number.get());
        let timeline = self
            .paginate::<wire::Timeline>(&suffix, page, &[], cancel)
            .await?;
        let records = timeline
            .records
            .into_iter()
            .enumerate()
            .filter(|(_, event)| !event.is_comment())
            .map(|(index, event)| event.into_event(self, page, index))
            .collect::<Result<_, _>>()?;
        Ok(Page {
            records,
            next_page: timeline.next_page,
            sso_limited: timeline.sso_limited,
        })
    }

    /// Search only the currently verified repository. A cap or incomplete reply is
    /// returned separately from the presence of a next page.
    pub async fn search(
        &self,
        query: &RemoteSearchQuery,
        page: u32,
        cancel: &CancellationToken,
    ) -> Result<RemoteSearchPage, AccessError> {
        const MAX_PAGE: u32 = SEARCH_RESULT_CAP as u32 / PAGE_SIZE;
        if !(1..=MAX_PAGE).contains(&page) {
            return Err(AccessError::local(AccessKind::InvalidPagination));
        }
        let terms = format!(
            "repo:{}/{} \"{}\"",
            self.route.owner(),
            self.route.name(),
            query.0
        );
        let request = self.request("search/issues", page, &[("q", &terms)])?;
        let (results, headers): (wire::Search, _) =
            self.session.get(request.as_str(), cancel).await?;
        let next = next_page_for(&headers, &request, page, |path| path == "/search/issues")?
            .map(|n| n.get());
        let capped = results.total_count >= SEARCH_RESULT_CAP
            || next.is_some_and(|number| number > MAX_PAGE);
        let hits = results
            .items
            .into_iter()
            .map(|item| item.into_summary(self, true))
            .collect::<Result<_, _>>()?;
        Ok(RemoteSearchPage {
            hits,
            next_page: next.filter(|number| *number <= MAX_PAGE),
            reported_total: results.total_count,
            incomplete: results.incomplete_results || partial_sso(&headers).is_some(),
            capped,
        })
    }

    async fn paginate<T: DeserializeOwned>(
        &self,
        suffix: &str,
        page: u32,
        filters: &[(&str, &str)],
        cancel: &CancellationToken,
    ) -> Result<Page<T>, AccessError> {
        let request = self.request(&self.path(suffix), page, filters)?;
        let (records, headers): (Vec<T>, HeaderMap) =
            self.session.get(request.as_str(), cancel).await?;
        let next_page = next_page_for(&headers, &request, page, |path| {
            self.accepts_path(path, suffix)
        })?
        .map(|n| n.get());
        Ok(Page {
            records,
            next_page,
            sso_limited: partial_sso(&headers).is_some(),
        })
    }

    fn request(&self, path: &str, page: u32, filters: &[(&str, &str)]) -> Result<Url, AccessError> {
        if page == 0 {
            return Err(AccessError::local(AccessKind::InvalidPagination));
        }
        let mut url = self
            .session
            .client
            .base
            .join(path)
            .map_err(|_| AccessError::local(AccessKind::WrongScope))?;
        url.query_pairs_mut()
            .extend_pairs(filters.iter().copied())
            .append_pair("per_page", &PAGE_SIZE.to_string())
            .append_pair("page", &page.to_string());
        Ok(url)
    }

    fn path(&self, suffix: &str) -> String {
        let base = format!("repos/{}/{}", self.route.owner(), self.route.name());
        if suffix.is_empty() {
            base
        } else {
            format!("{base}/{suffix}")
        }
    }

    fn accepts_path(&self, path: &str, suffix: &str) -> bool {
        let canonical = format!("/{}", self.path(suffix));
        let numeric = if suffix.is_empty() {
            format!("/repositories/{}", self.repository.get())
        } else {
            format!("/repositories/{}/{suffix}", self.repository.get())
        };
        path.eq_ignore_ascii_case(&canonical) || path == numeric
    }

    fn is_api_record_url(&self, url: &Url) -> bool {
        let canonical = Url::parse("https://api.github.com").expect("fixed API origin");
        (url.origin() == canonical.origin()
            || self.session.client.fixture && url.origin() == self.session.client.base.origin())
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    }

    fn require_record_url(&self, url: &str, suffix: &str) -> Result<(), AccessError> {
        let url = Url::parse(url).map_err(|_| malformed())?;
        if self.is_api_record_url(&url) && self.accepts_path(url.path(), suffix) {
            Ok(())
        } else {
            Err(AccessError::received(AccessKind::IdentityMismatch, 200))
        }
    }

    fn linked_item(
        &self,
        issue_url: &str,
        repository_url: &str,
        number: ItemNumber,
        pull_request: bool,
        issue_id: Option<IssueApiId>,
    ) -> Option<LinkedItem> {
        let issue = Url::parse(issue_url).ok()?;
        let repository = Url::parse(repository_url).ok()?;
        if !self.is_api_record_url(&issue)
            || !self.is_api_record_url(&repository)
            || issue.origin() != repository.origin()
        {
            return None;
        }
        let mut parts = repository.path_segments()?;
        if parts.next()? != "repos" {
            return None;
        }
        let route = RepositoryName::new(parts.next()?, parts.next()?).ok()?;
        if parts.next().is_some()
            || !issue.path().eq_ignore_ascii_case(&format!(
                "{}/issues/{}",
                repository.path(),
                number.get()
            ))
        {
            return None;
        }
        let current = route.owner().eq_ignore_ascii_case(self.route.owner())
            && route.name().eq_ignore_ascii_case(self.route.name());
        Some(LinkedItem {
            route,
            number,
            pull_request,
            repository_id: current.then_some(self.repository),
            issue_id,
        })
    }
}

fn malformed() -> AccessError {
    AccessError::received(AccessKind::MalformedResponse, 200)
}

fn require_number(expected: ItemNumber, observed: ItemNumber) -> Result<(), AccessError> {
    if expected != observed {
        return Err(AccessError::received(AccessKind::IdentityMismatch, 200));
    }
    Ok(())
}
