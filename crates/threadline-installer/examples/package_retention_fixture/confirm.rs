use super::{
    R, console,
    fixture::{self, DB as DATA, MARKER, SENTINEL as GENERATED, USER, ordinary},
    resume,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap, env, ffi::OsStr, fs, io::Read, os::windows::ffi::OsStrExt, path::Path,
    process::Command,
};
use threadline_platform::{
    Credential, CredentialRef, CredentialStore, NativeCredentialStore, NativePaths, PathResolver,
};
use windows_sys::Win32::Storage::FileSystem::MoveFileW;
const EXPORT: &str = "my-ordinary-export.txt";
const INDEPENDENT: &[u8] = b"Unrelated owned package removal sentinel";
const PREVIEW: &str = "This Windows user's Threadline workspace contains 1 accounts, 1 drafts and 9 unresolved Outbox operations.";
const FILES: &str = "Removal includes 2 managed databases and migration backups plus 1 recorded credential references. Backups contain 1 draft rows and 9 unresolved operation rows.";
const IMPACT: &str = "Queued work can no longer be sent or reconciled here. Work already submitted may still take effect on GitHub.";
const PROMPT: &str = "Type REMOVE LOCAL WORKSPACE to confirm:";
const REMOVED: &str =
    "Removed the previewed Threadline databases and protected credential references.";
fn require(condition: bool, error: &'static str) -> R<()> {
    if condition { Ok(()) } else { Err(error.into()) }
}
fn hash(path: &Path) -> R<String> {
    ordinary(path)?;
    let mut sha = Sha256::new();
    let mut file = fs::File::open(path)?;
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        sha.update(&buffer[..n]);
    }
    Ok(sha.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
fn check_files(dir: &Path, files: &BTreeMap<String, String>) -> R<()> {
    ordinary(dir)?;
    let entries = fs::read_dir(dir)?.collect::<std::io::Result<Vec<_>>>()?;
    require(
        entries.len() == files.len(),
        "unexpected contents preserved in fixture or archive",
    )?;
    for (key, expected) in files {
        require(
            hash(&dir.join(key))? == *expected,
            "fixture or archive bytes differ",
        )?;
    }
    Ok(())
}
fn rename_new(source: &Path, dest: &Path) -> R<()> {
    ordinary(source)?;
    ordinary(dest.parent().ok_or("missing destination parent")?)?;
    let wide = |p: &OsStr| p.encode_wide().chain([0]).collect::<Vec<_>>();
    if unsafe {
        MoveFileW(
            wide(source.as_os_str()).as_ptr(),
            wide(dest.as_os_str()).as_ptr(),
        )
    } == 0
    {
        return Err("could not move the owned fixture without replacing existing data".into());
    }
    Ok(())
}
fn native_key(owner: &str, nonce: &str) -> R<CredentialRef> {
    Ok(CredentialRef::github_profile(USER, owner, nonce)?)
}
fn key_matches(reference: &CredentialRef, expected: &[u8]) -> R<bool> {
    Ok(NativeCredentialStore
        .read(reference)?
        .is_some_and(|v| v.0 == expected))
}
struct TestKeys {
    registered: Vec<(CredentialRef, &'static [u8])>,
}
impl Drop for TestKeys {
    fn drop(&mut self) {
        for (r, marker) in &self.registered {
            if key_matches(r, marker).unwrap_or(false) {
                let _ = NativeCredentialStore.remove(r);
            }
        }
    }
}

struct StagedDefault<'a> {
    data: &'a Path,
    root: &'a Path,
    expected: &'a BTreeMap<String, String>,
}
impl Drop for StagedDefault<'_> {
    fn drop(&mut self) {
        let preserve = || -> R<()> {
            if !self.data.try_exists()? {
                return Ok(());
            }
            ordinary(self.data)?;
            let mut actual = BTreeMap::new();
            for entry in fs::read_dir(self.data)? {
                let entry = entry?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "unknown fixture filename")?;
                let wanted = self
                    .expected
                    .get(&name)
                    .ok_or("unrecognized contents preserved in place")?;
                require(
                    &hash(&entry.path())? == wanted,
                    "changed fixture preserved in place",
                )?;
                actual.insert(name, ());
            }
            require(
                actual.contains_key(MARKER) && actual.contains_key(EXPORT),
                "lost ownership evidence",
            )?;
            rename_new(self.data, &self.root.join("preserved-after-stopped-check"))
        };
        if preserve().is_err() {
            eprintln!("Changed or unrecognized default contents were preserved for inspection.");
        }
    }
}

pub(super) fn run(
    exe: &Path,
    expected: &str,
    archive: &Path,
    archive_nonce: &str,
    archive_hash: &str,
) -> R<Value> {
    require(
        hash(exe)? == expected
            && exe
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("threadline-installer.exe")),
        "supply the exact independently pinned Windows bootstrap",
    )?;
    resume::read_archive(archive, archive_nonce, archive_hash)?;
    let data = NativePaths.application_paths()?.data;
    let local = data
        .parent()
        .and_then(Path::parent)
        .ok_or("missing local application data base")?;
    let local_install = local.join("Programs").join("Threadline Installer Test");
    require(
        !local_install.try_exists()?,
        "finish uninstalling the development candidate first",
    )?;
    ordinary(
        data.parent()
            .ok_or("missing local application data parent")?,
    )?;
    require(
        !data.try_exists()?,
        "default profile already exists and was preserved",
    )?;
    let temp = env::temp_dir();
    ordinary(&temp)?;
    let nonce: String =
        Connection::open_in_memory()?
            .query_row("SELECT lower(hex(randomblob(16)))", [], |r| r.get(0))?;
    let root = temp.join(format!("threadline-package-retention-{nonce}"));
    fs::create_dir(&root)?;
    let primary = native_key("fixture", &nonce)?;
    let other = native_key("other-owned-conpty-fixture", &nonce)?;
    require(
        NativeCredentialStore.read(&primary)?.is_none()
            && NativeCredentialStore.read(&other)?.is_none(),
        "generated credential target already exists",
    )?;
    let keys = TestKeys {
        registered: vec![
            (native_key("fixture", &nonce)?, GENERATED),
            (
                native_key("other-owned-conpty-fixture", &nonce)?,
                INDEPENDENT,
            ),
        ],
    };
    let snapshot = fixture::run("seed", &nonce)?;
    require(
        snapshot["status"] == "Pass"
            && snapshot["queued_operations"] == 8
            && snapshot["drafts"] == 1
            && snapshot["step_receipts"] == 1
            && snapshot["protected_credential_present"] == true
            && snapshot["outbox_paused"] == true
            && snapshot["work_offline"] == true,
        "unexpected newly generated fixture inventory",
    )?;
    NativeCredentialStore.save(&other, &Credential(INDEPENDENT.to_vec()))?;
    let seed = root.join("profile");
    let staged = root.join("prior-to-confirmation");
    fs::create_dir(&staged)?;
    for file in [DATA, MARKER] {
        fs::copy(seed.join(file), staged.join(file))?;
    }
    let schema = snapshot["schema"]
        .as_i64()
        .filter(|value| *value > 0)
        .ok_or("the generated fixture has no schema version")?;
    let backup = format!("threadline-schema-{schema}-Fix123.sqlite3");
    fs::copy(staged.join(DATA), staged.join(&backup))?;
    fs::write(
        staged.join(EXPORT),
        format!("owned independent export {nonce}"),
    )?;
    let contents: BTreeMap<_, _> = [DATA, MARKER, backup.as_str(), EXPORT]
        .into_iter()
        .map(|s| Ok((s.to_owned(), hash(&staged.join(s))?)))
        .collect::<R<_>>()?;
    let marker = format!("package-retention-v1/{nonce}");
    require(
        fs::read(staged.join(MARKER))? == marker.as_bytes(),
        "generated fixture marker failed",
    )?;
    check_files(&staged, &contents)?;
    // MoveFileW never replaces an existing destination.
    rename_new(&staged, &data)?;
    let _staged = StagedDefault {
        data: &data,
        root: &root,
        expected: &contents,
    };
    let known = || -> R<()> {
        check_files(&data, &contents)?;
        require(
            fs::read(data.join(MARKER))? == marker.as_bytes(),
            "default profile ownership changed",
        )?;
        require(
            key_matches(&primary, GENERATED)? && key_matches(&other, INDEPENDENT)?,
            "fixture credential binding changed",
        )?;
        require(hash(exe)? == expected, "bootstrap bytes changed")?;
        resume::read_archive(archive, archive_nonce, archive_hash).map(drop)
    };
    known()?;
    let pre = Command::new(exe).arg("--preview-local-removal").output()?;
    let pre_text = std::str::from_utf8(&pre.stdout)?;
    require(
        pre.status.success()
            && [PREVIEW, FILES, IMPACT]
                .iter()
                .all(|line| pre_text.contains(line)),
        "preview differs from the generated profile; no confirmation was sent",
    )?;
    console::invoke(
        exe,
        "--remove-local-data-and-credentials",
        &[PREVIEW, FILES, IMPACT, PROMPT],
        "REMOVE LOCAL WORKSPACE",
        REMOVED,
        known,
    )?;
    let remaining: BTreeMap<_, _> = [MARKER, EXPORT]
        .into_iter()
        .map(|name| (name.to_owned(), contents[name].clone()))
        .collect();
    check_files(&data, &remaining)?;
    require(
        NativeCredentialStore.read(&primary)?.is_none() && key_matches(&other, INDEPENDENT)?,
        "credential scope verification failed",
    )?;
    let remainder = root.join("retained-independent-files");
    rename_new(&data, &remainder)?;
    check_files(&remainder, &remaining)?;
    resume::read_archive(archive, archive_nonce, archive_hash)?;
    drop(keys);
    require(
        NativeCredentialStore.read(&other)?.is_none() && !data.try_exists()?,
        "fixture teardown incomplete",
    )?;
    Ok(
        json!({"status":"Pass","bootstrap_sha256":expected,"normal_preview_exit":0,"normal_confirmation_exit":0,
        "conpty_prompt_verified":true,"accounts":1,"drafts":1,"unresolved_operations":9,"databases_and_backups":2,
        "recorded_credential_references":1,"backup_drafts":1,"backup_unresolved":9,"actual_confirmation_sent_once":true,
        "current_db_and_backup_removed":true,"bound_protected_credential_removed":true,"independent_export_unchanged":true,
        "independent_credential_preserved_then_fixture_cleaned":true,"prior_retention_archive_unchanged":true,
        "default_profile_absent_before_and_after":true}),
    )
}
