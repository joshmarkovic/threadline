use super::*;
use crate::{
    access::{AccessProfile, profile},
    history::{
        HistoryEntry, HistoryRef, HistoryWalk, RefMovement, commit_history_page_in, observations,
    },
    resources::CommitOid,
};
use std::collections::{BTreeMap, BTreeSet};

const OBSERVER: &str = "history-monitor";
const BRANCHES: &str = "history:branches";
const REF: &str = "history:ref:";
const WALK: &str = "history:walk:";
const MAX_BRANCH_PAGES: u32 = 8;
const RECENT_ANCESTRY_PAGES: u32 = 2;

pub struct MonitoredBranches<'a> {
    pub rows: &'a [(String, CommitOid)],
    pub next: Option<u32>,
    pub partial: bool,
}

pub struct MonitoredAncestors<'a> {
    pub walk: Option<&'a HistoryWalk>,
    pub rows: &'a [HistoryEntry],
    pub next: Option<&'a str>,
    pub partial: bool,
}

pub enum HistoryRead {
    Branches,
    Reference(HistoryRef),
    Ancestors(HistoryRef),
}

impl HistoryRead {
    pub fn from_target(target: &ReadTarget) -> Option<Self> {
        if target.family != ResourceFamily::History || target.item.is_some() {
            return None;
        }
        if target.revision == BRANCHES {
            return Some(Self::Branches);
        }
        if let Some(reference) = target.revision.strip_prefix(REF) {
            return HistoryRef::from_key(reference).map(Self::Reference);
        }
        if let Some(reference) = target.revision.strip_prefix(WALK) {
            return HistoryRef::from_key(reference).map(Self::Ancestors);
        }
        None
    }
}

fn target(repository: RepositoryId, revision: String) -> ReadTarget {
    ReadTarget {
        repository,
        item: None,
        family: ResourceFamily::History,
        revision,
    }
}

pub(super) fn enroll(tx: &Transaction<'_>, account: AccountId, now: i64) -> Result<()> {
    let mut repositories = tx.prepare(
        "SELECT repository_id FROM github_repository_access WHERE account_id=? AND selected=1",
    )?;
    let repositories = repositories
        .query_map([account], |row| row.get::<_, RepositoryId>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut desired = BTreeSet::new();
    let mut settings = BTreeMap::new();
    for repository in repositories {
        let monitoring = observations::settings(tx, account, repository)?;
        let branches = monitoring.default_branch.iter().chain(&monitoring.branches);
        let refs = branches.cloned().map(HistoryRef::Branch).chain(
            monitoring
                .followed_prs
                .iter()
                .copied()
                .map(HistoryRef::FollowedPr),
        );
        for reference in refs {
            let revision = format!("{REF}{}", reference.key()?);
            if revision.len() <= 512 {
                desired.insert((repository.0, revision));
            }
        }
        if monitoring.default_branch.is_some()
            || !monitoring.branches.is_empty()
            || !monitoring.branch_patterns.is_empty()
        {
            desired.insert((repository.0, BRANCHES.to_owned()));
        }
        settings.insert(repository.0, monitoring);
    }
    let mut active = tx.prepare(
        "SELECT j.id,j.repository_id,j.revision FROM scheduled_reads j
         JOIN sync_subscriptions s ON s.read_id=j.id AND s.subscriber=?1
         WHERE j.account_id=?2",
    )?;
    let active = active
        .query_map(params![OBSERVER, account], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, RepositoryId>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, repository, revision) in active {
        let retain = if let Some(key) = revision.strip_prefix(WALK) {
            settings.get(&repository.0).is_some_and(|settings| {
                HistoryRef::from_key(key).is_some_and(|reference| settings.watches(&reference))
            })
        } else {
            desired.contains(&(repository.0, revision))
        };
        if !retain {
            retire(tx, id)?;
        }
    }
    for (repository, revision) in desired {
        subscribe_in(
            tx,
            account,
            &target(RepositoryId(repository), revision),
            OBSERVER,
            ReadPriority::Recent,
            now,
        )?;
    }
    Ok(())
}

fn retire(tx: &Transaction<'_>, id: i64) -> Result<()> {
    tx.execute(
        "DELETE FROM sync_subscriptions WHERE read_id=? AND subscriber=?",
        params![id, OBSERVER],
    )?;
    tx.execute(
        "UPDATE scheduled_reads SET lease_until=NULL,leased_generation=NULL,cursor=NULL,walk_started=NULL,
            failures=0,failure=NULL,suspended=0
         WHERE id=?1 AND NOT EXISTS(SELECT 1 FROM sync_subscriptions WHERE read_id=?1)",
        [id],
    )?;
    Ok(())
}

fn current_profile(tx: &Transaction<'_>, claim: &ReadClaim, now: i64) -> Result<AccessProfile> {
    require_claim(tx, claim, now)?;
    let owned: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sync_subscriptions WHERE read_id=? AND subscriber=?)",
        params![claim.id, OBSERVER],
        |row| row.get(0),
    )?;
    if !owned {
        return Err(Error::Conflict(
            "The monitored history subscription changed",
        ));
    }
    let owner: String = tx.query_row(
        "SELECT owner_scope FROM github_repository_access WHERE account_id=? AND repository_id=?",
        params![claim.account, claim.target.repository],
        |row| row.get(0),
    )?;
    profile(tx, claim.account, &owner)?.ok_or(Error::NotFound)
}

fn schedule_ancestry(
    tx: &Transaction<'_>,
    claim: &ReadClaim,
    reference: &HistoryRef,
    now: i64,
) -> Result<()> {
    let revision = format!("{WALK}{}", reference.key()?);
    if revision.len() <= 512 {
        let id = subscribe_in(
            tx,
            claim.account,
            &target(claim.target.repository, revision),
            OBSERVER,
            ReadPriority::Recent,
            now,
        )?;
        // A new head has its own immutable checkpoint. A previous lease cannot publish against it.
        tx.execute(
            "UPDATE scheduled_reads SET due_at=?2,lease_until=NULL,leased_generation=NULL,
                walk_started=NULL,failures=0,failure=NULL,suspended=0 WHERE id=?1",
            params![id, now],
        )?;
    }
    Ok(())
}

fn started_at(tx: &Transaction<'_>, claim: &ReadClaim) -> Result<i64> {
    Ok(tx.query_row(
        "SELECT last_started FROM scheduled_reads WHERE id=?",
        [claim.id],
        |row| row.get(0),
    )?)
}

fn observe(
    tx: &Transaction<'_>,
    claim: &ReadClaim,
    profile: &AccessProfile,
    reference: &HistoryRef,
    oid: &CommitOid,
    now: i64,
) -> Result<bool> {
    let movement = observations::observe_ref_in(
        tx,
        profile,
        claim.target.repository,
        reference,
        Some(oid),
        now,
    )?;
    let changed = !matches!(movement, RefMovement::Stale | RefMovement::Unchanged);
    if changed {
        schedule_ancestry(tx, claim, reference, now)?;
    }
    Ok(changed)
}

fn required_target(claim: &ReadClaim) -> Result<HistoryRead> {
    HistoryRead::from_target(&claim.target).ok_or(Error::Conflict("Unexpected history read target"))
}

impl Store {
    /// Revalidate the durable monitor subscription immediately before its next provider read.
    pub fn monitored_history_target(&self, claim: &ReadClaim, now: i64) -> Result<HistoryRead> {
        let planned = required_target(claim)?;
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            current_profile(&tx, claim, now)?;
            let settings = observations::settings(&tx, claim.account, claim.target.repository)?;
            let current = match &planned {
                HistoryRead::Branches => {
                    settings.default_branch.is_some()
                        || !settings.branches.is_empty()
                        || !settings.branch_patterns.is_empty()
                }
                HistoryRead::Reference(reference) | HistoryRead::Ancestors(reference) => {
                    settings.watches(reference)
                }
            };
            if !current {
                return Err(Error::Conflict(
                    "The monitored history subscription changed",
                ));
            }
            Ok(planned)
        })
    }

    /// Persist a branch page and its checkpoint. Only a complete single-page inventory proves absence.
    pub fn commit_history_branches(
        &self,
        claim: &ReadClaim,
        page: MonitoredBranches<'_>,
        now: i64,
        system_zone: &str,
    ) -> Result<bool> {
        let MonitoredBranches {
            rows,
            next,
            partial,
        } = page;
        if !matches!(required_target(claim)?, HistoryRead::Branches) || rows.len() > 100 {
            return Err(Error::InvalidInput("Invalid monitored branch page"));
        }
        let page = claim
            .cursor
            .as_deref()
            .map_or(Ok(1), |s| s.parse::<u32>().map_err(|_| Error::Corrupt))?;
        if next.is_some_and(|n| page.checked_add(1) != Some(n)) {
            return Err(Error::InvalidInput(
                "The monitored branch page did not advance",
            ));
        }
        self.write(|tx| {
            let profile = current_profile(tx, claim, now)?;
            let monitoring = observations::settings(tx, claim.account, claim.target.repository)?;
            if monitoring.default_branch.is_none() && monitoring.branches.is_empty() && monitoring.branch_patterns.is_empty() {
                return Err(Error::Conflict("The branch inventory subscription changed"));
            }
            let started = started_at(tx, claim)?;
            let mut changed = false;
            for (name, oid) in rows {
                let reference = HistoryRef::Branch(name.clone());
                if monitoring.watches(&reference) {
                    changed |= observe(tx, claim, &profile, &reference, oid, started)?;
                }
            }
            if page == 1 && next.is_none() && !partial {
                let mut known = tx.prepare("SELECT name FROM cached_refs WHERE repository_id=? AND oid IS NOT NULL
                    AND name GLOB 'refs/heads/*' AND observed_at_ms<?")?;
                let known = known.query_map(params![claim.target.repository, started], |row| row.get::<_,String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                for name in known {
                    let Some(HistoryRef::Branch(branch)) = HistoryRef::from_key(&name) else { continue };
                    if rows.iter().any(|(known,_)| known == &branch) { continue; }
                    let reference = HistoryRef::Branch(branch);
                    if monitoring.watches(&reference) {
                        let movement = observations::observe_ref_in(tx,&profile,claim.target.repository,
                            &reference,None,started)?;
                        if movement == RefMovement::Deleted {
                            changed = true;
                            let revision = format!("{WALK}{}", reference.key()?);
                            if let Some(id) = target_id(tx, claim.account, &target(claim.target.repository, revision))? {
                                retire(tx, id)?;
                            }
                        }
                    }
                }
            }
            if let Some(next) = next.filter(|next| !partial && *next <= MAX_BRANCH_PAGES) {
                tx.execute(
                    "UPDATE scheduled_reads SET cursor=?1,lease_until=NULL,leased_generation=NULL,due_at=?2 WHERE id=?3",
                    params![next.to_string(), now, claim.id],
                )?;
            } else {
                complete_in(tx, claim, now, system_zone, &Validators::default())?;
            }
            Ok(changed)
        })
    }

    /// Commit only a positively identified ref. A provider denial leaves the last known tip intact.
    pub fn commit_history_ref(
        &self,
        claim: &ReadClaim,
        oid: Option<&CommitOid>,
        now: i64,
        system_zone: &str,
    ) -> Result<bool> {
        let HistoryRead::Reference(reference) = required_target(claim)? else {
            return Err(Error::Conflict("Unexpected monitored reference result"));
        };
        self.write(|tx| {
            let profile = current_profile(tx, claim, now)?;
            if !observations::settings(tx, claim.account, claim.target.repository)?
                .watches(&reference)
            {
                return Err(Error::Conflict("The monitored reference was removed"));
            }
            let changed = if let Some(oid) = oid {
                observe(tx, claim, &profile, &reference, oid, started_at(tx, claim)?)?
            } else {
                false
            };
            complete_in(tx, claim, now, system_zone, &Validators::default())?;
            Ok(changed)
        })
    }

    pub fn prepare_history_ancestors(
        &self,
        claim: &ReadClaim,
        now: i64,
    ) -> Result<Option<HistoryWalk>> {
        let HistoryRead::Ancestors(reference) = required_target(claim)? else {
            return Err(Error::Conflict("Unexpected monitored ancestry request"));
        };
        let profile = self.read(|db| {
            let tx = db.unchecked_transaction()?;
            let profile = current_profile(&tx, claim, now)?;
            if !observations::settings(&tx, claim.account, claim.target.repository)?
                .watches(&reference)
            {
                return Err(Error::Conflict("The monitored reference was removed"));
            }
            Ok(profile)
        })?;
        self.begin_history_page(&profile, claim.target.repository, &reference)
    }

    /// Commit immutable ancestry and release or reschedule its one-use durable work together.
    pub fn commit_history_ancestors(
        &self,
        claim: &ReadClaim,
        page: MonitoredAncestors<'_>,
        now: i64,
        system_zone: &str,
    ) -> Result<bool> {
        let MonitoredAncestors {
            walk,
            rows,
            next,
            partial,
        } = page;
        let HistoryRead::Ancestors(reference) = required_target(claim)? else {
            return Err(Error::Conflict("Unexpected monitored ancestry result"));
        };
        if walk.is_none() && (!rows.is_empty() || next.is_some() || partial)
            || partial && next.is_some()
        {
            return Err(Error::InvalidInput("Invalid monitored ancestry page"));
        }
        self.write(|tx| {
            current_profile(tx, claim, now)?;
            if !observations::settings(tx, claim.account, claim.target.repository)?.watches(&reference) {
                return Err(Error::Conflict("The monitored reference was removed"));
            }
            if let Some(walk) = walk {
                if walk.reference != reference || walk.repository != claim.target.repository {
                    return Err(Error::Conflict("Ancestry result belongs to another monitored reference"));
                }
                commit_history_page_in(tx, walk, rows, next, next.is_none() && !partial, now)?;
            }
            let page = walk.and_then(|walk| walk.cursor.as_deref()).map_or(Ok(1), |s| s.parse::<u32>().map_err(|_|Error::Corrupt))?;
            if next.is_some() && page < RECENT_ANCESTRY_PAGES {
                tx.execute(
                    "UPDATE scheduled_reads SET lease_until=NULL,leased_generation=NULL,due_at=?1 WHERE id=?2",
                    params![now, claim.id],
                )?;
            } else {
                complete_in(tx, claim, now, system_zone, &Validators::default())?;
                retire(tx, claim.id)?;
            }
            Ok(!rows.is_empty())
        })
    }
}
