use std::{
    str,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use threadline_core::{
    domain::{RepositoryApiId, RepositoryName},
    github::{
        AccessError, AccessKind, GitHubClient, GitHubSession, OwnerScope, RateLimit, Reachability,
        RepositoryRead, SecretToken,
    },
    sync::Cancellation,
};
use threadline_platform::{
    Credential, CredentialRef, CredentialStore, NativeCredentialStore, PlatformError,
    github_cli::{CliError, GitHubCli},
};
use threadline_storage::{
    AccountId, RepositoryId, Store,
    access::{
        AccessProfile, AccessState, CredentialKind, DiscoveryState, RepositoryObservation,
        RepositoryObservationSource, RepositoryView,
    },
};
use zeroize::Zeroize;

pub(crate) const FIXTURE_PAT: &str = "threadline-test-fixture";

#[cfg(feature = "e2e")]
mod fixture_auth;

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
            return (bytes == FIXTURE_PAT.as_bytes())
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
    #[cfg(feature = "e2e")]
    LoadFixture,
    Connect {
        repository: String,
        pat: Pat,
        renew: Option<AccessProfile>,
    },
    ConnectGitHubCli {
        repository: String,
        login: String,
        user_id: std::num::NonZeroU64,
        renew: Option<AccessProfile>,
    },
    Refresh {
        profile: AccessProfile,
        repository: Option<RepositoryView>,
        requested: Option<String>,
    },
    RefreshAll(AccountId),
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
    pub(crate) background_updates_allowed: Option<bool>,
}

impl AccountView {
    pub(crate) fn visible_repositories(&self) -> impl Iterator<Item = &RepositoryView> {
        self.repositories.iter().filter(|repo| {
            repo.selected || repo.cached_items > 0 || repo.access != AccessState::NotFoundOrDenied
        })
    }
}

pub(crate) struct AccountReply {
    pub(crate) view: AccountView,
    pub(crate) message: Option<&'static str>,
    pub(crate) github_error: Option<AccessError>,
    pub(crate) reachability: Reachability,
    pub(crate) succeeded: bool,
    pub(crate) issue_scope: Option<AccessIssueScope>,
    pub(crate) retry_at_ms: Option<i64>,
    #[cfg(feature = "e2e")]
    pub(crate) input_issue: Option<AccountInputIssue>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccessIssueScope {
    SignIn,
    Repository,
    Discovery,
}

#[cfg(feature = "e2e")]
#[derive(Clone, Copy, serde::Serialize)]
pub(crate) enum AccountInputIssue {
    InvalidRepository,
    InvalidPat,
}

enum Problem {
    Cli(CliError),
    CliAccountChanged,
    GitHub(AccessError),
    GitHubCheck(AccessError, Reachability, AccessIssueScope),
    Storage(threadline_storage::Error),
    Credential(PlatformError),
    InvalidRepository,
    InvalidPat,
    NoSavedCredential,
    Offline,
    InactiveProfile,
    Cancelled,
    ServerCooldown(i64),
}

enum Connection {
    Pat(Pat),
    GitHubCli {
        login: String,
        user_id: std::num::NonZeroU64,
    },
}

struct RepositoryCheck<'a> {
    route: &'a RepositoryName,
    prior: Option<(RepositoryId, RepositoryApiId)>,
}

enum RefreshTargets<'a> {
    One {
        repository: Option<&'a RepositoryView>,
        requested: Option<&'a str>,
        select: bool,
    },
    All,
}

enum RepositoryChecks<'a> {
    One {
        target: Option<RepositoryCheck<'a>>,
        select: bool,
    },
    All,
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
            let credentials: Arc<dyn CredentialStore + Send + Sync> =
                if std::env::var_os("THREADLINE_E2E_NATIVE_LIFECYCLE").is_some() {
                    Arc::new(crate::lifecycle_fixture::LifecycleVault::open()?)
                } else {
                    Arc::new(FixtureVault::default())
                };
            return Some(Self {
                store,
                github,
                credentials,
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

    pub(crate) fn reader_store(&self) -> Arc<Store> {
        self.store.clone()
    }

    pub(crate) fn initial_command(&self) -> AccountCommand {
        #[cfg(feature = "e2e")]
        if self.fixture
            && std::env::var_os("THREADLINE_E2E_NATIVE_LIFECYCLE").is_none()
            && std::env::var_os("THREADLINE_E2E_FIXTURE_AUTH").is_some_and(|value| value == "1")
        {
            return AccountCommand::LoadFixture;
        }
        AccountCommand::Load
    }

    pub(crate) fn sync_worker_enabled(&self) -> bool {
        !self.fixture
            || (cfg!(feature = "e2e")
                && std::env::var_os("THREADLINE_E2E_SYNC").is_some_and(|value| value == "1"))
    }

    pub(crate) async fn reader_session_with_guidance(
        &self,
        profile: &AccessProfile,
        cancel: &Cancellation,
    ) -> Result<(GitHubSession, threadline_core::github::RateLimit), crate::reader::ReaderError>
    {
        use crate::reader::ReaderError;

        self.saved_session(profile, cancel)
            .await
            .map_err(|problem| match problem {
                Problem::Cli(error) => ReaderError::GitHubCli(error),
                Problem::CliAccountChanged | Problem::InactiveProfile => ReaderError::ScopeChanged,
                Problem::GitHub(error) => ReaderError::Remote(error),
                Problem::Storage(error) => error.into(),
                Problem::Credential(error) => error.into(),
                Problem::Offline => ReaderError::Offline,
                Problem::Cancelled => ReaderError::Cancelled,
                _ => ReaderError::CredentialUnavailable,
            })
    }

    async fn cli_token(&self, login: &str, cancel: &Cancellation) -> Result<SecretToken, Problem> {
        check_cancel(cancel)?;
        if self.fixture {
            #[cfg(test)]
            if login.eq_ignore_ascii_case("fixture-user") {
                return Ok(SecretToken::fixture());
            }
            return Err(Problem::Cli(CliError::SignedOut));
        }
        let cli = GitHubCli::discover().map_err(Problem::Cli)?;
        cli.token_for(login, cancel)
            .await
            .map_err(|error| match error {
                CliError::Cancelled => Problem::Cancelled,
                error => Problem::Cli(error),
            })
    }

    fn require_online(&self, account: AccountId) -> Result<(), Problem> {
        let state = self.store.dispatch_profile(account)?;
        if !state.active {
            return Err(Problem::InactiveProfile);
        }
        if state.offline {
            return Err(Problem::Offline);
        }
        Ok(())
    }

    async fn saved_session(
        &self,
        profile: &AccessProfile,
        cancel: &Cancellation,
    ) -> Result<(GitHubSession, RateLimit), Problem> {
        check_cancel(cancel)?;
        self.require_online(profile.account)?;
        let cli = profile.kind == CredentialKind::GitHubCli;
        let token = if cli {
            self.store.with_installed_credential(profile, || ())?;
            let token = self.cli_token(&profile.login, cancel).await?;
            self.store.with_installed_credential(profile, || ())?;
            self.require_online(profile.account)?;
            token
        } else {
            let reference = credential_ref(profile)?;
            let saved = self
                .store
                .with_installed_credential(profile, || self.credentials.read(&reference))??
                .ok_or(Problem::NoSavedCredential)?;
            let pat = Pat(saved);
            pat.validated_kind(self.fixture)?;
            pat.token()?
        };
        let owner = OwnerScope::new(&profile.owner_scope)?;
        let result = self
            .github
            .authenticate_with_guidance(token, owner, Some(profile.user_id), cancel)
            .await?;
        if cli {
            if !result.0.user().login.eq_ignore_ascii_case(&profile.login) {
                return Err(Problem::CliAccountChanged);
            }
            check_cancel(cancel)?;
            self.store.with_installed_credential(profile, || ())?;
            self.require_online(profile.account)?;
        }
        Ok(result)
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
                succeeded: true,
                issue_scope: None,
                retry_at_ms: None,
                #[cfg(feature = "e2e")]
                input_issue: None,
            },
            Err(error) => problem_reply(view, error),
        }
    }

    fn view(&self) -> Result<AccountView, Problem> {
        let active = self.store.active_github_account()?;
        let profiles = self.store.github_access_profiles()?;
        let (repositories, controls) = match active {
            Some(account) => (
                self.store.github_repository_views(account)?,
                self.store.sync_settings(account)?.controls,
            ),
            None => (Vec::new(), Default::default()),
        };
        Ok(AccountView {
            active,
            profiles,
            repositories,
            offline: controls.work_offline,
            background_updates_allowed: Some(!controls.work_offline && !controls.pause_background),
        })
    }

    async fn run(
        &self,
        command: AccountCommand,
        cancel: &Cancellation,
    ) -> Result<(Option<&'static str>, Reachability), Problem> {
        match command {
            AccountCommand::Load => Ok((None, Reachability::NotAttempted)),
            #[cfg(feature = "e2e")]
            AccountCommand::LoadFixture => self.seed_fixture_access(cancel).await,
            AccountCommand::Connect {
                repository,
                pat,
                renew,
            } => {
                self.connect(&repository, Connection::Pat(pat), renew.as_ref(), cancel)
                    .await
            }
            AccountCommand::ConnectGitHubCli {
                repository,
                login,
                user_id,
                renew,
            } => {
                self.connect(
                    &repository,
                    Connection::GitHubCli { login, user_id },
                    renew.as_ref(),
                    cancel,
                )
                .await
            }
            AccountCommand::Refresh {
                profile,
                repository,
                requested,
            } => {
                self.refresh(
                    &profile,
                    RefreshTargets::One {
                        repository: repository.as_ref(),
                        requested: requested.as_deref(),
                        select: false,
                    },
                    cancel,
                )
                .await
            }
            AccountCommand::RefreshAll(account) => self.refresh_all(account, cancel).await,
            AccountCommand::Select {
                profile,
                repository,
            } => {
                self.refresh(
                    &profile,
                    RefreshTargets::One {
                        repository: Some(&repository),
                        requested: None,
                        select: true,
                    },
                    cancel,
                )
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
                self.store.update_sync_settings(account, |settings| {
                    settings.controls.work_offline = offline
                })?;
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
        connection: Connection,
        renew: Option<&AccessProfile>,
        cancel: &Cancellation,
    ) -> Result<(Option<&'static str>, Reachability), Problem> {
        let route = parse_repository(repository).ok_or(Problem::InvalidRepository)?;
        check_cancel(cancel)?;
        let active = self.store.active_github_account()?;
        if let Some(account) = active {
            self.require_online(account)?;
        }
        if let Some(profile) = renew {
            self.require_active(profile.account)?;
            if !profile.owner_scope.eq_ignore_ascii_case(route.owner()) {
                return Err(Problem::InvalidRepository);
            }
        }
        let owner = OwnerScope::new(route.owner())?;
        let (kind, token, expected) = match &connection {
            Connection::Pat(pat) => (
                pat.validated_kind(self.fixture)?,
                pat.token()?,
                renew.map(|profile| profile.user_id),
            ),
            Connection::GitHubCli { login, user_id } => {
                if renew.is_some_and(|profile| profile.user_id != *user_id) {
                    return Err(Problem::CliAccountChanged);
                }
                let token = self.cli_token(login, cancel).await?;
                if let Some(account) = active {
                    self.require_online(account)?;
                }
                (CredentialKind::GitHubCli, token, Some(*user_id))
            }
        };
        let session = self
            .github
            .authenticate(token, owner, expected, cancel)
            .await?;
        check_cancel(cancel)?;
        if self.store.active_github_account()? != active {
            return Err(Problem::InactiveProfile);
        }
        if let Some(account) = active {
            self.require_online(account)?;
        }
        let user = session.user();
        if let Connection::GitHubCli { login, .. } = &connection
            && !user.login.eq_ignore_ascii_case(login)
        {
            return Err(Problem::CliAccountChanged);
        }
        let account = self.store.ensure_account("github.com", user.id.get())?;
        let known = self.known_repository(account, &route)?;
        check_cancel(cancel)?;
        let profile = match connection {
            Connection::GitHubCli { .. } => self.store.link_github_cli(
                account,
                user.id,
                &user.login,
                session.owner().as_str(),
                renew,
                |old| {
                    self.credentials
                        .remove(&credential_ref(old)?)
                        .map_err(Problem::Credential)
                },
            )??,
            Connection::Pat(pat) => {
                let profile = self.store.reserve_access_profile_credential(
                    account,
                    user.id,
                    &user.login,
                    session.owner().as_str(),
                    kind,
                    renew,
                )?;
                let reference = credential_ref(&profile)?;
                if let Err(error) = self.store.install_current_credential(&profile, || {
                    self.credentials.save(&reference, &pat.0)
                })? {
                    self.store.record_access_result(
                        &profile,
                        kind,
                        AccessState::CredentialUnavailable,
                        DiscoveryState::Unchecked,
                        now_ms(),
                    )?;
                    return Err(error.into());
                }
                profile
            }
        };
        self.store.activate_local_profile(account)?;
        self.check_session(
            &profile,
            kind,
            &session,
            RepositoryChecks::One {
                target: Some(RepositoryCheck {
                    route: &route,
                    prior: known,
                }),
                select: false,
            },
            cancel,
        )
        .await
    }

    async fn refresh(
        &self,
        previous: &AccessProfile,
        targets: RefreshTargets<'_>,
        cancel: &Cancellation,
    ) -> Result<(Option<&'static str>, Reachability), Problem> {
        self.require_active(previous.account)?;
        if self.store.dispatch_profile(previous.account)?.offline {
            return Err(Problem::Offline);
        }
        let sweep_account = matches!(targets, RefreshTargets::All).then_some(previous.account);
        self.sweep_budget(sweep_account)?;
        let current = self
            .store
            .github_access_profile(previous.account, &previous.owner_scope)?
            .ok_or(Problem::InactiveProfile)?;
        if current.access == AccessState::SignedOut {
            return Err(Problem::NoSavedCredential);
        }
        let profile = self.store.begin_access_check(&current)?;
        let kind = profile.kind;
        let session = match self.saved_session(&profile, cancel).await {
            Ok((session, rate)) => {
                self.note_sweep_rate(sweep_account, Some(rate), None)?;
                session
            }
            Err(Problem::GitHub(error)) => {
                self.note_sweep_rate(sweep_account, error.rate, Some(error.kind))?;
                if error.kind != AccessKind::Cancelled {
                    self.store.record_access_result(
                        &profile,
                        profile.kind,
                        if sweep_account.is_some() && error.kind == AccessKind::RateLimited {
                            current.access
                        } else {
                            access_state(&error)
                        },
                        DiscoveryState::Unchecked,
                        now_ms(),
                    )?;
                }
                return Err(error.into());
            }
            Err(
                error @ (Problem::Cli(_)
                | Problem::NoSavedCredential
                | Problem::Credential(_)
                | Problem::CliAccountChanged),
            ) => {
                self.store.record_access_result(
                    &profile,
                    kind,
                    AccessState::CredentialUnavailable,
                    DiscoveryState::Unchecked,
                    now_ms(),
                )?;
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        let RefreshTargets::One {
            repository,
            requested,
            select,
        } = targets
        else {
            return self
                .check_session(&profile, kind, &session, RepositoryChecks::All, cancel)
                .await;
        };
        let typed_route = repository
            .map(|repo| RepositoryName::new(&repo.owner, &repo.name))
            .transpose()
            .map_err(|_| Problem::InvalidRepository)?;
        let requested_route = if repository.is_none() {
            requested
                .map(|raw| parse_repository(raw).ok_or(Problem::InvalidRepository))
                .transpose()?
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
                });
        let checked = match (listed_target, requested_route.as_ref()) {
            (Some(target), _) => Some(target),
            (None, Some(route)) => Some(RepositoryCheck {
                route,
                prior: self.known_repository(profile.account, route)?,
            }),
            (None, None) => None,
        };
        self.check_session(
            &profile,
            kind,
            &session,
            RepositoryChecks::One {
                target: checked,
                select,
            },
            cancel,
        )
        .await
    }

    async fn refresh_all(
        &self,
        account: AccountId,
        cancel: &Cancellation,
    ) -> Result<(Option<&'static str>, Reachability), Problem> {
        self.require_active(account)?;
        if self.store.dispatch_profile(account)?.offline {
            return Err(Problem::Offline);
        }
        self.sweep_budget(Some(account))?;
        let mut issue = None;
        let mut reachability = Reachability::NotAttempted;
        for profile in self
            .store
            .github_access_profiles()?
            .into_iter()
            .filter(|profile| {
                profile.account == account && profile.access != AccessState::SignedOut
            })
        {
            check_cancel(cancel)?;
            self.sweep_budget(Some(account))?;
            match self.refresh(&profile, RefreshTargets::All, cancel).await {
                Ok((_, result)) => reachability = result,
                Err(error) if issue.is_none() => issue = Some(error),
                Err(_) => {}
            }
        }
        check_cancel(cancel)?;
        self.sweep_budget(Some(account))?;
        if let Some(issue) = issue {
            return Err(issue);
        }
        if reachability == Reachability::NotAttempted {
            return Err(Problem::NoSavedCredential);
        }
        Ok((Some("GitHub repositories refreshed."), reachability))
    }

    async fn check_session(
        &self,
        profile: &AccessProfile,
        kind: CredentialKind,
        session: &GitHubSession,
        checks: RepositoryChecks<'_>,
        cancel: &Cancellation,
    ) -> Result<(Option<&'static str>, Reachability), Problem> {
        let mut issue = None;
        let mut reachability = Reachability::ReceivedResponse;
        let mut access = AccessState::Unchecked;
        let mut checked_repo = None;
        let (target, select, sweep) = match checks {
            RepositoryChecks::One { target, select } => (target, select, false),
            RepositoryChecks::All => (None, false, true),
        };
        let sweep_account = sweep.then_some(profile.account);
        if let Some(target) = target.as_ref() {
            match self
                .check_repository(profile, session, target, true, None, cancel)
                .await?
            {
                Ok(repository) => {
                    checked_repo = Some(repository);
                    access = AccessState::Readable;
                }
                Err(error) => {
                    reachability = error.reachability;
                    access = access_state(&error);
                    issue = Some((error, AccessIssueScope::Repository));
                }
            }
        }

        let mut discovery = DiscoveryState::Partial;
        let mut restricted = false;
        let mut listed = std::collections::HashSet::new();
        let mut cursor = None;
        let mut deferred = None;
        for _ in 0..100 {
            if let Err(error) = self.sweep_budget(sweep_account) {
                deferred = Some(error);
                break;
            }
            match session.discover(cursor.as_ref(), cancel).await {
                Ok(page) => {
                    reachability = Reachability::ReceivedResponse;
                    self.note_sweep_rate(sweep_account, Some(page.rate), None)?;
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
                        let id =
                            self.observe(profile, &repo, RepositoryObservationSource::Discovery)?;
                        if sweep {
                            listed.insert(id);
                        }
                    }
                    restricted |= page.sso_restricted_organizations.is_some();
                    discovery = if restricted {
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
                    self.note_sweep_rate(sweep_account, error.rate, Some(error.kind))?;
                    check_cancel(cancel)?;
                    if issue.is_none() {
                        issue = Some((error, AccessIssueScope::Discovery));
                    }
                    break;
                }
            }
        }
        check_cancel(cancel)?;
        let repositories = self.store.github_repository_views(profile.account)?;
        let scoped = repositories
            .iter()
            .filter(|repo| repo.owner.eq_ignore_ascii_case(&profile.owner_scope));
        let mut readable = false;
        for repo in scoped {
            let listed = listed.contains(&repo.id);
            if sweep
                && (repo.selected
                    || (listed && repo.access == AccessState::NotFoundOrDenied)
                    || (discovery == DiscoveryState::Complete && !listed))
            {
                if deferred.is_some() {
                    readable |= repo.access == AccessState::Readable && repo.verified_current;
                    continue;
                }
                if let Err(error) = self.sweep_budget(sweep_account) {
                    deferred = Some(error);
                    readable |= repo.access == AccessState::Readable && repo.verified_current;
                    continue;
                }
                let route = RepositoryName::new(&repo.owner, &repo.name)
                    .map_err(|_| Problem::InvalidRepository)?;
                let check = RepositoryCheck {
                    route: &route,
                    prior: Some((repo.id, RepositoryApiId::new(repo.provider_id))),
                };
                match self
                    .check_repository(
                        profile,
                        session,
                        &check,
                        repo.selected,
                        sweep_account,
                        cancel,
                    )
                    .await?
                {
                    Ok(_) => readable = true,
                    Err(error) if repo.selected => {
                        if !readable {
                            access = access_state(&error);
                        }
                        if issue.is_none() {
                            issue = Some((error, AccessIssueScope::Repository));
                        }
                    }
                    Err(_) => {}
                }
            } else if repo.access == AccessState::Readable && repo.verified_current {
                readable = true;
            }
        }
        if readable {
            access = AccessState::Readable;
        }
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
        if let Some(error) = deferred {
            return Err(error);
        }
        if let Some((error, scope)) = issue {
            return Err(Problem::GitHubCheck(error, reachability, scope));
        }
        let notice = match (checked_repo, select) {
            (Some(_), true) => Some("Repository selected. Read access verified."),
            (Some(_), false) => Some("Read access verified. Select a repository to monitor it."),
            _ => Some("Repository list refreshed."),
        };
        Ok((notice, reachability))
    }

    async fn check_repository(
        &self,
        profile: &AccessProfile,
        session: &GitHubSession,
        target: &RepositoryCheck<'_>,
        record_failure: bool,
        sweep_account: Option<AccountId>,
        cancel: &Cancellation,
    ) -> Result<Result<RepositoryId, AccessError>, Problem> {
        let expected = target.prior.map(|(_, provider_id)| provider_id);
        let (result, rate) = match session
            .repository_with_guidance(target.route, expected, None, cancel)
            .await
        {
            Ok((RepositoryRead::Fresh { repository, .. }, rate)) => (Ok(repository), Some(rate)),
            Ok((RepositoryRead::NotModified, rate)) => (
                Err(AccessError {
                    kind: AccessKind::UnexpectedStatus,
                    reachability: Reachability::ReceivedResponse,
                    http_status: Some(304),
                    sso_action: None,
                    rate: Some(rate),
                }),
                Some(rate),
            ),
            Err(error) => {
                let rate = error.rate;
                (Err(error), rate)
            }
        };
        self.note_sweep_rate(
            sweep_account,
            rate,
            result.as_ref().err().map(|error| error.kind),
        )?;
        check_cancel(cancel)?;
        match result {
            Ok(repository) => self
                .observe(profile, &repository, RepositoryObservationSource::Direct)
                .map(Ok),
            Err(error) => {
                if let Some((id, _)) = target.prior
                    && (record_failure
                        || (error.kind == AccessKind::MissingOrInaccessible
                            && error.http_status == Some(404)))
                {
                    self.store.mark_repository_access(
                        profile,
                        id,
                        access_state(&error),
                        now_ms(),
                    )?;
                }
                Ok(Err(error))
            }
        }
    }

    fn sweep_budget(&self, account: Option<AccountId>) -> Result<(), Problem> {
        if let Some(account) = account
            && let Some(until) = self.store.server_read_budget(account)?
            && until > now_ms()
        {
            return Err(Problem::ServerCooldown(until));
        }
        Ok(())
    }

    fn note_sweep_rate(
        &self,
        account: Option<AccountId>,
        rate: Option<RateLimit>,
        reason: Option<AccessKind>,
    ) -> Result<(), Problem> {
        if let Some(account) = account
            && let Some(until) = crate::sync_worker::floor(rate, reason, now_ms())
        {
            self.store.hold_sync_budget(account, until)?;
        }
        Ok(())
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
        if profile.kind == CredentialKind::GitHubCli {
            return Ok(());
        }
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

pub(crate) const REPOSITORY_FORMAT: &str =
    "Enter a repository as owner/repository, for example octocat/hello-world.";

pub(crate) fn parse_repository(raw: &str) -> Option<RepositoryName> {
    let (owner, name) = raw.trim().split_once('/')?;
    RepositoryName::new(owner, name).ok()
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
    if let Problem::GitHubCheck(github_error, reachability, scope) = error {
        return AccountReply {
            view,
            message: None,
            reachability,
            github_error: Some(github_error),
            succeeded: false,
            issue_scope: Some(scope),
            retry_at_ms: None,
            #[cfg(feature = "e2e")]
            input_issue: None,
        };
    }
    if let Problem::GitHub(github_error) = error {
        return AccountReply {
            view,
            message: None,
            reachability: github_error.reachability,
            github_error: Some(github_error),
            succeeded: false,
            issue_scope: Some(AccessIssueScope::SignIn),
            retry_at_ms: None,
            #[cfg(feature = "e2e")]
            input_issue: None,
        };
    }
    #[cfg(feature = "e2e")]
    let input_issue = match &error {
        Problem::InvalidRepository => Some(AccountInputIssue::InvalidRepository),
        Problem::InvalidPat => Some(AccountInputIssue::InvalidPat),
        _ => None,
    };
    let retry_at_ms = if let Problem::ServerCooldown(until) = &error {
        Some(*until)
    } else {
        None
    };
    let message = match error {
        Problem::Cli(error) => error.message(),
        Problem::CliAccountChanged => {
            "The GitHub CLI account changed. Choose the account again in Settings."
        }
        Problem::Storage(error) => {
            let _ = error;
            "The account change could not be saved."
        }
        Problem::Credential(error) => {
            let _ = error;
            "Protected credential storage is unavailable or locked. Local content is still available."
        }
        Problem::InvalidRepository => REPOSITORY_FORMAT,
        Problem::InvalidPat => "Enter a valid personal access token.",
        Problem::NoSavedCredential => {
            "No saved token is available. Enter a token for this account."
        }
        Problem::Offline => "Connection is paused. Go online before checking GitHub.",
        Problem::InactiveProfile => "The active account changed. Try the account action again.",
        Problem::Cancelled => "GitHub check cancelled.",
        Problem::ServerCooldown(_) => "GitHub rate-limited repository refresh.",
        Problem::GitHub(_) | Problem::GitHubCheck(_, _, _) => unreachable!(),
    };
    AccountReply {
        view,
        message: Some(message),
        github_error: None,
        reachability: Reachability::NotAttempted,
        succeeded: false,
        issue_scope: None,
        retry_at_ms,
        #[cfg(feature = "e2e")]
        input_issue,
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
        if value.0 != FIXTURE_PAT.as_bytes() {
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

#[cfg(test)]
#[path = "write_tests.rs"]
mod write_tests;
