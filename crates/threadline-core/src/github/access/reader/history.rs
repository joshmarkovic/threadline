//! Read repository history from a verified repository and pin all continuation to commit IDs.
use super::{
    AccessError, AccessKind, CancellationToken, GitHubReader, ItemNumber, Url, malformed,
    partial_sso,
    pr::{PrFile, wire::File},
    refresh::{ReadValidators, Refresh},
    wire::{millis, oid},
};
use crate::github::access::response::RequestMode;
use serde::Deserialize;
use std::num::NonZeroU64;

mod objects;
pub use objects::*;

#[derive(Clone, Copy)]
pub enum HistoryRefKind {
    Branch,
    Tag,
}

pub struct RemoteRef {
    pub name: String,
    pub oid: String,
}

pub struct HistoryPage<T> {
    pub records: Vec<T>,
    pub next_page: Option<u32>,
    pub limited: bool,
}

pub struct HistoryCommit {
    pub oid: String,
    pub parents: Vec<String>,
    pub message: String,
    pub author: Option<String>,
    pub authored_at_ms: Option<i64>,
    pub committed_at_ms: Option<i64>,
}

#[derive(Clone, Copy)]
pub enum HistoryDiff<'a> {
    Root { head: &'a str },
    Direct { base: &'a str, head: &'a str },
    MergeBase { base: &'a str, head: &'a str },
}

pub struct HistoryFiles {
    pub effective_base: Option<String>,
    pub page: HistoryPage<PrFile>,
}

#[derive(Deserialize)]
struct RefWire {
    name: String,
    commit: Sha,
}

#[derive(Deserialize)]
struct Sha {
    sha: String,
}

#[derive(Deserialize)]
struct RefObject {
    #[serde(rename = "ref")]
    name: String,
    object: GitObject,
}

#[derive(Deserialize)]
struct GitObject {
    #[serde(rename = "type")]
    kind: String,
    sha: String,
}

#[derive(Deserialize)]
struct AnnotatedTag {
    sha: String,
    object: GitObject,
}

#[derive(Deserialize)]
struct CommitWire {
    sha: String,
    commit: Metadata,
    parents: Vec<Sha>,
    files: Option<Vec<File>>,
}

#[derive(Deserialize)]
struct Metadata {
    message: String,
    author: Option<Person>,
    committer: Option<Person>,
}

#[derive(Deserialize)]
struct Person {
    name: Option<String>,
    date: Option<String>,
}

#[derive(Deserialize)]
struct Comparison {
    base_commit: Sha,
    merge_base_commit: Sha,
    files: Option<Vec<File>>,
}

fn mismatch() -> AccessError {
    AccessError::received(AccessKind::IdentityMismatch, 200)
}

fn check_ref(name: &str) -> Result<(), AccessError> {
    if name.is_empty()
        || name.len() > 1024
        || name == "@"
        || name.ends_with('.')
        || name.contains("..")
        || name.contains("@{")
        || name
            .split('/')
            .any(|s| s.is_empty() || s.starts_with('.') || s.ends_with(".lock"))
        || name
            .bytes()
            .any(|b| b <= 32 || b == 127 || b"~^:?*[\\".contains(&b))
    {
        return Err(AccessError::local(AccessKind::WrongScope));
    }
    Ok(())
}

fn segment_path(prefix: &str, name: &str) -> String {
    let mut url = Url::parse("https://api.github.com").expect("GitHub API URL");
    url.set_path(prefix);
    url.path_segments_mut().expect("GitHub API path").push(name);
    url.path()[1..].into()
}

impl RefWire {
    fn read(self) -> Result<RemoteRef, AccessError> {
        check_ref(&self.name).map_err(|_| malformed())?;
        Ok(RemoteRef {
            name: self.name,
            oid: oid(self.commit.sha)?,
        })
    }
}

impl CommitWire {
    fn read(self) -> Result<HistoryCommit, AccessError> {
        let author = self.commit.author;
        let authored_at_ms = author
            .as_ref()
            .and_then(|person| person.date.as_deref())
            .map(millis)
            .transpose()?;
        let committed_at_ms = self
            .commit
            .committer
            .as_ref()
            .and_then(|person| person.date.as_deref())
            .map(millis)
            .transpose()?;
        Ok(HistoryCommit {
            oid: oid(self.sha)?,
            parents: self
                .parents
                .into_iter()
                .map(|parent| oid(parent.sha))
                .collect::<Result<_, _>>()?,
            message: self.commit.message,
            author: author.and_then(|person| person.name),
            authored_at_ms,
            committed_at_ms,
        })
    }
}

impl GitHubReader<'_> {
    pub async fn history_refs(
        &self,
        kind: HistoryRefKind,
        page: u32,
        cancel: &CancellationToken,
    ) -> Result<Refresh<HistoryPage<RemoteRef>>, AccessError> {
        let suffix = match kind {
            HistoryRefKind::Branch => "branches",
            HistoryRefKind::Tag => "tags",
        };
        let (response, next_page) = self
            .pr_page::<Vec<RefWire>>(suffix, page, &[], &ReadValidators::default(), cancel)
            .await?;
        let (rows, headers) = response.require_body()?;
        let limited = partial_sso(&headers).is_some();
        let records = rows
            .into_iter()
            .map(RefWire::read)
            .collect::<Result<_, _>>()?;
        Ok(Refresh::from_headers(
            Some(HistoryPage {
                records,
                next_page,
                limited,
            }),
            &headers,
        ))
    }

    pub async fn history_branch(
        &self,
        name: &str,
        cancel: &CancellationToken,
    ) -> Result<Refresh<RemoteRef>, AccessError> {
        check_ref(name)?;
        let suffix = segment_path("branches", name);
        let (row, headers) = self
            .get_refresh::<RefWire>(&self.permanent_path(&suffix), RequestMode::Read, cancel)
            .await?
            .require_body()?;
        let branch = row.read()?;
        if branch.name != name {
            return Err(mismatch());
        }
        Ok(Refresh::from_headers(Some(branch), &headers))
    }

    /// Observe an exact tag ref. The caller resolves annotated object IDs through the same repository.
    pub async fn history_tag_ref(
        &self,
        name: &str,
        cancel: &CancellationToken,
    ) -> Result<Refresh<HistoryTagObject>, AccessError> {
        check_ref(name)?;
        let suffix = segment_path("git/ref/tags", name);
        let (row, headers) = self
            .get_refresh::<RefObject>(&self.permanent_path(&suffix), RequestMode::Read, cancel)
            .await?
            .require_body()?;
        if row.name != format!("refs/tags/{name}") {
            return Err(mismatch());
        }
        Ok(Refresh::from_headers(Some(row.object.read()?), &headers))
    }

    pub async fn history_annotated_tag(
        &self,
        tag: &str,
        cancel: &CancellationToken,
    ) -> Result<Refresh<HistoryTagObject>, AccessError> {
        let tag = pin(tag)?;
        let (row, headers) = self
            .get_refresh::<AnnotatedTag>(
                &self.permanent_path(&format!("git/tags/{tag}")),
                RequestMode::Read,
                cancel,
            )
            .await?
            .require_body()?;
        if oid(row.sha)? != tag {
            return Err(mismatch());
        }
        Ok(Refresh::from_headers(Some(row.object.read()?), &headers))
    }

    pub async fn history_pull_tip(
        &self,
        number: ItemNumber,
        expected: NonZeroU64,
        cancel: &CancellationToken,
    ) -> Result<Refresh<String>, AccessError> {
        let response = self
            .pr_snapshot(number, &ReadValidators::default(), cancel)
            .await?;
        Ok(Refresh {
            value: response
                .value
                .map(|pull| {
                    let facts = pull.details();
                    if facts.pull_request_id != expected {
                        return Err(mismatch());
                    }
                    Ok(facts.head_oid.clone())
                })
                .transpose()?,
            validators: response.validators,
            rate: response.rate,
        })
    }

    pub async fn history_commits(
        &self,
        tip: &str,
        page: u32,
        cancel: &CancellationToken,
    ) -> Result<Refresh<HistoryPage<HistoryCommit>>, AccessError> {
        let tip = pin(tip)?;
        let (response, next_page) = self
            .pr_page::<Vec<CommitWire>>(
                "commits",
                page,
                &[("sha", &tip)],
                &ReadValidators::default(),
                cancel,
            )
            .await?;
        let (rows, headers) = response.require_body()?;
        let records = rows
            .into_iter()
            .map(CommitWire::read)
            .collect::<Result<Vec<_>, _>>()?;
        if page == 1 && records.first().is_none_or(|commit| commit.oid != tip) {
            return Err(mismatch());
        }
        let limited = partial_sso(&headers).is_some();
        Ok(Refresh::from_headers(
            Some(HistoryPage {
                records,
                next_page,
                limited,
            }),
            &headers,
        ))
    }

    pub async fn history_files(
        &self,
        diff: HistoryDiff<'_>,
        page: u32,
        cancel: &CancellationToken,
    ) -> Result<Refresh<HistoryFiles>, AccessError> {
        match diff {
            HistoryDiff::Root { head } => self.history_root_files(&pin(head)?, page, cancel).await,
            HistoryDiff::Direct { base, head } => {
                self.history_compare_files(&pin(base)?, &pin(head)?, true, page, cancel)
                    .await
            }
            HistoryDiff::MergeBase { base, head } => {
                self.history_compare_files(&pin(base)?, &pin(head)?, false, page, cancel)
                    .await
            }
        }
    }

    async fn history_root_files(
        &self,
        head: &str,
        page: u32,
        cancel: &CancellationToken,
    ) -> Result<Refresh<HistoryFiles>, AccessError> {
        let (response, next_page) = self
            .pr_page::<CommitWire>(
                &format!("commits/{head}"),
                page,
                &[],
                &ReadValidators::default(),
                cancel,
            )
            .await?;
        let (row, headers) = response.require_body()?;
        if oid(row.sha)? != head || !row.parents.is_empty() {
            return Err(mismatch());
        }
        let files_known = row.files.is_some();
        let records = files(row.files)?;
        let limited = partial_sso(&headers).is_some()
            || !files_known
            || next_page.is_none()
                && (page as usize - 1) * super::PAGE_SIZE as usize + records.len() >= 3000;
        let page = HistoryPage {
            records,
            next_page,
            limited,
        };
        Ok(Refresh::from_headers(
            Some(HistoryFiles {
                effective_base: None,
                page,
            }),
            &headers,
        ))
    }

    async fn history_compare_files(
        &self,
        base: &str,
        head: &str,
        direct: bool,
        page: u32,
        cancel: &CancellationToken,
    ) -> Result<Refresh<HistoryFiles>, AccessError> {
        if page != 1 {
            return Err(AccessError::local(AccessKind::InvalidPagination));
        }
        let (response, _) = self
            .pr_page::<Comparison>(
                &format!("compare/{base}...{head}"),
                page,
                &[],
                &ReadValidators::default(),
                cancel,
            )
            .await?;
        let (row, headers) = response.require_body()?;
        if oid(row.base_commit.sha)? != base {
            return Err(mismatch());
        }
        let merge_base = oid(row.merge_base_commit.sha)?;
        let compatible = !direct || merge_base == base;
        let files_known = row.files.is_some();
        let records = if compatible {
            files(row.files)?
        } else {
            Vec::new()
        };
        let limited =
            !compatible || !files_known || records.len() >= 300 || partial_sso(&headers).is_some();
        let page = HistoryPage {
            records,
            next_page: None,
            limited,
        };
        let effective_base = compatible.then_some(merge_base);
        Ok(Refresh::from_headers(
            Some(HistoryFiles {
                effective_base,
                page,
            }),
            &headers,
        ))
    }
}

fn files(rows: Option<Vec<File>>) -> Result<Vec<PrFile>, AccessError> {
    rows.unwrap_or_default()
        .into_iter()
        .map(File::read)
        .collect()
}

fn pin(value: &str) -> Result<String, AccessError> {
    oid(value.into()).map_err(|_| AccessError::local(AccessKind::WrongScope))
}

pub enum HistoryTagObject {
    Commit(String),
    Tag(String),
}
impl GitObject {
    fn read(self) -> Result<HistoryTagObject, AccessError> {
        let oid = oid(self.sha)?;
        match self.kind.as_str() {
            "commit" => Ok(HistoryTagObject::Commit(oid)),
            "tag" => Ok(HistoryTagObject::Tag(oid)),
            _ => Err(malformed()),
        }
    }
}
