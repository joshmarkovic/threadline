use serde::Serialize;

use super::*;

const BODY_LIMIT: usize = 2_097_152;
const ERROR_BODY_LIMIT: usize = 16_384;

pub(super) struct Request {
    method: Method,
    path: String,
    graph: bool,
    pub(super) status: StatusCode,
    body: Vec<u8>,
}

impl Request {
    pub(super) fn new(
        method: Method,
        path: String,
        status: StatusCode,
        body: &impl Serialize,
    ) -> Result<Self, AccessError> {
        let bytes = serde_json::to_vec(body)
            .map_err(|_| AccessError::local(AccessKind::MalformedResponse))?;
        if bytes.len() > BODY_LIMIT {
            return Err(AccessError::local(AccessKind::ResponseTooLarge));
        }
        Ok(Self {
            method,
            path,
            graph: false,
            status,
            body: bytes,
        })
    }

    pub(super) fn graph(body: &serde_json::Value) -> Result<Self, AccessError> {
        let mut request = Self::new(Method::POST, "graphql".to_owned(), StatusCode::OK, body)?;
        request.graph = true;
        Ok(request)
    }
}

pub(super) fn ambiguous(mut access: AccessError, headers: &HeaderMap) -> MutationFailure {
    let rate = RateLimit::from_headers(headers);
    if rate != RateLimit::default() {
        access.rate = Some(rate);
    }
    MutationFailure {
        evidence: DispatchEvidence::Ambiguous,
        access,
    }
}

impl GitHubWriter<'_> {
    pub(super) async fn send(
        &self,
        request: Request,
        cancel: &CancellationToken,
    ) -> Result<(Vec<u8>, HeaderMap), MutationFailure> {
        let unsent = |access| MutationFailure {
            evidence: DispatchEvidence::DefinitelyUnsent,
            access,
        };
        let cancelled = || AccessError::local(AccessKind::Cancelled);
        if cancel.is_cancelled() {
            return Err(unsent(cancelled()));
        }
        let session = self.session;
        let path = if request.graph {
            request.path
        } else {
            self.path(&request.path)
        };
        let url = self.url(&path).map_err(unsent)?;
        let mut builder = session
            .client
            .http
            .request(request.method, url)
            .header(header::ACCEPT, "application/vnd.github+json")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, session.token.0.clone())
            .body(request.body);
        if !request.graph {
            builder = builder.header("X-GitHub-Api-Version", GITHUB_REST_VERSION);
        }
        let mut response = tokio::select! { biased;
            _=cancel.cancelled()=>return Err(ambiguous(AccessError {
                reachability:Reachability::NoResponse,..cancelled()
            }, &HeaderMap::new())),
            result=builder.send()=>result.map_err(|error| {
                if error.is_connect() {
                    unsent(AccessError::transport(error))
                } else {
                    ambiguous(AccessError::transport(error), &HeaderMap::new())
                }
            })?,
        };
        let status = response.status();
        let headers = response.headers().clone();
        let rejection = |message| {
            let access = map_status(status, &headers, message, &session.scope, &session.token);
            let evidence = match access.kind {
                AccessKind::RateLimited => DispatchEvidence::Throttled,
                AccessKind::Unauthorized
                | AccessKind::PermissionDenied
                | AccessKind::ApprovalPending
                | AccessKind::SsoRequired
                | AccessKind::MissingOrInaccessible => DispatchEvidence::AccessRejected,
                _ if matches!(
                    status,
                    StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
                ) =>
                {
                    DispatchEvidence::InvalidInput
                }
                _ => DispatchEvidence::Ambiguous,
            };
            MutationFailure { evidence, access }
        };
        if status.is_redirection() {
            return Err(ambiguous(
                AccessError::received(AccessKind::RedirectRejected, status.as_u16()),
                &headers,
            ));
        }
        let accepted = status == request.status;
        let maximum = if accepted {
            BODY_LIMIT
        } else {
            ERROR_BODY_LIMIT
        };
        let failed_body = |kind| {
            if accepted {
                ambiguous(AccessError::received(kind, status.as_u16()), &headers)
            } else {
                rejection(None)
            }
        };
        if response
            .content_length()
            .is_some_and(|length| length > maximum as u64)
        {
            return Err(failed_body(AccessKind::ResponseTooLarge));
        }
        let mut bytes = Vec::new();
        loop {
            let next = tokio::select! { biased;
                _=cancel.cancelled()=>return Err(failed_body(AccessKind::Cancelled)),
                chunk=response.chunk()=>chunk.map_err(|error|failed_body(if error.is_timeout() {
                    AccessKind::Timeout
                } else {
                    AccessKind::Transport
                }))?,
            };
            let Some(chunk) = next else { break };
            if chunk.len() > maximum - bytes.len() {
                return Err(failed_body(AccessKind::ResponseTooLarge));
            }
            bytes.extend_from_slice(&chunk);
        }
        if !accepted {
            let payload = serde_json::from_slice::<serde_json::Value>(&bytes).ok();
            let message = payload
                .as_ref()
                .and_then(|payload| payload.get("message"))
                .and_then(serde_json::Value::as_str);
            return Err(rejection(message));
        }
        Ok((bytes, headers))
    }
}

pub(super) fn request_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-github-request-id")
        .and_then(|header| header.to_str().ok())
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 160
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_'))
        })
        .map(str::to_owned)
}
