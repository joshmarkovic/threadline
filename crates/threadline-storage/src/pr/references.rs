use super::*;
use crate::{
    IssueApiId,
    queue::{ReferenceCandidate, refresh_contexts_in, replace_refs_in},
};
use threadline_core::{domain::RepositoryName, references::closing_references};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verification {
    Pending,
    Verified,
    PullRequest,
    Missing,
    Offline,
    Inaccessible,
    Failed,
}
impl Verification {
    fn code(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Verified => "verified",
            Self::PullRequest => "pull_request",
            Self::Missing => "missing",
            Self::Offline => "offline",
            Self::Inaccessible => "inaccessible",
            Self::Failed => "failed",
        }
    }
    fn parse(s: &str) -> Result<Self> {
        [
            Self::Pending,
            Self::Verified,
            Self::PullRequest,
            Self::Missing,
            Self::Offline,
            Self::Inaccessible,
            Self::Failed,
        ]
        .into_iter()
        .find(|v| v.code() == s)
        .ok_or(Error::Corrupt)
    }
}
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BodyReference {
    pub position: i64,
    pub raw: String,
    pub provenance: String,
    pub owner: String,
    pub repository: String,
    pub number: u64,
    pub verification: Verification,
    /// Prior proof for this account and body-bound route, retained for local reading after a retryable check.
    pub verified_item: Option<ItemId>,
    pub checked_at_ms: Option<i64>,
}
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ReferenceObservation {
    /// Provider response verified the immutable repository, ordinary issue or PR ID, and exact local route.
    Item {
        repository: RepositoryId,
        item: ItemId,
        issue_id: IssueApiId,
    },
    Missing,
    Offline,
    Inaccessible,
    Failed,
}
pub type PrRevisionContext = (crate::Change<String>, Option<(String, Vec<BodyReference>)>);

impl Store {
    /// Derive and replace candidates from the actual downloaded body. Reading context never changes repository selection.
    pub fn index_pr_body(
        &self,
        account: AccountId,
        pr: ItemId,
        previous_revision: Option<&str>,
        body_revision: &str,
        expected_body_at_ms: i64,
    ) -> Result<Vec<BodyReference>> {
        self.write(|tx|{
            let row:Option<(String,String,Option<String>,Option<i64>)>=tx.query_row(
                "SELECT r.owner,r.name,i.body,i.body_at FROM items i JOIN repositories r ON i.repository_id=r.id AND i.account_id=r.account_id
                 WHERE i.account_id=? AND i.id=? AND i.kind='pull_request'",params![account,pr],
                |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
            let (owner,repository,body,body_at)=row.ok_or(Error::NotFound)?;
            if body_at!=Some(expected_body_at_ms){return Err(Error::Conflict("Downloaded PR body changed before indexing"));}
            let route=RepositoryName::new(&owner,&repository).map_err(|_|Error::Corrupt)?;
            let refs=closing_references(body.as_deref().unwrap_or_default(),&route);
            let candidates=refs.iter().enumerate().map(|(idx,r)|ReferenceCandidate{
                position:idx as i64,raw:r.raw.clone(),
                provenance:format!("PR body: {}/{}#{}",r.repository.owner().to_ascii_lowercase(),r.repository.name().to_ascii_lowercase(),r.number.get()),
                verified:None}).collect::<Vec<_>>();
            replace_refs_in(tx,account,pr,previous_revision,body_revision,&candidates)?;
            let proof_id:i64=tx.query_row(
                "INSERT INTO cached_resources(account_id,item_id,family,coverage,revision,last_validation_at_ms)
                 VALUES(?1,?2,'pr-body-reference-proof',2,?3,?4)
                 ON CONFLICT(item_id,family) WHERE item_id IS NOT NULL DO UPDATE SET
                   coverage=2,revision=excluded.revision,last_validation_at_ms=excluded.last_validation_at_ms
                 RETURNING id",params![account,pr,body_revision,expected_body_at_ms],|row|row.get(0))?;
            tx.execute("INSERT INTO cached_resource_records(resource_id,record_key,body) VALUES(?,'source',?)
                ON CONFLICT(resource_id,record_key) DO UPDATE SET body=excluded.body",
                params![proof_id,encoded(&body)?])?;
            for (index,link) in refs.into_iter().enumerate(){
                let number=i64::try_from(link.number.get()).map_err(|_|Error::InvalidInput("Issue number exceeds the storage range"))?;
                tx.execute("INSERT INTO pr_reference_results(account_id,pr_id,position,owner,repository,number,verification,checked_at)
                    SELECT ?1,?2,?3,?4,?5,?6,CASE WHEN verified_issue IS NULL THEN 'pending' ELSE 'verified' END,verified_at
                    FROM pr_reference_candidates WHERE account_id=?1 AND pr_id=?2 AND position=?3",
                    params![account,pr,index as i64,link.repository.owner(),link.repository.name(),number])?;
            }
            references_in(tx,account,pr)
        })
    }

    /// Read one revision body and only the candidates derived from exactly those downloaded bytes.
    pub fn pr_revision_context(&self, rev: &PrRevision) -> Result<PrRevisionContext> {
        self.read(|db|{
            let tx=db.unchecked_transaction()?;
            validate_revision(&tx,rev)?;
            let (body,stamp)=revision_body_in(&tx,rev)?;
            let Some(stamp)=stamp else{return Ok((body,None))};
            let source=match &body{crate::Change::Value(text)=>Some(text),_=>None};
            let label=tx.query_row(
                "SELECT s.body_revision FROM pr_reference_sets s JOIN cached_resources r ON r.account_id=s.account_id
                 AND r.item_id=s.pr_id AND r.family='pr-body-reference-proof' AND r.coverage=2 AND r.revision=s.body_revision
                 JOIN cached_resource_records d ON d.resource_id=r.id AND d.record_key='source'
                 JOIN items i ON i.id=s.pr_id AND i.account_id=s.account_id AND i.body_at=r.last_validation_at_ms
                 AND json_extract(d.body,'$') IS i.body
                 WHERE s.account_id=? AND s.pr_id=? AND r.last_validation_at_ms=? AND d.body=?",
                params![rev.account,rev.pr,stamp,encoded(&source)?],|row|row.get::<_,String>(0)).optional()?;
            let refs=label.map(|revision|Ok::<_,Error>((revision,references_in(&tx,rev.account,rev.pr)?))).transpose()?;
            Ok((body,refs))
        })
    }

    pub fn pr_reference_set(
        &self,
        account: AccountId,
        pr: ItemId,
    ) -> Result<Option<(String, Vec<BodyReference>)>> {
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            let revision = tx
                .query_row(
                    "SELECT body_revision FROM pr_reference_sets WHERE account_id=? AND pr_id=?",
                    params![account, pr],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            revision
                .map(|rev| Ok((rev, references_in(&tx, account, pr)?)))
                .transpose()
        })
    }

    pub fn pr_body_references(&self, account: AccountId, pr: ItemId) -> Result<Vec<BodyReference>> {
        self.read(|db| references_in(db, account, pr))
    }

    pub fn record_pr_reference(
        &self,
        account: AccountId,
        pr: ItemId,
        body_revision: &str,
        position: i64,
        observation: ReferenceObservation,
        checked_at_ms: i64,
    ) -> Result<Verification> {
        self.write(|tx|{
            let row:Option<(String,String,i64,Option<i64>)>=tx.query_row(
                "SELECT o.owner,o.repository,o.number,o.checked_at FROM pr_reference_results o
                 JOIN pr_reference_sets b USING(account_id,pr_id) WHERE o.account_id=? AND o.pr_id=? AND b.body_revision=? AND o.position=?
                   AND EXISTS(SELECT 1 FROM cached_resources r JOIN cached_resource_records d ON d.resource_id=r.id
                     JOIN items i ON i.id=r.item_id AND i.account_id=r.account_id WHERE r.item_id=o.pr_id
                       AND r.account_id=o.account_id AND r.family='pr-body-reference-proof' AND r.coverage=2
                       AND r.revision=b.body_revision AND r.last_validation_at_ms=i.body_at
                       AND d.record_key='source' AND json_extract(d.body,'$') IS i.body)",
                params![account,pr,body_revision,position],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
            let (owner,name,number,previous)=row.ok_or(Error::Conflict("PR body or reference changed"))?;
            if previous.is_some_and(|t|t>checked_at_ms){return Err(Error::Conflict("Reference already has newer verification"));}
            let (status,verified)=match observation {
                ReferenceObservation::Item{repository,item,issue_id}=>{
                    let kind:Option<String>=tx.query_row(
                        "SELECT i.kind FROM items i JOIN repositories r ON r.id=i.repository_id AND r.account_id=i.account_id
                         JOIN github_repository_access ra ON ra.repository_id=r.id AND ra.account_id=r.account_id
                         JOIN github_access_profiles p ON p.account_id=ra.account_id AND p.owner_scope=ra.owner_scope
                         WHERE i.account_id=?1 AND i.id=?2 AND r.id=?3 AND r.owner=?4 AND r.name=?5 AND i.number=?6 AND i.issue_api_id=?7
                         AND p.owner_scope=r.owner AND p.access=1 AND ra.access=1 AND p.installed_generation=p.credential_generation
                         AND ra.verified_generation=p.credential_generation",
                        params![account,item,repository,owner,name,number,issue_id.get().to_string()],|r|r.get(0)).optional()?;
                    match kind.as_deref(){
                        Some("issue")=>(Verification::Verified,Some(item)),
                        Some("pull_request")=>(Verification::PullRequest,None),
                        _=>return Err(Error::Conflict("Reference needs a verified target in its original repository")),
                    }
                }
                ReferenceObservation::Missing=>(Verification::Missing,None),
                ReferenceObservation::Offline=>(Verification::Offline,None),
                ReferenceObservation::Inaccessible=>(Verification::Inaccessible,None),
                ReferenceObservation::Failed=>(Verification::Failed,None),
            };
            tx.execute("UPDATE pr_reference_results SET verification=?,checked_at=? WHERE account_id=? AND pr_id=? AND position=?",
                params![status.code(),checked_at_ms,account,pr,position])?;
            // An offline/retry condition preserves past verification for local reading.
            if matches!(status,Verification::Verified|Verification::PullRequest|Verification::Missing) {
                tx.execute("UPDATE pr_reference_candidates SET verified_issue=?,verified_at=? WHERE account_id=? AND pr_id=? AND position=?",
                    params![verified,verified.map(|_|checked_at_ms),account,pr,position])?;
            }
            if matches!(status,Verification::Missing|Verification::PullRequest) { refresh_contexts_in(tx,account,pr,body_revision)?; }
            Ok(status)
        })
    }
}
fn references_in(db: &Connection, account: AccountId, pr: ItemId) -> Result<Vec<BodyReference>> {
    let mut q=db.prepare("SELECT v.position,c.raw_reference,c.provenance,v.owner,v.repository,v.number,v.verification,c.verified_issue,v.checked_at
        FROM pr_reference_results v JOIN pr_reference_candidates c USING(account_id,pr_id,position)
        WHERE v.account_id=? AND v.pr_id=? ORDER BY v.position")?;
    let mut rows = q.query(params![account, pr])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        let code: String = row.get(6)?;
        result.push(BodyReference {
            position: row.get(0)?,
            raw: row.get(1)?,
            provenance: row.get(2)?,
            owner: row.get(3)?,
            repository: row.get(4)?,
            number: u64::try_from(row.get::<_, i64>(5)?).map_err(|_| Error::Corrupt)?,
            verification: Verification::parse(&code)?,
            verified_item: row.get(7)?,
            checked_at_ms: row.get(8)?,
        });
    }
    Ok(result)
}
