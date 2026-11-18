use super::{check_item, json, parse};
use crate::{AccountId, Error, ItemId, Result, Store};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    pub id: String,
    pub item: ItemId,
    pub view: TabView,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TabView {
    pub scroll_offset: u64,
    pub scroll_anchor: Option<String>,
    pub navigation: Option<String>,
    pub draft: Option<String>,
    pub context: Option<PrContext>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pane {
    pub active: Option<usize>,
    pub tabs: Vec<Tab>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub focused_pane: usize,
    pub proportions: Vec<f64>,
    pub panes: Vec<Pane>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftKind {
    IssueEdit,
    Comment,
    Review,
    Reply,
}
impl DraftKind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::IssueEdit => "issue_edit",
            Self::Comment => "comment",
            Self::Review => "review",
            Self::Reply => "reply",
        }
    }
    fn decode(kind: &str) -> Result<Self> {
        match kind {
            "issue_edit" => Ok(Self::IssueEdit),
            "comment" => Ok(Self::Comment),
            "review" => Ok(Self::Review),
            "reply" => Ok(Self::Reply),
            _ => Err(Error::InvalidInput("unsupported draft kind")),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftContent {
    pub body: String,
    pub base: String,
    pub anchor: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub id: String,
    pub item: ItemId,
    pub kind: DraftKind,
    pub content: DraftContent,
    pub revision: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceCandidate {
    pub position: i64,
    pub raw: String,
    pub provenance: String,
    /// Verified stable issue identity and Unix validation time, when known.
    pub verified: Option<(ItemId, i64)>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrContext {
    pub selected_issue: Option<ItemId>,
    pub body_revision: String,
    pub provenance: String,
    pub ratio: f64,
}

fn validate_workspace(
    db: &rusqlite::Connection,
    account: AccountId,
    workspace: &Workspace,
) -> Result<()> {
    if workspace.focused_pane >= workspace.panes.len()
        || workspace.proportions.len() != workspace.panes.len()
        || workspace
            .proportions
            .iter()
            .any(|p| !p.is_finite() || *p <= 0.0)
        || (workspace.proportions.iter().sum::<f64>() - 1.0).abs() > 0.0001
    {
        return Err(Error::InvalidInput(
            "invalid workspace proportions or focused pane",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for pane in &workspace.panes {
        if pane.active.is_some_and(|i| i >= pane.tabs.len())
            || pane.active.is_none() != pane.tabs.is_empty()
        {
            return Err(Error::InvalidInput("invalid active tab"));
        }
        for tab in &pane.tabs {
            if tab.id.is_empty() || !seen.insert(&tab.id) {
                return Err(Error::InvalidInput("duplicate or empty tab identity"));
            }
            check_item(db, account, tab.item)?;
            if let Some(context) = &tab.view.context {
                check_context(db, account, tab.item, context)?;
            }
            if let Some(id) = &tab.view.draft {
                let item: Option<ItemId> = db
                    .query_row(
                        "SELECT item_id FROM drafts WHERE account_id=? AND id=?",
                        params![account, id],
                        |r| r.get(0),
                    )
                    .optional()?;
                if item.is_some_and(|target| target != tab.item) {
                    return Err(Error::Conflict("draft belongs to another item"));
                }
            }
        }
    }
    Ok(())
}
pub(crate) fn write_workspace(
    tx: &Transaction<'_>,
    account: AccountId,
    workspace: &Workspace,
) -> Result<()> {
    validate_workspace(tx, account, workspace)?;
    let previous: Option<String> = tx
        .query_row(
            "SELECT layout FROM workspaces WHERE account_id=?",
            [account],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(previous) = previous {
        let previous: Workspace = parse(&previous)?;
        let open: std::collections::HashSet<&str> = workspace
            .panes
            .iter()
            .flat_map(|p| p.tabs.iter().map(|t| t.id.as_str()))
            .collect();
        for (pi, pane) in previous.panes.iter().enumerate() {
            for (ti, tab) in pane
                .tabs
                .iter()
                .enumerate()
                .filter(|(_, t)| !open.contains(t.id.as_str()))
            {
                tx.execute(
                    "DELETE FROM reader_closed_tabs WHERE account_id=? AND tab_id=?",
                    params![account, tab.id],
                )?;
                tx.execute("INSERT INTO reader_closed_tabs(account_id,item_id,tab_id,pane_index,tab_index,view) VALUES(?,?,?,?,?,?)",
                    params![account,tab.item,tab.id,i64::try_from(pi).map_err(|_|Error::Corrupt)?,i64::try_from(ti).map_err(|_|Error::Corrupt)?,json(&tab.view)?])?;
            }
        }
        let mut forget =
            tx.prepare_cached("DELETE FROM reader_closed_tabs WHERE account_id=? AND tab_id=?")?;
        for id in open {
            forget.execute(params![account, id])?;
        }
    }
    tx.execute(
        "INSERT INTO workspaces(account_id,layout) VALUES(?1,?2)
        ON CONFLICT(account_id) DO UPDATE SET layout=excluded.layout",
        params![account, json(workspace)?],
    )?;
    Ok(())
}

impl Store {
    pub fn save_workspace(&self, account: AccountId, workspace: &Workspace) -> Result<()> {
        self.write(|tx| write_workspace(tx, account, workspace))
    }
    pub fn workspace(&self, account: AccountId) -> Result<Option<Workspace>> {
        self.read(|db| {
            let raw: Option<String> = db
                .query_row(
                    "SELECT layout FROM workspaces WHERE account_id=?",
                    [account],
                    |r| r.get(0),
                )
                .optional()?;
            raw.as_deref()
                .map(|data| {
                    let ws = parse(data)?;
                    validate_workspace(db, account, &ws)?;
                    Ok(ws)
                })
                .transpose()
        })
    }
    /// Restore the most recently closed view and its location in one transaction.
    pub fn reopen_last_tab(
        &self,
        account: AccountId,
        target_pane: Option<usize>,
    ) -> Result<Option<Tab>> {
        self.write(|tx| {
        let Some(layout) = tx
            .query_row("SELECT layout FROM workspaces WHERE account_id=?", [account], |r| r.get::<_, String>(0))
            .optional()?
        else {
            return Ok(None);
        };
        let mut ws: Workspace = parse(&layout)?;
        validate_workspace(tx, account, &ws)?;
        let row: Option<(ItemId, String, i64, i64, String)> = tx
            .query_row("SELECT item_id,tab_id,pane_index,tab_index,view FROM reader_closed_tabs
               WHERE account_id=? ORDER BY sequence DESC LIMIT 1", [account], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .optional()?;
        let Some((item, id, old_pane, old_index, view)) = row else {
            return Ok(None);
        };
        let old_pane = usize::try_from(old_pane).map_err(|_| Error::Corrupt)?;
        let old_index = usize::try_from(old_index).map_err(|_| Error::Corrupt)?;
        let mut view: TabView = parse(&view)?;
        if let Some(context) = view.context.as_mut()
            && context.selected_issue.is_some()
                && !tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM pr_reference_sets s JOIN pr_reference_candidates c ON c.account_id=s.account_id AND c.pr_id=s.pr_id WHERE s.account_id=? AND s.pr_id=? AND s.body_revision=? AND c.verified_issue=? AND c.provenance=?)",
                    params![
                        account,
                        item,
                        context.body_revision,
                        context.selected_issue,
                        context.provenance
                    ],
                    |r| r.get::<_, bool>(0),
                )?
            {
            context.selected_issue = None;
        }
        let tab = Tab { id, item, view };
        let position = target_pane.unwrap_or(if old_pane < ws.panes.len() {
            old_pane
        } else {
            ws.focused_pane
        });
        let pane = ws
            .panes
            .get_mut(position)
            .ok_or(Error::InvalidInput("Invalid target pane"))?;
        let index = old_index.min(pane.tabs.len());
        pane.tabs.insert(index, tab.clone());
        pane.active = Some(index);
        ws.focused_pane = position;
        write_workspace(tx, account, &ws)?;
        Ok(Some(tab))
    })
    }
    pub fn set_preference(&self, account: AccountId, name: &str, value: &str) -> Result<()> {
        if name.is_empty() {
            return Err(Error::InvalidInput("preference key is empty"));
        }
        self.write(|tx|{tx.execute("INSERT INTO local_preferences VALUES(?,?,?) ON CONFLICT(account_id,name) DO UPDATE SET value=excluded.value",params![account,name,value])?;Ok(())})
    }
    pub fn preference(&self, account: AccountId, name: &str) -> Result<Option<String>> {
        self.read(|db| {
            Ok(db
                .query_row(
                    "SELECT value FROM local_preferences WHERE account_id=? AND name=?",
                    params![account, name],
                    |r| r.get(0),
                )
                .optional()?)
        })
    }

    /// Atomically replace extracted candidates after a verified body read. A delayed response cannot
    /// overwrite a newer revision; pass the previously stored revision as the compare-and-swap key.
    pub fn replace_pr_references(
        &self,
        account: AccountId,
        pr: ItemId,
        previous: Option<&str>,
        revision: &str,
        candidates: &[ReferenceCandidate],
    ) -> Result<()> {
        self.write(|tx|{
            if revision.is_empty() || item_kind(tx,account,pr)?!="pull_request" {return Err(Error::InvalidInput("invalid PR reference source"))}
            let current:Option<String>=tx.query_row("SELECT body_revision FROM pr_reference_sets WHERE account_id=? AND pr_id=?",params![account,pr],|r|r.get(0)).optional()?;
            if current.as_deref()!=previous {return Err(Error::Conflict("PR body revision changed"))}
            // Carry forward only a previously verified identical reference and provenance.
            let previous_candidates=read_candidates(tx,account,pr)?;
            tx.execute("INSERT INTO pr_reference_sets VALUES(?,?,?) ON CONFLICT(account_id,pr_id) DO UPDATE SET body_revision=excluded.body_revision",params![account,pr,revision])?;
            tx.execute("DELETE FROM pr_reference_candidates WHERE account_id=? AND pr_id=?",params![account,pr])?;
            for candidate in candidates {
                if candidate.position<0 || candidate.raw.is_empty() || candidate.provenance.is_empty(){return Err(Error::InvalidInput("invalid PR reference candidate"))}
                let verified=candidate.verified.or_else(||{
                    previous_candidates.iter().find(|p|p.raw==candidate.raw && p.provenance==candidate.provenance).and_then(|p|p.verified)
                });
                if let Some((issue,_))=verified && item_kind(tx,account,issue)?!="issue" {return Err(Error::InvalidInput("reference target must be a verified issue"))}
                tx.execute("INSERT INTO pr_reference_candidates VALUES(?,?,?,?,?,?,?)",params![account,pr,candidate.position,candidate.raw,candidate.provenance,verified.map(|v|v.0),verified.map(|v|v.1)])?;
            }
            tx.execute("UPDATE pr_contexts SET body_revision=?3 WHERE account_id=?1 AND pr_id=?2",params![account,pr,revision])?;
            tx.execute("UPDATE pr_contexts AS c SET mode='pr',issue_id=NULL,provenance='' WHERE account_id=?1 AND pr_id=?2 AND mode='issue'
                AND NOT EXISTS(SELECT 1 FROM pr_reference_candidates r WHERE r.account_id=c.account_id AND r.pr_id=c.pr_id AND r.verified_issue=c.issue_id AND r.provenance=c.provenance)",params![account,pr])?;
            let layout:Option<String>=tx.query_row("SELECT layout FROM workspaces WHERE account_id=?",[account],|r|r.get(0)).optional()?;
            if let Some(raw)=layout{
                let mut layout:Workspace=parse(&raw)?;
                for tab in layout.panes.iter_mut().flat_map(|p|&mut p.tabs).filter(|t|t.item==pr) {
                    if let Some(ctx)=&mut tab.view.context {
                        ctx.body_revision=revision.to_owned();
                        if ctx.selected_issue.is_some() && !verified_context(tx,account,pr,ctx)? {ctx.selected_issue=None;ctx.provenance.clear();}
                    }
                }
                write_workspace(tx,account,&layout)?;
            }
            Ok(())
        })
    }
    pub fn pr_references(&self, account: AccountId, pr: ItemId) -> Result<Vec<ReferenceCandidate>> {
        self.read(|db| read_candidates(db, account, pr))
    }
    pub fn save_pr_context(
        &self,
        account: AccountId,
        pr: ItemId,
        context: &PrContext,
    ) -> Result<()> {
        self.write(|tx|{
            check_context(tx,account,pr,context)?;
            tx.execute("INSERT INTO pr_contexts VALUES(?,?,?,?,?,?,?) ON CONFLICT(account_id,pr_id) DO UPDATE SET mode=excluded.mode,issue_id=excluded.issue_id,
                body_revision=excluded.body_revision,provenance=excluded.provenance,context_ratio=excluded.context_ratio",
                params![account,pr,if context.selected_issue.is_some(){"issue"}else{"pr"},context.selected_issue,context.body_revision,context.provenance,context.ratio])?;
            Ok(())
        })
    }
    pub fn pr_context(&self, account: AccountId, pr: ItemId) -> Result<Option<PrContext>> {
        self.read(|db|Ok(db.query_row("SELECT issue_id,body_revision,provenance,context_ratio FROM pr_contexts WHERE account_id=? AND pr_id=?",params![account,pr],
            |r|Ok(PrContext{selected_issue:r.get(0)?,body_revision:r.get(1)?,provenance:r.get(2)?,ratio:r.get(3)?})).optional()?))
    }

    pub fn create_draft(
        &self,
        account: AccountId,
        id: &str,
        item: ItemId,
        kind: DraftKind,
        content: &DraftContent,
    ) -> Result<()> {
        self.write(|tx|{
            super::check_kind(tx,account,item,kind.name())?;
            if id.is_empty(){return Err(Error::InvalidInput("draft identity is empty"))}
            let inserted=tx.execute("INSERT INTO drafts(account_id,id,item_id,kind,body,base,anchor,revision) VALUES(?,?,?,?,?,?,?,1) ON CONFLICT(account_id,id) DO NOTHING",
                params![account,id,item,kind.name(),content.body,content.base,content.anchor])?;
            if inserted==0 {
                let current=read_draft(tx,account,id)?.ok_or(Error::Conflict("missing draft"))?;
                if current.item!=item || current.kind!=kind {return Err(Error::Conflict("draft identity already has another target"))}
            }
            Ok(())
        })
    }
    pub fn draft(&self, account: AccountId, id: &str) -> Result<Option<Draft>> {
        self.read(|db| read_draft(db, account, id))
    }
    /// Editor IDs identify individual editing views; a fresh process must use fresh IDs.
    pub fn acquire_editor(
        &self,
        account: AccountId,
        id: &str,
        editor: &str,
        now: i64,
        until: i64,
    ) -> Result<bool> {
        if editor.is_empty() || until <= now {
            return Err(Error::InvalidInput("invalid editor lease"));
        }
        self.write(|tx| {
            Ok(tx.execute(
                "UPDATE drafts SET editor=?3,editor_until=?5 WHERE account_id=?1 AND id=?2
            AND (editor IS NULL OR editor=?3 OR editor_until<=?4)",
                params![account, id, editor, now, until],
            )? == 1)
        })
    }
    pub fn release_editor(&self, account: AccountId, id: &str, editor: &str) -> Result<bool> {
        self.write(|tx|Ok(tx.execute("UPDATE drafts SET editor=NULL,editor_until=NULL WHERE account_id=? AND id=? AND editor=?",params![account,id,editor])?==1))
    }
    pub fn save_draft(
        &self,
        account: AccountId,
        id: &str,
        editor: &str,
        now: i64,
        revision: i64,
        content: &DraftContent,
    ) -> Result<i64> {
        self.write(|tx|{
            let changed=tx.execute("UPDATE drafts SET body=?6,base=?7,anchor=?8,revision=revision+1 WHERE account_id=?1 AND id=?2 AND editor=?3 AND editor_until>?4 AND revision=?5",
                params![account,id,editor,now,revision,content.body,content.base,content.anchor])?;
            if changed!=1{return Err(Error::Conflict("draft revision or editor lease changed"))}
            Ok(revision+1)
        })
    }
    pub fn delete_draft(
        &self,
        account: AccountId,
        id: &str,
        editor: &str,
        now: i64,
        revision: i64,
    ) -> Result<bool> {
        self.write(|tx|Ok(tx.execute("DELETE FROM drafts WHERE account_id=? AND id=? AND editor=? AND editor_until>? AND revision=?",params![account,id,editor,now,revision])?==1))
    }
}

fn item_kind(db: &rusqlite::Connection, account: AccountId, item: ItemId) -> Result<String> {
    check_item(db, account, item)?;
    Ok(db.query_row("SELECT kind FROM items WHERE id=?", [item], |r| r.get(0))?)
}
fn check_context(
    db: &rusqlite::Connection,
    account: AccountId,
    pr: ItemId,
    context: &PrContext,
) -> Result<()> {
    if item_kind(db, account, pr)? != "pull_request"
        || !(0.0 < context.ratio && context.ratio < 1.0)
    {
        return Err(Error::InvalidInput("invalid PR context"));
    }
    if context.selected_issue.is_some() && !verified_context(db, account, pr, context)? {
        return Err(Error::Conflict(
            "issue is not verified on this PR body revision",
        ));
    }
    Ok(())
}
fn verified_context(
    db: &rusqlite::Connection,
    account: AccountId,
    pr: ItemId,
    context: &PrContext,
) -> Result<bool> {
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM pr_reference_sets s JOIN pr_reference_candidates r USING(account_id,pr_id)
        WHERE s.account_id=?1 AND s.pr_id=?2 AND s.body_revision=?3 AND r.verified_issue=?4 AND r.provenance=?5)",
        params![account,pr,context.body_revision,context.selected_issue,context.provenance],|r|r.get(0))?)
}
fn read_candidates(
    db: &rusqlite::Connection,
    account: AccountId,
    pr: ItemId,
) -> Result<Vec<ReferenceCandidate>> {
    let mut q=db.prepare("SELECT position,raw_reference,provenance,verified_issue,verified_at FROM pr_reference_candidates WHERE account_id=? AND pr_id=? ORDER BY position")?;
    let rows = q.query_map(params![account, pr], |r| {
        Ok(ReferenceCandidate {
            position: r.get(0)?,
            raw: r.get(1)?,
            provenance: r.get(2)?,
            verified: r.get::<_, Option<ItemId>>(3)?.zip(r.get(4)?),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
pub(super) fn read_draft(
    db: &rusqlite::Connection,
    account: AccountId,
    id: &str,
) -> Result<Option<Draft>> {
    let row = db
        .query_row(
            "SELECT item_id,kind,body,base,anchor,revision FROM drafts WHERE account_id=? AND id=?",
            params![account, id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get::<_, String>(1)?,
                    DraftContent {
                        body: r.get(2)?,
                        base: r.get(3)?,
                        anchor: r.get(4)?,
                    },
                    r.get(5)?,
                ))
            },
        )
        .optional()?;
    row.map(|(item, kind, content, revision)| {
        Ok(Draft {
            id: id.into(),
            item,
            kind: DraftKind::decode(&kind)?,
            content,
            revision,
        })
    })
    .transpose()
}
