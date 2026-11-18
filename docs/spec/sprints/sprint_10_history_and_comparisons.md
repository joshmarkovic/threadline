# Sprint 10: Branch graph and commit comparisons

Requirements version 1.7 | Initial sprint status Planned

## Outcome

Users can navigate remote branch ancestry and compare commits through the same native diff system without cloning the repository.

## Phase and prerequisites

- Release phase: Windows first release.
- Prerequisites: Sprint 05, Sprint 06, Sprint 08.
- Code areas: history, github/refs and commits, storage/commit graph, review/comparison reuse, ui/history, notifications/branch observer.
- Read the master, TESTING_PLAN.md, INSTALLATION_PLAN.md, CROSS_PLATFORM_PLAN.md and all relevant scenario/platform ledgers.
- Use actual prior evidence; a missing dependency remains blocked rather than passed. Later-platform release work does not silently move into the Windows phase.

## Implementation tickets

### S10-T01

Implement remote branch/tag refs, immutable commits, ordered parents, and a native ancestry graph with branch labels and visible merge relationships. Do not invent a permanent hierarchy of branch names.

### S10-T02

Load bounded recent history for the default and selected branches; page older history on demand. Cache records, preserve lanes/scroll/selection, and show unfetched parents as continuation boundaries.

### S10-T03

Open selected-commit file changes and comparisons between two commits. For a merge commit, label the selected parent and allow another available parent. Reuse cached diff rendering and explicit comparison semantics.

### S10-T04

Reconcile moved/deleted refs and force-pushes independently of immutable commits and review drafts. Maintain revision-aware view state and ignore stale graph/comparison work.

### S10-T05

Connect default/chosen branches, branch patterns, and followed PRs to lightweight commit monitoring. Establish per-scope baselines and known new-commit identities without inventing an unavailable push actor.

### S10-T06

Measure lazy-history layout and comparison responsiveness. Record GRAPH-07 as an optional future object-cache optimization; API-based history remains sufficient and no clone is required.

### S10-T07

Preserve graph selection/scroll, immutable commit content, mutable-ref validation, and branch observation baselines through normal exit and same-user PAT renewal. Integrate targeted restart ref checks without redownloading already retained ancestry; contribute these cases to the final lifecycle matrix.

### S10-T08

Add graph/comparison unit and integration fixtures plus native selection, history expansion and comparison navigation E2E. Run cross-target core/build checks and record actual Windows rendering/performance evidence; untested graphics environments remain pending.

## Required tests and commands

- Unit/integration: graph parents/lanes, unloaded boundaries, ref changes and exact comparison identities.
- Native E2E: graph selection, history expansion and single/two-commit comparisons with delayed data and offline restart.
- Regression/performance: large graph/diff behavior and remote-branch baseline detection without a full clone; run common CI matrix.

Implement and use the canonical runner from TESTING_PLAN.md: unit, integration, ui, native e2e smoke/full, install with an artifact, and separately guarded live tests. These are development commands to build, not commands already supplied by this ZIP. Record actual OS/build/fixture evidence and mark unavailable checks Blocked/Not run. Native test-build E2E and exact published-package smoke remain separate.

Scenario IDs introduced or closing here: UT-011, UT-016, E2E-011.

Live-provider cases are shared gates on an explicitly authorized host. A later platform can reference existing evidence only with a recorded applicability assessment; rerun changed provider/credential-policy or target-specific behavior. Missing authorization remains Blocked/Not run and never permits unapproved writes.

Use test_scenarios.csv for fixture IDs, expected results, first/closure ownership and evidence. Release sprints rerun all scenarios applicable to their target; earlier sprint scenario lists do not waive regression checks.

## Deliverables

Native remote-history graph; lazy ancestry cache; commit and two-commit comparisons; ref reconciliation; branch observer; graph performance evidence.

## Exit gates

- Pass AC-24, AC-25, and AC-26 with merges, partial ancestry, moved/deleted refs, and force-push fixtures plus representative live reads.
- Verify PR comparisons, individual commits, and two-commit comparisons retain distinct labels, source identities, options, and cache keys.
- Demonstrate expanding older history without resetting context or losing review drafts. Unloaded parents cannot be shown as proven roots.
- Test watched/unwatched branch observations and historical baselines. End-to-end branch pop-ups remain an acceptance gate in Sprint 11.

## Initial acceptance scenarios owned here

- **AC-24** Loading older graph history preserves ancestry and scroll context; unloaded parents are shown as continuation boundaries.
- **AC-25** Moved or deleted branch labels update correctly without changing immutable commit records or losing drafts.
- **AC-26** PR comparison and two-commit comparison are labeled and use the intended merge-base or endpoint semantics.

## Full owned requirements

### GRAPH-01

Include a native history view with branch labels, commits, parent connections, and visible merge relationships. The initial scope is remote GitHub branches. Branches are movable references to commits, and merge commits can have multiple parents; draw that ancestry rather than inventing permanent parent-child relationships between branch names. [S17]

Initial verification owner: Sprint 10. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### GRAPH-02

Show a bounded recent history for the default branch and selected branches first. Load older commits on scroll or explicit request, and cache them. Display messages, authors, dates, abbreviated commit identities, and relevant branch or tag labels.

Initial verification owner: Sprint 10. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### GRAPH-03

Selecting a commit opens its changed files and diff. Selecting two commits opens a labeled comparison. For merge commits, identify the parent used for a single-commit comparison and permit selection of another available parent.

Initial verification owner: Sprint 10. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### GRAPH-04

Lay out parent relationships consistently, preserve visible lane and scroll stability while expanding history, and mark unloaded parents as continuation boundaries. A missing fetched parent is not proof of a root commit.

Initial verification owner: Sprint 10. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### GRAPH-05

Update moved or deleted branch labels independently from cached immutable commit data. Handle force-pushes without corrupting the graph or discarding review drafts.

Initial verification owner: Sprint 10. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### GRAPH-06

Start with GitHub references and commit APIs. These expose the identities and relationships needed for a focused graph. A full clone must not be required. [S18] [S26]

Initial verification owner: Sprint 10. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### GRAPH-07

A bare Git object cache is an optional future optimization for deep history or complete comparisons. Bare repositories avoid a checkout but do not inherently minimize transfer; partial clone filtering can defer file contents when supported. Any shallow history must expose its incomplete boundary. [S19]

Initial verification owner: Sprint 10. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### DIFF-03

Distinguish PR comparison, individual-commit comparison, and arbitrary two-commit comparison. The default PR view must match the intended GitHub comparison. GitHub PRs use the merge base for a three-dot comparison; endpoint comparisons can differ. Label the comparison and use the correct source identities in the cache. [S16]

Initial verification owner: Sprint 10. Typed comparison model in 02; correct PR comparison in 06; individual/two-commit comparisons complete in 10.

### NOTIFY-04

Commit monitoring must support the default branch, chosen branches, branch patterns, or PRs the user follows. Identify observed new commits accurately. Do not infer who pushed a commit from its author if push-actor information is unavailable.

Initial verification owner: Sprint 11. Branch-scope selection, baselines and accurate observations in 10; grouped delivery verified in 11.

## Integration work owned elsewhere

### PORT-02

Share domain models, GitHub integration, cache/search, synchronization, Outbox, review/diff logic, history, notification rules, and workspace state across platforms. Isolate operating-system APIs behind small capability interfaces with conditional dependencies inside platform implementations. Common code must not depend on Windows handles, registry calls, COM types, fixed Windows paths, or platform-specific credential objects. Reuse portable GPUI facilities where suitable rather than wrapping the entire toolkit in a new framework. Test interface contracts with fakes and actual adapters; do not duplicate the product's business logic for each OS.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce platform boundaries in 01 and keep shared modules portable throughout; verify the full Windows implementation boundary in 12 and parity in later platform rows.

### PORT-03

Prove the actual native renderer and necessary GPUI/GPUI Kit components early with selectable Markdown, text input/copy, large annotated diffs, the bound PR context/code layout, resizing, focus, and a graph fixture. Windows proof is a Sprint 01 gate. Configure compilation/core-test jobs for all intended targets and attempt macOS/Linux native smoke launches early on real target desktops; record gaps honestly and resolve them before that platform's release. In-process/headless tests or a successful cross-compile are not native GUI proof. No phase may add an embedded browser engine as a portability shortcut. [S31] [S32] [S33]

Initial owner: Sprint 01. Initial verification: Sprint 12. Windows native proof begins in 01; expand actual controls as built. Early other-target smoke is tracked separately; full native release evidence closes at each platform gate.

### AUTH-11

Explicit sign-out MUST durably mark the affected account profile as signed out, prevent new network dispatch under that profile, and remove or invalidate its locally stored PAT material. Preserve readable private cached content, drafts, saved workspace, settings, synchronization checkpoints, activity baselines, and outbox records on the PC. Users may reopen that signed-out local profile for offline work without a renewed login; show its signed-out status and hold its remote actions until a PAT for the same verified user restores access. Keep that hold across restart. Ordinary exit is not sign-out. Expired or revoked PATs likewise preserve offline use and progress while holding affected remote work. A sign-out during an in-flight mutation does not prove remote cancellation; retain its evidence for reconciliation after access is restored. Do not expose one profile's data through a different active account or assume sign-out revokes the PAT at GitHub.

Initial owner: Sprint 03. Initial verification: Sprint 12. Signed-out credential/dispatch holds begin in 03; readable local-profile UI in 04 and all-writer PAT repair in 09; complete private-data retention and in-flight lifecycle matrix in 12.

### WORKSPACE-05

Persist view state independently for every tab instance: selected subview, selected file or commit, diff display mode, scroll anchors, expanded discussion or diff regions, and relevant navigation history. For a PR review tab also retain its context mode, selected verified linked issue, context/code proportions, collapsed/focused surface, and separate context scroll anchors. Switching tabs, resizing panes, or refreshing one resource MUST NOT reset another tab's position, selection, or focus. Anchor diff state to its comparison revision and handle unavailable anchors after a changed revision explicitly.

Initial owner: Sprint 04. Initial verification: Sprint 10. Independent state expands from conversations to per-PR context/code modes and linked-issue selections in 06, actual review drafts in 08, and commit comparisons in 10.

### WORKSPACE-08

Save and restore the workspace per account, including pane order and proportions, open tab order, active tabs, focused pane, per-tab view state, scroll position or anchors, navigation context, and draft references. Normal exit MUST retain this state. On restart, render cached visible content before waiting for any API response or renewed login; then start eligible background work through the shared scheduler. Load inactive tabs lazily and expose missing or inaccessible content without discarding its navigation target. Restoring a workspace MUST NOT submit unsubmitted drafts, create duplicate operations, or expose another account's private tabs. The independent outbox worker resumes already submitted eligible actions under OUTBOX-04 and the restored authentication/pause settings; view restoration is not a dispatch trigger.

Initial owner: Sprint 04. Initial verification: Sprint 12. Restore local workspace/drafts before networking in 04; integrate all views and writers as built. Final normal-exit, signed-out, expired-PAT and delayed-network restoration closes in 12. View restoration never submits drafts.

### WORKSPACE-10

Share request scheduling and deduplicate in-flight fetches across views of the same resource and revision. Opening additional panes MUST NOT create independent polling loops or duplicate alerts for that resource. Give visible tabs priority within the existing schedule and rate-limit budget. Inactive tabs must not retain every large diff or continuously redraw; evict reconstructible rendering state while preserving navigation state and drafts. Closing one view must not cancel work still needed by another view or the shared write journal.

Initial owner: Sprint 05. Initial verification: Sprint 12. Shared scheduling in 05 and large-view retention in 06/10; complete integrated twenty-tab performance gate in 12.

### CACHE-06

Retain fetched text, review data, commit metadata, and diffs across restarts. Do not impose an undocumented age cutoff. Bound memory and render state independently of disk retention. Expose per-repository usage and explicit cleanup controls.

Initial owner: Sprint 04. Initial verification: Sprint 12. Disk retention/cleanup in 04; integrate all content types and verify bounded memory on full workload in 12.

### CACHE-09

Associate comparison data with immutable commit identities and comparison options. Associate mutable branch labels, status checks, and review state with their relevant revisions and timestamps. A branch name alone is not an adequate diff cache key.

Initial owner: Sprint 02. Initial verification: Sprint 10. Typed immutable comparison identity in 02; actual PR/review and repository comparison integration later.

### CACHE-13

Normal window close and application exit MUST retain downloaded content and resource coverage, all draft types, saved tabs/layout/view positions, local preferences, offline pins, pending actions and receipts, synchronization checkpoints/validators, and notification baselines/delivery history on the PC. Persist settings and user changes as they occur and safely flush remaining local state on orderly exit; if a user-data write fails, keep it recoverable and report the failure. Retain the protected credential according to AUTH-07. Restart MUST reuse these records and render the saved local workspace before networking completes. It must not clear the cache, reset baselines, or redownload all history merely because the process was restarted. Sign-out preserves local content under AUTH-11. Explicit cache cleanup remains a separate action and continues to protect drafts and unresolved work under CACHE-07 and OUTBOX-10.

Initial owner: Sprint 02. Initial verification: Sprint 12. Complete persistence schema in 02, PAT lifecycle in 03, local-first restoration in 04, sync in 05/06, all writer/history/notification types through 11, and the complete quit/sign-out/reopen matrix in 12.

### OFFLINE-01

Once an account and content have been established, the application MUST start and remain usable without a network check or renewed login blocking local access. Users can browse/search downloaded issues, PRs, discussions, reviews, commits, diffs, graph history, and the activity feed; use tabs and panes; edit local settings and drafts; and inspect or cancel pending work. State the cached coverage and last fetch honestly. Unseen remote content and new server events cannot be available until fetched. First-time authentication and initial acquisition of private content still require access; do not require a full repository download.

Initial owner: Sprint 04. Initial verification: Sprint 12. Offline startup/workspace in 04; each new view and action integrates as built; complete all-feature cold-start offline matrix in 12.

### SYNC-09

Run synchronization, connectivity checks, and the outbox worker in the application or optional tray process. A fully exited application or sleeping machine cannot perform local polling or send actions. Normal exit preserves local state and the protected PAT; it does not sign the user out. On launch or wake, restore the local workspace and persisted control settings first, then resume eligible actions and bounded incremental read catch-up under SYNC-11 when connectivity and authorization permit. No hosted service is required for the initial release.

Initial owner: Sprint 05. Initial verification: Sprint 12. Lifecycle scheduling begins in 05, all writers integrate by 09, graph/feed families by 11, and retained-credential plus local-first incremental restart acceptance closes in 12.

### SYNC-11

Resume from persisted per-account, per-repository, per-resource-family synchronization state on restart, reconnection, and same-user PAT renewal. Retain the last successful checkpoint, overlapping update window or supported cursor, pagination progress, conditional-request validators, resource completeness/freshness, and notification baselines. Advance content and its successful progress transactionally after each completed unit; failures or partial pages must not skip unseen changes. Prioritize visible resources and run bounded, rate-aware incremental catch-up, using supported changed-since/update feeds, conditional reads, and targeted reconciliation. Do not blindly redownload the entire cache, re-enumerate every historical discussion, reset notification baselines, or use the largest issue/PR number as the only cursor. If a cursor expires or an endpoint has no reliable delta interface, re-enumerate the necessary resource/range with stable-ID deduplication and preserved coverage while keeping other completed families intact. An update-feed page one, overlap, unchanged revalidation, or targeted repair may be necessary; no universal exact-delta or zero-unchanged-bytes guarantee is made. Preserve old-item update discovery and honor restored sign-out, offline, and pause state. [S06] [S07] [S11] [S13]

Initial owner: Sprint 05. Initial verification: Sprint 12. Persist checkpoints and stable account identity in 02/03; issue/comment restart catch-up in 05 and PR-family AC-75/76 in 06; all graph/notification families and full lifecycle request traces complete in 12.

### NOTIFY-02

Activity monitoring MUST run independently of demand-loaded full content. Enabled events must be detectable on selected repositories even for issues and PRs the user has never opened. Monitoring must not require a complete historical download.

Initial owner: Sprint 05. Initial verification: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-13

Establish a baseline for each repository and event category when monitoring begins. Downloading old content for the first time MUST NOT itself create a new-activity notification. A genuinely new comment on an old unopened issue may still qualify based on its trustworthy event identity and timestamp.

Initial owner: Sprint 05. Initial verification: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-14

Where no trustworthy earlier state or event time exists, establish state without inventing a transition. Deduplicate events across polling endpoints and multiple notification categories. A comment that mentions the user should not produce duplicate comment and mention banners by default.

Initial owner: Sprint 05. Initial verification: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-15

Persist event keys, eligibility decisions, grouping keys, and delivery state. Reuse stable native notification identities where supported and report unsupported replacement/retraction capabilities truthfully. Prevent ordinary duplicate delivery across retries and restart; do not claim an unconditional exactly-once guarantee across application crashes and OS delivery.

Initial owner: Sprint 05. Initial verification: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### DATA-03

Keep secrets and private content out of routine diagnostic output. Export diagnostics only through an explicit user action with appropriate redaction. Treat remote Markdown, links, and filenames as data rather than instructions or executable paths.

Initial owner: Sprint 02. Initial verification: Sprint 12. Establish redaction/content safety boundaries early; verify all parsers and final explicit diagnostic export in 12.

### UX-01

Support platform-appropriate keyboard navigation, focus indication, copy and paste, high-DPI displays, light and dark appearance, and sensible resizing. Test real text editors and review controls with the accessibility features used by the target audience on each published OS. Windows is verified first; macOS/Linux parity closes in their release phases.

Initial owner: Sprint 04. Initial verification: Sprint 12. Keyboard, accessibility, DPI and appearance checked as native controls arrive; integrated release verification in 12.

### UX-02

Preserve user context while refreshing. Avoid replacing a useful cached view with a global spinner, jumping scroll positions, or losing a draft when selecting another tab. Loading and errors belong to the affected resource or pane; a slow request in one pane must not block navigation or editing in another.

Initial owner: Sprint 04. Initial verification: Sprint 10. Context-preserving loading/error behavior must hold through all reader, writer, diff and history views.

### TEST-01

Implement a documented development test runner and relevant tests in every sprint, not only at release. The required command contract is cargo xtask test unit; cargo xtask test integration; cargo xtask test ui; cargo xtask test e2e --suite smoke or --suite full; cargo xtask test install --artifact <path>; and cargo xtask test live --repo OWNER/REPO with an explicit --allow-live-writes opt-in for approved mutations. Implement these commands or a documented equivalent wrapper before claiming their gates passed. They are planned commands, not tools already provided by this requirements ZIP. Each command returns a meaningful exit status, reports selected/skipped/blocked tests and environment, and writes machine-readable results plus concise logs. End users install prebuilt binaries and need none of this development tooling.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### TEST-02

Unit tests MUST cover nontrivial domain and state-machine decisions introduced by each sprint: shared item identities, absent-versus-null updates, reference parsing/type guards, completeness, schedule/DST rules, notification precedence, draft ownership, dependencies, cancellation, retry eligibility, conflict/revision checks and diff source anchors. Use controlled clocks, seeded fixtures and small independently checkable expectations. Test meaningful boundaries and regressions rather than mirroring implementation lines or pursuing an arbitrary coverage percentage. Add the relevant tests with the feature; future feature tests remain pending.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### TEST-03

Integration tests MUST exercise real SQLite databases/migrations/FTS/transactions and actual provider serialization/HTTP paths against a controllable local API server, not only mocked repository methods. Cover pagination/resumption, SSO/permissions/partial data, rate limits, cache publication, persisted checkpoints, signed-out identity holds, Outbox claiming/reconciliation, and upgrade cleanup protection. Verify production interfaces with deterministic simulated remote responses and OS adapter contract tests; use real protected-store/notification integration checks on the applicable native host. Keep live GitHub tests separate from deterministic integration suites.

Initial owner: Sprint 02. Initial verification: Sprint 12. Real SQLite integration starts in 02 and actual HTTP adapter integration in 03; later modules add cases, with full Windows integration release evidence in 12.

### TEST-04

End-to-end tests MUST launch and drive the actual native application process through its user-visible controls using supported native automation/accessibility/input, with real local persistence and the implemented HTTP adapter connected to a deterministic test server where appropriate. Start with launch/focus/quit smoke coverage in Sprint 01 and expand to cache-first reading, tab/context switching, offline drafts, queued submissions/cancellation/reconnect, review/merge revision guards, notifications and restoration as those features arrive. Screenshots, GPUI in-process UI tests, callbacks, domain function calls, or a web browser test are not substitutes for native-process E2E. Keep GPUI UI integration tests as an additional layer. Distinguish simulator/test-build native E2E from smoke and release checks of the exact installed published package; the former does not prove packaging, release feature flags or publisher trust. A native automation gap is a visible blocked gate, not a reason to relabel a unit test E2E; record reproducible manual evidence for any inherently manual OS prompt and automate the remaining journey. [S32]

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### TEST-05

Provide isolated reproducible fixtures and fault injection for delayed/out-of-order responses, partial pagination, 401/403/404/429/5xx, offline/online changes, sleep/wake, interrupted writes/migrations, full or unwritable storage, process crash before/after dispatch, timeout after remote success, cancelled predecessor chains, changed PR heads and removed links. Exercise exact data/state invariants and request traces, including no blind replay or retargeting. Use temporary per-test profiles, synthetic secrets/data and controlled test servers; reset deterministically without accessing a user's profile or emitting real notifications/writes accidentally. Preserve failure artifacts with secret redaction.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### TEST-06

Configure native-target build and portable-core unit/integration CI jobs for Windows, macOS and Linux from Sprint 01, adding actual suites as modules become available. Native Windows smoke E2E is required as part of each relevant Sprint 01-12 increment; full Windows suites gate Sprint 12. Attempt and record early native macOS/Linux smoke evidence, then require complete applicable native/E2E/install/performance coverage at Sprints 13/14. A later-platform runner or graphics limitation remains visibly Blocked/Not run for that target and must be resolved before publishing it. It does not silently prevent an otherwise complete Windows phase, while a discovered defect in shared correctness still requires repair. Headless rendering, cross-compilation and fake providers must be labeled accurately.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### TEST-08

A sprint report MUST list exact commands, commit/build identity, OS/version/architecture, fixture/seed, test layer and scope, results, failure artifacts, limitations, and requirement/AC/scenario links. Distinguish Pass, Fail, Blocked and Not run; skipped or mocked behavior never counts as a live/native pass. Maintain requirement, acceptance and scenario evidence per release platform, identifying simulator/test builds separately from the exact installed published package and its digest/signature. Every phase gate requires applicable unit/integration/native E2E/install tests, published-package smoke evidence, and targeted performance/accessibility evidence with no unexplained required failures. Rerun affected regression tests when shared code changes; do not claim all-platform completion from a Windows report or from validation of this planning package.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

## Verification due for this phase

WORKSPACE-05, CACHE-09, DIFF-03, GRAPH-01, GRAPH-02, GRAPH-03, GRAPH-04, GRAPH-05, GRAPH-06, GRAPH-07, UX-02.

Use requirement_platform_traceability.csv for applicability and release-gate evidence. Initial ledger statuses refer only to their initial phase; no single Windows result closes all platforms. Keep future options and unavailable services explicit.

## Later scope and handoff

A bare object cache, local working-tree management, and mandatory cloning are outside the initial scope. Sprint 11 adds rules, grouping, and native delivery for branch observations.

## Coding LLM instruction

Implement Sprint 10 against v1.7. Keep the app buildable and preserve the current product constraints. Complete listed implementation and test work, use actual native controls for E2E, protect original targets/drafts/Outbox state, and add only the named platform integrations. Do not add issue/PR creation, merge queues or browser engines. Keep secrets out of code/results; live writes require explicit allowlisted authorization. Implement the short verified prebuilt install/update contract for the relevant publication phase and never claim placeholder commands runnable. Update initial/per-platform/scenario ledgers with commands, environment, package identity and evidence, then use SPRINT_REPORT_TEMPLATE.md. Continue only within the assigned sprint/Windows/all-platform scope without new routine approvals.

## Technical sources

- [S06 GitHub issue and PR conversation comments](https://docs.github.com/en/rest/issues/comments)
- [S07 GitHub REST pagination](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api)
- [S11 GitHub REST best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)
- [S13 GitHub inline review comments](https://docs.github.com/en/rest/pulls/comments)
- [S16 GitHub branch comparison semantics](https://docs.github.com/en/pull-requests/reference/branches)
- [S17 Git branch and commit relationships](https://git-scm.com/book/en/v2/Git-Branching-Branches-in-a-Nutshell)
- [S18 GitHub commit history and parent identities](https://docs.github.com/en/rest/commits/commits)
- [S19 Git clone and partial clone behavior](https://git-scm.com/docs/git-clone)
- [S26 GitHub branch and tag references](https://docs.github.com/en/rest/git/refs)
- [S31 GPUI Kit installation and target prerequisites](https://gpui-kit.com/docs/installation/)
- [S32 GPUI Kit testing facilities and limits](https://gpui-kit.com/docs/test/)
- [S33 GPUI Kit accessibility](https://gpui-kit.com/docs/accessibility/)

Additional current primary sources and platform/install/test constraints are in the three companion plans. Verify pinned tool/API behavior during implementation.
