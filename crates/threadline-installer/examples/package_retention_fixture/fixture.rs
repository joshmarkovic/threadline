use super::R;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, num::NonZeroU64, os::windows::fs::MetadataExt, path::Path};
use threadline_platform::{
    Credential, CredentialRef, CredentialStore, NativeCredentialStore, NativePaths, PathResolver,
};
use threadline_storage::{
    Store,
    access::{
        AccessState, CredentialKind, DiscoveryState, RepositoryObservation,
        RepositoryObservationSource,
    },
    fixtures::{Scale, populate_reader},
    reader::DiscussionComment,
    resources::{ResourceFamily, Validators},
    sync::{ObservationPage, ObservedComment, ReadPriority, ReadTarget},
};

pub(super) const USER: u64 = 12345;
const OWNER: &str = "fixture";
pub(super) const WHEN: i64 = 1_700_000_500_000;
pub(super) const SENTINEL: &[u8] = b"Threadline offline local package retention fixture";
pub(super) const MARKER: &str = "threadline-package-retention.owner";
pub(super) const DB: &str = "threadline.sqlite3";

pub(super) fn ordinary(path: &Path) -> R<()> {
    if !path.is_absolute() {
        return Err("use an absolute fixture path".into());
    }
    for part in path.ancestors() {
        if fs::symlink_metadata(part)?.file_attributes() & 0x400 != 0 {
            return Err("refused redirected fixture path".into());
        }
    }
    Ok(())
}

fn owned(path: &Path, nonce: &str) -> R<()> {
    ordinary(path)?;
    ordinary(&path.join(MARKER))?;
    if fs::read(path.join(MARKER))? != format!("package-retention-v1/{nonce}").as_bytes() {
        return Err("refused unowned fixture path".into());
    }
    ordinary(&path.join(DB))
}

fn settings(s: &Store, account: threadline_storage::AccountId, offline: bool) -> R<()> {
    s.update_sync_settings(account, |v| {
        v.controls.work_offline = offline;
        v.controls.pause_queued_actions = true;
        v.controls.pause_background = true;
    })?;
    Ok(())
}

fn seed(path: &Path, nonce: &str, key: &CredentialRef) -> R<()> {
    fs::write(path.join(MARKER), format!("package-retention-v1/{nonce}"))?;
    let store = Store::open_profile(path)?;
    let fixture = populate_reader(&store, USER, Scale::Small)?;
    let account = fixture.account;
    drop(store);
    let raw = Connection::open(path.join(DB))?;
    if raw.execute(
        "UPDATE github_access_profiles SET credential_ref=?1",
        [nonce],
    )? != 1
    {
        return Err("fixture profile count changed".into());
    }
    drop(raw);
    let store = Store::open_profile(path)?;
    let old = store
        .github_access_profile(account, OWNER)?
        .ok_or("missing fixture account")?;
    let prof = store.reserve_access_profile_credential(
        account,
        old.user_id,
        &old.login,
        OWNER,
        CredentialKind::Classic,
        Some(&old),
    )?;
    if prof.user_id.get() != USER || prof.credential_ref != nonce {
        return Err("unbound fixture credential".into());
    }
    store.install_current_credential(&prof, || {
        NativeCredentialStore.save(key, &Credential(SENTINEL.to_vec()))
    })??;
    store.observe_github_repository(
        &prof,
        &RepositoryObservation {
            provider_id: NonZeroU64::new(1).unwrap(),
            owner: OWNER,
            name: "relay",
            private: true,
            source: RepositoryObservationSource::Direct,
        },
        WHEN,
    )?;
    if !store.record_access_result(
        &prof,
        prof.kind,
        AccessState::Readable,
        DiscoveryState::Complete,
        WHEN,
    )? {
        return Err("fixture profile no longer current".into());
    }
    settings(&store, account, false)?;
    let target = ReadTarget {
        repository: fixture.repository,
        item: None,
        family: ResourceFamily::Comments,
        revision: String::new(),
    };
    store.subscribe_read(
        account,
        &target,
        "package-fixture-checkpoint",
        ReadPriority::Explicit,
        WHEN,
    )?;
    let claim = store
        .claim_sync_read(account, WHEN)?
        .ok_or("fixture scheduler did not claim the comment feed")?;
    if claim.target != target {
        return Err("wrong fixture claim".into());
    }
    store.commit_observed_page(
        &claim,
        ObservationPage {
            comments: vec![ObservedComment {
                number: 1,
                actor: Default::default(),
                comment: DiscussionComment {
                    provider_id: 800_012,
                    node_id: None,
                    author: Some("fixture".into()),
                    body: "package fixture".into(),
                    created_at_ms: WHEN - 50,
                    updated_at_ms: WHEN - 25,
                },
            }],
            items: Vec::new(),
            next_cursor: Some("2"),
            validators: Validators {
                etag: Some("package-fixture-page-1".into()),
                ..Default::default()
            },
        },
        WHEN + 1,
        "UTC",
    )?;
    settings(&store, account, true)?;
    Ok(())
}

fn inventory(path: &Path, nonce: &str, key: &CredentialRef) -> R<Value> {
    owned(path, nonce)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        if path.join(format!("{DB}{suffix}")).exists() {
            return Err("wait for normal application exit".into());
        }
    }
    let mut url =
        reqwest::Url::from_file_path(path.join(DB)).map_err(|_| "invalid fixture path")?;
    url.query_pairs_mut().append_pair("immutable", "1");
    let db = Connection::open_with_flags(
        url.as_str(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )?;
    db.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON;")?;
    let bound:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM github_access_profiles g JOIN accounts a ON a.id=g.account_id WHERE a.host='github.com' AND a.provider_user_id='12345' AND g.owner_scope='fixture' AND g.credential_ref=?1 AND g.installed_generation=g.credential_generation)",[nonce],|r|r.get(0))?;
    let (cursor, watermark):(String,i64)=db.query_row("SELECT cursor,page_watermark FROM scheduled_reads WHERE family='comments' AND walk_etag='package-fixture-page-1'",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let (queued,receipts,drafts,offline,paused):(i64,i64,i64,bool,bool)=db.query_row(
        "SELECT (SELECT count(*) FROM operations WHERE state='queued'),(SELECT count(*) FROM step_receipts),(SELECT count(*) FROM drafts),offline,outbox_paused FROM dispatch_profiles WHERE account_id=(SELECT account_id FROM active_profile)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    let visible = NativeCredentialStore
        .read(key)?
        .is_some_and(|v| v.0 == SENTINEL);
    if !bound
        || !visible
        || cursor != "2"
        || watermark != WHEN - 25
        || queued < 1
        || receipts < 1
        || drafts < 1
        || !offline
        || !paused
    {
        return Err(
            "retained cursor, approval state, receipt, or exact fixture credential failed".into(),
        );
    }
    let mut digest = Sha256::new();
    for sql in [
        "SELECT json_array(account_id,id,original_repository,item_id,original_number,kind,snapshot,state) FROM operations ORDER BY sequence",
        "SELECT json_array(account_id,operation_id,step,object_kind,remote_id,evidence) FROM step_receipts ORDER BY account_id,operation_id,step",
        "SELECT json_array(account_id,repository_id,family,revision,cursor,page_watermark,walk_etag,watermark,walk_started) FROM scheduled_reads WHERE walk_etag='package-fixture-page-1' ORDER BY id",
    ] {
        let mut q = db.prepare(sql)?;
        for row in q.query_map([], |row| row.get::<_, String>(0))? {
            digest.update(row?.as_bytes());
            digest.update(b"\n");
        }
    }
    let schema: i32 = db.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    Ok(
        json!({"status":"Pass","schema":schema,"profile_bound":bound,"protected_credential_present":visible,"work_offline":offline,
        "outbox_paused":paused,"saved_comment_page_cursor":cursor,"page_watermark_ms":watermark,
        "queued_operations":queued,"step_receipts":receipts,"drafts":drafts,"durable_projection_sha256":digest.finalize().iter().map(|b|format!("{b:02x}")).collect::<String>()}),
    )
}

pub(super) fn run(action: &str, nonce: &str) -> R<Value> {
    if nonce.len() != 32
        || !nonce
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("invalid fixture nonce".into());
    }
    let key = CredentialRef::github_profile(USER, OWNER, nonce)?;
    let root = env::temp_dir().join(format!("threadline-package-retention-{nonce}"));
    ordinary(&root)?;
    let stored = root.join("profile");
    match action {
        "seed" => {
            if NativeCredentialStore.read(&key)?.is_some() {
                return Err("the generated seed already exists".into());
            }
            fs::create_dir(&stored)?;
            let result = seed(&stored, nonce, &key).and_then(|()| inventory(&stored, nonce, &key));
            if result.is_err()
                && owned(&stored, nonce).is_ok()
                && NativeCredentialStore
                    .read(&key)?
                    .is_some_and(|value| value.0 == SENTINEL)
            {
                NativeCredentialStore.remove(&key)?;
            }
            result
        }
        "inspect-seed" => inventory(&stored, nonce, &key),
        "inspect-backup-copy" => inventory(&root.join("migration-backup-check"), nonce, &key),
        "inspect-default" => inventory(&NativePaths.application_paths()?.data, nonce, &key),
        "cleanup-key" => {
            owned(&stored, nonce)?;
            if let Some(value) = NativeCredentialStore.read(&key)? {
                if value.0 != SENTINEL {
                    return Err("preserved unexpected protected value".into());
                }
                NativeCredentialStore.remove(&key)?;
            }
            if NativeCredentialStore.read(&key)?.is_some() {
                return Err("fixture key remains".into());
            }
            Ok(json!({"status":"Pass","protected_fixture_key_absent":true}))
        }
        _ => Err("unknown retention fixture stage".into()),
    }
}
