use threadline_storage::pr::PrDownloadState;

use super::*;

impl ReaderService {
    pub(crate) async fn run_pr_download(
        &self,
        rev: &PrRevision,
        generation: i64,
        cancel: &Cancellation,
    ) -> Result<PrDownloadState, ReaderError> {
        if self.store.dispatch_profile(rev.account)?.offline {
            return Err(ReaderError::Offline);
        }
        self.store.start_pr_download(rev, generation)?;
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
            let current = self
                .store
                .pr_download(&prepared.revision)?
                .ok_or(ReaderError::ScopeChanged)?;
            if cancel.is_cancelled()
                || current.generation != generation
                || current.state == PrDownloadState::Cancelled
            {
                return Err(ReaderError::Cancelled);
            }
            if current.conversation.coverage != Coverage::Complete {
                if self
                    .fetch_comments(rev.account, rev.pr, cancel)
                    .await?
                    .has_more
                {
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
                    continue;
                }
                if state.coverage != Coverage::Complete || family == PrFamily::Threads {
                    let page = self
                        .read_pr_page(&mut prepared, family, Some(generation), cancel)
                        .await?;
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
        Ok(self
            .store
            .finish_pr_download(&prepared.revision, generation)?)
    }
}
