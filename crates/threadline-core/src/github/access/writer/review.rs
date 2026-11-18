//! Explicit revision-bound access to native GitHub review actions.

use super::{
    super::reader::{GitHubReader, refresh::ReadValidators},
    transport::Request,
    *,
};

mod graph;
mod model;
mod write;
pub use model::*;

use wire::UserWire;

#[derive(Deserialize)]
struct ReviewWire {
    id: NonZeroU64,
    user: Option<UserWire>,
    commit_id: String,
    state: String,
    pull_request_url: String,
}

#[derive(Deserialize)]
struct ReplyWire {
    id: NonZeroU64,
    user: UserWire,
    body: String,
    in_reply_to_id: NonZeroU64,
    pull_request_url: String,
}

impl GitHubWriter<'_> {
    pub async fn observe_pull_for_review(
        &self,
        number: ItemNumber,
        id: NonZeroU64,
        cancel: &CancellationToken,
    ) -> Result<(VerifiedPull, RateLimit), AccessError> {
        let reader = GitHubReader::from_access(self.session, &self.repository);
        let response = reader
            .pr_snapshot(number, &ReadValidators::default(), cancel)
            .await?;
        let snapshot = response
            .value
            .ok_or_else(|| AccessError::received(AccessKind::UnexpectedStatus, 304))?;
        let details = snapshot.details();
        if details.pull_request_id != id {
            return Err(AccessError::received(AccessKind::IdentityMismatch, 200));
        }
        Ok((
            VerifiedPull {
                actor: self.actor(),
                repository: self.repository(),
                number,
                id,
                node_id: details.node_id.clone(),
                base: details.base_oid.clone(),
                head: details.head_oid.clone(),
                open: !details.closed && !details.merged,
                author: details.presentation.author_id,
                locked: details.presentation.locked,
                draft: details.presentation.draft,
            },
            response.rate,
        ))
    }

    /// Repository entitlement and action-specific ownership, independent of the PAT write grant.
    pub fn review_permission(
        &self,
        pull: &VerifiedPull,
        event: Option<ReviewEvent>,
    ) -> WritePermission {
        if self.check_pull(pull).is_err() || !pull.open {
            return WritePermission::Denied;
        }
        if matches!(
            event,
            Some(ReviewEvent::Approve | ReviewEvent::RequestChanges)
        ) {
            match (pull.author, pull.draft) {
                (Some(author), _) if author == pull.actor => return WritePermission::Denied,
                (_, Some(true)) => return WritePermission::Denied,
                (None, _) | (_, None) => return WritePermission::Unknown,
                _ => {}
            }
        }
        let Some(role) = self.repository.permissions.as_ref() else {
            return WritePermission::Unknown;
        };
        match pull.locked {
            Some(false) if role.pull => WritePermission::Allowed,
            Some(true) if role.admin || role.maintain || role.push || role.triage => {
                WritePermission::Allowed
            }
            Some(_) => WritePermission::Denied,
            None => WritePermission::Unknown,
        }
    }

    pub async fn observe_own_pending(
        &self,
        pull: &VerifiedPull,
        page: NonZeroU32,
        cancel: &CancellationToken,
    ) -> Result<((PendingReview, Option<NonZeroU32>), RateLimit), AccessError> {
        self.check_pull(pull)?;
        let endpoint = format!(
            "pulls/{}/reviews?per_page=100&page={page}",
            pull.number.get()
        );
        let (rows, headers): (Vec<ReviewWire>, _) = self.get(&endpoint, cancel).await?;
        let url = self.url(&self.path(&endpoint))?;
        let permanent = format!(
            "/repositories/{}/pulls/{}/reviews",
            self.repository().get(),
            pull.number.get()
        );
        let next = next_page_for(&headers, &url, page.get(), |path| {
            path.eq_ignore_ascii_case(url.path()) || path == permanent
        })?;
        let mut own = PendingReview::Absent;
        for raw in rows.into_iter().filter(|wire| wire.state == "PENDING") {
            let Some(user) = &raw.user else {
                own = PendingReview::Unknown;
                break;
            };
            if user.id == self.actor() {
                if !matches!(own, PendingReview::Absent) {
                    own = PendingReview::Unknown;
                    break;
                }
                own = PendingReview::Present(self.review(raw, pull, None)?);
            }
        }
        if partial_sso(&headers).is_some() && matches!(own, PendingReview::Absent) {
            own = PendingReview::Unknown;
        }
        Ok(((own, next), RateLimit::from_headers(&headers)))
    }

    pub async fn observe_review_object(
        &self,
        pull: &VerifiedPull,
        id: NonZeroU64,
        cancel: &CancellationToken,
    ) -> Result<(ObservedReview, RateLimit), AccessError> {
        self.check_pull(pull)?;
        let (raw, headers) = self
            .get(&format!("pulls/{}/reviews/{id}", pull.number.get()), cancel)
            .await?;
        Ok((
            self.review(raw, pull, Some(id))?,
            RateLimit::from_headers(&headers),
        ))
    }

    pub async fn observe_review_reply(
        &self,
        pull: &VerifiedPull,
        id: NonZeroU64,
        cancel: &CancellationToken,
    ) -> Result<(ObservedReply, RateLimit), AccessError> {
        self.check_pull(pull)?;
        let (raw, headers) = self.get(&format!("pulls/comments/{id}"), cancel).await?;
        Ok((
            self.reply(raw, pull, Some(id))?,
            RateLimit::from_headers(&headers),
        ))
    }

    fn review(
        &self,
        raw: ReviewWire,
        pull: &VerifiedPull,
        id: Option<NonZeroU64>,
    ) -> Result<ObservedReview, AccessError> {
        if id.is_some_and(|id| id != raw.id)
            || !self.check_pull_url(&raw.pull_request_url, pull.number)
        {
            return Err(AccessError::received(AccessKind::IdentityMismatch, 200));
        }
        let author = raw
            .user
            .map(|user| user.id)
            .ok_or_else(|| AccessError::received(AccessKind::MalformedResponse, 200))?;
        if raw.commit_id.len() != 40 || !raw.commit_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(AccessError::received(AccessKind::MalformedResponse, 200));
        }
        let state = match raw.state.as_str() {
            "PENDING" => ReviewState::Pending,
            "COMMENTED" => ReviewState::Submitted(ReviewEvent::Comment),
            "APPROVED" => ReviewState::Submitted(ReviewEvent::Approve),
            "CHANGES_REQUESTED" => ReviewState::Submitted(ReviewEvent::RequestChanges),
            _ => ReviewState::Other,
        };
        Ok(ObservedReview {
            repository: self.repository(),
            pull: pull.id,
            id: raw.id,
            author,
            head: raw.commit_id.to_ascii_lowercase(),
            state,
        })
    }

    fn reply(
        &self,
        raw: ReplyWire,
        pull: &VerifiedPull,
        id: Option<NonZeroU64>,
    ) -> Result<ObservedReply, AccessError> {
        if id.is_some_and(|id| id != raw.id)
            || !self.check_pull_url(&raw.pull_request_url, pull.number)
        {
            return Err(AccessError::received(AccessKind::IdentityMismatch, 200));
        }
        Ok(ObservedReply {
            id: raw.id,
            author: raw.user.id,
            root: raw.in_reply_to_id,
            body: raw.body,
        })
    }

    fn check_pull(&self, pull: &VerifiedPull) -> Result<(), AccessError> {
        if pull.repository != self.repository() || pull.actor != self.actor() {
            return Err(AccessError::local(AccessKind::WrongScope));
        }
        Ok(())
    }

    fn check_pull_url(&self, value: &str, number: ItemNumber) -> bool {
        let path = format!("/{}", self.path(&format!("pulls/{}", number.get())));
        Url::parse(value).ok().is_some_and(|url| {
            url.scheme() == "https"
                && url.host_str() == Some("api.github.com")
                && url.port().is_none()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.path().eq_ignore_ascii_case(&path)
        })
    }
}
