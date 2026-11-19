//! Read PR data against the verified repository and provider-observed revision.

pub(super) mod graph;
mod source;
pub(super) mod wire;

pub use graph::{PrGraphPage, PrInlineComment, PrReview, PrReviewAnchor, PrSide, PrThread};
pub use source::{PrSource, PrSourceRange, SourceUnavailable};

use serde::de::DeserializeOwned;

use super::{
    AccessError, AccessKind, CancellationToken, GitHubReader, ItemNumber, PullRequestSummary,
    RepositoryApiId, RepositoryName, malformed, next_page_for, partial_sso,
    refresh::{ReadValidators, Refresh},
    require_number,
};
use crate::github::access::response::{ReadResponse, RequestMode};

const COMPARE_FILES_CAP: usize = 300;

pub struct PrSnapshot {
    repository: RepositoryApiId,
    route: RepositoryName,
    details: PullRequestSummary,
}

impl PrSnapshot {
    pub fn details(&self) -> &PullRequestSummary {
        &self.details
    }
}

pub struct PrComparisonPage {
    identity: PrComparisonIdentity,
    pub commits: PrPage<PrCommit>,
    /// GitHub supplies changed files on the first comparison page only.
    pub files: Option<PrFiles>,
}

impl PrComparisonPage {
    pub fn merge_base(&self) -> &str {
        self.identity.merge_base()
    }
    pub fn identity(&self) -> &PrComparisonIdentity {
        &self.identity
    }
}

#[derive(Clone)]
pub struct PrComparisonIdentity {
    pull_node: String,
    base: String,
    head: String,
    merge_base: String,
}

impl PrComparisonIdentity {
    pub fn merge_base(&self) -> &str {
        &self.merge_base
    }
}

pub struct PrPage<T> {
    pub records: Vec<T>,
    pub next_page: Option<u32>,
    pub partial: bool,
}

pub struct PrFiles {
    pub records: Vec<PrFile>,
    pub partial: bool,
}

pub struct PrCommit {
    pub oid: String,
    pub message: String,
    pub author: Option<String>,
    pub authored_at_ms: Option<i64>,
    pub committed_at_ms: Option<i64>,
}

pub struct PrFile {
    pub path: String,
    pub previous_path: Option<String>,
    pub status: String,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub patch: PatchEvidence,
}

#[derive(Eq, PartialEq)]
pub enum PatchEvidence {
    Text(String),
    Truncated(String),
    Binary,
    Unavailable,
}

pub struct PrCheck {
    pub id: u64,
    pub head_oid: String,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub completed_at_ms: Option<i64>,
}

pub struct PrStatus {
    pub id: u64,
    pub head_oid: String,
    pub context: String,
    pub state: String,
    pub updated_at_ms: Option<i64>,
}

impl GitHubReader<'_> {
    pub async fn pr_snapshot(
        &self,
        number: ItemNumber,
        validators: &ReadValidators,
        cancel: &CancellationToken,
    ) -> Result<Refresh<PrSnapshot>, AccessError> {
        let path = self.permanent_path(&format!("pulls/{}", number.get()));
        let response = self
            .get_refresh::<super::wire::PullRequest>(&path, validators.request_mode(), cancel)
            .await?;
        let snapshot = response
            .body
            .map(|wire| {
                let details = wire.into_summary()?;
                require_number(number, details.number)?;
                if details.presentation.base_repository_id != Some(self.repository) {
                    return Err(identity_mismatch());
                }
                Ok(PrSnapshot {
                    repository: self.repository,
                    route: self.route.clone(),
                    details,
                })
            })
            .transpose()?;
        Ok(Refresh::from_headers(snapshot, &response.headers))
    }

    /// GitHub's three-dot compare computes the merge base from the verified base/head objects.
    pub async fn pr_comparison(
        &self,
        snapshot: &PrSnapshot,
        page: u32,
        validators: &ReadValidators,
        cancel: &CancellationToken,
    ) -> Result<Refresh<PrComparisonPage>, AccessError> {
        let pr = self.require_snapshot(snapshot)?;
        let suffix = format!("compare/{}...{}", pr.base_oid, pr.head_oid);
        let (response, next) = self
            .pr_page::<wire::Comparison>(&suffix, page, &[], validators, cancel)
            .await?;
        let limited = partial_sso(&response.headers).is_some();
        let value = response
            .body
            .map(|wire| wire.read(pr, page, next, limited))
            .transpose()?;
        Ok(Refresh::from_headers(value, &response.headers))
    }

    pub async fn pr_checks(
        &self,
        snapshot: &PrSnapshot,
        page: u32,
        validators: &ReadValidators,
        cancel: &CancellationToken,
    ) -> Result<Refresh<PrPage<PrCheck>>, AccessError> {
        let pr = self.require_snapshot(snapshot)?;
        let suffix = format!("commits/{}/check-runs", pr.head_oid);
        let (response, next) = self
            .pr_page::<wire::Checks>(&suffix, page, &[("filter", "all")], validators, cancel)
            .await?;
        let partial = partial_sso(&response.headers).is_some();
        let checks = response
            .body
            .map(|wire| {
                let records = wire
                    .check_runs
                    .into_iter()
                    .map(|run| run.read(&pr.head_oid))
                    .collect::<Result<Vec<_>, _>>()?;
                let total = wire.total_count;
                Ok::<_, AccessError>(PrPage {
                    partial: partial || next.is_none() && total > delivered(page, records.len()),
                    records,
                    next_page: next,
                })
            })
            .transpose()?;
        Ok(Refresh::from_headers(checks, &response.headers))
    }

    pub async fn pr_statuses(
        &self,
        snapshot: &PrSnapshot,
        page: u32,
        validators: &ReadValidators,
        cancel: &CancellationToken,
    ) -> Result<Refresh<PrPage<PrStatus>>, AccessError> {
        let pr = self.require_snapshot(snapshot)?;
        let suffix = format!("commits/{}/statuses", pr.head_oid);
        let (response, next) = self
            .pr_page::<Vec<wire::Status>>(&suffix, page, &[], validators, cancel)
            .await?;
        let partial = partial_sso(&response.headers).is_some();
        let statuses = response
            .body
            .map(|records| {
                let records = records
                    .into_iter()
                    .map(|status| status.read(&pr.head_oid))
                    .collect::<Result<_, _>>()?;
                Ok::<_, AccessError>(PrPage {
                    records,
                    next_page: next,
                    partial,
                })
            })
            .transpose()?;
        Ok(Refresh::from_headers(statuses, &response.headers))
    }

    fn require_snapshot<'a>(
        &self,
        snapshot: &'a PrSnapshot,
    ) -> Result<&'a PullRequestSummary, AccessError> {
        if snapshot.repository != self.repository || snapshot.route != self.route {
            return Err(AccessError::local(AccessKind::WrongScope));
        }
        Ok(&snapshot.details)
    }

    pub(super) async fn pr_page<T: DeserializeOwned>(
        &self,
        suffix: &str,
        page: u32,
        filters: &[(&str, &str)],
        validators: &ReadValidators,
        cancel: &CancellationToken,
    ) -> Result<(ReadResponse<T>, Option<u32>), AccessError> {
        let url = self.request(&self.permanent_path(suffix), page, filters)?;
        let mode = if page == 1 {
            validators.request_mode()
        } else {
            RequestMode::Read
        };
        let response = self.get_refresh(url.as_str(), mode, cancel).await?;
        let next = next_page_for(&response.headers, &url, page, |path| {
            self.accepts_path(path, suffix)
        })?
        .map(|n| n.get());
        Ok((response, next))
    }
}

fn delivered(page: u32, count: usize) -> u64 {
    u64::from(page.saturating_sub(1)) * u64::from(super::PAGE_SIZE) + count as u64
}

pub(super) fn identity_mismatch() -> AccessError {
    AccessError::received(AccessKind::IdentityMismatch, 200)
}
