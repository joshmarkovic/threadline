use std::{collections::BTreeSet, num::NonZeroU64};

use serde::Serialize;

use super::{AccessError, ItemNumber, RateLimit, RepositoryApiId};

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct IssueWriteTarget {
    pub number: ItemNumber,
    pub issue_id: NonZeroU64,
    pub pull_request: bool,
}

pub struct VerifiedIssue {
    pub(super) repository: RepositoryApiId,
    pub(super) target: IssueWriteTarget,
    pub(super) actor: NonZeroU64,
    pub(super) author: Option<NonZeroU64>,
    pub(super) locked: Option<bool>,
    pub observed: IssueWriteValues,
    pub milestone_title: Option<String>,
    pub updated_at_ms: i64,
}

impl VerifiedIssue {
    pub fn author_id(&self) -> Option<NonZeroU64> {
        self.author
    }
}

#[derive(Default)]
pub struct IssueWriteValues {
    pub title: Option<String>,
    pub body: Option<Option<String>>,
    pub labels: Option<BTreeSet<String>>,
    pub assignees: Option<BTreeSet<String>>,
    pub milestone: Option<Option<NonZeroU64>>,
    pub closed: Option<bool>,
}

pub struct VerifiedComment {
    pub(super) repository: RepositoryApiId,
    pub(super) issue: IssueWriteTarget,
    pub(super) id: NonZeroU64,
    pub author: Option<String>,
    pub(super) author_id: Option<NonZeroU64>,
    pub body: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl VerifiedComment {
    pub fn id(&self) -> NonZeroU64 {
        self.id
    }
    pub fn author_id(&self) -> Option<NonZeroU64> {
        self.author_id
    }
}

#[derive(Default, Serialize)]
pub struct IssuePatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<BTreeSet<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignees: Option<BTreeSet<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub milestone: Option<Option<NonZeroU64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<IssueState>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueState {
    Open,
    Closed,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum WritePermission {
    Allowed,
    Denied,
    Unknown,
}

#[derive(Clone, Copy)]
pub enum IssueMutation<'a> {
    EditIssue(&'a IssuePatch),
    CreateComment {
        body: &'a str,
    },
    EditComment {
        comment: &'a VerifiedComment,
        body: &'a str,
    },
}

pub enum MutationObject {
    Issue(VerifiedIssue),
    Comment(VerifiedComment),
}

pub struct MutationResponse {
    pub object: MutationObject,
    pub request_id: String,
    pub rate: RateLimit,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DispatchEvidence {
    DefinitelyUnsent,
    AccessRejected,
    InvalidInput,
    Throttled,
    Ambiguous,
}

#[derive(Debug)]
pub struct MutationFailure {
    pub evidence: DispatchEvidence,
    pub access: AccessError,
}
