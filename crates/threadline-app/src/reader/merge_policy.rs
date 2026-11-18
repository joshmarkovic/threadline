use super::*;
use threadline_storage::{merges::MergePolicy, pr::PrRevision};

impl ReaderService {
    pub(crate) async fn fetch_merge_policy(
        &self,
        revision: &PrRevision,
        cancel: &Cancellation,
    ) -> Result<MergePolicy, ReaderError> {
        let account = revision.account;
        let item = self
            .store
            .item(account, revision.pr)?
            .filter(|item| item.locator.repository == revision.repository)
            .ok_or(ReaderError::ScopeChanged)?;
        let pr_id = item
            .pull_request
            .as_ref()
            .ok_or(ReaderError::InvalidIdentity)?
            .id
            .get()
            .try_into()
            .map_err(|_| ReaderError::InvalidIdentity)?;
        let (scope, number) = self.item_scope(account, item.locator, cancel)?;
        self.admit(account, cancel).await?;
        let (session, rate) = self
            .accounts
            .reader_session_with_guidance(&scope.profile, cancel)
            .await?;
        self.merge_rate(account, rate).await;
        self.check_item(&scope, revision.pr, item.locator, cancel)?;
        self.admit(account, cancel).await?;
        let (writer, rate) = self
            .response(
                account,
                session
                    .issue_writer(&scope.route, scope.provider_id, cancel)
                    .await,
            )
            .await?;
        self.merge_rate(account, rate).await;
        self.check_item(&scope, revision.pr, item.locator, cancel)?;
        self.admit(account, cancel).await?;
        let (pull, rate) = self
            .response(
                account,
                writer.observe_pull_for_review(number, pr_id, cancel).await,
            )
            .await?;
        self.merge_rate(account, rate).await;
        if pull.base() != revision.base.as_str() || pull.head() != revision.head.as_str() {
            return Err(ReaderError::ScopeChanged);
        }
        self.check_item(&scope, revision.pr, item.locator, cancel)?;
        self.admit(account, cancel).await?;
        let (mut observed, rate) = self
            .response(account, writer.observe_merge_policy(&pull, cancel).await)
            .await?;
        self.merge_rate(account, rate).await;
        if observed.needs_optional_checks() {
            self.check_item(&scope, revision.pr, item.locator, cancel)?;
            self.admit(account, cancel).await?;
            let (proof, rate) = self
                .response(
                    account,
                    writer
                        .observe_optional_merge_checks(&pull, &observed, cancel)
                        .await,
                )
                .await?;
            self.merge_rate(account, rate).await;
            if let Some(proof) = proof {
                self.check_item(&scope, revision.pr, item.locator, cancel)?;
                self.admit(account, cancel).await?;
                let rate = self
                    .response(
                        account,
                        writer
                            .confirm_optional_merge_checks(&pull, &mut observed, proof, cancel)
                            .await,
                    )
                    .await?;
                self.merge_rate(account, rate).await;
            }
        }
        self.check_item(&scope, revision.pr, item.locator, cancel)?;
        if self
            .store
            .current_pr_revision(account, revision.pr)?
            .is_none_or(|(current, _)| current.id != revision.id)
        {
            return Err(ReaderError::ScopeChanged);
        }
        let policy = crate::sync_worker::translate_merge_policy(observed.eligibility());
        self.store
            .record_merge_policy(&scope.profile, revision, &policy, UtcMillis::now()?.0)?;
        Ok(policy)
    }

    async fn merge_rate(&self, account: AccountId, rate: threadline_core::github::RateLimit) {
        if self.accounts.sync_worker_enabled() {
            hold(&self.store, account, Some(rate), None).await;
        }
    }
}
