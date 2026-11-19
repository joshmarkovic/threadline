use reqwest::Url;
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, IsTerminal, Read, Write},
    mem::size_of,
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Path, PathBuf},
};
use threadline_platform::{
    CredentialRef, CredentialStore, NativeCredentialStore, NativePaths, PathResolver,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_INFO, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_GENERIC_READ, FILE_SHARE_READ, FileDispositionInfo, SetFileInformationByHandle,
};

type Result<T> = std::result::Result<T, &'static str>;
const DATABASE: &str = "threadline.sqlite3";
const APPLICATION_ID: u32 = 0x5448_5244;
// Review credential references and backup discovery before raising this ceiling.
const LAST_REVIEWED_SCHEMA: u32 = 18;
const BUSY: &str =
    "Close Threadline and other programs using its database, then preview removal again.";
const UNRECOGNIZED: &str =
    "A managed workspace file has an unrecognized format. Its data and credentials were preserved.";
const CHANGED: &str = "The workspace changed since the preview. Review the current contents before confirming removal.";
const CONFIRMATION: &str = "REMOVE LOCAL WORKSPACE";

#[derive(Default)]
struct Preview {
    files: Vec<(PathBuf, [u8; 32])>,
    credentials: BTreeSet<(u64, String, String)>,
    accounts: i64,
    drafts: i64,
    unresolved: i64,
    backup_drafts: i64,
    backup_unresolved: i64,
}

fn backup_version(name: &str) -> Option<u32> {
    let name = name.to_ascii_lowercase();
    let (version, random) = name
        .strip_prefix("threadline-schema-")?
        .strip_suffix(".sqlite3")?
        .split_once('-')?;
    let version: u32 = version.parse().ok()?;
    (version > 0 && random.len() == 6 && random.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then_some(version)
}

fn managed_files(data: &Path) -> Result<Vec<PathBuf>> {
    for path in data.ancestors() {
        match fs::symlink_metadata(path) {
            Ok(m) if m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !m.is_dir() => {
                return Err(
                    "Local removal refused a redirected or unrecognized workspace directory.",
                );
            }
            Ok(_) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(_) => return Err(BUSY),
        }
    }
    let entries = match fs::read_dir(data) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(BUSY),
    };
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| BUSY)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let name = name.to_ascii_lowercase();
        if let Some(db) = ["-wal", "-shm", "-journal"]
            .into_iter()
            .find_map(|suffix| name.strip_suffix(suffix))
            && (db == DATABASE || backup_version(db).is_some())
        {
            return Err(
                "SQLite recovery files are present. Open Threadline, quit normally, then preview removal again.",
            );
        }
        if name == DATABASE || backup_version(&name).is_some() {
            files.push(entry.path());
        }
    }
    files.sort_by_key(|path| {
        let is_current = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(DATABASE));
        (is_current, path.clone())
    });
    Ok(files)
}

// During preview deny writes and renames. During removal acquire DELETE on the
// same verified bytes and keep every handle until credential revocation succeeds.
fn guard(path: &Path, deleting: bool) -> Result<File> {
    let access = FILE_GENERIC_READ | if deleting { DELETE } else { 0 };
    let file = OpenOptions::new()
        .access_mode(access)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| BUSY)?;
    let info = file.metadata().map_err(|_| BUSY)?;
    if !info.is_file() || info.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(UNRECOGNIZED);
    }
    Ok(file)
}

fn hash(file: &mut File) -> Result<[u8; 32]> {
    let mut digest = Sha256::new();
    let mut buf = [0; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|_| BUSY)?;
        if n == 0 {
            return Ok(digest.finalize().into());
        }
        digest.update(&buf[..n]);
    }
}

impl Preview {
    fn inspect(data: &Path) -> Result<Self> {
        let mut preview = Self::default();
        for path in managed_files(data)? {
            let mut file = guard(&path, false)?;
            let mut url = Url::from_file_path(&path).map_err(|_| UNRECOGNIZED)?;
            url.query_pairs_mut().append_pair("immutable", "1");
            let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
            let db = Connection::open_with_flags(url.as_str(), flags).map_err(|_| UNRECOGNIZED)?;
            db.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON;")
                .map_err(|_| UNRECOGNIZED)?;
            let (app, version): (u32, u32) = db
                .query_row(
                    "SELECT application_id, user_version
                       FROM pragma_application_id(), pragma_user_version()",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|_| UNRECOGNIZED)?;
            let backup = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(backup_version);
            if app != APPLICATION_ID
                || !(1..=LAST_REVIEWED_SCHEMA).contains(&version)
                || backup.is_some_and(|v| v != version)
            {
                return Err(UNRECOGNIZED);
            }
            let (accounts, drafts, unresolved): (i64, i64, i64) = db
                .query_row(
                    "SELECT (SELECT count(*) FROM accounts),
                            (SELECT count(*) FROM drafts),
                            (SELECT count(*) FROM operations WHERE state NOT IN ('confirmed','cancelled'))",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map_err(|_| UNRECOGNIZED)?;
            if backup.is_some() {
                preview.backup_drafts += drafts;
                preview.backup_unresolved += unresolved;
            } else {
                preview.accounts = accounts;
                preview.drafts = drafts;
                preview.unresolved = unresolved;
            }
            if version >= 3 {
                let mut query = db
                    .prepare(
                        "SELECT a.host, a.provider_user_id, p.owner_scope, p.credential_ref
                           FROM github_access_profiles p JOIN accounts a ON a.id=p.account_id",
                    )
                    .map_err(|_| UNRECOGNIZED)?;
                let rows = query
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                        ))
                    })
                    .map_err(|_| UNRECOGNIZED)?;
                for row in rows {
                    let (host, id, owner, key) = row.map_err(|_| UNRECOGNIZED)?;
                    let id = id.parse().map_err(|_| UNRECOGNIZED)?;
                    if !host.eq_ignore_ascii_case("github.com")
                        || key.len() != 32
                        || !key
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    {
                        return Err(UNRECOGNIZED);
                    }
                    CredentialRef::github_profile(id, &owner, &key).map_err(|_| UNRECOGNIZED)?;
                    preview
                        .credentials
                        .insert((id, owner.to_ascii_lowercase(), key));
                }
            }
            drop(db);
            preview.files.push((path, hash(&mut file)?));
        }
        Ok(preview)
    }

    fn confirm_removal(
        &self,
        data: &Path,
        answer: &str,
        vault: &impl CredentialStore,
        release_credential: &CredentialRef,
    ) -> Result<()> {
        if answer.trim_end_matches(['\r', '\n']) != CONFIRMATION {
            return Err("Removal cancelled. The previewed files and credentials were preserved.");
        }
        let paths = managed_files(data)?;
        if !paths.iter().eq(self.files.iter().map(|(p, _)| p)) {
            return Err(CHANGED);
        }
        let mut locked = Vec::new();
        for (path, expected) in &self.files {
            let mut file = guard(path, true)?;
            if hash(&mut file)? != *expected {
                return Err(CHANGED);
            }
            locked.push(file);
        }
        if managed_files(data)? != paths {
            return Err(CHANGED);
        }
        for (id, owner, key) in &self.credentials {
            let reference =
                CredentialRef::github_profile(*id, owner, key).map_err(|_| UNRECOGNIZED)?;
            vault.remove(&reference).map_err(|_| {
                "Windows could not remove a recorded credential. The databases were preserved. Retry after checking Credential Manager."
            })?;
        }
        vault.remove(release_credential).map_err(
            |_| "Windows could not remove the release credential. The databases were preserved.",
        )?;
        for file in locked {
            let info = FILE_DISPOSITION_INFO { DeleteFile: true };
            // The owned handle identifies the verified file and denies competing writes or renames.
            if unsafe {
                SetFileInformationByHandle(
                    file.as_raw_handle(),
                    FileDispositionInfo,
                    (&raw const info).cast(),
                    size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            } == 0
            {
                return Err(
                    "Windows could not finish removing the verified files. Preview the remaining local data and retry.",
                );
            }
        }
        Ok(())
    }
}

pub(super) fn command(remove: bool) -> Result<()> {
    let data = NativePaths
        .application_paths()
        .map_err(|_| "Windows could not locate the local Threadline data directory.")?
        .data;
    let input = io::stdin();
    run(
        &data,
        remove,
        input.is_terminal(),
        &mut input.lock(),
        &mut io::stdout().lock(),
        &NativeCredentialStore,
        &CredentialRef::github_releases(),
    )
}

fn run(
    data: &Path,
    remove: bool,
    interactive: bool,
    input: &mut impl BufRead,
    output: &mut impl Write,
    vault: &impl CredentialStore,
    release_credential: &CredentialRef,
) -> Result<()> {
    const DISPLAY_ERROR: &str = "Could not display the local workspace removal preview.";
    let plan = Preview::inspect(data)?;
    writeln!(output,
        "This Windows user's Threadline workspace contains {} accounts, {} drafts and {} unresolved Outbox operations.",
        plan.accounts, plan.drafts, plan.unresolved
    ).map_err(|_| DISPLAY_ERROR)?;
    writeln!(output,
        "Removal includes {} managed databases and migration backups plus {} recorded account credential references and the private release credential if saved. Backups contain {} draft rows and {} unresolved operation rows.",
        plan.files.len(),
        plan.credentials.len(),
        plan.backup_drafts,
        plan.backup_unresolved
    ).map_err(|_| DISPLAY_ERROR)?;
    writeln!(output,
        "Removal permanently discards these accounts' local sign-in credentials, cached content, saved layouts, drafts and Outbox history. Queued work can no longer be sent or reconciled here. Work already submitted may still take effect on GitHub."
    ).map_err(|_| DISPLAY_ERROR)?;
    if !remove {
        return Ok(());
    }
    if !interactive {
        return Err(
            "Run the removal command in an interactive terminal to review and confirm its effect.",
        );
    }
    write!(output, "Type {CONFIRMATION} to confirm: ").map_err(|_| DISPLAY_ERROR)?;
    output.flush().map_err(|_| DISPLAY_ERROR)?;
    let mut answer = String::new();
    input
        .read_line(&mut answer)
        .map_err(|_| "Could not read the removal confirmation.")?;
    plan.confirm_removal(data, &answer, vault, release_credential)?;
    writeln!(
        output,
        "Removed the previewed Threadline databases and protected credential references."
    )
    .map_err(|_| "Removal completed, but the final confirmation could not be displayed.")?;
    Ok(())
}

#[cfg(test)]
mod tests;
