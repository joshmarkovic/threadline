//! Account-scoped cached reading. Call these synchronous operations on a background worker.
mod discussion;
mod downloads;
mod images;
mod ingest;
mod search;
mod timeline;
use crate::{
    AccountId, Error, ItemId, ItemKind, RepositoryId, Result, Store, identity,
    resources::{Coverage, ResourceFamily, ResourceKey, ResourceTarget},
};
pub use discussion::*;
pub use downloads::*;
pub use images::*;
pub use ingest::*;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::Value};
pub use search::*;
use serde::{Deserialize, Serialize};
pub use timeline::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReaderScope {
    Selected,
    Repository(RepositoryId),
}
impl ReaderScope {
    fn repo(self) -> Option<i64> {
        match self {
            Self::Selected => None,
            Self::Repository(id) => Some(id.0),
        }
    }
}
#[derive(Default, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum KindFilter {
    #[default]
    All,
    Issues,
    PullRequests,
}
#[derive(Default, Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum StateFilter {
    #[default]
    All,
    Open,
    Closed,
    Merged,
}
#[derive(Default, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BrowseFilter {
    pub kind: KindFilter,
    pub state: StateFilter,
    pub author: Option<String>,
    pub label: Option<String>,
    pub assignee: Option<String>,
    pub reviewer: Option<String>,
    pub participant: Option<String>,
    pub mention: Option<String>,
    pub updated_since_ms: Option<i64>,
    pub updated_before_ms: Option<i64>,
    pub pinned_only: bool,
    pub recent_only: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowseRow {
    pub item: ItemId,
    pub repository: RepositoryId,
    pub owner: String,
    pub name: String,
    pub number: u64,
    pub kind: ItemKind,
    pub title: Option<String>,
    pub closed: Option<bool>,
    pub merged: Option<bool>,
    pub pinned: bool,
}
pub struct BrowsePage {
    pub items: Vec<BrowseRow>,
    pub next: Option<ItemId>,
    pub coverage: Coverage,
    pub last_validated_at_ms: Option<i64>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KnownCount {
    pub known: u64,
    pub complete: bool,
    pub last_validated_at_ms: Option<i64>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SidebarCounts {
    pub assigned: KnownCount,
    pub review_requested: KnownCount,
    pub outbox: u64,
}

const ROW_FIELDS: &str =
    "v.id,v.repository_id,v.owner,v.name,v.number,v.kind,v.title,v.closed,v.merged,v.pinned";
const SCOPE: &str = "v.account_id=?1 AND ((?2 IS NOT NULL AND v.repository_id=?2) OR
    (?2 IS NULL AND EXISTS(SELECT 1 FROM github_repository_access a WHERE a.account_id=?1 AND a.repository_id=v.repository_id AND a.selected=1)))";
const INCLUDED: &str = "(NOT EXISTS(SELECT 1 FROM cached_resources c WHERE c.repository_id=v.repository_id AND c.family='repository-items' AND c.coverage=2 AND c.fetch=0)
    OR EXISTS(SELECT 1 FROM cached_resources c JOIN cached_resource_records k ON k.resource_id=c.id
        WHERE c.repository_id=v.repository_id AND c.family='repository-items' AND k.record_key=CAST(v.id AS TEXT)))";
fn row_at(row: &Row<'_>, i: usize) -> rusqlite::Result<BrowseRow> {
    Ok(BrowseRow {
        item: row.get(i)?,
        repository: row.get(i + 1)?,
        owner: row.get(i + 2)?,
        name: row.get(i + 3)?,
        number: row.get::<_, i64>(i + 4)? as u64,
        kind: identity::kind(row, i + 5)?,
        title: row.get(i + 6)?,
        closed: row.get(i + 7)?,
        merged: row.get(i + 8)?,
        pinned: row.get(i + 9)?,
    })
}
fn limit_ok(limit: u16) -> Result<()> {
    if (1..=250).contains(&limit) {
        Ok(())
    } else {
        Err(Error::InvalidInput(
            "Reader page size must be between 1 and 250",
        ))
    }
}
fn scope_check(db: &Connection, account: AccountId, scope: ReaderScope) -> Result<()> {
    if let ReaderScope::Repository(repo) = scope {
        let exists: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM repositories WHERE id=? AND account_id=?)",
            params![repo, account],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(Error::NotFound);
        }
    }
    Ok(())
}
fn coverage(
    db: &Connection,
    account: AccountId,
    scope: ReaderScope,
) -> Result<(Coverage, Option<i64>)> {
    let (n, complete, observed, at): (i64, i64, i64, Option<i64>) =
        db.query_row("SELECT count(*),count(*) FILTER(WHERE c.coverage=2),count(*) FILTER(WHERE c.coverage>0),min(c.last_validation_at_ms)
        FROM repositories r LEFT JOIN cached_resources c ON c.repository_id=r.id AND c.family='repository-items'
        WHERE r.account_id=?1 AND (r.id=?2 OR (?2 IS NULL AND EXISTS(SELECT 1 FROM github_repository_access a
        WHERE a.account_id=?1 AND a.repository_id=r.id AND a.selected=1)))", params![account, scope.repo()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
    let level = if n > 0 && complete == n {
        Coverage::Complete
    } else if observed > 0 {
        Coverage::Partial
    } else {
        Coverage::Missing
    };
    Ok((level, if complete == n { at } else { None }))
}
pub fn repository_list_key(account: AccountId, repository: RepositoryId) -> ResourceKey {
    ResourceKey {
        account,
        target: ResourceTarget::Repository(repository),
        family: ResourceFamily::RepositoryItems,
    }
}
fn checked_label(text: &str) -> Result<()> {
    if text.trim().is_empty() || text.len() > 512 || text.contains('\0') {
        Err(Error::InvalidInput("Invalid reader filter or view name"))
    } else {
        Ok(())
    }
}
impl BrowseFilter {
    fn coverage(
        &self,
        db: &Connection,
        account: AccountId,
        scope: ReaderScope,
        listed: Coverage,
    ) -> Result<Coverage> {
        if listed != Coverage::Complete {
            return Ok(listed);
        }
        for (category, required) in [
            ("author", self.author.is_some()),
            ("label", self.label.is_some()),
            ("assignee", self.assignee.is_some()),
            ("reviewer", self.reviewer.is_some()),
            ("participant", self.participant.is_some()),
            ("mention", self.mention.is_some()),
        ] {
            if !required {
                continue;
            }
            let kind = match self.kind {
                KindFilter::All => None,
                KindFilter::Issues => Some("issue"),
                KindFilter::PullRequests => Some("pull_request"),
            };
            let sql = format!("SELECT EXISTS(SELECT 1 FROM reader_rows v WHERE {SCOPE} AND {INCLUDED}
                AND v.title IS NOT NULL AND (?4 IS NULL OR v.kind=?4) AND (?3!='reviewer' OR v.kind='pull_request')
                AND NOT EXISTS(SELECT 1 FROM reader_facets f WHERE f.item_id=v.id AND f.category=?3))");
            if db.query_row(&sql, params![account, scope.repo(), category, kind], |r| {
                r.get::<_, bool>(0)
            })? {
                return Ok(Coverage::Partial);
            }
        }
        Ok(listed)
    }
    fn predicate(&self, values: &mut Vec<Value>) -> Result<String> {
        let mut sql = String::new();
        match self.kind {
            KindFilter::All => {}
            KindFilter::Issues => sql.push_str(" AND v.kind='issue'"),
            KindFilter::PullRequests => sql.push_str(" AND v.kind='pull_request'"),
        }
        match self.state {
            StateFilter::All => {}
            StateFilter::Open => sql.push_str(" AND v.closed=0 AND coalesce(v.merged,0)=0"),
            StateFilter::Closed => sql.push_str(" AND (v.closed=1 OR v.merged=1)"),
            StateFilter::Merged => sql.push_str(" AND v.merged=1"),
        }
        if self.pinned_only {
            sql.push_str(" AND v.pinned=1")
        }
        if self.recent_only {
            sql.push_str(" AND v.opened_at IS NOT NULL")
        }
        for (category, value) in [
            ("author", &self.author),
            ("label", &self.label),
            ("assignee", &self.assignee),
            ("reviewer", &self.reviewer),
            ("participant", &self.participant),
            ("mention", &self.mention),
        ] {
            if let Some(value) = value {
                checked_label(value)?;
                sql.push_str(" AND EXISTS(SELECT 1 FROM reader_facet_values f WHERE f.item_id=v.id AND f.category=? AND f.value=?)");
                values.push(Value::Text(category.into()));
                values.push(Value::Text(value.clone()));
            }
        }
        if self
            .updated_since_ms
            .zip(self.updated_before_ms)
            .is_some_and(|(start, end)| start >= end)
        {
            return Err(Error::InvalidInput("Invalid updated-time range"));
        }
        for (expr, bound) in [
            (" AND v.updated_at>=?", self.updated_since_ms),
            (" AND v.updated_at<?", self.updated_before_ms),
        ] {
            if let Some(bound) = bound {
                sql.push_str(expr);
                values.push(Value::Integer(bound))
            }
        }
        Ok(sql)
    }
}
impl Store {
    /// Resolve a named repository for this account and honor previously verified transfer aliases.
    pub fn resolve_downloaded_route(
        &self,
        account: AccountId,
        route: &threadline_core::domain::RepositoryName,
        number: u64,
        expected_kind: Option<ItemKind>,
    ) -> Result<Option<ItemId>> {
        let number = i64::try_from(number)
            .ok()
            .filter(|n| *n > 0)
            .ok_or(Error::InvalidInput("Invalid item number"))?;
        self.read(|db| {
            let match_row: Option<(ItemId, ItemKind)> = db
                .query_row(
                    "WITH repo AS (SELECT id FROM repositories WHERE account_id=?1 AND owner=?2 AND name=?3),
            target AS (SELECT id FROM items WHERE repository_id=(SELECT id FROM repo) AND number=?4
                UNION SELECT item_id FROM item_aliases WHERE repository_id=(SELECT id FROM repo) AND number=?4)
            SELECT i.id,i.kind FROM items i JOIN target t ON i.id=t.id WHERE i.account_id=?1",
                    params![account, route.owner(), route.name(), number],
                    |r| Ok((r.get(0)?, identity::kind(r, 1)?)),
                )
                .optional()?;
            Ok(match_row
                .filter(|(_, kind)| expected_kind.is_none_or(|k| k == *kind))
                .map(|(id, _)| id))
        })
    }
    pub fn browse_page(
        &self,
        account: AccountId,
        scope: ReaderScope,
        filter: &BrowseFilter,
        before: Option<ItemId>,
        limit: u16,
    ) -> Result<BrowsePage> {
        limit_ok(limit)?;
        self.read(|conn| {
            let db = conn.unchecked_transaction()?;
            scope_check(&db, account, scope)?;
            let (listed, last_validated_at_ms) = coverage(&db, account, scope)?;
            let coverage = filter.coverage(&db, account, scope, listed)?;
            let mut values = vec![
                Value::Integer(account.0),
                scope.repo().map_or(Value::Null, Value::Integer),
            ];
            let mut where_sql = filter.predicate(&mut values)?;
            if let Some(cursor) = before {
                if filter.recent_only {
                    where_sql.push_str(" AND (v.opened_at,v.id)<(SELECT opened_at,id FROM reader_rows WHERE account_id=? AND id=?)");
                    values.push(Value::Integer(account.0));
                } else {
                    where_sql.push_str(" AND v.id<?")
                }
                values.push(Value::Integer(cursor.0));
            }
            let order = if filter.recent_only { "v.opened_at DESC,v.id DESC" } else { "v.id DESC" };
            let sql = format!("SELECT {ROW_FIELDS} FROM reader_rows v WHERE {SCOPE} AND {INCLUDED} AND v.title IS NOT NULL {where_sql} ORDER BY {order} LIMIT ?");
            values.push(Value::Integer(i64::from(limit) + 1));
            let mut stmt = db.prepare_cached(&sql)?;
            let mut items = stmt
                .query_map(params_from_iter(values), |r| row_at(r, 0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let next = if items.len() > usize::from(limit) {
                items.pop();
                items.last().map(|r| r.item)
            } else {
                None
            };
            Ok(BrowsePage {
                items,
                next,
                coverage,
                last_validated_at_ms,
            })
        })
    }
    pub fn save_reader_view(
        &self,
        account: AccountId,
        name: &str,
        filter: &BrowseFilter,
    ) -> Result<()> {
        checked_label(name)?;
        filter.predicate(&mut Vec::new())?;
        let encoded =
            serde_json::to_string(filter).map_err(|_| Error::InvalidInput("Invalid saved view"))?;
        self.write(|tx| {
            tx.execute(
                "INSERT INTO reader_saved_views(account_id,name,filters) VALUES(?,?,?)
            ON CONFLICT(account_id,name) DO UPDATE SET filters=excluded.filters",
                params![account, name, encoded],
            )?;
            Ok(())
        })
    }
    pub fn reader_views(&self, account: AccountId) -> Result<Vec<(String, BrowseFilter)>> {
        self.read(|db| {
            let mut stmt = db.prepare_cached(
                "SELECT name,filters FROM reader_saved_views WHERE account_id=? ORDER BY name",
            )?;
            let rows = stmt.query_map([account], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?;
            rows.map(|r| {
                let (name, filters) = r?;
                Ok((
                    name,
                    serde_json::from_str(&filters).map_err(|_| Error::Corrupt)?,
                ))
            })
            .collect()
        })
    }
    pub fn remove_reader_view(&self, account: AccountId, name: &str) -> Result<bool> {
        self.write(|db| {
            Ok(db.execute(
                "DELETE FROM reader_saved_views WHERE account_id=? AND name=?",
                params![account, name],
            )? > 0)
        })
    }
    pub fn set_pinned_item(&self, account: AccountId, item: ItemId, pinned: bool) -> Result<()> {
        self.write(|db| {
            require_item(db, account, item)?;
            db.execute(
                "INSERT INTO reader_marks(item_id,pinned) VALUES(?,?)
            ON CONFLICT(item_id) DO UPDATE SET pinned=excluded.pinned",
                params![item, pinned],
            )?;
            Ok(())
        })
    }
    pub fn record_recent_item(
        &self,
        account: AccountId,
        item: ItemId,
        opened_at_ms: i64,
    ) -> Result<()> {
        self.write(|db| {
            require_item(db, account, item)?;
            db.execute("INSERT INTO reader_marks(item_id,opened_at) VALUES(?,?)
            ON CONFLICT(item_id) DO UPDATE SET opened_at=excluded.opened_at WHERE opened_at IS NULL OR opened_at<=excluded.opened_at", params![item, opened_at_ms])?;
            Ok(())
        })
    }
    pub fn sidebar_counts(&self, account: AccountId, scope: ReaderScope) -> Result<SidebarCounts> {
        self.read(|conn| {
            let db = conn.unchecked_transaction()?;
            scope_check(&db, account, scope)?;
            let (coverage, last_validated_at_ms) = coverage(&db, account, scope)?;
            let query = |category: &str, pr_only: bool| -> Result<KnownCount> {
                let sql = format!("SELECT count(*) FILTER(WHERE EXISTS(SELECT 1 FROM reader_facet_values f
                  JOIN github_access_profiles a ON a.account_id=v.account_id AND a.owner_scope=v.owner AND f.value=a.login COLLATE NOCASE
                  WHERE f.item_id=v.id AND f.category=?3)),
                  count(*) FILTER(WHERE NOT EXISTS(SELECT 1 FROM reader_facets f WHERE f.item_id=v.id AND f.category=?3)
                    OR NOT EXISTS(SELECT 1 FROM github_access_profiles a WHERE a.account_id=v.account_id AND a.owner_scope=v.owner))
                  FROM reader_rows v WHERE {SCOPE} AND {INCLUDED} AND v.title IS NOT NULL AND (?4=0 OR v.kind='pull_request')");
                let (count, unknown): (i64, i64) = db.query_row(
                    &sql,
                    params![account, scope.repo(), category, pr_only],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                Ok(KnownCount {
                    known: count as u64,
                    complete: coverage == Coverage::Complete && unknown == 0,
                    last_validated_at_ms,
                })
            };
            let assigned = query("assignee", false)?;
            let review_requested = query("reviewer", true)?;
            // Holds, including a deselected repository, still belong to the account's Outbox.
            let outbox: i64 = db.query_row("SELECT count(*) FROM operations WHERE account_id=? AND state NOT IN ('confirmed','cancelled')", [account], |r| r.get(0))?;
            Ok(SidebarCounts {
                assigned,
                review_requested,
                outbox: outbox as u64,
            })
        })
    }
}
fn require_item(db: &Connection, account: AccountId, item: ItemId) -> Result<()> {
    db.query_row(
        "SELECT 1 FROM items WHERE id=? AND account_id=?",
        params![item, account],
        |_| Ok(()),
    )
    .optional()?
    .ok_or(Error::NotFound)
}
