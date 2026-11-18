use super::*;
use threadline_core::github::reader::{
    history::{HistoryCommit, HistoryDiff, HistoryFiles, HistoryRefKind, HistoryTagObject},
    refresh::Refresh,
};
use threadline_storage::{
    history::{DiffSemantics, HistoryComparison, HistoryDiffInput, HistoryEntry, HistoryRef},
    resources::{CommitOid, GraphCommit},
};

impl ReaderService {
    pub(crate) async fn fetch_history_refs(
        &self,
        account: AccountId,
        repository: RepositoryId,
        cancel: &Cancellation,
    ) -> Result<PageStatus, ReaderError> {
        let scope = self.scope(account, repository, cancel)?;
        let bound = self.binding(&scope, cancel).await?;
        let reader = bound.reader();
        let mut complete = true;
        for kind in [HistoryRefKind::Branch, HistoryRefKind::Tag] {
            let mut page = 1;
            // Large ref inventories stay bounded even when GitHub continues the listing.
            loop {
                self.check_scope(&scope, cancel)?;
                self.admit(account, cancel).await?;
                let reply = self
                    .history_value(account, reader.history_refs(kind, page, cancel).await)
                    .await?;
                self.check_scope(&scope, cancel)?;
                let at = UtcMillis::now()?.0;
                for row in reply.records {
                    let reference = match kind {
                        HistoryRefKind::Branch => HistoryRef::Branch(row.name),
                        HistoryRefKind::Tag => HistoryRef::Tag(row.name),
                    };
                    let tip = CommitOid::parse(&row.oid)?;
                    self.store.observe_history_ref(
                        &scope.profile,
                        repository,
                        &reference,
                        Some(&tip),
                        at,
                    )?;
                }
                complete &= !reply.limited;
                match reply.next_page {
                    Some(next) if page < 4 && !reply.limited => page = next,
                    Some(_) => {
                        complete = false;
                        break;
                    }
                    None => break,
                }
            }
        }
        Ok(PageStatus {
            has_more: false,
            complete,
            contacted: true,
        })
    }

    pub(crate) async fn fetch_history_page(
        &self,
        account: AccountId,
        repository: RepositoryId,
        reference: &HistoryRef,
        cancel: &Cancellation,
    ) -> Result<PageStatus, ReaderError> {
        reference.key()?;
        let scope = self.scope(account, repository, cancel)?;
        let bound = self.binding(&scope, cancel).await?;
        let reader = bound.reader();
        self.check_scope(&scope, cancel)?;
        self.admit(account, cancel).await?;
        let oid = match reference {
            HistoryRef::Branch(name) => {
                self.history_value(account, reader.history_branch(name, cancel).await)
                    .await?
                    .oid
            }
            HistoryRef::Tag(name) => {
                let mut object = self
                    .history_value(account, reader.history_tag_ref(name, cancel).await)
                    .await?;
                let mut hops = 0;
                loop {
                    match object {
                        HistoryTagObject::Commit(oid) => break oid,
                        HistoryTagObject::Tag(tag) if hops < 8 => {
                            self.check_scope(&scope, cancel)?;
                            self.admit(account, cancel).await?;
                            object = self
                                .history_value(
                                    account,
                                    reader.history_annotated_tag(&tag, cancel).await,
                                )
                                .await?;
                            hops += 1;
                        }
                        HistoryTagObject::Tag(_) => return Err(ReaderError::SourceUnavailable),
                    }
                }
            }
            HistoryRef::FollowedPr(number) => {
                let item = self
                    .store
                    .resolve(
                        account,
                        ItemLocator {
                            repository,
                            number: number.get(),
                        },
                    )?
                    .ok_or(ReaderError::InvalidIdentity)?;
                let pr_id = self
                    .store
                    .item(account, item)?
                    .and_then(|row| row.pull_request)
                    .and_then(|pr| std::num::NonZeroU64::new(pr.id.get()))
                    .ok_or(ReaderError::InvalidIdentity)?;
                self.history_value(
                    account,
                    reader
                        .history_pull_tip(
                            number
                                .get()
                                .try_into()
                                .map_err(|_| ReaderError::InvalidIdentity)?,
                            pr_id,
                            cancel,
                        )
                        .await,
                )
                .await?
            }
        };
        let tip = CommitOid::parse(&oid)?;
        self.check_scope(&scope, cancel)?;
        self.store.observe_history_ref(
            &scope.profile,
            repository,
            reference,
            Some(&tip),
            UtcMillis::now()?.0,
        )?;
        let Some(walk) = self
            .store
            .begin_history_page(&scope.profile, repository, reference)?
        else {
            return Ok(PageStatus {
                has_more: false,
                complete: true,
                contacted: true,
            });
        };
        if walk.tip != tip {
            return Err(ReaderError::ScopeChanged);
        }
        let page = history_page_number(walk.cursor.as_deref())?;
        self.admit(account, cancel).await?;
        let history = self
            .history_value(
                account,
                reader
                    .history_commits(walk.tip.as_str(), page, cancel)
                    .await,
            )
            .await?;
        self.check_scope(&scope, cancel)?;
        let entries = history
            .records
            .into_iter()
            .map(history_entry)
            .collect::<Result<Vec<_>, _>>()?;
        let next = if history.limited {
            None
        } else {
            history.next_page
        };
        let cursor = next.map(|next| next.to_string());
        self.store.commit_history_page(
            &walk,
            &entries,
            cursor.as_deref(),
            next.is_none() && !history.limited,
            UtcMillis::now()?.0,
        )?;
        let coverage = self
            .store
            .history_page(account, repository, &walk.tip, None, 1)?
            .coverage;
        Ok(PageStatus {
            has_more: coverage == threadline_storage::history::HistoryCoverage::Partial
                && next.is_some(),
            complete: coverage == threadline_storage::history::HistoryCoverage::Complete,
            contacted: true,
        })
    }

    pub(crate) async fn fetch_history_diff(
        &self,
        comparison: &HistoryComparison,
        cancel: &Cancellation,
    ) -> Result<PageStatus, ReaderError> {
        let identity = comparison.diff_identity();
        let account = self
            .store
            .active_github_account()?
            .ok_or(ReaderError::ScopeChanged)?;
        let scope = self.scope(account, identity.repository, cancel)?;
        let Some(walk) = self.store.begin_history_diff(&scope.profile, comparison)? else {
            let page = self.store.history_diff_page(account, comparison, None, 1)?;
            return Ok(PageStatus {
                complete: page.coverage == threadline_storage::history::HistoryCoverage::Complete,
                has_more: false,
                contacted: false,
            });
        };
        let page = history_page_number(walk.cursor.as_deref())?;
        let head = identity.head.as_str();
        let base = identity.base.as_ref().map(CommitOid::as_str);
        let query = match (identity.semantics, base) {
            (DiffSemantics::Root, None) => HistoryDiff::Root { head },
            (DiffSemantics::Direct, Some(base)) => HistoryDiff::Direct { base, head },
            (DiffSemantics::MergeBase, Some(base)) => HistoryDiff::MergeBase { base, head },
            _ => return Err(ReaderError::InvalidIdentity),
        };
        if identity.options.ignore_whitespace || identity.options.context_lines != 3 {
            self.store.commit_history_diff_page(
                &walk,
                HistoryDiffInput {
                    identity: &identity,
                    effective_base: None,
                    files: Vec::new(),
                    next_cursor: None,
                    complete: false,
                    observed_at_ms: UtcMillis::now()?.0,
                },
            )?;
            return Ok(PageStatus {
                complete: false,
                has_more: false,
                contacted: false,
            });
        }
        let bound = self.binding(&scope, cancel).await?;
        self.admit(account, cancel).await?;
        let HistoryFiles {
            effective_base,
            page,
        } = self
            .history_value(
                account,
                bound.reader().history_files(query, page, cancel).await,
            )
            .await?;
        self.check_scope(&scope, cancel)?;
        let effective_base = effective_base
            .as_deref()
            .map(CommitOid::parse)
            .transpose()?;
        let next = if page.limited { None } else { page.next_page };
        let cursor = next.map(|next| next.to_string());
        let records = page
            .records
            .into_iter()
            .map(pr_reads::convert::file)
            .collect();
        self.store.commit_history_diff_page(
            &walk,
            HistoryDiffInput {
                identity: &identity,
                effective_base: effective_base.as_ref(),
                files: records,
                next_cursor: cursor.as_deref(),
                complete: next.is_none() && !page.limited,
                observed_at_ms: UtcMillis::now()?.0,
            },
        )?;
        Ok(PageStatus::from_next(next, page.limited))
    }

    async fn history_value<T>(
        &self,
        account: AccountId,
        result: Result<Refresh<T>, AccessError>,
    ) -> Result<T, ReaderError> {
        let response = self.response(account, result).await?;
        if self.accounts.sync_worker_enabled() {
            hold(&self.store, account, Some(response.rate), None).await;
        }
        response.value.ok_or(ReaderError::InvalidIdentity)
    }
}

fn history_entry(row: HistoryCommit) -> Result<HistoryEntry, ReaderError> {
    Ok(HistoryEntry {
        commit: GraphCommit {
            oid: CommitOid::parse(&row.oid)?,
            title: row.message.lines().next().unwrap_or("").to_owned(),
            committed_at_ms: row.committed_at_ms.ok_or(ReaderError::InvalidIdentity)?,
            parents: row
                .parents
                .iter()
                .map(|parent| CommitOid::parse(parent))
                .collect::<Result<_, _>>()?,
        },
        message: row.message,
        author: row.author,
        authored_at_ms: row.authored_at_ms,
    })
}

fn history_page_number(cursor: Option<&str>) -> Result<u32, ReaderError> {
    cursor.map_or(Ok(1), |cursor| {
        cursor
            .parse::<u32>()
            .ok()
            .filter(|page| *page > 1)
            .ok_or(ReaderError::BadCursor)
    })
}
