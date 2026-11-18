use std::num::NonZeroU64;

use serde::Serialize;

use super::{ItemNumber, RateLimit, RepositoryApiId, WritePermission};

pub struct VerifiedPull {
    pub(super) actor: NonZeroU64,
    pub(super) repository: RepositoryApiId,
    pub(super) number: ItemNumber,
    pub(super) id: NonZeroU64,
    pub(super) node_id: String,
    pub(super) base: String,
    pub(super) head: String,
    pub(super) open: bool,
    pub(super) author: Option<NonZeroU64>,
    pub(super) locked: Option<bool>,
    pub(super) draft: Option<bool>,
}

impl VerifiedPull {
    pub fn base(&self) -> &str {
        &self.base
    }
    pub fn head(&self) -> &str {
        &self.head
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn author_id(&self) -> Option<NonZeroU64> {
        self.author
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewEvent {
    Comment,
    Approve,
    RequestChanges,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ReviewState {
    Pending,
    Submitted(ReviewEvent),
    Other,
}

pub struct ObservedReview {
    pub(super) repository: RepositoryApiId,
    pub(super) pull: NonZeroU64,
    pub(super) id: NonZeroU64,
    pub(super) author: NonZeroU64,
    pub(super) head: String,
    pub(super) state: ReviewState,
}

impl ObservedReview {
    pub fn id(&self) -> NonZeroU64 {
        self.id
    }
    pub fn author(&self) -> NonZeroU64 {
        self.author
    }
    pub fn head(&self) -> &str {
        &self.head
    }
    pub fn state(&self) -> ReviewState {
        self.state
    }
}

pub struct ObservedThread {
    pub(super) repository: RepositoryApiId,
    pub(super) pull: NonZeroU64,
    pub(super) head: String,
    pub(super) id: String,
    pub(super) root: Option<NonZeroU64>,
    pub(super) outdated: Option<bool>,
    pub(super) resolved: Option<bool>,
    pub(super) can_resolve: Option<bool>,
    pub(super) can_reopen: Option<bool>,
}

impl ObservedThread {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn root(&self) -> Option<NonZeroU64> {
        self.root
    }
    pub fn outdated(&self) -> Option<bool> {
        self.outdated
    }
    pub fn resolved(&self) -> Option<bool> {
        self.resolved
    }
    pub fn permission(&self, resolved: bool) -> WritePermission {
        match if resolved {
            self.can_resolve
        } else {
            self.can_reopen
        } {
            Some(true) => WritePermission::Allowed,
            Some(false) => WritePermission::Denied,
            None => WritePermission::Unknown,
        }
    }
}

pub enum PendingReview {
    Absent,
    Present(ObservedReview),
    Unknown,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ReviewSide {
    Left,
    Right,
}

#[derive(Serialize)]
pub struct InlineReview<'a> {
    pub path: &'a str,
    pub body: &'a str,
    pub side: ReviewSide,
    pub line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_side: Option<ReviewSide>,
}

pub enum ReviewMutation<'a> {
    CreatePending {
        pending: &'a PendingReview,
        summary: &'a str,
        comments: &'a [InlineReview<'a>],
    },
    SubmitPending {
        review: &'a ObservedReview,
        event: ReviewEvent,
        body: &'a str,
    },
    Reply {
        thread: &'a ObservedThread,
        root: NonZeroU64,
        body: &'a str,
    },
    SetThreadResolved {
        thread: &'a ObservedThread,
        resolved: bool,
        correlation: &'a str,
    },
}

pub enum ReviewObject {
    Review(ObservedReview),
    Reply(ObservedReply),
    Thread { id: String, resolved: bool },
}

pub struct ObservedReply {
    pub id: NonZeroU64,
    pub author: NonZeroU64,
    pub root: NonZeroU64,
    pub body: String,
}

pub struct ReviewMutationResponse {
    pub object: ReviewObject,
    pub request_id: String,
    pub rate: RateLimit,
}
