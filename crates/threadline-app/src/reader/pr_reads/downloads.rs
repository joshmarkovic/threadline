use threadline_storage::pr::{PrDownloadClaim, PrDownloadState};

use super::*;

impl ReaderService {
    pub(crate) async fn run_claimed_pr_download(
        &self,
        claim: &PrDownloadClaim,
        cancel: &Cancellation,
        publish_progress: impl Fn(),
    ) -> Result<PrDownloadState, ReaderError> {
        let rev = &claim.revision;
        let attempt = claim.attempt;
        self.check_pr_download(rev, Some(attempt))?;
        let mut prepared = self.prepare_pr(rev.account, rev.pr, cancel).await?;
        if prepared.revision.id != rev.id
            || prepared.revision.head != rev.head
            || prepared.revision.base != rev.base
        {
            return Err(ReaderError::ScopeChanged);
        }

        // Bound each run. An interrupted or continued walk keeps all completed pages.
        let mut terminal_families = std::collections::HashSet::new();
        for _ in 0..100 {
            if cancel.is_cancelled() {
                return Err(ReaderError::Cancelled);
            }
            self.check_pr_download(&prepared.revision, Some(attempt))?;
            let current = self
                .store
                .pr_download(&prepared.revision)?
                .ok_or(ReaderError::ScopeChanged)?;
            if current.conversation.coverage != Coverage::Complete {
                let page = self
                    .fetch_comments_for(rev.account, rev.pr, Some((rev, attempt)), cancel)
                    .await?;
                if page.contacted {
                    publish_progress();
                }
                if page.has_more {
                    continue;
                }
                let refreshed = self
                    .store
                    .pr_download(&prepared.revision)?
                    .ok_or(ReaderError::ScopeChanged)?;
                if refreshed.conversation.coverage != Coverage::Complete {
                    break;
                }
            }
            let mut advanced = false;
            for (family, state) in current.families {
                if matches!(family, PrFamily::Context | PrFamily::Media)
                    || terminal_families.contains(family.name())
                {
                    // These families need an explicit file range or media target.
                    continue;
                }
                if state.coverage != Coverage::Complete || family == PrFamily::Threads {
                    let page = self
                        .read_pr_page(&mut prepared, family, Some(attempt), cancel)
                        .await?;
                    if page.contacted {
                        publish_progress();
                    }
                    advanced |= page.has_more;
                    if !page.has_more {
                        terminal_families.insert(family.name());
                    }
                }
            }
            if !advanced {
                break;
            }
        }
        Ok(self.store.finish_pr_download_attempt(
            &prepared.revision,
            attempt,
            UtcMillis::now()?.0,
        )?)
    }
}
