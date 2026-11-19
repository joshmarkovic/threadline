//! Durable local authoring and submitted operations. No method makes network requests.
pub(crate) mod dispatch;
mod reconciliation;
mod workspace;
use crate::{AccountId, Error, ItemId, RepositoryId, Result, Store};
pub use dispatch::*;
pub use reconciliation::*;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
pub use workspace::*;

fn json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| Error::InvalidInput("invalid serialized content"))
}
fn parse<T: DeserializeOwned>(raw: &str) -> Result<T> {
    serde_json::from_str(raw).map_err(|_| Error::InvalidInput("unsupported serialized content"))
}
fn check_item(db: &Connection, account: AccountId, item: ItemId) -> Result<RepositoryId> {
    db.query_row(
        "SELECT repository_id FROM items WHERE id=? AND account_id=?",
        params![item, account],
        |r| r.get(0),
    )
    .optional()?
    .ok_or(Error::NotFound)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    EditIssue,
    Comment,
    Review,
    Reply,
    ResolveThread,
    Merge,
}
impl ActionKind {
    fn name(self) -> &'static str {
        match self {
            Self::EditIssue => "issue_edit",
            Self::Comment => "comment",
            Self::Review => "review",
            Self::Reply => "reply",
            Self::ResolveThread => "resolve_thread",
            Self::Merge => "merge",
        }
    }
    fn decode(value: &str) -> Result<Self> {
        match value {
            "issue_edit" => Ok(Self::EditIssue),
            "comment" => Ok(Self::Comment),
            "review" => Ok(Self::Review),
            "reply" => Ok(Self::Reply),
            "resolve_thread" => Ok(Self::ResolveThread),
            "merge" => Ok(Self::Merge),
            _ => Err(Error::Corrupt),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub payload: String,
    pub base: String,
    pub anchor: String,
    pub draft: Option<DraftContent>,
}
#[derive(Deserialize)]
struct LegacySnapshot {
    text: String,
    base_revision: String,
    anchor: String,
    draft: Option<DraftContent>,
}
fn snapshot(version: i64, raw: &str) -> Result<Snapshot> {
    match version {
        2 => parse(raw),
        1 => {
            let old: LegacySnapshot = parse(raw)?;
            Ok(Snapshot {
                payload: old.text,
                base: old.base_revision,
                anchor: old.anchor,
                draft: old.draft,
            })
        }
        _ => Err(Error::InvalidInput("unsupported operation payload version")),
    }
}

/// A submission copies the exact editor revision and retains it independently of future edits.
#[derive(Clone, Copy)]
pub struct DraftSubmission<'a> {
    pub id: &'a str,
    pub revision: i64,
    pub editor: &'a str,
    pub now: i64,
}
#[derive(Clone, Copy)]
pub struct Submission<'a> {
    pub id: &'a str,
    pub item: ItemId,
    pub kind: ActionKind,
    pub payload: &'a str,
    pub base: &'a str,
    pub anchor: &'a str,
    pub projection: &'a str,
    pub draft: Option<DraftSubmission<'a>>,
    pub dependencies: &'a [&'a str],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Queued,
    Validating,
    Sending,
    OutcomeUnknown,
    Waiting,
    NeedsAttention,
    Confirmed,
    Cancelled,
}
impl State {
    fn decode(v: &str) -> Result<Self> {
        match v {
            "queued" => Ok(Self::Queued),
            "validating" => Ok(Self::Validating),
            "sending" => Ok(Self::Sending),
            "unknown" => Ok(Self::OutcomeUnknown),
            "waiting" => Ok(Self::Waiting),
            "attention" => Ok(Self::NeedsAttention),
            "confirmed" => Ok(Self::Confirmed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(Error::Corrupt),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DraftSubmissionStatus {
    pub revision: i64,
    pub state: State,
    pub cancel_requested: bool,
}

impl DraftSubmissionStatus {
    pub const fn queued(revision: i64) -> Self {
        Self {
            revision,
            state: State::Queued,
            cancel_requested: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operation {
    pub id: String,
    pub account: AccountId,
    pub original_repository: RepositoryId,
    pub original_number: u64,
    pub item: ItemId,
    pub kind: ActionKind,
    pub snapshot: Snapshot,
    pub state: State,
    pub reason: Option<String>,
    pub retry_at: i64,
    pub cancel_requested: bool,
    pub attempt: i64,
    pub projection: String,
    pub dependencies: Vec<String>,
    pub draft: Option<(String, i64)>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projection {
    pub operation_id: String,
    pub state: State,
    pub value: String,
}
/// Account-scoped queue row for keyset pagination. Payloads and attempts load on inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueRow {
    pub sequence: i64,
    pub id: String,
    pub item: ItemId,
    pub kind: ActionKind,
    pub state: State,
    pub reason: Option<String>,
    pub cancel_requested: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub number: i64,
    pub claimed_at: i64,
    pub sending_at: Option<i64>,
    pub step: Option<String>,
    pub outcome: Option<String>,
    pub evidence: Option<String>,
}

fn dependencies(db: &Connection, account: AccountId, id: &str) -> Result<Vec<String>> {
    let mut q=db.prepare("SELECT predecessor FROM operation_dependencies WHERE account_id=? AND dependent=? ORDER BY predecessor")?;
    Ok(q.query_map(params![account, id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}
pub(crate) fn read_operation(
    db: &Connection,
    account: AccountId,
    id: &str,
) -> Result<Option<Operation>> {
    let row=db.query_row("SELECT original_repository,item_id,kind,payload_version,snapshot,state,reason,retry_at,cancel_requested,attempt,projection,draft_id,draft_revision,original_number
        FROM operations WHERE account_id=? AND id=?",params![account,id],|r|
        Ok((r.get(0)?,r.get(1)?,r.get::<_,String>(2)?,r.get(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get::<_,Option<String>>(11)?.zip(r.get(12)?), r.get::<_,i64>(13)?))).optional()?;
    row.map(
        |(
            original_repository,
            item,
            kind,
            version,
            value,
            state,
            reason,
            retry_at,
            cancel_requested,
            attempt,
            projection,
            draft,
            original_number,
        )| {
            Ok(Operation {
                id: id.into(),
                account,
                original_repository,
                original_number: u64::try_from(original_number).map_err(|_| Error::Corrupt)?,
                item,
                kind: ActionKind::decode(&kind)?,
                snapshot: snapshot(version, &value)?,
                state: State::decode(&state)?,
                reason,
                retry_at,
                cancel_requested,
                attempt,
                projection,
                dependencies: dependencies(db, account, id)?,
                draft,
            })
        },
    )
    .transpose()
}
fn uuid(text: &str) -> bool {
    text.len() == 36
        && text.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit() && !b.is_ascii_uppercase()
            }
        })
}
pub(crate) fn enqueue_tx(
    tx: &Transaction<'_>,
    account: AccountId,
    input: Submission<'_>,
) -> Result<()> {
    if !uuid(input.id) {
        return Err(Error::InvalidInput(
            "operation identity must be a lowercase UUID",
        ));
    }
    let mut deps = input.dependencies.to_vec();
    deps.sort_unstable();
    if deps.windows(2).any(|d| d[0] == d[1]) {
        return Err(Error::InvalidInput("duplicate dependency"));
    }
    let repo = check_item(tx, account, input.item)?;
    let draft_identity = input.draft.map(|d| (d.id.to_owned(), d.revision));
    if let Some(op) = read_operation(tx, account, input.id)? {
        if op.item == input.item
            && op.kind == input.kind
            && op.snapshot.payload == input.payload
            && op.snapshot.base == input.base
            && op.snapshot.anchor == input.anchor
            && op.projection == input.projection
            && op.dependencies == deps
            && op.draft == draft_identity
        {
            return Ok(());
        }
        return Err(Error::Conflict(
            "operation identity already has different intent",
        ));
    }
    check_kind(tx, account, input.item, input.kind.name())?;
    let draft = if let Some(reference) = input.draft {
        let owned:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM drafts WHERE account_id=? AND id=? AND revision=? AND editor=? AND editor_until>?)",
            params![account,reference.id,reference.revision,reference.editor,reference.now],|r|r.get(0))?;
        let draft = workspace::read_draft(tx, account, reference.id)?.ok_or(Error::NotFound)?;
        if !owned || draft.item != input.item || draft.kind.name() != input.kind.name() {
            return Err(Error::Conflict(
                "submission does not own the requested draft revision",
            ));
        }
        Some(draft.content)
    } else {
        None
    };
    let value = Snapshot {
        payload: input.payload.into(),
        base: input.base.into(),
        anchor: input.anchor.into(),
        draft,
    };
    tx.execute("INSERT INTO operations(account_id,id,original_repository,original_number,item_id,kind,payload_version,snapshot,projection,draft_id,draft_revision)
        VALUES(?1,?2,?3,(SELECT number FROM items WHERE id=?4 AND account_id=?1),?4,?5,2,?6,?7,?8,?9)",params![account,input.id,repo,input.item,input.kind.name(),json(&value)?,input.projection,
        input.draft.map(|d|d.id),input.draft.map(|d|d.revision)])?;
    for dep in deps {
        tx.execute(
            "INSERT INTO operation_dependencies VALUES(?,?,?)",
            params![account, input.id, dep],
        )?;
    }
    Ok(())
}

fn check_kind(db: &Connection, account: AccountId, item: ItemId, action: &str) -> Result<()> {
    let kind: String = db
        .query_row(
            "SELECT kind FROM items WHERE id=? AND account_id=?",
            params![item, account],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    let eligible = match action {
        "issue_edit" => kind == "issue",
        "comment" => kind == "issue" || kind == "pull_request",
        _ => kind == "pull_request",
    };
    if eligible {
        Ok(())
    } else {
        Err(Error::InvalidInput(
            "operation is incompatible with the identified item type",
        ))
    }
}
impl Store {
    /// Generate a stable UUID before submission. A caller retries the same intent with the same ID.
    pub fn new_operation_id(&self) -> Result<String> {
        self.read(|db| {
            let mut b: [u8; 16] = db.query_row("SELECT randomblob(16)", [], |r| r.get(0))?;
            b[6] = (b[6] & 15) | 64;
            b[8] = (b[8] & 63) | 128;
            let hex: String = b.iter().map(|b| format!("{b:02x}")).collect();
            Ok(format!(
                "{}-{}-{}-{}-{}",
                &hex[..8],
                &hex[8..12],
                &hex[12..16],
                &hex[16..20],
                &hex[20..]
            ))
        })
    }
    pub fn enqueue(&self, account: AccountId, input: Submission<'_>) -> Result<()> {
        self.write(|tx| enqueue_tx(tx, account, input))
    }
    pub fn operation(&self, account: AccountId, id: &str) -> Result<Option<Operation>> {
        self.read(|db| read_operation(db, account, id))
    }
    /// Read the operation bound to this account and saved draft revision.
    pub fn draft_submission_status(
        &self,
        account: AccountId,
        draft: &str,
        revision: i64,
    ) -> Result<Option<DraftSubmissionStatus>> {
        self.read(|db| {
            db.query_row(
                "SELECT state, cancel_requested FROM operations
                 WHERE account_id=? AND draft_id=? AND draft_revision=?",
                params![account, draft, revision],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
            )
            .optional()?
            .map(|(state, cancel_requested)| {
                Ok(DraftSubmissionStatus {
                    revision,
                    state: State::decode(&state)?,
                    cancel_requested,
                })
            })
            .transpose()
        })
    }
    pub fn queue_page(&self, account: AccountId, after: i64, limit: u16) -> Result<Vec<QueueRow>> {
        if limit == 0 || limit > 256 {
            return Err(Error::InvalidInput(
                "queue page size must be between 1 and 256",
            ));
        }
        self.read(|db| {
            let mut statement = db.prepare(
                "SELECT sequence,id,item_id,kind,state,reason,cancel_requested
                 FROM operations WHERE account_id=? AND sequence>? ORDER BY sequence LIMIT ?",
            )?;
            let rows = statement.query_map(params![account, after, limit], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })?;
            rows.map(|row| {
                let (sequence, id, item, kind, state, reason, cancel_requested) = row?;
                Ok(QueueRow {
                    sequence,
                    id,
                    item,
                    kind: ActionKind::decode(&kind)?,
                    state: State::decode(&state)?,
                    reason,
                    cancel_requested,
                })
            })
            .collect()
        })
    }
    pub fn dependents(&self, account: AccountId, id: &str) -> Result<Vec<String>> {
        self.read(|db|{
            let mut q=db.prepare("SELECT dependent FROM operation_dependencies WHERE account_id=? AND predecessor=? ORDER BY dependent")?;
            Ok(q.query_map(params![account,id],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?)
        })
    }
    pub fn projections(&self, account: AccountId, item: ItemId) -> Result<Vec<Projection>> {
        self.read(|db|{
            let mut q=db.prepare("SELECT id,state,projection FROM operations WHERE account_id=? AND item_id=? AND state NOT IN ('confirmed','cancelled') ORDER BY sequence")?;
            let rows=q.query_map(params![account,item],|r|Ok((r.get(0)?,r.get::<_,String>(1)?,r.get(2)?)))?;
            rows.map(|row|{let(operation_id,state,value)=row?;Ok(Projection{operation_id,state:State::decode(&state)?,value})}).collect()
        })
    }
    pub fn attempts(&self, account: AccountId, id: &str) -> Result<Vec<Attempt>> {
        self.read(|db|{
            let mut q=db.prepare("SELECT attempt,claimed_at,sending_at,step,outcome,evidence FROM operation_attempts WHERE account_id=? AND operation_id=? ORDER BY attempt")?;
            Ok(q.query_map(params![account,id],|r|Ok(Attempt{number:r.get(0)?,claimed_at:r.get(1)?,sending_at:r.get(2)?,step:r.get(3)?,outcome:r.get(4)?,evidence:r.get(5)?}))?.collect::<rusqlite::Result<_>>()?)
        })
    }
    /// Rewrite earlier supported representations in a single transaction without changing operation intent.
    pub fn migrate_operation_payloads(&self, account: AccountId) -> Result<usize> {
        self.write(|tx|{
            let mut q=tx.prepare("SELECT id,payload_version,snapshot FROM operations WHERE account_id=? AND payload_version<>2")?;
            let rows=q.query_map([account],|r|Ok((r.get::<_,String>(0)?,r.get(1)?,r.get::<_,String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            for (id,version,raw) in &rows{
                tx.execute("UPDATE operations SET payload_version=2,snapshot=? WHERE account_id=? AND id=?",params![json(&snapshot(*version,raw)?)?,account,id])?;
            }
            Ok(rows.len())
        })
    }
}

/// Used inside the large synthetic fixture's single database transaction.
pub(crate) fn load_fixture_outbox(
    tx: &Transaction<'_>,
    account: i64,
    targets: &[i64],
    seed: u64,
    count: usize,
) -> Result<()> {
    let account = AccountId(account);
    if targets.is_empty() {
        return Err(Error::InvalidInput("outbox fixture needs issue targets"));
    }
    let mut previous = String::new();
    for i in 0..count {
        let id = format!("00000000-0000-4000-8000-{i:012x}");
        let inherited = [previous.as_str()];
        let deps: &[&str] = if i % 10 == 1 { &inherited } else { &[] };
        let body = format!("Fixture {seed} operation {i}");
        let item = ItemId(targets[(i + seed as usize % targets.len()) % targets.len()]);
        enqueue_tx(
            tx,
            account,
            Submission {
                id: &id,
                item,
                kind: if i % 3 == 0 {
                    ActionKind::EditIssue
                } else {
                    ActionKind::Comment
                },
                payload: &body,
                base: "fixture-base",
                anchor: "",
                projection: &body,
                draft: None,
                dependencies: deps,
            },
        )?;
        match i % 10 {
            2 => {
                tx.execute("UPDATE operations SET state='attention',reason='base_changed' WHERE account_id=? AND id=?",params![account,id])?;
            }
            3 => {
                tx.execute("INSERT INTO step_receipts VALUES(?,?,'publish','comment',?,'synthetic confirmed fixture')",params![account,id,format!("server-{i}")])?;
                tx.execute(
                    "UPDATE operations SET state='confirmed' WHERE account_id=? AND id=?",
                    params![account, id],
                )?;
            }
            _ => (),
        }
        previous = id;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disk_full_rolls_back_enqueue_and_keeps_the_saved_editor() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_profile(dir.path()).unwrap();
        let account = store.ensure_account("github.com", 1).unwrap();
        let repo = store
            .ensure_repository(account, 1, "fixture", "one")
            .unwrap();
        let item = store.ensure_placeholder(account, repo, 41).unwrap();
        store
            .write(|tx| {
                tx.execute("UPDATE items SET kind='issue' WHERE id=?", [item])?;
                Ok(())
            })
            .unwrap();
        let draft = DraftContent {
            body: "recoverable".into(),
            base: "base".into(),
            anchor: "".into(),
        };
        store
            .create_draft(account, "draft", item, DraftKind::Comment, &draft)
            .unwrap();
        store
            .acquire_editor(account, "draft", "editor", 0, 10)
            .unwrap();
        store
            .write(|tx| {
                let pages: i64 = tx.pragma_query_value(None, "page_count", |r| r.get(0))?;
                tx.pragma_update(None, "max_page_count", pages)?;
                Ok(())
            })
            .unwrap();
        let id = store.new_operation_id().unwrap();
        let text = "x".repeat(2_000_000);
        let error = store
            .enqueue(
                account,
                Submission {
                    id: &id,
                    item,
                    kind: ActionKind::Comment,
                    payload: &text,
                    base: "base",
                    anchor: "",
                    projection: "pending",
                    draft: Some(DraftSubmission {
                        id: "draft",
                        revision: 1,
                        editor: "editor",
                        now: 1,
                    }),
                    dependencies: &[],
                },
            )
            .unwrap_err();
        assert!(
            matches!(error,Error::Sql(rusqlite::Error::SqliteFailure(e,_)) if e.code==rusqlite::ErrorCode::DiskFull)
        );
        let reopened = Store::open_profile(dir.path()).unwrap();
        assert_eq!(
            reopened.draft(account, "draft").unwrap().unwrap().content,
            draft
        );
        assert!(reopened.operation(account, &id).unwrap().is_none());
        assert!(reopened.projections(account, item).unwrap().is_empty());
    }
}
