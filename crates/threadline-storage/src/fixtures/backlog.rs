use super::*;
use crate::writes::{WriteIntent, WriteSubmission};

pub const LARGE_WORKSPACE_REVISION: &str = "s12-large-workspace-v1";
pub const LARGE_WORKSPACE_WRITES: usize = 12;
pub const BACKLOG_USER: u64 = 41;
pub const BACKLOG_REPOSITORY: u64 = 71;
pub const BACKLOG_ARCHIVE: u64 = 72;
pub const BACKLOG_OWNER: &str = "fixture-org";
pub const BACKLOG_REPO_NAME: &str = "private";
pub const BACKLOG_LOGIN: &str = "fixture-user";

fn operation(index: usize) -> String {
    format!("00000000-0000-4000-8001-{index:012x}")
}

pub fn backlog_comment(index: usize) -> String {
    format!("Threadline synthetic backlog action {index}.")
}

pub(super) fn seed_writes(store: &Store, account: AccountId, issue: ItemId) -> Result<()> {
    let first = operation(0);
    for index in 0..LARGE_WORKSPACE_WRITES {
        let dependency = [first.as_str()];
        let dependencies: &[&str] = if index == 1 { &dependency } else { &[] };
        store.submit_write(
            account,
            WriteSubmission {
                id: &operation(index),
                item: issue,
                intent: &WriteIntent::CreateComment {
                    body: backlog_comment(index),
                },
                draft: None,
                dependencies,
                submitted_at_ms: FIXTURE_TIME_MS,
            },
        )?;
    }
    Ok(())
}

/// Confirm the marker through normal durable intent decoding. It survives cancellation and restart.
pub fn has_backlog_marker(store: &Store, account: AccountId) -> Result<bool> {
    Ok(store.write_intent(account, &operation(0))?
        == Some(WriteIntent::CreateComment {
            body: backlog_comment(0),
        }))
}
