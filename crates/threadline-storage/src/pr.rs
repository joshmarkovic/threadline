//! Per-revision pull request records, read checkpoints and offline pins.
use crate::{AccountId, Error, ItemId, RepositoryId, Result, Store, resources::*};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

mod replies;
pub use replies::*;
mod downloads;
pub use downloads::*;
mod references;
pub use references::*;
mod observation;
pub(crate) use observation::enroll_selected;
pub(crate) use observation::monitor_event;
mod model;
pub use model::*;
mod lines;
mod records;
pub(crate) use lines::file_mappings;
pub use lines::{SourceLine, SourceLocation};
pub(crate) use records::publish_page;

pub struct PrPage {
    pub rows: Vec<(String, PrRecord)>,
    pub next: Option<String>,
    pub state: ResourceState,
    pub api_capped: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrPageProblem {
    Resource(ResourceError),
    ComparisonFilesCap,
}

impl PrPageProblem {
    pub(crate) fn resource_error(self) -> ResourceError {
        match self {
            Self::Resource(error) => error,
            Self::ComparisonFilesCap => ResourceError::InvalidResponse,
        }
    }
}

pub struct PrInputPage<'a> {
    pub records: Vec<PrRecord>,
    pub next_cursor: Option<&'a str>,
    pub validators: Option<&'a Validators>,
    /// Terminal inaccessible, capped or otherwise incomplete result.
    pub partial: Option<PrPageProblem>,
    pub fetched_at_ms: i64,
}

pub struct PrPageIdentity<'a> {
    pub merge_base: Option<&'a CommitOid>,
    pub header: &'a PrHeader,
    pub provider_updated_at_ms: i64,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct StoredRecord {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) mappings: Vec<SourceLine>,
    ordinal: i64,
    pub(crate) record: PrRecord,
    search: String,
}

fn encoded<T: Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| Error::InvalidInput("Invalid PR record"))
}
fn decoded<'a, T: Deserialize<'a>>(s: &'a str) -> Result<T> {
    serde_json::from_str(s).map_err(|_| Error::Corrupt)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Revision and its provider observation are published within the caller's transaction"
)]
pub(crate) fn record_revision_in(
    tx: &Transaction<'_>,
    account: AccountId,
    pr: ItemId,
    base: &CommitOid,
    head: &CommitOid,
    merge_base: Option<&CommitOid>,
    header: &PrHeader,
    updated_at_ms: i64,
) -> Result<PrRevision> {
    let (repository, body, body_at) = tx
        .query_row(
            "SELECT repository_id,body,body_at FROM items
         WHERE account_id=? AND id=? AND kind='pull_request'",
            params![account, pr],
            |row| {
                Ok((
                    row.get::<_, RepositoryId>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    let previous = current_in(tx, account, pr)?;
    let existing = tx
        .query_row(
            "SELECT merge_base_oid,header,updated_at FROM pr_revisions
         WHERE account_id=? AND repository_id=? AND item_id=? AND base_oid=? AND head_oid=?",
            params![account, repository, pr, base.as_str(), head.as_str()],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    let mut next_header = header.clone();
    if let Some((ancestor, raw, observed)) = existing {
        if let (Some(saved), Some(new)) = (ancestor, merge_base)
            && saved != new.as_str()
        {
            return Err(Error::Conflict(
                "Immutable PR comparison has a different merge base",
            ));
        }
        let old: PrHeader = decoded(&raw)?;
        next_header = if observed >= updated_at_ms {
            old.fill_unknown(header)
        } else {
            next_header.fill_unknown(&old)
        };
    }
    let body_at = body_at.filter(|stamp| *stamp <= updated_at_ms);
    let body = body_at.and(body);
    let id = tx.query_row(
        "INSERT INTO pr_revisions(account_id,repository_id,item_id,base_oid,head_oid,merge_base_oid,header,updated_at,body,body_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
         ON CONFLICT(account_id,repository_id,item_id,base_oid,head_oid) DO UPDATE SET
             merge_base_oid=COALESCE(pr_revisions.merge_base_oid,excluded.merge_base_oid),
             header=excluded.header,updated_at=MAX(pr_revisions.updated_at,excluded.updated_at),
             body=CASE WHEN excluded.body_at IS NOT NULL AND
                 (pr_revisions.body_at IS NULL OR excluded.body_at>pr_revisions.body_at)
                 THEN excluded.body ELSE pr_revisions.body END,
             body_at=COALESCE(MAX(excluded.body_at,pr_revisions.body_at),pr_revisions.body_at,excluded.body_at)
         RETURNING id",
        params![account, repository, pr, base.as_str(), head.as_str(),merge_base.map(CommitOid::as_str),
            encoded(&next_header)?, updated_at_ms, body, body_at],
        |row| row.get(0),
    )?;
    let (revision, _) = revision_in(tx, account, pr, id)?.ok_or(Error::Corrupt)?;
    if let Some((_, previous_header, observed)) = previous
        && updated_at_ms > observed
    {
        observation::reviewer_changes(
            tx,
            &revision,
            &previous_header,
            &next_header,
            updated_at_ms,
        )?;
    }
    Ok(revision)
}

impl Store {
    /// Record a provider response for immutable endpoints. A delayed older snapshot never selects a new current head.
    #[expect(
        clippy::too_many_arguments,
        reason = "The provider supplies base, head, merge base and metadata together in one observation"
    )]
    pub fn record_pr_revision(
        &self,
        account: AccountId,
        pr: ItemId,
        base: &CommitOid,
        head: &CommitOid,
        merge_base: Option<&CommitOid>,
        header: &PrHeader,
        updated_at_ms: i64,
    ) -> Result<PrRevision> {
        self.write(|tx| {
            record_revision_in(
                tx,
                account,
                pr,
                base,
                head,
                merge_base,
                header,
                updated_at_ms,
            )
        })
    }

    pub fn current_pr_revision(
        &self,
        account: AccountId,
        pr: ItemId,
    ) -> Result<Option<(PrRevision, PrHeader)>> {
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            Ok(current_in(&tx, account, pr)?.map(|(rev, header, _)| (rev, header)))
        })
    }

    /// Resolve a revision from an FTS result scoped to the original account and PR.
    pub fn pr_revision(
        &self,
        account: AccountId,
        pr: ItemId,
        id: i64,
    ) -> Result<Option<(PrRevision, PrHeader)>> {
        self.read(|db| revision_in(db, account, pr, id))
    }
    /// Return the downloaded body as observed for this revision, including a provider-reported null.
    pub fn pr_revision_body(&self, rev: &PrRevision) -> Result<crate::Change<String>> {
        self.read(|db| {
            validate_revision(db, rev)?;
            Ok(revision_body_in(db, rev)?.0)
        })
    }

    pub fn start_pr_fetch(
        &self,
        rev: &PrRevision,
        family: PrFamily,
        mode: WalkMode,
    ) -> Result<ResourceProgress> {
        self.write(|tx| start_walk_in(tx, ensure_family(tx, rev, family)?, mode))
    }
    pub fn resume_pr_fetch(
        &self,
        rev: &PrRevision,
        family: PrFamily,
    ) -> Result<Option<ResourceProgress>> {
        self.write(|tx| match family_id(tx, rev, family)? {
            Some(id) => resume_walk_in(tx, id),
            None => Ok(None),
        })
    }

    pub fn commit_pr_page(
        &self,
        rev: &PrRevision,
        family: PrFamily,
        expected: &ResourceProgress,
        page: PrInputPage<'_>,
    ) -> Result<Option<ResourceProgress>> {
        self.write(|tx| publish_page(tx, rev, family, expected, page))
    }

    pub fn commit_current_pr_page(
        &self,
        rev: &PrRevision,
        family: PrFamily,
        expected: &ResourceProgress,
        page: PrInputPage<'_>,
        identity: Option<PrPageIdentity<'_>>,
        download: Option<i64>,
    ) -> Result<PrRevision> {
        self.write(|tx| {
            require_current_revision(tx, rev)?;
            if let Some(generation) = download {
                downloads::require_download_page(tx, rev, generation, family)?;
            }
            let rev = attach_page_identity(tx, rev, identity)?;
            publish_page(tx, &rev, family, expected, page)?;
            Ok(rev)
        })
    }

    pub fn revalidate_current_pr(
        &self,
        rev: &PrRevision,
        family: PrFamily,
        expected: &ResourceProgress,
        matched: &Validators,
        at_ms: i64,
    ) -> Result<()> {
        self.write(|tx| {
            require_current_revision(tx, rev)?;
            revalidate_page(tx, rev, family, expected, matched, at_ms)
        })
    }

    pub fn pr_not_modified(
        &self,
        rev: &PrRevision,
        family: PrFamily,
        expected: &ResourceProgress,
        matched: &Validators,
        at_ms: i64,
    ) -> Result<()> {
        self.write(|tx| revalidate_page(tx, rev, family, expected, matched, at_ms))
    }

    pub fn record_pr_fetch_error(
        &self,
        rev: &PrRevision,
        family: PrFamily,
        expected: &ResourceProgress,
        reason: ResourceError,
    ) -> Result<()> {
        self.write(|tx| {
            let id = family_id(tx, rev, family)?.ok_or(Error::NotFound)?;
            require_walk_id(tx, id, expected)?;
            set_fetch_error(tx, id, reason)
        })
    }

    pub fn pr_file(&self, rev: &PrRevision, path: &str) -> Result<Option<PrRecord>> {
        self.pr_record(rev, PrFamily::Files, path)
    }

    pub fn pr_file_with_source_lines(
        &self,
        rev: &PrRevision,
        path: &str,
    ) -> Result<Option<(PrRecord, Vec<SourceLine>)>> {
        Ok(self
            .pr_stored_record(rev, PrFamily::Files, path)?
            .map(|row| (row.record, row.mappings)))
    }

    pub fn pr_record(
        &self,
        rev: &PrRevision,
        family: PrFamily,
        key: &str,
    ) -> Result<Option<PrRecord>> {
        Ok(self
            .pr_stored_record(rev, family, key)?
            .map(|row| row.record))
    }

    fn pr_stored_record(
        &self,
        rev: &PrRevision,
        family: PrFamily,
        key: &str,
    ) -> Result<Option<StoredRecord>> {
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            let Some(id) = family_id(&tx, rev, family)? else {
                return Ok(None);
            };
            let raw = tx
                .query_row(
                    "SELECT body FROM cached_resource_records WHERE resource_id=? AND record_key=?",
                    params![id, key],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            raw.as_deref().map(decoded).transpose()
        })
    }
    pub fn pr_page(
        &self,
        rev: &PrRevision,
        family: PrFamily,
        after: Option<&str>,
        limit: u16,
    ) -> Result<PrPage> {
        if !(1..=100).contains(&limit) {
            return Err(Error::InvalidInput("Invalid PR page size"));
        }
        self.read(|db|{
            let tx=db.unchecked_transaction()?;
            let Some(id)=family_id(&tx,rev,family)? else {
                return Ok(PrPage{rows:vec![],next:None,state:missing_state(),api_capped:false});
            };
            let start=match after {
                None=>-1,
                Some(after)=>tx.query_row("SELECT json_extract(body,'$.ordinal') FROM cached_resource_records WHERE resource_id=? AND record_key=?",
                    params![id,after],|row|row.get(0)).optional()?.ok_or(Error::Conflict("PR page anchor changed"))?,
            };
            let mut q=tx.prepare("SELECT record_key,body FROM cached_resource_records
                WHERE resource_id=? AND json_extract(body,'$.ordinal')>? ORDER BY json_extract(body,'$.ordinal'),record_key LIMIT ?")?;
            let mut rows=Vec::new();
            let found=q.query_map(params![id,start,i64::from(limit)+1],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?;
            for row in found {
                let (key,raw)=row?;
                let value:StoredRecord=decoded(&raw)?;
                rows.push((key,value.record));
            }
            let next=if rows.len()>usize::from(limit){rows.pop();rows.last().map(|r|r.0.clone())}else{None};
            let state = state_by_id(&tx,id)?;
            let api_capped = family == PrFamily::Files && state.coverage == Coverage::Partial
                && tx.query_row("SELECT api_capped FROM pr_family_resources WHERE resource_id=?",
                    [id], |row| row.get::<_,bool>(0))?;
            Ok(PrPage{rows,next,state,api_capped})
        })
    }
}

pub(crate) fn require_current_revision(tx: &Transaction<'_>, rev: &PrRevision) -> Result<()> {
    if current_in(tx, rev.account, rev.pr)?.is_none_or(|(current, _, _)| current.id != rev.id) {
        return Err(Error::Conflict("PR revision has been superseded"));
    }
    Ok(())
}

pub(crate) fn attach_page_identity(
    tx: &Transaction<'_>,
    rev: &PrRevision,
    identity: Option<PrPageIdentity<'_>>,
) -> Result<PrRevision> {
    let Some(identity) = identity else {
        return Ok(rev.clone());
    };
    record_revision_in(
        tx,
        rev.account,
        rev.pr,
        &rev.base,
        &rev.head,
        identity.merge_base,
        identity.header,
        identity.provider_updated_at_ms,
    )
}

fn revalidate_page(
    tx: &Transaction<'_>,
    rev: &PrRevision,
    family: PrFamily,
    expected: &ResourceProgress,
    matched: &Validators,
    at_ms: i64,
) -> Result<()> {
    let id = family_id(tx, rev, family)?.ok_or(Error::NotFound)?;
    let (_, has_page) = require_walk_id(tx, id, expected)?;
    accept_not_modified(tx, id, has_page, matched, at_ms)
}

impl PrHeader {
    fn fill_unknown(mut self, other: &Self) -> Self {
        self.author = self.author.or_else(|| other.author.clone());
        self.base_ref = self.base_ref.or_else(|| other.base_ref.clone());
        self.head_ref = self.head_ref.or_else(|| other.head_ref.clone());
        self.draft = self.draft.or(other.draft);
        self.requested_reviewers = self
            .requested_reviewers
            .or_else(|| other.requested_reviewers.clone());
        self.review_decision = self
            .review_decision
            .or_else(|| other.review_decision.clone());
        self.created_at_ms = self.created_at_ms.or(other.created_at_ms);
        self.merged_at_ms = self.merged_at_ms.or(other.merged_at_ms);
        self
    }
}
fn revision_body_in(
    db: &Connection,
    rev: &PrRevision,
) -> Result<(crate::Change<String>, Option<i64>)> {
    let (body, stamp) = db.query_row(
        "SELECT body,body_at FROM pr_revisions WHERE id=?",
        [rev.id],
        |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<i64>>(1)?)),
    )?;
    let body = match (body, stamp) {
        (_, None) => crate::Change::Missing,
        (None, Some(_)) => crate::Change::Null,
        (Some(text), Some(_)) => crate::Change::Value(text),
    };
    Ok((body, stamp))
}
pub(crate) fn current_in(
    db: &Connection,
    account: AccountId,
    pr: ItemId,
) -> Result<Option<(PrRevision, PrHeader, i64)>> {
    let selected = db
        .query_row(
            "SELECT v.id,v.updated_at FROM pr_revisions v JOIN items i ON v.item_id=i.id
         AND v.repository_id=i.repository_id WHERE v.account_id=? AND v.item_id=?
         ORDER BY v.updated_at DESC,v.id ASC LIMIT 1",
            params![account, pr],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;
    let Some((id, stamp)) = selected else {
        return Ok(None);
    };
    Ok(revision_in(db, account, pr, id)?.map(|(rev, header)| (rev, header, stamp)))
}
pub(crate) fn revision_in(
    db: &Connection,
    account: AccountId,
    pr: ItemId,
    id: i64,
) -> Result<Option<(PrRevision, PrHeader)>> {
    let row = db
        .query_row(
            "SELECT repository_id,base_oid,head_oid,merge_base_oid,header FROM pr_revisions
        WHERE id=? AND account_id=? AND item_id=?",
            params![id, account, pr],
            |r| {
                Ok((
                    r.get::<_, RepositoryId>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    row.map(|(repo, base, head, ancestor, header)| {
        Ok((
            PrRevision {
                id,
                account,
                pr,
                repository: repo,
                base: CommitOid::parse(&base)?,
                head: CommitOid::parse(&head)?,
                merge_base: ancestor.as_deref().map(CommitOid::parse).transpose()?,
            },
            decoded(&header)?,
        ))
    })
    .transpose()
}
pub(crate) fn validate_revision(db: &Connection, rev: &PrRevision) -> Result<()> {
    let saved = revision_in(db, rev.account, rev.pr, rev.id)?
        .ok_or(Error::NotFound)?
        .0;
    if saved.repository != rev.repository
        || saved.base != rev.base
        || saved.head != rev.head
        || rev
            .merge_base
            .as_ref()
            .is_some_and(|oid| Some(oid) != saved.merge_base.as_ref())
    {
        return Err(Error::Conflict(
            "Immutable PR revision does not match local identity",
        ));
    }
    Ok(())
}
pub(crate) fn family_id(
    db: &Connection,
    rev: &PrRevision,
    family: PrFamily,
) -> Result<Option<i64>> {
    validate_revision(db, rev)?;
    Ok(db
        .query_row(
            "SELECT resource_id FROM pr_family_resources WHERE revision_id=? AND family=?",
            params![rev.id, family.name()],
            |row| row.get(0),
        )
        .optional()?)
}
pub(crate) fn ensure_family(
    tx: &Transaction<'_>,
    rev: &PrRevision,
    family: PrFamily,
) -> Result<i64> {
    if let Some(id) = family_id(tx, rev, family)? {
        return Ok(id);
    }
    let scoped = format!("pr:{}:{}", family.name(), rev.id);
    let rid = tx.query_row(
        "INSERT INTO cached_resources(account_id,item_id,family) VALUES(?,?,?) RETURNING id",
        params![rev.account, rev.pr, scoped],
        |r| r.get::<_, i64>(0),
    )?;
    tx.execute(
        "INSERT INTO pr_family_resources(revision_id,family,resource_id) VALUES(?,?,?)",
        params![rev.id, family.name(), rid],
    )?;
    Ok(rid)
}
