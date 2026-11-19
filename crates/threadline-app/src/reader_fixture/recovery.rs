//! Read and interrupt one synthetic discussion transaction in the native recovery case.
use std::{
    ffi::{OsStr, OsString},
    fs::{self, OpenOptions},
    net::{Ipv4Addr, SocketAddrV4},
    path::PathBuf,
    time::Duration,
};

use rusqlite::{Connection, OpenFlags, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const ERROR: &str =
    "Discussion recovery requires the runner-owned loopback profile and original fixture data.";
const FAULT: &str = "native_reader_recovery_transaction_fault";

fn database(mutating: bool) -> Option<PathBuf> {
    if !cfg!(windows) || std::env::var_os("THREADLINE_E2E_READER_RECOVERY").as_deref()? != "1" {
        return None;
    }
    let address: SocketAddrV4 = std::env::var("THREADLINE_E2E_GITHUB_ADDR")
        .ok()?
        .parse()
        .ok()?;
    if *address.ip() != Ipv4Addr::LOCALHOST || address.port() == 0 {
        return None;
    }
    let profile = PathBuf::from(std::env::var_os("THREADLINE_PROFILE_DIR")?);
    if !profile.is_absolute() || !fs::symlink_metadata(&profile).ok()?.file_type().is_dir() {
        return None;
    }
    let profile = profile.canonicalize().ok()?;
    let runner = profile.parent()?;
    let nonce = profile.file_name()?.to_str()?.strip_prefix("initial-")?;
    if nonce.len() != 32
        || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !runner.file_name()?.to_str()?.starts_with("threadline-e2e-")
        || runner.parent()? != std::env::temp_dir().canonicalize().ok()?
    {
        return None;
    }
    let phase = runner.join("access-fixture-phase.txt");
    let metadata = fs::symlink_metadata(&phase).ok()?;
    if !metadata.file_type().is_file() || metadata.len() > 30 {
        return None;
    }
    let phase = fs::read(&phase).ok()?;
    if mutating && phase != b"recover-offline"
        || !matches!(
            phase.as_slice(),
            b"recover-baseline"
                | b"recover-offline"
                | b"recover-expired"
                | b"recover-rebuild"
                | b"recover-resume"
        )
    {
        return None;
    }
    let database = profile.join(threadline_storage::DATABASE_FILENAME);
    fs::symlink_metadata(&database)
        .ok()?
        .file_type()
        .is_file()
        .then_some(database)
}

fn digest_rows(
    db: &Connection,
    sql: &str,
    account: i64,
    item: i64,
) -> rusqlite::Result<Vec<String>> {
    db.prepare(sql)?
        .query_map(params![account, item], |row| {
            let fields = row.get::<_, String>(0)?;
            Ok(format!("{:x}", Sha256::digest(fields.as_bytes())))
        })?
        .collect()
}

fn receipt(db: &Connection, account: i64, item: i64, discussion: i64) -> rusqlite::Result<Value> {
    let states = db.query_row(
        "SELECT coverage,fetch,error,continuation,last_page_at_ms,last_fetch_at_ms,last_validation_at_ms
         FROM cached_resources WHERE id=?1",
        [discussion],
        |row| Ok(json!({
            "coverage":row.get::<_,i64>(0)?, "fetch":row.get::<_,i64>(1)?, "error":row.get::<_,Option<i64>>(2)?,
            "cursor":row.get::<_,Option<String>>(3)?, "last_page":row.get::<_,Option<i64>>(4)?,
            "last_success":row.get::<_,Option<i64>>(5)?, "last_validation":row.get::<_,Option<i64>>(6)?
        })),
    )?;
    let comments = db.prepare(
        "SELECT r.record_key,r.body,m.node_id,m.author,m.created_at,m.updated_at
         FROM cached_resource_records r LEFT JOIN reader_comment_metadata m USING(resource_id,record_key)
         WHERE r.resource_id=?1 ORDER BY r.record_key"
    )?.query_map([discussion], |row| {
        let id = row.get::<_, String>(0)?;
        let body = row.get::<_, String>(1)?;
        let edited = body == "The original comment changed while Threadline was closed.";
        let node = format!("IC_{}", id.trim_start_matches('0'));
        let created = 1_514_851_200_000_i64;
        let updated = if edited { 1_515_110_400_000 } else { created };
        let metadata_retained = row.get::<_,Option<String>>(2)?.as_deref() == Some(node.as_str())
            && row.get::<_,Option<String>>(3)?.as_deref() == Some("fixture-author")
            && row.get::<_,Option<i64>>(4)? == Some(created)
            && row.get::<_,Option<i64>>(5)? == Some(updated);
        Ok(json!({"id":id,"edited_original":edited,"metadata_retained":metadata_retained}))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    let other_resources = digest_rows(db,
        "SELECT json_array(r.family, r.coverage, r.last_page_at_ms, r.last_fetch_at_ms, r.last_validation_at_ms,
             r.etag, r.last_modified, r.continuation, x.record_key, x.body)
         FROM cached_resources r LEFT JOIN cached_resource_records x ON x.resource_id=r.id
         WHERE r.account_id=?1 AND r.item_id=?2 AND r.family IN ('body','timeline')
         ORDER BY r.family, x.record_key", account, item)?;
    let complete = db.query_row(
        "SELECT COUNT(*) FROM cached_resources WHERE account_id=?1 AND item_id=?2
         AND family IN ('body','timeline') AND coverage=2",
        params![account, item],
        |row| row.get::<_, i64>(0),
    )?;
    let drafts = digest_rows(
        db,
        "SELECT json_array(id,item_id,kind,body,base,anchor,revision) FROM drafts
         WHERE account_id=?1 AND item_id=?2 ORDER BY id",
        account,
        item,
    )?;
    let outbox = digest_rows(
        db,
        "SELECT json_array(id,original_repository,original_number,item_id,kind,payload_version,
             snapshot,projection,draft_id,draft_revision,state,attempt)
         FROM operations WHERE account_id=?1 AND item_id=?2 ORDER BY sequence",
        account,
        item,
    )?;
    Ok(json!({
        "item":item, "discussion":states, "comments":comments, "other_resource_hashes":other_resources,
        "completed_other_resources":complete, "draft_hashes":drafts, "outbox_hashes":outbox
    }))
}

pub(super) fn run(mut args: impl Iterator<Item = OsString>) -> Result<(), String> {
    let stage = args.next().ok_or(ERROR)?;
    let stage = stage.to_str().ok_or(ERROR)?;
    if !matches!(stage, "arm" | "snapshot" | "release")
        || args.next().as_deref() != Some(OsStr::new("--e2e-report"))
    {
        return Err(ERROR.into());
    }
    let report = args
        .next()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or(ERROR)?;
    if args.next().is_some() {
        return Err(ERROR.into());
    }
    let flags = if stage == "snapshot" {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    };
    let mut db = Connection::open_with_flags(database(stage != "snapshot").ok_or(ERROR)?, flags)
        .map_err(|_| ERROR)?;
    db.busy_timeout(Duration::from_secs(2)).map_err(|_| ERROR)?;
    let scope = db
        .query_row(
            "SELECT i.account_id,i.id,c.id,dp.offline FROM cached_resources c
         JOIN items i ON i.id=c.item_id AND i.account_id=c.account_id
         JOIN accounts a ON a.id=i.account_id
         JOIN repositories r ON r.id=i.repository_id AND r.account_id=i.account_id
         JOIN active_profile active ON active.account_id=i.account_id
         JOIN dispatch_profiles dp ON dp.account_id=i.account_id
         JOIN github_access_profiles p ON p.account_id=i.account_id AND p.owner_scope='fixture-org'
         WHERE a.host='github.com' AND a.provider_user_id='41' AND p.login='fixture-user'
         AND r.owner='fixture-org' AND r.name='private' AND r.provider_repository_id='71'
         AND i.number=41 AND i.issue_api_id='410' AND i.kind='issue' AND c.family='comments'",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            },
        )
        .map_err(|_| ERROR)?;
    let (account, item, resource, offline) = scope;
    let tx = db.transaction().map_err(|_| ERROR)?;
    let state = receipt(&tx, account, item, resource).map_err(|_| ERROR)?;
    if stage == "arm" {
        if !offline
            || state["discussion"]["cursor"] != "2"
            || state["discussion"]["coverage"] != 1
            || state["completed_other_resources"] != 2
            || state["draft_hashes"].as_array().is_none_or(Vec::is_empty)
            || state["outbox_hashes"].as_array().map(Vec::len) != Some(1)
            || state["comments"]
                != json!([{"id":"00000000000000005101","edited_original":false,"metadata_retained":true}])
        {
            return Err(ERROR.into());
        }
        tx.execute_batch(&format!(
            "CREATE TRIGGER {FAULT} BEFORE INSERT ON cached_resource_pending
             WHEN NEW.resource_id={resource} AND NEW.record_key='00000000000000005103'
             BEGIN SELECT RAISE(ABORT,'native fixture interrupted discussion publication'); END;"
        ))
        .map_err(|_| ERROR)?;
    } else if stage == "release" {
        if !offline
            || state["discussion"]["coverage"] != 1
            || state["discussion"]["cursor"] != "2"
            || state["discussion"]["error"] != 1
            || state["comments"]
                != json!([
                    {"id":"00000000000000005101","edited_original":true,"metadata_retained":true},
                    {"id":"00000000000000005102","edited_original":false,"metadata_retained":true}
                ])
        {
            return Err(ERROR.into());
        }
        tx.execute_batch(&format!("DROP TRIGGER {FAULT};"))
            .map_err(|_| ERROR)?;
    }
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report)
        .map_err(|_| ERROR)?;
    tx.commit().map_err(|_| ERROR)?;
    serde_json::to_writer(output, &state).map_err(|_| ERROR.into())
}
