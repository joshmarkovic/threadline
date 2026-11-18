# Sprint 06: PR context, commits, files, checks, and native diffs

Requirements version 1.7 | Initial sprint status Planned

## Outcome

A PR review tab presents prominent code beside one smaller body/verified-issue context pane, with cached discussions, reviews, commits, files, and checks.

## Phase and prerequisites

- Release phase: Windows first release.
- Prerequisites: Sprint 04, Sprint 05.
- Code areas: github/PR resource families, review/comparisons and source mapping, storage/PR ingestion, ui/PR subviews and diff, notifications/review and check observers.
- Read the master, TESTING_PLAN.md, INSTALLATION_PLAN.md, CROSS_PLATFORM_PLAN.md and all relevant scenario/platform ledgers.
- Use actual prior evidence; a missing dependency remains blocked rather than passed. Later-platform release work does not silently move into the Windows phase.

## Implementation tickets

### S06-T01

Build Conversation, Commits, Files, and Checks subviews with author, requested reviewers, source/target refs, draft/merge state, review results, linked issues, and independent resource completeness.

### S06-T02

Read formal reviews and inline threads separately from ordinary comments; preserve replies and resolved/outdated status. Add review text to local search and connect review-request, review, inline-comment, and supported check observation to Sprint 05 baselines.

### S06-T03

Implement immutable comparison identity and correct PR merge-base semantics as the PR portion of DIFF-03. Persist source-side and source-line mappings that Sprint 08 will use for review anchors; rendered row numbers are never source identities.

### S06-T04

Implement native unified/split diffs, file sidebar, syntax highlighting, selection/copy, collapsed files, next/previous change, keyboard navigation, and early viewed-file storage. Parse/highlight off-thread and virtualize long lists and diff rows.

### S06-T05

Fetch missing source context separately, detect truncated/binary/unavailable/API-capped data, and retain an explicit partial state when no practical alternate path exists. Bind checks and statuses to their head revision and expose unknown permissions.

### S06-T06

Deduplicate PR resources across panes, preserve independent Conversation/Files selection, ignore stale comparison results, and avoid retaining full render trees for inactive tabs. Measure large-diff responsiveness on Windows.

### S06-T07

Complete Download for offline use for a selected PR revision, including discussions/reviews, file/commit metadata, available patches, and chosen optional media/context. Expose partial/unavailable coverage, preserve offline pins and anchors, and pass AC-70 including offline request/cancellation and interrupted download resumption.

### S06-T08

Integrate PR resource families with persisted restart checkpoints and conditional/overlapping catch-up. Complete AC-75 and AC-76 using request traces, old-item changes, expired cursors, absent delta support, and interrupted page transactions. Preserve unrelated completed families and distinguish targeted repair from an unjustified full historical download.

### S06-T09

Implement the default PR-tab review arrangement: one 30-35% context surface beside 65-70% code, initial View PR body, View issue toggle, resizable/collapsible context, and narrow-window surface switching. Preserve top-level multiple PR tabs and general Open beside without requiring a third issue pane. Persist per-tab mode, selected issue, proportions and independent context/code scroll anchors.

### S06-T10

Parse closing-style references from the current native Markdown body, excluding code examples. Support the specified keyword, local/qualified/URL forms, verify actual issue type and scoped identity, and cache body-revision provenance. Offer a picker for multiple verified issues. Fetch cross-repository context under the proper credential profile without enabling repository monitoring or implying an auto-close relationship.

### S06-T11

Bind context responses to account/PR/body revision/issue selection/tab generation. Preserve code file, comparison and position while switching context; expose no-link, verification, PR-target, partial, offline, denied, failed, and removed-reference states truthfully. Complete AC-79/80 with delayed responses and cached linkage; use the reader subset of AC-78 now and close actual inline-draft/focus behavior in Sprint 08.

### S06-T12

Extend native-process E2E through real PR/body/linked-issue and diff controls. Cover stale context responses, multiple targets, offline cached context, large-diff responsiveness and per-tab state; distinguish simulator/test-build evidence from later exact packaged release smoke.

## Required tests and commands

- Unit/integration: comparison/source mapping, PR body references/type verification, review/check observers and partial/API-capped data.
- Native E2E: select PR/file, switch body/verified issue and PR tabs with delayed responses, restore offline context and inspect a large diff without focus/position loss.
- Regression/performance: unified/split diff correctness, requested-review counts, download/cursor recovery and current Windows rendering budgets.

Implement and use the canonical runner from TESTING_PLAN.md: unit, integration, ui, native e2e smoke/full, install with an artifact, and separately guarded live tests. These are development commands to build, not commands already supplied by this ZIP. Record actual OS/build/fixture evidence and mark unavailable checks Blocked/Not run. Native test-build E2E and exact published-package smoke remain separate.

Scenario IDs introduced or closing here: UT-011, UT-012, IT-006, IT-016, IT-017, UI-001, E2E-005.

Live-provider cases are shared gates on an explicitly authorized host. A later platform can reference existing evidence only with a recorded applicability assessment; rerun changed provider/credential-policy or target-specific behavior. Missing authorization remains Blocked/Not run and never permits unapproved writes.

Use test_scenarios.csv for fixture IDs, expected results, first/closure ownership and evidence. Release sprints rerun all scenarios applicable to their target; earlier sprint scenario lists do not waive regression checks.

## Deliverables

Complete PR read screens; native diff viewer; comparison/source mapping model; PR-family cache and observers; large-diff and partial-data fixtures.

## Exit gates

- Pass AC-05, AC-22, AC-23, AC-44, and AC-52. Demonstrate the same real PR in Conversation and Files panes without shared UI state or duplicate fetches.
- Exercise delayed responses after a PR switch, missing source context, unknown checks, truncated/binary files, and endpoint caps. Do not mark incomplete content complete.
- Verify unified/split views share correct source mapping and that cached merged state survives a later common issue-shaped closed response.
- Record large-diff measurements and review/check observer baseline tests. Full inline-submission AC-21 remains for Sprint 08.
- Pass AC-75 and AC-76 for the implemented issue/PR reader families. A necessary update-feed first page, overlap, or unchanged revalidation is permitted; verify targeted bounded work instead of asserting a universal exact-delta API.
- Pass AC-79 and AC-80 with multiple and cross-repository references, shared numbers resolving to PRs, code examples, disabled auto-close/non-default branches, missing/offline/denied content, and rapid PR/body/selection changes. Show one bound context/code pair with persistent multiple PR tabs. Actual review-draft stability and complete AC-78 close in Sprint 08.

## Initial acceptance scenarios owned here

- **AC-05** Rapidly switching between PRs cannot display a delayed response or diff belonging to the previously selected PR.
- **AC-22** Large diffs remain navigable while parsing and highlighting run in the background.
- **AC-23** Truncated, binary, unavailable, and API-capped content is explicitly identified.
- **AC-44** The same PR can appear in two panes with Conversation selected in one and Files selected in the other. Shared data updates both views without changing their subview or scroll state, and concurrent requests for the same resource and revision are deduplicated.
- **AC-52** A merged PR remains displayed as merged after a common issue-shaped response reports state closed without merge details. A PR whose merge information is not downloaded is not falsely classified as merged or definitely unmerged.
- **AC-70** Download for offline use exposes selected resource/revision coverage and progress, survives interrupted pagination, and can be requested/cancelled while offline. Reconnection completes eligible downloads without archiving the whole repository; capped, missing, or inaccessible PR data is never shown as fully available.
- **AC-75** Seed completed and partially downloaded issue/PR resource families plus persisted checkpoints and validators, stop the app, then change an old issue comment, PR review/check state, and other mutable fixture data while it is stopped. On restart, display cached content before networking finishes and use a recorded request trace to demonstrate bounded catch-up from saved progress, discovery of the old-item changes, safe partial-page resumption, and unchanged-resource revalidation. Do not trigger a full historical redownload or reset baselines. Overlapping reads, an update-feed first page, and endpoints without exact deltas are valid when targeted and documented; no zero-unchanged-payload assertion is required.
- **AC-76** Expire a stored cursor or remove delta support for one fixture resource family during restart catch-up. Re-enumerate only the necessary resource/range with stable-ID deduplication, preserving readable data, partial coverage, completed unrelated families, and draft/outbox references. Crash or fail between a returned page and its transaction, then resume without skipped changes, duplicate records, or a falsely advanced success checkpoint.
- **AC-79** Parse fixtures covering Resolves #41, uppercase/colon variants, the supported closing keywords, owner/repository#41, GitHub.com issue URLs, duplicate references, multiple issues, code examples, and a shared-number target that is actually a PR. Verify actual issue type and scoped identity, offer a picker for multiple issues, and never label a PR as an issue or infer an auto-close outcome. A reference on a non-default base branch or with auto-close disabled can remain reading context without a false server-relationship claim.
- **AC-80** With delayed context responses, switch PRs and body revisions and change linked-issue selection. No response shows the previous PR's issue or a removed reference under the new PR. Exercise no link, unresolved verification, unavailable permissions, missing/offline content, failed fetch, and cached verified linkage states. Cached bodies/issues remain readable offline; a lost link preserves the original-target draft and does not retarget it. A cross-repository reference does not enable unrelated repository monitoring or leak credentials.

## Full owned requirements

### PR-01

Provide Conversation, Commits, Files, and Checks sections. Display author, source and target branches, draft status, open/closed/merged state, requested reviewers, review results, relevant timestamps, and linked issues.

Initial verification owner: Sprint 06. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### PR-02

Retrieve ordinary conversation comments, formal reviews, and inline review threads as separate resources. Preserve thread replies and available resolved or outdated status. GitHub exposes ordinary PR comments through issue-comment APIs and review comments separately. [S06] [S09] [S13] [S15]

Initial verification owner: Sprint 06. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### PR-03

The commit list must show commit identity, message, author, and available timestamps. The file list must show paths, status, additions, deletions, and renames where available. Identify missing or incomplete information.

Initial verification owner: Sprint 06. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### PR-04

Show checks and commit statuses associated with the relevant PR head. A cached result must show its freshness. Missing permissions or unavailable checks must appear as unknown or inaccessible, never as success.

Initial verification owner: Sprint 06. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### PR-05

The default PR review arrangement MUST show the native file list/diff prominently beside exactly one smaller context pane bound to the same active PR. At a usable window width, start with approximately 33% of the review content width for context and 67% for code, within a 30-35% / 65-70% range, excluding global navigation chrome. Context may be resized or collapsed; restore its last proportion. Provide View PR body and View issue controls in that one pane, with PR body as the initial mode for a newly opened review tab. They replace the context content rather than adding a simultaneous third pane. Preserve Conversation, Commits, Files, and Checks navigation and multiple PR tabs. At narrow widths, show a usable focused surface with an explicit way to switch context/code and restore the wider arrangement without losing either surface's state.

Initial verification owner: Sprint 08. Native layout feasibility in 01 and workspace primitives in 04; bound context/code PR reader in 06; full AC-78 with actual inline drafts, focus, narrow layout and offline restore closes in 08.

### PR-06

Derive linked-issue candidates from closing-style references in the current PR body. Support case-insensitive close/closes/closed, fix/fixes/fixed, and resolve/resolves/resolved with optional colon and a same-repository #number, qualified owner/repository#number, or GitHub.com issue URL. Parse native Markdown safely; ignore fenced/inline code examples and do not treat arbitrary numbers as references. Resolve local #number against the PR's repository, normalize and deduplicate targets, and verify repository identity, access, and ordinary issue type before displaying a candidate as an issue; a shared-number target can be a PR. Multiple verified targets require a picker, with a deterministic default from body order and a retained explicit selection. Cross-repository targets use the appropriate credential scope and load on demand without silently enabling repository-wide monitoring. Preserve reference provenance and body revision under DATA-10. A body reference is reading context, not proof that GitHub linked, will close, or already closed the issue: default-branch rules, permissions, and repository auto-close settings can differ. [S27] [S28] [S29] [S30]

Initial verification owner: Sprint 06. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### PR-07

View issue MUST display the selected verified issue's native body and available metadata/conversation in the context pane, with its repository, number, and coverage visible. View PR body restores that PR's body and context scroll state. Switching modes or linked issues must retain the code comparison, selected file, scroll/selection, review draft identity/anchors, and intended mutation target; background loads must not steal code-editor focus. Deliberate keyboard/pointer navigation may move focus normally. Use account, PR, body revision, selected issue, tab instance, and selection generation to discard stale context responses. Switching PR tabs must never show the previous PR's issue while loading. Distinguish no reference, verification pending, target is a PR, not downloaded/offline, inaccessible, failed, and removed or changed reference states; none is a confirmed empty issue. Cached PR bodies, linkage evidence, verified issues, and per-resource coverage remain usable offline with last-validation information. Preserve local drafts if body refresh removes a reference, and never silently attach them to a replacement issue.

Initial verification owner: Sprint 08. Reference/cache/error and stale-response reading behavior closes AC-80 in 06; actual review-draft/focus stability and AC-78 close in 08.

### DIFF-01

Provide unified and split views, a changed-file sidebar, syntax highlighting, additions and deletions, selection and copying, file collapsing, next/previous change navigation, and keyboard shortcuts. Reuse GPUI Kit components where compatible. [S02]

Initial verification owner: Sprint 06. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### DIFF-04

Parse large patches and prepare highlighting outside the UI thread. Virtualize long file lists and diff rows. Keep scroll and selection stable when annotations or background data change. Ignore stale background results for an earlier comparison.

Initial verification owner: Sprint 08. Virtualized async native diff in 06; prove annotation stability with actual review editing in 08.

### DIFF-06

Distinguish a patch from complete source content. Expanding outside supplied context may require another fetch. Cache fetched file versions separately and preserve the current review view while additional context loads.

Initial verification owner: Sprint 06. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### DIFF-07

Detect API caps, missing pages, truncated patches, binary changes, unavailable objects, and unsupported formats. The PR-files endpoint has a documented 3,000-file limit, and the PR-commits endpoint a 250-commit limit. Provide an alternate retrieval path where practical and explicitly label any remaining partial result. Never label capped output complete. [S10]

Initial verification owner: Sprint 06. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

## Integration work owned elsewhere

### FND-06

UI events MUST enqueue or invoke application operations without performing blocking work. Results MUST carry resource identity and revision. View-specific work must also retain a tab-instance identity and selection generation so a late response cannot replace another tab's content or a newer selection. Shared resource results may update the cache and notify all matching views without changing their selection or focus.

Initial owner: Sprint 04. Initial verification: Sprint 09. Carry view identity, resource revision, cache publication, and context preservation through all read/write workflows.

### FND-07

The cache is the normal read source. Successful reads and writes update persistent records, then notify subscribed views. Preserve scroll position, selected file, and unfinished input when background refresh completes.

Initial owner: Sprint 04. Initial verification: Sprint 09. Carry view identity, resource revision, cache publication, and context preservation through all read/write workflows.

### PORT-02

Share domain models, GitHub integration, cache/search, synchronization, Outbox, review/diff logic, history, notification rules, and workspace state across platforms. Isolate operating-system APIs behind small capability interfaces with conditional dependencies inside platform implementations. Common code must not depend on Windows handles, registry calls, COM types, fixed Windows paths, or platform-specific credential objects. Reuse portable GPUI facilities where suitable rather than wrapping the entire toolkit in a new framework. Test interface contracts with fakes and actual adapters; do not duplicate the product's business logic for each OS.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce platform boundaries in 01 and keep shared modules portable throughout; verify the full Windows implementation boundary in 12 and parity in later platform rows.

### PORT-03

Prove the actual native renderer and necessary GPUI/GPUI Kit components early with selectable Markdown, text input/copy, large annotated diffs, the bound PR context/code layout, resizing, focus, and a graph fixture. Windows proof is a Sprint 01 gate. Configure compilation/core-test jobs for all intended targets and attempt macOS/Linux native smoke launches early on real target desktops; record gaps honestly and resolve them before that platform's release. In-process/headless tests or a successful cross-compile are not native GUI proof. No phase may add an embedded browser engine as a portability shortcut. [S31] [S32] [S33]

Initial owner: Sprint 01. Initial verification: Sprint 12. Windows native proof begins in 01; expand actual controls as built. Early other-target smoke is tracked separately; full native release evidence closes at each platform gate.

### PORT-06

Implement and test platform-appropriate keyboard shortcuts, text input, accessibility, clipboard, high-DPI/scaling, theme changes, timezone defaults, external-link opening, startup registration, close versus Quit, background residence, sleep/wake and network-change hints. Feed normalized lifecycle events into the shared scheduler. Restore offline/pause settings before probes or dispatch. If a supported environment lacks a tray/menu-bar capability, keep the app reachable and explain background behavior rather than hiding an inaccessible running instance. Closing/quit must preserve drafts, workspace, Outbox and credentials according to the shared lifecycle contract. [S33] [S35]

Initial owner: Sprint 01. Initial verification: Sprint 12. Keep native interaction/lifecycle boundaries explicit from 01; complete Windows accessibility, restart and installed behavior in 12, and repeat for later targets.

### BROWSE-03

Provide saved views for authored items, assignments, mentions or participation where supported, and requested reviews. Provide repository, state, author, label, assignee, reviewer, and date filters when supported by the selected query mode. Distinguish filtering downloaded data from querying GitHub.

Initial owner: Sprint 04. Initial verification: Sprint 06. Saved views and supported filters begin in 04; requested-review metadata integration in 06.

### BROWSE-04

Support recent items, pinned items, back and forward navigation, and opening an accessible GitHub issue or PR URL through the workspace rules below. Keyboard navigation must reach repository lists, item lists, pane controls, item tabs, PR subviews, search, and the review editor.

Initial owner: Sprint 04. Initial verification: Sprint 08. Navigation begins in 04; verify access to actual PR subviews and review editor in 06/08.

### BROWSE-05

Review requested, Assigned to me, and Outbox MUST show a positive total in a right-aligned dark blue filled circular badge with centered white digits. Use a suitable accessible color, provisionally #173E70, and preserve a true circle, label clearance, and legibility for multi-digit totals and high-DPI scaling. A known zero draws neither a circle nor a number. Counts are totals for the current account and each view's stated repository/filter scope, not unread-notification counts or just the visible page length. Count distinct matching items for saved views. Use a trustworthy complete-query total when available; otherwise label the cached/known count and incomplete coverage, without presenting unknown or partial coverage as a confirmed empty result. Outbox counts all unresolved submitted actions, including paused, blocked, and outcome-unknown actions, and excludes drafts, download jobs, and confirmed/cancelled history. Refresh counts from shared state after ingestion, edits, scope changes, enqueue, cancellation, and confirmation without per-pane polling. Accessible names and count details must expose scope, freshness, and any coverage limitation.

Initial owner: Sprint 04. Initial verification: Sprint 09. Count component and shared query totals in 04; requested-review metadata in 06 and actual pending states in 07-09. Full AC-81 validates all supported writers, scope/coverage, circular styling and zero suppression in 09.

### WORKSPACE-05

Persist view state independently for every tab instance: selected subview, selected file or commit, diff display mode, scroll anchors, expanded discussion or diff regions, and relevant navigation history. For a PR review tab also retain its context mode, selected verified linked issue, context/code proportions, collapsed/focused surface, and separate context scroll anchors. Switching tabs, resizing panes, or refreshing one resource MUST NOT reset another tab's position, selection, or focus. Anchor diff state to its comparison revision and handle unavailable anchors after a changed revision explicitly.

Initial owner: Sprint 04. Initial verification: Sprint 10. Independent state expands from conversations to per-PR context/code modes and linked-issue selections in 06, actual review drafts in 08, and commit comparisons in 10.

### WORKSPACE-08

Save and restore the workspace per account, including pane order and proportions, open tab order, active tabs, focused pane, per-tab view state, scroll position or anchors, navigation context, and draft references. Normal exit MUST retain this state. On restart, render cached visible content before waiting for any API response or renewed login; then start eligible background work through the shared scheduler. Load inactive tabs lazily and expose missing or inaccessible content without discarding its navigation target. Restoring a workspace MUST NOT submit unsubmitted drafts, create duplicate operations, or expose another account's private tabs. The independent outbox worker resumes already submitted eligible actions under OUTBOX-04 and the restored authentication/pause settings; view restoration is not a dispatch trigger.

Initial owner: Sprint 04. Initial verification: Sprint 12. Restore local workspace/drafts before networking in 04; integrate all views and writers as built. Final normal-exit, signed-out, expired-PAT and delayed-network restoration closes in 12. View restoration never submits drafts.

### WORKSPACE-10

Share request scheduling and deduplicate in-flight fetches across views of the same resource and revision. Opening additional panes MUST NOT create independent polling loops or duplicate alerts for that resource. Give visible tabs priority within the existing schedule and rate-limit budget. Inactive tabs must not retain every large diff or continuously redraw; evict reconstructible rendering state while preserving navigation state and drafts. Closing one view must not cancel work still needed by another view or the shared write journal.

Initial owner: Sprint 05. Initial verification: Sprint 12. Shared scheduling in 05 and large-view retention in 06/10; complete integrated twenty-tab performance gate in 12.

### WORKSPACE-12

Provide documented keyboard commands for opening a new tab or side pane, cycling tabs within a pane, moving focus between panes, closing and reopening a tab, and moving a tab between panes. Expose tab and pane names, selection, and focus to the published operating system's accessibility features, with Windows tested first. Test these commands from real text editors without intercepting ordinary editing shortcuts unexpectedly.

Initial owner: Sprint 04. Initial verification: Sprint 08. Workspace shortcuts and accessibility names in 04; test conflicts from real review text editors in 08.

### SEARCH-01

Provide fast local full-text search of downloaded titles, descriptions, and comments. Include available downloaded review text. Search results MUST identify the repository and item, highlight the match where practical, and navigate to the matching discussion or review location.

Initial owner: Sprint 04. Initial verification: Sprint 08. FTS schema in 02 and conversations in 04; formal review ingestion in 06 and confirmed submitted reviews in 08.

### CACHE-01

Opening downloaded content MUST render it immediately without waiting for GitHub. Refresh stale resources independently. The first visit to uncached content may require network access and must expose useful incremental progress.

Initial owner: Sprint 04. Initial verification: Sprint 06. Cached issues/conversations in 04; cached PR resources and diff context in 06.

### CACHE-02

Fetch resource groups independently: metadata, ordinary comments, timeline events, reviews, review threads, commits, changed-file lists, patches, source context, and media. Loading a PR's Conversation tab must not wait for every patch or image.

Initial owner: Sprint 04. Initial verification: Sprint 06. Complete independently fetched PR review/thread/commit/file/source families in 06.

### CACHE-06

Retain fetched text, review data, commit metadata, and diffs across restarts. Do not impose an undocumented age cutoff. Bound memory and render state independently of disk retention. Expose per-repository usage and explicit cleanup controls.

Initial owner: Sprint 04. Initial verification: Sprint 12. Disk retention/cleanup in 04; integrate all content types and verify bounded memory on full workload in 12.

### CACHE-09

Associate comparison data with immutable commit identities and comparison options. Associate mutable branch labels, status checks, and review state with their relevant revisions and timestamps. A branch name alone is not an adequate diff cache key.

Initial owner: Sprint 02. Initial verification: Sprint 10. Typed immutable comparison identity in 02; actual PR/review and repository comparison integration later.

### CACHE-11

Existing images and accessible attachments SHOULD be fetched on demand, stored with cache metadata, and reused. Source code outside a patch is a separate resource. Show explicit placeholders for missing, inaccessible, binary, or unsupported content.

Initial owner: Sprint 04. Initial verification: Sprint 06. Media and attachment placeholders in 04; source context outside supplied patches in 06.

### CACHE-13

Normal window close and application exit MUST retain downloaded content and resource coverage, all draft types, saved tabs/layout/view positions, local preferences, offline pins, pending actions and receipts, synchronization checkpoints/validators, and notification baselines/delivery history on the PC. Persist settings and user changes as they occur and safely flush remaining local state on orderly exit; if a user-data write fails, keep it recoverable and report the failure. Retain the protected credential according to AUTH-07. Restart MUST reuse these records and render the saved local workspace before networking completes. It must not clear the cache, reset baselines, or redownload all history merely because the process was restarted. Sign-out preserves local content under AUTH-11. Explicit cache cleanup remains a separate action and continues to protect drafts and unresolved work under CACHE-07 and OUTBOX-10.

Initial owner: Sprint 02. Initial verification: Sprint 12. Complete persistence schema in 02, PAT lifecycle in 03, local-first restoration in 04, sync in 05/06, all writer/history/notification types through 11, and the complete quit/sign-out/reopen matrix in 12.

### OFFLINE-01

Once an account and content have been established, the application MUST start and remain usable without a network check or renewed login blocking local access. Users can browse/search downloaded issues, PRs, discussions, reviews, commits, diffs, graph history, and the activity feed; use tabs and panes; edit local settings and drafts; and inspect or cancel pending work. State the cached coverage and last fetch honestly. Unseen remote content and new server events cannot be available until fetched. First-time authentication and initial acquisition of private content still require access; do not require a full repository download.

Initial owner: Sprint 04. Initial verification: Sprint 12. Offline startup/workspace in 04; each new view and action integrates as built; complete all-feature cold-start offline matrix in 12.

### OFFLINE-03

Provide Download for offline use for a selected item, with the resource scope and revision shown before downloading. Include its accessible discussion and, for a selected PR revision, review/thread data, commit/file metadata and available patches; let users choose optional media or additional source context. Track progress and per-resource completion, expose caps or unavailable content, and resume interrupted downloads. An offline request to fetch missing data becomes a visible, cancellable pending download that starts when eligible. Download jobs are distinct from remote mutation actions. Never present partially fetched data as fully available or start a mandatory full-repository archive.

Initial owner: Sprint 04. Initial verification: Sprint 06. Issue downloads and coverage in 04, scheduled resumption in 05, full PR resource scope and AC-70 in 06.

### OFFLINE-04

Show submitted local work immediately as clearly pending in all matching views: for example a pending comment, a review action, or an edit to an existing issue awaiting confirmation. Keep pending projections separate from confirmed cached server records; a refresh cannot erase them, and cancellation removes only the pending projection while preserving recoverable user text. A pending merge or approval is not a confirmed merge or approval. Offline pins and minimal draft/outbox dependencies must survive ordinary cache cleanup until explicitly released or the action is resolved.

Initial owner: Sprint 04. Initial verification: Sprint 09. Storage protection in 02, pending-view foundation in 04, actual issue/comment/review/merge projections and cancellation integration in 07-09.

### SYNC-05

Centralize pagination, cancellation, request deduplication, retry limits, exponential backoff, and server budget handling. Honor relevant `Retry-After`, rate-limit, and polling headers. Authenticated conditional requests that return 304 can preserve the primary REST quota, but do not make polling unlimited. [S11] [S12]

Initial owner: Sprint 05. Initial verification: Sprint 06. General read scheduler and item/comment observers in 05; PR-family pagination and review/check reconciliation in 06.

### SYNC-06

Maintain independent synchronization progress for resource families. Use repository-level updated-comment feeds where available, but do not assume the parent issue or PR timestamp is a complete change feed for every child object. Use overlapping update windows and periodic reconciliation where required. [S06] [S13]

Initial owner: Sprint 05. Initial verification: Sprint 06. General read scheduler and item/comment observers in 05; PR-family pagination and review/check reconciliation in 06.

### SYNC-07

Include all states when checking for relevant updates. A new comment on an old closed issue or merged PR can be relevant. Do not permanently freeze previously downloaded historical items merely because they are closed.

Initial owner: Sprint 05. Initial verification: Sprint 06. General read scheduler and item/comment observers in 05; PR-family pagination and review/check reconciliation in 06.

### SYNC-11

Resume from persisted per-account, per-repository, per-resource-family synchronization state on restart, reconnection, and same-user PAT renewal. Retain the last successful checkpoint, overlapping update window or supported cursor, pagination progress, conditional-request validators, resource completeness/freshness, and notification baselines. Advance content and its successful progress transactionally after each completed unit; failures or partial pages must not skip unseen changes. Prioritize visible resources and run bounded, rate-aware incremental catch-up, using supported changed-since/update feeds, conditional reads, and targeted reconciliation. Do not blindly redownload the entire cache, re-enumerate every historical discussion, reset notification baselines, or use the largest issue/PR number as the only cursor. If a cursor expires or an endpoint has no reliable delta interface, re-enumerate the necessary resource/range with stable-ID deduplication and preserved coverage while keeping other completed families intact. An update-feed page one, overlap, unchanged revalidation, or targeted repair may be necessary; no universal exact-delta or zero-unchanged-bytes guarantee is made. Preserve old-item update discovery and honor restored sign-out, offline, and pause state. [S06] [S07] [S11] [S13]

Initial owner: Sprint 05. Initial verification: Sprint 12. Persist checkpoints and stable account identity in 02/03; issue/comment restart catch-up in 05 and PR-family AC-75/76 in 06; all graph/notification families and full lifecycle request traces complete in 12.

### DIFF-02

Provide inline thread display, comment-entry controls, unresolved-thread navigation, and persistent per-revision viewed-file markers. A file changed by a subsequent PR revision must not remain silently marked fully reviewed.

Initial owner: Sprint 08. Initial verification: Sprint 08. Read threads and establish viewed-file state in 06; inline entry, navigation and revision invalidation completed in 08.

### DIFF-03

Distinguish PR comparison, individual-commit comparison, and arbitrary two-commit comparison. The default PR view must match the intended GitHub comparison. GitHub PRs use the merge base for a three-dot comparison; endpoint comparisons can differ. Label the comparison and use the correct source identities in the cache. [S16]

Initial owner: Sprint 10. Initial verification: Sprint 10. Typed comparison model in 02; correct PR comparison in 06; individual/two-commit comparisons complete in 10.

### DIFF-05

Support an ignore-whitespace viewing option when backed by an accurate comparison. Preserve original source coordinates for review submission; do not derive comment positions from a visually filtered row index.

Initial owner: Sprint 08. Initial verification: Sprint 08. Source coordinates established in 06; accurate filtering and review-submission mapping verified in 08.

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

### DATA-06

Use an items table for shared fields and a pull_request_details table whose item_id is both its primary key and a foreign key to items. Only confirmed PR items may have this extension. The item kind distinguishes an ordinary issue from a PR once classified. PR summaries may exist before PR details are fetched; a navigation placeholder may remain unclassified until metadata arrives. Track missing detail separately from known false or null values. Keep PR merge state and source references in the extension, and derive the displayed merged state from confirmed PR data. A common closed-state update must not erase known merge information.

Initial owner: Sprint 02. Initial verification: Sprint 06. Storage fixtures first; live issue-shaped PR ingestion in 04 and PR endpoint/details integration in 06.

### DATA-07

Issue endpoints can return PRs, identified by their pull_request property. For that representation, the REST id is an issue ID; the pull-request endpoint supplies the separate PR ID. Store these in distinct typed fields or identity mappings and retain node IDs as opaque provider identifiers. If PR data arrives first, its issue-endpoint ID may remain unknown until fetched. Upsert both representations to the same item using the scoped repository number and verified provider identities. Resolve concurrent ingestion transactionally. Distinguish absent fields from explicit nulls and reject stale overwrites using applicable revision or freshness evidence. Never substitute one ID for the other, create a duplicate row for the second representation, downgrade a known PR because a partial payload omits its marker, or mark unrelated resource families complete. [S28]

Initial owner: Sprint 02. Initial verification: Sprint 06. Storage fixtures first; live issue-shaped PR ingestion in 04 and PR endpoint/details integration in 06.

### DATA-08

Store ordinary issue and PR conversation comments in a shared conversation-comments model keyed to item_id. Formal reviews and inline review comments remain distinct resources with their own provider identity namespaces and source anchors. Share common labels, assignees, search, drafts, and workspace references through the item identity. Route each remote operation by its API capability; a shared table does not make PR review or merge operations valid for ordinary issues. [S06] [S09] [S10] [S13]

Initial owner: Sprint 02. Initial verification: Sprint 09. Shared comments and typed resources in storage/reads; review/merge guards complete in 09.

### DATA-10

Store derived PR-body issue references separately from confirmed item identity and server relationship facts. Key a candidate set by source PR item_id and body revision/hash, preserve raw/reference position and provenance, and retain verification state and the verified target issue item_id when available. Recompute when the source body changes; retain unchanged verified targets and selection where still valid, but never associate a prior PR's or prior body's removed target with the current context. Cache reference evidence for offline use with its last validation, without marking current remote access or auto-closing behavior confirmed. Use the existing account/repository identity and transfer rules; do not create a new ordinary issue record or change a known PR kind merely to satisfy a parsed #number. Drafts and outbox actions keep their original stable targets even if a reference disappears. [S27] [S28] [S29] [S30]

Initial owner: Sprint 02. Initial verification: Sprint 06. Reference/provenance and view-state schema in 02; local state foundations in 04; real PR-body parsing, verified issue identities, changed-body reconciliation and offline context in 06.

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

BROWSE-03, CACHE-01, CACHE-02, CACHE-11, OFFLINE-03, SYNC-05, SYNC-06, SYNC-07, PR-01, PR-02, PR-03, PR-04, PR-06, DIFF-01, DIFF-06, DIFF-07, DATA-06, DATA-07, DATA-10.

Use requirement_platform_traceability.csv for applicability and release-gate evidence. Initial ledger statuses refer only to their initial phase; no single Windows result closes all platforms. Keep future options and unavailable services explicit.

## Later scope and handoff

Sprint 08 adds inline entry/submission, re-anchoring, full viewed-file/whitespace behavior, and AC-78 context-switch stability with actual review drafts. Sprint 10 completes individual-commit and two-commit comparisons. Notifications consume observers in Sprint 11.

## Coding LLM instruction

Implement Sprint 06 against v1.7. Keep the app buildable and preserve the current product constraints. Complete listed implementation and test work, use actual native controls for E2E, protect original targets/drafts/Outbox state, and add only the named platform integrations. Do not add issue/PR creation, merge queues or browser engines. Keep secrets out of code/results; live writes require explicit allowlisted authorization. Implement the short verified prebuilt install/update contract for the relevant publication phase and never claim placeholder commands runnable. Update initial/per-platform/scenario ledgers with commands, environment, package identity and evidence, then use SPRINT_REPORT_TEMPLATE.md. Continue only within the assigned sprint/Windows/all-platform scope without new routine approvals.

## Technical sources

- [S02 GPUI Kit native diff component](https://gpui-kit.com/component/diff/)
- [S06 GitHub issue and PR conversation comments](https://docs.github.com/en/rest/issues/comments)
- [S07 GitHub REST pagination](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api)
- [S09 GitHub pull request reviews](https://docs.github.com/en/rest/pulls/reviews)
- [S10 GitHub pull request reads and merges](https://docs.github.com/en/rest/pulls/pulls)
- [S11 GitHub REST best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)
- [S12 GitHub REST rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)
- [S13 GitHub inline review comments](https://docs.github.com/en/rest/pulls/comments)
- [S15 GitHub GraphQL pull request and review types](https://docs.github.com/en/graphql/reference/pulls)
- [S16 GitHub branch comparison semantics](https://docs.github.com/en/pull-requests/reference/branches)
- [S27 GitHub shared issue and PR numbering](https://docs.github.com/en/enterprise-cloud%40latest/rest/using-the-rest-api/issue-event-types)
- [S28 GitHub issue representations identifiers and updates](https://docs.github.com/en/rest/issues/issues)
- [S29 GitHub linking pull requests and issues](https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/linking-a-pull-request-to-an-issue)
- [S30 GitHub repository auto-close settings](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/managing-repository-settings/managing-auto-closing-issues)
- [S31 GPUI Kit installation and target prerequisites](https://gpui-kit.com/docs/installation/)
- [S32 GPUI Kit testing facilities and limits](https://gpui-kit.com/docs/test/)
- [S33 GPUI Kit accessibility](https://gpui-kit.com/docs/accessibility/)
- [S35 GPUI Kit packaging](https://gpui-kit.com/docs/packaging/)

Additional current primary sources and platform/install/test constraints are in the three companion plans. Verify pinned tool/API behavior during implementation.
