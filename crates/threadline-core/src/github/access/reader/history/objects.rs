//! Immutable Git objects for an exact two-endpoint comparison.

use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::BTreeMap;

pub const MAX_HISTORY_BLOB_BYTES: usize = 64 * 1024;
const MAX_TREE_ENTRIES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryObjectKind {
    Directory,
    File,
    Executable,
    Symlink,
    Submodule,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryTreeEntry {
    pub oid: String,
    pub kind: HistoryObjectKind,
    pub size: Option<u64>,
}

pub struct HistoryTree {
    pub entries: BTreeMap<String, HistoryTreeEntry>,
    pub truncated: bool,
}

#[derive(Deserialize)]
struct CommitTree {
    sha: String,
    tree: Sha,
}

#[derive(Deserialize)]
struct TreeWire {
    sha: String,
    tree: Vec<TreeEntryWire>,
    truncated: bool,
}

#[derive(Deserialize)]
struct TreeEntryWire {
    path: String,
    mode: String,
    #[serde(rename = "type")]
    kind: String,
    size: Option<u64>,
    sha: String,
}

#[derive(Deserialize)]
struct BlobWire {
    sha: String,
    content: String,
    encoding: String,
    size: u64,
}

fn read_entry(object: TreeEntryWire) -> Result<(String, HistoryTreeEntry), AccessError> {
    use HistoryObjectKind::*;
    if object.path.is_empty()
        || object.path.len() > 1024
        || matches!(object.path.as_str(), "." | "..")
        || object.path.contains(['/', '\0'])
    {
        return Err(malformed());
    }
    let kind = match (object.kind.as_str(), object.mode.as_str()) {
        ("tree", "040000") => Directory,
        ("blob", "100644") => File,
        ("blob", "100755") => Executable,
        ("blob", "120000") => Symlink,
        ("commit", "160000") => Submodule,
        _ => return Err(malformed()),
    };
    Ok((
        object.path,
        HistoryTreeEntry {
            oid: oid(object.sha)?,
            kind,
            size: object.size,
        },
    ))
}

impl GitHubReader<'_> {
    pub async fn history_commit_tree(
        &self,
        commit: &str,
        cancel: &CancellationToken,
    ) -> Result<Refresh<String>, AccessError> {
        let commit = pin(commit)?;
        let (row, headers) = self
            .get_refresh::<CommitTree>(
                &self.permanent_path(&format!("git/commits/{commit}")),
                RequestMode::Read,
                cancel,
            )
            .await?
            .require_body()?;
        if oid(row.sha)? != commit {
            return Err(mismatch());
        }
        Ok(Refresh::from_headers(Some(oid(row.tree.sha)?), &headers))
    }

    /// An exact, non-recursive Git tree. An incomplete tree never proves a missing path.
    pub async fn history_tree(
        &self,
        tree: &str,
        cancel: &CancellationToken,
    ) -> Result<Refresh<HistoryTree>, AccessError> {
        let tree = pin(tree)?;
        let (row, headers) = self
            .get_refresh::<TreeWire>(
                &self.permanent_path(&format!("git/trees/{tree}")),
                RequestMode::Read,
                cancel,
            )
            .await?
            .require_body()?;
        if oid(row.sha)? != tree {
            return Err(mismatch());
        }
        let truncated =
            row.truncated || row.tree.len() > MAX_TREE_ENTRIES || partial_sso(&headers).is_some();
        let mut entries = BTreeMap::new();
        if !truncated {
            for raw in row.tree {
                let (path, entry) = read_entry(raw)?;
                if entries.insert(path, entry).is_some() {
                    return Err(malformed());
                }
            }
        }
        Ok(Refresh::from_headers(
            Some(HistoryTree { entries, truncated }),
            &headers,
        ))
    }

    pub async fn history_blob(
        &self,
        blob: &str,
        expected_size: u64,
        cancel: &CancellationToken,
    ) -> Result<Refresh<Vec<u8>>, AccessError> {
        let blob = pin(blob)?;
        if expected_size > MAX_HISTORY_BLOB_BYTES as u64 {
            return Err(AccessError::local(AccessKind::WrongScope));
        }
        let (row, headers) = self
            .get_refresh::<BlobWire>(
                &self.permanent_path(&format!("git/blobs/{blob}")),
                RequestMode::Read,
                cancel,
            )
            .await?
            .require_body()?;
        if oid(row.sha)? != blob || row.size != expected_size {
            return Err(mismatch());
        }
        if row.encoding != "base64" || row.content.len() > MAX_HISTORY_BLOB_BYTES * 2 {
            return Err(malformed());
        }
        let encoded: Vec<_> = row
            .content
            .bytes()
            .filter(|value| !matches!(value, b'\r' | b'\n'))
            .collect();
        let decoded = STANDARD.decode(encoded).map_err(|_| malformed())?;
        if decoded.len() as u64 != expected_size {
            return Err(mismatch());
        }
        Ok(Refresh::from_headers(Some(decoded), &headers))
    }
}
