#![cfg_attr(windows, windows_subsystem = "windows")]

use std::path::PathBuf;
use std::time::Instant;

use threadline_app::Options;

#[cfg(feature = "e2e")]
mod reader_fixture;

fn main() {
    #[cfg(feature = "e2e")]
    if let Some(result) = reader_fixture::run() {
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    #[cfg(windows)]
    if threadline_app::handle_notification_command() {
        return;
    }
    let started = Instant::now();
    if let Err(error) = parse_args().and_then(|options| {
        threadline_app::run(options, started).map_err(|error| error.to_string())
    }) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn parse_args() -> Result<Options, String> {
    let mut options = Options::default();
    if cfg!(feature = "e2e")
        && let Ok(seed) = std::env::var("THREADLINE_TEST_SEED")
    {
        options.seed = seed
            .parse()
            .map_err(|_| "THREADLINE_TEST_SEED must be an unsigned integer.")?;
    }
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            #[cfg(windows)]
            Some("--notification-com-server") => options.notification_com_server = true,
            #[cfg(windows)]
            Some(arg)
                if options.notification_com_server
                    && (arg.eq_ignore_ascii_case("-embedding")
                        || arg.eq_ignore_ascii_case("/embedding")) =>
            {
                ()
            }
            Some("--open-review") => options.open_review = true,
            Some("--e2e-report") if cfg!(feature = "e2e") && options.e2e_report.is_none() => {
                let path = PathBuf::from(
                    arguments
                        .next()
                        .ok_or("--e2e-report requires an absolute path.")?,
                );
                if !path.is_absolute() {
                    return Err("--e2e-report requires an absolute path.".into());
                }
                options.e2e_report = Some(path);
            }
            Some("--measure") if cfg!(feature = "e2e") => options.measure = true,
            Some("--fixture-s01") if cfg!(feature = "e2e") => options.feasibility_fixture = true,
            _ => return Err(format!("Unknown argument: {}", argument.to_string_lossy())),
        }
    }
    if options.measure && options.e2e_report.is_none() {
        return Err("--measure requires --e2e-report.".into());
    }
    if cfg!(feature = "e2e")
        && let Some(profile) = std::env::var_os("THREADLINE_PROFILE_DIR")
    {
        let profile = PathBuf::from(profile);
        if !profile.is_absolute() || !profile.is_dir() {
            return Err("THREADLINE_PROFILE_DIR must be an existing absolute directory.".into());
        }
        options.profile = Some(profile);
    }
    Ok(options)
}
