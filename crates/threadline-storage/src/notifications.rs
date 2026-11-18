mod rules;
pub use rules::*;
mod delivery;
use crate::{
    AccountId, Error, ItemId, RepositoryId, Result, Store, access::AccessProfile, resources,
};
pub use delivery::*;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

fn encode<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| Error::Corrupt)
}
fn decode<T: DeserializeOwned>(raw: &str) -> Result<T> {
    serde_json::from_str(raw).map_err(|_| Error::Corrupt)
}
fn settings(db: &Connection, a: AccountId) -> Result<NotificationSettings> {
    db.query_row(
        "SELECT settings FROM notification_settings WHERE account_id=?",
        [a],
        |r| r.get::<_, String>(0),
    )
    .optional()?
    .map(|s| decode(&s))
    .transpose()
    .map(Option::unwrap_or_default)
}
fn rules(db: &Connection, a: AccountId, r: RepositoryId) -> Result<RepositoryRules> {
    db.query_row(
        "SELECT rules FROM repository_notification_rules WHERE account_id=? AND repository_id=?",
        params![a, r],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(|s| decode(&s))
    .transpose()
    .map(Option::unwrap_or_default)
}
fn validate_target(
    db: &Connection,
    a: AccountId,
    r: RepositoryId,
    target: &NotificationTarget,
) -> Result<()> {
    resources::require_repository(db, a, r)?;
    match target {
        NotificationTarget::Item { item, thread } => {
            if thread
                .as_ref()
                .is_some_and(|t| t.is_empty() || t.len() > 512 || t.contains('\0'))
            {
                return Err(Error::InvalidInput("Invalid notification thread identity"));
            }
            let found:bool=db.query_row(
                "SELECT EXISTS(SELECT 1 FROM items WHERE account_id=?1 AND repository_id=?2 AND id=?3
                 UNION ALL SELECT 1 FROM item_aliases WHERE account_id=?1 AND repository_id=?2 AND item_id=?3)",
                 params![a,r,item],|row|row.get(0))?;
            if !found {
                return Err(Error::Conflict(
                    "Notification target belongs to another repository or account",
                ));
            }
        }
        NotificationTarget::Comparison { reference, .. } => {
            if !reference.starts_with("refs/")
                || reference.len() > 1024
                || reference.chars().any(char::is_control)
            {
                return Err(Error::InvalidInput("Invalid branch comparison reference"));
            }
        }
        NotificationTarget::ActivityFeed => {
            return Err(Error::InvalidInput(
                "Provider event requires a repository target",
            ));
        }
        NotificationTarget::Repository => (),
    }
    Ok(())
}

pub(crate) fn attach_in(
    tx: &Transaction<'_>,
    a: AccountId,
    key: &str,
    facts: &EventFacts,
) -> Result<bool> {
    let row = tx
        .query_row(
            "SELECT repository_id,item_id FROM activity_events WHERE account_id=? AND event_key=?",
            params![a, key],
            |r| Ok((r.get::<_, RepositoryId>(0)?, r.get::<_, Option<ItemId>>(1)?)),
        )
        .optional()?;
    let Some((repo, item)) = row else {
        return Ok(false);
    };
    validate_target(tx, a, repo, &facts.target)?;
    if matches!(&facts.target,NotificationTarget::Item{item:id,..} if Some(*id)!=item) {
        return Err(Error::Conflict(
            "Notification cannot be routed to a different observed item",
        ));
    }
    let n=tx.execute("UPDATE notification_events SET facts=? WHERE account_id=? AND event_key=? AND facts IS NULL AND decision IS NULL",
        params![encode(facts)?,a,key])?;
    Ok(n == 1)
}

impl Store {
    pub fn notification_settings(&self, a: AccountId) -> Result<NotificationSettings> {
        self.read(|db| settings(db, a))
    }
    pub fn update_notification_settings(
        &self,
        a: AccountId,
        change: impl FnOnce(&mut NotificationSettings),
    ) -> Result<NotificationSettings> {
        self.write(|tx| {
            let mut settings = settings(tx, a)?;
            change(&mut settings);
            if let Some(hours) = &settings.quiet_hours {
                hours.schedule().validate().map_err(Error::InvalidInput)?;
            }
            tx.execute("INSERT INTO notification_settings VALUES(?,?) ON CONFLICT(account_id) DO UPDATE SET settings=excluded.settings",params![a,encode(&settings)?])?;
            Ok(settings)
        })
    }
    pub fn repository_notification_rules(
        &self,
        a: AccountId,
        r: RepositoryId,
    ) -> Result<RepositoryRules> {
        self.read(|db| {
            resources::require_repository(db, a, r)?;
            rules(db, a, r)
        })
    }
    pub fn update_repository_notification_rules(
        &self,
        a: AccountId,
        r: RepositoryId,
        change: impl FnOnce(&mut RepositoryRules),
    ) -> Result<RepositoryRules> {
        self.write(|tx| {
            resources::require_repository(tx, a, r)?;
            let mut settings = rules(tx, a, r)?;
            change(&mut settings);
            tx.execute("INSERT INTO repository_notification_rules VALUES(?,?,?) ON CONFLICT(account_id,repository_id) DO UPDATE SET rules=excluded.rules",
                               params![a,r,encode(&settings)?])?;
            Ok(settings)
        })
    }
    pub fn follow_notification_target(
        &self,
        a: AccountId,
        r: RepositoryId,
        target: &NotificationTarget,
        follow: bool,
    ) -> Result<()> {
        self.write(|tx| {
            validate_target(tx,a,r,target)?;
            tx.execute("INSERT INTO notification_targets(account_id,repository_id,group_key,followed) VALUES(?,?,?,?)
                ON CONFLICT(account_id,repository_id,group_key) DO UPDATE SET followed=excluded.followed",params![a,r,group_key(r,target)?,follow])?;
            Ok(())
        })
    }
    pub fn snooze_notification_target(
        &self,
        a: AccountId,
        r: RepositoryId,
        target: &NotificationTarget,
        until: Option<i64>,
    ) -> Result<()> {
        self.write(|tx| snooze_in(tx, a, r, target, until))
    }
    /// Attach additional facts while their durably observed event is pending a delivery decision.
    pub fn attach_notification_facts(
        &self,
        profile: &AccessProfile,
        key: &str,
        facts: &EventFacts,
    ) -> Result<bool> {
        self.write(|tx| {
            let repo: RepositoryId = tx.query_row(
                "SELECT repository_id FROM activity_events WHERE account_id=? AND event_key=?",
                params![profile.account, key],
                |r| r.get(0),
            )?;
            crate::writes::require_observation_profile(tx, profile, repo)?;
            attach_in(tx, profile.account, key, facts)
        })
    }
}
fn snooze_in(
    tx: &Transaction<'_>,
    a: AccountId,
    r: RepositoryId,
    target: &NotificationTarget,
    until: Option<i64>,
) -> Result<()> {
    validate_target(tx, a, r, target)?;
    tx.execute("INSERT INTO notification_targets(account_id,repository_id,group_key,snooze_until) VALUES(?,?,?,?)
        ON CONFLICT(account_id,repository_id,group_key) DO UPDATE SET snooze_until=excluded.snooze_until",params![a,r,group_key(r,target)?,until])?;
    Ok(())
}

/// A provider observation with trustworthy remote time and an immutable event identity.
pub struct ObservedNotification<'a> {
    pub repository: RepositoryId,
    pub stream: &'a str,
    pub key: &'a str,
    pub kind: &'a str,
    pub occurred_at_ms: i64,
    pub received_at_ms: i64,
    pub revision: Option<&'a str>,
    pub facts: &'a EventFacts,
}
impl Store {
    pub fn enroll_notification_stream(
        &self,
        profile: &AccessProfile,
        repo: RepositoryId,
        stream: &str,
        now: i64,
    ) -> Result<bool> {
        if stream.is_empty() || stream.len() > 128 {
            return Err(Error::InvalidInput("Invalid notification stream identity"));
        }
        self.write(|tx| {
            crate::writes::require_observation_profile(tx, profile, repo)?;
            Ok(tx.execute("INSERT INTO activity_baselines(repository_id,stream_key,checkpoint,observed_at_ms) VALUES(?,?,'monitoring-started',?)
                        ON CONFLICT(repository_id,stream_key) DO NOTHING",params![repo,format!("notify:{stream}"),now])?==1)
        })
    }
    pub fn observe_notification_change(
        &self,
        profile: &AccessProfile,
        e: ObservedNotification<'_>,
    ) -> Result<bool> {
        if e.key.is_empty()
            || e.key.len() > 1024
            || e.kind.is_empty()
            || e.facts.categories.is_empty()
            || e.occurred_at_ms > e.received_at_ms
        {
            return Err(Error::InvalidInput(
                "Notification needs a stable observed identity, category and remote timestamp",
            ));
        }
        self.write(|tx| {
            let a = profile.account;
            crate::writes::require_observation_profile(tx, profile, e.repository)?;
            validate_target(tx, a, e.repository, &e.facts.target)?;
            let item = match e.facts.target {
                NotificationTarget::Item { item, .. } => Some(item),
                _ => None,
            };
            let key = format!("observed:{}:{}:{}", e.repository.0, e.stream, e.key);
            let inserted=tx.execute("INSERT INTO activity_events(account_id,event_key,repository_id,item_id,event_kind,revision,occurred_at_ms,observed_at_ms)
                        SELECT ?1,?2,?3,?4,?5,?6,?7,?8 FROM activity_baselines WHERE repository_id=?3 AND stream_key=?9 AND observed_at_ms<?7
                        ON CONFLICT(account_id,event_key) DO NOTHING", params![a,key,e.repository,item,e.kind,e.revision,e.occurred_at_ms,e.received_at_ms,format!("notify:{}",e.stream)])?;
            if inserted == 1 {
                attach_in(tx, a, &key, e.facts)?;
            }
            Ok(inserted == 1)
        })
    }
}

/// Detect an explicit GitHub login token in ordinary prose. Skip inline code, fenced code and email addresses.
pub(crate) fn mentions_login(body: &str, login: &str) -> bool {
    if login.is_empty() || !login.is_ascii() {
        return false;
    }
    let mut fence = None;
    let mut code_ticks = 0;
    for line in body.lines() {
        let trimmed = line.trim_start();
        let c = trimmed.as_bytes().first().copied();
        let n = c.map_or(0, |c| trimmed.bytes().take_while(|b| *b == c).count());
        if matches!(c, Some(b'`' | b'~')) && n >= 3 {
            if fence.is_none() {
                fence = Some((c, n));
                code_ticks = 0
            } else if fence.is_some_and(|(ch, len)| ch == c && n >= len) {
                fence = None
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'`' {
                let len = bytes[i..].iter().take_while(|b| **b == b'`').count();
                if code_ticks == 0 {
                    code_ticks = len
                } else if code_ticks == len {
                    code_ticks = 0
                }
                i += len;
                continue;
            }
            if code_ticks == 0
                && bytes[i] == b'@'
                && (i == 0
                    || !matches!(bytes[i-1],b'a'..=b'z'|b'A'..=b'Z'|b'0'..=b'9'|b'_'|b'/'|b'-'|b'\\'))
            {
                let end = i
                    + 1
                    + bytes[i + 1..]
                        .iter()
                        .take_while(|c| c.is_ascii_alphanumeric() || **c == b'-')
                        .count();
                if bytes.get(end) != Some(&b'/')
                    && bytes[i + 1..end].eq_ignore_ascii_case(login.as_bytes())
                {
                    return true;
                }
            }
            i += 1;
        }
    }
    false
}
