use std::{path::PathBuf, process::ExitCode};
use threadline_installer::{Selection, Source};

#[cfg(windows)]
mod cleanup;
#[cfg(windows)]
use threadline_installer::windows_host;

struct Request {
    selection: Selection,
    offline: Option<PathBuf>,
    verify_only: bool,
}
fn request() -> Result<Request, &'static str> {
    let mut args = std::env::args_os().skip(1);
    let mut version = None;
    let mut channel = None;
    let mut digest = None;
    let mut offline = None;
    let mut verify_only = false;
    while let Some(key) = args.next() {
        if key == "--verify-only" && !verify_only {
            verify_only = true;
            continue;
        }
        let value = args.next().ok_or("Expected an argument value.")?;
        if key == "--offline-dir" && offline.is_none() {
            offline = Some(PathBuf::from(value));
            continue;
        }
        let value = value
            .into_string()
            .map_err(|_| "Invalid release selection.")?;
        match key.to_str() {
            Some("--version") if version.is_none() => version = Some(value),
            Some("--channel") if channel.is_none() => channel = Some(value),
            Some("--manifest-sha256") if digest.is_none() => digest = Some(value),
            _ => {
                return Err(
                    "Use --version VERSION --channel stable|preview --manifest-sha256 DIGEST [--offline-dir PATH] [--verify-only].",
                );
            }
        }
    }
    let selection = Selection::new(
        version
            .as_deref()
            .ok_or("Select the exact release version.")?,
        channel.as_deref().ok_or("Select stable or preview.")?,
        digest
            .as_deref()
            .ok_or("Supply the manifest digest from the trusted release announcement.")?,
    )?;
    Ok(Request {
        selection,
        offline,
        verify_only,
    })
}
fn configured_source() -> Result<Source, &'static str> {
    let origin = option_env!("THREADLINE_RELEASE_DIRECTORY").ok_or(
        "This development bootstrap has no approved release directory. Publication is pending.",
    )?;
    let publisher = option_env!("THREADLINE_RELEASE_PUBLISHER_SHA256")
        .ok_or("The Threadline publisher identity has not been configured.")?;
    Source::https(origin, publisher)
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
async fn run() -> Result<(), &'static str> {
    let mut arguments = std::env::args_os().skip(1);
    if let Some(option) = arguments.next()
        && (option == "--preview-local-removal" || option == "--remove-local-data-and-credentials")
    {
        if arguments.next().is_some() {
            return Err("The local removal commands accept no additional arguments or paths.");
        }
        #[cfg(windows)]
        return cleanup::command(option == "--remove-local-data-and-credentials");
        #[cfg(not(windows))]
        return Err("This local workspace removal command requires Windows.");
    }
    let request = request()?;
    let source = configured_source()?;
    #[cfg(not(windows))]
    {
        let Request {
            selection,
            offline,
            verify_only,
        } = request;
        let _ = (selection, offline, verify_only, source);
        Err("This release bootstrap requires native x64 Windows 11.")
    }
    #[cfg(windows)]
    {
        use std::process::Command;
        let host = windows_host::detect()?;
        if !request.verify_only {
            host.check()?;
            windows_host::require_launch_ready()?;
        }
        let staged = source
            .stage(&request.selection, &host, request.offline.as_deref())
            .await?;
        threadline_platform::publisher::verify(&staged.path, source.publisher()).map_err(|_| {
            "Windows could not verify the installer against the configured Threadline publisher. Use the trusted signed release."
        })?;
        if request.verify_only {
            println!(
                "Verified Threadline {} ({}) for Windows x64.",
                staged.manifest.version, staged.manifest.channel
            );
            return Ok(());
        }
        windows_host::require_launch_ready()?;
        let result = Command::new(&staged.path).status().map_err(|_| {
            "Windows prevented setup from starting. Check organization policy or use the verified manual installer."
        })?;
        match result.code() {
            Some(0) => {
                println!(
                    "Installed Threadline {} for the current user. Open Threadline from Start.",
                    staged.manifest.version
                );
                Ok(())
            }
            Some(11) => Err("Setup requires the supported Windows architecture and build."),
            Some(12) => {
                Err("Setup refused an incompatible downgrade. Use the same or a newer build.")
            }
            Some(13) => Err("Another setup is active. Wait for it to finish, then rerun setup."),
            Some(14) => Err("An application file is in use. Quit Threadline and rerun setup."),
            Some(15) => Err("The staged application failed verification. Obtain an intact setup."),
            Some(16) => {
                Err("Setup could not stage the update. Check available disk space and rerun setup.")
            }
            Some(17) => Err(
                "Setup found an unmanaged install location or shortcut. Inspect it before retrying.",
            ),
            Some(20) => Err("Setup could not verify its configured publisher."),
            Some(21 | 22) => Err(
                "Setup could not verify the expected Start shortcut or sender. Inspect the existing entry before retrying.",
            ),
            Some(23 | 24) => Err(
                "Windows denied registration access. Check current-user permissions and organization policy.",
            ),
            Some(25..=27) => Err(
                "Windows could not complete the Start menu registration. See the specific error in setup details.",
            ),
            _ => Err(
                "The installer did not complete. Inspect its on-screen error before rerunning setup.",
            ),
        }
    }
}
