use std::{mem::size_of, path::Path};
use threadline_installer::{LaunchEnvironment, Platform};
use threadline_platform::{NativePaths, PathResolver};
use windows::Win32::{
    Foundation::HMODULE,
    Graphics::{
        Direct3D::{
            D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_10_1,
            D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
        },
        Direct3D11::{D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice},
    },
};
use windows_sys::{
    Wdk::System::SystemServices::RtlGetVersion,
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
        System::{
            Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_SUBKEY_WOW6464KEY, RegGetValueW},
            StationsAndDesktops::{CloseDesktop, DESKTOP_READOBJECTS, OpenInputDesktop},
            SystemInformation::{IMAGE_FILE_MACHINE_AMD64, OSVERSIONINFOEXW, OSVERSIONINFOW},
            Threading::{GetCurrentProcess, IsWow64Process2},
        },
    },
};

pub(super) fn detect() -> Result<Platform, &'static str> {
    let mut arch = 0;
    let mut native = 0;
    let mut version = OSVERSIONINFOEXW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOEXW>() as u32,
        ..Default::default()
    };
    // Each Windows call writes into local storage of its documented size.
    if unsafe { IsWow64Process2(GetCurrentProcess(), &mut arch, &mut native) } == 0
        || unsafe { RtlGetVersion((&raw mut version).cast::<OSVERSIONINFOW>()) } != 0
    {
        return Err("Windows could not report its native platform and build.");
    }
    let paths = NativePaths
        .application_paths()
        .map_err(|_| "Windows could not locate the application data directory.")?;
    let profile_schema = schema_of(&paths.data.join("threadline.sqlite3"))?;
    Ok(Platform {
        native_x64: native == IMAGE_FILE_MACHINE_AMD64,
        workstation: version.wProductType == 1,
        build: version.dwBuildNumber,
        highest_build: highest_build()?,
        profile_schema,
    })
}

pub(super) fn require_launch_ready() -> Result<(), &'static str> {
    let desk = unsafe { OpenInputDesktop(0, 0, DESKTOP_READOBJECTS) };
    let desktop = !desk.is_null();
    if desktop {
        unsafe { CloseDesktop(desk) };
    }
    let graphics = [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP]
        .into_iter()
        .any(|driver| {
            let mut level = Default::default();
            unsafe {
                D3D11CreateDevice(
                    None,
                    driver,
                    HMODULE::default(),
                    D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                    Some(&[
                        D3D_FEATURE_LEVEL_11_1,
                        D3D_FEATURE_LEVEL_11_0,
                        D3D_FEATURE_LEVEL_10_1,
                    ]),
                    D3D11_SDK_VERSION,
                    None,
                    Some(&mut level),
                    None,
                )
            }
            .is_ok()
        });
    LaunchEnvironment { graphics, desktop }.check()
}
fn schema_of(path: &Path) -> Result<Option<u32>, &'static str> {
    match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(
                "Cannot read the existing data directory. Close Threadline and check its permissions.",
            );
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err("The existing database path is not a file.");
        }
        _ => (),
    }
    let db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| "Could not open the existing profile read-only.")?;
    let value: u32 = db
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| "Could not verify the existing database schema.")?;
    Ok(Some(value))
}
fn highest_build() -> Result<u32, &'static str> {
    let mut value = 0u32;
    let mut bytes = 4u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            windows_sys::core::w!("Software\\Threadline"),
            windows_sys::core::w!("HighestBuild"),
            RRF_RT_REG_DWORD | RRF_SUBKEY_WOW6464KEY,
            std::ptr::null_mut(),
            (&raw mut value).cast(),
            &mut bytes,
        )
    };
    match result {
        ERROR_SUCCESS if bytes == 4 => Ok(value),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(0),
        _ => Err("Could not read the retained Windows package version."),
    }
}
