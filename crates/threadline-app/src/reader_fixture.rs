//! Generate a cached reader fixture in a separate process before a native test.

use std::{fs::OpenOptions, io::BufWriter, path::PathBuf};

use threadline_storage::{
    ItemKind, Store, UtcMillis,
    access::AccessState,
    fixtures::{Scale, populate_large_workspace, populate_reader},
    notifications::DeliveryMode,
    resources::ActivityEvent,
    writes::{WriteIntent, WriteSubmission},
};

mod comment_links;
mod display_time_zone;
mod mixed;
mod panes;
mod recovery;

#[derive(Clone, Copy)]
enum ReaderFixture {
    Small,
    Section16,
    Backlog,
    PanePositions,
    CommentLinks,
    DisplayTimeZone,
}

pub fn run() -> Option<Result<(), String>> {
    let mut arguments = std::env::args_os().skip(1);
    let action = arguments.next()?;
    if action == "--e2e-read-recovery" {
        return Some(recovery::run(arguments));
    }
    let quiet_notification = action == "--e2e-seed-notification";
    if !quiet_notification && action != "--e2e-seed-reader" {
        return None;
    }
    Some((|| {
        let scale = if quiet_notification {
            None
        } else {
            Some(
                match arguments
                    .next()
                    .as_deref()
                    .and_then(std::ffi::OsStr::to_str)
                {
                    Some("small") => ReaderFixture::Small,
                    Some("section-16") => ReaderFixture::Section16,
                    Some("section-16-backlog") => ReaderFixture::Backlog,
                    Some("small-pane-positions") => ReaderFixture::PanePositions,
                    Some("small-comment-links") => ReaderFixture::CommentLinks,
                    Some("small-display-timezone") => ReaderFixture::DisplayTimeZone,
                    _ => {
                        return Err("Select a known isolated reader fixture.".to_owned());
                    }
                },
            )
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
        let Some(fixture) = scale else {
            return seed_quiet_notification(profile, report);
        };
        let seed: u64 = match std::env::var("THREADLINE_TEST_SEED") {
            Ok(seed) => seed.parse().map_err(|_| "Invalid reader fixture seed.")?,
            Err(std::env::VarError::NotPresent) => 12345,
            Err(_) => return Err("Invalid reader fixture seed.".into()),
        };
        seed_profile(profile, fixture, seed, report)
    })())
}

fn seed_quiet_notification(profile: PathBuf, report: PathBuf) -> Result<(), String> {
    use threadline_core::domain::RepositoryName;
    const EVENT: &str = "synthetic-quiet-activity-v1";
    let scope_error = "The quiet-hours fixture requires an owned, offline loopback profile and active UTC quiet hours.";
    let canonical = profile.canonicalize().map_err(|_| scope_error)?;
    let disposable = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("initial-"))
        .is_some_and(|id| id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        && canonical
            .parent()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("threadline-e2e-"));
    let loopback = std::env::var("THREADLINE_E2E_GITHUB_ADDR")
        .ok()
        .and_then(|address| address.parse::<std::net::SocketAddrV4>().ok())
        .is_some_and(|address| {
            *address.ip() == std::net::Ipv4Addr::LOCALHOST && address.port() > 0
        });
    let database = canonical.join("threadline.sqlite3");
    let offline_phase = canonical.parent().is_some_and(|root| {
        let phase = root.join("access-fixture-phase.txt");
        std::fs::symlink_metadata(&phase)
            .is_ok_and(|entry| entry.file_type().is_file() && entry.len() == 12)
            && std::fs::read(phase).is_ok_and(|contents| contents == b"sync-offline")
    });
    if !loopback
        || !disposable
        || !offline_phase
        || !std::fs::symlink_metadata(database).is_ok_and(|entry| entry.file_type().is_file())
    {
        return Err(scope_error.into());
    }
    let store = Store::open_profile(&canonical).map_err(|_| scope_error)?;
    let account = store
        .active_github_account()
        .map_err(|_| scope_error)?
        .ok_or(scope_error)?;
    let access = store
        .github_access_profile(account, "fixture-org")
        .map_err(|_| scope_error)?
        .ok_or(scope_error)?;
    let mut repositories = store
        .github_repository_views(account)
        .map_err(|_| scope_error)?;
    if repositories.len() != 1
        || access.user_id.get() != 41
        || access.login != "fixture-user"
        || access.access != AccessState::Readable
    {
        return Err(scope_error.into());
    }
    let repo = repositories.pop().expect("one scoped repository");
    if repo.owner != "fixture-org"
        || repo.name != "private"
        || repo.provider_id.get() != 71
        || !repo.selected
        || repo.private != Some(true)
    {
        return Err(scope_error.into());
    }
    let controls = store
        .sync_settings(account)
        .map_err(|_| scope_error)?
        .controls;
    let notifications = store
        .notification_settings(account)
        .map_err(|_| scope_error)?;
    let now = UtcMillis::now().map_err(|_| scope_error)?.0;
    let minute = (now / 60_000).rem_euclid(1440) as u16;
    let active_quiet = notifications.quiet_hours.is_some_and(|hours| {
        let active = if hours.start_minute < hours.end_minute {
            (hours.start_minute..hours.end_minute).contains(&minute)
        } else {
            minute >= hours.start_minute || minute < hours.end_minute
        };
        hours.timezone.as_deref() == Some("UTC") && hours.weekdays == 127 && active
    });
    if !controls.work_offline
        || !controls.pause_background
        || !controls.pause_queued_actions
        || controls.notification_quiet
        || notifications.fallback != DeliveryMode::Popup
        || !active_quiet
    {
        return Err(scope_error.into());
    }
    let route = RepositoryName::new("fixture-org", "private").map_err(|_| scope_error)?;
    let item = store
        .resolve_downloaded_route(account, &route, 41, Some(ItemKind::Issue))
        .map_err(|_| scope_error)?
        .ok_or(scope_error)?;
    if !store
        .queue_page(account, 0, 1)
        .map_err(|_| scope_error)?
        .is_empty()
    {
        return Err(scope_error.into());
    }
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report)
        .map_err(|_| scope_error)?;
    let operation = store.new_operation_id().map_err(|_| scope_error)?;
    store
        .submit_write(
            account,
            WriteSubmission {
                id: &operation,
                item,
                intent: &WriteIntent::CreateComment {
                    body: "Threadline S07 explicitly queued comment.".into(),
                },
                draft: None,
                dependencies: &[],
                submitted_at_ms: now,
            },
        )
        .map_err(|_| "Could not seed the isolated typed Outbox action.")?;
    if !store
        .record_activity_event(
            account,
            EVENT,
            &ActivityEvent {
                repository: repo.id,
                item: Some(item),
                kind: "comment_created".into(),
                revision: None,
                occurred_at_ms: Some(now),
                observed_at_ms: now,
            },
        )
        .map_err(|_| "Could not seed the isolated activity event.")?
    {
        return Err("The fixture activity was already present.".into());
    }
    serde_json::to_writer(output, &serde_json::json!({"fixture_revision":"s11-native-notification-schedule-v1", "queued":1, "events":1}))
        .map_err(|_| "Could not write isolated schedule fixture receipt.".into())
}

fn seed_profile(
    profile: PathBuf,
    variant: ReaderFixture,
    seed: u64,
    report: PathBuf,
) -> Result<(), String> {
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
    let fixture = match variant {
        ReaderFixture::Small
        | ReaderFixture::PanePositions
        | ReaderFixture::CommentLinks
        | ReaderFixture::DisplayTimeZone => populate_reader(&store, seed, Scale::Small),
        ReaderFixture::Section16 => populate_reader(&store, seed, Scale::Section16),
        ReaderFixture::Backlog => populate_large_workspace(&store, seed),
    }
    .map_err(|error| error.to_string())?;
    let pane_positions = if matches!(variant, ReaderFixture::PanePositions) {
        Some(
            panes::extend_archive_discussion(&store, &fixture)
                .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    let comment_links = matches!(variant, ReaderFixture::CommentLinks)
        .then(|| comment_links::seed(&store, &fixture))
        .transpose()
        .map_err(|error| error.to_string())?;
    let display_time_zone = matches!(variant, ReaderFixture::DisplayTimeZone)
        .then(|| display_time_zone::seed(&store, &fixture))
        .transpose()
        .map_err(|error| error.to_string())?;
    let unresolved = store
        .sidebar_counts(
            fixture.account,
            threadline_storage::reader::ReaderScope::Selected,
        )
        .map_err(|error| error.to_string())?
        .outbox;
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
    let mut receipt = serde_json::json!({
        "fixture_revision": match variant {
            ReaderFixture::PanePositions => "s12-pane-positions-v1",
            ReaderFixture::CommentLinks => "s12-cached-comment-links-v1",
            ReaderFixture::DisplayTimeZone => "s12-display-timezone-v1",
            _ => fixture.revision,
        },
        "seed": seed,
        "scale": match variant {
            ReaderFixture::Small => "small",
            ReaderFixture::Section16 => "section-16",
            ReaderFixture::Backlog => "section-16-backlog",
            ReaderFixture::PanePositions => "small-pane-positions",
            ReaderFixture::CommentLinks => "small-comment-links",
            ReaderFixture::DisplayTimeZone => "small-display-timezone",
        },
        "account_id": fixture.account.0,
        "repository_id": fixture.repository.0,
        "discussion_item": fixture.long_discussion.0,
        "review_item": fixture.reviewed_pr.0,
        "items": fixture.item_count,
        "comments": fixture.comment_count,
        "open_tabs": fixture.open_tabs,
        "outbox_count": fixture.outbox_count,
        "typed_outbox_count": fixture.typed_outbox_count,
        "unresolved_outbox": unresolved,
        "offline": offline
    });
    if let Some(pane_positions) = pane_positions {
        receipt["comments"] = (fixture.comment_count - pane_positions.replaced_comments
            + pane_positions.comments)
            .into();
        receipt["pane_positions"] = serde_json::to_value(pane_positions)
            .map_err(|_| "Could not encode pane-position fixture evidence.")?;
    }
    if let Some(comment_links) = comment_links {
        receipt["open_tabs"] = 3.into();
        receipt["items"] = comment_links["cached_items"].clone();
        receipt["comments"] = comment_links["cached_comments"].clone();
        receipt["comment_links"] = comment_links;
    }
    if let Some(display_time_zone) = display_time_zone {
        receipt["display_time_zone"] = display_time_zone;
    }
    let mut output = BufWriter::new(report);
    serde_json::to_writer_pretty(&mut output, &receipt)
        .map_err(|_| "Could not write reader fixture evidence.")?;
    std::io::Write::flush(&mut output).map_err(|_| "Could not flush reader fixture evidence.")?;
    Ok(())
}
