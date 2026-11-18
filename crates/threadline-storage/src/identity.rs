use std::num::NonZeroU64;

use rusqlite::{
    Connection, OptionalExtension as _, Row, ToSql, Transaction, named_params, params,
    types::{FromSql, FromSqlResult, ToSqlOutput, ValueRef},
};
use threadline_core::domain::{HostName, IssueApiId, RepositoryName};

use crate::{Error, Result, Store, UtcMillis, resources::clear_resource_cache};

macro_rules! local_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub i64);

        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                self.0.to_sql()
            }
        }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                i64::column_result(value).map(Self)
            }
        }
    };
}

local_id!(AccountId);
local_id!(RepositoryId);
local_id!(ItemId);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PullRequestApiId(NonZeroU64);

impl PullRequestApiId {
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl TryFrom<u64> for PullRequestApiId {
    type Error = Error;

    fn try_from(value: u64) -> Result<Self> {
        NonZeroU64::new(value)
            .map(Self)
            .ok_or(Error::InvalidInput("Provider identity must be nonzero"))
    }
}

/// Opaque provider identity of one API representation.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct NodeId(String);

impl NodeId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty() || value.len() > 1024 || value.contains('\0') {
            return Err(Error::InvalidInput("Invalid provider node identity"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemLocator {
    pub repository: RepositoryId,
    pub number: u64,
}

impl ItemLocator {
    fn sql_number(self) -> Result<i64> {
        i64::try_from(self.number)
            .ok()
            .filter(|number| *number > 0)
            .ok_or(Error::InvalidInput("Item number is out of range"))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemKind {
    Unknown,
    Issue,
    PullRequest,
}

impl ItemKind {
    fn observe_issue(self, evidence: IssueClassification) -> Self {
        match (self, evidence) {
            (Self::PullRequest, _) | (_, IssueClassification::PullRequest) => Self::PullRequest,
            (_, IssueClassification::Issue) => Self::Issue,
            (kind, IssueClassification::Unknown) => kind,
        }
    }

    fn as_sql(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Issue => "issue",
            Self::PullRequest => "pull_request",
        }
    }

    fn permits_pr_details(self) -> bool {
        self == Self::PullRequest
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IssueClassification {
    /// The response did not establish whether a PR marker was present.
    Unknown,
    Issue,
    PullRequest,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Change<T> {
    #[default]
    Missing,
    Null,
    Value(T),
}

impl<T> Change<T> {
    fn stamp(self, time: UtcMillis) -> Stamped<T> {
        match self {
            Self::Missing => (None, None),
            Self::Null => (None, Some(time.0)),
            Self::Value(value) => (Some(value), Some(time.0)),
        }
    }
}

type Stamped<T> = (Option<T>, Option<i64>);

fn merge_field<T>(current: Stamped<T>, patch: Change<T>, updated: UtcMillis) -> Stamped<T> {
    let incoming = patch.stamp(updated);
    if incoming.1 > current.1 {
        incoming
    } else {
        current
    }
}

fn merge_confirmed(
    current: Stamped<bool>,
    patch: Change<bool>,
    updated: UtcMillis,
) -> Stamped<bool> {
    let confirmed = current.0 == Some(true) || patch == Change::Value(true);
    let merged = merge_field(current, patch, updated);
    if confirmed {
        (Some(true), merged.1)
    } else {
        merged
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssueRecord {
    pub id: IssueApiId,
    pub node_id: Option<NodeId>,
    pub classification: IssueClassification,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PullRequestRecord {
    pub id: PullRequestApiId,
    pub node_id: Option<NodeId>,
    pub details: PullRequestUpdate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ItemRepresentation {
    Issue(IssueRecord),
    PullRequest(PullRequestRecord),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemUpdate {
    pub representation: ItemRepresentation,
    /// Comparable provider update time in UTC. Receipt time and local clocks
    /// cannot establish freshness. Equal times preserve existing supplied fields.
    /// Older responses can fill fields that have never been supplied.
    pub remote_updated_at: UtcMillis,
    pub title: Change<String>,
    pub body: Change<String>,
    pub closed: Change<bool>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PullRequestUpdate {
    pub merged: Change<bool>,
    pub base_oid: Change<String>,
    pub head_oid: Change<String>,
    pub merge_oid: Change<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PullRequestDetails {
    pub id: PullRequestApiId,
    pub node_id: Option<NodeId>,
    pub fields: PullRequestUpdate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    pub id: ItemId,
    pub locator: ItemLocator,
    pub kind: ItemKind,
    pub issue_id: Option<IssueApiId>,
    pub issue_node_id: Option<NodeId>,
    pub title: Change<String>,
    pub body: Change<String>,
    pub closed: Change<bool>,
    pub pull_request: Option<PullRequestDetails>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransferEvidence {
    Unverified,
    /// The provider verified a transfer to the new locator using the issue REST ID.
    VerifiedIssueId(IssueApiId),
    /// The provider verified a transfer to the new locator using the issue node ID.
    VerifiedIssueNode(NodeId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferOutcome {
    Moved(ItemId),
    AlreadyCurrent(ItemId),
    Unresolved(TransferProblem),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferProblem {
    Unverified,
    MissingSource,
    IdentityMismatch,
    DestinationOccupied,
    SourceMoved,
}

impl Store {
    /// Register verified provider identity. No authentication material is stored.
    pub fn ensure_account(&self, host: &str, provider_user_id: u64) -> Result<AccountId> {
        let host = HostName::new(host).map_err(|_| Error::InvalidInput("Invalid provider host"))?;
        let provider_user_id = provider_number(provider_user_id)?;
        self.write(|tx| {
            let id = tx.query_row(
                "INSERT INTO accounts (host, provider_user_id) VALUES (?1, ?2)
                 ON CONFLICT (host, provider_user_id) DO UPDATE SET host = excluded.host
                 RETURNING id",
                params![host.as_str(), provider_user_id],
                |row| row.get(0),
            )?;
            Ok(id)
        })
    }

    /// Upsert the current verified name of a provider repository in one account.
    pub fn ensure_repository(
        &self,
        account: AccountId,
        provider_repository_id: u64,
        owner: &str,
        name: &str,
    ) -> Result<RepositoryId> {
        let name = RepositoryName::new(owner, name)
            .map_err(|_| Error::InvalidInput("Invalid provider repository name"))?;
        let provider_repository_id = provider_number(provider_repository_id)?;
        self.write(|tx| {
            let id = tx.query_row(
                "INSERT INTO repositories (account_id, provider_repository_id, owner, name)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (account_id, provider_repository_id)
                    DO UPDATE SET owner = excluded.owner, name = excluded.name
                 RETURNING id",
                params![account, provider_repository_id, name.owner(), name.name()],
                |row| row.get(0),
            )?;
            Ok(id)
        })
    }

    pub fn ensure_placeholder(
        &self,
        account: AccountId,
        repository: RepositoryId,
        number: u64,
    ) -> Result<ItemId> {
        let locator = ItemLocator { repository, number };
        locator.sql_number()?;
        self.write(|tx| {
            require_repository(tx, account, repository)?;
            match resolve_locator(tx, account, locator)? {
                Some(id) => Ok(id),
                None => insert_placeholder(tx, account, locator),
            }
        })
    }

    pub fn ingest(
        &self,
        account: AccountId,
        locator: ItemLocator,
        update: ItemUpdate,
    ) -> Result<ItemId> {
        locator.sql_number()?;
        self.write(|tx| {
            require_repository(tx, account, locator.repository)?;
            // Retired locators cannot be used to change the current item. The
            // caller must first resolve a fresh locator through provider evidence.
            let id = match current_at(tx, account, locator)? {
                Some(id) => id,
                None if alias_at(tx, account, locator)?.is_some() => {
                    return Err(Error::Conflict("Item locator has moved"));
                }
                None => insert_placeholder(tx, account, locator)?,
            };
            match update.representation {
                ItemRepresentation::Issue(record) => ingest_issue(tx, id, record)?,
                ItemRepresentation::PullRequest(record) => {
                    ingest_pull_request(tx, account, id, record, update.remote_updated_at)?
                }
            }
            update_summary(
                tx,
                id,
                update.remote_updated_at,
                update.title,
                update.body,
                update.closed,
            )?;
            Ok(id)
        })
    }

    /// Resolve a current locator, then a confirmed historical alias in the same account.
    pub fn resolve(&self, account: AccountId, locator: ItemLocator) -> Result<Option<ItemId>> {
        locator.sql_number()?;
        self.read(|connection| resolve_locator(connection, account, locator))
    }

    pub fn item(&self, account: AccountId, item: ItemId) -> Result<Option<Item>> {
        self.read(|connection| {
            // One statement keeps the common row and its extension in the same WAL snapshot.
            Ok(connection.query_row(
                "SELECT i.id, i.repository_id, i.number, i.kind, i.issue_api_id, i.issue_node_id,
                        i.title, i.title_at, i.body, i.body_at, i.closed, i.closed_at,
                        p.pull_request_api_id, p.pull_request_node_id, p.merged, p.merged_at,
                        p.base_oid, p.base_oid_at, p.head_oid, p.head_oid_at, p.merge_oid, p.merge_oid_at
                 FROM items AS i
                 LEFT JOIN pull_request_details AS p ON p.item_id = i.id
                 WHERE i.id = ?1 AND i.account_id = ?2",
                params![item, account], map_item,
            ).optional()?)
        })
    }

    /// Update a verified locator while preserving every relationship to the item ID.
    pub fn transfer_issue(
        &self,
        account: AccountId,
        from: ItemLocator,
        to: ItemLocator,
        evidence: TransferEvidence,
    ) -> Result<TransferOutcome> {
        from.sql_number()?;
        let number = to.sql_number()?;
        self.write(|tx| {
            require_repository(tx, account, from.repository)?;
            require_repository(tx, account, to.repository)?;
            let Some(item) = resolve_locator(tx, account, from)? else {
                return Ok(TransferOutcome::Unresolved(TransferProblem::MissingSource));
            };
            if evidence == TransferEvidence::Unverified {
                return Ok(TransferOutcome::Unresolved(TransferProblem::Unverified));
            }
            let matches = verify_transfer(tx, item, evidence)?;
            if !matches {
                return Ok(TransferOutcome::Unresolved(TransferProblem::IdentityMismatch));
            }
            if current_at(tx, account, to)? == Some(item) {
                return Ok(TransferOutcome::AlreadyCurrent(item));
            }
            if current_at(tx, account, from)? != Some(item) {
                return Ok(TransferOutcome::Unresolved(TransferProblem::SourceMoved));
            }
            if resolve_locator(tx, account, to)?.is_some_and(|id| id != item) {
                return Ok(TransferOutcome::Unresolved(TransferProblem::DestinationOccupied));
            }
            tx.execute(
                "DELETE FROM item_aliases WHERE repository_id = ?1 AND number = ?2 AND item_id = ?3",
                params![to.repository, number, item],
            )?;
            tx.execute(
                "UPDATE items SET repository_id = ?1, number = ?2 WHERE id = ?3",
                params![to.repository, number, item],
            )?;
            tx.execute(
                "INSERT INTO item_aliases (repository_id, account_id, number, item_id)
                 VALUES (?1, ?2, ?3, ?4)",
                params![from.repository, account, from.sql_number()?, item],
            )?;
            Ok(TransferOutcome::Moved(item))
        })
    }

    /// Find literal text in the local title and body index for one account.
    pub fn search(&self, account: AccountId, text: &str, limit: u32) -> Result<Vec<ItemId>> {
        let text = text.trim();
        if text.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        if text.contains('\0') || text.chars().count() > 1024 || limit > 1000 {
            return Err(Error::InvalidInput("Search input exceeds supported bounds"));
        }
        // Quoting keeps remote/user text from changing the FTS query grammar.
        let phrase = format!("\"{}\"", text.replace('"', "\"\""));
        self.read(|connection| {
            let mut statement = connection.prepare_cached(
                "SELECT i.id FROM item_search
                 JOIN items AS i ON i.id = item_search.rowid
                 WHERE item_search MATCH ?1 AND i.account_id = ?2
                 ORDER BY rank, i.id LIMIT ?3",
            )?;
            let rows = statement.query_map(params![phrase, account, limit], |row| row.get(0))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Evict remote cache for one account. Identity, durable user content and
    /// pending-operation evidence are retained.
    pub fn clear_cache(&self, account: AccountId) -> Result<()> {
        self.write(|tx| {
            clear_resource_cache(tx, account)?;
            tx.execute(
                "UPDATE items SET title = NULL, title_at = NULL, body = NULL, body_at = NULL,
                    closed = NULL, closed_at = NULL WHERE account_id = ?1",
                [account],
            )?;
            Ok(())
        })
    }
}

fn provider_number(value: u64) -> Result<String> {
    NonZeroU64::new(value)
        .map(|number| number.to_string())
        .ok_or(Error::InvalidInput("Provider identity must be nonzero"))
}

fn require_repository(
    connection: &Connection,
    account: AccountId,
    repository: RepositoryId,
) -> Result<()> {
    let found: bool = connection.query_row(
        "SELECT EXISTS (SELECT 1 FROM repositories WHERE id = ?1 AND account_id = ?2)",
        params![repository, account],
        |row| row.get(0),
    )?;
    if !found {
        return Err(Error::NotFound);
    }
    Ok(())
}

fn current_at(
    connection: &Connection,
    account: AccountId,
    locator: ItemLocator,
) -> Result<Option<ItemId>> {
    Ok(connection
        .query_row(
            "SELECT id FROM items WHERE repository_id = ?1 AND number = ?2 AND account_id = ?3",
            params![locator.repository, locator.sql_number()?, account],
            |row| row.get(0),
        )
        .optional()?)
}

fn alias_at(
    connection: &Connection,
    account: AccountId,
    locator: ItemLocator,
) -> Result<Option<ItemId>> {
    Ok(connection.query_row(
        "SELECT item_id FROM item_aliases WHERE repository_id = ?1 AND number = ?2 AND account_id = ?3",
        params![locator.repository, locator.sql_number()?, account],
        |row| row.get(0),
    ).optional()?)
}

fn resolve_locator(
    connection: &Connection,
    account: AccountId,
    locator: ItemLocator,
) -> Result<Option<ItemId>> {
    Ok(connection.query_row(
        "SELECT coalesce(
            (SELECT id FROM items WHERE repository_id = ?1 AND number = ?2 AND account_id = ?3),
            (SELECT item_id FROM item_aliases WHERE repository_id = ?1 AND number = ?2 AND account_id = ?3)
         )",
        params![locator.repository, locator.sql_number()?, account],
        |row| row.get(0),
    )?)
}

fn insert_placeholder(
    tx: &Transaction<'_>,
    account: AccountId,
    locator: ItemLocator,
) -> Result<ItemId> {
    let id = tx.query_row(
        "INSERT INTO items (repository_id, account_id, number) VALUES (?1, ?2, ?3) RETURNING id",
        params![locator.repository, account, locator.sql_number()?],
        |row| row.get(0),
    )?;
    Ok(id)
}

fn ingest_issue(tx: &Transaction<'_>, item: ItemId, record: IssueRecord) -> Result<()> {
    let current = tx.query_row("SELECT kind FROM items WHERE id = ?1", [item], |row| {
        kind(row, 0)
    })?;
    let kind = current.observe_issue(record.classification);
    let changed = tx.execute(
        "UPDATE items SET
            issue_api_id = coalesce(issue_api_id, :api_id),
            issue_node_id = coalesce(issue_node_id, :node_id),
            kind = :kind
         WHERE id = :item AND (issue_api_id IS NULL OR issue_api_id = :api_id)
           AND (issue_node_id IS NULL OR :node_id IS NULL OR issue_node_id = :node_id)",
        named_params! {
            ":item": item,
            ":api_id": record.id.get().to_string(),
            ":node_id": record.node_id.as_ref().map(NodeId::as_str),
            ":kind": kind.as_sql(),
        },
    )?;
    if changed == 0 {
        return Err(Error::Conflict(
            "Issue identity does not match the current item",
        ));
    }
    Ok(())
}

fn ingest_pull_request(
    tx: &Transaction<'_>,
    account: AccountId,
    item: ItemId,
    record: PullRequestRecord,
    time: UtcMillis,
) -> Result<()> {
    tx.execute(
        "UPDATE items SET kind = 'pull_request' WHERE id = ?1",
        [item],
    )?;
    let changed = tx.execute(
        "INSERT INTO pull_request_details (item_id, account_id, pull_request_api_id, pull_request_node_id)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (item_id) DO UPDATE SET
            pull_request_node_id = coalesce(pull_request_details.pull_request_node_id, excluded.pull_request_node_id)
         WHERE pull_request_details.pull_request_api_id = excluded.pull_request_api_id
           AND (pull_request_details.pull_request_node_id IS NULL OR excluded.pull_request_node_id IS NULL
                OR pull_request_details.pull_request_node_id = excluded.pull_request_node_id)",
        params![item, account, record.id.get().to_string(), record.node_id.as_ref().map(NodeId::as_str)],
    )?;
    if changed == 0 {
        return Err(Error::Conflict(
            "Pull request identity does not match the current item",
        ));
    }
    update_details(tx, item, time, record.details)
}

fn update_summary(
    tx: &Transaction<'_>,
    item: ItemId,
    time: UtcMillis,
    title: Change<String>,
    body: Change<String>,
    closed: Change<bool>,
) -> Result<()> {
    let ((title, title_at), (body, body_at), (closed, closed_at)) = tx.query_row(
        "SELECT title, title_at, body, body_at, closed, closed_at FROM items WHERE id = ?1",
        [item],
        |row| {
            Ok((
                merge_field(stamped(row, 0)?, title, time),
                merge_field(stamped(row, 2)?, body, time),
                merge_field(stamped(row, 4)?, closed, time),
            ))
        },
    )?;
    tx.execute(
        "UPDATE items SET title = :title, title_at = :title_at, body = :body, body_at = :body_at,
                closed = :closed, closed_at = :closed_at
         WHERE id = :item",
        named_params! {
            ":item": item, ":title": title, ":title_at": title_at,
            ":body": body, ":body_at": body_at, ":closed": closed, ":closed_at": closed_at,
        },
    )?;
    Ok(())
}

fn update_details(
    tx: &Transaction<'_>,
    item: ItemId,
    time: UtcMillis,
    details: PullRequestUpdate,
) -> Result<()> {
    let ((merged, merged_at), (base, base_at), (head, head_at), (merge, merge_at)) = tx.query_row(
        "SELECT merged, merged_at, base_oid, base_oid_at, head_oid, head_oid_at, merge_oid, merge_oid_at
         FROM pull_request_details WHERE item_id = ?1",
        [item],
        |row| Ok((
            merge_confirmed(stamped(row, 0)?, details.merged, time),
            merge_field(stamped(row, 2)?, details.base_oid, time),
            merge_field(stamped(row, 4)?, details.head_oid, time),
            merge_field(stamped(row, 6)?, details.merge_oid, time),
        )),
    )?;
    tx.execute(
        "UPDATE pull_request_details SET merged = :merged, merged_at = :merged_at,
                base_oid = :base, base_oid_at = :base_at, head_oid = :head, head_oid_at = :head_at,
                merge_oid = :merge, merge_oid_at = :merge_at
         WHERE item_id = :item",
        named_params! {
            ":item": item, ":merged": merged, ":merged_at": merged_at,
            ":base": base, ":base_at": base_at, ":head": head, ":head_at": head_at,
            ":merge": merge, ":merge_at": merge_at,
        },
    )?;
    Ok(())
}

fn verify_transfer(
    connection: &Connection,
    item: ItemId,
    evidence: TransferEvidence,
) -> Result<bool> {
    let (id, node): (Option<String>, Option<String>) = connection
        .query_row(
            "SELECT issue_api_id, issue_node_id FROM items WHERE id = ?1 AND kind = 'issue'",
            [item],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .unwrap_or_default();
    Ok(match evidence {
        TransferEvidence::Unverified => false,
        TransferEvidence::VerifiedIssueId(proven) => {
            id.as_deref() == Some(&proven.get().to_string())
        }
        TransferEvidence::VerifiedIssueNode(proven) => node.as_deref() == Some(proven.as_str()),
    })
}

fn map_item(row: &Row<'_>) -> rusqlite::Result<Item> {
    let kind = kind(row, 3)?;
    let pull_request = match api_id(row, 12)? {
        Some(id) if kind.permits_pr_details() => Some(PullRequestDetails {
            id,
            node_id: row.get::<_, Option<String>>(13)?.map(NodeId),
            fields: PullRequestUpdate {
                merged: field(row, 14)?,
                base_oid: field(row, 16)?,
                head_oid: field(row, 18)?,
                merge_oid: field(row, 20)?,
            },
        }),
        Some(_) => return Err(invalid_row(12)),
        None => None,
    };
    Ok(Item {
        id: row.get(0)?,
        locator: ItemLocator {
            repository: row.get(1)?,
            number: u64::try_from(row.get::<_, i64>(2)?).map_err(|_| invalid_row(2))?,
        },
        kind,
        issue_id: api_id(row, 4)?,
        issue_node_id: row.get::<_, Option<String>>(5)?.map(NodeId),
        title: field(row, 6)?,
        body: field(row, 8)?,
        closed: field(row, 10)?,
        pull_request,
    })
}

fn kind(row: &Row<'_>, index: usize) -> rusqlite::Result<ItemKind> {
    match row.get_ref(index)?.as_str()? {
        "unknown" => Ok(ItemKind::Unknown),
        "issue" => Ok(ItemKind::Issue),
        "pull_request" => Ok(ItemKind::PullRequest),
        _ => Err(invalid_row(index)),
    }
}

fn stamped<T: FromSql>(row: &Row<'_>, index: usize) -> rusqlite::Result<Stamped<T>> {
    Ok((row.get(index)?, row.get(index + 1)?))
}

fn field<T: FromSql>(row: &Row<'_>, index: usize) -> rusqlite::Result<Change<T>> {
    Ok(match stamped(row, index)? {
        (Some(value), _) => Change::Value(value),
        (None, Some(_)) => Change::Null,
        (None, None) => Change::Missing,
    })
}

fn api_id<T: TryFrom<u64>>(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<T>> {
    let value: Option<String> = row.get(index)?;
    value
        .map(|value| {
            let number = value.parse::<u64>().map_err(|_| invalid_row(index))?;
            T::try_from(number).map_err(|_| invalid_row(index))
        })
        .transpose()
}

fn invalid_row(index: usize) -> rusqlite::Error {
    rusqlite::Error::InvalidColumnType(
        index,
        "invalid identity field".into(),
        rusqlite::types::Type::Text,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_001_item_locator_scopes_number_and_requires_pr_evidence() {
        let one = ItemLocator {
            repository: RepositoryId(5),
            number: 42,
        };
        let two = ItemLocator {
            repository: RepositoryId(6),
            ..one
        };
        assert_ne!(one, two);
        assert_eq!(one.sql_number().unwrap(), two.sql_number().unwrap());
        assert!(ItemLocator { number: 0, ..one }.sql_number().is_err());

        let unknown = ItemKind::Unknown.observe_issue(IssueClassification::Unknown);
        assert_eq!(unknown, ItemKind::Unknown);
        assert!(!unknown.permits_pr_details());
        let issue = unknown.observe_issue(IssueClassification::Issue);
        assert!(!issue.permits_pr_details());
        assert!(
            issue
                .observe_issue(IssueClassification::PullRequest)
                .permits_pr_details()
        );
    }

    #[test]
    fn ut_003_partial_and_stale_evidence_preserve_richer_state() {
        let current = (Some("current"), Some(100));
        assert_eq!(
            merge_field(current, Change::Missing, UtcMillis(120)),
            current
        );
        assert_eq!(merge_field(current, Change::Null, UtcMillis(80)), current);
        assert_eq!(
            merge_field(current, Change::Value("conflict"), UtcMillis(100)),
            current
        );
        assert_eq!(
            merge_field(current, Change::Null, UtcMillis(120)),
            (None, Some(120))
        );
        assert_eq!(
            merge_field((None, None), Change::Value("first evidence"), UtcMillis(-1)),
            (Some("first evidence"), Some(-1))
        );
        assert_eq!(
            ItemKind::PullRequest.observe_issue(IssueClassification::Unknown),
            ItemKind::PullRequest
        );
        assert_eq!(
            ItemKind::PullRequest.observe_issue(IssueClassification::Issue),
            ItemKind::PullRequest
        );
        assert_eq!(
            merge_confirmed(
                (Some(true), Some(100)),
                Change::Value(false),
                UtcMillis(120)
            )
            .0,
            Some(true)
        );
        assert_eq!(
            merge_confirmed(
                (Some(false), Some(120)),
                Change::Value(true),
                UtcMillis(100)
            )
            .0,
            Some(true)
        );
    }
}
