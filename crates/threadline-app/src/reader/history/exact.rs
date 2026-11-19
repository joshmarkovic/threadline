//! Evaluate a bounded exact comparison without reusing merge-base file data.

use super::*;
use std::{
    collections::{BTreeSet, VecDeque},
    future::Future,
};
use threadline_core::github::reader::{
    GitHubReader,
    history::{HistoryObjectKind, HistoryTree, HistoryTreeEntry, MAX_HISTORY_BLOB_BYTES},
};
use threadline_storage::pr::{PatchEvidence, PrRecord};

const MAX_TREE_REQUESTS: usize = 16;
const MAX_BLOB_REQUESTS: usize = 24;
const MAX_FILES: usize = 64;
const MAX_PATH_BYTES: usize = 4096;
const MAX_PATCH_BYTES: usize = 256 * 1024;

pub(super) struct ExactComparison {
    pub(super) records: Vec<PrRecord>,
    pub(super) limited: bool,
}

pub(super) struct Objects<'a, 'b> {
    pub(super) service: &'a ReaderService,
    pub(super) scope: &'a Scope,
    pub(super) reader: GitHubReader<'b>,
    pub(super) cancel: &'a Cancellation,
}

struct Directory {
    path: String,
    left: Option<String>,
    right: Option<String>,
}

type Sides = (Option<HistoryTreeEntry>, Option<HistoryTreeEntry>);

struct FileChange {
    path: String,
    sides: Sides,
}

impl FileChange {
    fn status(&self) -> &'static str {
        match (&self.sides.0, &self.sides.1) {
            (None, Some(_)) => "added",
            (Some(_), None) => "removed",
            _ => "modified",
        }
    }

    fn record(
        self,
        additions: Option<u64>,
        deletions: Option<u64>,
        patch: PatchEvidence,
    ) -> PrRecord {
        let status = self.status();
        PrRecord::File {
            path: self.path,
            previous_path: None,
            status: status.into(),
            additions,
            deletions,
            patch,
        }
    }
}

enum Blob {
    Bytes(Vec<u8>),
    Capped,
    Unavailable,
}

impl Objects<'_, '_> {
    async fn get<T>(
        &self,
        pending: impl Future<Output = Result<Refresh<T>, AccessError>>,
    ) -> Result<T, ReaderError> {
        self.service.check_scope(self.scope, self.cancel)?;
        let account = self.scope.profile.account;
        self.service.admit(account, self.cancel).await?;
        self.service.check_scope(self.scope, self.cancel)?;
        let result = self.service.history_value(account, pending.await).await?;
        self.service.check_scope(self.scope, self.cancel)?;
        Ok(result)
    }

    async fn tree(
        &self,
        oid: Option<&str>,
        requests: &mut usize,
    ) -> Result<Option<HistoryTree>, ReaderError> {
        let Some(oid) = oid else {
            return Ok(Some(HistoryTree {
                entries: Default::default(),
                truncated: false,
            }));
        };
        if *requests == MAX_TREE_REQUESTS {
            return Ok(None);
        }
        *requests += 1;
        match self.get(self.reader.history_tree(oid, self.cancel)).await {
            Ok(tree) if !tree.truncated => Ok(Some(tree)),
            Ok(_) => Ok(None),
            Err(ReaderError::Remote(error)) if unavailable(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn blob(
        &self,
        entry: Option<&HistoryTreeEntry>,
        requests: &mut usize,
    ) -> Result<Blob, ReaderError> {
        let Some(entry) = entry else {
            return Ok(Blob::Bytes(Vec::new()));
        };
        if matches!(
            entry.kind,
            HistoryObjectKind::Submodule | HistoryObjectKind::Symlink
        ) {
            return Ok(Blob::Unavailable);
        }
        let Some(size) = entry.size else {
            return Ok(Blob::Unavailable);
        };
        if size > MAX_HISTORY_BLOB_BYTES as u64 || *requests == MAX_BLOB_REQUESTS {
            return Ok(Blob::Capped);
        }
        *requests += 1;
        match self
            .get(self.reader.history_blob(&entry.oid, size, self.cancel))
            .await
        {
            Ok(bytes) => Ok(Blob::Bytes(bytes)),
            Err(ReaderError::Remote(error)) if error.kind == AccessKind::ResponseTooLarge => {
                Ok(Blob::Capped)
            }
            Err(ReaderError::Remote(error)) if error.kind == AccessKind::MissingOrInaccessible => {
                Ok(Blob::Unavailable)
            }
            Err(error) => Err(error),
        }
    }

    pub(super) async fn compare(
        &self,
        base: &str,
        head: &str,
    ) -> Result<ExactComparison, ReaderError> {
        let left = self
            .get(self.reader.history_commit_tree(base, self.cancel))
            .await?;
        let right = self
            .get(self.reader.history_commit_tree(head, self.cancel))
            .await?;
        let mut queue = VecDeque::from([Directory {
            path: String::new(),
            left: Some(left),
            right: Some(right),
        }]);
        let mut tree_requests = 0;
        let mut changes = Vec::new();
        let mut limited = false;
        while let Some(directory) = queue.pop_front() {
            if directory.left == directory.right {
                continue;
            }
            let left = self
                .tree(directory.left.as_deref(), &mut tree_requests)
                .await?;
            let Some(left) = left else {
                limited = true;
                continue;
            };
            let right = self
                .tree(directory.right.as_deref(), &mut tree_requests)
                .await?;
            let Some(right) = right else {
                limited = true;
                continue;
            };
            let names = left
                .entries
                .keys()
                .chain(right.entries.keys())
                .collect::<BTreeSet<_>>();
            for name in names {
                let left = left.entries.get(name);
                let right = right.entries.get(name);
                if left
                    .zip(right)
                    .is_some_and(|(left, right)| left.oid == right.oid && left.kind == right.kind)
                {
                    continue;
                }
                let length =
                    directory.path.len() + usize::from(!directory.path.is_empty()) + name.len();
                if length > MAX_PATH_BYTES {
                    limited = true;
                    continue;
                }
                let path = if directory.path.is_empty() {
                    name.clone()
                } else {
                    format!("{}/{name}", directory.path)
                };
                let left_directory =
                    left.filter(|entry| entry.kind == HistoryObjectKind::Directory);
                let right_directory =
                    right.filter(|entry| entry.kind == HistoryObjectKind::Directory);
                if left_directory.is_some() || right_directory.is_some() {
                    if queue.len() >= MAX_TREE_REQUESTS {
                        limited = true;
                    } else {
                        queue.push_back(Directory {
                            path: path.clone(),
                            left: left_directory.map(|entry| entry.oid.clone()),
                            right: right_directory.map(|entry| entry.oid.clone()),
                        });
                    }
                }
                let left = left.filter(|entry| entry.kind != HistoryObjectKind::Directory);
                let right = right.filter(|entry| entry.kind != HistoryObjectKind::Directory);
                if left.is_some() || right.is_some() {
                    if changes.len() == MAX_FILES {
                        limited = true;
                    } else {
                        changes.push(FileChange {
                            path,
                            sides: (left.cloned(), right.cloned()),
                        });
                    }
                }
            }
        }

        let mut blob_requests = 0;
        let mut records = Vec::with_capacity(changes.len());
        let mut text = Vec::new();
        for change in changes {
            let (left, right) = (&change.sides.0, &change.sides.1);
            if left
                .as_ref()
                .zip(right.as_ref())
                .is_some_and(|(left, right)| left.oid == right.oid)
            {
                records.push(change.record(Some(0), Some(0), PatchEvidence::Text(String::new())));
                continue;
            }
            let left = self.blob(left.as_ref(), &mut blob_requests).await?;
            let right = self.blob(right.as_ref(), &mut blob_requests).await?;
            match (left, right) {
                (Blob::Bytes(left), Blob::Bytes(right)) => text.push((change, left, right)),
                (Blob::Capped, _) | (_, Blob::Capped) => {
                    limited = true;
                    records.push(change.record(None, None, PatchEvidence::ApiCapped));
                }
                _ => {
                    limited = true;
                    records.push(change.record(None, None, PatchEvidence::Unavailable));
                }
            }
        }
        let (text, patch_limited) = tokio::task::spawn_blocking(move || {
            let mut limited = false;
            let records = text
                .into_iter()
                .map(|(change, left, right)| {
                    let (record, partial) = text_patch(change, &left, &right);
                    limited |= partial;
                    record
                })
                .collect::<Vec<_>>();
            (records, limited)
        })
        .await
        .map_err(|_| ReaderError::Cancelled)?;
        records.extend(text);
        records.sort_by(|left, right| file_path(left).cmp(file_path(right)));
        Ok(ExactComparison {
            records,
            limited: limited || patch_limited,
        })
    }
}

fn unavailable(error: &AccessError) -> bool {
    matches!(
        error.kind,
        AccessKind::MissingOrInaccessible | AccessKind::ResponseTooLarge
    )
}

fn file_path(record: &PrRecord) -> &str {
    match record {
        PrRecord::File { path, .. } => path,
        _ => unreachable!("exact history emits file records"),
    }
}

fn text_patch(change: FileChange, left: &[u8], right: &[u8]) -> (PrRecord, bool) {
    use similar::{Algorithm, ChangeTag, TextDiff};
    let (Ok(left_text), Ok(right_text)) = (std::str::from_utf8(left), std::str::from_utf8(right))
    else {
        return (change.record(None, None, PatchEvidence::Binary), false);
    };
    if left.contains(&0) || right.contains(&0) {
        return (change.record(None, None, PatchEvidence::Binary), false);
    }
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Myers)
        .timeout(std::time::Duration::from_millis(75))
        .diff_lines(left_text, right_text);
    let (mut added, mut removed) = (0, 0);
    for line in diff.iter_all_changes() {
        match line.tag() {
            ChangeTag::Insert => added += 1,
            ChangeTag::Delete => removed += 1,
            ChangeTag::Equal => (),
        }
    }
    let patch = diff.unified_diff().context_radius(3).to_string();
    if patch.len() > MAX_PATCH_BYTES {
        return (
            change.record(Some(added), Some(removed), PatchEvidence::ApiCapped),
            true,
        );
    }
    (
        change.record(Some(added), Some(removed), PatchEvidence::Text(patch)),
        false,
    )
}
