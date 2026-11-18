#[cfg(windows)]
fn main() -> std::process::ExitCode {
    use std::{env, path::Path, process::ExitCode};

    let mut arguments = env::args_os().skip(1);
    let (Some(file), Some(pin), None) = (arguments.next(), arguments.next(), arguments.next())
    else {
        return ExitCode::from(3);
    };
    let Some(pin) = pin.into_string().ok().and_then(|pin| parse_pin(&pin)) else {
        return ExitCode::from(3);
    };
    match verify(Path::new(&file), pin) {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(3)
}

#[cfg(windows)]
fn parse_pin(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut pin = [0u8; 32];
    for (index, byte) in pin.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(pin)
}

#[cfg(windows)]
fn verify(path: &std::path::Path, pin: [u8; 32]) -> Result<(), u8> {
    use std::{
        fs::OpenOptions,
        mem::size_of,
        os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
    };
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Security::WinTrust::{
            WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
            WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_DISABLE_MD2_MD4,
            WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT, WTD_REVOKE_WHOLECHAIN, WTD_STATEACTION_CLOSE,
            WTD_STATEACTION_VERIFY, WTD_UI_NONE, WinVerifyTrust,
        },
        Storage::FileSystem::FILE_SHARE_READ,
    };

    let file_handle = OpenOptions::new()
        .share_mode(FILE_SHARE_READ)
        .read(true)
        .open(path)
        .map_err(|_| 3u8)?;
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
        } else if publisher_sha256(trust.hWVTStateData) == Some(pin) {
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
unsafe fn publisher_sha256(state: windows_sys::Win32::Foundation::HANDLE) -> Option<[u8; 32]> {
    use windows_sys::{
        Win32::Security::{
            Cryptography::CryptHashCertificate2,
            WinTrust::{
                WTHelperGetProvCertFromChain, WTHelperGetProvSignerFromChain,
                WTHelperProvDataFromStateData,
            },
        },
        core::w,
    };

    // Index zero is the actual verified signer and its leaf. Do not use a
    // timestamp countersigner or select some other certificate in the chain.
    unsafe {
        let provider = WTHelperProvDataFromStateData(state);
        if provider.is_null() {
            return None;
        }
        let signer = WTHelperGetProvSignerFromChain(provider, 0, 0, 0);
        if signer.is_null() {
            return None;
        }
        let leaf = WTHelperGetProvCertFromChain(signer, 0).as_ref()?;
        let certificate = leaf.pCert.as_ref()?;
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
