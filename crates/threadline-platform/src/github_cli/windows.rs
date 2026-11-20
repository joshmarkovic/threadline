use super::{CliError, GitHubCli, LOGIN_ARGS};
use std::{
    ffi::{OsStr, OsString},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::PathBuf,
    ptr,
    time::Duration,
};
use windows::Win32::{
    System::Com::CoTaskMemFree,
    UI::Shell::{
        FOLDERID_LocalAppData, FOLDERID_Profile, FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86,
        FOLDERID_RoamingAppData, FOLDERID_System, FOLDERID_Windows, KF_FLAG_DEFAULT,
        SHGetKnownFolderPath,
    },
};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        CREATE_NEW_CONSOLE, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, GetExitCodeProcess,
        PROCESS_INFORMATION, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
    },
};

fn folder(id: &windows::core::GUID) -> Option<PathBuf> {
    let path = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.ok()?;
    if path.is_null() {
        return None;
    }
    let result = PathBuf::from(OsString::from_wide(unsafe { path.as_wide() }));
    unsafe {
        CoTaskMemFree(Some(path.0.cast()));
    }
    result.is_absolute().then_some(result)
}

pub(super) fn candidates() -> Vec<PathBuf> {
    let mut candidates: Vec<_> = [FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86]
        .iter()
        .filter_map(folder)
        .map(|p| p.join(r"GitHub CLI\gh.exe"))
        .collect();
    if let Some(local) = folder(&FOLDERID_LocalAppData) {
        candidates.push(local.join(r"Microsoft\WinGet\Packages\GitHub.cli_Microsoft.Winget.Source_8wekyb3d8bbwe\bin\gh.exe"));
        candidates.push(local.join(r"Programs\GitHub CLI\gh.exe"));
    }
    candidates
}

pub(super) fn environment() -> Result<Vec<(OsString, OsString)>, CliError> {
    let required = |id| folder(id).ok_or(CliError::Unavailable);
    let local = required(&FOLDERID_LocalAppData)?;
    let profile = required(&FOLDERID_Profile)?;
    let system = required(&FOLDERID_System)?;
    let windows = required(&FOLDERID_Windows)?;
    let mut search = vec![system.clone(), windows.clone()];
    if let Some(programs) = folder(&FOLDERID_ProgramFiles) {
        search.push(programs.join(r"Git\cmd"));
    }
    let path = std::env::join_paths(search).map_err(|_| CliError::Unavailable)?;
    let temp = local.join("Temp");
    Ok(vec![
        ("APPDATA".into(), required(&FOLDERID_RoamingAppData)?.into()),
        ("COMSPEC".into(), system.join("cmd.exe").into()),
        ("GH_NO_EXTENSION_UPDATE_NOTIFIER".into(), "1".into()),
        ("GH_NO_UPDATE_NOTIFIER".into(), "1".into()),
        ("HOME".into(), profile.as_os_str().into()),
        ("LOCALAPPDATA".into(), local.into()),
        ("PATH".into(), path),
        ("SYSTEMROOT".into(), windows.as_os_str().into()),
        ("TEMP".into(), temp.as_os_str().into()),
        ("TMP".into(), temp.into()),
        ("USERPROFILE".into(), profile.into()),
        ("WINDIR".into(), windows.into()),
    ])
}

pub(super) async fn login(cli: &GitHubCli) -> Result<(), CliError> {
    console(cli, LOGIN_ARGS).await
}

/// Use a fresh console with OS-created standard handles, even when Threadline itself runs without a console.
pub(super) async fn console(cli: &GitHubCli, args: &[&str]) -> Result<(), CliError> {
    // The only arguments are internal literals. No shell, path quoting, or user-provided arguments are interpreted.
    debug_assert!(args.iter().all(|a| !a.contains([' ', '"', '\0'])));
    let mut line = format!("gh.exe {}", args.join(" "))
        .encode_utf16()
        .chain([0])
        .collect::<Vec<_>>();
    let wide = |s: &OsStr| s.encode_wide().chain([0]).collect::<Vec<_>>();
    let path = wide(cli.executable.as_os_str());
    let cwd = wide(
        cli.executable
            .parent()
            .ok_or(CliError::Unavailable)?
            .as_os_str(),
    );
    // CreateProcess expects case-insensitively sorted, NUL-separated KEY=VALUE entries and a final NUL.
    let mut environment = Vec::new();
    for (key, value) in &cli.environment {
        environment.extend(key.encode_wide());
        environment.push(u16::from(b'='));
        environment.extend(value.encode_wide());
        environment.push(0);
    }
    environment.push(0);
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    let ok = unsafe {
        CreateProcessW(
            path.as_ptr(),
            line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            0,
            CREATE_NEW_CONSOLE | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr().cast(),
            cwd.as_ptr(),
            &startup,
            &mut info,
        )
    };
    if ok == 0 {
        return Err(CliError::Unavailable);
    }
    // These handles belong to the caller on success. Drop the thread handle immediately.
    drop(unsafe { OwnedHandle::from_raw_handle(info.hThread) });
    let child = ConsoleProcess(unsafe { OwnedHandle::from_raw_handle(info.hProcess) });
    loop {
        match unsafe { WaitForSingleObject(child.0.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => {
                let mut exit = 0;
                if unsafe { GetExitCodeProcess(child.0.as_raw_handle(), &mut exit) } == 0 {
                    return Err(CliError::Unavailable);
                }
                return if exit == 0 {
                    Ok(())
                } else {
                    Err(CliError::LoginFailed)
                };
            }
            WAIT_TIMEOUT => tokio::time::sleep(Duration::from_millis(100)).await,
            _ => return Err(CliError::Unavailable),
        }
    }
}

struct ConsoleProcess(OwnedHandle);
impl Drop for ConsoleProcess {
    fn drop(&mut self) {
        let handle = self.0.as_raw_handle();
        if unsafe { WaitForSingleObject(handle, 0) } == WAIT_TIMEOUT {
            unsafe {
                TerminateProcess(handle, 1);
            }
        }
    }
}
