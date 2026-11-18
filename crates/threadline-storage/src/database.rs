use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use rusqlite::{
    Connection, OpenFlags, Transaction, TransactionBehavior,
    backup::{Backup, StepResult},
};
use tempfile::{Builder, NamedTempFile};

use crate::{Error, Result, queue::hold_restored_dispatch};

pub const DATABASE_FILENAME: &str = "threadline.sqlite3";
pub const SCHEMA_VERSION: u32 = 2;
const APPLICATION_ID: u32 = 0x5448_5244;
const BUSY_TIMEOUT: Duration = Duration::from_secs(3);
const BACKUP_TIMEOUT: Duration = Duration::from_secs(30);

/// Signed Unix-epoch milliseconds in UTC. Use a monotonic clock for in-process delays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UtcMillis(pub i64);

impl UtcMillis {
    pub fn now() -> Result<Self> {
        let millis = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_millis() as i128,
            Err(before_epoch) => -(before_epoch.duration().as_millis() as i128),
        };
        Ok(Self(millis.try_into().map_err(|_| {
            Error::InvalidInput("UTC timestamp is out of range")
        })?))
    }
}

/// At most one read-only connection and one writer for an open profile. Keep closures
/// short. A reader can see its WAL snapshot while a writer commits new data.
pub struct Store {
    reader: Mutex<Connection>,
    writer: Mutex<Connection>,
    migration_backup: Option<PathBuf>,
}

impl Store {
    /// Open or create the durable database under an existing profile directory.
    pub fn open_profile(profile_root: &Path) -> Result<Self> {
        if !profile_root.is_dir() {
            return Err(Error::InvalidInput("Profile directory does not exist"));
        }
        Self::open(&profile_root.join(DATABASE_FILENAME))
    }

    pub fn open(path: &Path) -> Result<Self> {
        if path.as_os_str().is_empty() || path == Path::new(":memory:") {
            return Err(Error::InvalidInput("An on-disk database path is required"));
        }
        create_private_file(path)?;
        let mut writer = connect(path, false)?;
        let version = schema_version(&writer)?;
        // Check known files before assuming an interrupted migration may be retried.
        if version != 0 && version < SCHEMA_VERSION {
            check_integrity(&writer)?;
        }
        let mode: String = writer.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(Error::Unavailable);
        }
        writer.pragma_update(None, "synchronous", "FULL")?;
        let migration_backup = migrate(&mut writer, path)?;
        let reader = connect(path, true)?;
        Ok(Self {
            reader: Mutex::new(reader),
            writer: Mutex::new(writer),
            migration_backup,
        })
    }

    /// Backup created when *this call* to open upgraded an existing older schema.
    pub fn migration_backup(&self) -> Option<&Path> {
        self.migration_backup.as_deref()
    }

    /// Export a consistent SQLite snapshot, including committed WAL data.
    /// The destination must be a new file in an existing directory.
    pub fn backup_to(&self, destination: &Path) -> Result<()> {
        self.read(|reader| publish_snapshot(reader, destination, false))
    }

    /// Verify a saved snapshot and copy it to a new file. The source and original
    /// profile are left intact. Open the recovered file to apply supported upgrades.
    pub fn recover_to(backup: &Path, destination: &Path) -> Result<()> {
        let reader = connect(backup, true)?;
        if schema_version(&reader)? == 0 {
            return Err(Error::ForeignDatabase);
        }
        check_integrity(&reader)?;
        publish_snapshot(&reader, destination, true)
    }

    /// Check SQLite pages and foreign keys without resetting or replacing data.
    pub fn check_integrity(&self) -> Result<()> {
        self.read(check_integrity)
    }

    /// Repair the derived search index transactionally. Durable rows remain in place.
    pub fn repair_search_index(&self) -> Result<()> {
        self.write(|tx| {
            tx.execute(
                "INSERT INTO item_search (item_search) VALUES ('rebuild')",
                [],
            )?;
            Ok(())
        })
    }

    pub(crate) fn read<T>(&self, run: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let reader = lock(&self.reader)?;
        run(&reader)
    }

    pub(crate) fn write<T>(&self, run: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let mut writer = lock(&self.writer)?;
        with_write(&mut writer, run)
    }
}

fn lock(connection: &Mutex<Connection>) -> Result<MutexGuard<'_, Connection>> {
    connection.lock().map_err(|_| Error::Unavailable)
}

fn with_write<T>(
    connection: &mut Connection,
    run: impl FnOnce(&Transaction<'_>) -> Result<T>,
) -> Result<T> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let result = run(&tx)?;
    tx.commit()?;
    Ok(result)
}

fn connect(path: &Path, read_only: bool) -> Result<Connection> {
    let access = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    };
    let connection = Connection::open_with_flags(path, access | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.pragma_update(None, "foreign_keys", true)?;
    Ok(connection)
}

fn create_private_file(path: &Path) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn schema_version(connection: &Connection) -> Result<u32> {
    let (application, version, empty): (u32, u32, bool) = connection.query_row(
        "SELECT application_id, user_version, NOT EXISTS (SELECT 1 FROM sqlite_schema)
         FROM pragma_application_id(), pragma_user_version()",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if application == APPLICATION_ID {
        return match version {
            1..=SCHEMA_VERSION => Ok(version),
            found => Err(Error::IncompatibleSchema { found }),
        };
    }
    // SQLite gives an empty, unclaimed file both zeros. Other applications can
    // leave both fields zero too, so refuse to migrate an existing foreign schema.
    if application == 0 && version == 0 && empty {
        return Ok(0);
    }
    Err(Error::ForeignDatabase)
}

fn migrate(connection: &mut Connection, path: &Path) -> Result<Option<PathBuf>> {
    with_write(connection, |tx| {
        let version = schema_version(tx)?;
        if version == SCHEMA_VERSION {
            return Ok(None);
        }
        let backup = if version > 0 {
            // A separate read-only connection snapshots the committed state while
            // this IMMEDIATE transaction prevents another process from writing it.
            let source = connect(path, true)?;
            check_integrity(&source)?;
            let prefix = format!("threadline-schema-{version}-");
            let scratch = Builder::new().prefix(&prefix).tempfile_in(parent(path))?;
            let destination = scratch.path().with_extension("sqlite3");
            write_snapshot(&source, scratch, &destination, false)?;
            Some(destination)
        } else {
            None
        };
        if version < 1 {
            tx.execute_batch(include_str!("identity.sql"))?;
            tx.execute_batch(include_str!("resources.sql"))?;
            tx.execute_batch(include_str!("queue.sql"))?;
            tx.pragma_update(None, "application_id", APPLICATION_ID)?;
            tx.pragma_update(None, "user_version", 1)?;
        }
        if version < 2 {
            tx.execute_batch(include_str!("search.sql"))?;
            tx.pragma_update(None, "user_version", 2)?;
        }
        check_foreign_keys(tx)?;
        Ok(backup)
    })
}

fn check_integrity(connection: &Connection) -> Result<()> {
    let result: String = connection.pragma_query_value(None, "quick_check", |row| row.get(0))?;
    if result != "ok" {
        return Err(Error::Corrupt);
    }
    check_foreign_keys(connection)
}

fn check_foreign_keys(connection: &Connection) -> Result<()> {
    let mut statement = connection.prepare("PRAGMA foreign_key_check")?;
    if statement.query([])?.next()?.is_some() {
        return Err(Error::Corrupt);
    }
    Ok(())
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn publish_snapshot(source: &Connection, destination: &Path, recovered: bool) -> Result<()> {
    if destination.file_name().is_none() {
        return Err(Error::InvalidInput("A backup filename is required"));
    }
    if destination.try_exists()? {
        return Err(Error::Conflict("Backup destination already exists"));
    }
    let scratch = Builder::new()
        .prefix(".threadline-backup-")
        .tempfile_in(parent(destination))?;
    write_snapshot(source, scratch, destination, recovered)
}

fn write_snapshot(
    source: &Connection,
    scratch: NamedTempFile,
    destination: &Path,
    recovered: bool,
) -> Result<()> {
    {
        let mut target = connect(scratch.path(), false)?;
        let deadline = Instant::now() + BACKUP_TIMEOUT;
        {
            let backup = Backup::new(source, &mut target)?;
            loop {
                if Instant::now() >= deadline {
                    return Err(Error::BackupTimeout);
                }
                match backup.step(256)? {
                    StepResult::Done => break,
                    StepResult::More => {}
                    _ => thread::sleep(Duration::from_millis(10)),
                }
            }
        }
        check_integrity(&target)?;
        if recovered {
            with_write(&mut target, hold_restored_dispatch)?;
        }
        // Publish a self-contained SQLite file without WAL sidecars.
        let mode: String =
            target.query_row("PRAGMA journal_mode = DELETE", [], |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("delete") {
            return Err(Error::Unavailable);
        }
    }
    scratch.as_file().sync_all()?;
    let snapshot = scratch
        .persist_noclobber(destination)
        .map_err(|error| error.error)?;
    snapshot.sync_all()?;
    #[cfg(unix)]
    std::fs::File::open(parent(destination))?.sync_all()?;
    Ok(())
}
