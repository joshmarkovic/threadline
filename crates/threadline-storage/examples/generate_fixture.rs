use std::{
    env,
    error::Error,
    path::{Path, PathBuf},
};

use threadline_storage::{
    Store,
    fixtures::{Scale, populate, populate_reader},
};

fn main() -> Result<(), Box<dyn Error>> {
    let usage = "Usage: generate_fixture <new-database-path> <small|section16|reader-small|reader-section16> <seed>";
    let [database, scale, seed]: [_; 3] = env::args_os()
        .skip(1)
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| usage)?;
    let (scale, reader) = match scale.to_str() {
        Some("small") => (Scale::Small, false),
        Some("section16") => (Scale::Section16, false),
        Some("reader-small") => (Scale::Small, true),
        Some("reader-section16") => (Scale::Section16, true),
        _ => return Err(usage.into()),
    };
    let seed = seed.to_str().ok_or(usage)?.parse()?;
    let database = PathBuf::from(database);
    if database.try_exists()? {
        return Err("The fixture database path already exists.".into());
    }
    let parent = database
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let staged = tempfile::Builder::new()
        .prefix(".threadline-fixture-")
        .tempfile_in(parent)?
        .into_temp_path();
    let store = Store::open(&staged)?;
    let fixture = if reader {
        populate_reader(&store, seed, scale)?
    } else {
        populate(&store, seed, scale)?
    };
    drop(store);
    staged.persist_noclobber(&database)?;

    println!(
        "{} seed {seed}: {} items, {} comments, {} open tabs, and {} Outbox entries.",
        fixture.revision,
        fixture.item_count,
        fixture.comment_count,
        fixture.open_tabs,
        fixture.outbox_count
    );
    Ok(())
}
