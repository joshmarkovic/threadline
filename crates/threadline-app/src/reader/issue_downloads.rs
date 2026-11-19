use super::*;

impl ReaderService {
    /// Process one missing resource or discussion page. A later claim owns any continuation.
    pub(crate) async fn run_issue_download_step(
        &self,
        claim: &IssueDownloadClaim,
        cancel: &Cancellation,
    ) -> Result<bool, ReaderError> {
        let (scope, number) = self.item_scope(claim.account, claim.locator, cancel)?;
        let read = || {
            self.store
                .issue_download_step(claim, &scope.profile, UtcMillis::now()?.0)
        };
        let job = read()?;
        let mut contacted = false;
        if job.metadata.coverage != Coverage::Complete || job.body.coverage != Coverage::Complete {
            let binding = self.binding(&scope, cancel).await?;
            self.admit(claim.account, cancel).await?;
            read()?;
            let summary = self
                .response(claim.account, binding.reader().issue(number, cancel).await)
                .await?;
            self.check_item(&scope, claim.item, claim.locator, cancel)?;
            self.store.commit_issue_download_metadata(
                claim,
                &scope.profile,
                issue(summary)?,
                UtcMillis::now()?.0,
            )?;
            let updated = read()?;
            if updated.metadata.coverage != Coverage::Complete
                || updated.body.coverage != Coverage::Complete
            {
                return Err(ReaderError::IncompleteIssueDownload);
            }
            contacted = true;
        } else if job.comments.coverage != Coverage::Complete {
            let page = self
                .fetch_comments_for(
                    claim.account,
                    claim.item,
                    Some(DiscussionDownload::Issue(claim)),
                    cancel,
                )
                .await?;
            if !page.complete && !page.has_more {
                return Err(ReaderError::IncompleteAccess);
            }
            contacted = page.contacted;
        }
        if cancel.is_cancelled() {
            return Err(ReaderError::Cancelled);
        }
        self.store
            .complete_issue_download_step(claim, &scope.profile, UtcMillis::now()?.0)?;
        Ok(contacted)
    }
}
