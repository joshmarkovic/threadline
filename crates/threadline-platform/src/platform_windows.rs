use std::cell::Cell;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedSender;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, SetLastError,
    WPARAM,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIIF_RESPECT_QUIET_TIME,
    NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETFOCUS, NIM_SETVERSION, NIN_BALLOONHIDE,
    NIN_BALLOONSHOW, NIN_BALLOONTIMEOUT, NIN_BALLOONUSERCLICK, NIN_SELECT, NINF_KEY,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use windows_sys::w;

use super::{Activation, PlatformError, PlatformEvent};

const COMMAND_READY: u32 = WM_APP + 1;
const TRAY_CALLBACK: u32 = WM_APP + 2;
const BALLOON_TIMER: usize = 1;
const BALLOON_TIMEOUT_MS: u32 = 60_000;
const ICON_ID: u32 = 1;
const MENU_OPEN: usize = 1;
const MENU_QUIT: usize = 2;
const KEY_SELECT: u32 = NIN_SELECT | NINF_KEY;

#[derive(Clone)]
pub struct Desktop(Arc<DesktopInner>);

struct DesktopInner {
    window: usize,
    commands: Sender<Command>,
}

enum Command {
    EnableTray(bool),
    NotifyReview,
    Shutdown,
}

impl Desktop {
    pub fn set_tray_enabled(&self, enabled: bool) -> Result<(), PlatformError> {
        self.post(Command::EnableTray(enabled))
    }

    pub fn show_notification(&self) -> Result<(), PlatformError> {
        self.post(Command::NotifyReview)
    }

    fn post(&self, command: Command) -> Result<(), PlatformError> {
        self.0
            .commands
            .send(command)
            .map_err(|_| PlatformError::Unavailable)?;
        // The worker owns this window and receives only a wake-up signal. Actual commands stay in-process.
        if unsafe { PostMessageW(self.0.window as HWND, COMMAND_READY, 0, 0) } == 0 {
            return Err(last_error("Queue desktop action"));
        }
        Ok(())
    }
}

impl Drop for DesktopInner {
    fn drop(&mut self) {
        if self.commands.send(Command::Shutdown).is_ok() {
            // Give the worker a bounded chance to remove the shell icon before process shutdown.
            unsafe {
                SendMessageTimeoutW(
                    self.window as HWND,
                    COMMAND_READY,
                    0,
                    0,
                    SMTO_ABORTIFHUNG,
                    500,
                    null_mut(),
                );
            }
        }
    }
}

struct InstanceMutex(usize);

impl Drop for InstanceMutex {
    fn drop(&mut self) {
        // The mutex handle is opened by CreateMutexW and owned until the worker stops.
        unsafe { CloseHandle(self.0 as HANDLE) };
    }
}

struct InstanceNames {
    mutex: Vec<u16>,
    class: Vec<u16>,
    activation_message: Vec<u16>,
}

impl InstanceNames {
    fn new(profile: Option<&Path>) -> Self {
        let scope = match profile {
            Some(profile) => {
                let mut hasher = DefaultHasher::new();
                profile.hash(&mut hasher);
                format!("Threadline.Desktop.Test.{:016x}", hasher.finish())
            }
            None => "Threadline.Desktop.Primary.v1".to_owned(),
        };
        Self {
            mutex: wide(&format!("Local\\{scope}")),
            activation_message: wide(&format!("{scope}.Activate")),
            class: wide(&scope),
        }
    }
}

pub fn start(
    activation: Activation,
    profile: Option<&Path>,
    events: UnboundedSender<PlatformEvent>,
) -> Result<Option<Desktop>, PlatformError> {
    let names = InstanceNames::new(profile);
    // Use the session-local named object for lifetime detection without acquiring mutex ownership.
    let (handle, status) = unsafe {
        SetLastError(0);
        let handle = CreateMutexW(null(), 0, names.mutex.as_ptr());
        (handle, GetLastError())
    };
    if handle.is_null() {
        return Err(PlatformError::Native {
            operation: "Open desktop instance",
            code: status,
        });
    }
    let instance = InstanceMutex(handle as usize);
    // Both processes register the same name. Only a fixed activation code crosses the process boundary.
    let activation_message = unsafe { RegisterWindowMessageW(names.activation_message.as_ptr()) };
    if activation_message == 0 {
        return Err(last_error("Register desktop activation"));
    }
    if status == ERROR_ALREADY_EXISTS {
        forward_activation(&names, activation_message, activation)?;
        return Ok(None);
    }

    let (sender, commands) = mpsc::channel();
    let (started, ready) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("threadline-windows-desktop".into())
        .spawn(move || {
            let _instance = instance;
            run_desktop(names, activation_message, events, commands, started);
        })
        .map_err(|_| PlatformError::Unavailable)?;
    let window = ready.recv().map_err(|_| PlatformError::Unavailable)??;
    Ok(Some(Desktop(Arc::new(DesktopInner {
        window,
        commands: sender,
    }))))
}

fn forward_activation(
    names: &InstanceNames,
    message: u32,
    activation: Activation,
) -> Result<(), PlatformError> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let window = loop {
        // The profile-specific class identifies the hidden top-level window without a caption.
        let window = unsafe { FindWindowW(names.class.as_ptr(), null()) };
        if !window.is_null() {
            break window;
        }
        if Instant::now() >= deadline {
            return Err(PlatformError::InstanceWindowNotFound);
        }
        thread::sleep(Duration::from_millis(25));
    };
    let mut acknowledged = 0;
    // Allow the primary process to raise its own app window if our process has foreground permission.
    let sent = unsafe {
        let mut process = 0;
        GetWindowThreadProcessId(window, &mut process);
        if process != 0 {
            AllowSetForegroundWindow(process);
        }
        SetLastError(0);
        SendMessageTimeoutW(
            window,
            message,
            encode_activation(activation),
            0,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            1_500,
            &mut acknowledged,
        )
    };
    if sent == 0 {
        return Err(last_error("Forward desktop activation"));
    }
    if acknowledged != 1 {
        return Err(PlatformError::ActivationRejected);
    }
    Ok(())
}

fn run_desktop(
    names: InstanceNames,
    activation_message: u32,
    events: UnboundedSender<PlatformEvent>,
    commands: Receiver<Command>,
    started: SyncSender<Result<usize, PlatformError>>,
) {
    // This worker owns every HWND operation except posting bounded messages to its queue.
    let module = unsafe { GetModuleHandleW(null()) };
    let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    let state = WindowState {
        activation_message,
        taskbar_created,
        events,
        commands,
        tray_enabled: Cell::new(false),
        icon_present: Cell::new(false),
        balloon_pending: Cell::new(false),
        balloon_shown: Cell::new(false),
    };
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: module,
        lpszClassName: names.class.as_ptr(),
        ..Default::default()
    };
    // `state` remains at a fixed address until the window and its message pump are torn down.
    let registered = unsafe { RegisterClassW(&class) };
    if registered == 0 {
        let _ = started.send(Err(last_error("Register the desktop worker window")));
        return;
    }
    let window = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            names.class.as_ptr(),
            null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            module,
            (&raw const state).cast(),
        )
    };
    if window.is_null() {
        let _ = started.send(Err(last_error("Create the desktop worker window")));
    } else if started.send(Ok(window as usize)).is_ok() {
        let mut message = MSG::default();
        // Win32 writes MSG, then dispatches the message back to this same thread.
        while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    // Normally WM_DESTROY has run. Clean up on the start/report or message-pump error paths as well.
    unsafe {
        if !window.is_null() && IsWindow(window) != 0 {
            DestroyWindow(window);
        }
        UnregisterClassW(names.class.as_ptr(), module);
    }
}

struct WindowState {
    activation_message: u32,
    taskbar_created: u32,
    events: UnboundedSender<PlatformEvent>,
    commands: Receiver<Command>,
    tray_enabled: Cell<bool>,
    icon_present: Cell<bool>,
    balloon_pending: Cell<bool>,
    balloon_shown: Cell<bool>,
}

impl WindowState {
    fn emit(&self, event: PlatformEvent) -> bool {
        self.events.send(event).is_ok()
    }

    fn process_commands(&self, hwnd: HWND) {
        for command in self.commands.try_iter() {
            let result = match command {
                Command::EnableTray(enabled) => self.set_tray(hwnd, enabled),
                Command::NotifyReview => self.show_balloon(hwnd),
                Command::Shutdown => {
                    // The worker owns this window and WM_DESTROY performs icon cleanup.
                    unsafe { DestroyWindow(hwnd) };
                    break;
                }
            };
            if let Err(error) = result {
                self.emit(PlatformEvent::Error(error));
            }
        }
    }

    fn set_tray(&self, hwnd: HWND, enabled: bool) -> Result<(), PlatformError> {
        if enabled {
            self.add_icon(hwnd)?;
        }
        self.tray_enabled.set(enabled);
        if !enabled && !self.balloon_pending.get() {
            self.remove_icon(hwnd);
        }
        self.emit(PlatformEvent::TrayChanged(enabled));
        Ok(())
    }

    fn add_icon(&self, hwnd: HWND) -> Result<(), PlatformError> {
        if self.icon_present.get() {
            return Ok(());
        }
        let mut icon = icon_data(hwnd);
        icon.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        icon.uCallbackMessage = TRAY_CALLBACK;
        set_label(&mut icon.szTip, "Threadline");
        // A stock icon is shared and does not require DestroyIcon.
        icon.hIcon = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
        if icon.hIcon.is_null() || !notify_icon(NIM_ADD, &icon) {
            return Err(PlatformError::Unavailable);
        }
        self.icon_present.set(true);
        icon.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        if !notify_icon(NIM_SETVERSION, &icon) {
            self.remove_icon(hwnd);
            return Err(PlatformError::Unavailable);
        }
        Ok(())
    }

    fn remove_icon(&self, hwnd: HWND) {
        if self.icon_present.replace(false) {
            notify_icon(NIM_DELETE, &icon_data(hwnd));
        }
    }

    fn show_balloon(&self, hwnd: HWND) -> Result<(), PlatformError> {
        if self.balloon_pending.get() {
            return Err(PlatformError::Unavailable);
        }
        self.add_icon(hwnd)?;
        let mut icon = icon_data(hwnd);
        icon.uFlags = NIF_INFO;
        icon.dwInfoFlags = NIIF_INFO | NIIF_RESPECT_QUIET_TIME;
        set_label(&mut icon.szInfoTitle, "Threadline review");
        set_label(
            &mut icon.szInfo,
            "Open the sample pull request in Threadline.",
        );
        self.balloon_pending.set(true);
        self.balloon_shown.set(false);
        // Bound the lifetime of the temporary icon if Windows suppresses the balloon without a callback.
        let timer = unsafe { SetTimer(hwnd, BALLOON_TIMER, BALLOON_TIMEOUT_MS, None) };
        if timer == 0 || !notify_icon(NIM_MODIFY, &icon) {
            self.finish_balloon(hwnd);
            return Err(PlatformError::Unavailable);
        }
        Ok(())
    }

    fn finish_balloon(&self, hwnd: HWND) {
        self.balloon_pending.set(false);
        // Only the message window can own this timer.
        unsafe { KillTimer(hwnd, BALLOON_TIMER) };
        if !self.tray_enabled.get() {
            self.remove_icon(hwnd);
        }
    }

    fn shell_restarted(&self, hwnd: HWND) {
        self.icon_present.set(false);
        self.finish_balloon(hwnd);
        if self.tray_enabled.get() && self.add_icon(hwnd).is_err() {
            self.tray_enabled.set(false);
            self.emit(PlatformEvent::TrayChanged(false));
            self.emit(PlatformEvent::Error(PlatformError::Unavailable));
        }
    }

    fn tray_callback(&self, hwnd: HWND, coordinates: WPARAM, value: LPARAM) {
        if ((value as u32) >> 16) != ICON_ID {
            return;
        }
        match value as u32 & 0xffff {
            NIN_BALLOONSHOW if self.balloon_pending.get() => {
                self.balloon_shown.set(true);
                self.emit(PlatformEvent::NotificationShown);
            }
            NIN_BALLOONUSERCLICK if self.balloon_pending.get() => {
                self.emit(PlatformEvent::Activate(Activation::OpenReview));
                self.finish_balloon(hwnd);
            }
            NIN_BALLOONHIDE | NIN_BALLOONTIMEOUT if self.balloon_pending.get() => {
                if !self.balloon_shown.get() {
                    self.emit(PlatformEvent::Error(PlatformError::Unavailable));
                }
                self.finish_balloon(hwnd);
            }
            NIN_SELECT | KEY_SELECT => {
                self.emit(PlatformEvent::Activate(Activation::Focus));
            }
            WM_CONTEXTMENU if self.tray_enabled.get() => {
                let x = (coordinates as u16) as i16 as i32;
                let y = ((coordinates >> 16) as u16) as i16 as i32;
                self.show_menu(hwnd, x, y);
            }
            _ => {}
        }
    }

    fn show_menu(&self, hwnd: HWND, x: i32, y: i32) {
        // This stack scope owns the popup menu. The system runs it on the worker's message thread.
        let selected = unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return;
            }
            AppendMenuW(menu, MF_STRING, MENU_OPEN, w!("Open Threadline"));
            AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("Quit Threadline"));
            SetForegroundWindow(hwnd);
            let selected = TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
                x,
                y,
                0,
                hwnd,
                null(),
            );
            PostMessageW(hwnd, WM_NULL, 0, 0);
            DestroyMenu(menu);
            selected as usize
        };
        match selected {
            MENU_OPEN => {
                self.emit(PlatformEvent::Activate(Activation::Focus));
            }
            MENU_QUIT => {
                self.emit(PlatformEvent::Quit);
            }
            _ => {
                notify_icon(NIM_SETFOCUS, &icon_data(hwnd));
            }
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        // Win32 passes the live CREATESTRUCTW from CreateWindowExW with our worker-owned state pointer.
        let creation = unsafe { &*(lparam as *const CREATESTRUCTW) };
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, creation.lpCreateParams as isize) };
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    // The state is thread-confined and remains valid until DestroyWindow finishes.
    let state = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowState).as_ref() };
    let Some(state) = state else {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    };
    if message == state.activation_message {
        return decode_activation(wparam)
            .is_some_and(|activation| state.emit(PlatformEvent::Activate(activation)))
            as LRESULT;
    }
    if message != 0 && message == state.taskbar_created {
        state.shell_restarted(hwnd);
        return 0;
    }
    match message {
        COMMAND_READY => state.process_commands(hwnd),
        TRAY_CALLBACK => state.tray_callback(hwnd, wparam, lparam),
        WM_TIMER if wparam == BALLOON_TIMER => {
            if state.balloon_pending.get() && !state.balloon_shown.get() {
                state.emit(PlatformEvent::Error(PlatformError::Unavailable));
            }
            state.finish_balloon(hwnd);
        }
        WM_DESTROY => {
            state.remove_icon(hwnd);
            // The receiver has terminated; wake the owning thread to release its resources.
            unsafe { PostQuitMessage(0) };
        }
        WM_NCDESTROY => {
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
        }
        _ => return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
    0
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn set_label(buffer: &mut [u16], text: &str) {
    for (slot, code_unit) in buffer.iter_mut().zip(text.encode_utf16().chain([0])) {
        *slot = code_unit;
    }
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: ICON_ID,
        ..Default::default()
    }
}

fn notify_icon(message: u32, icon: &NOTIFYICONDATAW) -> bool {
    // The shell copies the initialized icon structure during this call.
    unsafe { Shell_NotifyIconW(message, icon) != 0 }
}

fn last_error(operation: &'static str) -> PlatformError {
    PlatformError::Native {
        operation,
        code: unsafe { GetLastError() },
    }
}

fn encode_activation(activation: Activation) -> WPARAM {
    match activation {
        Activation::Focus => 1,
        Activation::OpenReview => 2,
    }
}

fn decode_activation(code: WPARAM) -> Option<Activation> {
    match code {
        1 => Some(Activation::Focus),
        2 => Some(Activation::OpenReview),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_message_accepts_only_known_routes() {
        assert_eq!(
            decode_activation(encode_activation(Activation::Focus)),
            Some(Activation::Focus)
        );
        assert_eq!(
            decode_activation(encode_activation(Activation::OpenReview)),
            Some(Activation::OpenReview)
        );
        assert_eq!(decode_activation(0), None);
        assert_eq!(decode_activation(usize::MAX), None);
    }

    #[test]
    fn isolated_instances_get_separate_native_names() {
        let first = InstanceNames::new(Some(Path::new("fixture-one")));
        let replay = InstanceNames::new(Some(Path::new("fixture-one")));
        let other = InstanceNames::new(Some(Path::new("fixture-two")));
        let normal = InstanceNames::new(None);
        assert_eq!(first.mutex, replay.mutex);
        assert_ne!(first.class, other.class);
        assert_ne!(first.activation_message, normal.activation_message);
    }
}
