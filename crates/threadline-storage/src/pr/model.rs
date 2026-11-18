use crate::{
    AccountId, ItemId, RepositoryId,
    resources::{CommitOid, ComparisonKey, ComparisonMode},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrRevision {
    pub id: i64,
    pub account: AccountId,
    pub repository: RepositoryId,
    pub pr: ItemId,
    pub base: CommitOid,
    pub head: CommitOid,
    pub merge_base: Option<CommitOid>,
}
impl PrRevision {
    pub fn comparison(&self, ignore_whitespace: bool, context_lines: u16) -> Option<ComparisonKey> {
        Some(ComparisonKey {
            repository: self.repository,
            base: self.merge_base.clone()?,
            head: self.head.clone(),
            mode: ComparisonMode::MergeBase,
            ignore_whitespace,
            context_lines,
        })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct PrHeader {
    pub author: Option<String>,
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
    pub draft: Option<bool>,
    pub requested_reviewers: Option<Vec<String>>,
    pub review_decision: Option<String>,
    pub created_at_ms: Option<i64>,
    pub merged_at_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PrFamily {
    Commits,
    Files,
    Reviews,
    Threads,
    Checks,
    Statuses,
    Context,
    Media,
}
impl PrFamily {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Commits => "commits",
            Self::Files => "files",
            Self::Reviews => "reviews",
            Self::Threads => "threads",
            Self::Checks => "checks",
            Self::Statuses => "statuses",
            Self::Context => "context",
            Self::Media => "media",
        }
    }
    pub(crate) const REQUIRED: [Self; 6] = [
        Self::Commits,
        Self::Files,
        Self::Reviews,
        Self::Threads,
        Self::Checks,
        Self::Statuses,
    ];
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PatchEvidence {
    Text(String),
    Truncated(String),
    Binary,
    Unavailable,
    ApiCapped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SourceSide {
    Left,
    Right,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReviewAnchor {
    pub path: String,
    pub side: SourceSide,
    pub line: u32,
    pub original_oid: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InlineComment {
    pub id: u64,
    pub node_id: Option<String>,
    pub in_reply_to: Option<u64>,
    pub author: Option<String>,
    pub body: String,
    pub created_at_ms: Option<i64>,
    pub updated_at_ms: Option<i64>,
    pub anchor: Option<ReviewAnchor>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PrRecord {
    Commit {
        oid: String,
        message: String,
        author: Option<String>,
        authored_at_ms: Option<i64>,
        committed_at_ms: Option<i64>,
    },
    File {
        path: String,
        previous_path: Option<String>,
        status: String,
        additions: Option<u64>,
        deletions: Option<u64>,
        patch: PatchEvidence,
    },
    Review {
        id: u64,
        node_id: Option<String>,
        author: Option<String>,
        body: String,
        state: String,
        submitted_at_ms: Option<i64>,
        commit_oid: Option<String>,
    },
    Thread {
        node_id: String,
        resolved: Option<bool>,
        outdated: Option<bool>,
        comments: Vec<InlineComment>,
        replies_complete: bool,
        #[serde(default)]
        reply_cursor: Option<String>,
    },
    Check {
        id: u64,
        head_oid: String,
        name: String,
        status: String,
        conclusion: Option<String>,
        completed_at_ms: Option<i64>,
    },
    Status {
        id: u64,
        head_oid: String,
        context: String,
        state: String,
        updated_at_ms: Option<i64>,
    },
    Context {
        oid: String,
        path: String,
        first_line: u32,
        text: String,
    },
    /// Only validated media already present in the bounded repository image cache is referenced here.
    Media { cache_key: String },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum PrSection {
    Conversation,
    Commits,
    #[default]
    Files,
    Checks,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum NarrowSurface {
    Context,
    #[default]
    Code,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum PrContextMode {
    #[default]
    Body,
    Issue,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum PrDiffMode {
    #[default]
    Unified,
    Split,
}
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrTabView {
    pub context_mode: PrContextMode,
    pub diff_mode: PrDiffMode,
    pub context_scroll: u32,
    pub code_scroll: u32,
    pub section: PrSection,
    pub context_collapsed: bool,
    pub narrow_surface: NarrowSurface,
    pub context_anchor: Option<String>,
    pub code_anchor: Option<ReviewAnchor>,
    pub selected_path: Option<String>,
}
