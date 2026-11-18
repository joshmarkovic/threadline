//! Synchronous, account-scoped SQLite storage. Run operations on background workers.

mod database;
mod identity;

pub mod access;
pub mod fixtures;
pub mod queue;
pub mod resources;

pub use threadline_core::domain::IssueApiId;

pub use database::{DATABASE_FILENAME, SCHEMA_VERSION, Store, UtcMillis};
pub use identity::{
    AccountId, Change, IssueClassification, IssueRecord, Item, ItemId, ItemKind, ItemLocator,
    ItemRepresentation, ItemUpdate, NodeId, PullRequestApiId, PullRequestDetails,
    PullRequestRecord, PullRequestUpdate, RepositoryId, TransferEvidence, TransferOutcome,
    TransferProblem,
};

pub type Result<T> = std::result::Result<T, Error>;

/// Display strings are safe for routine diagnostics. SQL messages can include input
/// and filesystem errors can include profile paths, so they are not used as display text.
#[derive(thiserror::Error)]
pub enum Error {
    #[error("SQLite operation failed")]
    Sql(rusqlite::Error),
    #[error("Profile filesystem operation failed")]
    Io(std::io::Error),
    #[error("The database belongs to a different application")]
    ForeignDatabase,
    #[error("Unsupported Threadline schema version {found}")]
    IncompatibleSchema { found: u32 },
    #[error("Database integrity check failed")]
    Corrupt,
    #[error("Database connection is unavailable")]
    Unavailable,
    #[error("Database backup exceeded its time limit")]
    BackupTimeout,
    #[error("Requested data was not found in this account")]
    NotFound,
    #[error("{0}")]
    InvalidInput(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
}

impl std::fmt::Debug for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sql(error)
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
