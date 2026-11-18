use threadline_storage as storage;
#[path = "support/write_profile.rs"]
mod write_profile;
use threadline_storage::{access::AccessProfile, history::*, resources::*, *};
use write_profile::setup;
fn oid(c: char) -> CommitOid {
    CommitOid::parse(&c.to_string().repeat(40)).unwrap()
}
fn commit(c: char, parents: &[char]) -> HistoryEntry {
    HistoryEntry {
        commit: GraphCommit {
            oid: oid(c),
            title: format!("commit {c}"),
            committed_at_ms: (c as u32).into(),
            parents: parents.iter().map(|c| oid(*c)).collect(),
        },
        message: format!("commit {c}\n\nbody"),
        author: Some("commit author".into()),
        authored_at_ms: Some(12),
    }
}
fn main_ref() -> HistoryRef {
    HistoryRef::Branch("main".into())
}
fn select(s: &Store, a: AccountId, r: RepositoryId) -> AccessProfile {
    let p = s.github_access_profile(a, "work").unwrap().unwrap();
    s.set_history_monitoring(
        a,
        r,
        &HistoryMonitoring {
            default_branch: Some("main".into()),
            branch_patterns: ["release/*".into()].into_iter().collect(),
            ..Default::default()
        },
    )
    .unwrap();
    p
}
#[test]
fn it_006_history_checkpoints_layout_boundaries_and_exact_parents_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (a, r, _) = setup(&s);
    let p = select(&s, a, r);
    assert_eq!(
        s.observe_history_ref(&p, r, &main_ref(), Some(&oid('f')), 10)
            .unwrap(),
        RefMovement::Baseline
    );
    let tag = HistoryRef::Tag("v1".into());
    s.observe_history_ref(&p, r, &tag, Some(&oid('f')), 11)
        .unwrap();
    let walk = s.begin_history_page(&p, r, &main_ref()).unwrap().unwrap();
    s.commit_history_page(
        &walk,
        &[commit('e', &['c']), commit('f', &['d', 'e'])],
        Some("page-2"),
        false,
        12,
    )
    .unwrap();
    assert!(s.commit_history_page(&walk, &[], None, true, 13).is_err());
    let first = s.history_page(a, r, &oid('f'), None, 32).unwrap();
    assert_eq!(first.coverage, HistoryCoverage::Partial);
    assert_eq!(
        first
            .rows
            .iter()
            .map(|row| row.entry.commit.oid.clone())
            .collect::<Vec<_>>(),
        [oid('f'), oid('e')]
    );
    assert_eq!(
        first.rows[0]
            .parents
            .iter()
            .map(|p| (p.lane, p.loaded))
            .collect::<Vec<_>>(),
        [(0, false), (1, true)]
    );
    assert_eq!(first.rows[0].labels.len(), 2);
    let opts = ComparisonOptions {
        ignore_whitespace: false,
        context_lines: 3,
    };
    assert!(matches!(
        s.comparison_for_parent(a, r, &oid('f'), Some(&oid('e')), opts)
            .unwrap(),
        HistoryComparison::Parent {
            parent_index: 1,
            parent_count: 2,
            ..
        }
    ));
    assert!(
        s.comparison_for_parent(a, r, &oid('f'), Some(&oid('a')), opts)
            .is_err()
    );
    let view = HistoryView {
        reference: main_ref(),
        tip: oid('f'),
        selected: Some(oid('f')),
        second: Some(oid('e')),
        parent: Some(oid('e')),
        scroll_anchor: Some(oid('e')),
        scroll_offset: 7,
        file: Some("src/lib.rs".into()),
        file_scroll: 12,
        ignore_whitespace: false,
        context_lines: 3,
    };
    assert_eq!(
        s.save_history_view(a, r, "view-one", None, &view).unwrap(),
        1
    );
    assert!(s.save_history_view(a, r, "view-one", None, &view).is_err());
    drop(s);
    let s = Store::open_profile(dir.path()).unwrap();
    assert_eq!(
        s.history_view(a, r, "view-one").unwrap().unwrap().state,
        view
    );
    let next = s.begin_history_page(&p, r, &main_ref()).unwrap().unwrap();
    assert_eq!(next.cursor.as_deref(), Some("page-2"));
    assert!(
        s.commit_history_page(&next, &[], Some("page-2"), false, 13)
            .is_err()
    );
    s.commit_history_page(
        &next,
        &[commit('c', &[]), commit('d', &['c'])],
        None,
        true,
        14,
    )
    .unwrap();
    let graph = s.history_page(a, r, &oid('f'), None, 2).unwrap();
    assert_eq!(graph.coverage, HistoryCoverage::Complete);
    assert_eq!(graph.next_ordinal, Some(1));
    assert_eq!(graph.rows[0].parents[0].lane, 0);
    assert!(graph.rows[0].parents.iter().all(|p| p.loaded));
    assert_eq!(
        s.history_page(a, r, &oid('f'), Some(1), 20)
            .unwrap()
            .rows
            .len(),
        2
    );
    assert!(matches!(
        s.comparison_for_parent(a, r, &oid('c'), None, opts)
            .unwrap(),
        HistoryComparison::Root { .. }
    ));
    assert!(s.begin_history_page(&p, r, &tag).unwrap().is_none());
    s.select_github_repository(a, r, false).unwrap();
    assert!(s.begin_history_page(&p, r, &main_ref()).is_err());
    assert_eq!(
        s.history_entry(a, r, &oid('c')).unwrap().unwrap().message,
        "commit c\n\nbody"
    );
    let outsider = s.ensure_account("github.com", 99).unwrap();
    assert!(s.history_page(outsider, r, &oid('f'), None, 20).is_err());
}
#[test]
fn it_006_branch_baseline_proves_new_commits_after_hydration_and_force_push_preserves_old_view() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (a, r, _) = setup(&s);
    let p = select(&s, a, r);
    let branch = HistoryRef::Branch("release/one".into());
    assert_eq!(
        s.observe_history_ref(&p, r, &branch, Some(&oid('a')), 10)
            .unwrap(),
        RefMovement::Baseline
    );
    let original = s.begin_history_page(&p, r, &branch).unwrap().unwrap();
    s.commit_history_page(&original, &[commit('a', &[])], None, true, 11)
        .unwrap();
    let event = |which: char| format!("commit:{}:{}", r.0, oid(which).as_str());
    assert!(s.load_activity_event(a, &event('a')).unwrap().is_none());
    assert_eq!(
        s.observe_history_ref(&p, r, &branch, Some(&oid('c')), 12)
            .unwrap(),
        RefMovement::Unknown
    );
    let c = s.begin_history_page(&p, r, &branch).unwrap().unwrap();
    s.commit_history_page(
        &c,
        &[commit('c', &['b']), commit('b', &['a'])],
        Some("redundant-page"),
        false,
        13,
    )
    .unwrap();
    assert_eq!(
        s.history_ref_moves(a, r, 0, 9).unwrap()[0].movement,
        RefMovement::FastForward
    );
    for n in ['b', 'c'] {
        let e = s.load_activity_event(a, &event(n)).unwrap().unwrap();
        assert_eq!(e.kind, "branch_commit_observed");
    }
    assert!(s.begin_history_page(&p, r, &branch).unwrap().is_none());
    assert_eq!(
        s.observe_history_ref(&p, r, &branch, Some(&oid('a')), 14)
            .unwrap(),
        RefMovement::Rewind
    );
    assert_eq!(
        s.observe_history_ref(&p, r, &branch, Some(&oid('d')), 15)
            .unwrap(),
        RefMovement::Unknown
    );
    let stale = s.begin_history_page(&p, r, &branch).unwrap().unwrap();
    s.observe_history_ref(&p, r, &branch, None, 16).unwrap();
    assert!(
        s.commit_history_page(&stale, &[commit('d', &[])], None, true, 17)
            .is_err()
    );
    assert!(s.history_ref(a, r, &branch).unwrap().unwrap().tip.is_none());
    assert_eq!(
        s.history_ref(a, r, &branch)
            .unwrap()
            .unwrap()
            .observed_at_ms,
        16
    );
    assert_eq!(
        s.observe_history_ref(&p, r, &branch, Some(&oid('c')), 14)
            .unwrap(),
        RefMovement::Stale
    );
    assert!(s.history_page(a, r, &oid('c'), None, 8).unwrap().rows.len() >= 3);
    assert_eq!(
        s.observe_history_ref(&p, r, &branch, Some(&oid('d')), 18)
            .unwrap(),
        RefMovement::Created
    );
    let d = s.begin_history_page(&p, r, &branch).unwrap().unwrap();
    s.commit_history_page(&d, &[commit('d', &[])], None, true, 19)
        .unwrap();
    assert!(s.load_activity_event(a, &event('d')).unwrap().is_none());
    assert!(
        s.history_ref_moves(a, r, 0, 20)
            .unwrap()
            .iter()
            .any(|m| m.movement == RefMovement::Diverged)
    );
}

use threadline_storage::pr::{PatchEvidence, PrRecord, SourceSide};

const PATCH: &str = "@@ -8,2 +12,2 @@\n same\n-old\n+new";
fn file(path: &str, previous: Option<&str>, patch: PatchEvidence) -> PrRecord {
    PrRecord::File {
        path: path.to_owned(),
        previous_path: previous.map(str::to_owned),
        status: if previous.is_some() {
            "renamed"
        } else {
            "modified"
        }
        .into(),
        additions: Some(1),
        deletions: Some(1),
        patch,
    }
}
fn options() -> ComparisonOptions {
    ComparisonOptions {
        ignore_whitespace: false,
        context_lines: 3,
    }
}
fn graph(s: &Store) -> (AccountId, RepositoryId, AccessProfile) {
    let (a, r, _) = setup(s);
    let p = select(s, a, r);
    s.observe_history_ref(&p, r, &main_ref(), Some(&oid('f')), 10)
        .unwrap();
    let w = s.begin_history_page(&p, r, &main_ref()).unwrap().unwrap();
    s.commit_history_page(
        &w,
        &[
            commit('f', &['d', 'e']),
            commit('e', &['c']),
            commit('d', &['c']),
            commit('c', &[]),
        ],
        None,
        true,
        11,
    )
    .unwrap();
    (a, r, p)
}
#[test]
fn it_006_comparison_exact_options_ordered_parent_and_rename_source_survive_offline_restart() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (a, r, p) = graph(&s);
    let direct = s
        .comparison_for_parent(a, r, &oid('f'), Some(&oid('e')), options())
        .unwrap();
    let id = direct.diff_identity();
    let w = s.begin_history_diff(&p, &direct).unwrap().unwrap();
    let new = file("new.rs", Some("old.rs"), PatchEvidence::Text(PATCH.into()));
    let send = |identity, base, files, at| HistoryDiffInput {
        identity,
        effective_base: base,
        files,
        next_cursor: None,
        complete: true,
        observed_at_ms: at,
    };
    let wrong = HistoryDiffIdentity {
        options: ComparisonOptions {
            ignore_whitespace: true,
            ..options()
        },
        ..id.clone()
    };
    let left = oid('e');
    assert!(
        s.commit_history_diff_page(&w, send(&wrong, Some(&left), vec![new.clone()], 12))
            .is_err()
    );
    let other_parent = oid('d');
    assert!(
        s.commit_history_diff_page(&w, send(&id, Some(&other_parent), vec![], 12))
            .is_err()
    );
    s.commit_history_diff_page(
        &w,
        HistoryDiffInput {
            identity: &id,
            effective_base: Some(&left),
            files: vec![new],
            next_cursor: Some("second"),
            complete: false,
            observed_at_ms: 13,
        },
    )
    .unwrap();
    assert!(
        s.commit_history_diff_page(&w, send(&id, Some(&left), vec![], 14))
            .is_err()
    );
    drop(s);
    let s = Store::open_profile(dir.path()).unwrap();
    let w = s.begin_history_diff(&p, &direct).unwrap().unwrap();
    assert_eq!(w.cursor.as_deref(), Some("second"));
    let binary = file("deep/picture.png", None, PatchEvidence::Binary);
    s.commit_history_diff_page(&w, send(&id, Some(&left), vec![binary.clone()], 15))
        .unwrap();
    let HistoryComparison::Parent { key, .. } = &direct else {
        panic!("merge parent")
    };
    let old = s.load_comparison(a, key).unwrap().unwrap();
    assert_eq!(
        old[0],
        ComparisonFile {
            path: "new.rs".into(),
            previous_path: Some("old.rs".into()),
            patch: Some(PATCH.into())
        }
    );
    assert_eq!(old[1].patch, None);
    s.select_github_repository(a, r, false).unwrap();
    let page = s.history_diff_page(a, &direct, None, 1).unwrap();
    assert_eq!(page.coverage, HistoryCoverage::Complete);
    assert_eq!(page.next_ordinal, Some(0));
    let l = page.source_at(0, SourceSide::Left, 3).unwrap();
    assert_eq!((l.oid, l.path, l.line), (oid('e'), "old.rs".to_owned(), 9));
    let rline = page.source_at(0, SourceSide::Right, 4).unwrap();
    assert_eq!(
        (rline.oid, rline.path, rline.line),
        (oid('f'), "new.rs".to_owned(), 13)
    );
    assert!(page.source_at(0, SourceSide::Right, 3).is_none());
    let target = s
        .history_diff_page_at_file(a, &direct, "deep/picture.png", 1)
        .unwrap();
    assert_eq!(target.files[0].record, binary);
    assert!(target.source_at(1, SourceSide::Left, 2).is_none());
    assert!(
        s.history_diff_page_at_file(a, &direct, "missing.rs", 1)
            .unwrap()
            .files
            .is_empty()
    );
    let outsider = s.ensure_account("github.com", 909).unwrap();
    assert!(s.history_diff_page(outsider, &direct, None, 2).is_err());
}
#[test]
fn it_006_root_merge_base_and_limited_patch_recovery_preserve_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (a, r, p) = graph(&s);
    let root = s
        .comparison_for_parent(a, r, &oid('c'), None, options())
        .unwrap();
    let root_walk = s.begin_history_diff(&p, &root).unwrap().unwrap();
    s.commit_history_diff_page(
        &root_walk,
        HistoryDiffInput {
            identity: &root.diff_identity(),
            effective_base: None,
            files: vec![],
            next_cursor: None,
            complete: true,
            observed_at_ms: 12,
        },
    )
    .unwrap();
    let root_page = s.history_diff_page(a, &root, None, 4).unwrap();
    assert_eq!(root_page.coverage, HistoryCoverage::Complete);
    assert_eq!(root_page.identity.semantics, DiffSemantics::Root);
    assert!(root_page.identity.base.is_none());
    let pair = s
        .comparison_between(
            a,
            r,
            &oid('d'),
            &oid('e'),
            ComparisonMode::MergeBase,
            options(),
        )
        .unwrap();
    let reversed = s
        .comparison_between(
            a,
            r,
            &oid('e'),
            &oid('d'),
            ComparisonMode::MergeBase,
            options(),
        )
        .unwrap();
    let w = s.begin_history_diff(&p, &pair).unwrap().unwrap();
    let id = pair.diff_identity();
    let inp = |identity, base, patch, complete, at| HistoryDiffInput {
        identity,
        effective_base: base,
        files: vec![file("new", Some("old"), patch)],
        next_cursor: None,
        complete,
        observed_at_ms: at,
    };
    let reverse_id = reversed.diff_identity();
    assert!(
        s.commit_history_diff_page(
            &w,
            inp(&reverse_id, None, PatchEvidence::ApiCapped, false, 13)
        )
        .is_err()
    );
    s.commit_history_diff_page(
        &w,
        inp(
            &id,
            None,
            PatchEvidence::Truncated("@@ -8,2 +12,2 @@\n same".into()),
            false,
            13,
        ),
    )
    .unwrap();
    let limited = s.history_diff_page(a, &pair, None, 10).unwrap();
    assert_eq!(limited.coverage, HistoryCoverage::Limited);
    assert!(limited.source_at(0, SourceSide::Left, 2).is_none());
    assert_eq!(
        limited.source_at(0, SourceSide::Right, 2).unwrap().oid,
        oid('e')
    );
    assert!(s.begin_history_diff(&p, &pair).unwrap().is_none());
    assert!(s.retry_limited_history_diff(&p, &pair).unwrap());
    let restart = s.begin_history_diff(&p, &pair).unwrap().unwrap();
    assert!(
        s.commit_history_diff_page(&w, inp(&id, None, PatchEvidence::Binary, true, 14))
            .is_err()
    );
    let base = oid('c');
    assert!(
        s.commit_history_diff_page(
            &restart,
            inp(&id, Some(&base), PatchEvidence::Binary, true, 14)
        )
        .is_err()
    );
    s.commit_history_diff_page(
        &restart,
        inp(
            &id,
            Some(&base),
            PatchEvidence::Text(PATCH.into()),
            true,
            14,
        ),
    )
    .unwrap();
    let page = s.history_diff_page(a, &pair, None, 10).unwrap();
    assert_eq!(page.source_at(0, SourceSide::Left, 3).unwrap().oid, base);
    assert_eq!(page.files.len(), 1);
    assert_eq!(
        s.history_diff_page(a, &reversed, None, 1).unwrap().coverage,
        HistoryCoverage::Missing
    );
}

#[test]
fn it_006_limited_ancestry_retries_without_changing_cached_row_ordinals_and_legacy_diffs_remain_downloaded()
 {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open_profile(dir.path()).unwrap();
    let (a, r, _) = setup(&s);
    let p = select(&s, a, r);
    s.observe_history_ref(&p, r, &main_ref(), Some(&oid('b')), 1)
        .unwrap();
    let first = s.begin_history_page(&p, r, &main_ref()).unwrap().unwrap();
    s.commit_history_page(&first, &[commit('b', &['a'])], None, false, 2)
        .unwrap();
    let limited = s.history_page(a, r, &oid('b'), None, 10).unwrap();
    assert_eq!(limited.coverage, HistoryCoverage::Limited);
    assert!(s.retry_limited_history(&p, r, &main_ref()).unwrap());
    let second = s.begin_history_page(&p, r, &main_ref()).unwrap().unwrap();
    assert!(
        s.commit_history_page(&first, &[commit('a', &[])], None, true, 3)
            .is_err()
    );
    s.commit_history_page(
        &second,
        &[commit('b', &['a']), commit('a', &[])],
        None,
        true,
        3,
    )
    .unwrap();
    let full = s.history_page(a, r, &oid('b'), None, 10).unwrap();
    assert_eq!(full.coverage, HistoryCoverage::Complete);
    assert_eq!(
        (full.rows[0].lane, full.rows[0].ordinal),
        (limited.rows[0].lane, limited.rows[0].ordinal)
    );
    let parent = s
        .comparison_for_parent(a, r, &oid('b'), Some(&oid('a')), options())
        .unwrap();
    let HistoryComparison::Parent { key, .. } = &parent else {
        panic!("first parent expected")
    };
    s.save_comparison(
        a,
        key,
        &[ComparisonFile {
            path: "retained".into(),
            previous_path: None,
            patch: None,
        }],
    )
    .unwrap();
    let old = s
        .history_diff_page_at_file(a, &parent, "retained", 1)
        .unwrap();
    assert_eq!(old.coverage, HistoryCoverage::Partial);
    assert!(matches!(
        old.files[0].record,
        PrRecord::File {
            patch: PatchEvidence::Unavailable,
            ..
        }
    ));
    let foreign = HistoryDiffIdentity {
        repository: RepositoryId(r.0 + 1),
        ..parent.diff_identity()
    };
    let w = s.begin_history_diff(&p, &parent).unwrap().unwrap();
    assert!(
        s.commit_history_diff_page(
            &w,
            HistoryDiffInput {
                identity: &foreign,
                effective_base: Some(&oid('a')),
                files: vec![],
                next_cursor: None,
                complete: true,
                observed_at_ms: 4
            }
        )
        .is_err()
    );
    assert_eq!(s.history_diff_page(a, &parent, None, 1).unwrap(), old);
}
