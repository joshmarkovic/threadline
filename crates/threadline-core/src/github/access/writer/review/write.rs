use super::*;
use crate::github::access::writer::transport::{ambiguous, request_id};

fn local_failure(kind: AccessKind, evidence: DispatchEvidence) -> MutationFailure {
    MutationFailure {
        access: AccessError::local(kind),
        evidence,
    }
}

impl GitHubWriter<'_> {
    pub async fn mutate_review(
        &self,
        pull: &VerifiedPull,
        action: ReviewMutation<'_>,
        cancel: &CancellationToken,
    ) -> Result<ReviewMutationResponse, MutationFailure> {
        self.check_pull(pull)
            .map_err(|error| local_failure(error.kind, DispatchEvidence::DefinitelyUnsent))?;
        let request = self.review_request(pull, &action)?;
        let status = request.status;
        let (raw, headers) = self.send(request, cancel).await?;
        let invalid = |kind| ambiguous(AccessError::received(kind, status.as_u16()), &headers);
        let object = self
            .review_response(pull, action, &raw)
            .map_err(|error| invalid(error.kind))?;
        Ok(ReviewMutationResponse {
            object,
            request_id: request_id(&headers)
                .ok_or_else(|| invalid(AccessKind::MalformedResponse))?,
            rate: RateLimit::from_headers(&headers),
        })
    }

    fn review_request(
        &self,
        pull: &VerifiedPull,
        action: &ReviewMutation<'_>,
    ) -> Result<Request, MutationFailure> {
        let denied = || {
            local_failure(
                AccessKind::PermissionDenied,
                DispatchEvidence::AccessRejected,
            )
        };
        let invalid = || local_failure(AccessKind::WrongScope, DispatchEvidence::InvalidInput);
        let number = pull.number.get();
        let request = match action {
            ReviewMutation::CreatePending {
                pending,
                summary,
                comments,
            } => {
                if self.review_permission(pull, None) != WritePermission::Allowed
                    || !matches!(pending, PendingReview::Absent)
                {
                    return Err(denied());
                }
                if comments.len() > 100
                    || summary.contains('\0')
                    || comments.iter().any(|row| {
                        row.path.is_empty()
                            || row.path.len() > 4096
                            || row.path.contains('\0')
                            || row.body.trim().is_empty()
                            || row.body.contains('\0')
                            || row.line == 0
                            || row
                                .start_line
                                .is_some_and(|start| start == 0 || start >= row.line)
                            || row.start_line.is_some() != row.start_side.is_some()
                    })
                {
                    return Err(invalid());
                }
                Request::new(
                    Method::POST,
                    format!("pulls/{number}/reviews"),
                    StatusCode::OK,
                    &serde_json::json!({
                        "commit_id":pull.head,"body":summary,"comments":comments
                    }),
                )
            }
            ReviewMutation::SubmitPending {
                review,
                event,
                body,
            } => {
                if self.review_permission(pull, Some(*event)) != WritePermission::Allowed {
                    return Err(denied());
                }
                if review.repository != pull.repository
                    || review.pull != pull.id
                    || review.author != pull.actor
                    || review.head != pull.head
                    || review.state != ReviewState::Pending
                    || body.contains('\0')
                {
                    return Err(invalid());
                }
                Request::new(
                    Method::POST,
                    format!("pulls/{number}/reviews/{}/events", review.id),
                    StatusCode::OK,
                    &serde_json::json!({"event":event,"body":body}),
                )
            }
            ReviewMutation::Reply { thread, root, body } => {
                if self.review_permission(pull, None) != WritePermission::Allowed {
                    return Err(denied());
                }
                if !valid_thread(pull, thread)
                    || thread.root != Some(*root)
                    || body.trim().is_empty()
                    || body.contains('\0')
                {
                    return Err(invalid());
                }
                Request::new(
                    Method::POST,
                    format!("pulls/{number}/comments/{root}/replies"),
                    StatusCode::CREATED,
                    &serde_json::json!({"body":body}),
                )
            }
            ReviewMutation::SetThreadResolved {
                thread,
                resolved,
                correlation,
            } => {
                if thread.permission(*resolved) != WritePermission::Allowed || !pull.open {
                    return Err(denied());
                }
                if !valid_thread(pull, thread) || thread.resolved != Some(!resolved) {
                    return Err(invalid());
                }
                graph::set_resolved(thread, *resolved, correlation)
            }
        };
        request.map_err(|error| local_failure(error.kind, DispatchEvidence::InvalidInput))
    }

    fn review_response(
        &self,
        pull: &VerifiedPull,
        action: ReviewMutation<'_>,
        body: &[u8],
    ) -> Result<ReviewObject, AccessError> {
        let malformed = || AccessError::received(AccessKind::MalformedResponse, 200);
        let mismatch = || AccessError::received(AccessKind::IdentityMismatch, 200);
        match action {
            ReviewMutation::CreatePending { .. } => {
                let review = self.review(
                    serde_json::from_slice(body).map_err(|_| malformed())?,
                    pull,
                    None,
                )?;
                if review.author != pull.actor
                    || review.head != pull.head
                    || review.state != ReviewState::Pending
                {
                    return Err(mismatch());
                }
                Ok(ReviewObject::Review(review))
            }
            ReviewMutation::SubmitPending { review, event, .. } => {
                let output = self.review(
                    serde_json::from_slice(body).map_err(|_| malformed())?,
                    pull,
                    Some(review.id),
                )?;
                if output.author != pull.actor
                    || output.head != pull.head
                    || output.state != ReviewState::Submitted(event)
                {
                    return Err(mismatch());
                }
                Ok(ReviewObject::Review(output))
            }
            ReviewMutation::Reply {
                root,
                body: expected,
                ..
            } => {
                let reply = self.reply(
                    serde_json::from_slice(body).map_err(|_| malformed())?,
                    pull,
                    None,
                )?;
                if reply.root != root || reply.author != pull.actor || reply.body != expected {
                    return Err(mismatch());
                }
                Ok(ReviewObject::Reply(reply))
            }
            ReviewMutation::SetThreadResolved {
                thread,
                resolved,
                correlation,
            } => {
                let reply: serde_json::Value =
                    serde_json::from_slice(body).map_err(|_| malformed())?;
                let change = graph::graph_data(&reply, "/data/change")?;
                if change["clientMutationId"] != correlation {
                    return Err(mismatch());
                }
                let observed = self.thread(&change["thread"], pull, &thread.id)?;
                if observed.resolved != Some(resolved) {
                    return Err(mismatch());
                }
                Ok(ReviewObject::Thread {
                    id: observed.id,
                    resolved,
                })
            }
        }
    }
}

fn valid_thread(pull: &VerifiedPull, thread: &ObservedThread) -> bool {
    thread.repository == pull.repository
        && thread.pull == pull.id
        && thread.head == pull.head
        && thread.outdated == Some(false)
}
