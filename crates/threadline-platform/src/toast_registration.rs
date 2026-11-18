use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PropVariantClear, PropVariantToGUID, PropVariantToStringAlloc,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize, IPersistFile, STGM_READ,
};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    FOLDERID_CommonPrograms, FOLDERID_Programs, IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
    ShellLink,
};
use windows::core::{GUID, Interface, PCWSTR, PWSTR};
use windows_sys::Win32::System::Registry::{HKEY, RRF_RT_REG_SZ, RegGetValueW};

const APP_PROPERTIES: GUID = GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3);

pub(super) fn registry_matches(root: HKEY, path: &str, name: Option<&str>, expected: &str) -> bool {
    let path: Vec<u16> = path.encode_utf16().chain([0]).collect();
    let name = name.map(|name| name.encode_utf16().chain([0]).collect::<Vec<_>>());
    let mut value = vec![0u16; expected.encode_utf16().count() + 1];
    let Ok(mut bytes) = u32::try_from(value.len().saturating_mul(2)) else {
        return false;
    };
    // Read only the named registration and its directly owned value. Windows receives the exact buffer size.
    let status = unsafe {
        RegGetValueW(
            root,
            path.as_ptr(),
            name.as_ref().map_or(null(), |name| name.as_ptr()),
            RRF_RT_REG_SZ,
            null_mut(),
            value.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    status == 0
        && String::from_utf16_lossy(&value)
            .trim_end_matches('\0')
            .eq_ignore_ascii_case(expected)
}

struct ComScope;

impl Drop for ComScope {
    fn drop(&mut self) {
        // Paired with this thread's successful CoInitializeEx.
        unsafe { CoUninitialize() };
    }
}

struct ShellString(PWSTR);

impl Drop for ShellString {
    fn drop(&mut self) {
        // Both known-folder and property string APIs allocate these strings via COM.
        unsafe { CoTaskMemFree(Some(self.0.0.cast())) };
    }
}

struct Variant(PROPVARIANT);

impl Drop for Variant {
    fn drop(&mut self) {
        // IPropertyStore populated this variant and transfers ownership to the caller.
        let _ = unsafe { PropVariantClear(&mut self.0) };
    }
}

pub(super) fn installed_link_matches(executable: &Path, app_id: &str, class: GUID) -> bool {
    // Use the shell's per-user and all-users Programs directories, including redirected folders.
    if unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_err() {
        return false;
    }
    let _com = ComScope;
    [FOLDERID_Programs, FOLDERID_CommonPrograms]
        .iter()
        .any(|folder| {
            let Ok(folder) =
                (unsafe { SHGetKnownFolderPath(folder, KF_FLAG_DEFAULT, None) }).map(ShellString)
            else {
                return false;
            };
            if folder.0.is_null() {
                return false;
            }
            let path = PathBuf::from(OsString::from_wide(unsafe { folder.0.as_wide() }))
                .join("Threadline.lnk");
            path.is_file() && load_link(&path, executable, app_id, class).unwrap_or(false)
        })
}

fn load_link(
    path: &Path,
    executable: &Path,
    app_id: &str,
    class: GUID,
) -> windows::core::Result<bool> {
    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut target = vec![0u16; 32768];
    // Load but do not resolve, execute, or modify the shortcut. These buffers live through each call.
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)? };
    unsafe {
        link.cast::<IPersistFile>()?
            .Load(PCWSTR(path.as_ptr()), STGM_READ)?;
        link.GetPath(&mut target, null_mut(), 0)?;
    }
    let target = &target[..target
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(target.len())];
    let target = PathBuf::from(OsString::from_wide(target));
    if target
        .canonicalize()
        .ok()
        .zip(executable.canonicalize().ok())
        .is_none_or(|(target, exe)| target != exe)
    {
        return Ok(false);
    }

    let properties = link.cast::<IPropertyStore>()?;
    let identity = Variant(unsafe {
        properties.GetValue(&PROPERTYKEY {
            fmtid: APP_PROPERTIES,
            pid: 5,
        })?
    });
    let identity = ShellString(unsafe { PropVariantToStringAlloc(&identity.0)? });
    let activator = Variant(unsafe {
        properties.GetValue(&PROPERTYKEY {
            fmtid: APP_PROPERTIES,
            pid: 26,
        })?
    });
    Ok(!identity.0.is_null()
        && unsafe { identity.0.to_string()? } == app_id
        && unsafe { PropVariantToGUID(&activator.0)? } == class)
}
