//! Ordinary direct pull request merges pinned to a verified head.

use super::{
    review::VerifiedPull,
    transport::{Request, ambiguous, request_id},
    *,
};
use serde::Serialize;

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum MergeGate {
    Satisfied,
    Blocking,
    Unknown,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum QueueRequirement {
    Required,
    NotRequired,
    Unknown,
}

pub struct MergeEligibility {
    pub methods: Option<BTreeSet<MergeMethod>>,
    pub permission: WritePermission,
    pub open: Option<bool>,
    pub draft: Option<bool>,
    pub merged: Option<bool>,
    pub mergeable: Option<bool>,
    pub queue: QueueRequirement,
    pub checks: MergeGate,
    pub reviews: MergeGate,
    pub branch_rules: MergeGate,
}

impl MergeEligibility {
    pub fn allows(&self, method: MergeMethod) -> bool {
        self.methods
            .as_ref()
            .is_some_and(|methods| methods.contains(&method))
            && self.permission == WritePermission::Allowed
            && self.open == Some(true)
            && self.draft == Some(false)
            && self.merged == Some(false)
            && self.mergeable == Some(true)
            && self.queue == QueueRequirement::NotRequired
            && [self.checks, self.reviews, self.branch_rules]
                .into_iter()
                .all(|gate| gate == MergeGate::Satisfied)
    }
}

pub struct VerifiedMergePolicy {
    actor: NonZeroU64,
    repository: RepositoryApiId,
    pull: NonZeroU64,
    base: String,
    head: String,
    value: MergeEligibility,
}

impl VerifiedMergePolicy {
    pub fn eligibility(&self) -> &MergeEligibility {
        &self.value
    }
}

pub struct DirectMergeReceipt {
    pub merge_oid: String,
    pub request_id: String,
    pub rate: RateLimit,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DirectMergeRejection {
    HeadChanged,
    NotPermitted,
    Other,
}

#[derive(Debug)]
pub struct DirectMergeFailure {
    pub reason: DirectMergeRejection,
    pub transport: MutationFailure,
}

const POLICY: &str = "query ThreadlineMergePolicy($id:ID!) { node(id:$id) { ... on PullRequest { id number baseRefName baseRefOid headRefOid state isDraft merged mergeable mergeStateStatus reviewDecision mergeQueue { id } repository { id viewerPermission mergeCommitAllowed squashMergeAllowed rebaseMergeAllowed } } } }";

#[derive(Deserialize)]
struct MergeResponse {
    merged: bool,
    sha: Option<String>,
}

impl GitHubWriter<'_> {
    pub async fn observe_merge_policy(
        &self,
        pull: &VerifiedPull,
        cancel: &CancellationToken,
    ) -> Result<(VerifiedMergePolicy, RateLimit), AccessError> {
        self.check_pull(pull)?;
        let body = serde_json::json!({"query":POLICY,"variables":{"id":pull.node_id()}});
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
        if response
            .get("errors")
            .is_some_and(|errors| errors.as_array().is_none_or(|rows| !rows.is_empty()))
        {
            return Err(AccessError::received(AccessKind::MalformedResponse, 200));
        }
        let node = response
            .pointer("/data/node")
            .filter(|node| node.is_object())
            .ok_or_else(|| AccessError::received(AccessKind::MissingOrInaccessible, 200))?;
        if node["id"].as_str() != Some(pull.node_id())
            || node["number"].as_u64() != Some(pull.number().get())
            || node["baseRefOid"].as_str() != Some(pull.base())
            || node["headRefOid"].as_str() != Some(pull.head())
            || node["baseRefName"].as_str() != pull.base_ref()
            || node["repository"]["id"].as_str() != Some(&self.repository.node_id)
        {
            return Err(AccessError::received(AccessKind::IdentityMismatch, 200));
        }
        Ok((
            VerifiedMergePolicy {
                actor: self.actor(),
                repository: self.repository(),
                pull: pull.id(),
                base: pull.base().to_owned(),
                head: pull.head().to_owned(),
                value: decode_policy(node),
            },
            RateLimit::from_headers(&headers),
        ))
    }

    /// Use only the documented synchronous merge endpoint. GitHub must compare the supplied head.
    pub async fn merge_at_head(
        &self,
        pull: &VerifiedPull,
        policy: &VerifiedMergePolicy,
        method: MergeMethod,
        expected_head: &str,
        cancel: &CancellationToken,
    ) -> Result<DirectMergeReceipt, DirectMergeFailure> {
        let unsent = |reason, kind| DirectMergeFailure {
            reason,
            transport: MutationFailure {
                evidence: DispatchEvidence::DefinitelyUnsent,
                access: AccessError::local(kind),
            },
        };
        self.check_pull(pull)
            .map_err(|_| unsent(DirectMergeRejection::Other, AccessKind::WrongScope))?;
        if expected_head != pull.head()
            || policy.actor != self.actor()
            || policy.repository != self.repository()
            || policy.pull != pull.id()
            || policy.base != pull.base()
            || policy.head != expected_head
        {
            return Err(unsent(
                DirectMergeRejection::HeadChanged,
                AccessKind::IdentityMismatch,
            ));
        }
        if !policy.eligibility().allows(method) || !pull.is_open() {
            return Err(unsent(
                DirectMergeRejection::NotPermitted,
                AccessKind::PermissionDenied,
            ));
        }
        let payload = serde_json::json!({"sha":expected_head,"merge_method":method});
        let request = Request::new(
            Method::PUT,
            format!("pulls/{}/merge", pull.number().get()),
            StatusCode::OK,
            &payload,
        )
        .map_err(|error| unsent(DirectMergeRejection::Other, error.kind))?;
        let (raw, headers) = self.send(request, cancel).await.map_err(|transport| {
            let reason = match transport.access.http_status {
                Some(409) => DirectMergeRejection::HeadChanged,
                Some(405) => DirectMergeRejection::NotPermitted,
                _ => DirectMergeRejection::Other,
            };
            DirectMergeFailure { reason, transport }
        })?;
        let unconfirmed = || DirectMergeFailure {
            reason: DirectMergeRejection::Other,
            transport: ambiguous(
                AccessError::received(AccessKind::MalformedResponse, 200),
                &headers,
            ),
        };
        let result: MergeResponse = serde_json::from_slice(&raw).map_err(|_| unconfirmed())?;
        let merge_oid = result
            .sha
            .filter(|sha| sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .filter(|_| result.merged)
            .ok_or_else(unconfirmed)?
            .to_ascii_lowercase();
        Ok(DirectMergeReceipt {
            merge_oid,
            request_id: request_id(&headers).ok_or_else(unconfirmed)?,
            rate: RateLimit::from_headers(&headers),
        })
    }
}

fn decode_policy(node: &serde_json::Value) -> MergeEligibility {
    let repository = &node["repository"];
    let method_flags = [
        ("mergeCommitAllowed", MergeMethod::Merge),
        ("squashMergeAllowed", MergeMethod::Squash),
        ("rebaseMergeAllowed", MergeMethod::Rebase),
    ];
    let methods = method_flags
        .into_iter()
        .map(|(name, method)| repository[name].as_bool().map(|allowed| (allowed, method)))
        .collect::<Option<Vec<_>>>()
        .map(|flags| {
            flags
                .into_iter()
                .filter_map(|(allowed, method)| allowed.then_some(method))
                .collect()
        });
    let permission = match repository["viewerPermission"].as_str() {
        Some("ADMIN" | "MAINTAIN" | "WRITE") => WritePermission::Allowed,
        Some("READ" | "TRIAGE") => WritePermission::Denied,
        _ => WritePermission::Unknown,
    };
    let queue = match node.get("mergeQueue") {
        Some(serde_json::Value::Null) => QueueRequirement::NotRequired,
        Some(queue) if queue["id"].as_str().is_some_and(|id| !id.is_empty()) => {
            QueueRequirement::Required
        }
        _ => QueueRequirement::Unknown,
    };
    let clean = match node["mergeStateStatus"].as_str() {
        Some("CLEAN") => MergeGate::Satisfied,
        Some("BEHIND" | "BLOCKED" | "DIRTY" | "DRAFT" | "UNSTABLE") => MergeGate::Blocking,
        _ => MergeGate::Unknown,
    };
    let reviews = match node.get("reviewDecision") {
        Some(value) if value == "APPROVED" => MergeGate::Satisfied,
        Some(value) if value == "REVIEW_REQUIRED" || value == "CHANGES_REQUESTED" => {
            MergeGate::Blocking
        }
        Some(serde_json::Value::Null) if clean == MergeGate::Satisfied => MergeGate::Satisfied,
        _ => MergeGate::Unknown,
    };
    let open = match node["state"].as_str() {
        Some("OPEN") => Some(true),
        Some("CLOSED" | "MERGED") => Some(false),
        _ => None,
    };
    let mergeable = match node["mergeable"].as_str() {
        Some("MERGEABLE") => Some(true),
        Some("CONFLICTING") => Some(false),
        _ => None,
    };
    MergeEligibility {
        methods,
        permission,
        open,
        draft: node["isDraft"].as_bool(),
        merged: node["merged"].as_bool(),
        mergeable,
        queue,
        checks: clean,
        reviews,
        branch_rules: clean,
    }
}
