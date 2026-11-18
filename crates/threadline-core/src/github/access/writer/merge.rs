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
    needs_optional_checks: bool,
}

impl VerifiedMergePolicy {
    pub fn eligibility(&self) -> &MergeEligibility {
        &self.value
    }
    pub fn needs_optional_checks(&self) -> bool {
        self.needs_optional_checks
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

const MERGED_PULL: &str = "query ThreadlineMergedPull($id:ID!) { node(id:$id) { ... on PullRequest { id number baseRefName baseRefOid headRefOid repository { id } state merged mergeCommit { oid } mergedBy { __typename ... on User { databaseId } } } } }";

pub struct ObservedMergedPull {
    pub oid: String,
    pub actor: Option<NonZeroU64>,
}

const OPTIONAL_CHECKS: &str = "query ThreadlineOptionalMergeChecks($id:ID!) { node(id:$id) { ... on PullRequest { id number baseRefName baseRefOid headRefOid repository { id } isMergeQueueEnabled mergeStateStatus reviewDecision state isDraft merged mergeable baseRef { name target { oid } branchProtectionRule { id } } commits(last:1) { nodes { commit { oid statusCheckRollup { contexts(first:100) { totalCount pageInfo { hasNextPage } nodes { __typename ... on CheckRun { isRequired(pullRequestId:$id) } ... on StatusContext { isRequired(pullRequestId:$id) } } } } } } } } } }";

pub struct OptionalChecks {
    actor: NonZeroU64,
    repository: RepositoryApiId,
    pull: NonZeroU64,
    base: String,
    head: String,
    contexts: usize,
}

impl OptionalChecks {
    pub fn contexts(&self) -> usize {
        self.contexts
    }
}

fn all_check_contexts_optional(node: &serde_json::Value, expected_head: &str) -> Option<usize> {
    let Some([entry]) = node
        .pointer("/commits/nodes")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
    else {
        return None;
    };
    let commit = &entry["commit"];
    let contexts = &commit["statusCheckRollup"]["contexts"];
    let rows = contexts["nodes"].as_array()?;
    (commit["oid"].as_str() == Some(expected_head)
        && contexts["pageInfo"]["hasNextPage"] == false
        && (1..=100).contains(&rows.len())
        && contexts["totalCount"].as_u64() == Some(rows.len() as u64)
        && rows.iter().all(|row| {
            matches!(
                row["__typename"].as_str(),
                Some("CheckRun" | "StatusContext")
            ) && row["isRequired"] == false
        }))
    .then_some(rows.len())
}

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
        let (node, rate) = self.merge_graph(pull, POLICY, cancel).await?;
        let value = decode_policy(&node);
        Ok((
            VerifiedMergePolicy {
                actor: self.actor(),
                repository: self.repository(),
                pull: pull.id(),
                base: pull.base().to_owned(),
                head: pull.head().to_owned(),
                needs_optional_checks: node["mergeStateStatus"] == "UNSTABLE"
                    && node
                        .get("reviewDecision")
                        .is_some_and(serde_json::Value::is_null)
                    && value.queue == QueueRequirement::NotRequired,
                value,
            },
            rate,
        ))
    }

    async fn merge_graph(
        &self,
        pull: &VerifiedPull,
        query: &str,
        cancel: &CancellationToken,
    ) -> Result<(serde_json::Value, RateLimit), AccessError> {
        self.check_pull(pull)?;
        let body = serde_json::json!({"query":query,"variables":{"id":pull.node_id()}});
        let session = self.session;
        let (mut response, headers): (serde_json::Value, _) = request(
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
            .pointer_mut("/data/node")
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
        Ok((node.take(), RateLimit::from_headers(&headers)))
    }

    pub async fn observe_optional_merge_checks(
        &self,
        pull: &VerifiedPull,
        policy: &VerifiedMergePolicy,
        cancel: &CancellationToken,
    ) -> Result<(Option<OptionalChecks>, RateLimit), AccessError> {
        if !policy.needs_optional_checks || !self.matches_policy(pull, policy) {
            return Err(AccessError::local(AccessKind::WrongScope));
        }
        let (node, rate) = self.merge_graph(pull, OPTIONAL_CHECKS, cancel).await?;
        let unprotected = node["isMergeQueueEnabled"] == false
            && node["mergeStateStatus"] == "UNSTABLE"
            && node
                .get("reviewDecision")
                .is_some_and(serde_json::Value::is_null)
            && node["state"] == "OPEN"
            && node["isDraft"] == false
            && node["merged"] == false
            && node["mergeable"] == "MERGEABLE"
            && node["baseRef"]["name"].as_str() == pull.base_ref()
            && node["baseRef"]["target"]["oid"].as_str() == Some(pull.base())
            && node["baseRef"]
                .get("branchProtectionRule")
                .is_some_and(serde_json::Value::is_null);
        let optional = all_check_contexts_optional(&node, pull.head()).filter(|_| unprotected);
        Ok((
            optional.map(|contexts| OptionalChecks {
                actor: self.actor(),
                repository: self.repository(),
                pull: pull.id(),
                base: pull.base().into(),
                head: pull.head().into(),
                contexts,
            }),
            rate,
        ))
    }

    pub async fn confirm_optional_merge_checks(
        &self,
        pull: &VerifiedPull,
        policy: &mut VerifiedMergePolicy,
        proof: OptionalChecks,
        cancel: &CancellationToken,
    ) -> Result<RateLimit, AccessError> {
        if !policy.needs_optional_checks
            || !self.matches_policy(pull, policy)
            || (
                proof.actor,
                proof.repository,
                proof.pull,
                proof.base.as_str(),
                proof.head.as_str(),
            ) != (
                self.actor(),
                self.repository(),
                pull.id(),
                pull.base(),
                pull.head(),
            )
        {
            return Err(AccessError::local(AccessKind::WrongScope));
        }
        let name = pull
            .base_ref()
            .ok_or_else(|| AccessError::local(AccessKind::WrongScope))?;
        let mut url = Url::parse("https://api.github.com/branches").expect("GitHub branch URL");
        url.path_segments_mut()
            .expect("hierarchical branch URL")
            .push(name);
        let (branch, headers): (serde_json::Value, _) = self.get(&url.path()[1..], cancel).await?;
        if branch["name"].as_str() != Some(name)
            || branch["commit"]["sha"].as_str() != Some(pull.base())
        {
            return Err(AccessError::received(AccessKind::IdentityMismatch, 200));
        }
        let required = &branch["protection"]["required_status_checks"];
        if branch["protected"] == false
            && required["enforcement_level"] == "off"
            && ["contexts", "checks"]
                .into_iter()
                .all(|field| required[field].as_array().is_some_and(Vec::is_empty))
        {
            policy.needs_optional_checks = false;
            policy.value.checks = MergeGate::Satisfied;
            policy.value.reviews = MergeGate::Satisfied;
            policy.value.branch_rules = MergeGate::Satisfied;
        }
        Ok(RateLimit::from_headers(&headers))
    }

    /// Read the authoritative merge commit for this exact PR and head.
    pub async fn observe_merged_pull(
        &self,
        pull: &VerifiedPull,
        cancel: &CancellationToken,
    ) -> Result<(Option<ObservedMergedPull>, RateLimit), AccessError> {
        let (node, rate) = self.merge_graph(pull, MERGED_PULL, cancel).await?;
        if node["merged"] != true || node["state"] != "MERGED" {
            return Ok((None, rate));
        }
        let Some(raw) = node["mergeCommit"]["oid"].as_str() else {
            return Ok((None, rate));
        };
        let oid = raw.to_ascii_lowercase();
        if raw.len() != 40 || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(AccessError::received(AccessKind::MalformedResponse, 200));
        }
        let actor = &node["mergedBy"];
        let actor = (actor["__typename"] == "User")
            .then(|| actor["databaseId"].as_u64().and_then(NonZeroU64::new))
            .flatten();
        if pull.merge_oid().is_some_and(|rest| rest != oid)
            || pull
                .merged_by()
                .zip(actor)
                .is_some_and(|(rest, graph)| rest != graph)
        {
            return Err(AccessError::received(AccessKind::IdentityMismatch, 200));
        }
        Ok((Some(ObservedMergedPull { oid, actor }), rate))
    }

    fn matches_policy(&self, pull: &VerifiedPull, policy: &VerifiedMergePolicy) -> bool {
        policy.actor == self.actor()
            && policy.repository == self.repository()
            && policy.pull == pull.id()
            && policy.base == pull.base()
            && policy.head == pull.head()
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
        if expected_head != pull.head() || !self.matches_policy(pull, policy) {
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
