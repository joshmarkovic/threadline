//! Mutations limited to existing issues and ordinary comments under a verified GitHub identity.

use std::collections::{BTreeMap, BTreeSet};

use reqwest::Method;

use crate::domain::ItemNumber;

use super::{
    response::{RequestMode, map_status, request},
    *,
};

pub mod merge;
mod model;
pub mod review;
mod wire;

pub use model::*;
use wire::*;

pub struct GitHubWriter<'a> {
    session: &'a GitHubSession,
    repository: RepositoryAccess,
}

impl GitHubSession {
    pub async fn issue_writer(
        &self,
        route: &RepositoryName,
        repository: RepositoryApiId,
        cancel: &CancellationToken,
    ) -> Result<(GitHubWriter<'_>, RateLimit), AccessError> {
        let (checked, rate) = self
            .repository_with_guidance(route, Some(repository), None, cancel)
            .await?;
        let RepositoryRead::Fresh {
            repository: checked,
            ..
        } = checked
        else {
            return Err(AccessError::received(AccessKind::UnexpectedStatus, 304));
        };
        Ok((
            GitHubWriter {
                session: self,
                repository: checked,
            },
            rate,
        ))
    }
}

#[derive(Clone, Copy)]
pub enum IssueChoices {
    Labels,
    Assignees,
    Milestones,
}

pub enum ChoiceValues {
    Labels(BTreeSet<String>),
    Assignees(BTreeSet<String>),
    Milestones(BTreeMap<NonZeroU64, String>),
}

pub struct ChoicePage {
    pub values: ChoiceValues,
    pub next: Option<NonZeroU32>,
    pub sso_limited: bool,
    pub rate: RateLimit,
}

impl GitHubWriter<'_> {
    pub fn actor(&self) -> NonZeroU64 {
        self.session.user.id
    }

    pub fn repository(&self) -> RepositoryApiId {
        self.repository.id
    }

    pub fn is_private(&self) -> bool {
        self.repository.private
    }

    /// A scoped, first-page check used before a one-time disposable issue verification.
    pub async fn has_ordinary_comments(
        &self,
        issue: &VerifiedIssue,
        cancel: &CancellationToken,
    ) -> Result<bool, AccessError> {
        self.check_issue(issue)?;
        let path = format!(
            "issues/{}/comments?per_page=1&page=1",
            issue.target.number.get()
        );
        let (rows, headers): (Vec<serde_json::Value>, _) = self.get(&path, cancel).await?;
        if partial_sso(&headers).is_some()
            || (rows.is_empty() && headers.contains_key(header::LINK))
        {
            return Err(AccessError::received(AccessKind::MalformedResponse, 200));
        }
        Ok(!rows.is_empty())
    }

    pub async fn observe_issue(
        &self,
        target: IssueWriteTarget,
        cancel: &CancellationToken,
    ) -> Result<(VerifiedIssue, RateLimit), AccessError> {
        let (wire, headers) = self
            .get(&format!("issues/{}", target.number.get()), cancel)
            .await?;
        Ok((self.issue(wire, target)?, RateLimit::from_headers(&headers)))
    }

    pub async fn observe_comment(
        &self,
        issue: &VerifiedIssue,
        id: NonZeroU64,
        cancel: &CancellationToken,
    ) -> Result<(VerifiedComment, RateLimit), AccessError> {
        self.check_issue(issue)?;
        let (wire, headers) = self.get(&format!("issues/comments/{id}"), cancel).await?;
        Ok((
            self.comment(wire, issue.target, Some(id))?,
            RateLimit::from_headers(&headers),
        ))
    }

    pub async fn issue_choices(
        &self,
        kind: IssueChoices,
        page: NonZeroU32,
        cancel: &CancellationToken,
    ) -> Result<ChoicePage, AccessError> {
        let endpoint = match kind {
            IssueChoices::Labels => "labels",
            IssueChoices::Assignees => "assignees",
            IssueChoices::Milestones => "milestones",
        };
        let mut route = format!("{endpoint}?per_page=100&page={page}");
        if matches!(kind, IssueChoices::Milestones) {
            route.push_str("&state=all");
        }
        let (values, headers) = match kind {
            IssueChoices::Labels => {
                let (rows, headers): (Vec<LabelWire>, _) = self.get(&route, cancel).await?;
                (
                    ChoiceValues::Labels(rows.into_iter().map(|row| row.name).collect()),
                    headers,
                )
            }
            IssueChoices::Assignees => {
                let (rows, headers): (Vec<UserWire>, _) = self.get(&route, cancel).await?;
                (
                    ChoiceValues::Assignees(rows.into_iter().map(|row| row.login).collect()),
                    headers,
                )
            }
            IssueChoices::Milestones => {
                let (rows, headers): (Vec<MilestoneWire>, _) = self.get(&route, cancel).await?;
                (
                    ChoiceValues::Milestones(
                        rows.into_iter()
                            .map(|row| (row.number, row.title))
                            .collect(),
                    ),
                    headers,
                )
            }
        };
        let path = self.path(&route);
        let request = self.url(&path)?;
        let permanent = format!("/repositories/{}/{endpoint}", self.repository.id.get());
        let next = next_page_for(&headers, &request, page.get(), |path| {
            path.eq_ignore_ascii_case(request.path()) || path == permanent
        })?;
        let sso_limited = partial_sso(&headers).is_some();
        Ok(ChoicePage {
            values,
            next,
            sso_limited,
            rate: RateLimit::from_headers(&headers),
        })
    }

    /// Repository roles and object ownership guide preflight. They do not prove PAT write access.
    pub fn permission(
        &self,
        issue: &VerifiedIssue,
        mutation: &IssueMutation<'_>,
    ) -> WritePermission {
        if self.check_issue(issue).is_err() {
            return WritePermission::Unknown;
        }
        let role = self.repository.permissions.as_ref();
        let can_manage = role.is_some_and(|p| p.admin || p.maintain || p.push || p.triage);
        let own_issue = issue.author == Some(self.actor());
        let denied_if_known = || {
            if role.is_some() {
                WritePermission::Denied
            } else {
                WritePermission::Unknown
            }
        };
        match mutation {
            IssueMutation::EditIssue(patch) => {
                if issue.target.pull_request {
                    return WritePermission::Denied;
                }
                let changes_metadata = patch.labels.is_some()
                    || patch.assignees.is_some()
                    || patch.milestone.is_some();
                if can_manage || (own_issue && !changes_metadata) {
                    WritePermission::Allowed
                } else if changes_metadata || issue.author.is_some() {
                    denied_if_known()
                } else {
                    WritePermission::Unknown
                }
            }
            IssueMutation::CreateComment { .. } => match issue.locked {
                Some(false) => WritePermission::Allowed,
                Some(true) if can_manage => WritePermission::Allowed,
                Some(true) => denied_if_known(),
                None => WritePermission::Unknown,
            },
            IssueMutation::EditComment { comment, .. } => {
                if self.check_comment(issue, comment).is_err() {
                    return WritePermission::Unknown;
                }
                match comment.author_id {
                    Some(author) if author == self.actor() => WritePermission::Allowed,
                    Some(_) => WritePermission::Denied,
                    None => WritePermission::Unknown,
                }
            }
        }
    }

    /// Send one request. A connection failure after dispatch never authorizes retry by itself.
    pub async fn mutate(
        &self,
        issue: &VerifiedIssue,
        mutation: IssueMutation<'_>,
        cancel: &CancellationToken,
    ) -> Result<MutationResponse, MutationFailure> {
        let unsent = |access| MutationFailure {
            evidence: DispatchEvidence::DefinitelyUnsent,
            access,
        };
        self.check_issue(issue).map_err(unsent)?;
        if self.permission(issue, &mutation) != WritePermission::Allowed {
            return Err(MutationFailure {
                evidence: DispatchEvidence::AccessRejected,
                access: AccessError::local(AccessKind::PermissionDenied),
            });
        }
        let request = match &mutation {
            IssueMutation::EditIssue(patch) if !issue.target.pull_request => Request::new(
                Method::PATCH,
                format!("issues/{}", issue.target.number.get()),
                StatusCode::OK,
                patch,
            ),
            IssueMutation::CreateComment { body } => Request::new(
                Method::POST,
                format!("issues/{}/comments", issue.target.number.get()),
                StatusCode::CREATED,
                &serde_json::json!({"body": body}),
            ),
            IssueMutation::EditComment { comment, body } => {
                self.check_comment(issue, comment).map_err(unsent)?;
                Request::new(
                    Method::PATCH,
                    format!("issues/comments/{}", comment.id),
                    StatusCode::OK,
                    &serde_json::json!({"body": body}),
                )
            }
            IssueMutation::EditIssue(_) => {
                return Err(unsent(AccessError::local(AccessKind::WrongScope)));
            }
        }
        .map_err(unsent)?;
        let expected = request.status;
        let (bytes, headers) = self.send(request, cancel).await?;
        let invalid = |kind| ambiguous(AccessError::received(kind, expected.as_u16()), &headers);
        let object = match mutation {
            IssueMutation::EditIssue(_) => {
                let decoded = serde_json::from_slice(&bytes)
                    .map_err(|_| invalid(AccessKind::MalformedResponse))?;
                MutationObject::Issue(
                    self.issue(decoded, issue.target)
                        .map_err(|error| invalid(error.kind))?,
                )
            }
            IssueMutation::CreateComment { body } | IssueMutation::EditComment { body, .. } => {
                let id = match mutation {
                    IssueMutation::EditComment { comment, .. } => Some(comment.id),
                    _ => None,
                };
                let decoded = serde_json::from_slice(&bytes)
                    .map_err(|_| invalid(AccessKind::MalformedResponse))?;
                let comment = self
                    .comment(decoded, issue.target, id)
                    .map_err(|error| invalid(error.kind))?;
                // A positive response must identify the exact signed-in author's content and target.
                if comment.author_id != Some(self.actor()) || comment.body != body {
                    return Err(invalid(AccessKind::IdentityMismatch));
                }
                MutationObject::Comment(comment)
            }
        };
        let request_id = transport::request_id(&headers)
            .ok_or_else(|| invalid(AccessKind::MalformedResponse))?;
        Ok(MutationResponse {
            object,
            request_id,
            rate: RateLimit::from_headers(&headers),
        })
    }

    async fn get<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        cancel: &CancellationToken,
    ) -> Result<(T, HeaderMap), AccessError> {
        let session = self.session;
        request(
            &session.client,
            &session.token,
            &session.scope,
            &self.path(endpoint),
            RequestMode::Read,
            cancel,
        )
        .await?
        .require_body()
    }

    fn path(&self, endpoint: &str) -> String {
        let route = &self.repository.route;
        format!("repos/{}/{}/{endpoint}", route.owner(), route.name())
    }

    fn url(&self, path: &str) -> Result<Url, AccessError> {
        self.session
            .client
            .base
            .join(path)
            .map_err(|_| AccessError::local(AccessKind::WrongScope))
    }

    fn matches_route(&self, url: &str, issue_number: Option<ItemNumber>) -> bool {
        let route = &self.repository.route;
        let mut expected = format!("/repos/{}/{}", route.owner(), route.name());
        if let Some(number) = issue_number {
            use std::fmt::Write;
            let _ = write!(expected, "/issues/{}", number.get());
        }
        Url::parse(url).ok().is_some_and(|url| {
            url.scheme() == "https"
                && url.host_str() == Some("api.github.com")
                && url.port().is_none()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.path().eq_ignore_ascii_case(&expected)
        })
    }

    fn check_issue(&self, issue: &VerifiedIssue) -> Result<(), AccessError> {
        if issue.repository != self.repository.id || issue.actor != self.actor() {
            return Err(AccessError::local(AccessKind::WrongScope));
        }
        Ok(())
    }

    fn check_comment(
        &self,
        issue: &VerifiedIssue,
        comment: &VerifiedComment,
    ) -> Result<(), AccessError> {
        if comment.repository != issue.repository || comment.issue != issue.target {
            return Err(AccessError::local(AccessKind::WrongScope));
        }
        Ok(())
    }

    fn issue(
        &self,
        wire: IssueWire,
        target: IssueWriteTarget,
    ) -> Result<VerifiedIssue, AccessError> {
        if wire.id != target.issue_id
            || wire.number.get() != target.number.get()
            || wire.pull_request.is_some() != target.pull_request
            || !self.matches_route(&wire.repository_url, None)
        {
            return Err(AccessError::received(AccessKind::IdentityMismatch, 200));
        }
        let closed = match wire.state.as_deref() {
            Some("open") => Some(false),
            Some("closed") => Some(true),
            None => None,
            Some(_) => return Err(AccessError::received(AccessKind::MalformedResponse, 200)),
        };
        let milestone_title = wire
            .milestone
            .as_ref()
            .and_then(Option::as_ref)
            .map(|value| value.title.clone());
        let observed = IssueWriteValues {
            title: wire.title,
            body: wire.body,
            closed,
            labels: wire
                .labels
                .map(|labels| labels.into_iter().map(|label| label.name).collect()),
            assignees: wire
                .assignees
                .map(|users| users.into_iter().map(|user| user.login).collect()),
            milestone: wire.milestone.map(|milestone| milestone.map(|m| m.number)),
        };
        Ok(VerifiedIssue {
            repository: self.repository(),
            target,
            actor: self.actor(),
            author: wire.user.map(|user| user.id),
            locked: wire.locked,
            observed,
            milestone_title,
            updated_at_ms: timestamp(&wire.updated_at)?,
        })
    }

    fn comment(
        &self,
        wire: CommentWire,
        issue: IssueWriteTarget,
        id: Option<NonZeroU64>,
    ) -> Result<VerifiedComment, AccessError> {
        if id.is_some_and(|id| id != wire.id)
            || !self.matches_route(&wire.issue_url, Some(issue.number))
        {
            return Err(AccessError::received(AccessKind::IdentityMismatch, 200));
        }
        Ok(VerifiedComment {
            repository: self.repository(),
            issue,
            id: wire.id,
            author_id: wire.user.as_ref().map(|user| user.id),
            author: wire.user.map(|user| user.login),
            body: wire.body,
            created_at_ms: timestamp(&wire.created_at)?,
            updated_at_ms: timestamp(&wire.updated_at)?,
        })
    }
}

mod transport;
use transport::{Request, ambiguous};
