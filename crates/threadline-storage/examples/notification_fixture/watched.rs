use super::owned_activity;
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use std::{error::Error, path::Path};
use threadline_storage::{
    RepositoryId, Store, UtcMillis,
    access::AccessProfile,
    history::{HistoryEntry, HistoryMonitoring, HistoryRef, HistoryView, RefMovement},
    notifications::{Category, DeliveryMode, Destination, EventFacts, NotificationTarget},
    resources::{CommitOid, GraphCommit},
};

pub(super) const MARKER: &str = "threadline-s11-watched-branch-v1";
const BRANCH: &str = "feature/fixture-search";
type Result<T> = std::result::Result<T, Box<dyn Error>>;
fn oid(c: char) -> CommitOid {
    CommitOid::parse(&c.to_string().repeat(40)).expect("fixed hexadecimal fixture OID")
}
fn entry(c: char, parent: Option<char>, at: i64) -> HistoryEntry {
    HistoryEntry {
        commit: GraphCommit {
            oid: oid(c),
            title: format!("Fixture commit {c}"),
            committed_at_ms: at,
            parents: parent.into_iter().map(oid).collect(),
        },
        message: format!("Fixture commit {c}"),
        author: None,
        authored_at_ms: Some(at),
    }
}
pub(super) fn seed(store: &Store, p: &AccessProfile, repo: RepositoryId) -> Result<()> {
    let branch = HistoryRef::Branch(BRANCH.into());
    let now = UtcMillis::now()?.0;
    store.set_history_monitoring(
        p.account,
        repo,
        &HistoryMonitoring {
            branches: [BRANCH.into()].into(),
            ..Default::default()
        },
    )?;
    store.update_notification_settings(p.account, |s| {
        s.categories
            .insert(Category::BranchUpdate, DeliveryMode::Popup);
    })?;
    if store.observe_history_ref(p, repo, &branch, Some(&oid('a')), now)? != RefMovement::Baseline {
        return Err("Unexpected initial reference movement".into());
    }
    let walk = store
        .begin_history_page(p, repo, &branch)?
        .ok_or("Missing baseline walk")?;
    store.commit_history_page(&walk, &[entry('a', None, now)], None, true, now)?;
    store.observe_history_ref(p, repo, &branch, Some(&oid('d')), now + 4)?;
    let walk = store
        .begin_history_page(p, repo, &branch)?
        .ok_or("Missing new reference walk")?;
    store.commit_history_page(
        &walk,
        &[
            entry('d', Some('c'), now + 3),
            entry('c', Some('b'), now + 2),
            entry('b', Some('a'), now + 1),
        ],
        None,
        true,
        now + 4,
    )?;
    Ok(())
}
pub(super) fn inspect(root: &Path) -> Result<Value> {
    let (mut db, account, repo) = owned_activity(root, MARKER)?;
    let tx = db.transaction()?;
    let target = NotificationTarget::Comparison {
        reference: HistoryRef::Branch(BRANCH.into()).key()?,
        base: oid('a'),
        head: oid('d'),
    };
    let mut commits = Vec::new();
    let mut q=tx.prepare("SELECT a.revision,n.facts FROM notification_events n JOIN activity_events a USING(account_id,event_key)
        WHERE n.account_id=? AND a.repository_id=? AND a.event_kind='branch_commit_observed' ORDER BY a.revision")?;
    for row in q.query_map(rusqlite::params![account, repo], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })? {
        let (commit, raw) = row?;
        let facts: EventFacts = serde_json::from_str(&raw)?;
        if facts.target != target || facts.categories != [Category::BranchUpdate].into() {
            return Err("Wrong watched-branch destination".into());
        }
        commits.push(commit);
    }
    if commits != ['b', 'c', 'd'].map(|c| oid(c).as_str().to_string()) {
        return Err("Expected the three proven successors of the watched baseline".into());
    }
    let (events, feed, read, grouped): (u32, u32, u32, u32) = tx.query_row(
        "SELECT count(*),sum(published),count(read_at),count(group_id) FROM notification_events",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let (groups,native_id,outcome,activated,destination):(u32,Option<String>,Option<String>,bool,Option<String>)=tx.query_row("SELECT count(*),min(native_id),min(state),count(activated_at)>0,min(destination) FROM native_notification_groups",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    if let Some(raw) = destination {
        let dest: Destination = serde_json::from_str(&raw)?;
        if dest.repository != Some(repo) || dest.target != target {
            return Err("The native group refers to a different comparison".into());
        }
    }
    let raw:Option<String>=tx.query_row("SELECT state FROM history_views WHERE account_id=? AND repo=? AND view_id='reader-commit-history'",
        rusqlite::params![account,repo],|r|r.get(0)).optional()?;
    let view = raw
        .map(|raw| serde_json::from_str::<HistoryView>(&raw))
        .transpose()?;
    let routed = view.is_some_and(|v| {
        v.reference == HistoryRef::Branch(BRANCH.into())
            && v.tip == oid('d')
            && v.selected == Some(oid('d'))
            && v.second == Some(oid('a'))
    });
    Ok(
        json!({"branch":BRANCH,"commits":commits,"base":oid('a'),"head":oid('d'),"events":events,"published":feed,"read":read,
        "grouped":grouped,"groups":groups,"native_id":native_id,"outcome":outcome,"activated":activated,"comparison_saved":routed}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn watched_baseline_emits_only_three_new_commits_with_one_pinned_comparison() {
        let dir = tempfile::tempdir().unwrap();
        let initial = crate::seed(dir.path(), crate::Fixture::Watched).unwrap();
        assert_eq!(initial["events"], 3);
        assert_eq!(initial["groups"], 0);
        let (_, a, _) = owned_activity(dir.path(), MARKER).unwrap();
        let store = Store::open_profile(dir.path()).unwrap();
        let now = UtcMillis::now().unwrap().0 + 5;
        store
            .process_notification_events(a, now, "UTC", true, 10)
            .unwrap();
        let group = store
            .claim_notification_popup(a, now, "UTC", true)
            .unwrap()
            .unwrap();
        assert_eq!(group.event_count, 3);
        let after = inspect(dir.path()).unwrap();
        assert_eq!(after["grouped"], 3);
        assert_eq!(after["comparison_saved"], false);
    }
}
