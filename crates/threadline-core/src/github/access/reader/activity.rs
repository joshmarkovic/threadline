//! Repository-scoped notification reads. A page contains identifiers and new activity,
//! independently of the user's downloaded PR conversation or comparison.
use super::*;
use crate::github::access::{GraphError, response::RequestMode};
use refresh::{ReadValidators, Refresh};
use serde::Deserialize;
use serde_json::json;

pub struct OpenPull {
    pub number: ItemNumber,
    pub id: NonZeroU64,
    pub node: String,
    pub updated_at: i64,
}

#[derive(Deserialize)]
struct IndexRow {
    number: NonZeroU64,
    id: NonZeroU64,
    node_id: String,
    updated_at: String,
    base: Base,
}
#[derive(Deserialize)]
struct Base {
    repo: RepositoryId,
}
#[derive(Deserialize)]
struct RepositoryId {
    id: NonZeroU64,
}

pub struct PullActivity {
    pub identity: OpenPull,
    pub title: String,
    pub closed: bool,
    pub merged: bool,
    pub base: String,
    pub head: String,
    pub events: Vec<PullEvent>,
    pub next: Option<String>,
    pub partial: bool,
    pub permission_limited: bool,
    pub rate: RateLimit,
}

pub enum PullEvent {
    Review {
        id: NonZeroU64,
        state: String,
        body: String,
        actor: ActorIdentity,
        at: i64,
    },
    Ready {
        at: i64,
        actor: ActorIdentity,
    },
    Merged {
        at: i64,
        actor: ActorIdentity,
        commit: Option<String>,
    },
    Requested {
        at: i64,
        actor: ActorIdentity,
        user: String,
    },
}

const QUERY: &str = r#"query ThreadlinePullActivity($owner:String!,$name:String!,$number:Int!,$since:DateTime!,$cursor:String) {
  repository(owner:$owner,name:$name) { databaseId name owner {login}
    pullRequest(number:$number) { id fullDatabaseId number title closed merged updatedAt baseRefOid headRefOid
      timelineItems(first:20,after:$cursor,since:$since,itemTypes:[PULL_REQUEST_REVIEW,READY_FOR_REVIEW_EVENT,MERGED_EVENT,REVIEW_REQUESTED_EVENT]) {
        pageInfo { hasNextPage endCursor } nodes { __typename
          ... on PullRequestReview { fullDatabaseId state body submittedAt author { ...ActorFacts } }
          ... on ReadyForReviewEvent { createdAt actor { ...ActorFacts } }
          ... on MergedEvent { createdAt actor { ...ActorFacts } commit {oid} }
          ... on ReviewRequestedEvent { createdAt actor { ...ActorFacts } requestedReviewer { ... on User { login } ... on Team { slug } } }
        }
      }
    }
  }
}
fragment ActorFacts on Actor { __typename ... on User { databaseId } ... on Bot { databaseId } }"#;

impl GitHubReader<'_> {
    pub async fn open_pull_index(
        &self,
        page: u32,
        validators: &ReadValidators,
        cancel: &CancellationToken,
    ) -> Result<Refresh<Page<OpenPull>>, AccessError> {
        self.refresh_page::<IndexRow, _>(
            "pulls",
            page,
            &[("state", "open"), ("sort", "created"), ("direction", "asc")],
            validators,
            cancel,
            |row| {
                if row.base.repo.id.get() != self.repository.get() {
                    return Err(pr::identity_mismatch());
                }
                Ok(OpenPull {
                    number: ItemNumber::new(row.number),
                    id: row.id,
                    node: wire::opaque_id(row.node_id)?,
                    updated_at: wire::millis(&row.updated_at)?,
                })
            },
        )
        .await
    }

    pub async fn pull_activity(
        &self,
        number: ItemNumber,
        since_ms: i64,
        cursor: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<PullActivity, AccessError> {
        if cursor.is_some_and(|c| !pr::graph::valid_cursor(c)) {
            return Err(AccessError::local(AccessKind::InvalidPagination));
        }
        let query = json!({"query":QUERY,"variables":{"owner":self.route.owner(),"name":self.route.name(),
            "number":number.get(),"since":refresh::since(since_ms)?,"cursor":cursor}});
        let (reply, headers) = self
            .get_refresh::<Envelope>("graphql", RequestMode::Graph(&query), cancel)
            .await?
            .require_body()?;
        let repository = reply
            .data
            .and_then(|v| v.repository)
            .ok_or_else(|| pr::graph::graph_failure(&reply.errors))?;
        if repository.database_id.get() != self.repository.get()
            || !repository.name.eq_ignore_ascii_case(self.route.name())
            || !repository
                .owner
                .login
                .eq_ignore_ascii_case(self.route.owner())
        {
            return Err(pr::identity_mismatch());
        }
        let row = repository
            .pull_request
            .ok_or_else(|| pr::graph::graph_failure(&reply.errors))?;
        if row.number.get() != number.get() {
            return Err(pr::identity_mismatch());
        }
        let mut partial = !reply.errors.is_empty();
        let permission_limited = partial
            && matches!(
                pr::graph::graph_failure(&reply.errors).kind,
                AccessKind::PermissionDenied | AccessKind::SsoRequired
            );
        let mut next = None;
        let mut events = Vec::new();
        if let Some(timeline) = row.timeline_items {
            match timeline.page_info {
                Some(info) if info.has_next_page => {
                    next = info
                        .end_cursor
                        .filter(|v| pr::graph::valid_cursor(v) && Some(v.as_str()) != cursor);
                    partial |= next.is_none();
                }
                Some(_) => (),
                None => partial = true,
            }
            match timeline.nodes {
                Some(rows) if rows.len() <= 20 => {
                    for entry in rows {
                        match entry {
                            Some(entry) => {
                                if let Some(event) = entry.event()? {
                                    events.push(event);
                                }
                            }
                            None => partial = true,
                        }
                    }
                }
                Some(_) => return Err(malformed()),
                None => partial = true,
            }
        } else {
            partial = true;
        }
        Ok(PullActivity {
            identity: OpenPull {
                number,
                id: row.full_database_id.number()?,
                node: wire::opaque_id(row.id)?,
                updated_at: wire::millis(&row.updated_at)?,
            },
            title: row.title,
            closed: row.closed,
            merged: row.merged,
            base: wire::oid(row.base_ref_oid)?,
            head: wire::oid(row.head_ref_oid)?,
            events,
            next,
            partial,
            permission_limited,
            rate: RateLimit::from_headers(&headers),
        })
    }
}

#[derive(Deserialize)]
struct Envelope {
    data: Option<Data>,
    #[serde(default)]
    errors: Vec<GraphError>,
}
#[derive(Deserialize)]
struct Data {
    repository: Option<Repository>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repository {
    database_id: NonZeroU64,
    name: String,
    owner: Login,
    pull_request: Option<Pull>,
}
#[derive(Deserialize)]
struct Login {
    login: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pull {
    id: String,
    full_database_id: LargeId,
    number: NonZeroU64,
    title: String,
    closed: bool,
    merged: bool,
    updated_at: String,
    base_ref_oid: String,
    head_ref_oid: String,
    timeline_items: Option<Timeline>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum LargeId {
    Number(NonZeroU64),
    Text(String),
}
impl LargeId {
    fn number(self) -> Result<NonZeroU64, AccessError> {
        match self {
            Self::Number(id) => Ok(id),
            Self::Text(raw) => raw.parse().map_err(|_| malformed()),
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Timeline {
    page_info: Option<PageInfo>,
    nodes: Option<Vec<Option<TimelineNode>>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}
#[derive(Deserialize)]
struct Actor {
    #[serde(rename = "__typename")]
    kind: String,
    #[serde(rename = "databaseId")]
    id: Option<NonZeroU64>,
}
impl Actor {
    fn facts(self) -> ActorIdentity {
        ActorIdentity {
            user_id: self
                .id
                .filter(|_| matches!(self.kind.as_str(), "User" | "Bot")),
            app_id: None,
            known_automation: self.kind == "Bot",
        }
    }
}
#[derive(Deserialize)]
struct Commit {
    oid: String,
}
#[derive(Deserialize)]
struct Reviewer {
    login: Option<String>,
    slug: Option<String>,
}
#[derive(Deserialize)]
#[serde(tag = "__typename", rename_all_fields = "camelCase")]
enum TimelineNode {
    PullRequestReview {
        full_database_id: LargeId,
        state: String,
        body: String,
        submitted_at: Option<String>,
        author: Option<Actor>,
    },
    ReadyForReviewEvent {
        created_at: String,
        actor: Option<Actor>,
    },
    MergedEvent {
        created_at: String,
        actor: Option<Actor>,
        commit: Option<Commit>,
    },
    ReviewRequestedEvent {
        created_at: String,
        actor: Option<Actor>,
        requested_reviewer: Option<Reviewer>,
    },
}
impl TimelineNode {
    fn event(self) -> Result<Option<PullEvent>, AccessError> {
        let facts = |actor: Option<Actor>| actor.map(Actor::facts).unwrap_or_default();
        Ok(Some(match self {
            Self::PullRequestReview {
                full_database_id,
                state,
                body,
                submitted_at: Some(at),
                author,
            } if matches!(
                state.as_str(),
                "APPROVED" | "CHANGES_REQUESTED" | "COMMENTED"
            ) =>
            {
                PullEvent::Review {
                    id: full_database_id.number()?,
                    state,
                    body,
                    at: wire::millis(&at)?,
                    actor: facts(author),
                }
            }
            Self::PullRequestReview { .. } => return Ok(None),
            Self::ReadyForReviewEvent { created_at, actor } => PullEvent::Ready {
                at: wire::millis(&created_at)?,
                actor: facts(actor),
            },
            Self::MergedEvent {
                created_at,
                actor,
                commit,
            } => PullEvent::Merged {
                at: wire::millis(&created_at)?,
                actor: facts(actor),
                commit: commit.map(|v| wire::oid(v.oid)).transpose()?,
            },
            Self::ReviewRequestedEvent {
                created_at,
                actor,
                requested_reviewer,
            } => {
                let Some(user) = requested_reviewer.and_then(|r| {
                    r.login
                        .or_else(|| r.slug.map(|slug| format!("team:{slug}")))
                }) else {
                    return Ok(None);
                };
                PullEvent::Requested {
                    at: wire::millis(&created_at)?,
                    actor: facts(actor),
                    user,
                }
            }
        }))
    }
}
