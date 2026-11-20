#[cfg(windows)]
/// Verifies the Windows trust chain and the SHA-256 digest of its actual signer leaf.
pub fn verify(path: &std::path::Path, pin: [u8; 32]) -> Result<(), u8> {
    let file = open_for_verification(path)?;
    verify_signer(path, &file, |cert| unsafe {
        publisher_sha256(cert) == Some(pin)
    })
}

/// Keep the verified executable open without write/delete sharing until the broker releases it.
pub(super) fn github_cli(path: &std::path::Path) -> Option<std::fs::File> {
    use windows_sys::Win32::Security::Cryptography::{CERT_NAME_ATTR_TYPE, CertGetNameStringW};
    let file = open_for_verification(path).ok()?;
    verify_signer(path, &file, |certificate| {
        let mut name = [0u16; 128];
        // Read the organization from the verified leaf's subject, using its X.500 attribute OID.
        let len = unsafe {
            CertGetNameStringW(
                certificate,
                CERT_NAME_ATTR_TYPE,
                0,
                c"2.5.4.10".as_ptr().cast(),
                name.as_mut_ptr(),
                name.len() as u32,
            )
        } as usize;
        (1..=name.len()).contains(&len)
            && name[..len - 1]
                .iter()
                .copied()
                .eq("GitHub, Inc.".encode_utf16())
    })
    .ok()?;
    Some(file)
}

fn open_for_verification(path: &std::path::Path) -> Result<std::fs::File, u8> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
    std::fs::OpenOptions::new()
        .share_mode(FILE_SHARE_READ)
        .read(true)
        .open(path)
        .map_err(|_| 3)
}

fn verify_signer(
    path: &std::path::Path,
    file_handle: &std::fs::File,
    accepts: impl FnOnce(&windows_sys::Win32::Security::Cryptography::CERT_CONTEXT) -> bool,
) -> Result<(), u8> {
    use std::{
        mem::size_of,
        os::windows::{ffi::OsStrExt, io::AsRawHandle},
    };
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Security::WinTrust::{
            WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
            WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_DISABLE_MD2_MD4,
            WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT, WTD_REVOKE_WHOLECHAIN, WTD_STATEACTION_CLOSE,
            WTD_STATEACTION_VERIFY, WTD_UI_NONE, WinVerifyTrust,
        },
    };

    let name = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: name.as_ptr(),
        hFile: file_handle.as_raw_handle(),
        ..Default::default()
    };
    let mut policy = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let mut trust = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL
            | WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT
            | WTD_DISABLE_MD2_MD4,
        ..Default::default()
    };
    // The file handle and UTF-16 path remain live through both calls. A
    // successful request owns provider pointers until the matching close.
    unsafe {
        let status = WinVerifyTrust(INVALID_HANDLE_VALUE, &mut policy, (&raw mut trust).cast());
        let result = if status != 0 {
            Err(1)
        } else if signer_leaf(trust.hWVTStateData)
            .as_ref()
            .is_some_and(accepts)
        {
            Ok(())
        } else {
            Err(2)
        };
        trust.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(INVALID_HANDLE_VALUE, &mut policy, (&raw mut trust).cast());
        result
    }
}

#[cfg(windows)]
unsafe fn signer_leaf(
    state: windows_sys::Win32::Foundation::HANDLE,
) -> *const windows_sys::Win32::Security::Cryptography::CERT_CONTEXT {
    use windows_sys::Win32::Security::WinTrust::{
        WTHelperGetProvCertFromChain, WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData,
    };

    // Index zero is the actual verified signer and its leaf. Do not use a
    // timestamp countersigner or select some other certificate in the chain.
    unsafe {
        let provider = WTHelperProvDataFromStateData(state);
        if provider.is_null() {
            return std::ptr::null();
        }
        let signer = WTHelperGetProvSignerFromChain(provider, 0, 0, 0);
        if signer.is_null() {
            return std::ptr::null();
        }
        WTHelperGetProvCertFromChain(signer, 0)
            .as_ref()
            .map_or(std::ptr::null(), |leaf| leaf.pCert)
    }
}

unsafe fn publisher_sha256(
    certificate: &windows_sys::Win32::Security::Cryptography::CERT_CONTEXT,
) -> Option<[u8; 32]> {
    use windows_sys::{Win32::Security::Cryptography::CryptHashCertificate2, core::w};
    unsafe {
        let mut bytes = [0u8; 32];
        let mut length = bytes.len() as u32;
        let status = CryptHashCertificate2(
            w!("SHA256"),
            0,
            std::ptr::null(),
            certificate.pbCertEncoded,
            certificate.cbCertEncoded,
            bytes.as_mut_ptr(),
            &mut length,
        );
        (status != 0 && length == bytes.len() as u32).then_some(bytes)
    }
}
