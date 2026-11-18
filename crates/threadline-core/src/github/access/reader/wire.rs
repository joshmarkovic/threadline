use std::num::NonZeroU64;

use chrono::DateTime;
use serde::{Deserialize, Deserializer, de::DeserializeOwned};

use crate::domain::{IssueApiId, ItemKind, ItemNumber};

use super::{
    AccessError, ConversationComment, GitHubReader, IssueSummary, PullPresentation,
    PullRequestSummary, ReactionCounts, Reported, TimelineEvent, malformed,
};

#[derive(Deserialize)]
struct Author {
    login: String,
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

#[derive(Deserialize)]
struct Milestone {
    title: String,
}

#[derive(Deserialize)]
struct Reactions {
    #[serde(rename = "+1")]
    positive: Option<u64>,
    #[serde(rename = "-1")]
    negative: Option<u64>,
    laugh: Option<u64>,
    hooray: Option<u64>,
    confused: Option<u64>,
    heart: Option<u64>,
    rocket: Option<u64>,
    eyes: Option<u64>,
    total_count: Option<u64>,
}

impl Reactions {
    fn counts(self) -> Option<ReactionCounts> {
        Some(ReactionCounts {
            positive: self.positive?,
            negative: self.negative?,
            laugh: self.laugh?,
            hooray: self.hooray?,
            confused: self.confused?,
            heart: self.heart?,
            rocket: self.rocket?,
            eyes: self.eyes?,
            total: self.total_count,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum State {
    Open,
    Closed,
}

#[derive(Deserialize)]
pub(super) struct Issue {
    id: NonZeroU64,
    node_id: String,
    number: NonZeroU64,
    title: String,
    #[serde(default, deserialize_with = "reported")]
    body: Reported<String>,
    state: State,
    created_at: Option<String>,
    updated_at: String,
    user: Option<Author>,
    labels: Option<Vec<Label>>,
    assignees: Option<Vec<Author>>,
    pull_request: Option<serde_json::Map<String, serde_json::Value>>,
    repository_url: Option<String>,
    #[serde(default, deserialize_with = "reported")]
    milestone: Reported<Milestone>,
    reactions: Option<Reactions>,
}

impl Issue {
    pub(super) fn into_update(
        self,
        reader: &GitHubReader<'_>,
    ) -> Result<IssueSummary, AccessError> {
        reader.validate_named_url(self.repository_url.as_deref().ok_or_else(malformed)?, "")?;
        self.into_summary(reader, false)
    }

    pub(super) fn into_summary(
        self,
        reader: &GitHubReader<'_>,
        search_hit: bool,
    ) -> Result<IssueSummary, AccessError> {
        if let Some(url) = &self.repository_url {
            reader.require_record_url(url, "")?;
        } else if search_hit {
            return Err(malformed());
        }
        Ok(IssueSummary {
            number: ItemNumber::new(self.number),
            issue_id: IssueApiId::new(self.id),
            node_id: opaque_id(self.node_id)?,
            kind: if self.pull_request.is_some() {
                ItemKind::PullRequest
            } else {
                ItemKind::Issue
            },
            title: self.title,
            body: self.body,
            closed: matches!(self.state, State::Closed),
            created_at_ms: self.created_at.as_deref().map(millis).transpose()?,
            updated_at_ms: millis(&self.updated_at)?,
            author: self.user.map(|user| user.login),
            labels: self
                .labels
                .map(|labels| labels.into_iter().map(|label| label.name).collect()),
            assignees: self
                .assignees
                .map(|people| people.into_iter().map(|user| user.login).collect()),
            milestone: match self.milestone {
                Reported::Absent => Reported::Absent,
                Reported::Null => Reported::Null,
                Reported::Value(milestone) => Reported::Value(milestone.title),
            },
            reactions: self.reactions.and_then(Reactions::counts),
        })
    }
}

#[derive(Deserialize)]
struct Ref {
    sha: String,
    #[serde(rename = "ref")]
    name: Option<String>,
    repo: Option<RefRepository>,
}

#[derive(Deserialize)]
struct RefRepository {
    id: NonZeroU64,
    node_id: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct PullRequest {
    id: NonZeroU64,
    node_id: String,
    number: NonZeroU64,
    title: String,
    #[serde(default, deserialize_with = "reported")]
    body: Reported<String>,
    state: State,
    updated_at: String,
    merged: bool,
    base: Ref,
    head: Ref,
    #[serde(default, deserialize_with = "reported")]
    merge_commit_sha: Reported<String>,
    user: Option<Author>,
    draft: Option<bool>,
    requested_reviewers: Option<Vec<Author>>,
    created_at: Option<String>,
    merged_at: Option<String>,
    changed_files: Option<u64>,
}

impl PullRequest {
    pub(super) fn into_summary(self) -> Result<PullRequestSummary, AccessError> {
        let merge_oid = match self.merge_commit_sha {
            Reported::Value(sha) => Reported::Value(oid(sha)?),
            Reported::Absent => Reported::Absent,
            Reported::Null => Reported::Null,
        };
        let summary = PullRequestSummary {
            number: ItemNumber::new(self.number),
            pull_request_id: self.id,
            node_id: opaque_id(self.node_id)?,
            title: self.title,
            body: self.body,
            closed: matches!(self.state, State::Closed),
            updated_at_ms: millis(&self.updated_at)?,
            merged: self.merged,
            base_oid: oid(self.base.sha)?,
            head_oid: oid(self.head.sha)?,
            merge_oid,
            presentation: PullPresentation {
                author: self.user.map(|user| user.login),
                base_ref: self.base.name,
                head_ref: self.head.name,
                draft: self.draft,
                requested_reviewers: self
                    .requested_reviewers
                    .map(|reviewers| reviewers.into_iter().map(|user| user.login).collect()),
                created_at_ms: self.created_at.as_deref().map(millis).transpose()?,
                merged_at_ms: self.merged_at.as_deref().map(millis).transpose()?,
                changed_files: self.changed_files,
                base_repository_id: self
                    .base
                    .repo
                    .as_ref()
                    .map(|repo| crate::domain::RepositoryApiId::new(repo.id)),
                base_repository_node: self
                    .base
                    .repo
                    .and_then(|repo| repo.node_id)
                    .map(opaque_id)
                    .transpose()?,
            },
        };
        Ok(summary)
    }
}

#[derive(Deserialize)]
pub(super) struct Comment {
    id: NonZeroU64,
    node_id: String,
    user: Option<Author>,
    body: String,
    created_at: String,
    updated_at: String,
    issue_url: Option<String>,
}

impl Comment {
    pub(super) fn into_update(
        self,
        reader: &GitHubReader<'_>,
    ) -> Result<super::refresh::UpdatedComment, AccessError> {
        let url = self.issue_url.as_deref().ok_or_else(malformed)?;
        let parsed = reqwest::Url::parse(url).map_err(|_| malformed())?;
        let issue = parsed
            .path_segments()
            .and_then(|mut path| path.next_back())
            .and_then(|number| number.parse::<NonZeroU64>().ok())
            .map(ItemNumber::new)
            .ok_or_else(malformed)?;
        let suffix = format!("issues/{}", issue.get());
        reader.validate_named_url(url, &suffix)?;
        Ok(super::refresh::UpdatedComment {
            issue,
            comment: self.into_comment(reader, &suffix)?,
        })
    }

    pub(super) fn into_comment(
        self,
        reader: &GitHubReader<'_>,
        issue: &str,
    ) -> Result<ConversationComment, AccessError> {
        if let Some(url) = &self.issue_url {
            reader.require_record_url(url, issue)?;
        }
        Ok(ConversationComment {
            id: self.id,
            node_id: opaque_id(self.node_id)?,
            author: self.user.map(|author| author.login),
            body: self.body,
            created_at_ms: millis(&self.created_at)?,
            updated_at_ms: millis(&self.updated_at)?,
        })
    }
}

#[derive(Deserialize)]
pub(super) struct Timeline {
    event: String,
    id: Option<NonZeroU64>,
    node_id: Option<String>,
    sha: Option<String>,
    actor: Option<Author>,
    created_at: Option<String>,
    source: Option<TimelineSource>,
}

#[derive(Deserialize)]
struct TimelineSource {
    #[serde(rename = "type")]
    kind: Option<String>,
    issue: Option<SourceIssue>,
}

#[derive(Deserialize)]
struct SourceIssue {
    id: Option<NonZeroU64>,
    number: Option<NonZeroU64>,
    url: Option<String>,
    repository_url: Option<String>,
    pull_request: Option<serde_json::Map<String, serde_json::Value>>,
}

impl Timeline {
    pub(super) fn is_comment(&self) -> bool {
        self.event == "commented"
    }

    pub(super) fn into_event(
        self,
        reader: &GitHubReader<'_>,
        page: u32,
        index: usize,
    ) -> Result<TimelineEvent, AccessError> {
        let kind = if self.event.len() <= 64
            && !self.event.is_empty()
            && self
                .event
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || matches!(byte, b'_' | b'-'))
        {
            self.event
        } else {
            "unsupported_event".into()
        };
        let stable_key = if let Some(id) = self.id {
            Some(format!("{kind}:id:{id}"))
        } else if let Some(node) = self.node_id {
            Some(format!("{kind}:node:{}", opaque_id(node)?))
        } else if let Some(sha) = self.sha.filter(|_| kind == "committed") {
            Some(format!("committed:sha:{}", oid(sha)?))
        } else {
            None
        };
        let linked_item = (kind == "cross-referenced")
            .then_some(self.source)
            .flatten()
            .filter(|source| source.kind.as_deref() == Some("issue"))
            .and_then(|source| {
                let issue = source.issue?;
                reader.linked_item(
                    issue.url.as_deref()?,
                    issue.repository_url.as_deref()?,
                    ItemNumber::new(issue.number?),
                    issue.pull_request.is_some(),
                    issue.id.map(IssueApiId::new),
                )
            });
        Ok(TimelineEvent {
            stable_identity: stable_key.is_some(),
            key: stable_key.unwrap_or_else(|| format!("unsupported:{page}:{index}")),
            kind,
            actor: self.actor.map(|actor| actor.login),
            created_at_ms: self.created_at.as_deref().map(millis).transpose()?,
            linked_item,
        })
    }
}

#[derive(Deserialize)]
pub(super) struct Search {
    pub total_count: u64,
    pub incomplete_results: bool,
    pub items: Vec<Issue>,
}

fn reported<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    deserializer: D,
) -> Result<Reported<T>, D::Error> {
    Ok(Option::<T>::deserialize(deserializer)?.map_or(Reported::Null, Reported::Value))
}

pub(super) fn millis(value: &str) -> Result<i64, AccessError> {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.timestamp_millis())
        .map_err(|_| malformed())
}

pub(super) fn opaque_id(id: String) -> Result<String, AccessError> {
    if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        return Err(malformed());
    }
    Ok(id)
}

pub(super) fn oid(sha: String) -> Result<String, AccessError> {
    if !matches!(sha.len(), 40 | 64) || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(malformed());
    }
    Ok(sha.to_ascii_lowercase())
}
