use std::net::SocketAddr;
use std::num::NonZeroU64;
use std::time::Duration;

use reqwest::header::{self, HeaderMap};
use reqwest::{Client, Response, StatusCode, Url, redirect};
use serde::{Deserialize, Deserializer};

use crate::domain::{
    IssueApiId, ItemKey, ItemKind, ItemNumber, RepositoryApiId, RepositoryKey, Revision,
};

use super::{ItemSummary, Provider, ProviderError, ProviderFuture, ReadSummary};

pub const GITHUB_REST_VERSION: &str = "2026-03-10";
const MAX_SUMMARY_BYTES: usize = 1_048_576;

/// HTTP provider for the Sprint 01 simulator. The simulator binds each reply to its repository ID.
pub struct RestProvider {
    client: Client,
    base: Url,
}

impl RestProvider {
    /// Connect to a test-owned listening socket without using environment proxies or redirects.
    pub fn loopback(address: SocketAddr) -> Result<Self, ProviderError> {
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(ProviderError::InvalidEndpoint);
        }
        let base = Url::parse(&format!("http://{address}/"))
            .map_err(|_| ProviderError::InvalidEndpoint)?;
        let client = Client::builder()
            .user_agent(concat!("Threadline/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .redirect(redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| ProviderError::InvalidEndpoint)?;
        Ok(Self { client, base })
    }

    async fn read(&self, request: &ReadSummary) -> Result<ItemSummary, ProviderError> {
        if request.item().repository.host.as_str() != "github.com" {
            return Err(ProviderError::UnsupportedHost);
        }
        let route = request.route();
        let number = request.item().number.get();
        let path = format!("repos/{}/{}/issues/{number}", route.owner(), route.name());
        let url = self
            .base
            .join(&path)
            .map_err(|_| ProviderError::InvalidEndpoint)?;
        let response = self
            .client
            .get(url)
            .header(header::ACCEPT, "application/vnd.github+json")
            .header("x-github-api-version", GITHUB_REST_VERSION)
            .send()
            .await
            .map_err(map_transport)?;
        if response.status() != StatusCode::OK {
            return Err(map_status(response.status(), response.headers()));
        }

        let entity_tag = response
            .headers()
            .get(header::ETAG)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let payload = read_summary_response(response).await?;
        let item = ItemKey {
            repository: RepositoryKey {
                id: RepositoryApiId::new(payload.repository.id),
                ..request.item().repository.clone()
            },
            number: ItemNumber::new(payload.number),
        };
        if &item != request.item() {
            return Err(ProviderError::WrongResource);
        }
        let revision = match entity_tag {
            Some(entity_tag) => Revision::EntityTag(entity_tag),
            None if !payload.updated_at.is_empty() => Revision::UpdatedAt(payload.updated_at),
            None => return Err(ProviderError::InvalidResponse),
        };
        let kind = if payload.pull_request.is_some() {
            ItemKind::PullRequest
        } else {
            ItemKind::Issue
        };
        Ok(ItemSummary::new(
            item,
            revision,
            IssueApiId::new(payload.id),
            kind,
            payload.title,
            payload.body,
        ))
    }
}

impl Provider for RestProvider {
    fn read_summary<'a>(&'a self, request: &'a ReadSummary) -> ProviderFuture<'a> {
        Box::pin(self.read(request))
    }
}

#[derive(Deserialize)]
struct IssueResponse {
    repository: RepositoryIdentity,
    number: NonZeroU64,
    id: NonZeroU64,
    title: String,
    #[serde(deserialize_with = "deserialize_nullable")]
    body: Option<String>,
    pull_request: Option<PullRequestMarker>,
    updated_at: String,
}

#[derive(Deserialize)]
struct RepositoryIdentity {
    id: NonZeroU64,
}

#[derive(Deserialize)]
struct PullRequestMarker {}

fn deserialize_nullable<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::deserialize(deserializer)
}

async fn read_summary_response(mut response: Response) -> Result<IssueResponse, ProviderError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_SUMMARY_BYTES as u64)
    {
        return Err(ProviderError::ResponseTooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_transport)? {
        if chunk.len() > MAX_SUMMARY_BYTES - bytes.len() {
            return Err(ProviderError::ResponseTooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| ProviderError::InvalidResponse)
}

fn map_transport(error: reqwest::Error) -> ProviderError {
    if error.is_timeout() {
        ProviderError::Timeout
    } else {
        ProviderError::Transport
    }
}

fn map_status(status: StatusCode, headers: &HeaderMap) -> ProviderError {
    let retry_after = headers
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|seconds| seconds.parse().ok())
        .map(Duration::from_secs);
    let exhausted = headers
        .get("x-ratelimit-remaining")
        .is_some_and(|value| value == "0");
    match status {
        StatusCode::UNAUTHORIZED => ProviderError::Unauthorized,
        StatusCode::FORBIDDEN if retry_after.is_some() || exhausted => {
            ProviderError::RateLimited { retry_after }
        }
        StatusCode::FORBIDDEN => ProviderError::Forbidden,
        StatusCode::NOT_FOUND => ProviderError::NotFound,
        StatusCode::TOO_MANY_REQUESTS => ProviderError::RateLimited { retry_after },
        status => ProviderError::HttpStatus(status.as_u16()),
    }
}
