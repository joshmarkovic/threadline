use super::*;

use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
};

use threadline_core::github::writer::{
    ChoiceValues, GitHubWriter, IssueChoices, IssueWriteTarget, IssueWriteValues,
};
use threadline_storage::writes::{
    CachedRepositoryChoices, ChoiceList, IssueValues, MilestoneChoice,
};

impl ReaderService {
    pub(crate) async fn fetch_write_choices(
        &self,
        account: AccountId,
        item: ItemId,
        cancel: &Cancellation,
    ) -> Result<CachedRepositoryChoices, ReaderError> {
        let saved = self
            .store
            .item(account, item)?
            .filter(|item| item.kind == ItemKind::Issue)
            .ok_or(ReaderError::InvalidIdentity)?;
        let (scope, number) = self.item_scope(account, saved.locator, cancel)?;
        let issue_id = saved
            .issue_id
            .ok_or(ReaderError::InvalidIdentity)?
            .get()
            .try_into()
            .map_err(|_| ReaderError::InvalidIdentity)?;
        self.admit(account, cancel).await?;
        let (session, rate) = self
            .accounts
            .reader_session_with_guidance(&scope.profile, cancel)
            .await?;
        self.hold_choice_rate(account, rate).await;
        self.check_item(&scope, item, saved.locator, cancel)?;
        self.admit(account, cancel).await?;
        let (writer, rate) = self
            .response(
                account,
                session
                    .issue_writer(&scope.route, scope.provider_id, cancel)
                    .await,
            )
            .await?;
        self.hold_choice_rate(account, rate).await;
        self.admit(account, cancel).await?;
        let (issue, rate) = self
            .response(
                account,
                writer
                    .observe_issue(
                        IssueWriteTarget {
                            number,
                            issue_id,
                            pull_request: false,
                        },
                        cancel,
                    )
                    .await,
            )
            .await?;
        self.hold_choice_rate(account, rate).await;
        self.check_item(&scope, item, saved.locator, cancel)?;
        self.store.record_issue_write_base(
            &scope.profile,
            scope.repository,
            item,
            issue_id,
            number
                .get()
                .try_into()
                .map_err(|_| ReaderError::InvalidIdentity)?,
            &values(&issue.observed),
            issue.milestone_title.as_deref(),
            issue.updated_at_ms,
        )?;
        let mut first_error = None;
        for kind in [
            IssueChoices::Labels,
            IssueChoices::Assignees,
            IssueChoices::Milestones,
        ] {
            match self.read_choices(&scope, &writer, kind, cancel).await {
                Ok(choices) => {
                    self.check_item(&scope, item, saved.locator, cancel)?;
                    self.store
                        .record_write_choices(&scope.profile, scope.repository, &choices)?;
                }
                Err(ReaderError::Cancelled) => return Err(ReaderError::Cancelled),
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        let saved = self.store.write_choices(account, scope.repository)?;
        if saved == CachedRepositoryChoices::default()
            && let Some(error) = first_error
        {
            return Err(error);
        }
        Ok(saved)
    }

    async fn hold_choice_rate(&self, account: AccountId, rate: threadline_core::github::RateLimit) {
        if self.accounts.sync_worker_enabled() {
            hold(&self.store, account, Some(rate), None).await;
        }
    }

    async fn read_choices(
        &self,
        scope: &Scope,
        writer: &GitHubWriter<'_>,
        kind: IssueChoices,
        cancel: &Cancellation,
    ) -> Result<CachedRepositoryChoices, ReaderError> {
        let mut page = NonZeroU32::MIN;
        let mut sso_restricted = false;
        let mut complete = false;
        let mut names = BTreeSet::new();
        let mut milestones = BTreeMap::new();
        let at = UtcMillis::now()?.0;
        for _ in 0..5 {
            self.check_scope(scope, cancel)?;
            self.admit(scope.account, cancel).await?;
            let result = self
                .response(
                    scope.account,
                    writer.issue_choices(kind, page, cancel).await,
                )
                .await?;
            self.hold_choice_rate(scope.account, result.rate).await;
            sso_restricted |= result.sso_limited;
            match result.values {
                ChoiceValues::Labels(values) | ChoiceValues::Assignees(values) => {
                    names.extend(values)
                }
                ChoiceValues::Milestones(values) => milestones.extend(values),
            }
            let Some(next) = result.next else {
                complete = !sso_restricted;
                break;
            };
            page = next;
        }
        let mut result = CachedRepositoryChoices::default();
        match kind {
            IssueChoices::Labels => {
                result.labels = Some(ChoiceList {
                    values: names.into_iter().collect(),
                    complete,
                    checked_at_ms: at,
                })
            }
            IssueChoices::Assignees => {
                result.assignees = Some(ChoiceList {
                    values: names.into_iter().collect(),
                    complete,
                    checked_at_ms: at,
                })
            }
            IssueChoices::Milestones => {
                result.milestones = Some(ChoiceList {
                    values: milestones
                        .into_iter()
                        .map(|(number, title)| MilestoneChoice { number, title })
                        .collect(),
                    complete,
                    checked_at_ms: at,
                })
            }
        }
        Ok(result)
    }
}

pub(crate) fn values(fields: &IssueWriteValues) -> IssueValues {
    IssueValues {
        title: fields.title.clone(),
        body: fields.body.clone(),
        labels: fields.labels.clone(),
        assignees: fields.assignees.clone(),
        milestone: fields.milestone,
        closed: fields.closed,
    }
}
