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
