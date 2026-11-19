use super::{
    R,
    fixture::{DB, MARKER, WHEN as T, ordinary},
};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use threadline_storage::{
    Store,
    reader::DiscussionComment,
    resources::{ResourceFamily, Validators},
    sync::{ObservationPage, ObservedComment},
};

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn state(db: &Connection) -> R<(i64, i64, String)> {
    let row = db.query_row(
        "SELECT (SELECT count(*) FROM operations WHERE state='queued'),
                (SELECT count(*) FROM step_receipts),
                (SELECT json_group_array(json_array(id,state,snapshot) ORDER BY sequence) FROM operations)
                || (SELECT json_group_array(json_array(operation_id,step,remote_id,evidence) ORDER BY operation_id,step) FROM step_receipts)",
        [], |r| Ok((r.get(0)?,r.get(1)?,r.get::<_,String>(2)?)))?;
    Ok((row.0, row.1, digest(row.2.as_bytes())))
}

pub(super) fn read_archive(archive: &Path, nonce: &str, expected: &str) -> R<Vec<u8>> {
    if nonce.len() != 32
        || !nonce.bytes().all(|b| b.is_ascii_hexdigit())
        || expected.len() != 64
        || !archive.is_absolute()
        || archive.file_name().and_then(|n| n.to_str()) != Some("uninstalled-owned-data")
    {
        return Err("expected the marked local retention archive, run nonce and exact closed database SHA-256".into());
    }
    let marker = archive.join(MARKER);
    let original = archive.join(DB);
    ordinary(&marker)?;
    ordinary(&original)?;
    if fs::read(&marker)? != format!("package-retention-v1/{nonce}").as_bytes()
        || fs::read_dir(archive)?
            .collect::<std::io::Result<Vec<_>>>()?
            .len()
            != 2
    {
        return Err("unexpected contents in test archive".into());
    }
    if fs::metadata(&original)?.len() > 32 * 1024 * 1024 {
        return Err("the database is too large for the generated small fixture".into());
    }
    let bytes = fs::read(&original)?;
    if digest(&bytes) != expected {
        return Err("archive digest differs from the retention receipt".into());
    }
    Ok(bytes)
}

pub(super) fn run(archive: &Path, nonce: &str, expected: &str) -> R<Value> {
    let bytes = read_archive(archive, nonce, expected)?;
    let original = archive.join(DB);
    let shadow = tempfile::tempdir()?;
    let copy = shadow.path().join(DB);
    fs::write(&copy, bytes)?;
    let raw = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let before = state(&raw)?;
    let bound:bool = raw.query_row(
        "SELECT EXISTS(SELECT 1 FROM active_profile a JOIN github_access_profiles p ON a.account_id=p.account_id
         JOIN dispatch_profiles d ON a.account_id=d.account_id WHERE p.credential_ref=? AND p.owner_scope='fixture'
         AND d.offline=1 AND d.outbox_paused=1)",[nonce],|r|r.get(0))?;
    drop(raw);
    if !bound || before.0 != 8 || before.1 != 1 {
        return Err("wrong paused fixture account or operations".into());
    }
    let db = Store::open(&copy)?;
    let account = db
        .active_github_account()?
        .ok_or("missing active account")?;
    db.update_sync_settings(account, |s| s.controls.work_offline = false)?;
    let claim = db
        .claim_sync_read(account, T + 2000)?
        .ok_or("stored claim did not resume")?;
    if claim.target.family != ResourceFamily::Comments || claim.cursor.as_deref() != Some("2") {
        return Err("scheduler did not resume the recorded second page".into());
    }
    db.commit_observed_page(
        &claim,
        ObservationPage {
            comments: vec![ObservedComment {
                number: 1,
                actor: Default::default(),
                comment: DiscussionComment {
                    provider_id: 800_013,
                    node_id: None,
                    author: Some("fixture".into()),
                    body: "fixture page two".into(),
                    created_at_ms: T + 5,
                    updated_at_ms: T + 10,
                },
            }],
            items: vec![],
            next_cursor: None,
            validators: Validators::default(),
        },
        T + 2001,
        "UTC",
    )?;
    drop(db);
    let raw = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let (watermark,clear):(i64,bool)=raw.query_row(
        "SELECT watermark,cursor IS NULL FROM scheduled_reads WHERE family='comments' AND etag='package-fixture-page-1'",
        [],|r|Ok((r.get(0)?,r.get(1)?)))?;
    if before != state(&raw)?
        || !clear
        || watermark != T + 10
        || digest(&fs::read(&original)?) != expected
    {
        return Err("copied resume did not preserve the archived file and pending writes".into());
    }
    Ok(
        json!({"status":"Pass","source_database_sha256":expected,"scheduler_claimed_page":2,"completed_page_watermark_ms":watermark,
        "next_cursor_empty":clear,"archived_bytes_unchanged":true,"network_requests":0,"queued_operations":before.0,"step_receipts":before.1,
        "queued_payloads_and_step_receipts_unchanged":true,"test_clock_ms":T+2001}),
    )
}
