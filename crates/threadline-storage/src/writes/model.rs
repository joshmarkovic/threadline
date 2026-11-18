use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::num::NonZeroU64;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FieldEdit<T> {
    pub base: T,
    pub wanted: T,
}

/// Missing outer fields are outside the edit. An inner None is an explicitly cleared nullable field.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct IssueChanges {
    pub title: Option<FieldEdit<String>>,
    pub body: Option<FieldEdit<Option<String>>>,
    pub labels: Option<FieldEdit<BTreeSet<String>>>,
    pub assignees: Option<FieldEdit<BTreeSet<String>>>,
    /// GitHub milestone number within the original repository.
    pub milestone: Option<FieldEdit<Option<NonZeroU64>>>,
    pub closed: Option<FieldEdit<bool>>,
}

/// Missing observed fields were not reported by the provider. A preflight cannot assume an empty value.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct IssueValues {
    pub title: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "known_nullable"
    )]
    pub body: Option<Option<String>>,
    pub labels: Option<BTreeSet<String>>,
    pub assignees: Option<BTreeSet<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "known_nullable"
    )]
    pub milestone: Option<Option<NonZeroU64>>,
    pub closed: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CommentTarget {
    Remote(NonZeroU64),
    CreatedBy(String),
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum WriteIntent {
    EditIssue(IssueChanges),
    CreateComment {
        body: String,
    },
    EditComment {
        target: CommentTarget,
        body: FieldEdit<String>,
    },
}
impl WriteIntent {
    pub fn validate(&self) -> Result<()> {
        let nonblank = |text: &str| !text.trim().is_empty() && !text.contains('\0');
        match self {
            Self::EditIssue(changes) => {
                if *changes == IssueChanges::default() {
                    return Err(Error::InvalidInput("No changed issue fields"));
                }
                if changes.title.as_ref().is_some_and(|v| !nonblank(&v.wanted))
                    || changes
                        .body
                        .as_ref()
                        .and_then(|v| v.wanted.as_ref())
                        .is_some_and(|v| v.contains('\0'))
                {
                    return Err(Error::InvalidInput("Invalid issue title or body"));
                }
                if [&changes.labels, &changes.assignees]
                    .into_iter()
                    .flatten()
                    .any(|v| v.wanted.iter().any(|v| !nonblank(v)))
                {
                    return Err(Error::InvalidInput("Empty label or assignee"));
                }
            }
            Self::CreateComment { body }
            | Self::EditComment {
                body: FieldEdit { wanted: body, .. },
                ..
            } if !nonblank(body) => {
                return Err(Error::InvalidInput("An ordinary comment needs a body"));
            }
            _ => {}
        }
        Ok(())
    }
    pub fn action(&self) -> &'static str {
        match self {
            Self::EditIssue(_) => "issue_edited",
            Self::CreateComment { .. } => "comment_created",
            Self::EditComment { .. } => "comment_edited",
        }
    }
}
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepositoryChoices {
    pub labels: Option<BTreeSet<String>>,
    pub assignees: Option<BTreeSet<String>>,
    pub milestones: Option<BTreeSet<NonZeroU64>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RemoteComment {
    pub id: NonZeroU64,
    pub author: Option<String>,
    pub author_id: Option<NonZeroU64>,
    pub body: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Permission {
    Allowed,
    Denied,
    Unknown,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WriteObservation {
    pub credential_generation: i64,
    pub actor: NonZeroU64,
    pub repository: NonZeroU64,
    pub issue: NonZeroU64,
    pub number: NonZeroU64,
    pub pull_request: bool,
    pub observed_at_ms: i64,
    pub permission: Permission,
    pub fields: IssueValues,
    pub comment: Option<RemoteComment>,
    pub choices: RepositoryChoices,
}

pub(super) fn matches_fields(fields: &IssueChanges, observed: &IssueValues, wanted: bool) -> bool {
    fn same<T: Eq>(edit: &Option<FieldEdit<T>>, observed: &Option<T>, wanted: bool) -> bool {
        edit.as_ref()
            .is_none_or(|x| observed.as_ref() == Some(if wanted { &x.wanted } else { &x.base }))
    }
    same(&fields.title, &observed.title, wanted)
        && same(&fields.body, &observed.body, wanted)
        && same(&fields.labels, &observed.labels, wanted)
        && same(&fields.assignees, &observed.assignees, wanted)
        && same(&fields.milestone, &observed.milestone, wanted)
        && same(&fields.closed, &observed.closed, wanted)
}

pub(super) fn available_choices(fields: &IssueChanges, offered: &RepositoryChoices) -> bool {
    fn set_available(
        change: &Option<FieldEdit<BTreeSet<String>>>,
        available: &Option<BTreeSet<String>>,
    ) -> bool {
        change.as_ref().is_none_or(|c| {
            c.wanted
                .difference(&c.base)
                .all(|v| available.as_ref().is_some_and(|set| set.contains(v)))
        })
    }
    set_available(&fields.labels, &offered.labels)
        && set_available(&fields.assignees, &offered.assignees)
        && fields.milestone.as_ref().is_none_or(|c| {
            c.wanted.is_none()
                || c.wanted == c.base
                || offered
                    .milestones
                    .as_ref()
                    .is_some_and(|set| c.wanted.is_some_and(|id| set.contains(&id)))
        })
}

fn known_nullable<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Option<T>>, D::Error> {
    Ok(Some(Option::<T>::deserialize(d)?))
}
