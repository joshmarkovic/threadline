//! Generate a cached reader fixture in a separate process before a native test.

use std::{fs::OpenOptions, io::BufWriter, path::PathBuf};

use threadline_storage::{
    Store,
    fixtures::{Scale, populate_reader},
};

pub fn run() -> Option<Result<(), String>> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next()?.as_os_str() != "--e2e-seed-reader" {
        return None;
    }
    Some((|| {
        let scale = match arguments
            .next()
            .as_deref()
            .and_then(std::ffi::OsStr::to_str)
        {
            Some("small") => Scale::Small,
            Some("section-16") => Scale::Section16,
            _ => return Err("Select small or section-16 for the reader fixture.".to_owned()),
        };
        if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--e2e-report")) {
            return Err("An absolute E2E report path is required.".into());
        }
        let report = arguments
            .next()
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or("An absolute E2E report path is required.")?;
        if arguments.next().is_some() {
            return Err("Unexpected reader fixture arguments.".into());
        }
        let profile = std::env::var_os("THREADLINE_PROFILE_DIR")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or("An explicit isolated profile is required.")?;
        let seed: u64 = match std::env::var("THREADLINE_TEST_SEED") {
            Ok(seed) => seed.parse().map_err(|_| "Invalid reader fixture seed.")?,
            Err(std::env::VarError::NotPresent) => 12345,
            Err(_) => return Err("Invalid reader fixture seed.".into()),
        };
        seed_profile(profile, scale, seed, report)
    })())
}

fn seed_profile(profile: PathBuf, scale: Scale, seed: u64, report: PathBuf) -> Result<(), String> {
    let mut entries = std::fs::read_dir(&profile)
        .map_err(|_| "The isolated profile directory is unavailable.")?;
    if entries.next().is_some() {
        return Err("The reader fixture requires an empty isolated profile.".into());
    }
    let report = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report)
        .map_err(|_| "Could not create the reader fixture report.")?;
    let store = Store::open_profile(&profile).map_err(|error| error.to_string())?;
    let fixture = populate_reader(&store, seed, scale).map_err(|error| error.to_string())?;
    let offline = store
        .dispatch_profile(fixture.account)
        .map_err(|error| error.to_string())?
        .offline;
    if !offline
        || store
            .active_github_account()
            .map_err(|error| error.to_string())?
            != Some(fixture.account)
    {
        return Err("The reader fixture did not establish its isolated offline account.".into());
    }
    drop(store);
    let receipt = serde_json::json!({
        "fixture_revision": fixture.revision,
        "seed": seed,
        "scale": if scale == Scale::Section16 { "section-16" } else { "small" },
        "account_id": fixture.account.0,
        "repository_id": fixture.repository.0,
        "discussion_item": fixture.long_discussion.0,
        "review_item": fixture.reviewed_pr.0,
        "items": fixture.item_count,
        "comments": fixture.comment_count,
        "open_tabs": fixture.open_tabs,
        "outbox_count": fixture.outbox_count,
        "offline": offline
    });
    let mut output = BufWriter::new(report);
    serde_json::to_writer_pretty(&mut output, &receipt)
        .map_err(|_| "Could not write reader fixture evidence.")?;
    std::io::Write::flush(&mut output).map_err(|_| "Could not flush reader fixture evidence.")?;
    Ok(())
}
