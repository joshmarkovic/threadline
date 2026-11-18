#[cfg(feature = "e2e")]
use std::path::PathBuf;

pub fn handle_notification_command() -> bool {
    let mut arguments = std::env::args_os().skip(1);
    let command = arguments.next();
    match command.as_deref().and_then(std::ffi::OsStr::to_str) {
        Some("--notification-com-server") => {
            // The running desktop worker already owns warm activation. A cold COM launch has no live review token.
            std::process::exit(78);
        }
        #[cfg(feature = "e2e")]
        Some("--cleanup-test-toast") => {
            let Some(path) = arguments
                .next()
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
            else {
                std::process::exit(2)
            };
            let Ok(token) = std::env::var("THREADLINE_TEST_TOAST_TOKEN") else {
                std::process::exit(2)
            };
            if arguments.next().is_some() {
                std::process::exit(2)
            }
            let result = threadline_platform::clear_test_toast_history(&token);
            let report = match result {
                Ok(removed) => serde_json::json!({"verified_absent": true, "removed": removed}),
                Err(threadline_platform::PlatformError::Native { operation, code }) => {
                    serde_json::json!({"verified_absent": false, "operation": operation, "code": code})
                }
                Err(_) => {
                    serde_json::json!({"verified_absent": false, "operation": "Verify test ownership"})
                }
            };
            use std::io::Write;
            let written = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path)
                .and_then(|mut file| writeln!(file, "{report}"));
            if written.is_err() || result.is_err() {
                std::process::exit(78)
            }
            true
        }
        _ => false,
    }
}
