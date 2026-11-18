# Sprint 04: Cached reading, search, tabs, and offline workspace

Requirements version 1.7 | Initial sprint status Planned

## Outcome

The first useful reader opens downloaded conversations instantly, searches locally, and keeps multiple issues and PRs visible with saved local drafts.

## Phase and prerequisites

- Release phase: Windows first release.
- Prerequisites: Sprint 02, Sprint 03.
- Code areas: queries, storage/FTS ingestion, github/list and discussion reads, workspace, ui/Markdown and search, local draft editor.
- Read the master, TESTING_PLAN.md, INSTALLATION_PLAN.md, CROSS_PLATFORM_PLAN.md and all relevant scenario/platform ledgers.
- Use actual prior evidence; a missing dependency remains blocked rather than passed. Later-platform release work does not silently move into the Windows phase.

## Implementation tickets

### S04-T01

Implement paginated all-state repository browsing with no age cutoff, common issue/PR ingestion, filters, saved views, recent/pinned items, and URL navigation. Persist each page and its progress atomically; distinguish partial results and authorization failures.

### S04-T02

Render issue metadata, accessible timeline and ordinary conversations, including ordinary PR comments, from the cache. Fetch missing resource groups independently. Provide native selectable Markdown, on-demand images/attachments, and explicit unsupported/missing-content placeholders.

### S04-T03

Implement FTS search over downloaded content and clearly separate remote GitHub discovery. Handle remote caps, incomplete results, unsupported filters, and offline targets without asserting exhaustive historical coverage.

### S04-T04

Implement resizable side-by-side panes with separate tab strips, independent view state, duplicate views, tab movement/reordering, open-beside routing, maximize/restore, narrow-window behavior, close/reopen, and account-scoped lazy session restoration.

### S04-T05

Provide a local Markdown draft composer and preview now, including existing-issue edit and comment drafts, one active editor per draft, focus-editor actions, autosave, recovery, and offline editing. Submission remains unavailable until Sprint 07; closing a tab does not discard its drafts.

### S04-T06

Wire resource/tab identity and selection generations into asynchronous results. Preserve other panes' focus, selection, input, and scroll. Add keyboard/accessibility names, high-DPI and theme behavior, usage/cleanup controls, and early cached-open/search/tab-switch measurements.

### S04-T07

Start offline without a blocking network/login check for an established account. Add persistent green/red connectivity chrome with accessible text, checking/stale state, last-check details, and a pending-count link; integrate real status signals in Sprint 05.

### S04-T08

Build an offline-accessible outbox shell backed by persisted fixture entries and distinct pending local projections. Add Download for offline use for issue content with resumable visible/cancellable download jobs and truthful resource coverage. Actual mutation submission/dispatch/cancellation is completed in Sprint 07; PR offline-download integration follows in Sprint 06.

### S04-T09

Restore the normal-exit workspace from retained local state before any API response, including tab/pane order, focus, positions, selections, drafts, and settings. Permit an explicitly selected signed-out local profile to remain readable while marking remote work held. Prove the current slice with delayed networking and expired credentials; later graph, notification, and full-writer state closes in Sprint 12.

### S04-T10

Implement the sidebar count component for Review requested, Assigned to me, and Outbox: right-aligned dark blue true circles, centered white digits, adequate label clearance, and no badge or number for a known zero. Bind counts to shared account/scope queries, deduplicate item totals, and disclose cached or incomplete coverage. Use persisted outbox fixtures for the UI now; actual cross-action count closure is AC-81 in Sprint 09.

### S04-T11

Add native Windows E2E for cached cold start, search, tabs, saved drafts and offline coverage using a real temporary profile and the HTTP simulator. Keep keyboard/input/accessibility paths portable and run the available common tests/builds on macOS/Linux without claiming later-platform GUI parity.

## Required tests and commands

- Unit/integration: cache completeness, URL routing, FTS coverage, per-tab state and draft ownership with delayed/partial data.
- Native E2E: cold offline startup, search, tabs/panes, saved draft restoration, scope counts and explicit missing-content states through actual controls.
- Regression: restart during pagination, close/reopen and account isolation; capture native focus/accessibility evidence and current cached-navigation timings.

Implement and use the canonical runner from TESTING_PLAN.md: unit, integration, ui, native e2e smoke/full, install with an artifact, and separately guarded live tests. These are development commands to build, not commands already supplied by this ZIP. Record actual OS/build/fixture evidence and mark unavailable checks Blocked/Not run. Native test-build E2E and exact published-package smoke remain separate.

Scenario IDs introduced or closing here: UT-008, UT-013, UT-018, IT-003, UI-002, UI-003, UI-008, E2E-003, E2E-004.

Live-provider cases are shared gates on an explicitly authorized host. A later platform can reference existing evidence only with a recorded applicability assessment; rerun changed provider/credential-policy or target-specific behavior. Missing authorization remains Blocked/Not run and never permits unapproved writes.

Use test_scenarios.csv for fixture IDs, expected results, first/closure ownership and evidence. Release sprints rerun all scenarios applicable to their target; earlier sprint scenario lists do not waive regression checks.

## Deliverables

Usable cache-first reader; native discussion renderer; local/remote search; pane/tab workspace; offline draft composer; restoration and cleanup UI; initial performance evidence.

## Exit gates

- Pass the assigned AC scenarios with live reads where needed and deterministic failures. Disable networking and reopen downloaded discussions and unsent drafts.
- Exercise page-two failure and process restart during pagination. A partial conversation must remain partial and resume without duplicate rows.
- Show an issue and PR in separate item panes, keyboard navigation from real editors, duplicate-view draft ownership, narrow-window recovery, and independent loading/errors. Keep these general item panes distinct from the bound context/code surfaces added to a PR tab in Sprint 06.
- Measure Section 16 cached-open, local-search, and tab-switch targets on the Windows fixture. Record and fix failures affecting this slice before calling it complete.

## Initial acceptance scenarios owned here

- **AC-01** A valid account with access to an SSO-protected private GitHub.com repository can select it and browse issues and PRs.
- **AC-04** A user can browse a closed issue or merged PR older than 90 days without enabling a bulk archive.
- **AC-06** With networking disabled, reopening a downloaded issue shows its content and unfinished draft.
- **AC-07** Failure on comment page two shows a partial discussion, not an empty or complete one; a retry resumes safely.
- **AC-08** Restarting during pagination does not lose completed records or duplicate them after recovery.
- **AC-09** Local search clearly limits coverage to downloaded content; remote search can discover an uncached item.
- **AC-10** Clearing downloaded content preserves all user drafts and unresolved submitted actions with their required snapshots and anchors, and explains which offline content is removed. At each earlier sprint, test the draft/queue types then implemented; final outbox integration is verified again in AC-68.
- **AC-42** An issue and a PR from different selected repositories remain simultaneously visible in resizable side-by-side panes, each with multiple item tabs. Switching a tab in one pane leaves the other pane's content, scroll position, and draft intact.
- **AC-43** Opening beside, reordering or moving a tab, maximizing and restoring a pane, and narrowing the window preserve all open items and their view state. A loading or failed request in one pane does not block the other pane.
- **AC-46** Closing and reopening a tab, closing a pane, and restarting offline restore saved drafts and applicable view state. The saved pane layout, tab order, and selected tabs return without downloading inactive content or automatically submitting unsubmitted drafts.
- **AC-49** Tab opening, selection, closing, reopening, pane focus, and moving tabs between panes are operable with the documented keyboard commands and meaningful accessibility names. Ordinary text editing remains usable in both panes.

## Full owned requirements

### FND-06

UI events MUST enqueue or invoke application operations without performing blocking work. Results MUST carry resource identity and revision. View-specific work must also retain a tab-instance identity and selection generation so a late response cannot replace another tab's content or a newer selection. Shared resource results may update the cache and notify all matching views without changing their selection or focus.

Initial verification owner: Sprint 09. Carry view identity, resource revision, cache publication, and context preservation through all read/write workflows.

### FND-07

The cache is the normal read source. Successful reads and writes update persistent records, then notify subscribed views. Preserve scroll position, selected file, and unfinished input when background refresh completes.

Initial verification owner: Sprint 09. Carry view identity, resource revision, cache publication, and context preservation through all read/write workflows.

### BROWSE-02

Repository lists MUST support issues and PRs in all states, with no hidden historical date cutoff. Fetch pages as users browse. Cache returned summaries immediately, and provide clear loading, empty, partial, offline, and error states. Identify PR records returned by issue endpoints, upsert them into the common item store, and apply the requested issue-only, PR-only, or combined view without duplicating an item. [S28]

Initial verification owner: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### BROWSE-03

Provide saved views for authored items, assignments, mentions or participation where supported, and requested reviews. Provide repository, state, author, label, assignee, reviewer, and date filters when supported by the selected query mode. Distinguish filtering downloaded data from querying GitHub.

Initial verification owner: Sprint 06. Saved views and supported filters begin in 04; requested-review metadata integration in 06.

### BROWSE-04

Support recent items, pinned items, back and forward navigation, and opening an accessible GitHub issue or PR URL through the workspace rules below. Keyboard navigation must reach repository lists, item lists, pane controls, item tabs, PR subviews, search, and the review editor.

Initial verification owner: Sprint 08. Navigation begins in 04; verify access to actual PR subviews and review editor in 06/08.

### BROWSE-05

Review requested, Assigned to me, and Outbox MUST show a positive total in a right-aligned dark blue filled circular badge with centered white digits. Use a suitable accessible color, provisionally #173E70, and preserve a true circle, label clearance, and legibility for multi-digit totals and high-DPI scaling. A known zero draws neither a circle nor a number. Counts are totals for the current account and each view's stated repository/filter scope, not unread-notification counts or just the visible page length. Count distinct matching items for saved views. Use a trustworthy complete-query total when available; otherwise label the cached/known count and incomplete coverage, without presenting unknown or partial coverage as a confirmed empty result. Outbox counts all unresolved submitted actions, including paused, blocked, and outcome-unknown actions, and excludes drafts, download jobs, and confirmed/cancelled history. Refresh counts from shared state after ingestion, edits, scope changes, enqueue, cancellation, and confirmation without per-pane polling. Accessible names and count details must expose scope, freshness, and any coverage limitation.

Initial verification owner: Sprint 09. Count component and shared query totals in 04; requested-review metadata in 06 and actual pending states in 07-09. Full AC-81 validates all supported writers, scope/coverage, circular styling and zero suppression in 09.

### SEARCH-01

Provide fast local full-text search of downloaded titles, descriptions, and comments. Include available downloaded review text. Search results MUST identify the repository and item, highlight the match where practical, and navigate to the matching discussion or review location.

Initial verification owner: Sprint 08. FTS schema in 02 and conversations in 04; formal review ingestion in 06 and confirmed submitted reviews in 08.

### SEARCH-02

Provide a separate Search GitHub action or clearly labeled remote result group to discover uncached items. Local results may appear immediately while a remote request runs. An empty local result means no matches in downloaded content, not no matches anywhere in the selected repositories.

Initial verification owner: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### SEARCH-03

Handle unsupported remote query filters, pagination, API limits, and incomplete results explicitly. GitHub REST search can cap a query at 1,000 results and can return incomplete results. Do not use one search request as proof that every historical item was enumerated. Repository browsing must have its own paginated path. [S07] [S08]

Initial verification owner: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### SEARCH-04

Offline search MUST remain usable and clearly indicate its downloaded-content scope. Opening an uncached remote result while offline must explain which content is missing and preserve the navigation target.

Initial verification owner: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### WORKSPACE-01

The application MUST support multiple open issue and PR tabs and at least two simultaneously visible side-by-side panes in one window. Users must be able to mix issues and PRs from the same or different selected repositories. Allow additional side-by-side panes while usable minimum widths can be maintained. A new workspace starts with one pane; a split action adds another without closing the existing content.

Initial verification owner: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### WORKSPACE-02

Each pane MUST maintain its own ordered item tabs and active tab. Show the item type, repository, item number, a readable title, and relevant draft or loading indicators through the tab label and accessible details. Provide tab overflow navigation and access to the full label when space is limited. Opening another item must not silently replace an existing tab. Provide explicit commands to open in a new tab, open beside the current pane, close a tab, and reopen the most recently closed tab.

Initial verification owner: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### WORKSPACE-03

Ordinary item navigation reuses an existing matching tab in the active pane or adds a tab there. Open beside must add or focus a tab in an adjacent pane without replacing its other tabs. Provide an explicit duplicate-view action to show the same issue or PR in different panes, for example a PR conversation beside its Files subview. If several tabs in the target pane match, reuse its active matching tab or otherwise its most recently active match.

Initial verification owner: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### WORKSPACE-04

Users MUST be able to resize panes, reorder tabs, and move tabs between panes. Provide visible or keyboard-accessible commands as well as any drag gestures. Show the focused pane clearly. A focus or maximize action MUST enlarge one pane temporarily and restore the previous layout. On a narrow window, preserve all tabs and drafts and offer a usable focused-pane layout instead of shrinking content beyond its minimum width.

Initial verification owner: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### WORKSPACE-05

Persist view state independently for every tab instance: selected subview, selected file or commit, diff display mode, scroll anchors, expanded discussion or diff regions, and relevant navigation history. For a PR review tab also retain its context mode, selected verified linked issue, context/code proportions, collapsed/focused surface, and separate context scroll anchors. Switching tabs, resizing panes, or refreshing one resource MUST NOT reset another tab's position, selection, or focus. Anchor diff state to its comparison revision and handle unavailable anchors after a changed revision explicitly.

Initial verification owner: Sprint 10. Independent state expands from conversations to per-PR context/code modes and linked-issue selections in 06, actual review drafts in 08, and commit comparisons in 10.

### WORKSPACE-08

Save and restore the workspace per account, including pane order and proportions, open tab order, active tabs, focused pane, per-tab view state, scroll position or anchors, navigation context, and draft references. Normal exit MUST retain this state. On restart, render cached visible content before waiting for any API response or renewed login; then start eligible background work through the shared scheduler. Load inactive tabs lazily and expose missing or inaccessible content without discarding its navigation target. Restoring a workspace MUST NOT submit unsubmitted drafts, create duplicate operations, or expose another account's private tabs. The independent outbox worker resumes already submitted eligible actions under OUTBOX-04 and the restored authentication/pause settings; view restoration is not a dispatch trigger.

Initial verification owner: Sprint 12. Restore local workspace/drafts before networking in 04; integrate all views and writers as built. Final normal-exit, signed-out, expired-PAT and delayed-network restoration closes in 12. View restoration never submits drafts.

### WORKSPACE-09

Closing a tab or pane MUST preserve its saved drafts, cached content, and already submitted operation tracking. Persist editor changes before closing; if that fails, keep the editor recoverable and explain the failure. Closing a pane closes its tab views and focuses a remaining pane; closing the final tab leaves a usable empty pane. Discarding a draft is a separate explicit action. Reopening a closed tab restores its saved view state when available.

Initial verification owner: Sprint 09. Persist closing-tab state in 04; prove pending issue/review/merge operations survive view closure.

### WORKSPACE-11

Repository lists, search results, in-app links, and notification activation MUST route through the workspace's open-item behavior. For notification or external URL activation, prefer a matching tab in the focused pane, otherwise the most recently active matching tab elsewhere, otherwise a new tab in the focused pane. Apply WORKSPACE-03 when choosing among matches within a pane. Navigate only the chosen tab to the requested comment or diff location. Other panes retain their positions and unsent input.

Initial verification owner: Sprint 11. Open-item routing begins in 04; real notification activation completes the requirement in 11.

### WORKSPACE-12

Provide documented keyboard commands for opening a new tab or side pane, cycling tabs within a pane, moving focus between panes, closing and reopening a tab, and moving a tab between panes. Expose tab and pane names, selection, and focus to the published operating system's accessibility features, with Windows tested first. Test these commands from real text editors without intercepting ordinary editing shortcuts unexpectedly.

Initial verification owner: Sprint 08. Workspace shortcuts and accessibility names in 04; test conflicts from real review text editors in 08.

### CACHE-01

Opening downloaded content MUST render it immediately without waiting for GitHub. Refresh stale resources independently. The first visit to uncached content may require network access and must expose useful incremental progress.

Initial verification owner: Sprint 06. Cached issues/conversations in 04; cached PR resources and diff context in 06.

### CACHE-02

Fetch resource groups independently: metadata, ordinary comments, timeline events, reviews, review threads, commits, changed-file lists, patches, source context, and media. Loading a PR's Conversation tab must not wait for every patch or image.

Initial verification owner: Sprint 06. Complete independently fetched PR review/thread/commit/file/source families in 06.

### CACHE-04

Only show an empty-content message when the relevant resource is known to be complete and empty. A cached issue summary does not establish that its comments, timeline, or attachments have been downloaded.

Initial verification owner: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### CACHE-05

Commit downloaded pages and their progress atomically. Retry and resume reads without duplicating records. Persist enough progress to resume interrupted pagination. If a cursor or page boundary becomes unreliable, safely re-enumerate the affected range using stable IDs and deduplication. [S07]

Initial verification owner: Sprint 05. Atomic progress primitives in 02, resumable UI pagination in 04, scheduled read resumption in 05.

### CACHE-06

Retain fetched text, review data, commit metadata, and diffs across restarts. Do not impose an undocumented age cutoff. Bound memory and render state independently of disk retention. Expose per-repository usage and explicit cleanup controls.

Initial verification owner: Sprint 12. Disk retention/cleanup in 04; integrate all content types and verify bounded memory on full workload in 12.

### CACHE-11

Existing images and accessible attachments SHOULD be fetched on demand, stored with cache metadata, and reused. Source code outside a patch is a separate resource. Show explicit placeholders for missing, inaccessible, binary, or unsupported content.

Initial verification owner: Sprint 06. Media and attachment placeholders in 04; source context outside supplied patches in 06.

### CACHE-12

All history means all currently accessible historical items and their available data. It does not promise reconstruction of deleted comments or every overwritten PR revision. Optional future archival capture must be a separately defined feature.

Initial verification owner: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### ISSUE-01

Display title, body, author, timestamps, state, labels, assignees, milestone, available reactions, linked items, and supported timeline events. Display the full accessible paginated conversation, with explicit partial-download states.

Initial verification owner: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### ISSUE-02

Render Markdown natively, including headings, emphasis, links, lists, task lists, tables, quotations, code blocks, and images supported by the implementation. Text must be selectable and copyable. Use a readable source or unsupported-content fallback for features requiring additional rendering, such as complex embedded diagrams or HTML. Do not add a webview to improve parity.

Initial verification owner: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### UX-01

Support platform-appropriate keyboard navigation, focus indication, copy and paste, high-DPI displays, light and dark appearance, and sensible resizing. Test real text editors and review controls with the accessibility features used by the target audience on each published OS. Windows is verified first; macOS/Linux parity closes in their release phases.

Initial verification owner: Sprint 12. Keyboard, accessibility, DPI and appearance checked as native controls arrive; integrated release verification in 12.

### UX-02

Preserve user context while refreshing. Avoid replacing a useful cached view with a global spinner, jumping scroll positions, or losing a draft when selecting another tab. Loading and errors belong to the affected resource or pane; a slow request in one pane must not block navigation or editing in another.

Initial verification owner: Sprint 10. Context-preserving loading/error behavior must hold through all reader, writer, diff and history views.

### OFFLINE-01

Once an account and content have been established, the application MUST start and remain usable without a network check or renewed login blocking local access. Users can browse/search downloaded issues, PRs, discussions, reviews, commits, diffs, graph history, and the activity feed; use tabs and panes; edit local settings and drafts; and inspect or cancel pending work. State the cached coverage and last fetch honestly. Unseen remote content and new server events cannot be available until fetched. First-time authentication and initial acquisition of private content still require access; do not require a full repository download.

Initial verification owner: Sprint 12. Offline startup/workspace in 04; each new view and action integrates as built; complete all-feature cold-start offline matrix in 12.

### OFFLINE-03

Provide Download for offline use for a selected item, with the resource scope and revision shown before downloading. Include its accessible discussion and, for a selected PR revision, review/thread data, commit/file metadata and available patches; let users choose optional media or additional source context. Track progress and per-resource completion, expose caps or unavailable content, and resume interrupted downloads. An offline request to fetch missing data becomes a visible, cancellable pending download that starts when eligible. Download jobs are distinct from remote mutation actions. Never present partially fetched data as fully available or start a mandatory full-repository archive.

Initial verification owner: Sprint 06. Issue downloads and coverage in 04, scheduled resumption in 05, full PR resource scope and AC-70 in 06.

### OFFLINE-04

Show submitted local work immediately as clearly pending in all matching views: for example a pending comment, a review action, or an edit to an existing issue awaiting confirmation. Keep pending projections separate from confirmed cached server records; a refresh cannot erase them, and cancellation removes only the pending projection while preserving recoverable user text. A pending merge or approval is not a confirmed merge or approval. Offline pins and minimal draft/outbox dependencies must survive ordinary cache cleanup until explicitly released or the action is resolved.

Initial verification owner: Sprint 09. Storage protection in 02, pending-view foundation in 04, actual issue/comment/review/merge projections and cancellation integration in 07-09.

### CONNECT-01

Show a persistent, prominent green/red connection indicator in the application chrome, visible across panes, with an accessible icon and text label; color alone is insufficient. Green means recently verified GitHub API reachability. Red means Offline or Cannot reach GitHub after a current transport failure or an explicit Work offline choice. Use a neutral Checking or Status stale state before an initial result and when evidence is too old, rather than asserting a connection indefinitely. Show last checked time, last successful contact, reason, and Check now in its details, plus a pending-action count linking to the Outbox.

Initial verification owner: Sprint 05. Provider status contract in 03, persistent accessible chrome in 04, real shared observer and reachability classification in 05.

## Integration work owned elsewhere

### PORT-02

Share domain models, GitHub integration, cache/search, synchronization, Outbox, review/diff logic, history, notification rules, and workspace state across platforms. Isolate operating-system APIs behind small capability interfaces with conditional dependencies inside platform implementations. Common code must not depend on Windows handles, registry calls, COM types, fixed Windows paths, or platform-specific credential objects. Reuse portable GPUI facilities where suitable rather than wrapping the entire toolkit in a new framework. Test interface contracts with fakes and actual adapters; do not duplicate the product's business logic for each OS.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce platform boundaries in 01 and keep shared modules portable throughout; verify the full Windows implementation boundary in 12 and parity in later platform rows.

### PORT-03

Prove the actual native renderer and necessary GPUI/GPUI Kit components early with selectable Markdown, text input/copy, large annotated diffs, the bound PR context/code layout, resizing, focus, and a graph fixture. Windows proof is a Sprint 01 gate. Configure compilation/core-test jobs for all intended targets and attempt macOS/Linux native smoke launches early on real target desktops; record gaps honestly and resolve them before that platform's release. In-process/headless tests or a successful cross-compile are not native GUI proof. No phase may add an embedded browser engine as a portability shortcut. [S31] [S32] [S33]

Initial owner: Sprint 01. Initial verification: Sprint 12. Windows native proof begins in 01; expand actual controls as built. Early other-target smoke is tracked separately; full native release evidence closes at each platform gate.

### PORT-06

Implement and test platform-appropriate keyboard shortcuts, text input, accessibility, clipboard, high-DPI/scaling, theme changes, timezone defaults, external-link opening, startup registration, close versus Quit, background residence, sleep/wake and network-change hints. Feed normalized lifecycle events into the shared scheduler. Restore offline/pause settings before probes or dispatch. If a supported environment lacks a tray/menu-bar capability, keep the app reachable and explain background behavior rather than hiding an inaccessible running instance. Closing/quit must preserve drafts, workspace, Outbox and credentials according to the shared lifecycle contract. [S33] [S35]

Initial owner: Sprint 01. Initial verification: Sprint 12. Keep native interaction/lifecycle boundaries explicit from 01; complete Windows accessibility, restart and installed behavior in 12, and repeat for later targets.

### AUTH-11

Explicit sign-out MUST durably mark the affected account profile as signed out, prevent new network dispatch under that profile, and remove or invalidate its locally stored PAT material. Preserve readable private cached content, drafts, saved workspace, settings, synchronization checkpoints, activity baselines, and outbox records on the PC. Users may reopen that signed-out local profile for offline work without a renewed login; show its signed-out status and hold its remote actions until a PAT for the same verified user restores access. Keep that hold across restart. Ordinary exit is not sign-out. Expired or revoked PATs likewise preserve offline use and progress while holding affected remote work. A sign-out during an in-flight mutation does not prove remote cancellation; retain its evidence for reconciliation after access is restored. Do not expose one profile's data through a different active account or assume sign-out revokes the PAT at GitHub.

Initial owner: Sprint 03. Initial verification: Sprint 12. Signed-out credential/dispatch holds begin in 03; readable local-profile UI in 04 and all-writer PAT repair in 09; complete private-data retention and in-flight lifecycle matrix in 12.

### BROWSE-01

Users MUST be able to select, remove, and revisit accessible repositories. Show organization, repository name, access state, last successful check, and cached storage usage. Removing a monitored repository stops its polling; deleting its cached data is a separate explicit action.

Initial owner: Sprint 03. Initial verification: Sprint 05. Repository selection/storage usage in 03-04; removal stops the running scheduler in 05.

### WORKSPACE-06

Bind comments, issue edits, review submission, approvals, and merges to the originating tab's account, repository, item, and relevant revision. Changing focus while an operation is pending MUST NOT retarget it. Global navigation shortcuts apply to the focused pane; controls inside a pane apply to that pane's displayed item. Show the target repository and item in write confirmations where needed to avoid ambiguous actions.

Initial owner: Sprint 07. Initial verification: Sprint 09. Bind operation targets before writes; final comment/review/merge target checks in 09.

### WORKSPACE-07

Multiple views of the same resource MUST share cached domain data, operation outcomes, and persistent draft identities. Reopening the same compose or review draft must reuse its draft ID. Use one active editor per draft by default; other views show the current draft preview and a Focus editor action. Do not allow independent unsynchronized copies to overwrite the same draft or submit the same pending operation twice. Separate intentional drafts remain separate records.

Initial owner: Sprint 07. Initial verification: Sprint 09. Draft ownership primitives in 02 and local editors in 04; integrate shared pending issue/comment, review and merge operations in 07-09.

### WORKSPACE-10

Share request scheduling and deduplicate in-flight fetches across views of the same resource and revision. Opening additional panes MUST NOT create independent polling loops or duplicate alerts for that resource. Give visible tabs priority within the existing schedule and rate-limit budget. Inactive tabs must not retain every large diff or continuously redraw; evict reconstructible rendering state while preserving navigation state and drafts. Closing one view must not cancel work still needed by another view or the shared write journal.

Initial owner: Sprint 05. Initial verification: Sprint 12. Shared scheduling in 05 and large-view retention in 06/10; complete integrated twenty-tab performance gate in 12.

### CACHE-07

Existing-issue edit drafts, comment drafts, review drafts, and submitted outbox payloads are user data. Cache clearing, repair, failed downloads, migrations, reconciliation, and closing tabs or panes MUST preserve them unless the user explicitly deletes the drafts or cancels the actions. Drafts and outbox entries exist independently of any viewing tab. Preserve source anchors, edit base values, dependency mappings, and the minimal referenced content needed by unresolved actions; apply OUTBOX-10 to cleanup and recovery.

Initial owner: Sprint 02. Initial verification: Sprint 09. Draft/outbox-safe storage in 02; recheck cleanup and recovery for every draft and queued-action type through 09; repeat final migration matrix in 12.

### CACHE-08

Namespace records by host and account access context, with stable repository and resource identities. Issues and PRs share one repository-local number namespace. Map their API representations to a common local item while preserving distinct issue and pull-request API object IDs. Follow DATA-05 through DATA-09 for keys and ingestion. Never expose another account's private cached content after switching identities. [S27] [S28]

Initial owner: Sprint 02. Initial verification: Sprint 04. Storage scoping in 02, real identity switching in 03, cached browsing isolation in 04.

### CACHE-13

Normal window close and application exit MUST retain downloaded content and resource coverage, all draft types, saved tabs/layout/view positions, local preferences, offline pins, pending actions and receipts, synchronization checkpoints/validators, and notification baselines/delivery history on the PC. Persist settings and user changes as they occur and safely flush remaining local state on orderly exit; if a user-data write fails, keep it recoverable and report the failure. Retain the protected credential according to AUTH-07. Restart MUST reuse these records and render the saved local workspace before networking completes. It must not clear the cache, reset baselines, or redownload all history merely because the process was restarted. Sign-out preserves local content under AUTH-11. Explicit cache cleanup remains a separate action and continues to protect drafts and unresolved work under CACHE-07 and OUTBOX-10.

Initial owner: Sprint 02. Initial verification: Sprint 12. Complete persistence schema in 02, PAT lifecycle in 03, local-first restoration in 04, sync in 05/06, all writer/history/notification types through 11, and the complete quit/sign-out/reopen matrix in 12.

### OFFLINE-02

Offline mode MUST allow preparation and explicit submission of every implemented remote mutation: existing-issue editing/state/metadata, ordinary comments and edits, review comments/replies/submission/approval/request changes, permitted thread actions, and merges. Keep their controls available when cached target data and capability information are sufficient, and queue intent locally. Unknown permissions or missing required target/source details are explicit prerequisites; never invent a target, bypass a known denial, or silently choose a newer revision. Local settings, drafts, viewed-file markers, and app-feed read state apply locally immediately.

Initial owner: Sprint 07. Initial verification: Sprint 09. Local preparation begins in 04; all supported mutation intents become offline-submittable in 07-09 with honest prerequisites.

### SYNC-09

Run synchronization, connectivity checks, and the outbox worker in the application or optional tray process. A fully exited application or sleeping machine cannot perform local polling or send actions. Normal exit preserves local state and the protected PAT; it does not sign the user out. On launch or wake, restore the local workspace and persisted control settings first, then resume eligible actions and bounded incremental read catch-up under SYNC-11 when connectivity and authorization permit. No hosted service is required for the initial release.

Initial owner: Sprint 05. Initial verification: Sprint 12. Lifecycle scheduling begins in 05, all writers integrate by 09, graph/feed families by 11, and retained-credential plus local-first incremental restart acceptance closes in 12.

### SYNC-11

Resume from persisted per-account, per-repository, per-resource-family synchronization state on restart, reconnection, and same-user PAT renewal. Retain the last successful checkpoint, overlapping update window or supported cursor, pagination progress, conditional-request validators, resource completeness/freshness, and notification baselines. Advance content and its successful progress transactionally after each completed unit; failures or partial pages must not skip unseen changes. Prioritize visible resources and run bounded, rate-aware incremental catch-up, using supported changed-since/update feeds, conditional reads, and targeted reconciliation. Do not blindly redownload the entire cache, re-enumerate every historical discussion, reset notification baselines, or use the largest issue/PR number as the only cursor. If a cursor expires or an endpoint has no reliable delta interface, re-enumerate the necessary resource/range with stable-ID deduplication and preserved coverage while keeping other completed families intact. An update-feed page one, overlap, unchanged revalidation, or targeted repair may be necessary; no universal exact-delta or zero-unchanged-bytes guarantee is made. Preserve old-item update discovery and honor restored sign-out, offline, and pause state. [S06] [S07] [S11] [S13]

Initial owner: Sprint 05. Initial verification: Sprint 12. Persist checkpoints and stable account identity in 02/03; issue/comment restart catch-up in 05 and PR-family AC-75/76 in 06; all graph/notification families and full lifecycle request traces complete in 12.

### CONNECT-02

Derive reachability from verified HTTPS responses from the configured GitHub API and current transport errors, not from cached data or a network adapter icon alone. Successful responses, including 304, establish contact. A genuine API 401, authorization 403/404, 429, or 5xx also demonstrates a reachable API endpoint, while separate status explains Sign-in required, SSO approval, Throttled, or Service error and blocks affected writes as appropriate. DNS, connection, TLS, and timeout failures indicate inability to reach the service securely; they do not prove that every Internet destination is down. Request cancellation caused by tab navigation is not an offline signal. Suppress older/out-of-order observations so they cannot overwrite newer connectivity evidence; scope authorization errors separately by account/repository.

Initial owner: Sprint 03. Initial verification: Sprint 05. Typed provider evidence in 03; UI consumes it in 04 and full stale/out-of-order/transport/error cases close in 05.

### PR-05

The default PR review arrangement MUST show the native file list/diff prominently beside exactly one smaller context pane bound to the same active PR. At a usable window width, start with approximately 33% of the review content width for context and 67% for code, within a 30-35% / 65-70% range, excluding global navigation chrome. Context may be resized or collapsed; restore its last proportion. Provide View PR body and View issue controls in that one pane, with PR body as the initial mode for a newly opened review tab. They replace the context content rather than adding a simultaneous third pane. Preserve Conversation, Commits, Files, and Checks navigation and multiple PR tabs. At narrow widths, show a usable focused surface with an explicit way to switch context/code and restore the wider arrangement without losing either surface's state.

Initial owner: Sprint 06. Initial verification: Sprint 08. Native layout feasibility in 01 and workspace primitives in 04; bound context/code PR reader in 06; full AC-78 with actual inline drafts, focus, narrow layout and offline restore closes in 08.

### PR-07

View issue MUST display the selected verified issue's native body and available metadata/conversation in the context pane, with its repository, number, and coverage visible. View PR body restores that PR's body and context scroll state. Switching modes or linked issues must retain the code comparison, selected file, scroll/selection, review draft identity/anchors, and intended mutation target; background loads must not steal code-editor focus. Deliberate keyboard/pointer navigation may move focus normally. Use account, PR, body revision, selected issue, tab instance, and selection generation to discard stale context responses. Switching PR tabs must never show the previous PR's issue while loading. Distinguish no reference, verification pending, target is a PR, not downloaded/offline, inaccessible, failed, and removed or changed reference states; none is a confirmed empty issue. Cached PR bodies, linkage evidence, verified issues, and per-resource coverage remain usable offline with last-validation information. Preserve local drafts if body refresh removes a reference, and never silently attach them to a replacement issue.

Initial owner: Sprint 06. Initial verification: Sprint 08. Reference/cache/error and stale-response reading behavior closes AC-80 in 06; actual review-draft/focus stability and AC-78 close in 08.

### WRITE-02

Save a draft snapshot and durably enqueue the user's explicit submission before attempting a request. Preserve them after authorization errors, validation failures, offline transitions, server errors, and closing their tab. Enforce shared draft ownership and one operation identity across panes. Merely saving, editing, or reopening an unsubmitted draft MUST NOT enqueue it. Submitted eligible actions MUST resume automatically after restart or reconnection without asking the user to submit them again, subject to the outbox rules.

Initial owner: Sprint 07. Initial verification: Sprint 09. Local drafts/outbox persistence first; automatic replay, cancellation, preflight and uncertain-outcome handling expand from issues/comments in 07 to reviews/merges in 08-09.

### WRITE-03

A timed-out issue, comment, review, or merge operation may have succeeded remotely. Keep the submitted operation in the outbox and automatically reconcile reliable identities or server state before considering another attempt. Retry automatically only when the action is known not to have been applied or the provider has a verified safe idempotency mechanism. If the result remains ambiguous, show Outcome unknown and allow inspection or stopping further attempts without claiming remote undo. Do not blindly replay a non-idempotent request; matching text alone is not proof of success. A deliberate retry of an unresolved action must explain the possible duplicate and cannot be triggered by reconnection alone.

Initial owner: Sprint 07. Initial verification: Sprint 09. Local drafts/outbox persistence first; automatic replay, cancellation, preflight and uncertain-outcome handling expand from issues/comments in 07 to reviews/merges in 08-09.

### WRITE-04

Preserve concurrent remote edits, submitted intent, and local input. Revalidate affected fields, permissions, and revisions immediately before dispatch, including automatic replay; explain conflicts and keep the action visible as Needs attention. Retain the draft for correction. Document unavoidable races where GitHub supplies no atomic conditional operation. A temporary connectivity interruption alone must not require resubmission of an already queued action.

Initial owner: Sprint 07. Initial verification: Sprint 09. Local drafts/outbox persistence first; automatic replay, cancellation, preflight and uncertain-outcome handling expand from issues/comments in 07 to reviews/merges in 08-09.

### OUTBOX-01

Route every explicit remote mutation through a durable SQLite outbox before sending, online as well as offline. Commit a stable operation ID, immutable submitted payload/version, original account and target identities, base values or commit revision, dependency references, draft snapshot, and locally pending projection atomically. Acknowledging Queued requires a successful durable commit; if storage is unavailable or full, keep the editor recoverable and clearly report that the action was not queued. Draft autosave does not authorize dispatch. The explicit action submission is authorization for its automatic later execution, without a routine second confirmation on reconnect.

Initial owner: Sprint 02. Initial verification: Sprint 09. Durable schema and operation/comment/review step identities in 02; offline UI foundation in 04; full lifecycle, replay, dependencies and step receipts across writers in 07-09.

### OUTBOX-02

Use a recoverable operation lifecycle: Queued (including waiting for connection, dependency, pause or retry time), Validating (fresh read/preflight), Sending (a mutation may reach GitHub), Outcome unknown (automatic reconciliation), Waiting for GitHub (accepted asynchronous work), Needs attention (blocked by auth, conflict, unsupported capability or permanent validation failure), Confirmed, and Cancelled. Store a machine-readable reason independently of the state. Unsubmitted Draft is not an outbox entry. Confirmed and Cancelled are terminal only when justified by evidence; an ambiguous or remotely pending action remains inspectable. Exhausted transient retries wait in Queued and resume automatically when eligible.

Initial owner: Sprint 02. Initial verification: Sprint 09. Durable schema and operation/comment/review step identities in 02; offline UI foundation in 04; full lifecycle, replay, dependencies and step receipts across writers in 07-09.

### OUTBOX-03

Provide an Outbox/Pending actions panel that works offline. Show all unresolved submitted actions for the active account, with type, repository, existing target item/comment/thread, payload preview, intended revision/method, state and reason, dependencies, creation/attempt times, and next eligible attempt. Support filters, opening the originating item, individual cancellation, Pause/Resume queued actions, and Retry now when safe. Explain any unavailable control. Pending counts and row states update across panes; confirmed/cancelled history remains inspectable under a documented retention policy. Returning cancelled content to a draft is supported; deleting drafts is separate.

Initial owner: Sprint 07. Initial verification: Sprint 09. Offline panel shell in 04; real inspection/cancel controls in 07; verify all supported action types in 09.

### OUTBOX-06

Preserve dependency order for related supported operations while allowing unrelated eligible work to progress under the shared budget. Retain stable local operation/draft identities and record confirmed server identities for comment, review, and thread steps where a later supported operation requires them. Preserve review/thread sequencing and meaningful per-target edit order, including proven predecessor results in later preflight comparisons. Persist step identities and receipts for multi-request workflows; resume eligible unfinished steps rather than repeating completed steps. Show partially applied remote effects explicitly. When a predecessor is cancelled, rejected, or uncertain, hold dependent actions with a clear reason; never guess a remote target or drop descendants silently. Cancelling a parent must show its dependent impact, and users may inspect/cancel each remaining dependent. New-issue placeholders, local issue creation IDs, and create-issue dependency chains are not part of this contract.

Initial owner: Sprint 02. Initial verification: Sprint 09. Durable schema and operation/comment/review step identities in 02; offline UI foundation in 04; full lifecycle, replay, dependencies and step receipts across writers in 07-09.

### OUTBOX-09

Bind each action permanently to the original host, user/account context, repository/resource identity, and credential owner scope; resolve credentials by protected reference at dispatch, never from a stored token in the payload. Switching users pauses the inactive account's work and isolates its queue contents. Reauthorization may resume the original user's eligible work; another user cannot inherit it. Removing a repository selection preserves its pending work and marks it paused for scope review instead of deleting or secretly sending it. Confirmed locator changes use verified identity and preserve the queue's stable local target.

Initial owner: Sprint 03. Initial verification: Sprint 09. Original-user credential/scope binding begins in 03; panel isolation in 04; deferred issue/review/merge dispatch verified through 09.

### OUTBOX-10

Treat unresolved outbox operations, payloads, cancellation flags, dependency mappings, base snapshots, anchors, and remote reconciliation evidence as durable user data. Protect them from cache eviction, repair resets, account sign-out, and migration. Recover interrupted claims conservatively after a crash; an interrupted Sending state becomes Outcome unknown unless evidence proves it was never dispatched. Reconcile a successful remote write whose local completion transaction failed. Pending actions have no silent age expiry. Bound and virtualize the outbox UI, redact routine diagnostics, and preserve essential local content until completion/cancellation and any dependent work are settled.

Initial owner: Sprint 02. Initial verification: Sprint 12. Persist/protect queue data in 02, integrate all writer snapshots/receipts in 07-09, then verify full upgrade/cleanup/crash/backlog matrix in 12.

### DATA-03

Keep secrets and private content out of routine diagnostic output. Export diagnostics only through an explicit user action with appropriate redaction. Treat remote Markdown, links, and filenames as data rather than instructions or executable paths.

Initial owner: Sprint 02. Initial verification: Sprint 12. Establish redaction/content safety boundaries early; verify all parsers and final explicit diagnostic export in 12.

### DATA-04

Credential switching must isolate private cached data, drafts, and queued actions. Bind each action to its original GitHub user/account access context and credential owner scope. Pause dispatch for inactive identities, signed-out profiles, or known revoked access; resume eligible work under the same verified identity after PAT repair and subject to persisted controls. Replace credentials for the same user without recreating the account, item identities, cache, outbox, or synchronization baselines. Never reroute queued work through another signed-in user. Failed authorization and explicit sign-out preserve local content under AUTH-11; a signed-out local profile remains readable without exposing it through another active account.

Initial owner: Sprint 03. Initial verification: Sprint 09. Identity isolation in 03; verify cached workspace, drafts and deferred issue/review/merge execution remain bound to the original account.

### DATA-05

Store the visible GitHub number separately from the local primary key and API object IDs. GitHub issues and PRs use the same increasing number sequence within a repository: an issue numbered 41 can be followed by PR 42 and issue 43. There cannot be two distinct items consisting of issue 42 and PR 42 in that repository. Enforce UNIQUE(repository_id, number), without kind in that constraint. Here repository_id identifies a local repository record already scoped to host and account access context. The same number in a different repository or account context must not collide. [S27]

Initial owner: Sprint 02. Initial verification: Sprint 04. Database identity contract in 02; live provider and browsing ingestion in 03/04.

### DATA-06

Use an items table for shared fields and a pull_request_details table whose item_id is both its primary key and a foreign key to items. Only confirmed PR items may have this extension. The item kind distinguishes an ordinary issue from a PR once classified. PR summaries may exist before PR details are fetched; a navigation placeholder may remain unclassified until metadata arrives. Track missing detail separately from known false or null values. Keep PR merge state and source references in the extension, and derive the displayed merged state from confirmed PR data. A common closed-state update must not erase known merge information.

Initial owner: Sprint 02. Initial verification: Sprint 06. Storage fixtures first; live issue-shaped PR ingestion in 04 and PR endpoint/details integration in 06.

### DATA-07

Issue endpoints can return PRs, identified by their pull_request property. For that representation, the REST id is an issue ID; the pull-request endpoint supplies the separate PR ID. Store these in distinct typed fields or identity mappings and retain node IDs as opaque provider identifiers. If PR data arrives first, its issue-endpoint ID may remain unknown until fetched. Upsert both representations to the same item using the scoped repository number and verified provider identities. Resolve concurrent ingestion transactionally. Distinguish absent fields from explicit nulls and reject stale overwrites using applicable revision or freshness evidence. Never substitute one ID for the other, create a duplicate row for the second representation, downgrade a known PR because a partial payload omits its marker, or mark unrelated resource families complete. [S28]

Initial owner: Sprint 02. Initial verification: Sprint 06. Storage fixtures first; live issue-shaped PR ingestion in 04 and PR endpoint/details integration in 06.

### DATA-08

Store ordinary issue and PR conversation comments in a shared conversation-comments model keyed to item_id. Formal reviews and inline review comments remain distinct resources with their own provider identity namespaces and source anchors. Share common labels, assignees, search, drafts, and workspace references through the item identity. Route each remote operation by its API capability; a shared table does not make PR review or merge operations valid for ordinary issues. [S06] [S09] [S10] [S13]

Initial owner: Sprint 02. Initial verification: Sprint 09. Shared comments and typed resources in storage/reads; review/merge guards complete in 09.

### DATA-09

Use the stable local item_id for relationships, including drafts, tabs, comments, fetch state, and search records. The repository number is the current locator. Resolve a confirmed issue transfer or redirect through verified provider identity before changing that locator; preserve local relationships and the previous locator as an alias where appropriate. Never combine unrelated items merely because their numbers match, and never treat repository names or URLs as immutable database identity. If identity cannot be confirmed, preserve the existing record and surface the unresolved relocation. [S28]

Initial owner: Sprint 02. Initial verification: Sprint 04. Fixture transfer in 02; provider resolution and workspace/search/draft references integrated in 03/04.

### DATA-10

Store derived PR-body issue references separately from confirmed item identity and server relationship facts. Key a candidate set by source PR item_id and body revision/hash, preserve raw/reference position and provenance, and retain verification state and the verified target issue item_id when available. Recompute when the source body changes; retain unchanged verified targets and selection where still valid, but never associate a prior PR's or prior body's removed target with the current context. Cache reference evidence for offline use with its last validation, without marking current remote access or auto-closing behavior confirmed. Use the existing account/repository identity and transfer rules; do not create a new ordinary issue record or change a known PR kind merely to satisfy a parsed #number. Drafts and outbox actions keep their original stable targets even if a reference disappears. [S27] [S28] [S29] [S30]

Initial owner: Sprint 02. Initial verification: Sprint 06. Reference/provenance and view-state schema in 02; local state foundations in 04; real PR-body parsing, verified issue identities, changed-body reconciliation and offline context in 06.

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

BROWSE-02, WORKSPACE-01, WORKSPACE-02, WORKSPACE-03, WORKSPACE-04, SEARCH-02, SEARCH-03, SEARCH-04, CACHE-04, CACHE-08, CACHE-12, ISSUE-01, ISSUE-02, DATA-05, DATA-09.

Use requirement_platform_traceability.csv for applicability and release-gate evidence. Initial ledger statuses refer only to their initial phase; no single Windows result closes all platforms. Keep future options and unavailable services explicit.

## Later scope and handoff

Real connectivity closes in Sprint 05. PR resources and their offline-download scope arrive in Sprint 06. The working action outbox arrives in Sprint 07, reviews in Sprint 08, merges in Sprint 09, graph-specific offline views in Sprint 10, and activity-feed integration in Sprint 11. Full offline acceptance closes in Sprint 12. Re-run cleanup for each new draft/action type.

## Coding LLM instruction

Implement Sprint 04 against v1.7. Keep the app buildable and preserve the current product constraints. Complete listed implementation and test work, use actual native controls for E2E, protect original targets/drafts/Outbox state, and add only the named platform integrations. Do not add issue/PR creation, merge queues or browser engines. Keep secrets out of code/results; live writes require explicit allowlisted authorization. Implement the short verified prebuilt install/update contract for the relevant publication phase and never claim placeholder commands runnable. Update initial/per-platform/scenario ledgers with commands, environment, package identity and evidence, then use SPRINT_REPORT_TEMPLATE.md. Continue only within the assigned sprint/Windows/all-platform scope without new routine approvals.

## Technical sources

- [S06 GitHub issue and PR conversation comments](https://docs.github.com/en/rest/issues/comments)
- [S07 GitHub REST pagination](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api)
- [S08 GitHub REST search](https://docs.github.com/en/rest/search/search)
- [S09 GitHub pull request reviews](https://docs.github.com/en/rest/pulls/reviews)
- [S10 GitHub pull request reads and merges](https://docs.github.com/en/rest/pulls/pulls)
- [S11 GitHub REST best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)
- [S13 GitHub inline review comments](https://docs.github.com/en/rest/pulls/comments)
- [S27 GitHub shared issue and PR numbering](https://docs.github.com/en/enterprise-cloud%40latest/rest/using-the-rest-api/issue-event-types)
- [S28 GitHub issue representations identifiers and updates](https://docs.github.com/en/rest/issues/issues)
- [S29 GitHub linking pull requests and issues](https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/linking-a-pull-request-to-an-issue)
- [S30 GitHub repository auto-close settings](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/managing-repository-settings/managing-auto-closing-issues)
- [S31 GPUI Kit installation and target prerequisites](https://gpui-kit.com/docs/installation/)
- [S32 GPUI Kit testing facilities and limits](https://gpui-kit.com/docs/test/)
- [S33 GPUI Kit accessibility](https://gpui-kit.com/docs/accessibility/)
- [S35 GPUI Kit packaging](https://gpui-kit.com/docs/packaging/)

Additional current primary sources and platform/install/test constraints are in the three companion plans. Verify pinned tool/API behavior during implementation.
