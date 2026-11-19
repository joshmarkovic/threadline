use std::num::NonZeroU64;

use serde::{Deserialize, de::DeserializeOwned};
use serde_json::json;

use super::{
    AccessError, AccessKind, CancellationToken, GitHubReader, PrSnapshot, PullRequestSummary,
    RequestMode, identity_mismatch, malformed, wire::checked_path,
};
use crate::github::access::reader::ActorIdentity;
use crate::github::access::{
    GraphError, RateLimit,
    reader::wire::{millis, oid, opaque_id},
};

const PR_IDENTITY: &str =
    "id fullDatabaseId baseRefOid headRefOid repository { id name owner { login } }";
const AUTHOR: &str =
    "author { login __typename ... on User { databaseId } ... on Bot { databaseId } }";

fn comments() -> String {
    format!(
        "nodes {{ id fullDatabaseId replyTo {{ fullDatabaseId }} {AUTHOR} body createdAt updatedAt path originalLine originalCommit {{ oid }} }} pageInfo {{ hasNextPage endCursor }}"
    )
}

pub struct PrGraphPage<T> {
    pub records: Vec<T>,
    pub next_cursor: Option<String>,
    pub partial: bool,
    pub review_decision: Option<String>,
    pub rate: RateLimit,
}

pub struct PrReview {
    pub id: u64,
    pub node_id: String,
    pub author: Option<String>,
    pub actor: ActorIdentity,
    pub body: String,
    pub state: String,
    pub submitted_at_ms: Option<i64>,
    pub commit_oid: Option<String>,
}

pub struct PrThread {
    node_id: String,
    pub resolved: Option<bool>,
    pub outdated: Option<bool>,
    pub location: Option<PrThreadLocation>,
    pub comments: Vec<PrInlineComment>,
    pub replies_complete: bool,
    pub reply_cursor: Option<String>,
    known_pull: String,
    known_head: String,
}

impl PrThread {
    pub fn node_id(&self) -> &str {
        &self.node_id
    }
}

pub struct PrInlineComment {
    pub id: u64,
    pub node_id: String,
    pub in_reply_to: Option<u64>,
    pub author: Option<String>,
    pub actor: ActorIdentity,
    pub body: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub anchor: Option<PrReviewAnchor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrSide {
    Left,
    Right,
}

#[derive(Debug, Eq, PartialEq)]
pub enum PrThreadLocation {
    Line {
        path: String,
        side: PrSide,
        line: u32,
    },
    File {
        path: String,
    },
}

pub struct PrReviewAnchor {
    pub path: String,
    pub side: PrSide,
    pub line: u32,
    pub original_oid: String,
}

impl GitHubReader<'_> {
    pub async fn pr_reviews(
        &self,
        snapshot: &PrSnapshot,
        cursor: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<PrGraphPage<PrReview>, AccessError> {
        let pull = self.require_snapshot(snapshot)?;
        let query = format!("query ThreadlineReviews($id:ID!,$cursor:String) {{ node(id:$id) {{ ... on PullRequest {{ {PR_IDENTITY}
            reviewDecision reviews(first:20,after:$cursor) {{ nodes {{ id fullDatabaseId {AUTHOR} body state submittedAt commit {{ oid }} }}
            pageInfo {{ hasNextPage endCursor }} }} }} }} }}");
        let data: GraphData<ReviewData> =
            self.pr_graph(query, &pull.node_id, cursor, cancel).await?;
        data.value.identity.verify(pull, self)?;
        let review_decision = data.value.review_decision;
        finish_connection(
            data.value.reviews,
            cursor,
            data.partial,
            data.rate,
            review_decision,
            ReviewWire::read,
        )
    }

    pub async fn pr_threads(
        &self,
        snapshot: &PrSnapshot,
        cursor: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<PrGraphPage<PrThread>, AccessError> {
        let pull = self.require_snapshot(snapshot)?;
        let comments = comments();
        let query = format!("query ThreadlineThreads($id:ID!,$cursor:String) {{ node(id:$id) {{ ... on PullRequest {{ {PR_IDENTITY}
            reviewThreads(first:10,after:$cursor) {{ nodes {{ id path line diffSide subjectType isResolved isOutdated comments(first:20) {{ {comments} }} }}
            pageInfo {{ hasNextPage endCursor }} }} }} }} }}");
        let data: GraphData<ThreadData> =
            self.pr_graph(query, &pull.node_id, cursor, cancel).await?;
        data.value.identity.verify(pull, self)?;
        finish_connection(
            data.value.review_threads,
            cursor,
            data.partial,
            data.rate,
            None,
            |thread| thread.read(pull, data.partial).map(Some),
        )
    }

    pub async fn pr_thread_replies(
        &self,
        snapshot: &PrSnapshot,
        thread: &PrThread,
        cursor: &str,
        cancel: &CancellationToken,
    ) -> Result<PrGraphPage<PrInlineComment>, AccessError> {
        let pull = self.require_snapshot(snapshot)?;
        if thread.known_pull != pull.node_id || thread.known_head != pull.head_oid {
            return Err(AccessError::local(AccessKind::WrongScope));
        }
        self.resume_pr_thread_replies(snapshot, &thread.node_id, cursor, cancel)
            .await
    }

    /// Fulfill the cursor saved for one verified thread, checking the PR binding again in the reply.
    pub async fn resume_pr_thread_replies(
        &self,
        snapshot: &PrSnapshot,
        thread_node: &str,
        cursor: &str,
        cancel: &CancellationToken,
    ) -> Result<PrGraphPage<PrInlineComment>, AccessError> {
        let pull = self.require_snapshot(snapshot)?;
        let thread_node = opaque_id(thread_node.to_owned())?;
        let comments = comments();
        let query = format!("query ThreadlineReplies($id:ID!,$cursor:String) {{ node(id:$id) {{ ... on PullRequestReviewThread {{
            id diffSide pullRequest {{ {PR_IDENTITY} }} comments(first:20,after:$cursor) {{ {comments} }} }} }} }}");
        let data: GraphData<RepliesData> = self
            .pr_graph(query, &thread_node, Some(cursor), cancel)
            .await?;
        if data.value.id != thread_node {
            return Err(identity_mismatch());
        }
        data.value.pull_request.verify(pull, self)?;
        let side = data.value.diff_side;
        finish_connection(
            data.value.comments,
            Some(cursor),
            data.partial,
            data.rate,
            None,
            |comment| comment.read(side),
        )
    }

    async fn pr_graph<T: DeserializeOwned>(
        &self,
        query: String,
        id: &str,
        cursor: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<GraphData<T>, AccessError> {
        if cursor.is_some_and(|cursor| !valid_cursor(cursor)) {
            return Err(AccessError::local(AccessKind::InvalidPagination));
        }
        let variables = json!({"query":query,"variables":{"id":id,"cursor":cursor}});
        let (reply, headers) = self
            .get_refresh::<Envelope<T>>("graphql", RequestMode::Graph(&variables), cancel)
            .await?
            .require_body()?;
        let value = reply
            .data
            .and_then(|data| data.node)
            .ok_or_else(|| graph_failure(&reply.errors))?;
        Ok(GraphData {
            value,
            partial: !reply.errors.is_empty(),
            rate: RateLimit::from_headers(&headers),
        })
    }
}

pub(in crate::github::access::reader) fn valid_cursor(cursor: &str) -> bool {
    !cursor.is_empty() && cursor.len() <= 512 && cursor.bytes().all(|byte| byte.is_ascii_graphic())
}

pub(in crate::github::access::reader) fn graph_failure(errors: &[GraphError]) -> AccessError {
    let kind = errors
        .iter()
        .find_map(|error| {
            match error.kind.as_deref().or_else(|| {
                error
                    .extensions
                    .as_ref()
                    .and_then(|extensions| extensions.code.as_deref())
            }) {
                Some("FORBIDDEN") => Some(AccessKind::PermissionDenied),
                Some("NOT_FOUND") => Some(AccessKind::MissingOrInaccessible),
                Some("RATE_LIMITED") => Some(AccessKind::RateLimited),
                Some("SAML_SSO_REQUIRED") => Some(AccessKind::SsoRequired),
                _ => None,
            }
        })
        .unwrap_or(if errors.is_empty() {
            AccessKind::MissingOrInaccessible
        } else {
            AccessKind::MalformedResponse
        });
    AccessError::received(kind, 200)
}

fn finish_connection<T, R>(
    connection: Option<Connection<T>>,
    cursor: Option<&str>,
    mut partial: bool,
    rate: RateLimit,
    review_decision: Option<String>,
    read: impl Fn(T) -> Result<Option<R>, AccessError>,
) -> Result<PrGraphPage<R>, AccessError> {
    let mut records = Vec::new();
    let mut next_cursor = None;
    if let Some(connection) = connection {
        match connection.page_info {
            Some(info) if info.has_next_page => {
                next_cursor = info
                    .end_cursor
                    .filter(|next| valid_cursor(next) && Some(next.as_str()) != cursor);
                partial |= next_cursor.is_none();
            }
            Some(_) => {}
            None => partial = true,
        }
        if let Some(nodes) = connection.nodes {
            for entry in nodes {
                if let Some(record) = entry.map(&read).transpose()?.flatten() {
                    records.push(record);
                } else {
                    partial = true;
                }
            }
        } else {
            partial = true;
        }
    } else {
        partial = true;
    }
    Ok(PrGraphPage {
        records,
        next_cursor,
        partial,
        review_decision,
        rate,
    })
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: Option<Node<T>>,
    #[serde(default)]
    errors: Vec<GraphError>,
}

#[derive(Deserialize)]
struct Node<T> {
    node: Option<T>,
}

struct GraphData<T> {
    value: T,
    partial: bool,
    rate: RateLimit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection<T> {
    nodes: Option<Vec<Option<T>>>,
    page_info: Option<PageInfo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum GraphId {
    Numeric(NonZeroU64),
    Text(String),
}

impl GraphId {
    fn id(self) -> Option<u64> {
        match self {
            Self::Numeric(id) => Some(id.get()),
            Self::Text(id) => id.parse::<NonZeroU64>().ok().map(NonZeroU64::get),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Identity {
    id: String,
    full_database_id: Option<GraphId>,
    base_ref_oid: String,
    head_ref_oid: String,
    repository: GraphRepository,
}

impl Identity {
    fn verify(
        self,
        pull: &PullRequestSummary,
        reader: &GitHubReader<'_>,
    ) -> Result<(), AccessError> {
        if self.id != pull.node_id
            || self.full_database_id.and_then(GraphId::id) != Some(pull.pull_request_id.get())
            || oid(self.base_ref_oid)? != pull.base_oid
            || oid(self.head_ref_oid)? != pull.head_oid
            || !self
                .repository
                .name
                .eq_ignore_ascii_case(reader.route.name())
            || !self
                .repository
                .owner
                .login
                .eq_ignore_ascii_case(reader.route.owner())
            || Some(self.repository.id.as_str())
                != pull.presentation.base_repository_node.as_deref()
        {
            return Err(identity_mismatch());
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct GraphRepository {
    id: String,
    name: String,
    owner: Login,
}

#[derive(Deserialize)]
struct Login {
    login: String,
}

#[derive(Deserialize)]
struct Author {
    login: String,
    #[serde(rename = "__typename")]
    kind: Option<String>,
    #[serde(rename = "databaseId")]
    database_id: Option<NonZeroU64>,
}

impl Author {
    fn identity(&self) -> ActorIdentity {
        let kind = self.kind.as_deref();
        ActorIdentity {
            user_id: self
                .database_id
                .filter(|_| matches!(kind, Some("User" | "Bot"))),
            known_automation: kind == Some("Bot"),
            app_id: None,
        }
    }
}

#[derive(Deserialize)]
struct GraphCommit {
    oid: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewData {
    #[serde(flatten)]
    identity: Identity,
    review_decision: Option<String>,
    reviews: Option<Connection<ReviewWire>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewWire {
    id: String,
    full_database_id: Option<GraphId>,
    author: Option<Author>,
    body: String,
    state: String,
    submitted_at: Option<String>,
    commit: Option<GraphCommit>,
}

impl ReviewWire {
    fn read(self) -> Result<Option<PrReview>, AccessError> {
        let Some(id) = self.full_database_id.and_then(GraphId::id) else {
            return Ok(None);
        };
        Ok(Some(PrReview {
            id,
            node_id: opaque_id(self.id)?,
            actor: self
                .author
                .as_ref()
                .map(Author::identity)
                .unwrap_or_default(),
            author: self.author.map(|author| author.login),
            body: self.body,
            state: self.state,
            submitted_at_ms: self.submitted_at.as_deref().map(millis).transpose()?,
            commit_oid: self.commit.map(|commit| oid(commit.oid)).transpose()?,
        }))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadData {
    #[serde(flatten)]
    identity: Identity,
    review_threads: Option<Connection<ThreadWire>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepliesData {
    id: String,
    diff_side: Option<WireSide>,
    pull_request: Identity,
    comments: Option<Connection<Comment>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadWire {
    id: String,
    path: Option<String>,
    line: Option<u32>,
    subject_type: Option<ThreadSubject>,
    diff_side: Option<WireSide>,
    is_resolved: Option<bool>,
    is_outdated: Option<bool>,
    comments: Option<Connection<Comment>>,
}

impl ThreadWire {
    fn read(self, pull: &PullRequestSummary, partial: bool) -> Result<PrThread, AccessError> {
        let children = finish_connection(
            self.comments,
            None,
            partial,
            RateLimit::default(),
            None,
            |comment| comment.read(self.diff_side),
        )?;
        let location = match (
            self.subject_type,
            self.path,
            self.diff_side,
            self.line.filter(|line| *line > 0),
        ) {
            (Some(ThreadSubject::Line), Some(path), Some(side), Some(line)) => {
                Some(PrThreadLocation::Line {
                    path: checked_path(path)?,
                    side: side.into(),
                    line,
                })
            }
            (Some(ThreadSubject::File), Some(path), _, _) => Some(PrThreadLocation::File {
                path: checked_path(path)?,
            }),
            _ => None,
        };
        Ok(PrThread {
            node_id: opaque_id(self.id)?,
            resolved: self.is_resolved,
            outdated: self.is_outdated,
            location,
            comments: children.records,
            replies_complete: !children.partial && children.next_cursor.is_none(),
            reply_cursor: children.next_cursor,
            known_pull: pull.node_id.clone(),
            known_head: pull.head_oid.clone(),
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ThreadSubject {
    Line,
    File,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum WireSide {
    Left,
    Right,
}

impl From<WireSide> for PrSide {
    fn from(value: WireSide) -> Self {
        match value {
            WireSide::Left => Self::Left,
            WireSide::Right => Self::Right,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Comment {
    id: String,
    full_database_id: Option<GraphId>,
    reply_to: Option<ReplyTo>,
    author: Option<Author>,
    body: String,
    created_at: String,
    updated_at: String,
    path: String,
    original_line: Option<u32>,
    original_commit: Option<GraphCommit>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplyTo {
    full_database_id: Option<GraphId>,
}

impl Comment {
    fn read(self, side: Option<WireSide>) -> Result<Option<PrInlineComment>, AccessError> {
        let Some(id) = self.full_database_id.and_then(GraphId::id) else {
            return Ok(None);
        };
        let created_at_ms = millis(&self.created_at)?;
        let updated_at_ms = millis(&self.updated_at)?;
        if updated_at_ms < created_at_ms {
            return Err(malformed());
        }
        let anchor = match (
            side,
            self.original_line.filter(|line| *line > 0),
            self.original_commit,
        ) {
            (Some(side), Some(line), Some(commit)) => Some(PrReviewAnchor {
                path: checked_path(self.path)?,
                side: side.into(),
                line,
                original_oid: oid(commit.oid)?,
            }),
            _ => None,
        };
        Ok(Some(PrInlineComment {
            id,
            node_id: opaque_id(self.id)?,
            in_reply_to: self
                .reply_to
                .and_then(|reply| reply.full_database_id)
                .and_then(GraphId::id),
            actor: self
                .author
                .as_ref()
                .map(Author::identity)
                .unwrap_or_default(),
            author: self.author.map(|author| author.login),
            body: self.body,
            created_at_ms,
            updated_at_ms,
            anchor,
        }))
    }
}
