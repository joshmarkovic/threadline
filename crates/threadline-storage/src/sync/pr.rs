use super::*;
use crate::pr::{
    PrFamily, PrInputPage, PrPageIdentity, PrRevision, attach_page_identity,
    require_current_revision,
};
use crate::resources::{
    ResourceError, ResourceProgress, WalkMode, accept_not_modified, require_walk_id,
    resume_walk_in, start_walk_in,
};

impl PrRevision {
    pub fn read_target(&self, family: PrFamily) -> ReadTarget {
        let resource = match family {
            PrFamily::Commits => ResourceFamily::History,
            PrFamily::Files | PrFamily::Context | PrFamily::Media => ResourceFamily::Files,
            PrFamily::Reviews => ResourceFamily::Reviews,
            PrFamily::Threads => ResourceFamily::ReviewThreads,
            PrFamily::Checks | PrFamily::Statuses => ResourceFamily::Checks,
        };
        ReadTarget {
            repository: self.repository,
            item: Some(self.pr),
            family: resource,
            revision: format!(
                "pr:{}:{}:{}",
                family.name(),
                self.base.as_str(),
                self.head.as_str()
            ),
        }
    }
}

fn authorize(
    tx: &Transaction<'_>,
    claim: &ReadClaim,
    rev: &PrRevision,
    family: PrFamily,
    now: i64,
) -> Result<()> {
    if claim.account != rev.account || claim.target != rev.read_target(family) {
        return Err(Error::Conflict(
            "Scheduled PR read has a different target or revision",
        ));
    }
    require_claim(tx, claim, now)?;
    require_current_revision(tx, rev)
}

impl Store {
    /// Resume the one PR family assigned to this claim without restarting any other endpoint.
    pub fn prepare_sync_pr_page(
        &self,
        claim: &ReadClaim,
        rev: &PrRevision,
        family: PrFamily,
        now: i64,
    ) -> Result<ResourceProgress> {
        self.write(|tx| {
            authorize(tx, claim, rev, family, now)?;
            let id = crate::pr::ensure_family(tx, rev, family)?;
            if let Some(saved) = resume_walk_in(tx, id)? {
                if saved.cursor == claim.cursor {
                    return Ok(saved);
                }
                if claim.cursor.is_some() {
                    return Err(Error::Conflict(
                        "PR resource and scheduler checkpoints disagree",
                    ));
                }
            }
            if claim.cursor.is_some() {
                return Err(Error::Conflict(
                    "Scheduled PR continuation has no cached walk",
                ));
            }
            start_walk_in(tx, id, WalkMode::Full)
        })
    }

    /// Commit the first-page validator, content, journal and continuation under the current authorisation.
    pub fn commit_sync_pr_page(
        &self,
        claim: &ReadClaim,
        rev: &PrRevision,
        family: PrFamily,
        expected: &ResourceProgress,
        page: PrInputPage<'_>,
        zone: &str,
    ) -> Result<()> {
        self.commit_sync_pr_observation(claim, rev, family, expected, page, zone, None)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The page and any new immutable identity evidence are accepted in the same transaction"
    )]
    pub fn commit_sync_pr_observation(
        &self,
        claim: &ReadClaim,
        rev: &PrRevision,
        family: PrFamily,
        expected: &ResourceProgress,
        page: PrInputPage<'_>,
        zone: &str,
        identity: Option<PrPageIdentity<'_>>,
    ) -> Result<()> {
        let now = page.fetched_at_ms;
        self.write(|tx| {
            authorize(tx,claim,rev,family,now)?;
            if expected.cursor!=claim.cursor {return Err(Error::Conflict("PR page is not the claimed continuation"))}
            let rev = attach_page_identity(tx, rev, identity)?;
            if claim.cursor.is_none() {
                let validators=page.validators.cloned().unwrap_or_default();
                tx.execute("UPDATE scheduled_reads SET walk_etag=?2,walk_modified=?3,walk_revision=?4 WHERE id=?1",
                    params![claim.id,validators.etag,validators.last_modified,validators.revision])?;
            }
            let validators=tx.query_row("SELECT walk_etag,walk_modified,walk_revision FROM scheduled_reads WHERE id=?",
                [claim.id],|row|Ok(Validators{etag:row.get(0)?,last_modified:row.get(1)?,revision:row.get(2)?}))?;
            let next=page.next_cursor;
            let reason=page.partial;
            let stored=PrInputPage{validators:if next.is_none() && reason.is_none(){Some(&validators)}else{None},..page};
            crate::pr::publish_page(tx,&rev,family,expected,stored)?;
            if let Some(reason)=reason {
                let suspended=matches!(reason,ResourceError::Forbidden|ResourceError::Unauthorized|ResourceError::NotFound);
                // The cache kept usable rows but this limited response cannot advance the walk.
                tx.execute("UPDATE scheduled_reads SET cursor=NULL,walk_started=NULL,lease_until=NULL,leased_generation=NULL,
                    failure='partial-pr-resource',failures=failures+1,suspended=?2,due_at=?3 WHERE id=?1",
                    params![claim.id,suspended,now.saturating_add(60_000)])?;
                return Ok(())
            }
            if let Some(next)=next {
                tx.execute("UPDATE scheduled_reads SET cursor=?2,due_at=?3,lease_until=NULL,leased_generation=NULL,
                    failures=0,failure=NULL,suspended=0 WHERE id=?1",params![claim.id,next,now])?;
                return Ok(())
            }
            complete_in(tx,claim,now,zone,&validators)
        })
    }

    pub fn revalidate_sync_pr(
        &self,
        claim: &ReadClaim,
        rev: &PrRevision,
        family: PrFamily,
        expected: &ResourceProgress,
        now: i64,
        zone: &str,
    ) -> Result<()> {
        self.write(|tx| {
            authorize(tx, claim, rev, family, now)?;
            if claim.cursor.is_some() {
                return Err(Error::Conflict(
                    "A partial PR list cannot revalidate with 304",
                ));
            }
            let rid = crate::pr::ensure_family(tx, rev, family)?;
            let (_, has_page) = require_walk_id(tx, rid, expected)?;
            accept_not_modified(tx, rid, has_page, &claim.validators, now)?;
            complete_in(tx, claim, now, zone, &claim.validators)
        })
    }
}
