use super::R;
use std::{
    fs::File,
    io::{Read, Write},
    mem::{size_of, size_of_val},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::Path,
    ptr,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, WAIT_OBJECT_0},
    System::{
        Console::{
            COORD, ClosePseudoConsole, CreatePseudoConsole, GetStdHandle, HPCON, STD_ERROR_HANDLE,
            STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
        },
        Pipes::CreatePipe,
        Threading::*,
    },
};

fn pipe() -> R<(File, File)> {
    let (mut r, mut w) = (ptr::null_mut(), ptr::null_mut());
    if unsafe { CreatePipe(&mut r, &mut w, ptr::null(), 0) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { (File::from_raw_handle(r), File::from_raw_handle(w)) })
}
struct Console(HPCON);
impl Drop for Console {
    fn drop(&mut self) {
        unsafe { ClosePseudoConsole(self.0) }
    }
}
struct Child(PROCESS_INFORMATION);
impl Drop for Child {
    fn drop(&mut self) {
        unsafe {
            if WaitForSingleObject(self.0.hProcess, 0) != WAIT_OBJECT_0 {
                // This handle belongs only to the exact child launched by this harness.
                TerminateProcess(self.0.hProcess, 120);
                WaitForSingleObject(self.0.hProcess, 3000);
            }
            CloseHandle(self.0.hThread);
            CloseHandle(self.0.hProcess);
        }
    }
}

fn launch(exe: &Path, arg: &str, pty: HPCON) -> R<Child> {
    let path = exe.as_os_str().encode_wide().chain([0]).collect::<Vec<_>>();
    let mut cmd = format!("\"{}\" {arg}", exe.display())
        .encode_utf16()
        .chain([0])
        .collect::<Vec<_>>();
    let mut size = 0;
    unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut size) };
    if size == 0 {
        return Err("failed to size the console startup attributes".into());
    }
    let mut attrs = vec![0usize; size.div_ceil(size_of::<usize>())];
    let list = attrs.as_mut_ptr().cast();
    if unsafe { InitializeProcThreadAttributeList(list, 1, 0, &mut size) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let result = (|| {
        if unsafe {
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                pty as *const _,
                size_of_val(&pty),
                ptr::null_mut(),
                ptr::null(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.lpAttributeList = list;
        let mut pi = PROCESS_INFORMATION::default();
        // A caller may have captured this host's standard streams. Leave those
        // handles with the host while Windows establishes the child's console.
        let handles = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE]
            .map(|id| unsafe { (id, GetStdHandle(id)) });
        for (id, _) in handles {
            unsafe {
                SetStdHandle(id, ptr::null_mut());
            }
        }
        let ok = unsafe {
            CreateProcessW(
                path.as_ptr(),
                cmd.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT,
                ptr::null(),
                ptr::null(),
                &startup.StartupInfo,
                &mut pi,
            )
        };
        let error = std::io::Error::last_os_error();
        for (id, handle) in handles {
            unsafe {
                SetStdHandle(id, handle);
            }
        }
        if ok == 0 {
            return Err(error.into());
        }
        Ok(Child(pi))
    })();
    unsafe { DeleteProcThreadAttributeList(list) };
    result
}

// Match complete product sentences even when the terminal inserts SGR, cursor, or title sequences.
fn visible(raw: &[u8]) -> String {
    let mut state = 0u8;
    let mut out = String::new();
    for &b in raw {
        match (state, b) {
            (0, 27) => state = 1,
            (0, 32..=126) => out.push(b as char),
            (0, 10 | 13) => out.push(' '),
            (1, b'[') => state = 2,
            (1, b']') => state = 3,
            (1, _) | (2, 64..=126) | (3, 7) | (4, b'\\') => state = 0,
            (3, 27) => state = 4,
            (4, _) => state = 3,
            _ => (),
        }
    }
    out.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn invoke(
    exe: &Path,
    arg: &str,
    required: &[&str],
    answer: &str,
    final_text: &str,
    guard: impl Fn() -> R<()>,
) -> R<()> {
    let (input_read, mut input_write) = pipe()?;
    let (mut output_read, output_write) = pipe()?;
    let mut pty = 0;
    if unsafe {
        CreatePseudoConsole(
            COORD { X: 480, Y: 36 },
            input_read.as_raw_handle(),
            output_write.as_raw_handle(),
            0,
            &mut pty,
        )
    } < 0
    {
        return Err("Windows could not create the fixture pseudoconsole".into());
    }
    let console = Console(pty);
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = [0u8; 2048];
        while let Ok(n) = output_read.read(&mut buf) {
            if n == 0 {
                break;
            }
            if tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let child = launch(exe, arg, console.0)?;
    drop((input_read, output_write));
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut transcript = Vec::new();
    let mut sent = false;
    loop {
        if Instant::now() >= deadline {
            return Err("timed out waiting for the exact preview, prompt or completion".into());
        }
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(50)) {
            transcript.extend_from_slice(&chunk);
        }
        if transcript.len() > 32 * 1024 {
            return Err("unexpected volume of console output".into());
        }
        let rendered = visible(&transcript);
        if !sent && required.iter().all(|s| rendered.contains(s)) {
            guard()?;
            input_write.write_all(answer.as_bytes())?;
            input_write.write_all(b"\r")?;
            input_write.flush()?;
            sent = true;
        }
        if unsafe { WaitForSingleObject(child.0.hProcess, 0) } == WAIT_OBJECT_0 {
            let mut exit = 1;
            if unsafe { GetExitCodeProcess(child.0.hProcess, &mut exit) } == 0 || exit != 0 {
                return Err("the console client exited unsuccessfully".into());
            }
            if sent && rendered.contains(final_text) {
                return Ok(());
            }
        }
    }
}
