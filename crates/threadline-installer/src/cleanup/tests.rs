use super::*;
use std::cell::Cell;
use tempfile::TempDir;
use threadline_platform::{Credential, PlatformError};
use threadline_storage::{
    Store,
    fixtures::{Scale, populate_reader},
};

fn synthetic_profile() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(DATABASE);
    let store = Store::open(&path).unwrap();
    let fixture = populate_reader(&store, 12345, Scale::Small).unwrap();
    assert_eq!(
        store.active_github_account().unwrap(),
        Some(fixture.account)
    );
    drop(store);
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.execute(
            "UPDATE github_access_profiles SET credential_ref=lower(hex(randomblob(16)))",
            []
        )
        .unwrap(),
        1
    );
    drop(db);
    dir
}

#[derive(Default)]
struct TestVault {
    removals: Cell<usize>,
    denied: bool,
}

impl CredentialStore for TestVault {
    fn read(&self, _: &CredentialRef) -> std::result::Result<Option<Credential>, PlatformError> {
        panic!("removal must never read a secret")
    }
    fn save(&self, _: &CredentialRef, _: &Credential) -> std::result::Result<(), PlatformError> {
        panic!("removal must never replace a secret")
    }
    fn remove(&self, _: &CredentialRef) -> std::result::Result<(), PlatformError> {
        self.removals.set(self.removals.get() + 1);
        if self.denied {
            Err(PlatformError::Denied)
        } else {
            Ok(())
        }
    }
}

#[test]
fn cleanup_skips_the_cli_binding_and_retains_pat_backups_in_its_inventory() {
    let dir = synthetic_profile();
    let data = dir.path();
    let current = data.join(DATABASE);
    let backup = data.join("threadline-schema-18-Prior1.sqlite3");
    fs::copy(&current, &backup).unwrap();
    Connection::open(&backup)
        .unwrap()
        .pragma_update(None, "user_version", 18)
        .unwrap();
    Connection::open(&current).unwrap().execute(
        "UPDATE github_access_profiles SET credential_kind=2, credential_ref=lower(hex(randomblob(16)))", []
    ).unwrap();
    assert_eq!(Preview::inspect(data).unwrap().credentials.len(), 1);
    fs::remove_file(backup).unwrap();
    assert!(Preview::inspect(data).unwrap().credentials.is_empty());
    Connection::open(&current)
        .unwrap()
        .execute(
            "INSERT INTO retired_github_pat_refs(credential_ref,account_id,owner_scope)
         SELECT lower(hex(randomblob(16))),account_id,owner_scope FROM github_access_profiles",
            [],
        )
        .unwrap();
    assert_eq!(Preview::inspect(data).unwrap().credentials.len(), 1);
}

#[test]
fn it_019_preview_counts_active_workspace_and_confirmation_removes_only_managed_files() {
    let dir = synthetic_profile();
    let data = dir.path();
    let current = data.join(DATABASE);
    let backup = data.join(format!(
        "threadline-schema-{LAST_REVIEWED_SCHEMA}-Safe01.sqlite3"
    ));
    fs::copy(&current, &backup).unwrap();
    let notes = data.join("my-export.sqlite3");
    fs::write(&notes, b"keep").unwrap();

    let preview = Preview::inspect(data).unwrap();
    assert_eq!(preview.files.len(), 2);
    assert_eq!(preview.credentials.len(), 1);
    assert!(preview.accounts > 0 && preview.drafts > 0 && preview.unresolved > 0);
    assert_eq!(preview.drafts, preview.backup_drafts);
    assert_eq!(preview.unresolved, preview.backup_unresolved);
    let vault = TestVault::default();
    let mut printed = Vec::new();
    run(
        data,
        false,
        false,
        &mut &b""[..],
        &mut printed,
        &vault,
        &CredentialRef::github_releases(),
    )
    .unwrap();
    let preview_text = String::from_utf8(printed).unwrap();
    assert!(preview_text.contains(&format!(
        "{} drafts and {} unresolved Outbox operations",
        preview.drafts, preview.unresolved
    )));
    assert!(preview_text.contains("Work already submitted may still take effect on GitHub."));
    assert!(
        run(
            data,
            true,
            false,
            &mut &b"REMOVE LOCAL WORKSPACE\n"[..],
            &mut Vec::new(),
            &vault,
            &CredentialRef::github_releases()
        )
        .is_err()
    );
    assert!(
        preview
            .confirm_removal(data, "remove", &vault, &CredentialRef::github_releases())
            .is_err()
    );
    assert_eq!(vault.removals.get(), 0);
    assert!(current.exists() && backup.exists());
    preview
        .confirm_removal(
            data,
            "REMOVE LOCAL WORKSPACE\r\n",
            &vault,
            &CredentialRef::github_releases(),
        )
        .unwrap();
    assert_eq!(vault.removals.get(), 2);
    assert!(!current.exists() && !backup.exists());
    assert_eq!(fs::read(&notes).unwrap(), b"keep");
    assert!(Preview::inspect(data).unwrap().files.is_empty());
}

#[test]
fn it_019_cleanup_covers_the_shipped_schema_and_refuses_a_newer_backup() {
    assert_eq!(
        LAST_REVIEWED_SCHEMA,
        threadline_storage::SCHEMA_VERSION,
        "Review the cleanup inventory for the current database and its backups."
    );
    let dir = synthetic_profile();
    let data = dir.path();
    let current = data.join(DATABASE);
    let future = LAST_REVIEWED_SCHEMA + 1;
    let backup = data.join(format!("threadline-schema-{future}-Future.sqlite3"));
    fs::copy(&current, &backup).unwrap();
    let db = Connection::open(&backup).unwrap();
    db.pragma_update(None, "user_version", future).unwrap();
    drop(db);
    let originals = (fs::read(&current).unwrap(), fs::read(&backup).unwrap());
    let vault = TestVault::default();
    let result = run(
        data,
        true,
        true,
        &mut &b"REMOVE LOCAL WORKSPACE\n"[..],
        &mut Vec::new(),
        &vault,
        &CredentialRef::github_releases(),
    );
    assert_eq!(result, Err(UNRECOGNIZED));
    assert_eq!(vault.removals.get(), 0);
    assert_eq!(fs::read(current).unwrap(), originals.0);
    assert_eq!(fs::read(backup).unwrap(), originals.1);
}

#[test]
fn it_019_busy_changed_or_unrecognized_profile_preserves_data_and_credentials() {
    let dir = synthetic_profile();
    let data = dir.path();
    let path = data.join(DATABASE);
    let vault = TestVault::default();
    let preview = Preview::inspect(data).unwrap();
    let busy = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&path)
        .unwrap();
    assert_eq!(
        preview.confirm_removal(
            data,
            CONFIRMATION,
            &vault,
            &CredentialRef::github_releases()
        ),
        Err(BUSY)
    );
    drop(busy);
    let journal = data.join("THREADLINE.SQLITE3-JOURNAL");
    fs::write(&journal, b"fixture").unwrap();
    assert!(Preview::inspect(data).is_err());
    fs::remove_file(journal).unwrap();
    let db = Connection::open(&path).unwrap();
    db.pragma_update(None, "user_version", LAST_REVIEWED_SCHEMA + 1)
        .unwrap();
    drop(db);
    assert_eq!(
        preview.confirm_removal(
            data,
            CONFIRMATION,
            &vault,
            &CredentialRef::github_releases()
        ),
        Err(CHANGED)
    );
    assert!(matches!(Preview::inspect(data), Err(UNRECOGNIZED)));
    assert!(path.exists());
    assert_eq!(vault.removals.get(), 0);
}

#[test]
fn it_019_vault_refusal_leaves_every_database_for_retry() {
    let dir = synthetic_profile();
    let path = dir.path().join(DATABASE);
    let backup = dir.path().join(format!(
        "threadline-schema-{LAST_REVIEWED_SCHEMA}-Old123.sqlite3"
    ));
    fs::copy(&path, &backup).unwrap();
    let db = Connection::open(&backup).unwrap();
    db.execute(
        "UPDATE github_access_profiles SET credential_ref=lower(hex(randomblob(16)))",
        [],
    )
    .unwrap();
    drop(db);
    let preview = Preview::inspect(dir.path()).unwrap();
    assert_eq!(preview.credentials.len(), 2);
    let denied = TestVault {
        denied: true,
        ..Default::default()
    };
    assert!(
        preview
            .confirm_removal(
                dir.path(),
                CONFIRMATION,
                &denied,
                &CredentialRef::github_releases()
            )
            .is_err()
    );
    assert!(path.exists() && backup.exists());
    let vault = TestVault::default();
    preview
        .confirm_removal(
            dir.path(),
            CONFIRMATION,
            &vault,
            &CredentialRef::github_releases(),
        )
        .unwrap();
    assert_eq!(vault.removals.get(), 3);
}

#[test]
#[ignore = "uses three freshly generated, test-owned native Windows Credential Manager entries"]
fn it_019_explicit_removal_deletes_only_the_generated_profile_credential() {
    struct Owned<'a>(Vec<&'a CredentialRef>);
    impl Drop for Owned<'_> {
        fn drop(&mut self) {
            for key in &self.0 {
                let _ = NativeCredentialStore.remove(key);
            }
        }
    }

    let dir = synthetic_profile();
    let data = dir.path();
    let plan = Preview::inspect(data).unwrap();
    assert_eq!(plan.credentials.len(), 1);
    let (id, owner, key) = plan.credentials.first().unwrap();
    let target = CredentialRef::github_profile(*id, owner, key).unwrap();
    let other = CredentialRef::github_profile(*id, "other-cleanup-fixture", key).unwrap();
    let release = CredentialRef::github_profile(*id, "release-cleanup-fixture", key).unwrap();
    let mut owned = Owned(Vec::new());
    for reference in [&target, &other, &release] {
        assert!(NativeCredentialStore.read(reference).unwrap().is_none());
        NativeCredentialStore
            .save(
                reference,
                &Credential(b"synthetic cleanup fixture".to_vec()),
            )
            .unwrap();
        owned.0.push(reference);
    }
    assert!(
        run(
            data,
            true,
            true,
            &mut &b"KEEP\n"[..],
            &mut Vec::new(),
            &NativeCredentialStore,
            &release
        )
        .is_err()
    );
    assert!(NativeCredentialStore.read(&target).unwrap().is_some());
    assert!(NativeCredentialStore.read(&other).unwrap().is_some());
    let mut displayed = Vec::new();
    run(
        data,
        true,
        true,
        &mut &b"REMOVE LOCAL WORKSPACE\r\n"[..],
        &mut displayed,
        &NativeCredentialStore,
        &release,
    )
    .unwrap();
    let displayed = String::from_utf8(displayed).unwrap();
    assert!(displayed.contains("Type REMOVE LOCAL WORKSPACE to confirm"));
    assert!(displayed.contains("Removed the previewed Threadline databases"));
    assert!(NativeCredentialStore.read(&target).unwrap().is_none());
    assert!(NativeCredentialStore.read(&release).unwrap().is_none());
    assert!(NativeCredentialStore.read(&other).unwrap().is_some());
    assert!(!data.join(DATABASE).exists());
    drop(owned);
    assert!(NativeCredentialStore.read(&other).unwrap().is_none());
}
