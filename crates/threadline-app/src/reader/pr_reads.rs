mod convert;
mod downloads;
mod flights;
mod references;

pub(crate) use flights::Flights;

use threadline_core::github::RateLimit;
use threadline_core::github::reader::{
    GitHubReader,
    pr::{PrSide, PrSnapshot, PrSource, PrSourceRange},
    refresh::{ReadValidators, Refresh},
};
use threadline_storage::pr::{PrFamily, PrHeader, PrInputPage, PrRecord, PrRevision, SourceSide};
use threadline_storage::resources::CommitOid;

use super::*;

pub(crate) struct PrSourceRequest<'a> {
    pub(crate) revision: &'a PrRevision,
    pub(crate) file: &'a str,
    pub(crate) side: SourceSide,
    pub(crate) first_line: u32,
    pub(crate) lines: u16,
}

struct Prepared {
    scope: Scope,
    item: ItemId,
    locator: ItemLocator,
    snapshot: PrSnapshot,
    reader: BoundRepositoryReader,
    revision: PrRevision,
    header: PrHeader,
}

pub(crate) struct FetchPage {
    pub(crate) records: Vec<PrRecord>,
    pub(crate) next: Option<String>,
    pub(crate) partial: Option<ResourceError>,
    pub(crate) validators: Validators,
    pub(crate) merge_base: Option<String>,
    pub(crate) review_decision: Option<String>,
}

impl ReaderService {
    pub(crate) async fn fetch_pr_family(
        &self,
        account: AccountId,
        pr: ItemId,
        family: PrFamily,
        cancel: &Cancellation,
    ) -> Result<PageStatus, ReaderError> {
        let current = self.store.current_pr_revision(account, pr)?;
        let intended = current.as_ref().map(|(revision, _)| revision.id);
        let key = (account, pr, intended, family.name());
        let service = self.clone();
        self.pr_reads
            .read(key, cancel, move |flight| async move {
                if let Some((rev, _)) = current
                    && !mutable_family(family)
                    && service.store.pr_page(&rev, family, None, 1)?.state.coverage
                        == Coverage::Complete
                {
                    return Ok(PageStatus::complete());
                }
                let mut prepared = service.prepare_pr(account, pr, &flight).await?;
                if intended.is_some_and(|id| id != prepared.revision.id) {
                    return Err(ReaderError::ScopeChanged);
                }
                service
                    .read_pr_page(&mut prepared, family, None, &flight)
                    .await
            })
            .await
    }

    pub(crate) async fn fetch_pr_source(
        &self,
        account: AccountId,
        pr: ItemId,
        request: PrSourceRequest<'_>,
        cancel: &Cancellation,
    ) -> Result<(), ReaderError> {
        if request.revision.account != account || request.revision.pr != pr {
            return Err(ReaderError::ScopeChanged);
        }
        let mut prepared = self.prepare_pr(account, pr, cancel).await?;
        if prepared.revision.id != request.revision.id
            || prepared.revision.base != request.revision.base
            || prepared.revision.head != request.revision.head
        {
            return Err(ReaderError::ScopeChanged);
        }
        self.admit(account, cancel).await?;
        let repo = prepared.reader.reader();
        let comparison = self
            .pr_value(
                account,
                repo.pr_comparison(&prepared.snapshot, 1, &ReadValidators::default(), cancel)
                    .await,
            )
            .await?;
        let comparison = comparison.0;
        if request
            .revision
            .merge_base
            .as_ref()
            .is_some_and(|oid| oid.as_str() != comparison.merge_base())
        {
            return Err(ReaderError::ScopeChanged);
        }
        self.check_item(&prepared.scope, pr, prepared.locator, cancel)?;
        prepared.revision = self.save_pr_revision(&prepared, Some(comparison.merge_base()))?;
        self.admit(account, cancel).await?;
        let side = match request.side {
            SourceSide::Left => PrSide::Left,
            SourceSide::Right => PrSide::Right,
        };
        let source = self
            .pr_value(
                account,
                repo.pr_source_context(
                    &prepared.snapshot,
                    comparison.identity(),
                    PrSourceRange {
                        side,
                        path: request.file,
                        start: request.first_line,
                        lines: request.lines,
                    },
                    cancel,
                )
                .await,
            )
            .await?
            .0;
        let record = match source {
            PrSource::Text {
                oid,
                path,
                first_line,
                text,
            } => PrRecord::Context {
                oid,
                path,
                first_line,
                text,
            },
            PrSource::Binary | PrSource::Unavailable(_) => {
                return Err(ReaderError::SourceUnavailable);
            }
        };
        self.check_item(&prepared.scope, pr, prepared.locator, cancel)?;
        let coverage = self
            .store
            .pr_page(&prepared.revision, PrFamily::Context, None, 1)?
            .state
            .coverage;
        let mode = if coverage == Coverage::Complete {
            WalkMode::Incremental
        } else {
            WalkMode::Full
        };
        let progress = self
            .store
            .start_pr_fetch(&prepared.revision, PrFamily::Context, mode)?;
        self.store.commit_pr_page(
            &prepared.revision,
            PrFamily::Context,
            &progress,
            PrInputPage {
                records: vec![record],
                next_cursor: None,
                validators: None,
                partial: None,
                fetched_at_ms: UtcMillis::now()?.0,
            },
        )?;
        Ok(())
    }

    async fn prepare_pr(
        &self,
        account: AccountId,
        item: ItemId,
        cancel: &Cancellation,
    ) -> Result<Prepared, ReaderError> {
        let original = self
            .store
            .item(account, item)?
            .filter(|item| item.kind == ItemKind::PullRequest)
            .ok_or(ReaderError::InvalidIdentity)?;
        let (scope, number) = self.item_scope(account, original.locator, cancel)?;
        let reader = self.binding(&scope, cancel).await?;
        self.admit(account, cancel).await?;
        let snapshot = self
            .pr_value(
                account,
                reader
                    .reader()
                    .pr_snapshot(number, &ReadValidators::default(), cancel)
                    .await,
            )
            .await?
            .0;
        self.check_item(&scope, item, original.locator, cancel)?;
        let details = snapshot.details();
        self.commit_item(&scope, item, pull_request(details.clone())?)?;
        let presentation = &details.presentation;
        let header = PrHeader {
            author: presentation.author.clone(),
            base_ref: presentation.base_ref.clone(),
            head_ref: presentation.head_ref.clone(),
            draft: presentation.draft,
            requested_reviewers: presentation.requested_reviewers.clone(),
            review_decision: None,
            created_at_ms: presentation.created_at_ms,
            merged_at_ms: presentation.merged_at_ms,
        };
        let (base, head) = pr_oids(details)?;
        let revision = self.store.record_pr_revision(
            account,
            item,
            &base,
            &head,
            None,
            &header,
            details.updated_at_ms,
        )?;
        let current = self
            .store
            .current_pr_revision(account, item)?
            .ok_or(ReaderError::ScopeChanged)?
            .0;
        if revision.id != current.id {
            return Err(ReaderError::ScopeChanged);
        }
        self.refresh_pr_references(account, item)?;
        Ok(Prepared {
            scope,
            item,
            locator: original.locator,
            snapshot,
            reader,
            revision,
            header,
        })
    }

    fn refresh_pr_references(&self, account: AccountId, pr: ItemId) -> Result<(), ReaderError> {
        let details = self
            .store
            .reader_item_metadata(account, pr)?
            .ok_or(ReaderError::ScopeChanged)?;
        let Some(stamp) = details.body_at_ms else {
            return Ok(());
        };
        let body_revision = format!("github-body-{stamp}");
        let previous = self
            .store
            .pr_reference_set(account, pr)?
            .map(|(revision, _)| revision);
        if previous.as_deref() != Some(body_revision.as_str()) {
            self.store
                .index_pr_body(account, pr, previous.as_deref(), &body_revision, stamp)?;
        }
        Ok(())
    }

    async fn pr_value<T>(
        &self,
        account: AccountId,
        result: Result<Refresh<T>, AccessError>,
    ) -> Result<(T, Validators), ReaderError> {
        let response = self.response(account, result).await?;
        self.pr_rate(account, response.rate).await;
        let value = response.value.ok_or(ReaderError::InvalidIdentity)?;
        Ok((value, convert::validators(response.validators)))
    }

    async fn pr_rate(&self, account: AccountId, rate: threadline_core::github::RateLimit) {
        if self.accounts.sync_worker_enabled() {
            hold(&self.store, account, Some(rate), None).await;
        }
    }

    fn save_pr_revision(
        &self,
        prepared: &Prepared,
        merge_base: Option<&str>,
    ) -> Result<PrRevision, ReaderError> {
        let details = prepared.snapshot.details();
        let (base, head) = pr_oids(details)?;
        let merge_base = merge_base.map(CommitOid::parse).transpose()?;
        Ok(self.store.record_pr_revision(
            prepared.scope.account,
            prepared.item,
            &base,
            &head,
            merge_base.as_ref(),
            &prepared.header,
            details.updated_at_ms,
        )?)
    }

    async fn read_pr_page(
        &self,
        prepared: &mut Prepared,
        family: PrFamily,
        download: Option<i64>,
        cancel: &Cancellation,
    ) -> Result<PageStatus, ReaderError> {
        let account = prepared.scope.account;
        if let Some(replied) = self.resume_pr_replies(prepared, family, cancel).await? {
            return Ok(replied);
        }
        let cached = self
            .store
            .pr_page(&prepared.revision, family, None, 1)?
            .state;
        let refresh = download.is_none() && mutable_family(family);
        let mut progress = self.store.resume_pr_fetch(&prepared.revision, family)?;
        if progress.is_none() {
            if !refresh && cached.coverage == Coverage::Complete {
                return Ok(PageStatus::complete());
            }
            progress = Some(self.store.start_pr_fetch(
                &prepared.revision,
                family,
                WalkMode::Full,
            )?);
        }
        let progress = progress.expect("existing or started PR fetch");
        let read_validators = if refresh && progress.cursor.is_none() {
            ReadValidators {
                etag: cached.validators.etag.clone(),
                last_modified: cached.validators.last_modified.clone(),
            }
        } else {
            ReadValidators::default()
        };
        let result: Result<PageStatus, ReaderError> = async {
            self.admit(account, cancel).await?;
            let fetched = match request_pr_page(
                &prepared.reader.reader(),
                &prepared.snapshot,
                family,
                progress.cursor.as_deref(),
                &read_validators,
                cancel,
            )
            .await
            {
                Ok((Some(fetched), rate)) => {
                    self.pr_rate(account, rate).await;
                    fetched
                }
                Ok((None, rate)) => {
                    self.pr_rate(account, rate).await;
                    self.check_pr_current(prepared, cancel)?;
                    self.store.revalidate_current_pr(
                        &prepared.revision,
                        family,
                        &progress,
                        &cached.validators,
                        UtcMillis::now()?.0,
                    )?;
                    return Ok(PageStatus {
                        has_more: false,
                        complete: true,
                        contacted: true,
                    });
                }
                Err(ReaderError::Remote(error)) => return self.response(account, Err(error)).await,
                Err(error) => return Err(error),
            };
            self.check_pr_current(prepared, cancel)?;
            let ancestor = fetched
                .merge_base
                .as_deref()
                .map(CommitOid::parse)
                .transpose()?;
            let updated_identity = ancestor.is_some() || fetched.review_decision.is_some();
            if let Some(decision) = fetched.review_decision {
                prepared.header.review_decision = Some(decision);
            }
            let identity = updated_identity.then_some(threadline_storage::pr::PrPageIdentity {
                merge_base: ancestor.as_ref(),
                header: &prepared.header,
                provider_updated_at_ms: prepared.snapshot.details().updated_at_ms,
            });
            let more = fetched.next.is_some();
            let partial = fetched.partial;
            let update = PrInputPage {
                records: fetched.records,
                next_cursor: fetched.next.as_deref(),
                validators: (progress.cursor.is_none() && !more).then_some(&fetched.validators),
                partial,
                fetched_at_ms: UtcMillis::now()?.0,
            };
            prepared.revision = self.store.commit_current_pr_page(
                &prepared.revision,
                family,
                &progress,
                update,
                identity,
                download,
            )?;
            Ok(PageStatus {
                has_more: more,
                complete: !more && partial.is_none(),
                contacted: true,
            })
        }
        .await;
        if let Err(error) = &result
            && !matches!(error, ReaderError::Cancelled | ReaderError::ScopeChanged)
        {
            let _ = self.store.record_pr_fetch_error(
                &prepared.revision,
                family,
                &progress,
                error.resource_error(),
            );
        }
        result
    }

    fn check_pr_current(
        &self,
        prepared: &Prepared,
        cancel: &Cancellation,
    ) -> Result<(), ReaderError> {
        self.check_item(&prepared.scope, prepared.item, prepared.locator, cancel)?;
        if self
            .store
            .current_pr_revision(prepared.scope.account, prepared.item)?
            .is_none_or(|(revision, _)| revision.id != prepared.revision.id)
        {
            return Err(ReaderError::ScopeChanged);
        }
        Ok(())
    }

    async fn resume_pr_replies(
        &self,
        prepared: &Prepared,
        family: PrFamily,
        cancel: &Cancellation,
    ) -> Result<Option<PageStatus>, ReaderError> {
        if family != PrFamily::Threads
            || self
                .store
                .resume_pr_fetch(&prepared.revision, family)?
                .is_some()
        {
            return Ok(None);
        }
        let mut after = None;
        loop {
            let page = self
                .store
                .pr_page(&prepared.revision, family, after.as_deref(), 50)?;
            for (_, record) in page.rows {
                let PrRecord::Thread {
                    node_id,
                    reply_cursor: Some(_),
                    ..
                } = record
                else {
                    continue;
                };
                let Some(checkpoint) = self
                    .store
                    .pr_reply_checkpoint(&prepared.revision, &node_id)?
                else {
                    continue;
                };
                let account = prepared.scope.account;
                self.admit(account, cancel).await?;
                let result = self
                    .response(
                        account,
                        prepared
                            .reader
                            .reader()
                            .resume_pr_thread_replies(
                                &prepared.snapshot,
                                &checkpoint.thread_node,
                                &checkpoint.cursor,
                                cancel,
                            )
                            .await,
                    )
                    .await?;
                self.pr_rate(account, result.rate).await;
                self.check_item(&prepared.scope, prepared.item, prepared.locator, cancel)?;
                if result.partial && result.next_cursor.is_none() {
                    return Err(ReaderError::IncompleteAccess);
                }
                self.store.commit_pr_replies(
                    &prepared.revision,
                    &checkpoint,
                    result
                        .records
                        .into_iter()
                        .map(convert::inline_comment)
                        .collect(),
                    result.next_cursor.as_deref(),
                    UtcMillis::now()?.0,
                )?;
                return Ok(Some(PageStatus {
                    has_more: true,
                    complete: false,
                    contacted: true,
                }));
            }
            after = page.next;
            if after.is_none() {
                return Ok(None);
            }
        }
    }
}

fn mutable_family(family: PrFamily) -> bool {
    matches!(
        family,
        PrFamily::Reviews | PrFamily::Threads | PrFamily::Checks | PrFamily::Statuses
    )
}

pub(crate) async fn request_pr_page(
    repo: &GitHubReader<'_>,
    snapshot: &PrSnapshot,
    family: PrFamily,
    cursor: Option<&str>,
    validators: &ReadValidators,
    cancel: &Cancellation,
) -> Result<(Option<FetchPage>, RateLimit), ReaderError> {
    fn map_response<T>(
        response: Refresh<T>,
        map: impl FnOnce(T, Validators) -> Result<FetchPage, ReaderError>,
    ) -> Result<(Option<FetchPage>, RateLimit), ReaderError> {
        let result = response
            .value
            .map(|value| map(value, convert::validators(response.validators)))
            .transpose()?;
        Ok((result, response.rate))
    }
    match family {
        PrFamily::Commits | PrFamily::Files => {
            let response = repo
                .pr_comparison(snapshot, numeric_page(cursor)?, validators, cancel)
                .await?;
            map_response(response, |data, validators| {
                let merge_base = Some(data.merge_base().to_owned());
                let mut page = if family == PrFamily::Commits {
                    convert::page(data.commits, validators, convert::commit)
                } else {
                    convert::files(data.files.ok_or(ReaderError::BadCursor)?, validators)
                };
                page.merge_base = merge_base;
                Ok(page)
            })
        }
        PrFamily::Reviews => {
            let result = repo.pr_reviews(snapshot, cursor, cancel).await?;
            let rate = result.rate;
            Ok((Some(convert::graph(result, convert::review)), rate))
        }
        PrFamily::Threads => {
            let result = repo.pr_threads(snapshot, cursor, cancel).await?;
            let rate = result.rate;
            Ok((Some(convert::graph(result, convert::thread)), rate))
        }
        PrFamily::Checks => {
            let result = repo
                .pr_checks(snapshot, numeric_page(cursor)?, validators, cancel)
                .await?;
            map_response(result, |data, validators| {
                Ok(convert::page(data, validators, convert::check))
            })
        }
        PrFamily::Statuses => {
            let result = repo
                .pr_statuses(snapshot, numeric_page(cursor)?, validators, cancel)
                .await?;
            map_response(result, |data, validators| {
                Ok(convert::page(data, validators, convert::status))
            })
        }
        PrFamily::Context | PrFamily::Media => Err(ReaderError::InvalidIdentity),
    }
}

fn numeric_page(cursor: Option<&str>) -> Result<u32, ReaderError> {
    cursor.map_or(Ok(1), |value| {
        value
            .parse::<u32>()
            .ok()
            .filter(|page| *page > 1)
            .ok_or(ReaderError::BadCursor)
    })
}

fn pr_oids(details: &PullRequestSummary) -> Result<(CommitOid, CommitOid), ReaderError> {
    Ok((
        CommitOid::parse(&details.base_oid)?,
        CommitOid::parse(&details.head_oid)?,
    ))
}
