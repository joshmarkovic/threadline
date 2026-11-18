//! One request per refresh against a repository bound to its immutable GitHub ID.

use chrono::{DateTime, SecondsFormat};
use reqwest::{
    Url,
    header::{self, HeaderMap, HeaderValue},
};
use serde::de::DeserializeOwned;

use super::{
    AccessError, AccessKind, CancellationToken, ConversationComment, GitHubReader, IssueSummary,
    ItemNumber, Page, next_page_for, partial_sso, require_number, wire,
};
use crate::github::access::{
    RateLimit,
    response::{ReadResponse, RequestMode, http_date, request},
    valid_etag,
};

#[derive(Clone, Default, PartialEq, Eq)]
pub struct ReadValidators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl ReadValidators {
    pub(super) fn request_mode(&self) -> RequestMode<'_> {
        RequestMode::Revalidate {
            etag: self
                .etag
                .as_deref()
                .and_then(|value| HeaderValue::from_str(value).ok())
                .filter(valid_etag),
            last_modified: self
                .last_modified
                .as_deref()
                .filter(|value| http_date(value).is_some())
                .and_then(|value| HeaderValue::from_str(value).ok()),
        }
    }

    fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            etag: headers
                .get(header::ETAG)
                .filter(|value| valid_etag(value))
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
            last_modified: headers
                .get(header::LAST_MODIFIED)
                .and_then(|value| value.to_str().ok())
                .filter(|value| http_date(value).is_some())
                .map(str::to_owned),
        }
    }
}

pub struct Refresh<T> {
    /// `None` is an authorized HTTP 304 to a request with a valid conditional header.
    pub value: Option<T>,
    pub validators: ReadValidators,
    pub rate: RateLimit,
}

impl<T> Refresh<T> {
    pub(super) fn from_headers(value: Option<T>, headers: &HeaderMap) -> Self {
        Self {
            value,
            validators: ReadValidators::from_headers(headers),
            rate: RateLimit::from_headers(headers),
        }
    }
}

pub struct UpdatedComment {
    pub issue: ItemNumber,
    pub comment: ConversationComment,
}

impl GitHubReader<'_> {
    /// Includes closed issues and merged PR issue representations.
    pub async fn updated_issues(
        &self,
        since_ms: i64,
        page: u32,
        validators: &ReadValidators,
        cancel: &CancellationToken,
    ) -> Result<Refresh<Page<IssueSummary>>, AccessError> {
        let since = since(since_ms)?;
        self.refresh_page::<wire::Issue, _>(
            "issues",
            page,
            &[
                ("state", "all"),
                ("sort", "updated"),
                ("direction", "asc"),
                ("since", &since),
            ],
            validators,
            cancel,
            |issue| issue.into_update(self),
        )
        .await
    }

    /// Ordinary issue and PR conversation edits, independent of the parent item's update time.
    pub async fn updated_comments(
        &self,
        since_ms: i64,
        page: u32,
        validators: &ReadValidators,
        cancel: &CancellationToken,
    ) -> Result<Refresh<Page<UpdatedComment>>, AccessError> {
        let since = since(since_ms)?;
        self.refresh_page::<wire::Comment, _>(
            "issues/comments",
            page,
            &[("sort", "updated"), ("direction", "asc"), ("since", &since)],
            validators,
            cancel,
            |comment| comment.into_update(self),
        )
        .await
    }

    pub async fn refresh_issue(
        &self,
        number: ItemNumber,
        validators: &ReadValidators,
        cancel: &CancellationToken,
    ) -> Result<Refresh<IssueSummary>, AccessError> {
        let path = self.permanent_path(&format!("issues/{}", number.get()));
        let response = self
            .get_refresh::<wire::Issue>(&path, validators.request_mode(), cancel)
            .await?;
        let issue = response
            .body
            .map(|wire| {
                let issue = wire.into_update(self)?;
                require_number(number, issue.number)?;
                Ok::<_, AccessError>(issue)
            })
            .transpose()?;
        Ok(Refresh::from_headers(issue, &response.headers))
    }

    pub async fn refresh_comments(
        &self,
        number: ItemNumber,
        page: u32,
        validators: &ReadValidators,
        cancel: &CancellationToken,
    ) -> Result<Refresh<Page<ConversationComment>>, AccessError> {
        let parent = format!("issues/{}", number.get());
        self.refresh_page::<wire::Comment, _>(
            &format!("{parent}/comments"),
            page,
            &[],
            validators,
            cancel,
            |comment| {
                let update = comment.into_update(self)?;
                require_number(number, update.issue)?;
                Ok(update.comment)
            },
        )
        .await
    }

    async fn refresh_page<T: DeserializeOwned, R>(
        &self,
        suffix: &str,
        page: u32,
        filters: &[(&str, &str)],
        validators: &ReadValidators,
        cancel: &CancellationToken,
        parse: impl Fn(T) -> Result<R, AccessError>,
    ) -> Result<Refresh<Page<R>>, AccessError> {
        let url = self.request(&self.permanent_path(suffix), page, filters)?;
        let mode = if page == 1 {
            validators.request_mode()
        } else {
            RequestMode::Read
        };
        let response = self
            .get_refresh::<Vec<T>>(url.as_str(), mode, cancel)
            .await?;
        let value = if let Some(records) = response.body {
            let next = next_page_for(&response.headers, &url, page, |path| {
                self.accepts_path(path, suffix)
            })?;
            Some(Page {
                records: records.into_iter().map(parse).collect::<Result<_, _>>()?,
                next_page: next.map(|number| number.get()),
                sso_limited: partial_sso(&response.headers).is_some(),
            })
        } else {
            None
        };
        Ok(Refresh::from_headers(value, &response.headers))
    }

    pub(super) fn permanent_path(&self, suffix: &str) -> String {
        format!("repositories/{}/{suffix}", self.repository.get())
    }

    pub(super) async fn get_refresh<T: DeserializeOwned>(
        &self,
        path: &str,
        mode: RequestMode<'_>,
        cancel: &CancellationToken,
    ) -> Result<ReadResponse<T>, AccessError> {
        let session = self.session;
        request(
            &session.client,
            &session.token,
            &session.scope,
            path,
            mode,
            cancel,
        )
        .await
    }

    pub(super) fn validate_named_url(&self, raw: &str, suffix: &str) -> Result<(), AccessError> {
        let error = || AccessError::received(AccessKind::IdentityMismatch, 200);
        let url = Url::parse(raw).map_err(|_| error())?;
        if !self.is_api_record_url(&url)
            || !url
                .path()
                .eq_ignore_ascii_case(&format!("/{}", self.path(suffix)))
        {
            return Err(error());
        }
        Ok(())
    }
}

fn since(since_ms: i64) -> Result<String, AccessError> {
    DateTime::from_timestamp_millis(since_ms)
        .map(|date| date.to_rfc3339_opts(SecondsFormat::Secs, true))
        .ok_or_else(|| AccessError::local(AccessKind::InvalidPagination))
}
