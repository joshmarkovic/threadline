use super::*;

const THREAD_FIELDS: &str = "id isOutdated isResolved viewerCanResolve viewerCanUnresolve pullRequest { id number headRefOid baseRefOid repository { id } } comments(first:1) { nodes { fullDatabaseId } }";

impl GitHubWriter<'_> {
    pub async fn observe_review_thread(
        &self,
        pull: &VerifiedPull,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<(ObservedThread, RateLimit), AccessError> {
        self.check_pull(pull)?;
        if !valid_node(id) {
            return Err(AccessError::local(AccessKind::WrongScope));
        }
        let query = format!(
            "query ThreadlineReviewThread($id:ID!) {{ node(id:$id) {{ ... on PullRequestReviewThread {{ {THREAD_FIELDS} }} }} }}"
        );
        let body = serde_json::json!({"query":query,"variables":{"id":id}});
        let session = self.session;
        let (response, headers): (serde_json::Value, _) = request(
            &session.client,
            &session.token,
            &session.scope,
            "graphql",
            RequestMode::Graph(&body),
            cancel,
        )
        .await?
        .require_body()?;
        let node = graph_data(&response, "/data/node")?;
        Ok((
            self.thread(node, pull, id)?,
            RateLimit::from_headers(&headers),
        ))
    }

    pub(super) fn thread(
        &self,
        node: &serde_json::Value,
        pull: &VerifiedPull,
        id: &str,
    ) -> Result<ObservedThread, AccessError> {
        if node["id"] != id
            || node
                .pointer("/pullRequest/id")
                .and_then(serde_json::Value::as_str)
                != Some(&pull.node_id)
            || node
                .pointer("/pullRequest/number")
                .and_then(serde_json::Value::as_u64)
                != Some(pull.number.get())
            || node
                .pointer("/pullRequest/repository/id")
                .and_then(serde_json::Value::as_str)
                != Some(&self.repository.node_id)
            || node
                .pointer("/pullRequest/headRefOid")
                .and_then(serde_json::Value::as_str)
                != Some(&pull.head)
            || node
                .pointer("/pullRequest/baseRefOid")
                .and_then(serde_json::Value::as_str)
                != Some(&pull.base)
        {
            return Err(AccessError::received(AccessKind::IdentityMismatch, 200));
        }
        let root = node
            .pointer("/comments/nodes/0/fullDatabaseId")
            .and_then(|id| {
                id.as_u64()
                    .and_then(NonZeroU64::new)
                    .or_else(|| id.as_str()?.parse().ok())
            });
        Ok(ObservedThread {
            repository: self.repository(),
            pull: pull.id,
            head: pull.head.clone(),
            id: id.to_owned(),
            root,
            outdated: node["isOutdated"].as_bool(),
            resolved: node["isResolved"].as_bool(),
            can_resolve: node["viewerCanResolve"].as_bool(),
            can_reopen: node["viewerCanUnresolve"].as_bool(),
        })
    }
}

pub(super) fn set_resolved(
    thread: &ObservedThread,
    resolved: bool,
    correlation: &str,
) -> Result<Request, AccessError> {
    if !valid_node(correlation) {
        return Err(AccessError::local(AccessKind::WrongScope));
    }
    let (mutation, input) = if resolved {
        ("resolveReviewThread", "ResolveReviewThreadInput")
    } else {
        ("unresolveReviewThread", "UnresolveReviewThreadInput")
    };
    let query = format!(
        "mutation ThreadlineSetReviewThread($input:{input}!) {{ change:{mutation}(input:$input) {{ clientMutationId thread {{ {THREAD_FIELDS} }} }} }}"
    );
    Request::graph(&serde_json::json!({
        "query":query,"variables":{"input":{"threadId":thread.id,"clientMutationId":correlation}}
    }))
}

pub(super) fn graph_data<'a>(
    response: &'a serde_json::Value,
    path: &str,
) -> Result<&'a serde_json::Value, AccessError> {
    if response
        .get("errors")
        .is_some_and(|errors| errors.as_array().is_none_or(|errors| !errors.is_empty()))
    {
        return Err(AccessError::received(AccessKind::MalformedResponse, 200));
    }
    response
        .pointer(path)
        .filter(|node| node.is_object())
        .ok_or_else(|| AccessError::received(AccessKind::MissingOrInaccessible, 200))
}

fn valid_node(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'=' | b':'))
}
