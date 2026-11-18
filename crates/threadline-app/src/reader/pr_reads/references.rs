use threadline_storage::{
    access::{RepositoryObservation, RepositoryObservationSource},
    pr::{BodyReference, ReferenceObservation},
};

use super::*;

impl ReaderService {
    pub(crate) async fn verify_pr_reference(
        &self,
        account: AccountId,
        pr: ItemId,
        body_revision: &str,
        position: i64,
        cancel: &Cancellation,
    ) -> Result<(), ReaderError> {
        let candidate = self.pr_candidate(account, pr, body_revision, position)?;
        let outcome = self.verify_target(account, &candidate, cancel).await;
        let observation = match &outcome {
            Ok(observed) => observed.clone(),
            Err(ReaderError::Cancelled | ReaderError::ScopeChanged) => return outcome.map(|_| ()),
            Err(ReaderError::Offline) => ReferenceObservation::Offline,
            Err(ReaderError::Remote(error))
                if matches!(
                    error.kind,
                    AccessKind::MissingOrInaccessible
                        | AccessKind::PermissionDenied
                        | AccessKind::SsoRequired
                        | AccessKind::ApprovalPending
                        | AccessKind::Unauthorized
                ) =>
            {
                ReferenceObservation::Inaccessible
            }
            Err(ReaderError::CredentialUnavailable | ReaderError::ProtectedStore(_)) => {
                ReferenceObservation::Inaccessible
            }
            Err(_) => ReferenceObservation::Failed,
        };
        self.store.record_pr_reference(
            account,
            pr,
            body_revision,
            position,
            observation,
            UtcMillis::now()?.0,
        )?;
        outcome.map(|_| ())
    }

    fn pr_candidate(
        &self,
        account: AccountId,
        pr: ItemId,
        body_revision: &str,
        position: i64,
    ) -> Result<BodyReference, ReaderError> {
        let (saved, rows) = self
            .store
            .pr_reference_set(account, pr)?
            .ok_or(ReaderError::ScopeChanged)?;
        if saved != body_revision {
            return Err(ReaderError::ScopeChanged);
        }
        rows.into_iter()
            .find(|candidate| candidate.position == position)
            .ok_or(ReaderError::ScopeChanged)
    }

    async fn verify_target(
        &self,
        account: AccountId,
        candidate: &BodyReference,
        cancel: &Cancellation,
    ) -> Result<ReferenceObservation, ReaderError> {
        let route = RepositoryName::new(&candidate.owner, &candidate.repository)
            .map_err(|_| ReaderError::InvalidIdentity)?;
        let number = candidate
            .number
            .try_into()
            .map_err(|_| ReaderError::InvalidIdentity)?;
        let profile = self
            .store
            .github_access_profile(account, route.owner())?
            .filter(|profile| profile.access == AccessState::Readable)
            .ok_or(ReaderError::CredentialUnavailable)?;
        self.check_reference_scope(&profile, cancel)?;
        self.admit(account, cancel).await?;
        let (session, rate) = self
            .accounts
            .reader_session_with_guidance(&profile, cancel)
            .await?;
        self.pr_rate(account, rate).await;
        let known = self
            .store
            .github_repository_views(account)?
            .into_iter()
            .find(|repo| {
                repo.owner.eq_ignore_ascii_case(route.owner())
                    && repo.name.eq_ignore_ascii_case(route.name())
            })
            .map(|repo| RepositoryApiId::new(repo.provider_id));
        self.admit(account, cancel).await?;
        let remote = self
            .response(account, session.repository(&route, known, cancel).await)
            .await?;
        self.check_reference_scope(&profile, cancel)?;
        let repository = self.store.observe_github_repository(
            &profile,
            &RepositoryObservation {
                provider_id: std::num::NonZeroU64::new(remote.id.get())
                    .expect("verified repository ID"),
                owner: remote.route.owner(),
                name: remote.route.name(),
                private: remote.private,
                source: RepositoryObservationSource::Direct,
            },
            UtcMillis::now()?.0,
        )?;
        self.admit(account, cancel).await?;
        let reader = self
            .response(account, session.reader(&route, remote.id, cancel).await)
            .await?;
        self.admit(account, cancel).await?;
        let summary = self
            .response(account, reader.issue(number, cancel).await)
            .await?;
        self.check_reference_scope(&profile, cancel)?;
        let id = summary.issue_id;
        let item = self.store.commit_item_metadata(
            account,
            repository,
            issue(summary)?,
            &Validators::default(),
            UtcMillis::now()?.0,
        )?;
        Ok(ReferenceObservation::Item {
            repository,
            item,
            issue_id: id,
        })
    }

    fn check_reference_scope(
        &self,
        profile: &AccessProfile,
        cancel: &Cancellation,
    ) -> Result<(), ReaderError> {
        if cancel.is_cancelled() {
            return Err(ReaderError::Cancelled);
        }
        if self.store.active_github_account()? != Some(profile.account)
            || self
                .store
                .github_access_profile(profile.account, &profile.owner_scope)?
                .as_ref()
                != Some(profile)
        {
            return Err(ReaderError::ScopeChanged);
        }
        if self.store.dispatch_profile(profile.account)?.offline {
            return Err(ReaderError::Offline);
        }
        Ok(())
    }
}
