use super::*;
use gpui_kit::{
    HighlightStyle,
    component::diff::{DiffLineRange, DiffSide, DiffState},
};
use std::{
    sync::{
        Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, ThreadId},
    time::{Duration, Instant},
};
use threadline_storage::{
    pr::{
        PatchEvidence, PrFamily, PrHeader, PrInputPage, PrRecord, PrRevision, PrSection, PrTabView,
    },
    resources::{CommitOid, WalkMode},
};

const LARGE: &str = "src/ac22-large.rs";
const FIRST: &str = "src/ac22-anchor.rs";
const SECOND: &str = "src/ac22-other.rs";
const HELD_TAB: &str = "ac22-held";
const OTHER_TAB: &str = "ac22-other";
const OLD: &str = "before_fixture_revision";
const NEW: &str = "after_fixture_revision";
const ROWS: usize = 1_200;
static PARSE_HOLD: Mutex<Option<Arc<WorkGate>>> = Mutex::new(None);

struct WorkGate {
    foreground: ThreadId,
    entered: AtomicBool,
    discarded: AtomicBool,
    release: Mutex<bool>,
    changed: Condvar,
}

impl WorkGate {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            foreground: thread::current().id(),
            entered: AtomicBool::new(false),
            discarded: AtomicBool::new(false),
            release: Mutex::new(false),
            changed: Condvar::new(),
        })
    }

    fn hold(&self) {
        assert_ne!(thread::current().id(), self.foreground);
        if self.entered.swap(true, Ordering::AcqRel) {
            return;
        }
        let released = self.release.lock().unwrap();
        let (released, _) = self
            .changed
            .wait_timeout_while(released, Duration::from_secs(20), |released| !*released)
            .unwrap();
        assert!(*released, "the fixture did not release its background work");
    }

    fn release(&self) {
        *self.release.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

struct ParseRegistration(Arc<WorkGate>);

impl Drop for ParseRegistration {
    fn drop(&mut self) {
        self.0.release();
        PARSE_HOLD.lock().unwrap().take();
    }
}

pub(crate) fn hold_fixture_parse(path: &str, patch: &str) {
    if path == LARGE && patch.contains(OLD) {
        let gate = PARSE_HOLD.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.hold();
        }
    }
}

pub(crate) fn record_discarded_read(tab: &str) {
    if tab == HELD_TAB
        && let Some(gate) = PARSE_HOLD.lock().unwrap().as_ref()
    {
        gate.discarded.store(true, Ordering::Release);
    }
}

fn cache_revision(setup: &ReaderHarness, head: char, marker: &str, stamp: i64) -> PrRevision {
    let oid = |byte: char| CommitOid::parse(&byte.to_string().repeat(40)).unwrap();
    let revision = setup
        .store
        .record_pr_revision(
            setup.fixture.account,
            setup.fixture.reviewed_pr,
            &oid('a'),
            &oid(head),
            Some(&oid('c')),
            &PrHeader::default(),
            stamp,
        )
        .unwrap();
    let file = |path: &str, rows: usize, patch: String| PrRecord::File {
        path: path.into(),
        previous_path: None,
        status: "modified".into(),
        additions: Some(rows as u64),
        deletions: Some(rows as u64),
        patch: PatchEvidence::Text(patch),
    };
    let mut patch = format!("@@ -1,{ROWS} +1,{ROWS} @@\n");
    use std::fmt::Write as _;
    for row in 1..=ROWS {
        writeln!(patch, "-let old_{row} = {row};").unwrap();
    }
    for row in 1..=ROWS {
        writeln!(patch, "+let {marker}_{row} = {row};").unwrap();
    }
    let walk = setup
        .store
        .start_pr_fetch(&revision, PrFamily::Files, WalkMode::Full)
        .unwrap();
    setup
        .store
        .commit_pr_page(
            &revision,
            PrFamily::Files,
            &walk,
            PrInputPage {
                records: vec![
                    file(FIRST, 1, "@@ -1 +1 @@\n-old\n+let first = true;".into()),
                    file(SECOND, 1, "@@ -1 +1 @@\n-old\n+let other = true;".into()),
                    file(LARGE, ROWS, patch),
                ],
                next_cursor: None,
                validators: None,
                partial: None,
                fetched_at_ms: stamp,
            },
        )
        .unwrap();
    revision
}

fn shown_patch(
    reader: &ReaderWorkspace,
    tab: &str,
    revision: &PrRevision,
    path: &str,
) -> Option<Entity<DiffState>> {
    let pr = reader.tab_views.get(tab)?.pr.as_deref()?;
    if &pr.data.revision != revision
        || pr.data.files.as_ref()?.selected.as_ref()?.summary.path != path
    {
        return None;
    }
    pr.diff.clone()
}

fn choose_file(
    cx: &mut TestAppContext,
    window: gpui_kit::AnyWindowHandle,
    reader: &Entity<ReaderWorkspace>,
    pane: usize,
    tab: &str,
    path: &str,
) {
    let index = cx.read(|cx| {
        reader.read(cx).tab_views[tab]
            .pr
            .as_ref()
            .unwrap()
            .data
            .files
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .position(|file| file.path == path)
            .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(format!("pr-{pane}-file-{index}"), cx);
    })
    .unwrap();
}

fn assert_line(
    cx: &mut TestAppContext,
    diff: &Entity<DiffState>,
    path: &str,
    line: usize,
    text: &str,
) {
    cx.update(|cx| {
        diff.update(cx, |diff, cx| {
            diff.set_selected_lines(
                Some(DiffLineRange::new(
                    path.to_owned(),
                    DiffSide::Modified,
                    line,
                    line,
                )),
                cx,
            );
        });
        assert_eq!(diff.read(cx).selected_text(cx).trim_end(), text);
    });
}

#[gpui_kit::test]
fn diff_background_parse_and_highlighting_leave_native_file_navigation_usable(
    cx: &mut TestAppContext,
) {
    let setup = ReaderHarness::seeded();
    let account = setup.fixture.account;
    let item = setup.fixture.reviewed_pr;
    let stamp = 1_900_000_000_000;
    let original = cache_revision(&setup, 'b', OLD, stamp);
    let pane = |id: &str| Pane {
        active: Some(0),
        tabs: vec![Tab {
            id: id.into(),
            item,
            view: TabView {
                pr_reader: Some(PrTabView {
                    section: PrSection::Files,
                    selected_path: Some(FIRST.into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
        }],
    };
    setup
        .store
        .save_workspace(
            account,
            &Workspace {
                focused_pane: 0,
                proportions: vec![0.5, 0.5],
                panes: vec![pane(HELD_TAB), pane(OTHER_TAB)],
            },
        )
        .unwrap();
    let (window, reader) = setup.open_at(cx, size(px(2100.), px(1100.)));
    let current = |cx: &TestAppContext, tab, revision: &PrRevision, path| {
        cx.read(|cx| shown_patch(reader.read(cx), tab, revision, path))
    };
    until(cx, |cx| {
        current(cx, HELD_TAB, &original, FIRST).is_some()
            && current(cx, OTHER_TAB, &original, FIRST).is_some()
    });

    let parse = ParseRegistration(WorkGate::new());
    *PARSE_HOLD.lock().unwrap() = Some(Arc::clone(&parse.0));
    choose_file(cx, window, &reader, 0, HELD_TAB, LARGE);
    until(cx, |_| parse.0.entered.load(Ordering::Acquire));
    click(cx, window, "reader-tab-1-0");
    until(cx, |cx| {
        cx.read(|cx| {
            reader
                .read(cx)
                .active_tab()
                .is_some_and(|tab| tab.id == OTHER_TAB)
        })
    });
    choose_file(cx, window, &reader, 1, OTHER_TAB, SECOND);
    until(cx, |cx| current(cx, OTHER_TAB, &original, SECOND).is_some());
    assert_line(
        cx,
        &current(cx, OTHER_TAB, &original, SECOND).unwrap(),
        SECOND,
        1,
        "let other = true;",
    );

    let replacement = cache_revision(&setup, 'd', NEW, stamp + 1);
    let connection =
        threadline_core::connectivity::ConnectivityObserver::new(1).snapshot(Duration::ZERO);
    cx.update_window(window, |_, window, cx| {
        reader.update(cx, |reader, cx| {
            reader.update_sync(
                crate::sync_worker::SyncUpdate {
                    account: Some(account),
                    status: None,
                    connection,
                    cache_generation: 1,
                    history_generation: 0,
                    eligible_outbox: 0,
                },
                window,
                cx,
            );
        });
    })
    .unwrap();
    until(cx, |cx| {
        current(cx, HELD_TAB, &replacement, LARGE).is_some()
    });
    click(cx, window, "reader-tab-0-0");
    until(cx, |cx| {
        cx.read(|cx| {
            reader
                .read(cx)
                .active_tab()
                .is_some_and(|tab| tab.id == HELD_TAB)
        })
    });
    let large = current(cx, HELD_TAB, &replacement, LARGE).unwrap();
    parse.0.release();
    until(cx, |_| parse.0.discarded.load(Ordering::Acquire));
    assert_eq!(
        current(cx, HELD_TAB, &replacement, LARGE),
        Some(large.clone())
    );
    assert_line(cx, &large, LARGE, 1, "let after_fixture_revision_1 = 1;");
    assert_line(
        cx,
        &large,
        LARGE,
        ROWS,
        "let after_fixture_revision_1200 = 1200;",
    );
    drop(parse);

    cx.run_until_parked();
    let highlight = WorkGate::new();
    let worker_gate = Arc::clone(&highlight);
    cx.update(|cx| {
        large.update(cx, |diff, cx| {
            diff.set_syntax_highlighter(
                move |language, text| {
                    assert_eq!(language, "rust");
                    worker_gate.hold();
                    text.find("let").map_or_else(Vec::new, |start| {
                        vec![(
                            start..start + 3,
                            HighlightStyle {
                                color: Some(gpui_kit::rgb(0x4878aa).into()),
                                ..Default::default()
                            },
                        )]
                    })
                },
                cx,
            );
        });
    });
    let executor = cx.executor().scheduler_executor();
    let scheduler = executor.scheduler().clone();
    let dispatcher = scheduler.as_test().unwrap();
    while dispatcher.pending_task_counts().1 == 0 {
        assert!(
            dispatcher.tick(),
            "the syntax preparation was not scheduled"
        );
    }
    thread::scope(|scope| {
        struct Release<'a>(&'a WorkGate);
        impl Drop for Release<'_> {
            fn drop(&mut self) {
                self.0.release();
            }
        }
        let _release = Release(&highlight);
        let worker = scope.spawn(move || {
            assert!(scheduler.as_test().unwrap().tick_background_only());
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while !highlight.entered.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        assert!(highlight.entered.load(Ordering::Acquire));
        choose_file(cx, window, &reader, 0, HELD_TAB, FIRST);
        until(cx, |cx| {
            current(cx, HELD_TAB, &replacement, FIRST).is_some()
        });
        let other = current(cx, HELD_TAB, &replacement, FIRST).unwrap();
        assert_ne!(other, large);
        assert_line(cx, &other, FIRST, 1, "let first = true;");
        highlight.release();
        worker.join().unwrap();
    });
    cx.run_until_parked();
    assert!(current(cx, HELD_TAB, &replacement, FIRST).is_some());
    choose_file(cx, window, &reader, 0, HELD_TAB, LARGE);
    until(cx, |cx| {
        current(cx, HELD_TAB, &replacement, LARGE).is_some()
    });
    assert_line(
        cx,
        &current(cx, HELD_TAB, &replacement, LARGE).unwrap(),
        LARGE,
        ROWS,
        "let after_fixture_revision_1200 = 1200;",
    );
}
