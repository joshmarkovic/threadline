use super::*;
use std::num::NonZeroU64;

const OWNER: &str = "fixture-org";
const REPO: &str = "private";
const USER: u64 = 41;
const REPOSITORY: u64 = 71;

impl AccountService {
    /// Authenticate the read/write fixture once, using ordinary provider and profile checks.
    /// A separate-process relaunch restores only this fixture's in-memory credential.
    pub(super) async fn seed_fixture_access(
        &self,
        cancel: &Cancellation,
    ) -> Result<(Option<&'static str>, Reachability), Problem> {
        if !self.fixture {
            return Err(Problem::InactiveProfile);
        }
        check_cancel(cancel)?;
        let credential = Pat::from_input(FIXTURE_PAT.to_owned());
        let previous = if let Some(account) = self.store.active_github_account()? {
            let profile = self
                .store
                .github_access_profile(account, OWNER)?
                .filter(|profile| profile.user_id.get() == USER && profile.login == "fixture-user")
                .ok_or(Problem::InactiveProfile)?;
            let repositories = self.store.github_repository_views(account)?;
            let large_workspace = repositories.len() == 2
                && repositories.iter().any(|row| {
                    row.provider_id.get() == threadline_storage::fixtures::BACKLOG_ARCHIVE
                        && row.owner == OWNER
                        && row.name == "archive"
                        && row.private == Some(true)
                })
                && self.store.dispatch_profile(account)?.offline
                && threadline_storage::fixtures::has_backlog_marker(&self.store, account)?;
            if (repositories.len() != 1 && !large_workspace)
                || !repositories.iter().any(|row| {
                    row.provider_id.get() == REPOSITORY
                        && row.owner == OWNER
                        && row.name == REPO
                        && row.private == Some(true)
                        && row.selected
                })
            {
                return Err(Problem::InactiveProfile);
            }
            match profile.access {
                AccessState::Readable => {
                    let reference = credential_ref(&profile)?;
                    self.store.with_current_credential(&profile, || {
                        self.credentials.save(&reference, &credential.0)
                    })??;
                    return Ok((None, Reachability::NotAttempted));
                }
                AccessState::SignedOut if large_workspace => Some(profile),
                _ => return Err(Problem::InactiveProfile),
            }
        } else {
            if !self.store.github_access_profiles()?.is_empty() {
                return Err(Problem::InactiveProfile);
            }
            None
        };

        let kind = credential.validated_kind(true)?;
        let owner = OwnerScope::new(OWNER)?;
        let route = RepositoryName::new(OWNER, REPO).map_err(|_| Problem::InvalidRepository)?;
        let session = self
            .github
            .authenticate(credential.token()?, owner, NonZeroU64::new(USER), cancel)
            .await?;
        let user = session.user();
        if user.login != "fixture-user" {
            return Err(Problem::InactiveProfile);
        }
        let repository = session
            .verify_private_repository(
                &route,
                Some(REPOSITORY.try_into().expect("fixture ID")),
                cancel,
            )
            .await?;
        check_cancel(cancel)?;
        let account = self.store.ensure_account("github.com", USER)?;
        let profile = self.store.reserve_access_profile_credential(
            account,
            user.id,
            &user.login,
            OWNER,
            kind,
            previous.as_ref(),
        )?;
        let reference = credential_ref(&profile)?;
        self.store.install_current_credential(&profile, || {
            self.credentials.save(&reference, &credential.0)
        })??;
        let repo = self.observe(&profile, &repository, RepositoryObservationSource::Direct)?;
        if !self.store.record_access_result(
            &profile,
            kind,
            AccessState::Readable,
            DiscoveryState::Unchecked,
            now_ms(),
        )? {
            return Err(Problem::InactiveProfile);
        }
        self.store.activate_local_profile(account)?;
        self.store.select_github_repository(account, repo, true)?;
        Ok((None, Reachability::ReceivedResponse))
    }
}
