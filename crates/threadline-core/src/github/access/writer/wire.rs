use serde::{Deserialize, Deserializer};
use std::num::NonZeroU64;

use super::*;

#[derive(Deserialize)]
pub(super) struct IssueWire {
    pub id: NonZeroU64,
    pub number: NonZeroU64,
    pub repository_url: String,
    pub state: Option<String>,
    pub title: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub body: Option<Option<String>>,
    pub labels: Option<Vec<LabelWire>>,
    pub assignees: Option<Vec<UserWire>>,
    #[serde(default, deserialize_with = "nullable")]
    pub milestone: Option<Option<MilestoneWire>>,
    pub pull_request: Option<serde_json::Value>,
    pub user: Option<UserWire>,
    pub locked: Option<bool>,
    pub updated_at: String,
}

#[derive(Deserialize)]
pub(super) struct CommentWire {
    pub id: NonZeroU64,
    pub issue_url: String,
    pub user: Option<UserWire>,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
pub(super) struct UserWire {
    pub id: NonZeroU64,
    pub login: String,
}

#[derive(Deserialize)]
pub(super) struct LabelWire {
    pub name: String,
}

#[derive(Deserialize)]
pub(super) struct MilestoneWire {
    pub number: NonZeroU64,
    pub title: String,
}

fn nullable<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error> {
    Ok(Some(Option::<T>::deserialize(deserializer)?))
}

pub(super) fn timestamp(value: &str) -> Result<i64, AccessError> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|date| date.timestamp_millis())
        .map_err(|_| AccessError::received(AccessKind::MalformedResponse, 200))
}
