//! Read only the two fixed registration records for the installed Windows application.
#[cfg(windows)]
#[path = "../src/toast_registration.rs"]
mod toast_registration;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::path::PathBuf;
    use windows::UI::Notifications::ToastNotificationManager;
    use windows::core::{GUID, HSTRING};
    use windows_sys::Win32::System::Registry::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER};
    let local = match std::env::args().nth(1).as_deref() {
        Some("Local") => true,
        Some("Release") => false,
        _ => return Err("Use Local or Release to inspect the fixed installed application.".into()),
    };
    let (title, app_id, uuid, class) = if local {
        (
            "Threadline Installer Test",
            "Threadline.InstallTest",
            "df3bb9a2-0794-44a6-b130-a35197093afe",
            GUID::from_u128(0xdf3bb9a2_0794_44a6_b130_a35197093afe),
        )
    } else {
        (
            "Threadline",
            "Threadline.Desktop",
            "5a20cea8-a5d8-4b5e-9be3-4121ba0aa753",
            GUID::from_u128(0x5a20cea8_a5d8_4b5e_9be3_4121ba0aa753),
        )
    };
    let dir = PathBuf::from(
        std::env::var_os("LOCALAPPDATA").ok_or("Missing local application directory")?,
    );
    let app = dir.join("Programs").join(title).join("threadline.exe");
    let command = format!("\"{}\" --notification-com-server", app.display());
    println!("installed_target_exists={}", app.is_file());
    let server = format!(r"CLSID\{{{uuid}}}\LocalServer32");
    println!(
        "merged_registry_command={}",
        toast_registration::registry_matches(HKEY_CLASSES_ROOT, &server, None, &command)
    );
    println!(
        "user_registry_command={}",
        toast_registration::registry_matches(
            HKEY_CURRENT_USER,
            &format!(r"Software\Classes\{server}"),
            None,
            &command
        )
    );
    if let Ok(canon) = app.canonicalize() {
        println!(
            "merged_registry_extended_command={}",
            toast_registration::registry_matches(
                HKEY_CLASSES_ROOT,
                &server,
                None,
                &format!("\"{}\" --notification-com-server", canon.display())
            )
        );
    }
    println!(
        "exact_rust_shortcut={}",
        toast_registration::installed_link_matches(&app, app_id, class, &format!("{title}.lnk"))
    );
    // This diagnostic neither registers an activation class nor publishes a notification.
    unsafe {
        windows::Win32::System::WinRT::RoInitialize(
            windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
        )
    }?;
    let app_id = HSTRING::from(app_id);
    let probe = |phase| match ToastNotificationManager::CreateToastNotifierWithId(&app_id) {
        Ok(notifier) => {
            println!("{phase}_create_notifier=true");
            match notifier.Setting() {
                Ok(setting) => println!("{phase}_sender_setting={}", setting.0),
                Err(error) => println!(
                    "{phase}_sender_setting_hresult=0x{:08X}",
                    error.code().0 as u32
                ),
            }
        }
        Err(error) => println!("{phase}_create_hresult=0x{:08X}", error.code().0 as u32),
    };
    probe("before_process_id");
    unsafe {
        windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(windows::core::PCWSTR(
            app_id.as_ptr(),
        ))
    }?;
    probe("with_process_id");
    unsafe { windows::Win32::System::WinRT::RoUninitialize() };
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This diagnostic requires Windows.");
    std::process::exit(78);
}
