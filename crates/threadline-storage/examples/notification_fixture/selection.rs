use super::{OWNER, RUNTIME_MARKER};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{Value, json};
use std::{error::Error, fs, path::Path, time::Duration};
use threadline_storage::{ItemId, queue::Workspace, reviews::ReviewDraft};

pub(super) fn inspect(root: &Path) -> Result<Value, Box<dyn Error>> {
    if fs::read(root.join(RUNTIME_MARKER))? != RUNTIME_MARKER.as_bytes() {
        return Err("Unknown runtime fixture.".into());
    }
    let mut db = Connection::open_with_flags(
        root.join("threadline.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    db.busy_timeout(Duration::from_secs(3))?;
    let tx = db.transaction()?;
    let (raw, item, rev, base, head): (String, ItemId, i64, String, String) = tx.query_row(
        "SELECT w.layout, i.id, v.id, v.base_oid, v.head_oid
         FROM workspaces w
         JOIN dispatch_profiles d USING (account_id)
         JOIN github_access_profiles p USING (account_id)
         JOIN repositories r USING (account_id)
         JOIN items i ON i.repository_id = r.id
         JOIN pr_revisions v ON v.item_id = i.id AND v.account_id = w.account_id
         WHERE p.owner_scope = ? AND r.provider_repository_id = '917123456' AND i.number = 70
           AND d.offline = 1 AND (SELECT COUNT(*) FROM accounts) = 1
           AND (SELECT COUNT(*) FROM pr_revisions) = 1",
        [OWNER],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    let layout: Workspace = serde_json::from_str(&raw)?;
    let pane = layout.panes.first().ok_or("Missing fixture pane")?;
    let tab = pane
        .tabs
        .get(1)
        .filter(|t| t.item == item)
        .ok_or("Missing fixture PR tab")?;
    let pr = tab.view.pr_reader.as_ref();
    let count: u32 = tx.query_row("SELECT COUNT(*) FROM drafts", [], |r| r.get(0))?;
    let draft: Option<(String, String)> = tx
        .query_row(
            "SELECT id,anchor FROM drafts WHERE item_id=?1 AND kind='review' AND base=?2",
            rusqlite::params![item, format!("pr-review-v1/{rev}")],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let draft = if let Some((id, raw)) = draft {
        let parsed: ReviewDraft = serde_json::from_str(&raw)?;
        let anchors: Vec<_> = parsed
            .comments
            .iter()
            .map(|comment| &comment.anchor)
            .collect();
        Some(json!({"id": id, "tab_draft": tab.view.draft, "anchors": anchors}))
    } else {
        None
    };
    Ok(json!({
        "revision": rev, "base": base, "head": head, "active": pane.active,
        "section": pr.map(|view| view.section),
        "path": pr.and_then(|view| view.selected_path.as_ref()),
        "source_selection": pr.and_then(|view| view.code_anchor.as_ref()),
        "drafts": count, "draft": draft
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    use threadline_storage::{
        Store,
        pr::SourceSide,
        reviews::{DraftInline, ReviewEvent},
    };
    #[test]
    fn fresh_runtime_selection_has_no_draft_or_anchor() {
        let dir = tempfile::tempdir().unwrap();
        assert!(inspect(dir.path()).is_err());
        crate::seed(dir.path(), crate::Fixture::Runtime).unwrap();
        let value = inspect(dir.path()).unwrap();
        assert_eq!(value["drafts"], 0);
        assert!(value["source_selection"].is_null());
        assert!(value["draft"].is_null());
        assert_eq!(value["head"], "b".repeat(40));
        assert_eq!(value["base"], "a".repeat(40));

        let store = Store::open_profile(dir.path()).unwrap();
        let account = store.active_github_account().unwrap().unwrap();
        let mut workspace = store.workspace(account).unwrap().unwrap();
        let tab = &mut workspace.panes[0].tabs[1];
        let (revision, _) = store
            .current_pr_revision(account, tab.item)
            .unwrap()
            .unwrap();
        let anchor = store
            .capture_inline_source_anchor(&revision, "second.txt", SourceSide::Right, 1, 1)
            .unwrap();
        let id = "fixture-selection-review";
        store
            .create_review_draft(
                &revision,
                id,
                &ReviewDraft {
                    event: ReviewEvent::Comment,
                    summary: String::new(),
                    comments: vec![DraftInline {
                        key: "first".into(),
                        body: String::new(),
                        anchor,
                    }],
                },
            )
            .unwrap();
        tab.view.draft = Some(id.into());
        store.save_workspace(account, &workspace).unwrap();
        drop(store);

        let saved = inspect(dir.path()).unwrap();
        assert_eq!(saved["drafts"], 1);
        assert_eq!(saved["draft"]["id"], id);
        assert_eq!(saved["draft"]["tab_draft"], id);
        let line = &saved["draft"]["anchors"][0];
        assert_eq!(line["path"], "second.txt");
        assert_eq!(line["side"], "Right");
        assert_eq!(line["first_line"], 1);
        assert_eq!(line["source_oid"], value["head"]);
    }
}
