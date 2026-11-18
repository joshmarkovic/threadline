use super::*;
use std::collections::BTreeSet;

const BURST_MS: i64 = 30_000;
const CATCH_UP_MS: i64 = 15 * 60_000;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Destination {
    pub repository: Option<RepositoryId>,
    pub target: NotificationTarget,
}
#[derive(Clone, Debug)]
pub struct FeedEntry {
    pub id: i64,
    pub event_key: String,
    pub event_kind: String,
    pub occurred_at_ms: Option<i64>,
    pub revision: Option<String>,
    pub repository: RepositoryId,
    pub target: NotificationTarget,
    pub categories: BTreeSet<Category>,
    pub decision: Decision,
    pub read_at_ms: Option<i64>,
    pub observed_at_ms: i64,
}
#[derive(Clone, Debug)]
pub struct PopupGroup {
    pub native_id: String,
    pub destination: Destination,
    pub event_count: u32,
    pub sound: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PopupOutcome {
    Delivered,
    Unknown,
    Rejected,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationAction {
    Open,
    MarkRead,
    SnoozeUntil(i64),
}
#[derive(Debug)]
struct Event {
    id: i64,
    key: String,
    repo: RepositoryId,
    item: Option<ItemId>,
    facts: EventFacts,
    kind: String,
    revision: Option<String>,
    occurred: Option<i64>,
    at: i64,
}
type RawEvent = (
    i64,
    String,
    RepositoryId,
    Option<ItemId>,
    String,
    Option<String>,
    i64,
    Option<i64>,
    Option<String>,
);
fn collect_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawEvent> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    ))
}
fn fallback(db: &Connection, item: Option<ItemId>, kind: &str) -> Result<EventFacts> {
    let class = match kind {
        "comment_created" | "comment_edited" => Some(Category::OrdinaryComment),
        "inline_comment_created" | "inline_comment_edited" => Some(Category::InlineComment),
        "review_requested" => Some(Category::ReviewRequest),
        "check_completed" | "commit_status" => Some(Category::CheckStatus),
        "branch_commit_observed" => Some(Category::BranchUpdate),
        "item_state_changed" => Some(Category::IssueState),
        "item_created" if item.is_some() => {
            let pr: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM items WHERE id=? AND kind='pull_request')",
                [item],
                |row| row.get(0),
            )?;
            if pr {
                Some(Category::PrOpened)
            } else {
                Some(Category::IssueState)
            }
        }
        _ => None,
    };
    Ok(EventFacts {
        target: item.map_or(NotificationTarget::Repository, |item| {
            NotificationTarget::Item { item, thread: None }
        }),
        categories: class.into_iter().collect(),
        actor: EventActor::default(),
        involves_me: false,
        receipt: None,
    })
}
fn event(db: &Connection, raw: RawEvent) -> Result<Event> {
    let (id, key, repo, item, kind, facts, at, occurred, revision) = raw;
    let facts = match facts {
        Some(s) => decode(&s)?,
        None => fallback(db, item, &kind)?,
    };
    Ok(Event {
        id,
        key,
        repo,
        item,
        facts,
        kind,
        revision,
        occurred,
        at,
    })
}
const EVENT_COLUMNS:&str="SELECT n.id,n.event_key,e.repository_id,e.item_id,e.event_kind,n.facts,e.observed_at_ms,e.occurred_at_ms,e.revision
    FROM notification_events n JOIN activity_events e ON e.account_id=n.account_id AND e.event_key=n.event_key";
fn dispatch_allowed(db: &Connection, a: AccountId) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM active_profile a JOIN dispatch_profiles d USING(account_id)
        WHERE a.account_id=? AND d.authorized=1 AND d.offline=0)",
        [a],
        |r| r.get(0),
    )?)
}
fn receipt_proves_own(db: &Connection, a: AccountId, e: &Event) -> Result<bool> {
    let Some(item) = e.item else { return Ok(false) };
    let own=match &e.facts.receipt{
        Some(ReceiptMatch::Ordinary{action,remote_id,revision})=>db.query_row(
            "SELECT EXISTS(SELECT 1 FROM own_write_actions WHERE account_id=? AND repository_id=? AND item_id=? AND action=? AND remote_id=? AND remote_revision=?)",
            params![a,e.repo,item,action,remote_id,revision],|r|r.get(0))?,
        Some(ReceiptMatch::Review{step,remote_id,head})=>db.query_row(
            "SELECT EXISTS(SELECT 1 FROM review_activity WHERE account_id=? AND item_id=? AND step=? AND remote_id=? AND head=?)",
            params![a,item,step,remote_id,head.as_str()],|r|r.get(0))?,
        Some(ReceiptMatch::Merge{head,merge_oid})=>db.query_row(
            "SELECT EXISTS(SELECT 1 FROM confirmed_merges WHERE account_id=? AND repository_id=? AND item_id=? AND head=? AND merge_oid=? AND own_action=1)",
            params![a,e.repo,item,head.as_str(),merge_oid.as_str()],|r|r.get(0))?,
        None=>false,
    };
    Ok(own)
}
fn decide(
    db: &Connection,
    a: AccountId,
    e: &Event,
    settings: &NotificationSettings,
    now: i64,
    zone: &str,
    os: bool,
) -> Result<Decision> {
    let group = group_key(e.repo, &e.facts.target)?;
    let(mut followed,until):(bool,Option<i64>)=db.query_row("SELECT followed,snooze_until FROM notification_targets WHERE account_id=? AND repository_id=? AND group_key=?",
        params![a,e.repo,group],|r|Ok((r.get(0)?,r.get::<_,Option<i64>>(1)?))).optional()?.unwrap_or_default();
    let selected:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM eligible_sync_repositories WHERE account_id=? AND repository_id=?)",params![a,e.repo],|r|r.get(0))?;
    let repo = rules(db, a, e.repo)?;
    if !selected {
        return Ok(Decision {
            mode: DeliveryMode::Off,
            eligible: BTreeSet::new(),
            reasons: vec![DecisionReason::RepositoryUnavailable],
            sound: false,
        });
    }
    let mut facts = e.facts.clone();
    if let Some(item) = e.item {
        let (pinned,involved):(bool,bool)=db.query_row(
            "SELECT EXISTS(SELECT 1 FROM reader_marks WHERE item_id=?1 AND pinned=1),
                    EXISTS(SELECT 1 FROM reader_facet_values v JOIN github_repository_access ra ON ra.repository_id=?2
                      JOIN github_access_profiles p USING(account_id,owner_scope)
                      WHERE v.item_id=?1 AND ra.account_id=?3 AND v.category IN ('author','assignee','reviewer','participant','mention')
                      AND v.value=p.login COLLATE NOCASE)", params![item,e.repo,a],|r|Ok((r.get(0)?,r.get(1)?)))?;
        followed |= pinned;
        facts.involves_me |= involved;
    }
    let actor_own = if let Some(user) = e.facts.actor.user_id {
        db.query_row(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=? AND provider_user_id=?)",
            params![a, user.to_string()],
            |r| r.get(0),
        )?
    } else {
        false
    };
    let quiet_override: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sync_settings WHERE account_id=? AND notification_quiet=1)",
        [a],
        |r| r.get(0),
    )?;
    let quiet = settings
        .quiet_hours
        .as_ref()
        .map(|hours| hours.schedule().in_work_window(now, zone))
        .transpose()
        .map_err(Error::InvalidInput)?
        .unwrap_or(false);
    Ok(preview(
        settings,
        &repo,
        &facts,
        DeliveryContext {
            own: actor_own || receipt_proves_own(db, a, e)?,
            followed,
            snoozed: until.is_some_and(|t| t > now),
            quiet: quiet || quiet_override,
            os_allows_popup: os,
        },
    ))
}
fn save_decision(tx: &Transaction<'_>, event: &Event, decision: &Decision) -> Result<()> {
    tx.execute(
        "UPDATE notification_events SET decision=?,published=? WHERE id=?",
        params![
            encode(decision)?,
            decision.mode != DeliveryMode::Off,
            event.id
        ],
    )?;
    Ok(())
}
fn catchup(at: i64, now: i64) -> bool {
    at < now.saturating_sub(CATCH_UP_MS)
}
fn destination(e: &Event) -> Destination {
    Destination {
        repository: Some(e.repo),
        target: e.facts.target.clone(),
    }
}
fn combine_destination(old: Destination, new: Destination) -> Destination {
    if old == new {
        return old;
    }
    if old.repository.is_some() && old.repository == new.repository {
        let target = match (&old.target, &new.target) {
            (
                NotificationTarget::Item { item: a, .. },
                NotificationTarget::Item { item: b, .. },
            ) if a == b => Some(NotificationTarget::Item {
                item: *a,
                thread: None,
            }),
            (
                NotificationTarget::Comparison {
                    reference: a,
                    base,
                    head: previous,
                },
                NotificationTarget::Comparison {
                    reference: b,
                    base: next,
                    head,
                },
            ) if a == b && previous == next => Some(NotificationTarget::Comparison {
                reference: a.clone(),
                base: base.clone(),
                head: head.clone(),
            }),
            _ => None,
        };
        if let Some(target) = target {
            return Destination {
                repository: old.repository,
                target,
            };
        }
    }
    Destination {
        repository: None,
        target: NotificationTarget::ActivityFeed,
    }
}
fn attach_group(tx: &Transaction<'_>, a: AccountId, e: &Event, now: i64) -> Result<i64> {
    let observed_time = if e.kind == "branch_commit_observed" {
        e.at
    } else {
        e.occurred.unwrap_or(e.at)
    };
    let key = if catchup(observed_time, now) {
        "catch-up".into()
    } else {
        group_key(e.repo, &e.facts.target)?
    };
    let prior=tx.query_row("SELECT id,destination FROM native_notification_groups WHERE account_id=? AND group_key=?
            AND state='pending' AND (?='catch-up' OR created_at>=?) ORDER BY id DESC LIMIT 1",
        params![a,key,key,now.saturating_sub(BURST_MS)],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?))).optional()?;
    let mut next = destination(e);
    let id = if let Some((id, old)) = prior {
        let old: Destination = decode(&old)?;
        if old != next {
            next = combine_destination(old, next);
            tx.execute(
                "UPDATE native_notification_groups SET destination=? WHERE id=?",
                params![encode(&next)?, id],
            )?;
        }
        id
    } else {
        tx.execute("INSERT INTO native_notification_groups(account_id,group_key,destination,created_at,changed_at) VALUES(?,?,?,?,?)",
            params![a,key,encode(&next)?,now,now])?;
        tx.last_insert_rowid()
    };
    tx.execute(
        "UPDATE notification_events SET group_id=? WHERE id=?",
        params![id, e.id],
    )?;
    Ok(id)
}
fn journal(
    tx: &Transaction<'_>,
    a: AccountId,
    e: &Event,
    decision: &Decision,
    group: Option<i64>,
    phase: i64,
    now: i64,
) -> Result<()> {
    let reason = decision
        .reasons
        .iter()
        .map(|r| r.description())
        .collect::<Vec<_>>()
        .join(" ");
    tx.execute("INSERT INTO notification_delivery(account_id,event_key,phase,group_key,native_id,reason,updated_at_ms)
        VALUES(?1,?2,?3,(SELECT group_key FROM native_notification_groups WHERE id=?4),
        (SELECT native_id FROM native_notification_groups WHERE id=?4),?5,?6)
        ON CONFLICT(account_id,event_key) DO UPDATE SET phase=excluded.phase,group_key=excluded.group_key,
        native_id=excluded.native_id,reason=excluded.reason,updated_at_ms=excluded.updated_at_ms
        WHERE notification_delivery.phase NOT IN (2,3)",
        params![a,e.key,phase,group,reason,now])?;
    Ok(())
}
impl Store {
    pub fn notification_work_pending(&self, account: AccountId) -> Result<bool> {
        self.read(|db| {
            if !dispatch_allowed(db,account)? {return Ok(false)}
            Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM notification_events WHERE account_id=?1 AND decision IS NULL)
                  OR EXISTS(SELECT 1 FROM native_notification_groups WHERE account_id=?1 AND state='pending')",
                [account],|r|r.get(0))?)
        })
    }
    pub fn process_notification_events(
        &self,
        a: AccountId,
        now: i64,
        zone: &str,
        os_allows_popup: bool,
        limit: u16,
    ) -> Result<usize> {
        if !(1..=512).contains(&limit) {
            return Err(Error::InvalidInput("Invalid notification batch size"));
        }
        self.write(|tx| {
            if !dispatch_allowed(tx, a)? {
                return Ok(0);
            }
            let settings = settings(tx, a)?;
            let sql = format!(
                "{EVENT_COLUMNS} WHERE n.account_id=? AND n.decision IS NULL ORDER BY n.id LIMIT ?"
            );
            let mut q = tx.prepare(&sql)?;
            let rows = q
                .query_map(params![a, limit], collect_event)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let count = rows.len();
            for row in rows {
                let e = event(tx, row)?;
                let decision = decide(tx, a, &e, &settings, now, zone, os_allows_popup)?;
                save_decision(tx, &e, &decision)?;
                let group = if decision.mode == DeliveryMode::Popup {
                    Some(attach_group(tx, a, &e, now)?)
                } else {
                    None
                };
                journal(
                    tx,
                    a,
                    &e,
                    &decision,
                    group,
                    if group.is_some() { 0 } else { 1 },
                    now,
                )?;
            }
            Ok(count)
        })
    }
    /// Reserve before crossing the native boundary. An interrupted delivery remains uncertain after restart.
    pub fn claim_notification_popup(
        &self,
        a: AccountId,
        now: i64,
        zone: &str,
        os_allows_popup: bool,
    ) -> Result<Option<PopupGroup>> {
        self.write(|tx| {
            if !dispatch_allowed(tx, a)? {
                return Ok(None);
            }
            let settings = settings(tx, a)?;
            // Collect all eligible members before returning a catch-up digest across bounded processing batches.
            let more: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM notification_events WHERE account_id=? AND decision IS NULL)",
                [a],
                |r| r.get(0),
            )?;
            if more {
                return Ok(None);
            }
            loop {
                let group=tx.query_row(
                            "SELECT id,native_id FROM native_notification_groups WHERE account_id=? AND state='pending' ORDER BY id LIMIT 1",
                            [a], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)),
                        ).optional()?;
                let Some((id, native_id)) = group else {
                    return Ok(None);
                };
                let mut count = 0u32;
                let mut sound = false;
                let mut route: Option<Destination> = None;
                let mut last_id = 0;
                let sql = format!(
                    "{EVENT_COLUMNS} WHERE n.account_id=? AND n.group_id=? AND n.id>? ORDER BY n.id LIMIT 128"
                );
                let mut query = tx.prepare(&sql)?;
                loop {
                    let rows = query
                        .query_map(params![a, id, last_id], collect_event)?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    if rows.is_empty() {
                        break;
                    }
                    for raw in rows {
                        let event = event(tx, raw)?;
                        last_id = event.id;
                        let decision = decide(tx, a, &event, &settings, now, zone, os_allows_popup)?;
                        save_decision(tx, &event, &decision)?;
                        let read: bool = tx.query_row(
                            "SELECT read_at IS NOT NULL FROM notification_events WHERE id=?",
                            [event.id],
                            |r| r.get(0),
                        )?;
                        if read || decision.mode != DeliveryMode::Popup {
                            tx.execute(
                                "UPDATE notification_events SET group_id=NULL WHERE id=?",
                                [event.id],
                            )?;
                            journal(tx, a, &event, &decision, None, 1, now)?;
                            continue;
                        }
                        count = count.checked_add(1).ok_or(Error::Corrupt)?;
                        sound |= decision.sound;
                        let dest = destination(&event);
                        route = Some(match route {
                            Some(old) => combine_destination(old, dest),
                            None => dest,
                        });
                        journal(tx, a, &event, &decision, Some(id), 3, now)?;
                    }
                }
                if let Some(destination) = route {
                    tx.execute("UPDATE native_notification_groups SET state='unknown',changed_at=?,destination=? WHERE id=?",
                                params![now,encode(&destination)?,id])?;
                    return Ok(Some(PopupGroup {
                        native_id,
                        destination,
                        event_count: count,
                        sound,
                    }));
                }
                tx.execute(
                    "UPDATE native_notification_groups SET state='withdrawn',changed_at=? WHERE id=?",
                    params![now, id],
                )?;
            }
        })
    }
    pub fn record_notification_outcome(
        &self,
        a: AccountId,
        native_id: &str,
        result: PopupOutcome,
        now: i64,
    ) -> Result<bool> {
        if result == PopupOutcome::Unknown {
            return Ok(false);
        }
        self.write(|tx| {
            let (final_state, phase) = match result {
                PopupOutcome::Delivered => ("delivered", 2),
                PopupOutcome::Rejected => ("withdrawn", 1),
                PopupOutcome::Unknown => unreachable!(),
            };
            let row=tx.query_row("UPDATE native_notification_groups SET state=?,changed_at=? WHERE account_id=? AND native_id=? AND state='unknown' RETURNING id",
                        params![final_state,now,a,native_id],|r|r.get::<_,i64>(0)).optional()?;
            if row.is_none() {
                return Ok(false);
            }
            tx.execute("UPDATE notification_delivery SET phase=?,updated_at_ms=? WHERE account_id=? AND native_id=? AND phase=3",params![phase,now,a,native_id])?;
            Ok(true)
        })
    }
    pub fn notification_feed(
        &self,
        a: AccountId,
        before: Option<i64>,
        limit: u16,
    ) -> Result<Vec<FeedEntry>> {
        if !(1..=256).contains(&limit) {
            return Err(Error::InvalidInput("Invalid activity feed page size"));
        }
        self.read(|db| {
            let sql = format!(
                "{EVENT_COLUMNS} WHERE n.account_id=? AND n.published=1 AND n.id<? ORDER BY n.id DESC LIMIT ?"
            );
            let mut q = db.prepare(&sql)?;
            let rows = q.query_map(params![a, before.unwrap_or(i64::MAX), limit], collect_event)?;
            let mut out = Vec::new();
            for row in rows {
                let e = event(db, row?)?;
                let (decision, read_at_ms) = db.query_row(
                    "SELECT decision,read_at FROM notification_events WHERE id=?",
                    [e.id],
                    |r| Ok((r.get::<_, String>(0)?, r.get(1)?)),
                )?;
                out.push(FeedEntry {
                    id: e.id,
                    event_key: e.key,
                    event_kind:e.kind,
                    occurred_at_ms:e.occurred,
                    revision:e.revision,
                    repository: e.repo,
                    target: e.facts.target,
                    categories: e.facts.categories,
                    decision: decode(&decision)?,
                    read_at_ms,
                    observed_at_ms: e.at,
                });
            }
            Ok(out)
        })
    }
    pub fn mark_notification_read(&self, a: AccountId, event_id: i64, now: i64) -> Result<bool> {
        self.write(|tx|Ok(tx.execute("UPDATE notification_events SET read_at=? WHERE account_id=? AND id=? AND published=1 AND read_at IS NULL",params![now,a,event_id])?==1))
    }
    pub fn pending_notification_open(
        &self,
        account: AccountId,
    ) -> Result<Option<(String, Destination)>> {
        self.read(|db| {
            let next = db
                .query_row(
                    "SELECT g.native_id, g.destination FROM pending_notification_opens p
                 JOIN native_notification_groups g ON g.id = p.group_id
                 JOIN active_profile a ON a.account_id = g.account_id
                 WHERE g.account_id = ? ORDER BY g.activated_at, g.id LIMIT 1",
                    [account],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?;
            next.map(|(key, route)| Ok((key, decode(&route)?)))
                .transpose()
        })
    }
    pub fn complete_notification_open(&self, account: AccountId, native_id: &str) -> Result<bool> {
        self.write(|tx| {
            Ok(tx.execute(
                "DELETE FROM pending_notification_opens WHERE group_id IN
                 (SELECT g.id FROM native_notification_groups g
                  JOIN active_profile a ON a.account_id = g.account_id
                  WHERE g.account_id = ? AND g.native_id = ?)",
                params![account, native_id],
            )? == 1)
        })
    }

    pub fn activate_notification(
        &self,
        a: AccountId,
        native_id: &str,
        action: NotificationAction,
        now: i64,
    ) -> Result<Option<Destination>> {
        self.write(|tx| {
            let group = tx
                .query_row(
                    "SELECT g.id, g.destination FROM native_notification_groups g
                 JOIN active_profile a ON a.account_id = g.account_id
                 WHERE g.account_id = ? AND g.native_id = ? AND g.activated_at IS NULL
                   AND g.state IN ('unknown', 'delivered')",
                    params![a, native_id],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?;
            let Some((id, raw)) = group else {
                return Ok(None);
            };
            let route: Destination = decode(&raw)?;
            tx.execute(
                "UPDATE native_notification_groups SET activated_at = ? WHERE id = ?",
                params![now, id],
            )?;
            if action == NotificationAction::Open {
                tx.execute(
                    "INSERT INTO pending_notification_opens (group_id) VALUES (?)",
                    [id],
                )?;
            }
            match action {
                NotificationAction::Open | NotificationAction::MarkRead => {
                    tx.execute(
                        "UPDATE notification_events SET read_at = COALESCE(read_at, ?)
                         WHERE group_id = ? AND published = 1",
                        params![now, id],
                    )?;
                }
                NotificationAction::SnoozeUntil(until) => {
                    if until <= now {
                        return Err(Error::InvalidInput("Snooze must end in the future"));
                    }
                    if let Some(repo) = route.repository {
                        snooze_in(tx, a, repo, &route.target, Some(until))?
                    } else {
                        let sql = format!("{EVENT_COLUMNS} WHERE n.group_id=? ORDER BY n.id");
                        let mut q = tx.prepare(&sql)?;
                        let rows = q.query_map([id], collect_event)?;
                        for row in rows {
                            let e = event(tx, row?)?;
                            snooze_in(tx, a, e.repo, &e.facts.target, Some(until))?
                        }
                    }
                }
            }
            Ok(Some(route))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ut_015_deduplication_groups_only_correlated_targets_and_keeps_unattributed_commits() {
        let repo = Some(RepositoryId(4));
        let item = |id, thread| Destination {
            repository: repo,
            target: NotificationTarget::Item {
                item: ItemId(id),
                thread,
            },
        };
        let none = item(2, None);
        assert_eq!(
            combine_destination(item(2, Some("one".into())), item(2, Some("two".into()))),
            none
        );
        assert_eq!(combine_destination(none.clone(), none.clone()), none);
        assert_eq!(
            combine_destination(none, item(3, None)),
            Destination {
                repository: None,
                target: NotificationTarget::ActivityFeed
            }
        );
        assert!(!catchup(1, 1 + CATCH_UP_MS));
        assert!(catchup(0, 1 + CATCH_UP_MS));
    }
}
