use std::num::NonZeroU64;

use serde::Deserialize;

use super::{
    AccessError, COMPARE_FILES_CAP, PatchEvidence, PrCheck, PrCommit, PrComparisonIdentity,
    PrComparisonPage, PrFile, PrFiles, PrPage, PrStatus, PullRequestSummary, delivered,
    identity_mismatch, malformed,
};
use crate::github::access::reader::wire::{millis, oid};

#[derive(Deserialize)]
pub(super) struct Comparison {
    base_commit: CommitId,
    merge_base_commit: CommitId,
    total_commits: u64,
    commits: Vec<Commit>,
    files: Option<Vec<File>>,
}

#[derive(Deserialize)]
struct CommitId {
    sha: String,
}

impl Comparison {
    pub(super) fn read(
        self,
        pull: &PullRequestSummary,
        page: u32,
        next: Option<u32>,
        limited: bool,
    ) -> Result<PrComparisonPage, AccessError> {
        if oid(self.base_commit.sha)? != pull.base_oid {
            return Err(identity_mismatch());
        }
        let merge_base = oid(self.merge_base_commit.sha)?;
        let commits = self
            .commits
            .into_iter()
            .map(Commit::read)
            .collect::<Result<Vec<_>, _>>()?;
        let partial =
            limited || next.is_none() && self.total_commits > delivered(page, commits.len());
        if !partial
            && next.is_none()
            && self.total_commits != 0
            && commits
                .last()
                .is_none_or(|commit| commit.oid != pull.head_oid)
        {
            return Err(identity_mismatch());
        }
        let files = if page == 1 {
            Some(self.files.map_or_else(
                || {
                    Ok(PrFiles {
                        records: Vec::new(),
                        partial: pull.presentation.changed_files != Some(0) || limited,
                    })
                },
                |files| {
                    let count = files.len();
                    let records = files
                        .into_iter()
                        .map(File::read)
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok::<_, AccessError>(PrFiles {
                        records,
                        partial: limited
                            || count >= COMPARE_FILES_CAP
                            || pull
                                .presentation
                                .changed_files
                                .is_some_and(|total| total > count as u64),
                    })
                },
            )?)
        } else {
            None
        };
        Ok(PrComparisonPage {
            identity: PrComparisonIdentity {
                pull_node: pull.node_id.clone(),
                base: pull.base_oid.clone(),
                head: pull.head_oid.clone(),
                merge_base,
            },
            commits: PrPage {
                records: commits,
                next_page: next,
                partial,
            },
            files,
        })
    }
}

#[derive(Deserialize)]
struct Commit {
    sha: String,
    commit: CommitMetadata,
    author: Option<Login>,
}

#[derive(Deserialize)]
struct Login {
    login: String,
}

#[derive(Deserialize)]
struct CommitMetadata {
    message: String,
    author: Option<CommitPerson>,
    committer: Option<CommitPerson>,
}

#[derive(Deserialize)]
struct CommitPerson {
    name: Option<String>,
    date: Option<String>,
}

impl Commit {
    fn read(self) -> Result<PrCommit, AccessError> {
        let authored_at_ms = self
            .commit
            .author
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
        Ok(PrCommit {
            oid: oid(self.sha)?,
            message: self.commit.message,
            author: self
                .author
                .map(|user| user.login)
                .or_else(|| self.commit.author.and_then(|person| person.name)),
            authored_at_ms,
            committed_at_ms,
        })
    }
}

#[derive(Deserialize)]
struct File {
    filename: String,
    previous_filename: Option<String>,
    status: String,
    additions: Option<u64>,
    deletions: Option<u64>,
    patch: Option<String>,
}

impl File {
    fn read(self) -> Result<PrFile, AccessError> {
        let path = checked_path(self.filename)?;
        let previous_path = self.previous_filename.map(checked_path).transpose()?;
        let patch = match self.patch {
            None => PatchEvidence::Unavailable,
            Some(text)
                if text.lines().any(|line| {
                    line == "GIT binary patch"
                        || line.starts_with("Binary files ") && line.ends_with(" differ")
                }) =>
            {
                PatchEvidence::Binary
            }
            Some(text)
                if patch_counts(&text).is_some_and(|(plus, minus)| {
                    self.additions.is_none_or(|expected| plus == expected)
                        && self.deletions.is_none_or(|expected| minus == expected)
                }) =>
            {
                PatchEvidence::Text(text)
            }
            Some(text) => PatchEvidence::Truncated(text),
        };
        Ok(PrFile {
            path,
            previous_path,
            status: safe_code(self.status)?,
            additions: self.additions,
            deletions: self.deletions,
            patch,
        })
    }
}

fn patch_counts(text: &str) -> Option<(u64, u64)> {
    let mut hunk = false;
    let mut added = 0;
    let mut removed = 0;
    for line in text.lines() {
        if line.starts_with("@@ ") {
            hunk = true;
        } else if hunk && line.starts_with('+') {
            added += 1;
        } else if hunk && line.starts_with('-') {
            removed += 1;
        }
    }
    hunk.then_some((added, removed))
}

pub(super) fn checked_path(value: String) -> Result<String, AccessError> {
    if value.len() > 4096
        || value.chars().any(char::is_control)
        || value.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(malformed());
    }
    Ok(value)
}

fn safe_code(value: String) -> Result<String, AccessError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'-'))
    {
        return Err(malformed());
    }
    Ok(value)
}

#[derive(Deserialize)]
pub(super) struct Checks {
    pub(super) total_count: u64,
    pub(super) check_runs: Vec<Check>,
}

#[derive(Deserialize)]
pub(super) struct Check {
    id: NonZeroU64,
    head_sha: String,
    name: String,
    status: String,
    conclusion: Option<String>,
    completed_at: Option<String>,
}

impl Check {
    pub(super) fn read(self, expected_head: &str) -> Result<PrCheck, AccessError> {
        let head_oid = oid(self.head_sha)?;
        if head_oid != expected_head {
            return Err(identity_mismatch());
        }
        Ok(PrCheck {
            id: self.id.get(),
            head_oid,
            name: self.name,
            status: safe_code(self.status)?,
            conclusion: self.conclusion.map(safe_code).transpose()?,
            completed_at_ms: self.completed_at.as_deref().map(millis).transpose()?,
        })
    }
}

#[derive(Deserialize)]
pub(super) struct Status {
    id: NonZeroU64,
    context: String,
    state: String,
    updated_at: Option<String>,
}

impl Status {
    pub(super) fn read(self, head: &str) -> Result<PrStatus, AccessError> {
        Ok(PrStatus {
            id: self.id.get(),
            head_oid: head.into(),
            context: self.context,
            state: safe_code(self.state)?,
            updated_at_ms: self.updated_at.as_deref().map(millis).transpose()?,
        })
    }
}
