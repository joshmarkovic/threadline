//! GitHub access scoped to a verified user and a selected resource owner.
use super::GITHUB_REST_VERSION;
use crate::domain::{RepositoryApiId, RepositoryName};
use reqwest::{
    Client, StatusCode, Url,
    header::{self, HeaderMap, HeaderValue},
    redirect,
};
use serde::{Deserialize, de::DeserializeOwned};
use std::{
    collections::BTreeSet,
    fmt,
    net::SocketAddr,
    num::{NonZeroU32, NonZeroU64},
    time::Duration,
};
pub use tokio_util::sync::CancellationToken;
pub mod reader;
mod response;
pub mod writer;
pub use response::{AccessError, AccessKind, RateLimit, Reachability, SsoAction};
use response::{RequestMode, partial_sso, request};

/// Never prints the credential or places it in a URL. The header remains in memory.
pub struct SecretToken(HeaderValue);
impl SecretToken {
    pub fn new(value: &str) -> Result<Self, AccessError> {
        if value.is_empty() || value.len() > 8192 || !value.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(AccessError::local(AccessKind::InvalidCredential));
        }
        let mut header = HeaderValue::from_str(&format!("Bearer {value}"))
            .map_err(|_| AccessError::local(AccessKind::InvalidCredential))?;
        header.set_sensitive(true);
        Ok(Self(header))
    }
    pub fn fixture() -> Self {
        Self::new("threadline-test-fixture").expect("fixed header")
    }
}
impl fmt::Debug for SecretToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretToken([redacted])")
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct OwnerScope(String);
impl OwnerScope {
    pub fn new(owner: &str) -> Result<Self, AccessError> {
        RepositoryName::new(owner, "repo")
            .map_err(|_| AccessError::local(AccessKind::WrongScope))?;
        Ok(Self(owner.to_ascii_lowercase()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    fn accepts(&self, owner: &str) -> bool {
        owner.eq_ignore_ascii_case(&self.0)
    }
}
#[derive(Clone, Eq, PartialEq, Deserialize)]
pub struct GitHubUser {
    pub id: NonZeroU64,
    pub login: String,
}
#[derive(Clone, Eq, PartialEq)]
pub struct RepositoryAccess {
    pub id: RepositoryApiId,
    pub node_id: String,
    pub route: RepositoryName,
    pub private: bool,
    /// The authenticated user's repository role, independent of the token's write permissions.
    pub permissions: Option<RepositoryPermissions>,
}
#[derive(Debug, Clone, Eq, PartialEq, Deserialize)]
pub struct RepositoryPermissions {
    pub pull: bool,
    pub push: bool,
    pub admin: bool,
    #[serde(default)]
    pub triage: bool,
    #[serde(default)]
    pub maintain: bool,
}
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Capability {
    Verified,
    Unverified,
}
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct CredentialCapabilities {
    pub repository_metadata_read: Capability,
    pub issue_write: Capability,
    pub pull_request_write: Capability,
    pub contents_write: Capability,
}
impl RepositoryAccess {
    /// A successful GET proves metadata access only. Repository roles do not prove PAT grants.
    pub fn credential_capabilities(&self) -> CredentialCapabilities {
        CredentialCapabilities {
            repository_metadata_read: Capability::Verified,
            issue_write: Capability::Unverified,
            pull_request_write: Capability::Unverified,
            contents_write: Capability::Unverified,
        }
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct RepositoryCursor {
    page: NonZeroU32,
    user: NonZeroU64,
    owner: OwnerScope,
    partial: Option<BTreeSet<NonZeroU64>>,
}
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RepositoryPage {
    pub repositories: Vec<RepositoryAccess>,
    pub next: Option<RepositoryCursor>,
    pub rate: RateLimit,
    /// Some, including an empty set, means GitHub withheld results on at least one page.
    pub sso_restricted_organizations: Option<BTreeSet<NonZeroU64>>,
}
impl RepositoryPage {
    pub fn complete(&self) -> bool {
        self.next.is_none() && self.sso_restricted_organizations.is_none()
    }
}
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ViewerPermission {
    Read,
    Triage,
    Write,
    Maintain,
    Admin,
    Unknown,
}
#[derive(Clone, Eq, PartialEq)]
pub struct GraphRepository {
    pub node_id: String,
    pub route: RepositoryName,
    pub private: bool,
    pub permission: Option<ViewerPermission>,
}
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum GraphIssue {
    Forbidden,
    NotFound,
    RateLimited,
    SsoRequired,
    Other,
}
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GraphRepositoryResult {
    pub repository: Option<GraphRepository>,
    pub errors: Vec<GraphIssue>,
}
impl GraphRepositoryResult {
    pub fn complete(&self) -> bool {
        self.repository.is_some() && self.errors.is_empty()
    }
}

/// Opaque HTTP entity tag scoped to the previously verified resource.
#[derive(Clone)]
pub struct RepositoryValidator {
    user: NonZeroU64,
    route: RepositoryName,
    id: RepositoryApiId,
    tag: HeaderValue,
}
#[derive(Debug)]
pub enum RepositoryRead {
    Fresh {
        repository: RepositoryAccess,
        validator: Option<RepositoryValidator>,
    },
    NotModified,
}
fn valid_etag(value: &HeaderValue) -> bool {
    let Ok(raw) = value.to_str() else {
        return false;
    };
    let tag = raw.strip_prefix("W/").unwrap_or(raw);
    tag.len() <= 1024
        && tag
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .is_some_and(|s| s.bytes().all(|b| b == 0x21 || (0x23..=0x7e).contains(&b)))
}
#[derive(Clone)]
pub struct GitHubClient {
    base: Url,
    http: Client,
    fixture: bool,
}
impl GitHubClient {
    pub fn production() -> Result<Self, AccessError> {
        Self::build(
            Url::parse("https://api.github.com/").expect("fixed origin"),
            false,
        )
    }
    /// A test transport restricted to the fixed synthetic credential. Production credentials never use HTTP.
    pub fn loopback(address: SocketAddr) -> Result<Self, AccessError> {
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(AccessError::local(AccessKind::WrongScope));
        }
        let base = Url::parse(&format!("http://{address}/"))
            .map_err(|_| AccessError::local(AccessKind::WrongScope))?;
        Self::build(base, true)
    }
    fn build(base: Url, fixture: bool) -> Result<Self, AccessError> {
        let mut builder = Client::builder()
            .user_agent(concat!("Threadline/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .retry(reqwest::retry::never())
            .redirect(redirect::Policy::none());
        if fixture {
            builder = builder.no_proxy();
        }
        Ok(Self {
            base,
            http: builder
                .build()
                .map_err(|_| AccessError::local(AccessKind::Transport))?,
            fixture,
        })
    }
    pub async fn authenticate(
        &self,
        token: SecretToken,
        owner: OwnerScope,
        expected_user: Option<NonZeroU64>,
        cancel: &CancellationToken,
    ) -> Result<GitHubSession, AccessError> {
        self.authenticate_with_guidance(token, owner, expected_user, cancel)
            .await
            .map(|(session, _)| session)
    }

    pub async fn authenticate_with_guidance(
        &self,
        token: SecretToken,
        owner: OwnerScope,
        expected_user: Option<NonZeroU64>,
        cancel: &CancellationToken,
    ) -> Result<(GitHubSession, RateLimit), AccessError> {
        if self.fixture && token.0 != "Bearer threadline-test-fixture" {
            return Err(AccessError::local(AccessKind::InvalidCredential));
        }
        let (user, headers): (GitHubUser, _) =
            request(self, &token, &owner, "user", RequestMode::Read, cancel)
                .await?
                .require_body()?;
        if expected_user.is_some_and(|id| id != user.id) {
            return Err(AccessError::received(AccessKind::IdentityMismatch, 200));
        }
        if user.login.is_empty() || user.login.len() > 255 {
            return Err(AccessError::received(AccessKind::MalformedResponse, 200));
        }
        let session = GitHubSession {
            client: self.clone(),
            user,
            scope: owner,
            token,
        };
        Ok((session, RateLimit::from_headers(&headers)))
    }
}
pub struct GitHubSession {
    client: GitHubClient,
    user: GitHubUser,
    scope: OwnerScope,
    token: SecretToken,
}
impl GitHubSession {
    pub fn user(&self) -> &GitHubUser {
        &self.user
    }
    pub fn owner(&self) -> &OwnerScope {
        &self.scope
    }
    fn check_route(&self, route: &RepositoryName) -> Result<(), AccessError> {
        if self.scope.accepts(route.owner()) {
            Ok(())
        } else {
            Err(AccessError::local(AccessKind::WrongScope))
        }
    }
    pub async fn repository(
        &self,
        route: &RepositoryName,
        expected: Option<RepositoryApiId>,
        cancel: &CancellationToken,
    ) -> Result<RepositoryAccess, AccessError> {
        match self
            .repository_if_changed(route, expected, None, cancel)
            .await?
        {
            RepositoryRead::Fresh { repository, .. } => Ok(repository),
            RepositoryRead::NotModified => {
                Err(AccessError::received(AccessKind::UnexpectedStatus, 304))
            }
        }
    }
    /// A validator is bound to the user, owner, route and immutable repository ID that produced it.
    pub async fn repository_if_changed(
        &self,
        route: &RepositoryName,
        expected: Option<RepositoryApiId>,
        previous: Option<&RepositoryValidator>,
        cancel: &CancellationToken,
    ) -> Result<RepositoryRead, AccessError> {
        self.repository_with_guidance(route, expected, previous, cancel)
            .await
            .map(|(repository, _)| repository)
    }

    /// Return the scoped repository and its server retry guidance together.
    pub async fn repository_with_guidance(
        &self,
        route: &RepositoryName,
        expected: Option<RepositoryApiId>,
        previous: Option<&RepositoryValidator>,
        cancel: &CancellationToken,
    ) -> Result<(RepositoryRead, RateLimit), AccessError> {
        self.check_route(route)?;
        if previous.is_some_and(|v| {
            v.user != self.user.id
                || !v.route.owner().eq_ignore_ascii_case(route.owner())
                || !v.route.name().eq_ignore_ascii_case(route.name())
                || expected.is_some_and(|id| id != v.id)
        }) {
            return Err(AccessError::local(AccessKind::WrongScope));
        }
        let path = format!("repos/{}/{}", route.owner(), route.name());
        let mode = previous.map_or(RequestMode::Read, |v| RequestMode::Conditional(&v.tag));
        let response =
            request::<RepoWire>(&self.client, &self.token, &self.scope, &path, mode, cancel)
                .await?;
        let rate = RateLimit::from_headers(&response.headers);
        let Some(wire) = response.body else {
            return Ok((RepositoryRead::NotModified, rate));
        };
        let repo = wire.into_repo()?;
        if !self.scope.accepts(repo.route.owner())
            || !repo.route.name().eq_ignore_ascii_case(route.name())
            || expected
                .or(previous.map(|v| v.id))
                .is_some_and(|id| id != repo.id)
        {
            return Err(AccessError::received(AccessKind::IdentityMismatch, 200));
        }
        let validator = response
            .headers
            .get(header::ETAG)
            .filter(|tag| valid_etag(tag))
            .map(|tag| RepositoryValidator {
                user: self.user.id,
                route: repo.route.clone(),
                id: repo.id,
                tag: tag.clone(),
            });
        Ok((
            RepositoryRead::Fresh {
                repository: repo,
                validator,
            },
            rate,
        ))
    }
    pub async fn verify_private_repository(
        &self,
        route: &RepositoryName,
        expected: Option<RepositoryApiId>,
        cancel: &CancellationToken,
    ) -> Result<RepositoryAccess, AccessError> {
        let repo = self.repository(route, expected, cancel).await?;
        if !repo.private {
            return Err(AccessError::received(
                AccessKind::PrivateRepositoryRequired,
                200,
            ));
        }
        Ok(repo)
    }
    pub async fn discover(
        &self,
        cursor: Option<&RepositoryCursor>,
        cancel: &CancellationToken,
    ) -> Result<RepositoryPage, AccessError> {
        if cursor.is_some_and(|c| c.owner != self.scope || c.user != self.user.id) {
            return Err(AccessError::local(AccessKind::WrongScope));
        }
        let page = cursor.map_or(1, |c| c.page.get());
        let (wire, headers): (Vec<RepoWire>, _) = self
            .get(&format!("user/repos?per_page=100&page={page}"), cancel)
            .await?;
        let repositories = wire
            .into_iter()
            .filter(|r| self.scope.accepts(&r.owner.login))
            .map(RepoWire::into_repo)
            .collect::<Result<_, _>>()?;
        let mut partial = cursor.and_then(|c| c.partial.clone());
        if let Some(ids) = partial_sso(&headers) {
            partial.get_or_insert_with(BTreeSet::new).extend(ids);
        }
        let next = next_page(&headers, &self.client.base, page)?.map(|page| RepositoryCursor {
            page,
            owner: self.scope.clone(),
            user: self.user.id,
            partial: partial.clone(),
        });
        Ok(RepositoryPage {
            repositories,
            next,
            rate: RateLimit::from_headers(&headers),
            sso_restricted_organizations: partial,
        })
    }
    /// The query is fixed; validated user input enters through GraphQL variables.
    pub async fn graphql_repository(
        &self,
        route: &RepositoryName,
        cancel: &CancellationToken,
    ) -> Result<GraphRepositoryResult, AccessError> {
        self.check_route(route)?;
        const QUERY: &str = "query RepositoryAccess($owner:String!,$name:String!){repository(owner:$owner,name:$name){id name isPrivate owner{login} viewerPermission}}";
        let variables = serde_json::json!({"query":QUERY,"variables":{"owner":route.owner(),"name":route.name()}});
        let (wire, _): (GraphEnvelope, _) = request(
            &self.client,
            &self.token,
            &self.scope,
            "graphql",
            RequestMode::Graph(&variables),
            cancel,
        )
        .await?
        .require_body()?;
        let errors = wire
            .errors
            .into_iter()
            .map(|e| {
                match e
                    .kind
                    .as_deref()
                    .or_else(|| e.extensions.as_ref().and_then(|v| v.code.as_deref()))
                {
                    Some("FORBIDDEN") => GraphIssue::Forbidden,
                    Some("NOT_FOUND") => GraphIssue::NotFound,
                    Some("RATE_LIMITED") => GraphIssue::RateLimited,
                    Some("SAML_SSO_REQUIRED") => GraphIssue::SsoRequired,
                    _ => GraphIssue::Other,
                }
            })
            .collect::<Vec<_>>();
        let repository = wire
            .data
            .and_then(|d| d.repository)
            .map(|node| {
                let parsed = RepositoryName::new(&node.owner.login, &node.name)
                    .map_err(|_| AccessError::received(AccessKind::MalformedResponse, 200))?;
                if !self.scope.accepts(parsed.owner())
                    || !parsed.name().eq_ignore_ascii_case(route.name())
                    || node.id.is_empty()
                {
                    return Err(AccessError::received(AccessKind::IdentityMismatch, 200));
                }
                let permission = node.permission.map(|v| match v.as_str() {
                    "READ" => ViewerPermission::Read,
                    "TRIAGE" => ViewerPermission::Triage,
                    "WRITE" => ViewerPermission::Write,
                    "MAINTAIN" => ViewerPermission::Maintain,
                    "ADMIN" => ViewerPermission::Admin,
                    _ => ViewerPermission::Unknown,
                });
                Ok(GraphRepository {
                    node_id: node.id,
                    route: parsed,
                    private: node.private,
                    permission,
                })
            })
            .transpose()?;
        if errors.is_empty() && repository.is_none() {
            return Err(AccessError::received(
                AccessKind::MissingOrInaccessible,
                200,
            ));
        }
        Ok(GraphRepositoryResult { repository, errors })
    }
    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        cancel: &CancellationToken,
    ) -> Result<(T, HeaderMap), AccessError> {
        request(
            &self.client,
            &self.token,
            &self.scope,
            path,
            RequestMode::Read,
            cancel,
        )
        .await?
        .require_body()
    }
}

#[derive(Deserialize)]
struct OwnerWire {
    login: String,
}
#[derive(Deserialize)]
struct RepoWire {
    id: NonZeroU64,
    node_id: String,
    name: String,
    owner: OwnerWire,
    private: bool,
    permissions: Option<RepositoryPermissions>,
}
impl RepoWire {
    fn into_repo(self) -> Result<RepositoryAccess, AccessError> {
        if self.node_id.is_empty() {
            return Err(AccessError::received(AccessKind::MalformedResponse, 200));
        }
        let route = RepositoryName::new(&self.owner.login, &self.name)
            .map_err(|_| AccessError::received(AccessKind::MalformedResponse, 200))?;
        Ok(RepositoryAccess {
            id: RepositoryApiId::new(self.id),
            node_id: self.node_id,
            route,
            private: self.private,
            permissions: self.permissions,
        })
    }
}
#[derive(Deserialize)]
struct GraphEnvelope {
    data: Option<GraphData>,
    #[serde(default)]
    errors: Vec<GraphError>,
}
#[derive(Deserialize)]
struct GraphData {
    repository: Option<GraphNode>,
}
#[derive(Deserialize)]
struct GraphNode {
    id: String,
    name: String,
    #[serde(rename = "isPrivate")]
    private: bool,
    owner: OwnerWire,
    #[serde(rename = "viewerPermission")]
    permission: Option<String>,
}
#[derive(Deserialize)]
struct GraphError {
    #[serde(rename = "type")]
    kind: Option<String>,
    extensions: Option<GraphExtensions>,
}
#[derive(Deserialize)]
struct GraphExtensions {
    code: Option<String>,
}

fn next_page(
    headers: &HeaderMap,
    base: &Url,
    current: u32,
) -> Result<Option<NonZeroU32>, AccessError> {
    let request = base
        .join("user/repos?per_page=100")
        .expect("fixed discovery route");
    next_page_for(headers, &request, current, |path| path == "/user/repos")
}

fn next_page_for(
    headers: &HeaderMap,
    request: &Url,
    current: u32,
    accepts_path: impl Fn(&str) -> bool,
) -> Result<Option<NonZeroU32>, AccessError> {
    let invalid = || AccessError::received(AccessKind::InvalidPagination, 200);
    let mut next = None;
    for header in headers.get_all(header::LINK) {
        let header = header.to_str().map_err(|_| invalid())?;
        for part in header.split(',') {
            let mut segments = part.trim().split(';');
            let target = segments.next().ok_or_else(invalid)?.trim();
            let rel = segments
                .find_map(|attr| {
                    attr.trim()
                        .split_once('=')
                        .filter(|(key, _)| key.eq_ignore_ascii_case("rel"))
                        .map(|(_, val)| val.trim_matches('"'))
                })
                .ok_or_else(invalid)?;
            let target = target
                .strip_prefix('<')
                .and_then(|v| v.strip_suffix('>'))
                .ok_or_else(invalid)?;
            let is_next = rel.split_ascii_whitespace().any(|r| r == "next");
            if !is_next {
                continue;
            }
            let url = request.join(target).map_err(|_| invalid())?;
            if url.origin() != request.origin()
                || !accepts_path(url.path())
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
            {
                return Err(invalid());
            }
            let mut number = None;
            let mut parameters = std::collections::HashSet::new();
            for (k, v) in url.query_pairs() {
                if !parameters.insert(k.clone()) {
                    return Err(invalid());
                }
                match k.as_ref() {
                    "page" if number.is_none() => {
                        number = Some(v.parse::<NonZeroU32>().map_err(|_| invalid())?)
                    }
                    _ if request.query_pairs().any(|(expected_key, expected_value)| {
                        expected_key == k && expected_value == v
                    }) => {}
                    _ => return Err(invalid()),
                }
            }
            let n = number.ok_or_else(invalid)?;
            if current.checked_add(1) != Some(n.get()) || next.replace(n).is_some() {
                return Err(invalid());
            }
        }
    }
    Ok(next)
}

// Keep profile identifiers, private routes, entity tags and tokens out of routine diagnostics.
macro_rules! private_debug {
    ($($ty:ident),+ $(,)?) => {$(
        impl fmt::Debug for $ty{
            fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{
                f.debug_struct(stringify!($ty)).finish_non_exhaustive()
            }
        }
    )+};
}
private_debug!(
    OwnerScope,
    GitHubUser,
    RepositoryAccess,
    GraphRepository,
    RepositoryCursor,
    RepositoryValidator,
    GitHubSession
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_010_owner_and_origin_bind_credential_routing() {
        let scope = OwnerScope::new("Fixture-Owner").unwrap();
        assert!(scope.accepts("FIXTURE-OWNER"));
        assert!(!scope.accepts("other"));
        for bad in [
            "github.com/fixture",
            "https://fixture",
            "name@github.com",
            "../fixture",
            "",
        ] {
            assert!(OwnerScope::new(bad).is_err());
        }
        assert!(GitHubClient::loopback("192.0.2.1:443".parse().unwrap()).is_err());
        assert!(GitHubClient::loopback("127.0.0.1:0".parse().unwrap()).is_err());
    }
    #[test]
    fn ut_020_credentials_private_routes_and_sso_requests_are_redacted() {
        let token = SecretToken::new("synthetic-secret").unwrap();
        let owner = OwnerScope::new("private-owner").unwrap();
        let repo = RepoWire {
            id: NonZeroU64::new(1).unwrap(),
            node_id: "private-node".into(),
            name: "private-repo".into(),
            owner: OwnerWire {
                login: "private-owner".into(),
            },
            private: true,
            permissions: None,
        }
        .into_repo()
        .unwrap();
        let user = GitHubUser {
            id: NonZeroU64::new(2).unwrap(),
            login: "private-login".into(),
        };
        let action = response::web_action(
            "https://github.com/orgs/private-owner/sso?authorization_request=sensitive-handshake",
            &owner,
            &token,
        )
        .unwrap();
        let debug = format!(
            "{token:?} {:?} {repo:?} {user:?} {owner:?} {action:?}",
            token.0
        );
        for secret in [
            "synthetic-secret",
            "private-repo",
            "private-node",
            "private-owner",
            "private-login",
            "sensitive-handshake",
        ] {
            assert!(!debug.contains(secret));
        }
        assert!(token.0.is_sensitive());
        assert!(SecretToken::new("line\nbreak").is_err());
    }
}
