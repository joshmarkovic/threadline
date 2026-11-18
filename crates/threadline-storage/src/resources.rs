use rusqlite::{Connection, OptionalExtension, Row, Transaction, params, types::FromSql};

use crate::{AccountId, Error, ItemId, RepositoryId, Result, Store};

macro_rules! stored_enum {
    ($(#[$attr:meta])* $name:ident { $($variant:ident = $value:literal),+ $(,)? }) => {
        $(#[$attr])*
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[repr(i64)]
        pub enum $name { $($variant = $value),+ }

        impl FromSql for $name {
            fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                match value.as_i64()? {
                    $($value => Ok(Self::$variant),)+
                    _ => Err(rusqlite::types::FromSqlError::OutOfRange(value.as_i64()?)),
                }
            }
        }
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceFamily {
    Metadata,
    Body,
    Comments,
    Files,
    Reviews,
    ReviewThreads,
    Checks,
    History,
    References,
}

impl ResourceFamily {
    fn name(self) -> &'static str {
        match self {
            Self::Metadata => "metadata",
            Self::Body => "body",
            Self::Comments => "comments",
            Self::Files => "files",
            Self::Reviews => "reviews",
            Self::ReviewThreads => "review-threads",
            Self::Checks => "checks",
            Self::History => "history",
            Self::References => "references",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceTarget {
    Repository(RepositoryId),
    Item(ItemId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceKey {
    pub account: AccountId,
    pub target: ResourceTarget,
    pub family: ResourceFamily,
}

impl ResourceKey {
    fn owned_columns(self, conn: &Connection) -> Result<(Option<i64>, Option<i64>)> {
        match self.target {
            ResourceTarget::Repository(repo) => {
                require_repository(conn, self.account, repo)?;
                Ok((Some(repo.0), None))
            }
            ResourceTarget::Item(item) => {
                item_repository(conn, self.account, item)?;
                Ok((None, Some(item.0)))
            }
        }
    }
}

stored_enum!(Coverage { Missing = 0, Partial = 1, Complete = 2 });
stored_enum!(FetchState { Idle = 0, Downloading = 1, Unavailable = 2, Failed = 3 });
stored_enum!(ResourceError {
    Unknown = 1,
    Unauthorized = 2,
    Forbidden = 3,
    NotFound = 4,
    ConfirmedDeletion = 5,
    RateLimited = 6,
    Transport = 7,
    InvalidResponse = 8,
    CursorExpired = 9,
});

impl ResourceError {
    fn fetch_state(self) -> FetchState {
        match self {
            Self::Unauthorized | Self::Forbidden | Self::NotFound | Self::ConfirmedDeletion => {
                FetchState::Unavailable
            }
            _ => FetchState::Failed,
        }
    }
}

stored_enum!(
    /// A full scan can discard absent records only after the final page commits.
    WalkMode { Full = 0, Incremental = 1 }
);

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub revision: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceState {
    pub coverage: Coverage,
    pub fetch: FetchState,
    pub error: Option<ResourceError>,
    pub last_page_at_ms: Option<i64>,
    pub last_fetch_at_ms: Option<i64>,
    pub last_validation_at_ms: Option<i64>,
    pub validators: Validators,
}

impl ResourceState {
    /// Completeness describes the cached data. Validation only changes its age.
    pub fn age_ms(&self, now_ms: i64) -> Option<u64> {
        let observed = self.last_validation_at_ms.or(self.last_fetch_at_ms)?;
        Some(now_ms.saturating_sub(observed).max(0) as u64)
    }

    pub fn is_stale(&self, now_ms: i64, max_age_ms: u64) -> bool {
        self.age_ms(now_ms).is_none_or(|age| age > max_age_ms)
    }

    pub fn shows_empty(&self, record_count: usize) -> bool {
        self.coverage == Coverage::Complete && record_count == 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceProgress {
    resource_id: i64,
    generation: i64,
    pub cursor: Option<String>,
    pub mode: WalkMode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceRecord {
    pub key: String,
    pub body: String,
}

pub struct ResourcePage<'a> {
    pub records: &'a [ResourceRecord],
    pub next_cursor: Option<&'a str>,
    pub completed_validators: Option<&'a Validators>,
    pub fetched_at_ms: i64,
}

fn missing_state() -> ResourceState {
    ResourceState {
        coverage: Coverage::Missing,
        fetch: FetchState::Idle,
        error: None,
        last_page_at_ms: None,
        last_fetch_at_ms: None,
        last_validation_at_ms: None,
        validators: Validators::default(),
    }
}

fn resource_id(conn: &Connection, key: ResourceKey) -> Result<Option<i64>> {
    let (sql, target) = match key.target {
        ResourceTarget::Repository(repo) => {
            require_repository(conn, key.account, repo)?;
            (
                "SELECT id FROM cached_resources WHERE repository_id=?1 AND family=?2 AND account_id=?3",
                repo.0,
            )
        }
        ResourceTarget::Item(item) => {
            item_repository(conn, key.account, item)?;
            (
                "SELECT id FROM cached_resources WHERE item_id=?1 AND family=?2 AND account_id=?3",
                item.0,
            )
        }
    };
    Ok(conn
        .prepare_cached(sql)?
        .query_row(params![target, key.family.name(), key.account.0], |row| {
            row.get(0)
        })
        .optional()?)
}

pub(crate) fn ensure_resource(conn: &Connection, key: ResourceKey) -> Result<i64> {
    let (repository, item) = key.owned_columns(conn)?;
    let inserted = conn.prepare_cached(
        "INSERT INTO cached_resources(account_id,repository_id,item_id,family) VALUES (?1,?2,?3,?4)
         ON CONFLICT DO NOTHING RETURNING id",
    )?.query_row(params![key.account.0, repository, item, key.family.name()], |row| row.get(0)).optional()?;
    match inserted {
        Some(id) => Ok(id),
        None => {
            resource_id(conn, key)?.ok_or(Error::Conflict("resource belongs to another account"))
        }
    }
}

fn read_state(row: &Row<'_>) -> rusqlite::Result<ResourceState> {
    Ok(ResourceState {
        coverage: row.get(0)?,
        fetch: row.get(1)?,
        error: row.get(2)?,
        last_page_at_ms: row.get(3)?,
        last_fetch_at_ms: row.get(4)?,
        last_validation_at_ms: row.get(5)?,
        validators: Validators {
            etag: row.get(6)?,
            last_modified: row.get(7)?,
            revision: row.get(8)?,
        },
    })
}

fn read_progress(
    conn: &Connection,
    resource_id: i64,
) -> Result<Option<(ResourceProgress, FetchState, bool)>> {
    Ok(conn.query_row(
        "SELECT generation,continuation,walk_mode,fetch,has_page FROM cached_resources WHERE id=?1",
        [resource_id],
        |row| {
            Ok((
                ResourceProgress { resource_id, generation: row.get(0)?, cursor: row.get(1)?, mode: row.get(2)? },
                row.get(3)?,
                row.get(4)?,
            ))
        },
    ).optional()?)
}

fn require_walk(
    conn: &Connection,
    key: ResourceKey,
    expected: &ResourceProgress,
) -> Result<(i64, bool)> {
    let id = resource_id(conn, key)?.ok_or(Error::Conflict("resource fetch does not exist"))?;
    match read_progress(conn, id)? {
        Some((current, FetchState::Downloading, has_page)) if current == *expected => {
            Ok((id, has_page))
        }
        _ => Err(Error::Conflict("resource checkpoint changed before commit")),
    }
}

impl Store {
    pub fn resource_state(&self, key: ResourceKey) -> Result<ResourceState> {
        self.read(|conn| {
            let Some(id) = resource_id(conn, key)? else {
                return Ok(missing_state());
            };
            Ok(conn.query_row(
                "SELECT coverage,fetch,error,last_page_at_ms,last_fetch_at_ms,last_validation_at_ms,
                 etag,last_modified,revision FROM cached_resources WHERE id=?1",
                [id],
                read_state,
            )?)
        })
    }

    pub fn resource_records(&self, key: ResourceKey) -> Result<Vec<ResourceRecord>> {
        self.read(|conn| {
            let Some(id) = resource_id(conn, key)? else { return Ok(Vec::new()); };
            query_many(conn, "SELECT record_key,body FROM cached_resource_records WHERE resource_id=?1 ORDER BY record_key", [id],
                |row| Ok(ResourceRecord { key: row.get(0)?, body: row.get(1)? }))
        })
    }

    /// Start a new walk while keeping the last readable data, even if its old cursor expired.
    pub fn start_resource_fetch(
        &self,
        key: ResourceKey,
        mode: WalkMode,
    ) -> Result<ResourceProgress> {
        self.write(|tx| {
            let id = ensure_resource(tx, key)?;
            let coverage: Coverage = tx.query_row(
                "SELECT coverage FROM cached_resources WHERE id=?1",
                [id],
                |row| row.get(0),
            )?;
            if mode == WalkMode::Incremental && coverage != Coverage::Complete {
                return Err(Error::Conflict(
                    "an incremental walk needs complete cached coverage",
                ));
            }
            discard_walk(tx, id)?;
            let generation = tx.query_row(
                "UPDATE cached_resources SET generation=generation+1, fetch=1, error=NULL,
                 walk_mode=?2, continuation=NULL, has_page=0 WHERE id=?1 RETURNING generation",
                params![id, mode as i64],
                |row| row.get(0),
            )?;
            Ok(ResourceProgress {
                resource_id: id,
                generation,
                cursor: None,
                mode,
            })
        })
    }

    /// Resume an interrupted attempt using the cursor committed with its preceding page.
    pub fn resume_resource_fetch(&self, key: ResourceKey) -> Result<Option<ResourceProgress>> {
        self.write(|tx| {
            let Some(id) = resource_id(tx, key)? else { return Ok(None); };
            let error: Option<ResourceError> = tx.query_row("SELECT error FROM cached_resources WHERE id=?1", [id], |row| row.get(0))?;
            if error == Some(ResourceError::CursorExpired) { return Ok(None); }
            match read_progress(tx, id)? {
                Some((mut progress, fetch, _)) if fetch != FetchState::Idle => {
                    progress.generation = tx.query_row(
                        "UPDATE cached_resources SET generation=generation+1,fetch=1,error=NULL WHERE id=?1 RETURNING generation",
                        [id],
                        |row| row.get(0),
                    )?;
                    Ok(Some(progress))
                }
                _ => Ok(None),
            }
        })
    }

    pub fn commit_resource_page(
        &self,
        key: ResourceKey,
        expected: &ResourceProgress,
        page: ResourcePage<'_>,
    ) -> Result<Option<ResourceProgress>> {
        if page.next_cursor == Some("")
            || (page.next_cursor.is_some() && page.completed_validators.is_some())
        {
            return Err(Error::InvalidInput("invalid resource page metadata"));
        }
        self.write(|tx| {
            let (id, _) = require_walk(tx, key, expected)?;
            if let Some(cursor) = page.next_cursor {
                let fresh = tx.execute(
                    "INSERT INTO cached_resource_cursors(resource_id,cursor) VALUES (?1,?2) ON CONFLICT DO NOTHING",
                    params![id, cursor],
                )?;
                if fresh == 0 {
                    return Err(Error::Conflict("resource page repeated a continuation cursor"));
                }
            }
            let coverage: Coverage = tx.query_row("SELECT coverage FROM cached_resources WHERE id=?1", [id], |row| row.get(0))?;
            let show_partial = page.next_cursor.is_some() && coverage != Coverage::Complete;
            let mut write = tx.prepare_cached(
                "INSERT INTO cached_resource_pending(resource_id,record_key,body) VALUES (?1,?2,?3)
                 ON CONFLICT(resource_id,record_key) DO UPDATE SET body=excluded.body",
            )?;
            let mut write_visible = tx.prepare_cached(
                "INSERT INTO cached_resource_records(resource_id,record_key,body) VALUES (?1,?2,?3)
                 ON CONFLICT(resource_id,record_key) DO UPDATE SET body=excluded.body",
            )?;
            for record in page.records {
                require_label(&record.key)?;
                write.execute(params![id, record.key, record.body])?;
                if show_partial { write_visible.execute(params![id,record.key,record.body])?; }
            }
            if let Some(cursor) = page.next_cursor {
                tx.execute(
                    "UPDATE cached_resources SET coverage=MAX(coverage,1),has_page=1,continuation=?2,
                     last_page_at_ms=?3 WHERE id=?1",
                    params![id,cursor,page.fetched_at_ms],
                )?;
                return Ok(Some(ResourceProgress { cursor: Some(cursor.into()), ..expected.clone() }));
            }
            if expected.mode == WalkMode::Full {
                tx.execute("DELETE FROM cached_resource_records WHERE resource_id=?1", [id])?;
            }
            publish_pending_records(tx, id)?;
            let validators = page.completed_validators.cloned().unwrap_or_default();
            tx.execute(
                "UPDATE cached_resources SET coverage=2,fetch=0,error=NULL,has_page=0,continuation=NULL,
                 last_page_at_ms=?2,last_fetch_at_ms=?2,last_validation_at_ms=?2,etag=?3,last_modified=?4,revision=?5
                 WHERE id=?1",
                params![id,page.fetched_at_ms,validators.etag,validators.last_modified,validators.revision],
            )?;
            discard_walk(tx, id)?;
            Ok(None)
        })
    }

    pub fn resource_not_modified(
        &self,
        key: ResourceKey,
        expected: &ResourceProgress,
        matched: &Validators,
        at_ms: i64,
    ) -> Result<()> {
        self.write(|tx| {
            let (id, has_page) = require_walk(tx, key, expected)?;
            if has_page || (matched.etag.is_none() && matched.last_modified.is_none()) {
                return Err(Error::Conflict("conditional validation requires an unchanged complete resource"));
            }
            let updated = tx.execute(
                "UPDATE cached_resources SET fetch=0,error=NULL,last_validation_at_ms=?2
                 WHERE id=?1 AND coverage=2 AND etag IS ?3 AND last_modified IS ?4 AND revision IS ?5",
                params![id,at_ms,matched.etag,matched.last_modified,matched.revision],
            )?;
            if updated != 1 {
                return Err(Error::Conflict("conditional response does not match cached validators"));
            }
            Ok(())
        })
    }

    pub fn record_resource_error(
        &self,
        key: ResourceKey,
        expected: &ResourceProgress,
        reason: ResourceError,
    ) -> Result<()> {
        self.write(|tx| {
            let (id, _) = require_walk(tx, key, expected)?;
            tx.execute(
                "UPDATE cached_resources SET fetch=?2,error=?3 WHERE id=?1",
                params![id, reason.fetch_state() as i64, reason as i64],
            )?;
            Ok(())
        })
    }
}

fn publish_pending_records(tx: &Transaction<'_>, id: i64) -> Result<()> {
    tx.execute(
        "INSERT INTO cached_resource_records(resource_id,record_key,body)
         SELECT resource_id,record_key,body FROM cached_resource_pending WHERE resource_id=?1
         ON CONFLICT(resource_id,record_key) DO UPDATE SET body=excluded.body",
        [id],
    )?;
    Ok(())
}

fn discard_walk(tx: &Transaction<'_>, id: i64) -> Result<()> {
    tx.execute(
        "DELETE FROM cached_resource_pending WHERE resource_id=?1",
        [id],
    )?;
    tx.execute(
        "DELETE FROM cached_resource_cursors WHERE resource_id=?1",
        [id],
    )?;
    Ok(())
}

fn require_repository(conn: &Connection, account: AccountId, repo: RepositoryId) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM repositories WHERE id=?1 AND account_id=?2)",
        params![repo.0, account.0],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(Error::Conflict("repository is not in this account"));
    }
    Ok(())
}

fn item_repository(conn: &Connection, account: AccountId, item: ItemId) -> Result<RepositoryId> {
    conn.query_row(
        "SELECT repositories.id FROM items JOIN repositories ON repositories.id=items.repository_id
         WHERE items.id=?1 AND repositories.account_id=?2",
        params![item.0, account.0],
        |row| row.get::<_, i64>(0).map(RepositoryId),
    )
    .optional()?
    .ok_or(Error::Conflict("item is not in this account"))
}

fn require_label(value: &str) -> Result<()> {
    if value.is_empty() || value.contains('\0') {
        return Err(Error::InvalidInput("missing or invalid resource identity"));
    }
    Ok(())
}

fn query_many<T>(
    conn: &Connection,
    sql: &str,
    values: impl rusqlite::Params,
    decode: impl FnMut(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    let mut query = conn.prepare_cached(sql)?;
    Ok(query
        .query_map(values, decode)?
        .collect::<rusqlite::Result<_>>()?)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitOid(String);

impl CommitOid {
    pub fn parse(value: &str) -> Result<Self> {
        if !matches!(value.len(), 40 | 64) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::InvalidInput("invalid immutable commit identity"));
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

stored_enum!(ComparisonMode { Direct = 0, MergeBase = 1 });

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComparisonKey {
    pub repository: RepositoryId,
    pub base: CommitOid,
    pub head: CommitOid,
    pub mode: ComparisonMode,
    pub ignore_whitespace: bool,
    pub context_lines: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComparisonFile {
    /// Remote labels are stored as data, never opened as local paths.
    pub path: String,
    pub previous_path: Option<String>,
    pub patch: Option<String>,
}

fn comparison_id(conn: &Connection, key: &ComparisonKey) -> Result<Option<i64>> {
    Ok(conn.query_row(
        "SELECT id FROM cached_comparisons WHERE repository_id=?1 AND base_commit=?2 AND head_commit=?3
         AND compare_mode=?4 AND ignore_whitespace=?5 AND context_lines=?6",
        params![key.repository.0,key.base.as_str(),key.head.as_str(),key.mode as i64,key.ignore_whitespace,key.context_lines],
        |row| row.get(0),
    ).optional()?)
}

fn comparison_files(conn: &Connection, id: i64) -> Result<Vec<ComparisonFile>> {
    query_many(
        conn,
        "SELECT path_label,previous_path_label,patch FROM cached_comparison_files WHERE comparison_id=?1 ORDER BY file_index",
        [id],
        |row| {
            Ok(ComparisonFile {
                path: row.get(0)?,
                previous_path: row.get(1)?,
                patch: row.get(2)?,
            })
        },
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphCommit {
    pub oid: CommitOid,
    pub title: String,
    pub committed_at_ms: i64,
    pub parents: Vec<CommitOid>,
}

fn graph_commit(
    conn: &Connection,
    repo: RepositoryId,
    oid: &CommitOid,
) -> Result<Option<GraphCommit>> {
    let Some((title, committed_at_ms)) = conn
        .query_row(
            "SELECT title,committed_at_ms FROM cached_commits WHERE repository_id=?1 AND oid=?2",
            params![repo.0, oid.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let parents: Vec<String> = query_many(
        conn,
        "SELECT parent_oid FROM cached_commit_parents WHERE repository_id=?1 AND child_oid=?2 ORDER BY parent_index",
        params![repo.0, oid.as_str()],
        |row| row.get(0),
    )?;
    let parents = parents
        .iter()
        .map(|id| CommitOid::parse(id))
        .collect::<Result<_>>()?;
    Ok(Some(GraphCommit {
        oid: oid.clone(),
        title,
        committed_at_ms,
        parents,
    }))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphRef {
    pub name: String,
    pub target: Option<CommitOid>,
    pub observed_at_ms: i64,
}

stored_enum!(RevisionFamily { Checks = 0, Review = 1 });

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevisionState {
    pub payload: String,
    pub observed_at_ms: i64,
}

impl Store {
    pub fn save_comparison(
        &self,
        account: AccountId,
        key: &ComparisonKey,
        files: &[ComparisonFile],
    ) -> Result<()> {
        self.write(|tx| {
            require_repository(tx, account, key.repository)?;
            if let Some(id) = comparison_id(tx, key)? {
                if comparison_files(tx, id)? != files {
                    return Err(Error::Conflict("immutable comparison contents changed"));
                }
                return Ok(());
            }
            tx.execute(
                "INSERT INTO cached_comparisons(repository_id,base_commit,head_commit,compare_mode,ignore_whitespace,context_lines)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![key.repository.0,key.base.as_str(),key.head.as_str(),key.mode as i64,key.ignore_whitespace,key.context_lines],
            )?;
            let id = tx.last_insert_rowid();
            let mut insert = tx.prepare_cached(
                "INSERT INTO cached_comparison_files(comparison_id,file_index,path_label,previous_path_label,patch)
                 VALUES (?1,?2,?3,?4,?5)",
            )?;
            for (index,file) in files.iter().enumerate() {
                require_label(&file.path)?;
                if let Some(previous) = &file.previous_path { require_label(previous)?; }
                let index = i64::try_from(index).map_err(|_| Error::InvalidInput("too many comparison files"))?;
                insert.execute(params![id,index,file.path,file.previous_path,file.patch])?;
            }
            Ok(())
        })
    }

    pub fn load_comparison(
        &self,
        account: AccountId,
        key: &ComparisonKey,
    ) -> Result<Option<Vec<ComparisonFile>>> {
        self.read(|conn| {
            require_repository(conn, account, key.repository)?;
            comparison_id(conn, key)?
                .map(|id| comparison_files(conn, id))
                .transpose()
        })
    }

    pub fn save_commits(
        &self,
        account: AccountId,
        repo: RepositoryId,
        commits: &[GraphCommit],
    ) -> Result<()> {
        self.write(|tx| {
            require_repository(tx, account, repo)?;
            let mut insert = tx.prepare_cached(
                "INSERT INTO cached_commits(repository_id,oid,title,committed_at_ms) VALUES (?1,?2,?3,?4)
                 ON CONFLICT DO NOTHING",
            )?;
            let mut insert_parent = tx.prepare_cached(
                "INSERT INTO cached_commit_parents(repository_id,child_oid,parent_index,parent_oid) VALUES (?1,?2,?3,?4)",
            )?;
            for commit in commits {
                if commit.parents.contains(&commit.oid) { return Err(Error::InvalidInput("commit cannot be its own parent")); }
                let inserted = insert.execute(params![repo.0,commit.oid.as_str(),commit.title,commit.committed_at_ms])?;
                if inserted == 0 {
                    if graph_commit(tx, repo, &commit.oid)?.as_ref() != Some(commit) {
                        return Err(Error::Conflict("immutable commit contents changed"));
                    }
                    continue;
                }
                for (index,parent) in commit.parents.iter().enumerate() {
                    let index = i64::try_from(index).map_err(|_| Error::InvalidInput("too many commit parents"))?;
                    insert_parent.execute(params![repo.0,commit.oid.as_str(),index,parent.as_str()])?;
                }
            }
            Ok(())
        })
    }

    pub fn load_commit(
        &self,
        account: AccountId,
        repo: RepositoryId,
        oid: &CommitOid,
    ) -> Result<Option<GraphCommit>> {
        self.read(|conn| {
            require_repository(conn, account, repo)?;
            graph_commit(conn, repo, oid)
        })
    }

    /// A missing ref target records an observed deletion without erasing commits.
    pub fn save_graph_ref(
        &self,
        account: AccountId,
        repo: RepositoryId,
        reference: &GraphRef,
    ) -> Result<bool> {
        require_label(&reference.name)?;
        self.write(|tx| {
            require_repository(tx, account, repo)?;
            Ok(tx.execute(
                "INSERT INTO cached_refs(repository_id,name,oid,observed_at_ms) VALUES (?1,?2,?3,?4)
                 ON CONFLICT(repository_id,name) DO UPDATE SET oid=excluded.oid,observed_at_ms=excluded.observed_at_ms
                 WHERE excluded.observed_at_ms > cached_refs.observed_at_ms",
                params![repo.0,reference.name,reference.target.as_ref().map(CommitOid::as_str),reference.observed_at_ms],
            )? == 1)
        })
    }

    pub fn load_graph_ref(
        &self,
        account: AccountId,
        repo: RepositoryId,
        name: &str,
    ) -> Result<Option<GraphRef>> {
        self.read(|conn| {
            require_repository(conn, account, repo)?;
            let raw: Option<(Option<String>, i64)> = conn
                .query_row(
                    "SELECT oid,observed_at_ms FROM cached_refs WHERE repository_id=?1 AND name=?2",
                    params![repo.0, name],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            raw.map(|(target, observed_at_ms)| {
                Ok(GraphRef {
                    name: name.into(),
                    target: target.as_deref().map(CommitOid::parse).transpose()?,
                    observed_at_ms,
                })
            })
            .transpose()
        })
    }

    pub fn save_revision_state(
        &self,
        account: AccountId,
        item: ItemId,
        family: RevisionFamily,
        oid: &CommitOid,
        state: &RevisionState,
    ) -> Result<bool> {
        self.write(|tx| {
            item_repository(tx, account, item)?;
            Ok(tx.execute(
                "INSERT INTO cached_revision_states(item_id,family,oid,payload,observed_at_ms) VALUES (?1,?2,?3,?4,?5)
                 ON CONFLICT(item_id,family,oid) DO UPDATE SET payload=excluded.payload,observed_at_ms=excluded.observed_at_ms
                 WHERE excluded.observed_at_ms > cached_revision_states.observed_at_ms",
                params![item.0,family as i64,oid.as_str(),state.payload,state.observed_at_ms],
            )? == 1)
        })
    }

    pub fn load_revision_state(
        &self,
        account: AccountId,
        item: ItemId,
        family: RevisionFamily,
        oid: &CommitOid,
    ) -> Result<Option<RevisionState>> {
        self.read(|conn| {
            item_repository(conn, account, item)?;
            Ok(conn.query_row(
                "SELECT payload,observed_at_ms FROM cached_revision_states WHERE item_id=?1 AND family=?2 AND oid=?3",
                params![item.0,family as i64,oid.as_str()],
                |row| Ok(RevisionState { payload: row.get(0)?, observed_at_ms: row.get(1)? }),
            ).optional()?)
        })
    }
}

stored_enum!(SyncPhase { Queued = 0, Running = 1, Held = 2, Settled = 3 });

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncJob {
    pub phase: SyncPhase,
    pub resume_after_ms: Option<i64>,
    pub checkpoint: Option<String>,
    pub overlap_from_ms: Option<i64>,
    pub updated_at_ms: i64,
    pub reason: Option<ResourceError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityBaseline {
    pub checkpoint: String,
    pub observed_at_ms: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityEvent {
    pub repository: RepositoryId,
    pub item: Option<ItemId>,
    pub kind: String,
    pub revision: Option<String>,
    pub occurred_at_ms: Option<i64>,
    pub observed_at_ms: i64,
}

stored_enum!(DeliveryPhase { Eligible = 0, Suppressed = 1, Delivered = 2, Unknown = 3 });

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryState {
    pub phase: DeliveryPhase,
    pub group_key: Option<String>,
    pub native_id: Option<String>,
    pub reason: Option<String>,
    pub updated_at_ms: i64,
}

impl Store {
    pub fn save_sync_job(
        &self,
        account: AccountId,
        repo: RepositoryId,
        family: ResourceFamily,
        job: &SyncJob,
    ) -> Result<bool> {
        self.write(|tx| {
            require_repository(tx, account, repo)?;
            Ok(tx.execute(
                "INSERT INTO sync_jobs(repository_id,family,phase,resume_after_ms,updated_at_ms,reason,checkpoint,overlap_from_ms)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
                 ON CONFLICT(repository_id,family) DO UPDATE SET phase=excluded.phase,resume_after_ms=excluded.resume_after_ms,
                 updated_at_ms=excluded.updated_at_ms,reason=excluded.reason,checkpoint=excluded.checkpoint,
                 overlap_from_ms=excluded.overlap_from_ms
                 WHERE excluded.updated_at_ms > sync_jobs.updated_at_ms",
                params![repo.0,family.name(),job.phase as i64,job.resume_after_ms,job.updated_at_ms,job.reason.map(|r| r as i64),
                    job.checkpoint,job.overlap_from_ms],
            )? == 1)
        })
    }

    pub fn load_sync_job(
        &self,
        account: AccountId,
        repo: RepositoryId,
        family: ResourceFamily,
    ) -> Result<Option<SyncJob>> {
        self.read(|conn| {
            require_repository(conn, account, repo)?;
            Ok(conn
                .query_row(
                    "SELECT phase,resume_after_ms,updated_at_ms,reason,checkpoint,overlap_from_ms
                 FROM sync_jobs WHERE repository_id=?1 AND family=?2",
                    params![repo.0, family.name()],
                    |row| {
                        Ok(SyncJob {
                            phase: row.get(0)?,
                            resume_after_ms: row.get(1)?,
                            updated_at_ms: row.get(2)?,
                            reason: row.get(3)?,
                            checkpoint: row.get(4)?,
                            overlap_from_ms: row.get(5)?,
                        })
                    },
                )
                .optional()?)
        })
    }

    pub fn save_activity_baseline(
        &self,
        account: AccountId,
        repo: RepositoryId,
        stream: &str,
        baseline: &ActivityBaseline,
    ) -> Result<bool> {
        require_label(stream)?;
        self.write(|tx| {
            require_repository(tx,account,repo)?;
            Ok(tx.execute(
                "INSERT INTO activity_baselines(repository_id,stream_key,checkpoint,observed_at_ms) VALUES (?1,?2,?3,?4)
                 ON CONFLICT(repository_id,stream_key) DO UPDATE SET checkpoint=excluded.checkpoint,observed_at_ms=excluded.observed_at_ms
                 WHERE excluded.observed_at_ms > activity_baselines.observed_at_ms",
                params![repo.0,stream,baseline.checkpoint,baseline.observed_at_ms],
            )? == 1)
        })
    }

    pub fn load_activity_baseline(
        &self,
        account: AccountId,
        repo: RepositoryId,
        stream: &str,
    ) -> Result<Option<ActivityBaseline>> {
        self.read(|conn| {
            require_repository(conn,account,repo)?;
            Ok(conn.query_row(
                "SELECT checkpoint,observed_at_ms FROM activity_baselines WHERE repository_id=?1 AND stream_key=?2",
                params![repo.0,stream],
                |row| Ok(ActivityBaseline { checkpoint: row.get(0)?, observed_at_ms: row.get(1)? }),
            ).optional()?)
        })
    }

    /// An event key represents the same remote observation across categories.
    pub fn record_activity_event(
        &self,
        account: AccountId,
        key: &str,
        event: &ActivityEvent,
    ) -> Result<bool> {
        require_label(key)?;
        require_label(&event.kind)?;
        self.write(|tx| {
            require_repository(tx,account,event.repository)?;
            if let Some(item) = event.item
                && item_repository(tx, account, item)? != event.repository
            {
                let known_alias: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM item_aliases WHERE account_id=?1 AND repository_id=?2 AND item_id=?3)",
                    params![account.0, event.repository.0, item.0],
                    |row| row.get(0),
                )?;
                if !known_alias {
                    return Err(Error::Conflict("activity item has no verified locator in the event repository"));
                }
            }
            Ok(tx.execute(
                "INSERT INTO activity_events(account_id,event_key,repository_id,item_id,event_kind,revision,occurred_at_ms,observed_at_ms)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
                 ON CONFLICT(account_id,event_key) DO NOTHING",
                params![account.0,key,event.repository.0,event.item.map(|item| item.0),event.kind,event.revision,event.occurred_at_ms,event.observed_at_ms],
            )? == 1)
        })
    }

    pub fn load_activity_event(
        &self,
        account: AccountId,
        key: &str,
    ) -> Result<Option<ActivityEvent>> {
        self.read(|conn| {
            Ok(conn
                .query_row(
                    "SELECT repository_id,item_id,event_kind,revision,occurred_at_ms,observed_at_ms
             FROM activity_events WHERE account_id=?1 AND event_key=?2",
                    params![account.0, key],
                    |row| {
                        Ok(ActivityEvent {
                            repository: RepositoryId(row.get(0)?),
                            item: row.get::<_, Option<i64>>(1)?.map(ItemId),
                            kind: row.get(2)?,
                            revision: row.get(3)?,
                            occurred_at_ms: row.get(4)?,
                            observed_at_ms: row.get(5)?,
                        })
                    },
                )
                .optional()?)
        })
    }

    /// A recorded delivery stays terminal. Unknown delivery is preserved for future reconciliation.
    pub fn save_delivery_state(
        &self,
        account: AccountId,
        key: &str,
        delivery: &DeliveryState,
    ) -> Result<bool> {
        self.write(|tx| Ok(tx.execute(
            "INSERT INTO notification_delivery(account_id,event_key,phase,group_key,native_id,reason,updated_at_ms)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(account_id,event_key) DO UPDATE SET phase=excluded.phase,group_key=excluded.group_key,
             native_id=excluded.native_id,reason=excluded.reason,updated_at_ms=excluded.updated_at_ms
             WHERE excluded.updated_at_ms > notification_delivery.updated_at_ms
                 AND notification_delivery.phase != 2
                 AND (notification_delivery.phase != 3 OR excluded.phase IN (2,3))",
            params![account.0,key,delivery.phase as i64,delivery.group_key,delivery.native_id,delivery.reason,delivery.updated_at_ms],
        )? == 1))
    }

    pub fn load_delivery_state(
        &self,
        account: AccountId,
        key: &str,
    ) -> Result<Option<DeliveryState>> {
        self.read(|conn| {
            Ok(conn
                .query_row(
                    "SELECT phase,group_key,native_id,reason,updated_at_ms
             FROM notification_delivery WHERE account_id=?1 AND event_key=?2",
                    params![account.0, key],
                    |row| {
                        Ok(DeliveryState {
                            phase: row.get(0)?,
                            group_key: row.get(1)?,
                            native_id: row.get(2)?,
                            reason: row.get(3)?,
                            updated_at_ms: row.get(4)?,
                        })
                    },
                )
                .optional()?)
        })
    }
}

pub(crate) fn clear_resource_cache(tx: &Transaction<'_>, account: AccountId) -> Result<()> {
    // Mid-walk records and cursors form one checkpoint. Keep them together.
    tx.execute(
        "DELETE FROM cached_resource_records WHERE resource_id IN (
        SELECT id FROM cached_resources WHERE account_id=?1 AND has_page=0)",
        [account.0],
    )?;
    tx.execute(
        "UPDATE cached_resources SET coverage=0,fetch=0,error=NULL,generation=generation+1,
        continuation=NULL,last_page_at_ms=NULL,last_fetch_at_ms=NULL,last_validation_at_ms=NULL,
        etag=NULL,last_modified=NULL,revision=NULL WHERE account_id=?1 AND has_page=0",
        [account.0],
    )?;
    tx.execute("DELETE FROM cached_revision_states WHERE item_id IN (
        SELECT items.id FROM items JOIN repositories ON items.repository_id=repositories.id WHERE repositories.account_id=?1)",
        [account.0])?;
    for table in ["cached_comparisons", "cached_commits", "cached_refs"] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE repository_id IN (SELECT id FROM repositories WHERE account_id=?1)"),
            [account.0],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_004_only_complete_empty_coverage_can_show_empty() {
        for coverage in [Coverage::Missing, Coverage::Partial, Coverage::Complete] {
            for fetch in [
                FetchState::Idle,
                FetchState::Downloading,
                FetchState::Unavailable,
                FetchState::Failed,
            ] {
                let state = ResourceState {
                    coverage,
                    fetch,
                    last_validation_at_ms: Some(100),
                    ..missing_state()
                };
                assert!(!state.is_stale(110, 15));
                assert!(state.is_stale(120, 15));
                assert_eq!(state.shows_empty(0), coverage == Coverage::Complete);
                assert!(!state.shows_empty(1));
            }
        }
    }
}
