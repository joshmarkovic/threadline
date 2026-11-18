use std::{
    str,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use threadline_core::{
    domain::{RepositoryApiId, RepositoryName},
    github::{
        AccessError, AccessKind, GitHubClient, GitHubSession, OwnerScope, Reachability, SecretToken,
    },
    sync::Cancellation,
};
use threadline_platform::{
    Credential, CredentialRef, CredentialStore, NativeCredentialStore, PlatformError,
};
use threadline_storage::{
    AccountId, RepositoryId, Store,
    access::{
        AccessProfile, AccessState, CredentialKind, DiscoveryState, RepositoryObservation,
        RepositoryObservationSource, RepositoryView,
    },
};
use zeroize::Zeroize;

pub(crate) struct Pat(Credential);

impl Pat {
    pub(crate) fn from_input(value: String) -> Self {
        Self(Credential(value.into_bytes()))
    }

    fn token(&self) -> Result<SecretToken, Problem> {
        let value = str::from_utf8(&self.0.0).map_err(|_| Problem::InvalidPat)?;
        SecretToken::new(value).map_err(Problem::GitHub)
    }

    fn validated_kind(&self, fixture: bool) -> Result<CredentialKind, Problem> {
        let bytes = self.0.0.as_slice();
        if fixture {
            return (bytes == b"threadline-test-fixture")
                .then_some(CredentialKind::Classic)
                .ok_or(Problem::InvalidPat);
        }
        if classify_pat(bytes, b"github_pat_", true) {
            return Ok(CredentialKind::FineGrained);
        }
        if classify_pat(bytes, b"ghp_", false) {
            return Ok(CredentialKind::Classic);
        }
        Err(Problem::InvalidPat)
    }
}

fn classify_pat(bytes: &[u8], prefix: &[u8], underscores: bool) -> bool {
    bytes.strip_prefix(prefix).is_some_and(|tail| {
        !tail.is_empty()
            && tail
                .iter()
                .all(|&byte| byte.is_ascii_alphanumeric() || (underscores && byte == b'_'))
    })
}

impl Drop for Pat {
    fn drop(&mut self) {
        self.0.0.zeroize();
    }
}

#[derive(Clone)]
pub(crate) struct AccountService {
    store: Arc<Store>,
    credentials: Arc<dyn CredentialStore + Send + Sync>,
    github: GitHubClient,
    pub(crate) fixture: bool,
}

pub(crate) enum AccountCommand {
    Load,
    Connect {
        repository: String,
        pat: Pat,
        renew: Option<AccessProfile>,
    },
    Refresh {
        profile: AccessProfile,
        repository: Option<RepositoryView>,
        requested: Option<String>,
    },
    Select {
        profile: AccessProfile,
        repository: RepositoryView,
    },
    Remove {
        account: AccountId,
        repository: RepositoryId,
    },
    Switch(AccountId),
    SetOffline {
        account: AccountId,
        offline: bool,
    },
    SignOut(AccountId),
}

#[derive(Default)]
pub(crate) struct AccountView {
    pub(crate) active: Option<AccountId>,
    pub(crate) profiles: Vec<AccessProfile>,
    pub(crate) repositories: Vec<RepositoryView>,
    pub(crate) offline: bool,
}

pub(crate) struct AccountReply {
    pub(crate) view: AccountView,
    pub(crate) message: Option<&'static str>,
    pub(crate) github_error: Option<AccessError>,
    pub(crate) reachability: Reachability,
}

enum Problem {
    GitHub(AccessError),
    GitHubCheck(AccessError, Reachability),
    Storage(threadline_storage::Error),
    Credential(PlatformError),
    InvalidRepository,
    InvalidPat,
    NoSavedCredential,
    Offline,
    InactiveProfile,
    Cancelled,
}

struct RepositoryCheck<'a> {
    route: &'a RepositoryName,
    prior: Option<(RepositoryId, RepositoryApiId)>,
    private: bool,
}

impl From<threadline_storage::Error> for Problem {
    fn from(error: threadline_storage::Error) -> Self {
        Self::Storage(error)
    }
}

impl From<AccessError> for Problem {
    fn from(error: AccessError) -> Self {
        Self::GitHub(error)
    }
}

impl From<PlatformError> for Problem {
    fn from(error: PlatformError) -> Self {
        Self::Credential(error)
    }
}

impl AccountService {
    pub(crate) fn new(store: Arc<Store>) -> Option<Self> {
        #[cfg(feature = "e2e")]
        if let Some(address) = std::env::var_os("THREADLINE_E2E_GITHUB_ADDR") {
            let isolated = std::env::args_os().any(|arg| arg == "--e2e-report")
                && std::env::var_os("THREADLINE_PROFILE_DIR").is_some();
            if !isolated {
                return None;
            }
            let github = GitHubClient::loopback(address.to_str()?.parse().ok()?).ok()?;
            return Some(Self {
                store,
                github,
                credentials: Arc::new(FixtureVault::default()),
                fixture: true,
            });
        }
        Some(Self {
            store,
            credentials: Arc::new(NativeCredentialStore),
            github: GitHubClient::production().ok()?,
            fixture: false,
        })
    }

    pub(crate) async fn execute(
        &self,
        command: AccountCommand,
        cancel: &Cancellation,
    ) -> AccountReply {
        let result = self.run(command, cancel).await;
        let view = match self.view() {
            Ok(view) => view,
            Err(error) => return problem_reply(AccountView::default(), error),
        };
        match result {
            Ok((message, reachability)) => AccountReply {
                view,
                message,
                github_error: None,
                reachability,
            },
            Err(error) => problem_reply(view, error),
        }
    }

    fn view(&self) -> Result<AccountView, Problem> {
        let active = self.store.active_github_account()?;
        let profiles = self.store.github_access_profiles()?;
        let (repositories, offline) = match active {
            Some(account) => (
                self.store.github_repository_views(account)?,
                self.store.dispatch_profile(account)?.offline,
            ),
            None => (Vec::new(), false),
        };
        Ok(AccountView {
            active,
            profiles,
            repositories,
            offline,
        })
    }

    async fn run(
        &self,
        command: AccountCommand,
        cancel: &Cancellation,
    ) -> Result<(Option<&'static str>, Reachability), Problem> {
        match command {
            AccountCommand::Load => Ok((None, Reachability::NotAttempted)),
            AccountCommand::Connect {
                repository,
                pat,
                renew,
            } => self.connect(&repository, pat, renew.as_ref(), cancel).await,
            AccountCommand::Refresh {
                profile,
                repository,
                requested,
            } => {
                self.refresh(
                    &profile,
                    repository.as_ref(),
                    requested.as_deref(),
                    false,
                    cancel,
                )
                .await
            }
            AccountCommand::Select {
                profile,
                repository,
            } => {
                self.refresh(&profile, Some(&repository), None, true, cancel)
                    .await
            }
            AccountCommand::Remove {
                account,
                repository,
            } => {
                self.require_active(account)?;
                self.store
                    .select_github_repository(account, repository, false)?;
                Ok((
                    Some("Monitoring is disabled. Cached content is retained."),
                    Reachability::NotAttempted,
                ))
            }
            AccountCommand::Switch(account) => {
                self.store.activate_local_profile(account)?;
                Ok((None, Reachability::NotAttempted))
            }
            AccountCommand::SetOffline { account, offline } => {
                self.require_active(account)?;
                let profile = self.store.dispatch_profile(account)?;
                self.store
                    .set_dispatch_controls(account, offline, profile.outbox_paused)?;
                Ok((None, Reachability::NotAttempted))
            }
            AccountCommand::SignOut(account) => {
                self.require_active(account)?;
                let profiles = self.store.sign_out_github_account(account, now_ms())?;
                let mut failure = None;
                for profile in profiles {
                    if let Err(error) = self.remove_saved_credential(&profile) {
                        failure = Some(error);
                    }
                }
                match failure {
                    Some(error) => Err(error),
                    None => Ok((
                        Some("Signed out. Local content is still available."),
                        Reachability::NotAttempted,
                    )),
                }
            }
        }
    }

    async fn connect(
        &self,
        repository: &str,
        pat: Pat,
        renew: Option<&AccessProfile>,
        cancel: &Cancellation,
    ) -> Result<(Option<&'static str>, Reachability), Problem> {
        let route = parse_repository(repository)?;
        let kind = pat.validated_kind(self.fixture)?;
        if let Some(account) = self.store.active_github_account()?
            && self.store.dispatch_profile(account)?.offline
        {
            return Err(Problem::Offline);
        }
        if let Some(profile) = renew {
            self.require_active(profile.account)?;
            if !profile.owner_scope.eq_ignore_ascii_case(route.owner()) {
                return Err(Problem::InvalidRepository);
            }
        }
        let owner = OwnerScope::new(route.owner())?;
        let session = self
            .github
            .authenticate(
                pat.token()?,
                owner,
                renew.map(|profile| profile.user_id),
                cancel,
            )
            .await?;
        check_cancel(cancel)?;
        let user = session.user();
        let account = self.store.ensure_account("github.com", user.id.get())?;
        let known = self.known_repository(account, &route)?;
        let profile = self.store.reserve_access_profile_credential(
            account,
            user.id,
            &user.login,
            session.owner().as_str(),
            kind,
            renew,
        )?;
        let reference = credential_ref(&profile)?;
        if let Err(error) = self
            .store
            .install_current_credential(&profile, || self.credentials.save(&reference, &pat.0))?
        {
            self.store.record_access_result(
                &profile,
                kind,
                AccessState::CredentialUnavailable,
                DiscoveryState::Unchecked,
                now_ms(),
            )?;
            return Err(error.into());
        }
        self.store.activate_local_profile(account)?;
        drop(pat);
        self.check_session(
            &profile,
            kind,
            &session,
            Some(RepositoryCheck {
                route: &route,
                prior: known,
                private: true,
            }),
            false,
            cancel,
        )
        .await
    }

    async fn refresh(
        &self,
        previous: &AccessProfile,
        repository: Option<&RepositoryView>,
        requested: Option<&str>,
        select: bool,
        cancel: &Cancellation,
    ) -> Result<(Option<&'static str>, Reachability), Problem> {
        self.require_active(previous.account)?;
        if self.store.dispatch_profile(previous.account)?.offline {
            return Err(Problem::Offline);
        }
        let current = self
            .store
            .github_access_profile(previous.account, &previous.owner_scope)?
            .ok_or(Problem::InactiveProfile)?;
        if current.access == AccessState::SignedOut {
            return Err(Problem::NoSavedCredential);
        }
        let profile = self.store.begin_access_check(&current)?;
        let reference = credential_ref(&profile)?;
        let pat = match self
            .store
            .with_current_credential(&profile, || self.credentials.read(&reference))?
        {
            Ok(Some(value)) => Pat(value),
            Ok(None) => {
                self.store.record_access_result(
                    &profile,
                    profile.kind,
                    AccessState::CredentialUnavailable,
                    DiscoveryState::Unchecked,
                    now_ms(),
                )?;
                return Err(Problem::NoSavedCredential);
            }
            Err(error) => {
                self.store.record_access_result(
                    &profile,
                    profile.kind,
                    AccessState::CredentialUnavailable,
                    DiscoveryState::Unchecked,
                    now_ms(),
                )?;
                return Err(error.into());
            }
        };
        let kind = pat.validated_kind(self.fixture)?;
        let owner = OwnerScope::new(&profile.owner_scope)?;
        let session = match self
            .github
            .authenticate(pat.token()?, owner, Some(profile.user_id), cancel)
            .await
        {
            Ok(session) => session,
            Err(error) => {
                if error.kind != AccessKind::Cancelled {
                    self.store.record_access_result(
                        &profile,
                        profile.kind,
                        access_state(&error),
                        DiscoveryState::Unchecked,
                        now_ms(),
                    )?;
                }
                return Err(error.into());
            }
        };
        drop(pat);
        let typed_route = repository
            .map(|repo| RepositoryName::new(&repo.owner, &repo.name))
            .transpose()
            .map_err(|_| Problem::InvalidRepository)?;
        let requested_route = if repository.is_none() {
            requested.map(parse_repository).transpose()?
        } else {
            None
        };
        let requested_route = requested_route
            .filter(|route| route.owner().eq_ignore_ascii_case(&profile.owner_scope));
        let listed_target =
            typed_route
                .as_ref()
                .zip(repository)
                .map(|(route, repo)| RepositoryCheck {
                    route,
                    prior: Some((repo.id, RepositoryApiId::new(repo.provider_id))),
                    private: repo.private == Some(true),
                });
        let checked = match (listed_target, requested_route.as_ref()) {
            (Some(target), _) => Some(target),
            (None, Some(route)) => Some(RepositoryCheck {
                route,
                prior: self.known_repository(profile.account, route)?,
                private: true,
            }),
            (None, None) => None,
        };
        self.check_session(&profile, kind, &session, checked, select, cancel)
            .await
    }

    async fn check_session(
        &self,
        profile: &AccessProfile,
        kind: CredentialKind,
        session: &GitHubSession,
        target: Option<RepositoryCheck<'_>>,
        select: bool,
        cancel: &Cancellation,
    ) -> Result<(Option<&'static str>, Reachability), Problem> {
        let mut issue = None;
        let mut reachability = Reachability::ReceivedResponse;
        let mut access = AccessState::Unchecked;
        let mut checked_repo = None;
        if let Some(RepositoryCheck {
            route,
            prior,
            private,
        }) = target.as_ref()
        {
            let expected = prior.map(|(_, provider_id)| provider_id);
            let result = if *private {
                session
                    .verify_private_repository(route, expected, cancel)
                    .await
            } else {
                session.repository(route, expected, cancel).await
            };
            check_cancel(cancel)?;
            match result {
                Ok(repo) => {
                    checked_repo =
                        Some(self.observe(profile, &repo, RepositoryObservationSource::Direct)?);
                    access = AccessState::Readable;
                }
                Err(error) => {
                    reachability = error.reachability;
                    access = access_state(&error);
                    if let Some((id, _)) = prior {
                        self.store
                            .mark_repository_access(profile, *id, access, now_ms())?;
                    }
                    issue = Some(error);
                }
            }
        }

        let mut discovery = DiscoveryState::Partial;
        let mut cursor = None;
        for _ in 0..100 {
            match session.discover(cursor.as_ref(), cancel).await {
                Ok(page) => {
                    reachability = Reachability::ReceivedResponse;
                    check_cancel(cancel)?;
                    for repo in page.repositories {
                        // Keep a direct denial authoritative even if the repository appears in discovery.
                        if issue.is_some()
                            && target.as_ref().is_some_and(|check| {
                                let route = check.route;
                                repo.route.owner().eq_ignore_ascii_case(route.owner())
                                    && repo.route.name().eq_ignore_ascii_case(route.name())
                            })
                        {
                            continue;
                        }
                        self.observe(profile, &repo, RepositoryObservationSource::Discovery)?;
                    }
                    discovery = if page.sso_restricted_organizations.is_some() {
                        DiscoveryState::RestrictedBySso
                    } else if page.next.is_none() {
                        DiscoveryState::Complete
                    } else {
                        DiscoveryState::Partial
                    };
                    cursor = page.next;
                    if cursor.is_none() {
                        break;
                    }
                }
                Err(error) => {
                    reachability = error.reachability;
                    check_cancel(cancel)?;
                    if issue.is_none() {
                        issue = Some(error);
                    }
                    break;
                }
            }
        }
        check_cancel(cancel)?;
        if !self
            .store
            .record_access_result(profile, kind, access, discovery, now_ms())?
        {
            return Err(Problem::InactiveProfile);
        }
        if select && let Some(id) = checked_repo {
            self.store
                .select_github_repository(profile.account, id, true)?;
        }
        if let Some(error) = issue {
            return Err(Problem::GitHubCheck(error, reachability));
        }
        let notice = match (checked_repo, select, discovery) {
            (_, _, DiscoveryState::RestrictedBySso) => {
                Some("GitHub withheld some repositories because of SSO.")
            }
            (Some(_), true, _) => Some("Repository selected. Read access verified."),
            (Some(_), false, _) => Some("Read access verified. Select a repository to monitor it."),
            _ => Some("Repository list refreshed."),
        };
        Ok((notice, reachability))
    }

    fn observe(
        &self,
        profile: &AccessProfile,
        repo: &threadline_core::github::RepositoryAccess,
        source: RepositoryObservationSource,
    ) -> Result<RepositoryId, Problem> {
        Ok(self.store.observe_github_repository(
            profile,
            &RepositoryObservation {
                provider_id: std::num::NonZeroU64::new(repo.id.get())
                    .ok_or(Problem::InvalidRepository)?,
                owner: repo.route.owner(),
                name: repo.route.name(),
                private: repo.private,
                source,
            },
            now_ms(),
        )?)
    }

    fn known_repository(
        &self,
        account: AccountId,
        route: &RepositoryName,
    ) -> Result<Option<(RepositoryId, RepositoryApiId)>, Problem> {
        Ok(self
            .store
            .github_repository_views(account)?
            .into_iter()
            .find(|repo| {
                repo.owner.eq_ignore_ascii_case(route.owner())
                    && repo.name.eq_ignore_ascii_case(route.name())
            })
            .map(|repo| (repo.id, RepositoryApiId::new(repo.provider_id))))
    }

    fn remove_saved_credential(&self, profile: &AccessProfile) -> Result<(), Problem> {
        let reference = credential_ref(profile)?;
        self.store
            .with_current_credential(profile, || self.credentials.remove(&reference))??;
        Ok(())
    }

    fn require_active(&self, account: AccountId) -> Result<(), Problem> {
        if self.store.active_github_account()? != Some(account) {
            return Err(Problem::InactiveProfile);
        }
        Ok(())
    }
}

fn credential_ref(profile: &AccessProfile) -> Result<CredentialRef, Problem> {
    Ok(CredentialRef::github_profile(
        profile.user_id.get(),
        &profile.owner_scope,
        &profile.credential_ref,
    )?)
}

fn parse_repository(raw: &str) -> Result<RepositoryName, Problem> {
    let (owner, name) = raw
        .trim()
        .split_once('/')
        .ok_or(Problem::InvalidRepository)?;
    RepositoryName::new(owner, name).map_err(|_| Problem::InvalidRepository)
}

fn check_cancel(cancel: &Cancellation) -> Result<(), Problem> {
    if cancel.is_cancelled() {
        return Err(Problem::Cancelled);
    }
    Ok(())
}

fn now_ms() -> i64 {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
}

fn access_state(error: &AccessError) -> AccessState {
    match error.kind {
        AccessKind::Unauthorized | AccessKind::InvalidCredential => AccessState::Unauthorized,
        AccessKind::SsoRequired => AccessState::SsoRequired,
        AccessKind::ApprovalPending => AccessState::ApprovalPending,
        AccessKind::PermissionDenied => AccessState::InsufficientPermission,
        AccessKind::MissingOrInaccessible
        | AccessKind::PrivateRepositoryRequired
        | AccessKind::IdentityMismatch => AccessState::NotFoundOrDenied,
        AccessKind::Transport | AccessKind::Timeout => AccessState::TransportUnavailable,
        AccessKind::RateLimited => AccessState::RateLimited,
        _ => AccessState::ServiceUnavailable,
    }
}

fn problem_reply(view: AccountView, error: Problem) -> AccountReply {
    if let Problem::GitHubCheck(github_error, reachability) = error {
        return AccountReply {
            view,
            message: None,
            reachability,
            github_error: Some(github_error),
        };
    }
    if let Problem::GitHub(github_error) = error {
        return AccountReply {
            view,
            message: None,
            reachability: github_error.reachability,
            github_error: Some(github_error),
        };
    }
    let message = match error {
        Problem::Storage(error) => {
            let _ = error;
            "The account change could not be saved."
        }
        Problem::Credential(error) => {
            let _ = error;
            "Protected credential storage is unavailable or locked. Local content is still available."
        }
        Problem::InvalidRepository => "Enter a GitHub repository as owner/name.",
        Problem::InvalidPat => "Enter a valid personal access token.",
        Problem::NoSavedCredential => {
            "No saved token is available. Enter a token for this account."
        }
        Problem::Offline => "Connection is paused. Go online before checking GitHub.",
        Problem::InactiveProfile => "The active account changed. Try the account action again.",
        Problem::Cancelled => "GitHub check cancelled.",
        Problem::GitHub(_) | Problem::GitHubCheck(_, _) => unreachable!(),
    };
    AccountReply {
        view,
        message: Some(message),
        github_error: None,
        reachability: Reachability::NotAttempted,
    }
}

#[cfg(any(test, feature = "e2e"))]
type FixtureCredentials = std::collections::HashMap<(String, String), zeroize::Zeroizing<Vec<u8>>>;

#[cfg(any(test, feature = "e2e"))]
#[derive(Default)]
struct FixtureVault(std::sync::Mutex<FixtureCredentials>);

#[cfg(any(test, feature = "e2e"))]
impl CredentialStore for FixtureVault {
    fn read(&self, key: &CredentialRef) -> Result<Option<Credential>, PlatformError> {
        let map = self.0.lock().map_err(|_| PlatformError::Unavailable)?;
        Ok(map
            .get(&(key.scope.clone(), key.name.clone()))
            .map(|entry| Credential(entry.to_vec())))
    }

    fn save(&self, key: &CredentialRef, value: &Credential) -> Result<(), PlatformError> {
        if value.0 != b"threadline-test-fixture" {
            return Err(PlatformError::InvalidCredentialValue);
        }
        let mut map = self.0.lock().map_err(|_| PlatformError::Unavailable)?;
        map.insert(
            (key.scope.clone(), key.name.clone()),
            zeroize::Zeroizing::new(value.0.clone()),
        );
        Ok(())
    }

    fn remove(&self, key: &CredentialRef) -> Result<(), PlatformError> {
        let mut map = self.0.lock().map_err(|_| PlatformError::Unavailable)?;
        map.remove(&(key.scope.clone(), key.name.clone()));
        Ok(())
    }
}

#[cfg(test)]
#[path = "account_tests.rs"]
mod tests;
