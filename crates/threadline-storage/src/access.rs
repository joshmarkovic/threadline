use std::num::NonZeroU64;

use rusqlite::{
    Connection, OptionalExtension, Row, params,
    types::{FromSql, FromSqlError, ValueRef},
};

use crate::{AccountId, Error, RepositoryId, Result, Store, identity::ensure_repository_in};

const GITHUB_HOST: &str = "github.com";

macro_rules! stored_enum {
    ($name:ident { $($variant:ident = $number:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        #[repr(i64)]
        pub enum $name { $($variant = $number),+ }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> std::result::Result<Self, FromSqlError> {
                match value.as_i64()? {
                    $($number => Ok(Self::$variant),)+
                    value => Err(FromSqlError::OutOfRange(value)),
                }
            }
        }
    };
}

stored_enum!(CredentialKind { FineGrained = 0, Classic = 1 });
stored_enum!(AccessState {
    Unchecked = 0,
    Readable = 1,
    SignedOut = 2,
    CredentialUnavailable = 3,
    Unauthorized = 4,
    SsoRequired = 5,
    ApprovalPending = 6,
    InsufficientPermission = 7,
    NotFoundOrDenied = 8,
    TransportUnavailable = 9,
    RateLimited = 10,
    ServiceUnavailable = 11,
});
stored_enum!(DiscoveryState { Unchecked = 0, Partial = 1, RestrictedBySso = 2, Complete = 3 });

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessProfile {
    pub account: AccountId,
    pub user_id: NonZeroU64,
    pub login: String,
    pub owner_scope: String,
    /// Opaque, per-profile reference. Credential bytes remain with the OS adapter.
    pub credential_ref: String,
    pub kind: CredentialKind,
    pub access: AccessState,
    pub discovery: DiscoveryState,
    pub checked_at_ms: Option<i64>,
    generation: i64,
    credential_generation: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryView {
    pub id: RepositoryId,
    pub provider_id: NonZeroU64,
    pub owner: String,
    pub name: String,
    pub selected: bool,
    pub private: Option<bool>,
    pub access: AccessState,
    pub checked_at_ms: i64,
    pub last_success_at_ms: Option<i64>,
    pub verified_current: bool,
    pub cached_items: i64,
    pub cached_text_bytes: i64,
}

pub struct RepositoryObservation<'a> {
    pub provider_id: NonZeroU64,
    pub owner: &'a str,
    pub name: &'a str,
    pub private: bool,
    pub source: RepositoryObservationSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryObservationSource {
    Discovery,
    Direct,
}

fn decode_id(row: &Row<'_>, index: usize) -> rusqlite::Result<NonZeroU64> {
    row.get::<_, String>(index)?.parse().map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn read_profile(row: &Row<'_>) -> rusqlite::Result<AccessProfile> {
    Ok(AccessProfile {
        account: row.get(0)?,
        user_id: decode_id(row, 1)?,
        login: row.get(2)?,
        owner_scope: row.get(3)?,
        credential_ref: row.get(4)?,
        kind: row.get(5)?,
        access: row.get(6)?,
        discovery: row.get(7)?,
        checked_at_ms: row.get(8)?,
        generation: row.get(9)?,
        credential_generation: row.get(10)?,
    })
}

const PROFILE_COLUMNS: &str =
    "p.account_id,a.provider_user_id,p.login,p.owner_scope,p.credential_ref,p.credential_kind,
    p.access,p.discovery,p.checked_at_ms,p.generation,p.credential_generation";
const PROFILE_TABLES: &str = "github_access_profiles p JOIN accounts a ON p.account_id=a.id";

fn profile(db: &Connection, account: AccountId, owner: &str) -> Result<Option<AccessProfile>> {
    Ok(db.query_row(
        &format!("SELECT {PROFILE_COLUMNS} FROM {PROFILE_TABLES} WHERE p.account_id=?1 AND p.owner_scope=?2 AND a.host=?3"),
        params![account,owner,GITHUB_HOST],
        read_profile,
    ).optional()?)
}

fn verify_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 100
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    if !valid {
        return Err(Error::InvalidInput("invalid GitHub owner or account name"));
    }
    Ok(())
}

fn require_current_profile(db: &Connection, profile: &AccessProfile) -> Result<()> {
    let found: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM github_access_profiles
         WHERE account_id=?1 AND owner_scope=?2 AND credential_ref=?3 AND generation=?4)",
        params![
            profile.account,
            profile.owner_scope,
            profile.credential_ref,
            profile.generation
        ],
        |row| row.get(0),
    )?;
    if !found {
        return Err(Error::Conflict("access profile changed during the check"));
    }
    Ok(())
}

fn refresh_account_authorization(db: &Connection, account: AccountId) -> Result<()> {
    db.execute(
        "INSERT INTO dispatch_profiles(account_id,authorized) VALUES (?1, EXISTS(
            SELECT 1 FROM github_access_profiles WHERE account_id=?1 AND access=1))
         ON CONFLICT(account_id) DO UPDATE SET authorized=excluded.authorized",
        [account],
    )?;
    Ok(())
}

impl Store {
    /// Reserve or reuse the stable protected-store reference after verifying the GitHub user's identity.
    pub fn reserve_access_profile_credential(
        &self,
        account: AccountId,
        user_id: NonZeroU64,
        login: &str,
        owner: &str,
        kind: CredentialKind,
        previous: Option<&AccessProfile>,
    ) -> Result<AccessProfile> {
        verify_name(login)?;
        verify_name(owner)?;
        self.write(|tx| {
            if let Some(previous) = previous {
                if previous.account != account || !previous.owner_scope.eq_ignore_ascii_case(owner) {
                    return Err(Error::Conflict("replacement credential does not match the profile"));
                }
                require_current_profile(tx,previous)?;
            }
            let verified: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1 AND host=?2 AND provider_user_id=?3)",
                params![account,GITHUB_HOST,user_id.to_string()],
                |row| row.get(0),
            )?;
            if !verified { return Err(Error::Conflict("GitHub identity does not match the local account")); }
            tx.execute(
                "INSERT INTO github_access_profiles(account_id,owner_scope,login,credential_ref,credential_kind)
                 VALUES (?1,?2,?3,lower(hex(randomblob(16))),?4)
                 ON CONFLICT(account_id,owner_scope) DO UPDATE SET login=excluded.login,
                   credential_kind=excluded.credential_kind,
                   generation=github_access_profiles.generation+1,
                   credential_generation=github_access_profiles.credential_generation+1,
                   access=0,discovery=0,checked_at_ms=NULL",
                params![account,owner,login,kind as i64],
            )?;
            refresh_account_authorization(tx,account)?;
            profile(tx,account,owner)?.ok_or(Error::NotFound)
        })
    }

    /// Fence a refresh without disturbing repository checks made with the same stored token.
    pub fn begin_access_check(&self, previous: &AccessProfile) -> Result<AccessProfile> {
        self.write(|tx| {
            require_current_profile(tx, previous)?;
            if previous.access == AccessState::SignedOut {
                return Err(Error::Conflict("signed out credentials cannot be refreshed"));
            }
            let changed = tx.execute(
                "UPDATE github_access_profiles SET generation=generation+1,access=0,checked_at_ms=NULL
                 WHERE account_id=?1 AND owner_scope=?2 AND installed_generation=credential_generation",
                params![previous.account,previous.owner_scope],
            )?;
            if changed != 1 {
                return Err(Error::Conflict("profile has no installed credential"));
            }
            refresh_account_authorization(tx,previous.account)?;
            profile(tx,previous.account,&previous.owner_scope)?.ok_or(Error::NotFound)
        })
    }

    /// Save only after the earlier reservation committed a dispatch hold.
    /// A failed save or database commit leaves that hold in effect.
    pub fn install_current_credential<E>(
        &self,
        profile: &AccessProfile,
        save: impl FnOnce() -> std::result::Result<(), E>,
    ) -> Result<std::result::Result<(), E>> {
        self.write(|tx| {
            require_current_profile(tx, profile)?;
            if profile.access != AccessState::Unchecked {
                return Err(Error::Conflict(
                    "profile is not awaiting credential installation",
                ));
            }
            let installed = save();
            if installed.is_ok() {
                tx.execute(
                    "UPDATE github_access_profiles SET installed_generation=credential_generation
                     WHERE account_id=?1 AND owner_scope=?2",
                    params![profile.account, profile.owner_scope],
                )?;
            }
            Ok(installed)
        })
    }

    /// Serialize a short protected-store read or deletion with account-generation changes.
    /// The callback must not perform network work or reenter the database.
    pub fn with_current_credential<T>(
        &self,
        profile: &AccessProfile,
        run: impl FnOnce() -> T,
    ) -> Result<T> {
        self.write(|tx| {
            require_current_profile(tx, profile)?;
            Ok(run())
        })
    }

    pub fn github_access_profile(
        &self,
        account: AccountId,
        owner: &str,
    ) -> Result<Option<AccessProfile>> {
        self.read(|db| profile(db, account, owner))
    }

    pub fn github_access_profiles(&self) -> Result<Vec<AccessProfile>> {
        self.read(|db| {
            let mut query = db.prepare(&format!(
                "SELECT {PROFILE_COLUMNS} FROM {PROFILE_TABLES} WHERE a.host=?1 ORDER BY p.login COLLATE NOCASE,p.owner_scope"
            ))?;
            let entries = query.query_map([GITHUB_HOST],read_profile)?.collect::<rusqlite::Result<_>>()?;
            Ok(entries)
        })
    }

    pub fn active_github_account(&self) -> Result<Option<AccountId>> {
        self.read(|db| {
            Ok(db
                .query_row(
                    "SELECT account_id FROM active_profile WHERE EXISTS(
               SELECT 1 FROM github_access_profiles WHERE account_id=active_profile.account_id)",
                    [],
                    |row| row.get(0),
                )
                .optional()?)
        })
    }

    pub fn record_access_result(
        &self,
        profile: &AccessProfile,
        kind: CredentialKind,
        access: AccessState,
        discovery: DiscoveryState,
        at_ms: i64,
    ) -> Result<bool> {
        self.write(|tx| {
            let changed = tx.execute(
                "UPDATE github_access_profiles SET credential_kind=?4,access=?5,discovery=?6,checked_at_ms=?7
                 WHERE account_id=?1 AND owner_scope=?2 AND credential_ref=?3 AND (checked_at_ms IS NULL OR checked_at_ms<=?7)
                       AND generation=?8 AND (?5<>1 OR installed_generation=credential_generation)",
                params![profile.account,profile.owner_scope,profile.credential_ref,kind as i64,access as i64,discovery as i64,at_ms,profile.generation],
            )?;
            if changed != 1 { return Ok(false); }

            // Any other verified owner for that account may remain usable.
            refresh_account_authorization(tx,profile.account)?;
            Ok(true)
        })
    }

    /// Persist the network hold before asking the platform adapter to erase protected entries.
    pub fn sign_out_github_account(
        &self,
        account: AccountId,
        at_ms: i64,
    ) -> Result<Vec<AccessProfile>> {
        self.write(|tx| {
            tx.execute(
                "UPDATE github_access_profiles SET access=?2,checked_at_ms=?3,generation=generation+1,
                         credential_generation=credential_generation+1 WHERE account_id=?1",
                params![account, AccessState::SignedOut as i64, at_ms],
            )?;
            tx.execute(
                "INSERT INTO dispatch_profiles(account_id,authorized) VALUES (?1,0)
                 ON CONFLICT(account_id) DO UPDATE SET authorized=0",
                [account],
            )?;
            let mut query = tx.prepare(&format!(
                "SELECT {PROFILE_COLUMNS} FROM {PROFILE_TABLES} WHERE p.account_id=?1"
            ))?;
            let entries = query
                .query_map([account], read_profile)?
                .collect::<rusqlite::Result<_>>()?;
            Ok(entries)
        })
    }

    pub fn observe_github_repository(
        &self,
        profile: &AccessProfile,
        repo: &RepositoryObservation<'_>,
        at_ms: i64,
    ) -> Result<RepositoryId> {
        if !repo.owner.eq_ignore_ascii_case(&profile.owner_scope) {
            return Err(Error::Conflict(
                "repository is outside the verified credential owner",
            ));
        }
        self.write(|tx| {
            require_current_profile(tx,profile)?;
            let id = ensure_repository_in(tx,profile.account,repo.provider_id.get(),repo.owner,repo.name)?;
            match repo.source {
                RepositoryObservationSource::Discovery => tx.execute(
                    "INSERT INTO github_repository_access(repository_id,account_id,owner_scope,private,access,checked_at_ms)
                     VALUES (?1,?2,?3,?4,0,?5) ON CONFLICT(repository_id,account_id) DO NOTHING",
                    params![id,profile.account,profile.owner_scope,repo.private,at_ms],
                )?,
                RepositoryObservationSource::Direct => tx.execute(
                    "INSERT INTO github_repository_access(repository_id,account_id,owner_scope,private,access,checked_at_ms,last_success_at_ms,verified_generation)
                     VALUES (?1,?2,?3,?4,1,?5,?5,?6)
                     ON CONFLICT(repository_id,account_id) DO UPDATE SET owner_scope=excluded.owner_scope,
                     private=excluded.private,access=1,checked_at_ms=excluded.checked_at_ms,
                     last_success_at_ms=excluded.last_success_at_ms,verified_generation=excluded.verified_generation",
                    params![id,profile.account,profile.owner_scope,repo.private,at_ms,profile.credential_generation],
                )?,
            };
            Ok(id)
        })
    }

    /// Store denials without deleting cache, selection, or the prior successful check.
    pub fn mark_repository_access(
        &self,
        profile: &AccessProfile,
        repo: RepositoryId,
        access: AccessState,
        at_ms: i64,
    ) -> Result<bool> {
        self.write(|tx| {
            require_current_profile(tx, profile)?;
            Ok(tx.execute(
                "UPDATE github_repository_access SET access=?4,checked_at_ms=?5,verified_generation=NULL
                 WHERE repository_id=?1 AND account_id=?2 AND owner_scope=?3",
                params![
                    repo,
                    profile.account,
                    profile.owner_scope,
                    access as i64,
                    at_ms
                ],
            )? == 1)
        })
    }

    pub fn select_github_repository(
        &self,
        account: AccountId,
        id: RepositoryId,
        selected: bool,
    ) -> Result<()> {
        self.write(|tx| {
            let updated = tx.execute(
                "UPDATE github_repository_access SET selected=?3 WHERE repository_id=?1 AND account_id=?2
                 AND (?3=0 OR (access=1 AND EXISTS(SELECT 1 FROM github_access_profiles profile
                      WHERE profile.account_id=?2 AND profile.owner_scope=github_repository_access.owner_scope
                        AND profile.access=1 AND profile.credential_generation=github_repository_access.verified_generation)))",
                params![id,account,selected],
            )?;
            if updated!=1 { return Err(Error::NotFound); }
            Ok(())
        })
    }

    pub fn github_repository_views(&self, account: AccountId) -> Result<Vec<RepositoryView>> {
        self.read(|db| {
            let mut query = db.prepare(
                "SELECT r.id,r.provider_repository_id,r.owner,r.name,p.selected,p.private,p.access,p.checked_at_ms,p.last_success_at_ms,
                         p.verified_generation IS profile.credential_generation,
                         (SELECT count(*) FROM items i WHERE i.repository_id=r.id AND i.account_id=r.account_id),
                         (SELECT coalesce(sum(coalesce(length(cast(title AS BLOB)),0)+coalesce(length(cast(body AS BLOB)),0)),0)
                          FROM items i WHERE i.repository_id=r.id AND i.account_id=r.account_id)
                  FROM github_repository_access p JOIN repositories r ON r.id=p.repository_id AND r.account_id=p.account_id
                  JOIN github_access_profiles profile ON profile.account_id=p.account_id AND profile.owner_scope=p.owner_scope
                  WHERE r.account_id=?1 ORDER BY p.selected DESC,r.owner,r.name",
            )?;
            let entries = query.query_map([account],|row| Ok(RepositoryView {
                id:row.get(0)?,provider_id:decode_id(row,1)?,owner:row.get(2)?,name:row.get(3)?,selected:row.get(4)?,
                private:row.get(5)?,access:row.get(6)?,checked_at_ms:row.get(7)?,last_success_at_ms:row.get(8)?,
                verified_current:row.get(9)?,cached_items:row.get(10)?,cached_text_bytes:row.get(11)?,
            }))?.collect::<rusqlite::Result<_>>()?;
            Ok(entries)
        })
    }

    pub fn credential_for_bound_operation(
        &self,
        account: AccountId,
        operation: &str,
    ) -> Result<Option<AccessProfile>> {
        self.read(|db| Ok(db.query_row(
            &format!("SELECT {PROFILE_COLUMNS} FROM {PROFILE_TABLES}
                JOIN github_operation_scopes s ON s.account_id=p.account_id AND s.owner_scope=p.owner_scope
                JOIN operation_gates g ON g.account_id=s.account_id AND g.id=s.operation_id
                WHERE s.account_id=?1 AND s.operation_id=?2 AND g.hold IS NULL AND p.access=1 AND a.host=?3"),
            params![account,operation,GITHUB_HOST],
            read_profile,
        ).optional()?))
    }
}
