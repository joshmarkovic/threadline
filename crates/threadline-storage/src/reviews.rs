//! Revision-bound local review drafts and typed Outbox steps.
use crate::access::AccessProfile;
use crate::pr::{self, PatchEvidence, PrFamily, PrRecord, PrRevision, SourceSide, StoredRecord};
use crate::queue::{self, ActionKind, Claim, Delivery, DraftSubmission, Receipt, Submission};
use crate::writes::{decode, encode};
use crate::{AccountId, Error, ItemId, Result, Store};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::num::NonZeroU64;

mod dispatch;
mod drafts;
pub use dispatch::*;
mod recovery;
pub use recovery::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReviewEvent {
    Comment,
    Approve,
    RequestChanges,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InlineAnchor {
    /// GitHub's comparison path. The source path on the original side of a rename can differ.
    pub path: String,
    pub source_path: String,
    pub side: SourceSide,
    pub first_line: u32,
    pub last_line: u32,
    pub source_oid: String,
    pub original_hunk: String,
    pub first_patch_line: u32,
    pub last_patch_line: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DraftInline {
    pub key: String,
    pub body: String,
    pub anchor: InlineAnchor,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReviewDraft {
    pub event: ReviewEvent,
    pub summary: String,
    pub comments: Vec<DraftInline>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedReviewDraft {
    pub revision: i64,
    pub item: ItemId,
    pub revision_id: i64,
    pub draft: ReviewDraft,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReviewIntent {
    Review(ReviewDraft),
    Reply {
        thread_id: String,
        root_comment_id: NonZeroU64,
        body: String,
    },
    SetThreadResolved {
        thread_id: String,
        resolved: bool,
    },
}
impl ReviewIntent {
    fn kind(&self) -> ActionKind {
        match self {
            Self::Review(_) => ActionKind::Review,
            Self::Reply { .. } => ActionKind::Reply,
            Self::SetThreadResolved { .. } => ActionKind::ResolveThread,
        }
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Review(draft) => {
                unique_keys(draft)?;
                if draft.comments.iter().any(|c| c.body.trim().is_empty())
                    || draft.event == ReviewEvent::RequestChanges && draft.summary.trim().is_empty()
                    || draft.event == ReviewEvent::Comment
                        && draft.comments.is_empty()
                        && draft.summary.trim().is_empty()
                {
                    return Err(Error::InvalidInput(
                        "The review action requires nonempty text",
                    ));
                }
            }
            Self::Reply {
                thread_id, body, ..
            } if thread_id.is_empty() || body.trim().is_empty() => {
                return Err(Error::InvalidInput(
                    "Reply requires an original thread and text",
                ));
            }
            Self::SetThreadResolved { thread_id, .. } if thread_id.is_empty() => {
                return Err(Error::InvalidInput("Missing original thread"));
            }
            _ => {}
        }
        Ok(())
    }
}

fn unique_keys(draft: &ReviewDraft) -> Result<()> {
    let mut seen = BTreeSet::new();
    if draft
        .comments
        .iter()
        .any(|c| c.key.is_empty() || !seen.insert(&c.key))
    {
        return Err(Error::InvalidInput(
            "Each inline draft needs a distinct stable key",
        ));
    }
    Ok(())
}

fn stored_record(
    db: &Connection,
    rev: &PrRevision,
    family: PrFamily,
    key: &str,
) -> Result<Option<StoredRecord>> {
    let Some(resource) = pr::family_id(db, rev, family)? else {
        return Ok(None);
    };
    let raw: Option<String> = db
        .query_row(
            "SELECT body FROM cached_resource_records WHERE resource_id=? AND record_key=?",
            params![resource, key],
            |r| r.get(0),
        )
        .optional()?;
    raw.as_deref().map(decode).transpose()
}

fn numeric(s: &str) -> Result<NonZeroU64> {
    s.parse().map_err(|_| Error::Corrupt)
}
