use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;

use super::{
    AccessError, AccessKind, CancellationToken, GitHubReader, PrComparisonIdentity, PrSide,
    PrSnapshot, Refresh, RequestMode, identity_mismatch, malformed, wire::checked_path,
};
use crate::github::access::reader::wire::oid;

const MAX_BLOB_BYTES: usize = 512 * 1024;
const MAX_CONTEXT_LINES: u16 = 200;

pub enum PrSource {
    Text {
        oid: String,
        path: String,
        first_line: u32,
        text: String,
    },
    Binary,
    Unavailable(SourceUnavailable),
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum SourceUnavailable {
    TooLarge,
    Encoding,
    MissingContent,
    OutsideFile,
}

pub struct PrSourceRange<'a> {
    pub side: PrSide,
    pub path: &'a str,
    pub start: u32,
    pub lines: u16,
}

impl GitHubReader<'_> {
    pub async fn pr_source_context(
        &self,
        snapshot: &PrSnapshot,
        comparison: &PrComparisonIdentity,
        range: PrSourceRange<'_>,
        cancel: &CancellationToken,
    ) -> Result<Refresh<PrSource>, AccessError> {
        let pull = self.require_snapshot(snapshot)?;
        if comparison.pull_node != pull.node_id
            || comparison.base != pull.base_oid
            || comparison.head != pull.head_oid
        {
            return Err(AccessError::local(AccessKind::WrongScope));
        }
        if range.start == 0 || !(1..=MAX_CONTEXT_LINES).contains(&range.lines) {
            return Err(AccessError::local(AccessKind::InvalidPagination));
        }
        let path = checked_path(range.path.to_owned())?;
        let commit = match range.side {
            PrSide::Left => &comparison.merge_base,
            PrSide::Right => &comparison.head,
        };
        let mut url = self
            .session
            .client
            .base
            .join(&self.permanent_path("contents"))
            .map_err(|_| AccessError::local(AccessKind::WrongScope))?;
        url.path_segments_mut()
            .map_err(|_| AccessError::local(AccessKind::WrongScope))?
            .extend(path.split('/'));
        url.query_pairs_mut().append_pair("ref", commit);
        let response = self
            .get_refresh::<FileContents>(url.as_str(), RequestMode::Read, cancel)
            .await?;
        let value = response
            .body
            .map(|file| file.read(commit, &path, range))
            .transpose()?;
        Ok(Refresh::from_headers(value, &response.headers))
    }
}

#[derive(Deserialize)]
struct FileContents {
    path: String,
    sha: String,
    #[serde(rename = "type")]
    kind: String,
    size: usize,
    encoding: Option<String>,
    content: Option<String>,
}

impl FileContents {
    fn read(
        self,
        commit: &str,
        expected_path: &str,
        range: PrSourceRange<'_>,
    ) -> Result<PrSource, AccessError> {
        if self.path != expected_path {
            return Err(identity_mismatch());
        }
        oid(self.sha)?;
        if self.size > MAX_BLOB_BYTES {
            return Ok(PrSource::Unavailable(SourceUnavailable::TooLarge));
        }
        if self.kind != "file" || self.encoding.as_deref() != Some("base64") {
            return Ok(PrSource::Unavailable(SourceUnavailable::Encoding));
        }
        let Some(encoded) = self.content else {
            return Ok(PrSource::Unavailable(SourceUnavailable::MissingContent));
        };
        let encoded: Vec<_> = encoded
            .bytes()
            .filter(|byte| !matches!(byte, b'\n' | b'\r'))
            .collect();
        let bytes = STANDARD.decode(encoded).map_err(|_| malformed())?;
        if bytes.len() != self.size {
            return Err(malformed());
        }
        if bytes.contains(&0) {
            return Ok(PrSource::Binary);
        }
        let Ok(source) = std::str::from_utf8(&bytes) else {
            return Ok(PrSource::Unavailable(SourceUnavailable::Encoding));
        };
        let text = source
            .lines()
            .skip(range.start.saturating_sub(1) as usize)
            .take(usize::from(range.lines))
            .collect::<Vec<_>>();
        if text.is_empty() {
            return Ok(PrSource::Unavailable(SourceUnavailable::OutsideFile));
        }
        Ok(PrSource::Text {
            oid: commit.to_owned(),
            path: self.path,
            first_line: range.start,
            text: text.join("\n"),
        })
    }
}
