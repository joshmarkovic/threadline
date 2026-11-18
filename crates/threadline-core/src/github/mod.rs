mod rest;
mod scripted;

use std::error::Error;
use std::fmt::{self, Display};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::domain::{IssueApiId, ItemKey, ItemKind, RepositoryName, Revision};

pub use rest::{GITHUB_REST_VERSION, RestProvider};
pub use scripted::{ScriptedProvider, ScriptedReply};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadSummary {
    item: ItemKey,
    route: RepositoryName,
}

impl ReadSummary {
    pub fn new(item: ItemKey, route: RepositoryName) -> Self {
        Self { item, route }
    }

    pub fn item(&self) -> &ItemKey {
        &self.item
    }

    pub fn route(&self) -> &RepositoryName {
        &self.route
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ItemSummary {
    item: ItemKey,
    revision: Revision,
    pub issue_api_id: IssueApiId,
    pub kind: ItemKind,
    title: String,
    body: Option<String>,
}

impl ItemSummary {
    pub fn new(
        item: ItemKey,
        revision: Revision,
        issue_api_id: IssueApiId,
        kind: ItemKind,
        title: String,
        body: Option<String>,
    ) -> Self {
        Self {
            item,
            revision,
            issue_api_id,
            kind,
            title,
            body,
        }
    }

    pub fn item(&self) -> &ItemKey {
        &self.item
    }

    pub fn revision(&self) -> &Revision {
        &self.revision
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }
}

impl fmt::Debug for ItemSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ItemSummary")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderError {
    Cancelled,
    SchedulerBusy,
    NotScripted,
    UnsupportedHost,
    InvalidEndpoint,
    Unauthorized,
    Forbidden,
    NotFound,
    RateLimited { retry_after: Option<Duration> },
    HttpStatus(u16),
    Timeout,
    Transport,
    InvalidResponse,
    ResponseTooLarge,
    WrongResource,
}

impl Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::HttpStatus(status) => return write!(formatter, "GitHub HTTP status {status}"),
            Self::RateLimited {
                retry_after: Some(delay),
            } => {
                return write!(
                    formatter,
                    "GitHub rate limit reached. Retry after {} seconds",
                    delay.as_secs()
                );
            }
            Self::Cancelled => "Request cancelled",
            Self::SchedulerBusy => "The request queue is full",
            Self::NotScripted => "No fixture reply configured for this resource",
            Self::UnsupportedHost => "This provider only serves GitHub.com",
            Self::InvalidEndpoint => "Invalid API endpoint configuration",
            Self::Unauthorized => "GitHub authentication required",
            Self::Forbidden => "GitHub access denied",
            Self::NotFound => "GitHub resource is missing or inaccessible",
            Self::RateLimited { retry_after: None } => "GitHub rate limit reached",
            Self::Timeout => "GitHub request timed out",
            Self::Transport => "GitHub connection failed",
            Self::InvalidResponse => "GitHub returned an invalid response",
            Self::ResponseTooLarge => "GitHub response exceeds the item preview limit",
            Self::WrongResource => "The response belongs to a different resource",
        };
        formatter.write_str(message)
    }
}

impl Error for ProviderError {}

pub type ProviderFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ItemSummary, ProviderError>> + Send + 'a>>;

/// Provider boundary for the first read operation. Authenticated reads and writes have separate gates.
pub trait Provider: Send + Sync {
    fn read_summary<'a>(&'a self, request: &'a ReadSummary) -> ProviderFuture<'a>;
}
