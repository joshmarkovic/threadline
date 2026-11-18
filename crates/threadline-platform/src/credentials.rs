use crate::{Credential, CredentialRef, CredentialStore, PlatformError};

/// Credentials are stored by Windows Credential Manager for the current OS user.
/// On other platforms this adapter returns `Unsupported`.
pub struct NativeCredentialStore;

impl CredentialRef {
    /// Build a reference after the provider has verified the user and resource owner.
    /// Keep the same local profile key across authorized same-user renewal.
    pub fn github_profile(
        user_id: u64,
        owner: &str,
        profile_key: &str,
    ) -> Result<Self, PlatformError> {
        let owner = owner.as_bytes();
        if user_id == 0
            || owner.len() > 39
            || !owner.first().is_some_and(u8::is_ascii_alphanumeric)
            || !owner.last().is_some_and(u8::is_ascii_alphanumeric)
            || !owner
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
            || !valid_component(profile_key)
        {
            return Err(PlatformError::InvalidCredentialReference);
        }
        Ok(Self {
            scope: format!(
                "github.com/{user_id}/{}",
                owner.to_ascii_lowercase().escape_ascii()
            ),
            name: profile_key.to_owned(),
        })
    }
}

fn valid_component(value: &str) -> bool {
    !value.is_empty() && value.len() <= 1024 && !value.contains('\0')
}

#[cfg(windows)]
impl CredentialStore for NativeCredentialStore {
    fn read(&self, credential: &CredentialRef) -> Result<Option<Credential>, PlatformError> {
        windows::read(&target_name(credential)?)
    }

    fn save(&self, credential: &CredentialRef, value: &Credential) -> Result<(), PlatformError> {
        windows::save(&target_name(credential)?, value)
    }

    fn remove(&self, credential: &CredentialRef) -> Result<(), PlatformError> {
        windows::remove(&target_name(credential)?)
    }
}

#[cfg(not(windows))]
impl CredentialStore for NativeCredentialStore {
    fn read(&self, _: &CredentialRef) -> Result<Option<Credential>, PlatformError> {
        Err(PlatformError::Unsupported)
    }

    fn save(&self, _: &CredentialRef, _: &Credential) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }

    fn remove(&self, _: &CredentialRef) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported)
    }
}

#[cfg(any(windows, test))]
fn target_name(reference: &CredentialRef) -> Result<Vec<u16>, PlatformError> {
    use std::fmt::Write as _;

    if !valid_component(&reference.scope) || !valid_component(&reference.name) {
        return Err(PlatformError::InvalidCredentialReference);
    }
    // Generic credential targets compare case-insensitively. Encoding both
    // opaque components protects distinct case, Unicode and delimiter values.
    let mut target = String::from("Threadline:v1:");
    for byte in reference.scope.as_bytes() {
        write!(&mut target, "{byte:02x}").expect("write to String");
    }
    target.push(':');
    for byte in reference.name.as_bytes() {
        write!(&mut target, "{byte:02x}").expect("write to String");
    }
    Ok(target.encode_utf16().chain([0]).collect())
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{ptr, slice};
    use windows_sys::Win32::{
        Foundation::{
            ERROR_ACCESS_DENIED, ERROR_ACCESS_DISABLED_BY_POLICY, ERROR_LOCK_FAILED,
            ERROR_LOCK_VIOLATION, ERROR_NO_SUCH_LOGON_SESSION, ERROR_NOT_FOUND,
            ERROR_NOT_LOGGED_ON, ERROR_PRIVILEGE_NOT_HELD, ERROR_SERVICE_NOT_ACTIVE, GetLastError,
        },
        Security::Credentials::{
            CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
            CREDENTIALW, CredDeleteW, CredFree, CredReadW, CredWriteW,
        },
    };

    struct OwnedCredential(ptr::NonNull<CREDENTIALW>);

    impl Drop for OwnedCredential {
        fn drop(&mut self) {
            unsafe { CredFree(self.0.as_ptr().cast()) };
        }
    }

    pub fn read(target: &[u16]) -> Result<Option<Credential>, PlatformError> {
        let mut raw = ptr::null_mut();
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) } == 0 {
            return match unsafe { GetLastError() } {
                ERROR_NOT_FOUND => Ok(None),
                error => Err(map_error("Read protected credential", error)),
            };
        }
        let credential = OwnedCredential(ptr::NonNull::new(raw).ok_or(PlatformError::Unavailable)?);
        let credential = unsafe { credential.0.as_ref() };
        let size = credential.CredentialBlobSize;
        if size == 0 || size > CRED_MAX_CREDENTIAL_BLOB_SIZE || credential.CredentialBlob.is_null()
        {
            return Err(PlatformError::InvalidCredentialValue);
        }
        let bytes = unsafe { slice::from_raw_parts(credential.CredentialBlob, size as usize) };
        Ok(Some(Credential(bytes.to_vec())))
    }

    pub fn save(target: &[u16], credential: &Credential) -> Result<(), PlatformError> {
        let length = credential.0.len();
        if length == 0 || length > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
            return Err(PlatformError::InvalidCredentialValue);
        }
        let raw = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_ptr().cast_mut(),
            CredentialBlobSize: length as u32,
            CredentialBlob: credential.0.as_ptr().cast_mut(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..Default::default()
        };
        if unsafe { CredWriteW(&raw, 0) } == 0 {
            return Err(map_error("Save protected credential", unsafe {
                GetLastError()
            }));
        }
        Ok(())
    }

    pub fn remove(target: &[u16]) -> Result<(), PlatformError> {
        if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
            let error = unsafe { GetLastError() };
            if error != ERROR_NOT_FOUND {
                return Err(map_error("Remove protected credential", error));
            }
        }
        Ok(())
    }

    fn map_error(operation: &'static str, code: u32) -> PlatformError {
        match code {
            ERROR_ACCESS_DENIED | ERROR_ACCESS_DISABLED_BY_POLICY | ERROR_PRIVILEGE_NOT_HELD => {
                PlatformError::Denied
            }
            ERROR_LOCK_FAILED | ERROR_LOCK_VIOLATION => PlatformError::Locked,
            ERROR_NO_SUCH_LOGON_SESSION | ERROR_NOT_LOGGED_ON | ERROR_SERVICE_NOT_ACTIVE => {
                PlatformError::Unavailable
            }
            code => PlatformError::Native { operation, code },
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::windows::process::CommandExt as _;

        const RESTART_KEY: &str = "THREADLINE_WINDOWS_CREDENTIAL_TEST_RESTART";
        const REPLACEMENT: &[u8] = "Threadline synthetic renewal 例".as_bytes();

        fn refs(key: &str) -> [CredentialRef; 3] {
            [
                CredentialRef::github_profile(1201, "fixture-owner-a", key).unwrap(),
                CredentialRef::github_profile(1201, "fixture-owner-b", key).unwrap(),
                CredentialRef::github_profile(1202, "fixture-owner-a", key).unwrap(),
            ]
        }

        struct CleanCredentials(Vec<CredentialRef>);

        impl Drop for CleanCredentials {
            fn drop(&mut self) {
                for reference in &self.0 {
                    let _ = NativeCredentialStore.remove(reference);
                }
            }
        }

        #[test]
        fn native_failure_categories_do_not_expose_target_or_credential() {
            assert_eq!(
                map_error("fixture", ERROR_ACCESS_DENIED),
                PlatformError::Denied
            );
            assert_eq!(
                map_error("fixture", ERROR_LOCK_VIOLATION),
                PlatformError::Locked
            );
            assert_eq!(
                map_error("fixture", ERROR_NO_SUCH_LOGON_SESSION),
                PlatformError::Unavailable
            );
        }

        #[test]
        #[ignore = "requires a Windows user logon with Credential Manager access"]
        fn native_synthetic_protected_store_round_trip() {
            let paths = crate::PathResolver::application_paths(&crate::NativePaths).unwrap();
            assert!(paths.data.is_absolute());
            assert!(paths.data.ends_with("Threadline/Data"));
            assert!(paths.cache.ends_with("Threadline/Cache"));
            assert!(!paths.data.starts_with(&paths.cache));

            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let key = format!("threadline-fixture-{}-{nanos}-例", std::process::id());
            let [origin, sibling, other_user] = refs(&key);
            let store = NativeCredentialStore;
            for reference in [&origin, &sibling, &other_user] {
                assert!(store.read(reference).unwrap().is_none());
            }

            let cleanup = CleanCredentials(refs(&key).into_iter().take(2).collect());
            store
                .save(&origin, &Credential(vec![1, 2, 0, 255]))
                .unwrap();
            assert!(
                store
                    .read(&origin)
                    .unwrap()
                    .is_some_and(|value| value.0 == [1, 2, 0, 255])
            );
            assert!(store.read(&sibling).unwrap().is_none());
            store.save(&sibling, &Credential(vec![15])).unwrap();
            store
                .save(&origin, &Credential(REPLACEMENT.to_vec()))
                .unwrap();
            assert!(
                store
                    .read(&sibling)
                    .unwrap()
                    .is_some_and(|value| value.0 == [15])
            );
            assert!(store.read(&other_user).unwrap().is_none());

            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let restart = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "credentials::windows::tests::native_restart_fixture_reader",
                ])
                .env(RESTART_KEY, key)
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .unwrap();
            assert!(
                restart.status.success()
                    && String::from_utf8_lossy(&restart.stdout).contains("1 passed"),
                "synthetic credential was not available after restart"
            );

            store.remove(&origin).unwrap();
            store.remove(&sibling).unwrap();
            assert!(store.read(&origin).unwrap().is_none());
            assert!(store.read(&sibling).unwrap().is_none());
            drop(cleanup);
        }

        #[test]
        #[ignore = "child invoked only by the protected-store restart fixture"]
        fn native_restart_fixture_reader() {
            let key = std::env::var(RESTART_KEY).expect("parent test fixture key");
            assert!(key.starts_with("threadline-fixture-") && key.ends_with("-例"));
            let [origin, sibling, other_user] = refs(&key);
            let store = NativeCredentialStore;
            assert!(
                store
                    .read(&origin)
                    .unwrap()
                    .is_some_and(|value| value.0 == REPLACEMENT)
            );
            assert!(
                store
                    .read(&sibling)
                    .unwrap()
                    .is_some_and(|value| value.0 == [15])
            );
            assert!(store.read(&other_user).unwrap().is_none());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_separates_owner_user_profile_and_opaque_components() {
        let first = CredentialRef::github_profile(10, "Test-Owner", "Fixture-例").unwrap();
        let same = CredentialRef::github_profile(10, "test-owner", "Fixture-例").unwrap();
        let other_profile = CredentialRef::github_profile(10, "test-owner", "fixture-例").unwrap();
        let other_owner = CredentialRef::github_profile(10, "other-owner", "Fixture-例").unwrap();
        let other_user = CredentialRef::github_profile(11, "test-owner", "Fixture-例").unwrap();
        assert_eq!(target_name(&first), target_name(&same));
        for other in [other_profile, other_owner, other_user] {
            assert_ne!(target_name(&first), target_name(&other));
        }
        let left = CredentialRef {
            scope: "a:b".into(),
            name: "c".into(),
        };
        let right = CredentialRef {
            scope: "a".into(),
            name: "b:c".into(),
        };
        assert_ne!(target_name(&left), target_name(&right));
        assert!(CredentialRef::github_profile(10, "foreign/owner", "fixture").is_err());
        assert!(CredentialRef::github_profile(10, "fixture", "name\0tail").is_err());
    }
}
