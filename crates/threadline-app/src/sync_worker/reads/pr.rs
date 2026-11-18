use threadline_core::github::reader::pr::PrSnapshot;
use threadline_storage::{
    pr::{PrFamily, PrHeader, PrInputPage, PrPageIdentity, PrRevision},
    resources::{CommitOid, ResourceProgress},
};

use crate::reader::pr_reads::{FetchPage, request_pr_page};

use super::*;

const FAMILIES: [PrFamily; 6] = [
    PrFamily::Commits,
    PrFamily::Files,
    PrFamily::Reviews,
    PrFamily::Threads,
    PrFamily::Checks,
    PrFamily::Statuses,
];

pub(super) struct ClaimedPage {
    revision: PrRevision,
    header: PrHeader,
    family: PrFamily,
    expected: ResourceProgress,
    provider_updated_at: i64,
    fetched: Option<FetchPage>,
}

impl ClaimedPage {
    pub(super) fn changed(&self) -> bool {
        self.fetched.is_some()
    }

    pub(super) fn commit(
        self,
        store: &Store,
        claim: &ReadClaim,
        now: i64,
        zone: &str,
    ) -> threadline_storage::Result<()> {
        let Some(fetched) = self.fetched else {
            return store.revalidate_sync_pr(
                claim,
                &self.revision,
                self.family,
                &self.expected,
                now,
                zone,
            );
        };
        let mut header = self.header;
        let ancestor = fetched
            .merge_base
            .as_deref()
            .map(CommitOid::parse)
            .transpose()?;
        let has_identity = ancestor.is_some() || fetched.review_decision.is_some();
        if let Some(decision) = fetched.review_decision {
            header.review_decision = Some(decision);
        }
        store.commit_sync_pr_observation(
            claim,
            &self.revision,
            self.family,
            &self.expected,
            PrInputPage {
                records: fetched.records,
                next_cursor: fetched.next.as_deref(),
                validators: Some(&fetched.validators),
                partial: fetched.partial,
                fetched_at_ms: now,
            },
            zone,
            has_identity.then_some(PrPageIdentity {
                merge_base: ancestor.as_ref(),
                header: &header,
                provider_updated_at_ms: self.provider_updated_at,
            }),
        )
    }
}

pub(super) async fn read(
    store: &Arc<Store>,
    binding: &mut Bound,
    claim: &ReadClaim,
    cancel: &Cancellation,
) -> Result<(Outcome, RateLimit), ReaderError> {
    let item = claim.target.item.ok_or(ReaderError::InvalidIdentity)?;
    let account = claim.account;
    let repository = claim.target.repository;
    let (revision, header, number, provider_id) = blocking(store, move |store| {
        let (rev, header) = store
            .current_pr_revision(account, item)?
            .ok_or(Error::NotFound)?;
        let item = store.item(account, item)?.ok_or(Error::NotFound)?;
        if item.locator.repository != repository {
            return Err(Error::Conflict("PR moved from the scheduled repository"));
        }
        let provider_id = item.pull_request.ok_or(Error::NotFound)?.id.get();
        Ok((rev, header, item.locator.number, provider_id))
    })
    .await?;
    let family = FAMILIES
        .into_iter()
        .find(|family| revision.read_target(*family) == claim.target)
        .ok_or(ReaderError::ScopeChanged)?;
    let reader = binding.reader.reader();
    let Some(snapshot) = binding.pr_snapshots.get(&revision.id) else {
        let number = number
            .try_into()
            .map_err(|_| ReaderError::InvalidIdentity)?;
        let fetched = reader
            .pr_snapshot(number, &ReadValidators::default(), cancel)
            .await?;
        let snapshot = fetched.value.ok_or(ReaderError::InvalidIdentity)?;
        check_revision(&snapshot, &revision, provider_id)?;
        binding
            .pr_snapshots
            .retain(|_, active| active.details().number != number);
        binding.pr_snapshots.insert(revision.id, snapshot);
        return Ok((Outcome::Prepared, fetched.rate));
    };
    check_revision(snapshot, &revision, provider_id)?;
    let provider_updated_at = snapshot.details().updated_at_ms;
    let lease = claim.clone();
    let rev = revision.clone();
    let expected = blocking(store, move |store| {
        store.prepare_sync_pr_page(&lease, &rev, family, UtcMillis::now()?.0)
    })
    .await?;
    let validators = ReadValidators {
        etag: claim.validators.etag.clone(),
        last_modified: claim.validators.last_modified.clone(),
    };
    let (fetched, rate) = request_pr_page(
        &reader,
        snapshot,
        family,
        expected.cursor.as_deref(),
        &validators,
        cancel,
    )
    .await?;
    Ok((
        Outcome::Pr(Box::new(ClaimedPage {
            revision,
            header,
            family,
            expected,
            provider_updated_at,
            fetched,
        })),
        rate,
    ))
}

fn check_revision(
    snapshot: &PrSnapshot,
    revision: &PrRevision,
    provider_id: u64,
) -> Result<(), ReaderError> {
    let details = snapshot.details();
    if details.pull_request_id.get() != provider_id
        || details.base_oid != revision.base.as_str()
        || details.head_oid != revision.head.as_str()
    {
        return Err(ReaderError::ScopeChanged);
    }
    Ok(())
}
