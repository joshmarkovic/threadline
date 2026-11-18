use super::*;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Reachability {
    NotAttempted,
    NoResponse,
    ReceivedResponse,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AccessKind {
    Cancelled,
    InvalidCredential,
    WrongScope,
    IdentityMismatch,
    PrivateRepositoryRequired,
    Unauthorized,
    SsoRequired,
    ApprovalPending,
    PermissionDenied,
    MissingOrInaccessible,
    RateLimited,
    RedirectRejected,
    InvalidPagination,
    ServiceFailure,
    UnexpectedStatus,
    Timeout,
    Transport,
    MalformedResponse,
    ResponseTooLarge,
}
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub struct RateLimit {
    pub retry_after_seconds: Option<u64>,
    pub reset_at_unix_seconds: Option<u64>,
    pub remaining: Option<u64>,
}
/// A user-initiated external browser action. Its short-lived query must not enter routine logs.
#[derive(Clone, Eq, PartialEq)]
pub struct SsoAction(Box<Url>);
impl SsoAction {
    pub fn as_url(&self) -> &Url {
        &self.0
    }
}
impl fmt::Debug for SsoAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SsoAction([redacted])")
    }
}
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AccessError {
    pub kind: AccessKind,
    pub reachability: Reachability,
    pub http_status: Option<u16>,
    pub sso_action: Option<SsoAction>,
    pub rate: Option<RateLimit>,
}
impl AccessError {
    /// A user may open these in the system browser. API credentials are never attached.
    pub fn authorization_url(&self) -> Option<Url> {
        if let Some(action) = &self.sso_action {
            return Some(action.as_url().clone());
        }
        matches!(
            self.kind,
            AccessKind::ApprovalPending | AccessKind::Unauthorized | AccessKind::InvalidCredential
        )
        .then(|| {
            Url::parse("https://github.com/settings/personal-access-tokens")
                .expect("fixed GitHub settings URL")
        })
    }
    pub(super) fn local(kind: AccessKind) -> Self {
        Self {
            kind,
            reachability: Reachability::NotAttempted,
            http_status: None,
            sso_action: None,
            rate: None,
        }
    }
    pub(super) fn received(kind: AccessKind, status: u16) -> Self {
        Self {
            reachability: Reachability::ReceivedResponse,
            http_status: Some(status),
            ..Self::local(kind)
        }
    }
    fn transport(err: reqwest::Error) -> Self {
        let kind = if err.is_timeout() {
            AccessKind::Timeout
        } else {
            AccessKind::Transport
        };
        Self {
            reachability: Reachability::NoResponse,
            ..Self::local(kind)
        }
    }
}
impl fmt::Display for AccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.kind {
            AccessKind::Cancelled => "GitHub read cancelled",
            AccessKind::InvalidCredential => "Invalid credential or simulator credential",
            AccessKind::WrongScope => "Request is outside the credential's owner or origin",
            AccessKind::IdentityMismatch => {
                "GitHub identity does not match the selected account or repository"
            }
            AccessKind::PrivateRepositoryRequired => "The designated repository must be private",
            AccessKind::Unauthorized => "GitHub rejected this credential",
            AccessKind::SsoRequired => "GitHub requires single sign-on authorization",
            AccessKind::ApprovalPending => "Organization approval for this token is pending",
            AccessKind::PermissionDenied => "The credential lacks access to this endpoint",
            AccessKind::MissingOrInaccessible => "GitHub resource is missing or inaccessible",
            AccessKind::RateLimited => "GitHub rate limit reached",
            AccessKind::RedirectRejected => "GitHub returned an unapproved redirect",
            AccessKind::InvalidPagination => "GitHub returned an invalid pagination link",
            AccessKind::ServiceFailure => "GitHub service failed",
            AccessKind::UnexpectedStatus => "Unexpected GitHub HTTP status",
            AccessKind::Timeout => "GitHub request timed out",
            AccessKind::Transport => "Could not complete the GitHub connection",
            AccessKind::MalformedResponse => "GitHub response has an invalid shape",
            AccessKind::ResponseTooLarge => "GitHub response exceeds its size limit",
        })
    }
}
impl std::error::Error for AccessError {}

pub(super) enum RequestMode<'a> {
    Read,
    Conditional(&'a HeaderValue),
    Graph(&'a serde_json::Value),
}

pub(super) struct ReadResponse<T> {
    pub body: Option<T>,
    pub headers: HeaderMap,
}
impl<T> ReadResponse<T> {
    pub fn require_body(self) -> Result<(T, HeaderMap), AccessError> {
        self.body
            .map(|body| (body, self.headers))
            .ok_or_else(|| AccessError::received(AccessKind::UnexpectedStatus, 304))
    }
}

pub(super) async fn request<T: DeserializeOwned>(
    client: &GitHubClient,
    token: &SecretToken,
    scope: &OwnerScope,
    path: &str,
    mode: RequestMode<'_>,
    cancel: &CancellationToken,
) -> Result<ReadResponse<T>, AccessError> {
    if cancel.is_cancelled() {
        return Err(AccessError::local(AccessKind::Cancelled));
    }
    let url = client
        .base
        .join(path)
        .map_err(|_| AccessError::local(AccessKind::WrongScope))?;
    if url.origin() != client.base.origin()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(AccessError::local(AccessKind::WrongScope));
    }
    let conditional = matches!(mode, RequestMode::Conditional(_));
    let builder = match mode {
        RequestMode::Read => client
            .http
            .get(url)
            .header("X-GitHub-Api-Version", GITHUB_REST_VERSION),
        RequestMode::Conditional(tag) => client
            .http
            .get(url)
            .header("X-GitHub-Api-Version", GITHUB_REST_VERSION)
            .header(header::IF_NONE_MATCH, tag),
        RequestMode::Graph(body) => client
            .http
            .post(url)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.to_string()),
    }
    .header(header::ACCEPT, "application/vnd.github+json")
    .header(header::AUTHORIZATION, token.0.clone());
    let mut response = tokio::select! {biased;
        _=cancel.cancelled()=>return Err(AccessError {reachability:Reachability::NoResponse,..AccessError::local(AccessKind::Cancelled)}),
        result=builder.send()=>result.map_err(AccessError::transport)?,
    };
    let status = response.status();
    let headers = response.headers().clone();
    let fail = |kind| AccessError::received(kind, status.as_u16());
    if status == StatusCode::NOT_MODIFIED {
        return if conditional {
            Ok(ReadResponse {
                body: None,
                headers,
            })
        } else {
            Err(fail(AccessKind::UnexpectedStatus))
        };
    }
    if status.is_redirection() {
        return Err(fail(AccessKind::RedirectRejected));
    }
    if status != StatusCode::OK
        && status != StatusCode::FORBIDDEN
        && status != StatusCode::TOO_MANY_REQUESTS
        && status != StatusCode::NOT_FOUND
    {
        return Err(map_status(status, &headers, None, scope, token));
    }
    let limit = if status == StatusCode::OK {
        2_097_152
    } else {
        16_384
    };
    let mut bytes = Vec::new();
    if response.content_length().is_some_and(|n| n > limit as u64) {
        return Err(if status == StatusCode::OK {
            fail(AccessKind::ResponseTooLarge)
        } else {
            map_status(status, &headers, None, scope, token)
        });
    }
    loop {
        let chunk = tokio::select! {biased;
            _=cancel.cancelled()=>return Err(fail(AccessKind::Cancelled)),
            result=response.chunk()=>result.map_err(|e|fail(if e.is_timeout(){AccessKind::Timeout}else{AccessKind::Transport}))?,
        };
        let Some(chunk) = chunk else { break };
        if chunk.len() > limit - bytes.len() {
            return Err(if status == StatusCode::OK {
                fail(AccessKind::ResponseTooLarge)
            } else {
                map_status(status, &headers, None, scope, token)
            });
        }
        bytes.extend_from_slice(&chunk);
    }
    if status != StatusCode::OK {
        let message = serde_json::from_slice::<ErrorMessage>(&bytes).ok();
        return Err(map_status(
            status,
            &headers,
            message.as_ref().map(|m| m.message.as_str()),
            scope,
            token,
        ));
    }
    let body = serde_json::from_slice(&bytes).map_err(|_| fail(AccessKind::MalformedResponse))?;
    Ok(ReadResponse {
        body: Some(body),
        headers,
    })
}

#[derive(Deserialize)]
struct ErrorMessage {
    message: String,
}

fn map_status(
    status: StatusCode,
    headers: &HeaderMap,
    message: Option<&str>,
    scope: &OwnerScope,
    token: &SecretToken,
) -> AccessError {
    let num = |name: &str| headers.get(name)?.to_str().ok()?.parse::<u64>().ok();
    let rate = RateLimit {
        retry_after_seconds: num("retry-after"),
        remaining: num("x-ratelimit-remaining"),
        reset_at_unix_seconds: num("x-ratelimit-reset"),
    };
    let message = message.unwrap_or("").to_ascii_lowercase();
    let sso = header_sso(headers);
    let kind = match status {
        StatusCode::UNAUTHORIZED => AccessKind::Unauthorized,
        StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
            if sso.is_some_and(|v| {
                v.split(';')
                    .next()
                    .is_some_and(|k| k.trim().eq_ignore_ascii_case("required"))
            }) =>
        {
            AccessKind::SsoRequired
        }
        StatusCode::TOO_MANY_REQUESTS => AccessKind::RateLimited,
        StatusCode::FORBIDDEN
            if rate.retry_after_seconds.is_some()
                || rate.remaining == Some(0)
                || message.contains("secondary rate limit") =>
        {
            AccessKind::RateLimited
        }
        StatusCode::FORBIDDEN
            if message.contains("pending approval")
                || message.contains("pending organization approval")
                || message.contains("pending organisation approval") =>
        {
            AccessKind::ApprovalPending
        }
        StatusCode::FORBIDDEN => AccessKind::PermissionDenied,
        StatusCode::NOT_FOUND => AccessKind::MissingOrInaccessible,
        s if s.is_server_error() => AccessKind::ServiceFailure,
        _ => AccessKind::UnexpectedStatus,
    };
    let mut error = AccessError::received(kind, status.as_u16());
    if kind == AccessKind::RateLimited {
        error.rate = Some(rate);
    }
    if kind == AccessKind::SsoRequired {
        error.sso_action = sso.and_then(|v| {
            v.split(';').find_map(|attr| {
                let (k, url) = attr.trim().split_once('=')?;
                k.eq_ignore_ascii_case("url")
                    .then(|| web_action(url, scope, token))
                    .flatten()
            })
        });
    }
    error
}
pub(super) fn header_sso(headers: &HeaderMap) -> Option<&str> {
    headers.get("x-github-sso")?.to_str().ok()
}
pub(super) fn partial_sso(headers: &HeaderMap) -> Option<BTreeSet<NonZeroU64>> {
    let value = headers.get("x-github-sso")?;
    let Ok(value) = value.to_str() else {
        return Some(BTreeSet::new());
    };
    let mut parts = value.split(';');
    // An unfamiliar advisory still prevents a claim of complete coverage.
    if !parts
        .next()
        .unwrap_or("")
        .trim()
        .eq_ignore_ascii_case("partial-results")
    {
        return Some(BTreeSet::new());
    }
    let ids = parts
        .filter_map(|p| p.trim().split_once('='))
        .filter(|(k, _)| k.eq_ignore_ascii_case("organizations"))
        .flat_map(|(_, v)| v.split(','))
        .filter_map(|id| id.trim().parse::<NonZeroU64>().ok())
        .collect();
    Some(ids)
}
pub(super) fn web_action(raw: &str, owner: &OwnerScope, token: &SecretToken) -> Option<SsoAction> {
    let url = Url::parse(raw).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let parts: Vec<_> = url.path_segments()?.collect();
    if parts.len() != 3 || parts[0] != "orgs" || !owner.accepts(parts[1]) || parts[2] != "sso" {
        return None;
    }
    let bearer = token.0.to_str().ok()?.strip_prefix("Bearer ")?;
    let pairs: Vec<_> = url.query_pairs().collect();
    if let [(key, value)] = pairs.as_slice() {
        if key != "authorization_request"
            || value.is_empty()
            || value.len() > 2048
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'='))
            || value.contains(bearer)
        {
            return None;
        }
    } else if !pairs.is_empty() {
        return None;
    }
    Some(SsoAction(Box::new(url)))
}
