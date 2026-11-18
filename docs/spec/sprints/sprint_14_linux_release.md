# Sprint 14: Linux parity, native testing, and simple installation

Requirements version 1.7 | Initial sprint status Planned

## Outcome

A published Linux release supports a bounded tested desktop/session matrix with full product parity, native services, and a simple verified prebuilt installation.

## Phase and prerequisites

- Release phase: Linux release.
- Prerequisites: Sprint 12, Sprint 13.
- Code areas: platform_linux, shared UI/core regressions, packaging/Linux, tests/native/session and installer, release documentation.
- Read the master, TESTING_PLAN.md, INSTALLATION_PLAN.md, CROSS_PLATFORM_PLAN.md and all relevant scenario/platform ledgers.
- Use actual prior evidence; a missing dependency remains blocked rather than passed. Later-platform release work does not silently move into the Windows phase.

## Implementation tickets

### S14-T01

Validate and publish the actual Linux distribution/version/architecture/desktop/session and graphics prerequisites. Start from Ubuntu 24.04 x64 GNOME with separately tested Wayland/X11 configurations; do not imply support for every Linux desktop or require a source build.

### S14-T02

Implement the supported secure credential service, persistent XDG paths, native input/accessibility/clipboard/scaling, single-instance and external activation, startup/background/close/Quit/wake behavior. If required services are absent or locked, retain offline reading and provide exact guidance without plaintext tokens or silent system modifications.

### S14-T03

Verify renderer, large diffs, tabs/context switching and notification delivery/actions in real graphical sessions. Report replacement/retraction/tray limitations explicitly and supply required in-app alternatives while preserving feed, rules and correct item routing.

### S14-T04

Verify the same cache/search/data model, review/diff/graph, existing-issue/comment/review/direct-merge, offline Outbox and incremental restart workflows as Windows/macOS. Maintain one shared domain implementation and no browser engine.

### S14-T05

Publish verified prebuilt Linux artifacts, a versioned manifest and the one-to-three-command user install/update route. Safely stage/extract/install, check target/runtime prerequisites, preserve user data on rerun/update, and document uninstall/reinstall without silent elevation or package-source changes.

### S14-T06

Run all applicable unit/integration/native-process E2E/install/live/performance/accessibility cases in each claimed session/configuration, including exact published-package smoke and failure recovery. Re-run Windows/macOS shared-code regressions and complete Linux phase evidence before any all-platform completion claim.

## Required tests and commands

- Rerun all applicable unit/integration/native E2E/live cases on each claimed Linux desktop/session configuration, including secure store, graphics/input, lifecycle and notification capabilities.
- Installer: exact verified published artifact and quick install/update on clean user targets; prove safe prerequisite errors, staging/extraction, data retention and uninstall/reinstall.
- Measure Linux budgets, close Linux phase rows and recheck already released Windows/macOS regressions before declaring all-platform completion.

Implement and use the canonical runner from TESTING_PLAN.md: unit, integration, ui, native e2e smoke/full, install with an artifact, and separately guarded live tests. These are development commands to build, not commands already supplied by this ZIP. Record actual OS/build/fixture evidence and mark unavailable checks Blocked/Not run. Native test-build E2E and exact published-package smoke remain separate.

Scenario IDs introduced or closing here: UT-001, UT-002, UT-003, UT-004, UT-005, UT-006, UT-007, UT-008, UT-009, UT-010, UT-011, UT-012, UT-013, UT-014, UT-015, UT-016, UT-017, UT-018, UT-019, UT-020, UT-021, IT-001, IT-002, IT-003, IT-004, IT-005, IT-006, IT-007, IT-008, IT-009, IT-010, IT-011, IT-012, IT-013, IT-014, IT-015, IT-016, IT-017, IT-018, IT-019, IT-020, IT-021, IT-023, UI-001, UI-002, UI-003, UI-004, UI-005, UI-006, UI-007, UI-008, E2E-001, E2E-002, E2E-003, E2E-004, E2E-005, E2E-006, E2E-007, E2E-008, E2E-009, E2E-010, E2E-011, E2E-012, E2E-013, E2E-014, E2E-015, E2E-017, E2E-018, E2E-019, E2E-020, PKG-002, PKG-003, PKG-004, PKG-005, PKG-006, PKG-008, PKG-009, LIVE-001, LIVE-002, LIVE-003, LIVE-004.

Live-provider cases are shared gates on an explicitly authorized host. A later platform can reference existing evidence only with a recorded applicability assessment; rerun changed provider/credential-policy or target-specific behavior. Missing authorization remains Blocked/Not run and never permits unapproved writes.

Use test_scenarios.csv for fixture IDs, expected results, first/closure ownership and evidence. Release sprints rerun all scenarios applicable to their target; earlier sprint scenario lists do not waive regression checks.

## Deliverables

Published Linux package and verified quick-install/update/uninstall documentation; supported service/session matrix; Linux phase ledgers; native/package/performance evidence and all-published-platform regression report.

## Exit gates

- Pass AC-88 and every Linux-applicable existing product/test/install scenario, including AC-36/38, AC-73/74 and AC-83 through AC-99 except macOS-only AC-87, on each claimed session/configuration.
- Use real installed native processes for GUI journeys and test exact published commands on clean standard-user targets without Rust tools. Missing graphics/credential/notification prerequisites must fail truthfully without weakening OS security.
- Close Linux rows only with their own evidence; verify Windows/macOS regressions for shared changes. All-platform completion requires all three published-phase ledgers, not one build or a shared-source claim.

## Initial acceptance scenarios owned here

- **AC-88** The Linux milestone passes applicable product/credential/lifecycle/notification/native E2E/performance/installation gates for every claimed distribution/desktop/session/architecture combination. Missing graphics, secure-store or system prerequisites produce truthful guidance; untested variants are not declared supported. Previously released platform regressions are checked.

## Platform release rerun

Run every applicable acceptance scenario in acceptance_platform_traceability.csv for linux. This is additional target evidence, not reassignment of its original closure owner. Other-platform-only scenarios are explicitly Not applicable. Applicable IDs: AC-01, AC-02, AC-03, AC-04, AC-05, AC-06, AC-07, AC-08, AC-09, AC-10, AC-11, AC-12, AC-13, AC-14, AC-15, AC-16, AC-17, AC-18, AC-19, AC-20, AC-21, AC-22, AC-23, AC-24, AC-25, AC-26, AC-27, AC-28, AC-29, AC-30, AC-31, AC-32, AC-33, AC-34, AC-35, AC-36, AC-37, AC-38, AC-39, AC-40, AC-41, AC-42, AC-43, AC-44, AC-45, AC-46, AC-47, AC-48, AC-49, AC-50, AC-51, AC-52, AC-53, AC-54, AC-55, AC-56, AC-57, AC-58, AC-59, AC-60, AC-61, AC-62, AC-63, AC-64, AC-65, AC-66, AC-67, AC-68, AC-69, AC-70, AC-71, AC-72, AC-73, AC-74, AC-75, AC-76, AC-77, AC-78, AC-79, AC-80, AC-81, AC-82, AC-83, AC-84, AC-85, AC-86, AC-88, AC-89, AC-90, AC-91, AC-92, AC-93, AC-94, AC-95, AC-96, AC-97, AC-98, AC-99.

## Full owned requirements

### PORT-08

Sprint 14 MUST deliver a tested Linux package for the declared support matrix with the same applicable product workflows and correctness safeguards. Verify the selected credential service, application-data paths, graphics renderer, X11/Wayland sessions claimed as supported, input/accessibility, notification/activation and background behavior. Bundle application dependencies where appropriate and give precise prerequisite/unsupported-environment guidance where system components are required. Complete one-to-three-command installation/update and native-process end-to-end/performance/upgrade evidence on each claimed configuration. Do not label untested distributions, sessions or architectures supported or require a repository clone/source build for users.

Initial verification owner: Sprint 14. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

## Integration work owned elsewhere

### FND-01

The shipped application MUST have a native interface and no runtime dependency on a browser engine. Rust with GPUI is the implementation default. A different native approach requires a documented concrete feasibility reason and must preserve the no-browser constraint. An explicit sign-in or external-link action may open the user's existing default browser; this does not embed a browser in the application.

Initial owner: Sprint 01. Initial verification: Sprint 12. Native feasibility in 01; shipped dependency/process inventory in 12.

### FND-02

Use GPUI and GPUI Kit as the initial GUI choice. Validate selectable rich text, a large annotated diff, two simultaneously visible panes with independent tabs and focus, a PR review fixture with a smaller switchable context surface beside prominent code, Windows notifications, tray behavior, and click activation before building the entire UI. Pin a tested compatible dependency set and commit the lockfile. GPUI Kit provides native text and diff primitives, but GitHub workflow integration remains application work. [S01] [S02] [S03]

Initial owner: Sprint 01. Initial verification: Sprint 01. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### FND-03

Use SQLite with migrations, foreign keys, suitable indexes, and bounded connections. WAL allows reads during writes; FTS5 supports local full-text search. Keep transactions short and perform database work outside the UI thread. [S04] [S05]

Initial owner: Sprint 02. Initial verification: Sprint 02. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### FND-04

Use Tokio for asynchronous application work and reqwest for HTTP. Use typed request and response models, explicit cancellation, structured errors, and a centralized request scheduler. A different library is acceptable only with a recorded compatibility reason.

Initial owner: Sprint 01. Initial verification: Sprint 05. Background/provider contracts begin in 01; complete central scheduler in 05.

### FND-05

Use direct GitHub APIs. Do not scrape rendered GitHub pages or depend on a user's browser session. Keep REST and GraphQL behind a provider interface with explicit capabilities and version handling.

Initial owner: Sprint 03. Initial verification: Sprint 03. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### FND-06

UI events MUST enqueue or invoke application operations without performing blocking work. Results MUST carry resource identity and revision. View-specific work must also retain a tab-instance identity and selection generation so a late response cannot replace another tab's content or a newer selection. Shared resource results may update the cache and notify all matching views without changing their selection or focus.

Initial owner: Sprint 04. Initial verification: Sprint 09. Carry view identity, resource revision, cache publication, and context preservation through all read/write workflows.

### FND-07

The cache is the normal read source. Successful reads and writes update persistent records, then notify subscribed views. Preserve scroll position, selected file, and unfinished input when background refresh completes.

Initial owner: Sprint 04. Initial verification: Sprint 09. Carry view identity, resource revision, cache publication, and context preservation through all read/write workflows.

### PORT-01

Windows is the highest-priority first release, completed through Sprints 01-12. macOS in Sprint 13 and Linux in Sprint 14 are required later product milestones, not optional replacements for Windows. Publish an explicit support matrix covering OS version, CPU architecture, packaging and, for Linux, supported distribution/desktop/session/graphics prerequisites. Use the provisional targets in Section 3 until validated. Keep Windows completion distinct from later-platform readiness; no supported-platform claim is earned by compiling alone, and no later-platform release gate silently moves into the Windows phase.

Initial owner: Sprint 01. Initial verification: Sprint 12. Define phased scope and support matrix in 01; verify Windows phase statement in 12. macOS/Linux publication closes their separate platform rows in 13/14.

### PORT-02

Share domain models, GitHub integration, cache/search, synchronization, Outbox, review/diff logic, history, notification rules, and workspace state across platforms. Isolate operating-system APIs behind small capability interfaces with conditional dependencies inside platform implementations. Common code must not depend on Windows handles, registry calls, COM types, fixed Windows paths, or platform-specific credential objects. Reuse portable GPUI facilities where suitable rather than wrapping the entire toolkit in a new framework. Test interface contracts with fakes and actual adapters; do not duplicate the product's business logic for each OS.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce platform boundaries in 01 and keep shared modules portable throughout; verify the full Windows implementation boundary in 12 and parity in later platform rows.

### PORT-03

Prove the actual native renderer and necessary GPUI/GPUI Kit components early with selectable Markdown, text input/copy, large annotated diffs, the bound PR context/code layout, resizing, focus, and a graph fixture. Windows proof is a Sprint 01 gate. Configure compilation/core-test jobs for all intended targets and attempt macOS/Linux native smoke launches early on real target desktops; record gaps honestly and resolve them before that platform's release. In-process/headless tests or a successful cross-compile are not native GUI proof. No phase may add an embedded browser engine as a portability shortcut. [S31] [S32] [S33]

Initial owner: Sprint 01. Initial verification: Sprint 12. Windows native proof begins in 01; expand actual controls as built. Early other-target smoke is tracked separately; full native release evidence closes at each platform gate.

### PORT-04

Implement protected credential and application-data adapters per published OS. Use Windows Credential Manager or equivalent on Windows, the platform keychain on macOS, and a documented supported secure credential service on Linux. Store only credential references in SQLite; never fall back silently to plaintext tokens when a store is unavailable or locked. Preserve offline cached access while affected network work is held. Use appropriate per-user data/config/cache locations, stable app identity and relative managed asset paths rather than Windows-specific absolute paths. Test filesystem permissions, file locking, SQLite WAL/FTS availability, path/Unicode handling, and stable location across upgrades. Credentials and caches are per-device; this does not add cross-device synchronization. [S36]

Initial owner: Sprint 03. Initial verification: Sprint 12. Path/storage contracts in 02 and real Windows credential tests in 03; installed path and retention behavior closes in 12, with OS adapters in 13/14.

### PORT-05

Keep notification eligibility, filtering, grouping, delivery history and item routing shared, with per-OS notification/activation implementations. Verify permissions, native delivery, actions, cold/warm launch, single-instance routing, and any supported identity/replacement semantics in the packaged target app. Unsupported OS capabilities must have truthful capability reporting and usable in-app alternatives for actions such as Snooze and Mark read; no silent loss of activity or false replacement/retraction promise. Define supported desktop behavior explicitly before publishing, preserving the persistent feed and correct Open behavior. A toolkit method or Windows test does not prove another OS's integration. [S34]

Initial owner: Sprint 11. Initial verification: Sprint 12. Windows notification feasibility in 01, actual product delivery in 11, and exact installed-package activation in 12; later OS parity closes at its release.

### PORT-06

Implement and test platform-appropriate keyboard shortcuts, text input, accessibility, clipboard, high-DPI/scaling, theme changes, timezone defaults, external-link opening, startup registration, close versus Quit, background residence, sleep/wake and network-change hints. Feed normalized lifecycle events into the shared scheduler. Restore offline/pause settings before probes or dispatch. If a supported environment lacks a tray/menu-bar capability, keep the app reachable and explain background behavior rather than hiding an inaccessible running instance. Closing/quit must preserve drafts, workspace, Outbox and credentials according to the shared lifecycle contract. [S33] [S35]

Initial owner: Sprint 01. Initial verification: Sprint 12. Keep native interaction/lifecycle boundaries explicit from 01; complete Windows accessibility, restart and installed behavior in 12, and repeat for later targets.

### AUTH-01

The application MUST support GitHub.com accounts accessing SSO-protected organization repositories. Use `https://api.github.com` for REST, `https://api.github.com/graphql` for GraphQL, and `https://github.com` as the authorization web origin. Validate credential routing and any authorized redirects. GitHub Enterprise Server compatibility is not a first-release requirement.

Initial owner: Sprint 03. Initial verification: Sprint 03. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### AUTH-02

Authenticate as the user for issue changes, reviews, approvals, and merges. Show the active identity and relevant organization or credential profile. Do not submit the user's reviews as an installation bot identity.

Initial owner: Sprint 03. Initial verification: Sprint 09. Verify user identity and actual endpoint permissions for issue, review, and merge actions as each becomes available.

### AUTH-03

Use a personal access token as the confirmed first-release authentication path. Prefer a fine-grained PAT when all required endpoints and organization policy support it; support an organization-approved classic PAT when endpoint coverage or organization compatibility requires that alternative. Do not switch to OAuth or device authorization as an unrequested first-release dependency. A fine-grained token can be limited to one resource owner and selected repositories, so credential profiles must route appropriate PATs for different selected owners without sending an unrelated token. [S20]

Initial owner: Sprint 03. Initial verification: Sprint 03. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### AUTH-04

For organizations using SAML SSO, account for the distinction between token creation, SSO authorization, and organization approval. Fine-grained tokens receive SSO authorization during creation; classic tokens require separate SSO authorization. Fine-grained tokens may also require an organization owner's approval before private resources are accessible. [S21] [S22]

Initial owner: Sprint 03. Initial verification: Sprint 03. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### AUTH-05

Handle expired or revoked tokens, insufficient permissions, pending approval when identifiable, organization restrictions, and required SSO authorization as distinct actionable states. Interpret `X-GitHub-SSO` when present, including partial results. A 403 or 404 caused by authorization MUST NOT be treated automatically as resource deletion. [S21]

Initial owner: Sprint 03. Initial verification: Sprint 03. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### AUTH-06

Provide a user-initiated action to open an appropriate GitHub PAT or SSO authorization page in the external default browser. Validate returned authorization URLs before opening them. Preserve the current screen, cached content, drafts, outbox, and synchronization checkpoints while authorization is repaired. After verifying that a replacement PAT belongs to the same GitHub user and restores the required access, reuse the existing account/resource identities and progress, retry eligible reads, and automatically resume eligible submitted actions under the persisted pause/offline settings. A different user must receive a separate account context; replacing a token must not create a new cache for the same user.

Initial owner: Sprint 03. Initial verification: Sprint 09. Authorization repair restores the same identity in 03; reconnect signals in 05 and automatic eligible outbox resumption across all writers in 07-09.

### AUTH-07

Store PATs through the platform credential adapter in Windows Credential Manager or an equivalent native protected store; published macOS/Linux builds follow PORT-04. Persist only credential references in SQLite. Normal window close or application exit retains the protected token so the next launch can resume without re-entry while it remains valid. Explicit sign-out follows AUTH-11 and clears the relevant local credential material. Never include tokens in logs, URLs, diagnostics, exports, or source control.

Initial owner: Sprint 03. Initial verification: Sprint 03. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### AUTH-08

Request and document the permissions actually required by the chosen endpoints. Issue writes, PR review writes, commit access, and merging must each be validated against current endpoint documentation. Check access to an explicitly selected private repository, not merely whether `/user` succeeds. Preserve readable functions when a write permission is missing. [S06] [S09] [S10]

Initial owner: Sprint 03. Initial verification: Sprint 09. Verify user identity and actual endpoint permissions for issue, review, and merge actions as each becomes available.

### AUTH-09

Custom activity notifications MUST work independently of GitHub's own notifications inbox. Do not require a classic token solely to show the app's custom pop-ups. Synchronizing GitHub notification read/unread state is optional and has separate token compatibility constraints. [S14]

Initial owner: Sprint 11. Initial verification: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### AUTH-10

OAuth or device authorization is optional future scope and is inactive for the confirmed PAT-based first release. Retain a provider boundary that permits a separately authorized future registered GitHub OAuth or GitHub App integration. If that scope is explicitly activated later, verify its SSO, organization approval, and user-identity behavior and keep login secrets out of the distributed binary. Distribution alone does not activate this requirement. [S23]

Initial owner: Sprint 12. Initial verification: Sprint 12. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### AUTH-11

Explicit sign-out MUST durably mark the affected account profile as signed out, prevent new network dispatch under that profile, and remove or invalidate its locally stored PAT material. Preserve readable private cached content, drafts, saved workspace, settings, synchronization checkpoints, activity baselines, and outbox records on the PC. Users may reopen that signed-out local profile for offline work without a renewed login; show its signed-out status and hold its remote actions until a PAT for the same verified user restores access. Keep that hold across restart. Ordinary exit is not sign-out. Expired or revoked PATs likewise preserve offline use and progress while holding affected remote work. A sign-out during an in-flight mutation does not prove remote cancellation; retain its evidence for reconciliation after access is restored. Do not expose one profile's data through a different active account or assume sign-out revokes the PAT at GitHub.

Initial owner: Sprint 03. Initial verification: Sprint 12. Signed-out credential/dispatch holds begin in 03; readable local-profile UI in 04 and all-writer PAT repair in 09; complete private-data retention and in-flight lifecycle matrix in 12.

### BROWSE-01

Users MUST be able to select, remove, and revisit accessible repositories. Show organization, repository name, access state, last successful check, and cached storage usage. Removing a monitored repository stops its polling; deleting its cached data is a separate explicit action.

Initial owner: Sprint 03. Initial verification: Sprint 05. Repository selection/storage usage in 03-04; removal stops the running scheduler in 05.

### BROWSE-02

Repository lists MUST support issues and PRs in all states, with no hidden historical date cutoff. Fetch pages as users browse. Cache returned summaries immediately, and provide clear loading, empty, partial, offline, and error states. Identify PR records returned by issue endpoints, upsert them into the common item store, and apply the requested issue-only, PR-only, or combined view without duplicating an item. [S28]

Initial owner: Sprint 04. Initial verification: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### BROWSE-03

Provide saved views for authored items, assignments, mentions or participation where supported, and requested reviews. Provide repository, state, author, label, assignee, reviewer, and date filters when supported by the selected query mode. Distinguish filtering downloaded data from querying GitHub.

Initial owner: Sprint 04. Initial verification: Sprint 06. Saved views and supported filters begin in 04; requested-review metadata integration in 06.

### BROWSE-04

Support recent items, pinned items, back and forward navigation, and opening an accessible GitHub issue or PR URL through the workspace rules below. Keyboard navigation must reach repository lists, item lists, pane controls, item tabs, PR subviews, search, and the review editor.

Initial owner: Sprint 04. Initial verification: Sprint 08. Navigation begins in 04; verify access to actual PR subviews and review editor in 06/08.

### BROWSE-05

Review requested, Assigned to me, and Outbox MUST show a positive total in a right-aligned dark blue filled circular badge with centered white digits. Use a suitable accessible color, provisionally #173E70, and preserve a true circle, label clearance, and legibility for multi-digit totals and high-DPI scaling. A known zero draws neither a circle nor a number. Counts are totals for the current account and each view's stated repository/filter scope, not unread-notification counts or just the visible page length. Count distinct matching items for saved views. Use a trustworthy complete-query total when available; otherwise label the cached/known count and incomplete coverage, without presenting unknown or partial coverage as a confirmed empty result. Outbox counts all unresolved submitted actions, including paused, blocked, and outcome-unknown actions, and excludes drafts, download jobs, and confirmed/cancelled history. Refresh counts from shared state after ingestion, edits, scope changes, enqueue, cancellation, and confirmation without per-pane polling. Accessible names and count details must expose scope, freshness, and any coverage limitation.

Initial owner: Sprint 04. Initial verification: Sprint 09. Count component and shared query totals in 04; requested-review metadata in 06 and actual pending states in 07-09. Full AC-81 validates all supported writers, scope/coverage, circular styling and zero suppression in 09.

### WORKSPACE-01

The application MUST support multiple open issue and PR tabs and at least two simultaneously visible side-by-side panes in one window. Users must be able to mix issues and PRs from the same or different selected repositories. Allow additional side-by-side panes while usable minimum widths can be maintained. A new workspace starts with one pane; a split action adds another without closing the existing content.

Initial owner: Sprint 04. Initial verification: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### WORKSPACE-02

Each pane MUST maintain its own ordered item tabs and active tab. Show the item type, repository, item number, a readable title, and relevant draft or loading indicators through the tab label and accessible details. Provide tab overflow navigation and access to the full label when space is limited. Opening another item must not silently replace an existing tab. Provide explicit commands to open in a new tab, open beside the current pane, close a tab, and reopen the most recently closed tab.

Initial owner: Sprint 04. Initial verification: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### WORKSPACE-03

Ordinary item navigation reuses an existing matching tab in the active pane or adds a tab there. Open beside must add or focus a tab in an adjacent pane without replacing its other tabs. Provide an explicit duplicate-view action to show the same issue or PR in different panes, for example a PR conversation beside its Files subview. If several tabs in the target pane match, reuse its active matching tab or otherwise its most recently active match.

Initial owner: Sprint 04. Initial verification: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### WORKSPACE-04

Users MUST be able to resize panes, reorder tabs, and move tabs between panes. Provide visible or keyboard-accessible commands as well as any drag gestures. Show the focused pane clearly. A focus or maximize action MUST enlarge one pane temporarily and restore the previous layout. On a narrow window, preserve all tabs and drafts and offer a usable focused-pane layout instead of shrinking content beyond its minimum width.

Initial owner: Sprint 04. Initial verification: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### WORKSPACE-05

Persist view state independently for every tab instance: selected subview, selected file or commit, diff display mode, scroll anchors, expanded discussion or diff regions, and relevant navigation history. For a PR review tab also retain its context mode, selected verified linked issue, context/code proportions, collapsed/focused surface, and separate context scroll anchors. Switching tabs, resizing panes, or refreshing one resource MUST NOT reset another tab's position, selection, or focus. Anchor diff state to its comparison revision and handle unavailable anchors after a changed revision explicitly.

Initial owner: Sprint 04. Initial verification: Sprint 10. Independent state expands from conversations to per-PR context/code modes and linked-issue selections in 06, actual review drafts in 08, and commit comparisons in 10.

### WORKSPACE-06

Bind comments, issue edits, review submission, approvals, and merges to the originating tab's account, repository, item, and relevant revision. Changing focus while an operation is pending MUST NOT retarget it. Global navigation shortcuts apply to the focused pane; controls inside a pane apply to that pane's displayed item. Show the target repository and item in write confirmations where needed to avoid ambiguous actions.

Initial owner: Sprint 07. Initial verification: Sprint 09. Bind operation targets before writes; final comment/review/merge target checks in 09.

### WORKSPACE-07

Multiple views of the same resource MUST share cached domain data, operation outcomes, and persistent draft identities. Reopening the same compose or review draft must reuse its draft ID. Use one active editor per draft by default; other views show the current draft preview and a Focus editor action. Do not allow independent unsynchronized copies to overwrite the same draft or submit the same pending operation twice. Separate intentional drafts remain separate records.

Initial owner: Sprint 07. Initial verification: Sprint 09. Draft ownership primitives in 02 and local editors in 04; integrate shared pending issue/comment, review and merge operations in 07-09.

### WORKSPACE-08

Save and restore the workspace per account, including pane order and proportions, open tab order, active tabs, focused pane, per-tab view state, scroll position or anchors, navigation context, and draft references. Normal exit MUST retain this state. On restart, render cached visible content before waiting for any API response or renewed login; then start eligible background work through the shared scheduler. Load inactive tabs lazily and expose missing or inaccessible content without discarding its navigation target. Restoring a workspace MUST NOT submit unsubmitted drafts, create duplicate operations, or expose another account's private tabs. The independent outbox worker resumes already submitted eligible actions under OUTBOX-04 and the restored authentication/pause settings; view restoration is not a dispatch trigger.

Initial owner: Sprint 04. Initial verification: Sprint 12. Restore local workspace/drafts before networking in 04; integrate all views and writers as built. Final normal-exit, signed-out, expired-PAT and delayed-network restoration closes in 12. View restoration never submits drafts.

### WORKSPACE-09

Closing a tab or pane MUST preserve its saved drafts, cached content, and already submitted operation tracking. Persist editor changes before closing; if that fails, keep the editor recoverable and explain the failure. Closing a pane closes its tab views and focuses a remaining pane; closing the final tab leaves a usable empty pane. Discarding a draft is a separate explicit action. Reopening a closed tab restores its saved view state when available.

Initial owner: Sprint 04. Initial verification: Sprint 09. Persist closing-tab state in 04; prove pending issue/review/merge operations survive view closure.

### WORKSPACE-10

Share request scheduling and deduplicate in-flight fetches across views of the same resource and revision. Opening additional panes MUST NOT create independent polling loops or duplicate alerts for that resource. Give visible tabs priority within the existing schedule and rate-limit budget. Inactive tabs must not retain every large diff or continuously redraw; evict reconstructible rendering state while preserving navigation state and drafts. Closing one view must not cancel work still needed by another view or the shared write journal.

Initial owner: Sprint 05. Initial verification: Sprint 12. Shared scheduling in 05 and large-view retention in 06/10; complete integrated twenty-tab performance gate in 12.

### WORKSPACE-11

Repository lists, search results, in-app links, and notification activation MUST route through the workspace's open-item behavior. For notification or external URL activation, prefer a matching tab in the focused pane, otherwise the most recently active matching tab elsewhere, otherwise a new tab in the focused pane. Apply WORKSPACE-03 when choosing among matches within a pane. Navigate only the chosen tab to the requested comment or diff location. Other panes retain their positions and unsent input.

Initial owner: Sprint 04. Initial verification: Sprint 11. Open-item routing begins in 04; real notification activation completes the requirement in 11.

### WORKSPACE-12

Provide documented keyboard commands for opening a new tab or side pane, cycling tabs within a pane, moving focus between panes, closing and reopening a tab, and moving a tab between panes. Expose tab and pane names, selection, and focus to the published operating system's accessibility features, with Windows tested first. Test these commands from real text editors without intercepting ordinary editing shortcuts unexpectedly.

Initial owner: Sprint 04. Initial verification: Sprint 08. Workspace shortcuts and accessibility names in 04; test conflicts from real review text editors in 08.

### SEARCH-01

Provide fast local full-text search of downloaded titles, descriptions, and comments. Include available downloaded review text. Search results MUST identify the repository and item, highlight the match where practical, and navigate to the matching discussion or review location.

Initial owner: Sprint 04. Initial verification: Sprint 08. FTS schema in 02 and conversations in 04; formal review ingestion in 06 and confirmed submitted reviews in 08.

### SEARCH-02

Provide a separate Search GitHub action or clearly labeled remote result group to discover uncached items. Local results may appear immediately while a remote request runs. An empty local result means no matches in downloaded content, not no matches anywhere in the selected repositories.

Initial owner: Sprint 04. Initial verification: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### SEARCH-03

Handle unsupported remote query filters, pagination, API limits, and incomplete results explicitly. GitHub REST search can cap a query at 1,000 results and can return incomplete results. Do not use one search request as proof that every historical item was enumerated. Repository browsing must have its own paginated path. [S07] [S08]

Initial owner: Sprint 04. Initial verification: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### SEARCH-04

Offline search MUST remain usable and clearly indicate its downloaded-content scope. Opening an uncached remote result while offline must explain which content is missing and preserve the navigation target.

Initial owner: Sprint 04. Initial verification: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### CACHE-01

Opening downloaded content MUST render it immediately without waiting for GitHub. Refresh stale resources independently. The first visit to uncached content may require network access and must expose useful incremental progress.

Initial owner: Sprint 04. Initial verification: Sprint 06. Cached issues/conversations in 04; cached PR resources and diff context in 06.

### CACHE-02

Fetch resource groups independently: metadata, ordinary comments, timeline events, reviews, review threads, commits, changed-file lists, patches, source context, and media. Loading a PR's Conversation tab must not wait for every patch or image.

Initial owner: Sprint 04. Initial verification: Sprint 06. Complete independently fetched PR review/thread/commit/file/source families in 06.

### CACHE-03

Persist completeness separately from freshness. Resource state must distinguish missing, downloading, partial, complete, unavailable, and failed. Freshness records include last successful fetch, last validation, validators, and applicable revision. A complete but stale resource remains readable.

Initial owner: Sprint 02. Initial verification: Sprint 02. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### CACHE-04

Only show an empty-content message when the relevant resource is known to be complete and empty. A cached issue summary does not establish that its comments, timeline, or attachments have been downloaded.

Initial owner: Sprint 04. Initial verification: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### CACHE-05

Commit downloaded pages and their progress atomically. Retry and resume reads without duplicating records. Persist enough progress to resume interrupted pagination. If a cursor or page boundary becomes unreliable, safely re-enumerate the affected range using stable IDs and deduplication. [S07]

Initial owner: Sprint 04. Initial verification: Sprint 05. Atomic progress primitives in 02, resumable UI pagination in 04, scheduled read resumption in 05.

### CACHE-06

Retain fetched text, review data, commit metadata, and diffs across restarts. Do not impose an undocumented age cutoff. Bound memory and render state independently of disk retention. Expose per-repository usage and explicit cleanup controls.

Initial owner: Sprint 04. Initial verification: Sprint 12. Disk retention/cleanup in 04; integrate all content types and verify bounded memory on full workload in 12.

### CACHE-07

Existing-issue edit drafts, comment drafts, review drafts, and submitted outbox payloads are user data. Cache clearing, repair, failed downloads, migrations, reconciliation, and closing tabs or panes MUST preserve them unless the user explicitly deletes the drafts or cancels the actions. Drafts and outbox entries exist independently of any viewing tab. Preserve source anchors, edit base values, dependency mappings, and the minimal referenced content needed by unresolved actions; apply OUTBOX-10 to cleanup and recovery.

Initial owner: Sprint 02. Initial verification: Sprint 09. Draft/outbox-safe storage in 02; recheck cleanup and recovery for every draft and queued-action type through 09; repeat final migration matrix in 12.

### CACHE-08

Namespace records by host and account access context, with stable repository and resource identities. Issues and PRs share one repository-local number namespace. Map their API representations to a common local item while preserving distinct issue and pull-request API object IDs. Follow DATA-05 through DATA-09 for keys and ingestion. Never expose another account's private cached content after switching identities. [S27] [S28]

Initial owner: Sprint 02. Initial verification: Sprint 04. Storage scoping in 02, real identity switching in 03, cached browsing isolation in 04.

### CACHE-09

Associate comparison data with immutable commit identities and comparison options. Associate mutable branch labels, status checks, and review state with their relevant revisions and timestamps. A branch name alone is not an adequate diff cache key.

Initial owner: Sprint 02. Initial verification: Sprint 10. Typed immutable comparison identity in 02; actual PR/review and repository comparison integration later.

### CACHE-10

A 404, authentication failure, or temporary API error MUST NOT trigger bulk cache deletion. Record the reason when known, stop inappropriate retries, and reconcile after access is restored. Distinguish an unknown resource from a confirmed deletion.

Initial owner: Sprint 02. Initial verification: Sprint 05. Storage preservation in 02; authorization/provider handling in 03; live sync reconciliation in 05.

### CACHE-11

Existing images and accessible attachments SHOULD be fetched on demand, stored with cache metadata, and reused. Source code outside a patch is a separate resource. Show explicit placeholders for missing, inaccessible, binary, or unsupported content.

Initial owner: Sprint 04. Initial verification: Sprint 06. Media and attachment placeholders in 04; source context outside supplied patches in 06.

### CACHE-12

All history means all currently accessible historical items and their available data. It does not promise reconstruction of deleted comments or every overwritten PR revision. Optional future archival capture must be a separately defined feature.

Initial owner: Sprint 04. Initial verification: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### CACHE-13

Normal window close and application exit MUST retain downloaded content and resource coverage, all draft types, saved tabs/layout/view positions, local preferences, offline pins, pending actions and receipts, synchronization checkpoints/validators, and notification baselines/delivery history on the PC. Persist settings and user changes as they occur and safely flush remaining local state on orderly exit; if a user-data write fails, keep it recoverable and report the failure. Retain the protected credential according to AUTH-07. Restart MUST reuse these records and render the saved local workspace before networking completes. It must not clear the cache, reset baselines, or redownload all history merely because the process was restarted. Sign-out preserves local content under AUTH-11. Explicit cache cleanup remains a separate action and continues to protect drafts and unresolved work under CACHE-07 and OUTBOX-10.

Initial owner: Sprint 02. Initial verification: Sprint 12. Complete persistence schema in 02, PAT lifecycle in 03, local-first restoration in 04, sync in 05/06, all writer/history/notification types through 11, and the complete quit/sign-out/reopen matrix in 12.

### OFFLINE-01

Once an account and content have been established, the application MUST start and remain usable without a network check or renewed login blocking local access. Users can browse/search downloaded issues, PRs, discussions, reviews, commits, diffs, graph history, and the activity feed; use tabs and panes; edit local settings and drafts; and inspect or cancel pending work. State the cached coverage and last fetch honestly. Unseen remote content and new server events cannot be available until fetched. First-time authentication and initial acquisition of private content still require access; do not require a full repository download.

Initial owner: Sprint 04. Initial verification: Sprint 12. Offline startup/workspace in 04; each new view and action integrates as built; complete all-feature cold-start offline matrix in 12.

### OFFLINE-02

Offline mode MUST allow preparation and explicit submission of every implemented remote mutation: existing-issue editing/state/metadata, ordinary comments and edits, review comments/replies/submission/approval/request changes, permitted thread actions, and merges. Keep their controls available when cached target data and capability information are sufficient, and queue intent locally. Unknown permissions or missing required target/source details are explicit prerequisites; never invent a target, bypass a known denial, or silently choose a newer revision. Local settings, drafts, viewed-file markers, and app-feed read state apply locally immediately.

Initial owner: Sprint 07. Initial verification: Sprint 09. Local preparation begins in 04; all supported mutation intents become offline-submittable in 07-09 with honest prerequisites.

### OFFLINE-03

Provide Download for offline use for a selected item, with the resource scope and revision shown before downloading. Include its accessible discussion and, for a selected PR revision, review/thread data, commit/file metadata and available patches; let users choose optional media or additional source context. Track progress and per-resource completion, expose caps or unavailable content, and resume interrupted downloads. An offline request to fetch missing data becomes a visible, cancellable pending download that starts when eligible. Download jobs are distinct from remote mutation actions. Never present partially fetched data as fully available or start a mandatory full-repository archive.

Initial owner: Sprint 04. Initial verification: Sprint 06. Issue downloads and coverage in 04, scheduled resumption in 05, full PR resource scope and AC-70 in 06.

### OFFLINE-04

Show submitted local work immediately as clearly pending in all matching views: for example a pending comment, a review action, or an edit to an existing issue awaiting confirmation. Keep pending projections separate from confirmed cached server records; a refresh cannot erase them, and cancellation removes only the pending projection while preserving recoverable user text. A pending merge or approval is not a confirmed merge or approval. Offline pins and minimal draft/outbox dependencies must survive ordinary cache cleanup until explicitly released or the action is resolved.

Initial owner: Sprint 04. Initial verification: Sprint 09. Storage protection in 02, pending-view foundation in 04, actual issue/comment/review/merge projections and cancellation integration in 07-09.

### SYNC-01

Users MUST configure workdays, work windows, timezone, work-hours interval, off-hours interval, and pause behavior. Support daylight-saving changes through timezone-aware scheduling. Default to the operating system timezone through the platform adapter and allow an explicit override.

Initial owner: Sprint 05. Initial verification: Sprint 05. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### SYNC-02

Use these provisional defaults: Monday through Friday, 09:00 to 17:00; work-hours checks every 120 seconds; off-hours checks every 30 minutes; stale items in the selected tab of each visible pane eligible for a 60-second check. Expose these values in settings. Treat them as requested intervals subject to API constraints and actual workload. Multiple views of one item share its refresh schedule.

Initial owner: Sprint 05. Initial verification: Sprint 05. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### SYNC-03

Give priority to explicit user actions and their eligible submitted outbox work, then the selected item in each visible pane, enabled activity monitoring, recently used or pinned content, and slower reconciliation. Apply a shared budget and fair scheduling so a busy pane or outbox cannot starve other work. Fetch older history on demand. Do not launch a mandatory full historical backfill. Keep dependency order and mutation pacing even when draining the outbox.

Initial owner: Sprint 05. Initial verification: Sprint 09. Read scheduling and lifecycle in 05; prove eligible outbox dispatch outside read schedules and on restart across issue/review/merge actions in 07-09.

### SYNC-04

A user-triggered open or refresh may run outside work hours. Explicitly submitted outbox actions also resume outside work hours when eligible; quiet hours control notification delivery and read-refresh schedules control background reads, not authorization to send queued actions. Provide a separately labeled Pause queued actions control, and distinguish it from Pause background refresh and Work offline. Persist these settings and restore them before any startup network work. Work offline blocks all networking until disabled; Pause queued actions blocks new mutation dispatch, and Pause background refresh blocks scheduled catch-up reads. After sleep or reconnection, coalesce overdue read jobs into a bounded catch-up pass and resume eligible actions without replaying every missed timer tick or sending a mutation burst. Required outbox preflight/reconciliation reads remain governed by the action and connectivity controls, not the background-read schedule.

Initial owner: Sprint 05. Initial verification: Sprint 09. Read scheduling and lifecycle in 05; prove eligible outbox dispatch outside read schedules and on restart across issue/review/merge actions in 07-09.

### SYNC-05

Centralize pagination, cancellation, request deduplication, retry limits, exponential backoff, and server budget handling. Honor relevant `Retry-After`, rate-limit, and polling headers. Authenticated conditional requests that return 304 can preserve the primary REST quota, but do not make polling unlimited. [S11] [S12]

Initial owner: Sprint 05. Initial verification: Sprint 06. General read scheduler and item/comment observers in 05; PR-family pagination and review/check reconciliation in 06.

### SYNC-06

Maintain independent synchronization progress for resource families. Use repository-level updated-comment feeds where available, but do not assume the parent issue or PR timestamp is a complete change feed for every child object. Use overlapping update windows and periodic reconciliation where required. [S06] [S13]

Initial owner: Sprint 05. Initial verification: Sprint 06. General read scheduler and item/comment observers in 05; PR-family pagination and review/check reconciliation in 06.

### SYNC-07

Include all states when checking for relevant updates. A new comment on an old closed issue or merged PR can be relevant. Do not permanently freeze previously downloaded historical items merely because they are closed.

Initial owner: Sprint 05. Initial verification: Sprint 06. General read scheduler and item/comment observers in 05; PR-family pagination and review/check reconciliation in 06.

### SYNC-08

Expose last successful synchronization, pending work, throttling, authentication problems, and resumable failures. Background failures must not replace usable cached screens with blocking error pages.

Initial owner: Sprint 05. Initial verification: Sprint 05. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### SYNC-09

Run synchronization, connectivity checks, and the outbox worker in the application or optional tray process. A fully exited application or sleeping machine cannot perform local polling or send actions. Normal exit preserves local state and the protected PAT; it does not sign the user out. On launch or wake, restore the local workspace and persisted control settings first, then resume eligible actions and bounded incremental read catch-up under SYNC-11 when connectivity and authorization permit. No hosted service is required for the initial release.

Initial owner: Sprint 05. Initial verification: Sprint 12. Lifecycle scheduling begins in 05, all writers integrate by 09, graph/feed families by 11, and retained-credential plus local-first incremental restart acceptance closes in 12.

### SYNC-10

The highest downloaded issue or PR number MUST NOT be the sole synchronization cursor. Updates to old items and their comments, reviews, or state must still be discovered. Use supported update timestamps or cursors, overlapping windows, pagination, and resource-family reconciliation. Enumerate through supported list APIs instead of requesting every integer in a number range. Missing numbers do not establish deletion or complete cache coverage. [S28]

Initial owner: Sprint 05. Initial verification: Sprint 05. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### SYNC-11

Resume from persisted per-account, per-repository, per-resource-family synchronization state on restart, reconnection, and same-user PAT renewal. Retain the last successful checkpoint, overlapping update window or supported cursor, pagination progress, conditional-request validators, resource completeness/freshness, and notification baselines. Advance content and its successful progress transactionally after each completed unit; failures or partial pages must not skip unseen changes. Prioritize visible resources and run bounded, rate-aware incremental catch-up, using supported changed-since/update feeds, conditional reads, and targeted reconciliation. Do not blindly redownload the entire cache, re-enumerate every historical discussion, reset notification baselines, or use the largest issue/PR number as the only cursor. If a cursor expires or an endpoint has no reliable delta interface, re-enumerate the necessary resource/range with stable-ID deduplication and preserved coverage while keeping other completed families intact. An update-feed page one, overlap, unchanged revalidation, or targeted repair may be necessary; no universal exact-delta or zero-unchanged-bytes guarantee is made. Preserve old-item update discovery and honor restored sign-out, offline, and pause state. [S06] [S07] [S11] [S13]

Initial owner: Sprint 05. Initial verification: Sprint 12. Persist checkpoints and stable account identity in 02/03; issue/comment restart catch-up in 05 and PR-family AC-75/76 in 06; all graph/notification families and full lifecycle request traces complete in 12.

### CONNECT-01

Show a persistent, prominent green/red connection indicator in the application chrome, visible across panes, with an accessible icon and text label; color alone is insufficient. Green means recently verified GitHub API reachability. Red means Offline or Cannot reach GitHub after a current transport failure or an explicit Work offline choice. Use a neutral Checking or Status stale state before an initial result and when evidence is too old, rather than asserting a connection indefinitely. Show last checked time, last successful contact, reason, and Check now in its details, plus a pending-action count linking to the Outbox.

Initial owner: Sprint 04. Initial verification: Sprint 05. Provider status contract in 03, persistent accessible chrome in 04, real shared observer and reachability classification in 05.

### CONNECT-02

Derive reachability from verified HTTPS responses from the configured GitHub API and current transport errors, not from cached data or a network adapter icon alone. Successful responses, including 304, establish contact. A genuine API 401, authorization 403/404, 429, or 5xx also demonstrates a reachable API endpoint, while separate status explains Sign-in required, SSO approval, Throttled, or Service error and blocks affected writes as appropriate. DNS, connection, TLS, and timeout failures indicate inability to reach the service securely; they do not prove that every Internet destination is down. Request cancellation caused by tab navigation is not an offline signal. Suppress older/out-of-order observations so they cannot overwrite newer connectivity evidence; scope authorization errors separately by account/repository.

Initial owner: Sprint 03. Initial verification: Sprint 05. Typed provider evidence in 03; UI consumes it in 04 and full stale/out-of-order/transport/error cases close in 05.

### CONNECT-03

Maintain one shared connectivity observer. Reuse ordinary API traffic; issue a lightweight non-mutating probe at startup, wake, network-change hints, or Check now when permitted. Use a provisional 120-second freshness window, then show stale evidence instead of increasing idle background polling beyond the configured refresh schedule. While submitted work is waiting on connectivity and the app/tray is active, probe automatically with bounded backoff and jitter, provisionally 5, 15, 30, 60, then at most every 120 seconds. Respect shared request budgets and longer server Retry-After/rate-limit delays. A new reachable result wakes the dispatcher once; many panes or queued items must not each probe or drain independently. Work offline disables probes and sends until explicitly turned off.

Initial owner: Sprint 05. Initial verification: Sprint 07. Probe/backoff and eligibility signal tests in 05; integrate actual waiting submissions and automatic dispatcher wake-up in 07.

### ISSUE-01

Display title, body, author, timestamps, state, labels, assignees, milestone, available reactions, linked items, and supported timeline events. Display the full accessible paginated conversation, with explicit partial-download states.

Initial owner: Sprint 04. Initial verification: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### ISSUE-02

Render Markdown natively, including headings, emphasis, links, lists, task lists, tables, quotations, code blocks, and images supported by the implementation. Text must be selectable and copyable. Use a readable source or unsupported-content fallback for features requiring additional rendering, such as complex embedded diagrams or HTML. Do not add a webview to improve parity.

Initial owner: Sprint 04. Initial verification: Sprint 04. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### ISSUE-03

Edit existing issue titles, descriptions, labels, assignees, milestones, and open/closed state where authorized. Show available choices from the repository instead of inventing labels or identities. Bind edits to an already established issue identity and preserve its base values and draft; do not create a new issue as a fallback for a failed edit.

Initial owner: Sprint 07. Initial verification: Sprint 07. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### ISSUE-04

Add comments and edit the user's editable comments. Provide a Markdown editor, native preview, and persistent drafts. On explicit submission, durably enqueue the action before attempting delivery, whether online or offline, and show the OUTBOX-02 lifecycle. Preserve text after errors or cancellation, and publish confirmed results to all views without duplicating a locally pending comment.

Initial owner: Sprint 07. Initial verification: Sprint 07. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### ISSUE-05

Preserve the base values used to begin an edit. Immediately before dispatch, including delayed offline replay, refresh the edited fields and detect conflicting changes. Keep a conflict in the outbox as Needs attention while preserving local intent. Do not overwrite another author's changes silently. Do not assume the endpoint provides an atomic conditional-write guarantee unless it has been verified.

Initial owner: Sprint 07. Initial verification: Sprint 07. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### ISSUE-06

New issue creation is excluded from the current release. Do not expose a New issue button, creation form/template workflow, shortcut, command, issue-creation draft type, create-issue outbox operation, or provider mutation flow. Existing-issue reading, comments, and authorized editing remain available. PR creation is also outside scope; this restriction does not remove comment creation, review submission, or their durable dependency identities.

Initial owner: Sprint 07. Initial verification: Sprint 07. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### PR-01

Provide Conversation, Commits, Files, and Checks sections. Display author, source and target branches, draft status, open/closed/merged state, requested reviewers, review results, relevant timestamps, and linked issues.

Initial owner: Sprint 06. Initial verification: Sprint 06. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### PR-02

Retrieve ordinary conversation comments, formal reviews, and inline review threads as separate resources. Preserve thread replies and available resolved or outdated status. GitHub exposes ordinary PR comments through issue-comment APIs and review comments separately. [S06] [S09] [S13] [S15]

Initial owner: Sprint 06. Initial verification: Sprint 06. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### PR-03

The commit list must show commit identity, message, author, and available timestamps. The file list must show paths, status, additions, deletions, and renames where available. Identify missing or incomplete information.

Initial owner: Sprint 06. Initial verification: Sprint 06. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### PR-04

Show checks and commit statuses associated with the relevant PR head. A cached result must show its freshness. Missing permissions or unavailable checks must appear as unknown or inaccessible, never as success.

Initial owner: Sprint 06. Initial verification: Sprint 06. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### PR-05

The default PR review arrangement MUST show the native file list/diff prominently beside exactly one smaller context pane bound to the same active PR. At a usable window width, start with approximately 33% of the review content width for context and 67% for code, within a 30-35% / 65-70% range, excluding global navigation chrome. Context may be resized or collapsed; restore its last proportion. Provide View PR body and View issue controls in that one pane, with PR body as the initial mode for a newly opened review tab. They replace the context content rather than adding a simultaneous third pane. Preserve Conversation, Commits, Files, and Checks navigation and multiple PR tabs. At narrow widths, show a usable focused surface with an explicit way to switch context/code and restore the wider arrangement without losing either surface's state.

Initial owner: Sprint 06. Initial verification: Sprint 08. Native layout feasibility in 01 and workspace primitives in 04; bound context/code PR reader in 06; full AC-78 with actual inline drafts, focus, narrow layout and offline restore closes in 08.

### PR-06

Derive linked-issue candidates from closing-style references in the current PR body. Support case-insensitive close/closes/closed, fix/fixes/fixed, and resolve/resolves/resolved with optional colon and a same-repository #number, qualified owner/repository#number, or GitHub.com issue URL. Parse native Markdown safely; ignore fenced/inline code examples and do not treat arbitrary numbers as references. Resolve local #number against the PR's repository, normalize and deduplicate targets, and verify repository identity, access, and ordinary issue type before displaying a candidate as an issue; a shared-number target can be a PR. Multiple verified targets require a picker, with a deterministic default from body order and a retained explicit selection. Cross-repository targets use the appropriate credential scope and load on demand without silently enabling repository-wide monitoring. Preserve reference provenance and body revision under DATA-10. A body reference is reading context, not proof that GitHub linked, will close, or already closed the issue: default-branch rules, permissions, and repository auto-close settings can differ. [S27] [S28] [S29] [S30]

Initial owner: Sprint 06. Initial verification: Sprint 06. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### PR-07

View issue MUST display the selected verified issue's native body and available metadata/conversation in the context pane, with its repository, number, and coverage visible. View PR body restores that PR's body and context scroll state. Switching modes or linked issues must retain the code comparison, selected file, scroll/selection, review draft identity/anchors, and intended mutation target; background loads must not steal code-editor focus. Deliberate keyboard/pointer navigation may move focus normally. Use account, PR, body revision, selected issue, tab instance, and selection generation to discard stale context responses. Switching PR tabs must never show the previous PR's issue while loading. Distinguish no reference, verification pending, target is a PR, not downloaded/offline, inaccessible, failed, and removed or changed reference states; none is a confirmed empty issue. Cached PR bodies, linkage evidence, verified issues, and per-resource coverage remain usable offline with last-validation information. Preserve local drafts if body refresh removes a reference, and never silently attach them to a replacement issue.

Initial owner: Sprint 06. Initial verification: Sprint 08. Reference/cache/error and stale-response reading behavior closes AC-80 in 06; actual review-draft/focus stability and AC-78 close in 08.

### REVIEW-01

Users MUST create inline comments, replies, and a draft review, then submit a comment-only review, approval, or request for changes. Support resolving and reopening review threads where the API and permissions allow it. [S09] [S15]

Initial owner: Sprint 08. Initial verification: Sprint 08. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### REVIEW-02

Persist each inline draft with repository, PR, file path, original or modified side, source line range, reviewed commit identity, and available context. Store source coordinates rather than visible row numbers.

Initial owner: Sprint 08. Initial verification: Sprint 08. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### REVIEW-03

New commits and force-pushes MUST NOT silently move a draft or queued review onto different code. Preserve the original context and intended reviewed commit, indicate that the PR changed, and provide an explicit refresh or re-anchor workflow. Revalidate queued reviews, approvals, requests for changes, and thread actions before dispatch. A changed reviewed revision or invalid anchor requires attention instead of automatically approving different code. GitHub's review API accepts a commit identity; use the intended reviewed revision. [S09]

Initial owner: Sprint 08. Initial verification: Sprint 08. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### REVIEW-04

Preserve unfinished reviews and submitted review actions across restart and connection loss. Offline users MUST be able to read cached review data, edit drafts, submit them to the local outbox, inspect pending actions, and cancel eligible entries. Actual GitHub submission and thread mutations execute automatically after connectivity returns and current permissions, source anchors, and revision checks pass. An unsent saved draft remains a draft.

Initial owner: Sprint 08. Initial verification: Sprint 08. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### DIFF-01

Provide unified and split views, a changed-file sidebar, syntax highlighting, additions and deletions, selection and copying, file collapsing, next/previous change navigation, and keyboard shortcuts. Reuse GPUI Kit components where compatible. [S02]

Initial owner: Sprint 06. Initial verification: Sprint 06. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### DIFF-02

Provide inline thread display, comment-entry controls, unresolved-thread navigation, and persistent per-revision viewed-file markers. A file changed by a subsequent PR revision must not remain silently marked fully reviewed.

Initial owner: Sprint 08. Initial verification: Sprint 08. Read threads and establish viewed-file state in 06; inline entry, navigation and revision invalidation completed in 08.

### DIFF-03

Distinguish PR comparison, individual-commit comparison, and arbitrary two-commit comparison. The default PR view must match the intended GitHub comparison. GitHub PRs use the merge base for a three-dot comparison; endpoint comparisons can differ. Label the comparison and use the correct source identities in the cache. [S16]

Initial owner: Sprint 10. Initial verification: Sprint 10. Typed comparison model in 02; correct PR comparison in 06; individual/two-commit comparisons complete in 10.

### DIFF-04

Parse large patches and prepare highlighting outside the UI thread. Virtualize long file lists and diff rows. Keep scroll and selection stable when annotations or background data change. Ignore stale background results for an earlier comparison.

Initial owner: Sprint 06. Initial verification: Sprint 08. Virtualized async native diff in 06; prove annotation stability with actual review editing in 08.

### DIFF-05

Support an ignore-whitespace viewing option when backed by an accurate comparison. Preserve original source coordinates for review submission; do not derive comment positions from a visually filtered row index.

Initial owner: Sprint 08. Initial verification: Sprint 08. Source coordinates established in 06; accurate filtering and review-submission mapping verified in 08.

### DIFF-06

Distinguish a patch from complete source content. Expanding outside supplied context may require another fetch. Cache fetched file versions separately and preserve the current review view while additional context loads.

Initial owner: Sprint 06. Initial verification: Sprint 06. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### DIFF-07

Detect API caps, missing pages, truncated patches, binary changes, unavailable objects, and unsupported formats. The PR-files endpoint has a documented 3,000-file limit, and the PR-commits endpoint a 250-commit limit. Provide an alternate retrieval path where practical and explicitly label any remaining partial result. Never label capped output complete. [S10]

Initial owner: Sprint 06. Initial verification: Sprint 06. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### GRAPH-01

Include a native history view with branch labels, commits, parent connections, and visible merge relationships. The initial scope is remote GitHub branches. Branches are movable references to commits, and merge commits can have multiple parents; draw that ancestry rather than inventing permanent parent-child relationships between branch names. [S17]

Initial owner: Sprint 10. Initial verification: Sprint 10. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### GRAPH-02

Show a bounded recent history for the default branch and selected branches first. Load older commits on scroll or explicit request, and cache them. Display messages, authors, dates, abbreviated commit identities, and relevant branch or tag labels.

Initial owner: Sprint 10. Initial verification: Sprint 10. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### GRAPH-03

Selecting a commit opens its changed files and diff. Selecting two commits opens a labeled comparison. For merge commits, identify the parent used for a single-commit comparison and permit selection of another available parent.

Initial owner: Sprint 10. Initial verification: Sprint 10. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### GRAPH-04

Lay out parent relationships consistently, preserve visible lane and scroll stability while expanding history, and mark unloaded parents as continuation boundaries. A missing fetched parent is not proof of a root commit.

Initial owner: Sprint 10. Initial verification: Sprint 10. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### GRAPH-05

Update moved or deleted branch labels independently from cached immutable commit data. Handle force-pushes without corrupting the graph or discarding review drafts.

Initial owner: Sprint 10. Initial verification: Sprint 10. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### GRAPH-06

Start with GitHub references and commit APIs. These expose the identities and relationships needed for a focused graph. A full clone must not be required. [S18] [S26]

Initial owner: Sprint 10. Initial verification: Sprint 10. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### GRAPH-07

A bare Git object cache is an optional future optimization for deep history or complete comparisons. Bare repositories avoid a checkout but do not inherently minimize transfer; partial clone filtering can defer file contents when supported. Any shallow history must expose its incomplete boundary. [S19]

Initial owner: Sprint 10. Initial verification: Sprint 10. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-01

Provide native desktop notifications and a persistent in-app activity feed, with Windows integration first and macOS/Linux integration in their release phases under PORT-05. The native integration must support opening the correct item when clicked, single-instance behavior, and useful notification identity or grouping. Validate the chosen Rust and native API integration early, with Windows first and actual target evidence before each later release. [S24]

Initial owner: Sprint 11. Initial verification: Sprint 11. Native notification/tray/activation feasibility in 01; persistent feed, rules and production activation in 11.

### NOTIFY-02

Activity monitoring MUST run independently of demand-loaded full content. Enabled events must be detectable on selected repositories even for issues and PRs the user has never opened. Monitoring must not require a complete historical download.

Initial owner: Sprint 05. Initial verification: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-03

Provide independent controls for new commits or observed branch updates, ordinary comments, inline comments, review requests, approvals, requests for changes, comment-only reviews, PR opening or readiness, merges, issue assignments, issue state changes, and direct mentions. Check-status notifications are included where the credential and API support them.

Initial owner: Sprint 11. Initial verification: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-04

Commit monitoring must support the default branch, chosen branches, branch patterns, or PRs the user follows. Identify observed new commits accurately. Do not infer who pushed a commit from its author if push-actor information is unavailable.

Initial owner: Sprint 10. Initial verification: Sprint 11. Branch-scope selection, baselines and accurate observations in 10; grouped delivery verified in 11.

### NOTIFY-05

Support per-user global defaults and per-repository overrides. Relevant-item filters include all monitored activity, items involving the user, and explicitly followed or pinned items. Store preferences locally.

Initial owner: Sprint 11. Initial verification: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-06

Each enabled category must support pop-up plus activity feed, activity feed only, or off. Sound is independent and off by default. Provide repository mute, thread snooze, quiet hours, and suppression of the user's own actions.

Initial owner: Sprint 11. Initial verification: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-07

Suppression rules match stable actor identity, event category, and optional repository scope. Use bot or app metadata where reliable and allow explicit account rules. Do not promise perfect automatic recognition of every automated review.

Initial owner: Sprint 11. Initial verification: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-08

A rule for automated review activity must cover the selected actor's formal reviews and inline review comments. Ordinary PR comments from that account are separately configurable. Human replies, unrelated events, and all activity on a bot-authored PR must not be suppressed merely because the PR author is a bot.

Initial owner: Sprint 11. Initial verification: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-09

Muting notifications MUST NOT remove or hide the underlying review or comment in its issue or PR view. Provide a convenient Mute review activity from this account action and a settings preview explaining the resulting rule.

Initial owner: Sprint 11. Initial verification: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-10

Resolve notification categories independently. An Off setting disables that category's delivery path, not every other category matched by the same activity. Comments Off with Direct mentions Pop-up therefore permits one mention notification. Resolve per-repository overrides before global category defaults, then combine eligible paths into one notification using the strongest permitted delivery mode.

Apply global disablement and repository mute first. Apply own-action suppression and actor rules to the underlying activity across its eligible category paths. An explicitly enabled mention exception can restore only an enabled mention path suppressed by an actor rule. It cannot override global disablement, repository mute, own-action suppression, an Off mention category, snooze, or quiet hours. Apply snooze and quiet-hour delivery restrictions before grouping and deduplication. Keep enough decision information to explain the result.

Initial owner: Sprint 11. Initial verification: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-11

Quiet hours and snooze suppress pop-ups while retaining otherwise eligible activity-feed entries. Synchronization continues according to its independent schedule. An explicit category Off setting disables that category's feed and pop-up path; a separately enabled matching category may still qualify under NOTIFY-10. Global disablement and repository mute suppress all notification paths in their scope. Underlying repository content remains available normally.

Initial owner: Sprint 11. Initial verification: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-12

Group related bursts by repository and issue, PR, or branch. A review with many inline comments should normally produce one alert. An example is 3 new commits on feature/search, with one action opening the corresponding comparison.

Initial owner: Sprint 11. Initial verification: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-13

Establish a baseline for each repository and event category when monitoring begins. Downloading old content for the first time MUST NOT itself create a new-activity notification. A genuinely new comment on an old unopened issue may still qualify based on its trustworthy event identity and timestamp.

Initial owner: Sprint 05. Initial verification: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-14

Where no trustworthy earlier state or event time exists, establish state without inventing a transition. Deduplicate events across polling endpoints and multiple notification categories. A comment that mentions the user should not produce duplicate comment and mention banners by default.

Initial owner: Sprint 05. Initial verification: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-15

Persist event keys, eligibility decisions, grouping keys, and delivery state. Reuse stable native notification identities where supported and report unsupported replacement/retraction capabilities truthfully. Prevent ordinary duplicate delivery across retries and restart; do not claim an unconditional exactly-once guarantee across application crashes and OS delivery.

Initial owner: Sprint 05. Initial verification: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-16

Distinguish comment creation from editing. Record the user's successful writes so a subsequent poll does not echo them as unsolicited new activity when own-action suppression is enabled.

Initial owner: Sprint 07. Initial verification: Sprint 11. Creation/edit distinction in 05; own successful issue/comment/review/merge writes recorded in 07-09; suppression verified in 11.

### NOTIFY-17

After a long offline period, group eligible catch-up events into a readable summary. Do not replay a storm of old banners. Use current notification preferences when delivering queued activity.

Initial owner: Sprint 11. Initial verification: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-18

Clicking a notification must navigate to the correct repository, item, and available thread or diff using the tab routing in WORKSPACE-11. Show cached summary content immediately and fetch missing details on demand while preserving unrelated panes. Support useful actions such as Open, Snooze, and Mark read.

Initial owner: Sprint 11. Initial verification: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-19

Respect the published operating system's notification preferences. Local monitoring runs only while the application or tray process is active. Polling detects observed changes but cannot guarantee every transient push or state transition between checks. GitHub's general Events API has retention limits and documented latency; do not use it as the sole reliable event source. [S25]

Initial owner: Sprint 11. Initial verification: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-20

A hosted webhook relay and stronger always-on event delivery are later capabilities. Keep the event-ingestion interface extensible, but do not add required infrastructure to the first release.

Initial owner: Sprint 11. Initial verification: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### WRITE-01

Keep unsubmitted drafts separate from submitted operations. Model submitted operations using queued, validating, sending, outcome unknown, waiting for remote completion, blocked/needs attention, confirmed, and cancelled states as defined in OUTBOX-02. Persist intent before delivery; show pending local changes distinctly until GitHub confirms them. Share operation state and server results across all views, independently of the originating tab's lifetime.

Initial owner: Sprint 07. Initial verification: Sprint 09. Journal primitives in 02; shared write engine in 07; integrate reviews and merges before final closure.

### WRITE-02

Save a draft snapshot and durably enqueue the user's explicit submission before attempting a request. Preserve them after authorization errors, validation failures, offline transitions, server errors, and closing their tab. Enforce shared draft ownership and one operation identity across panes. Merely saving, editing, or reopening an unsubmitted draft MUST NOT enqueue it. Submitted eligible actions MUST resume automatically after restart or reconnection without asking the user to submit them again, subject to the outbox rules.

Initial owner: Sprint 07. Initial verification: Sprint 09. Local drafts/outbox persistence first; automatic replay, cancellation, preflight and uncertain-outcome handling expand from issues/comments in 07 to reviews/merges in 08-09.

### WRITE-03

A timed-out issue, comment, review, or merge operation may have succeeded remotely. Keep the submitted operation in the outbox and automatically reconcile reliable identities or server state before considering another attempt. Retry automatically only when the action is known not to have been applied or the provider has a verified safe idempotency mechanism. If the result remains ambiguous, show Outcome unknown and allow inspection or stopping further attempts without claiming remote undo. Do not blindly replay a non-idempotent request; matching text alone is not proof of success. A deliberate retry of an unresolved action must explain the possible duplicate and cannot be triggered by reconnection alone.

Initial owner: Sprint 07. Initial verification: Sprint 09. Local drafts/outbox persistence first; automatic replay, cancellation, preflight and uncertain-outcome handling expand from issues/comments in 07 to reviews/merges in 08-09.

### WRITE-04

Preserve concurrent remote edits, submitted intent, and local input. Revalidate affected fields, permissions, and revisions immediately before dispatch, including automatic replay; explain conflicts and keep the action visible as Needs attention. Retain the draft for correction. Document unavoidable races where GitHub supplies no atomic conditional operation. A temporary connectivity interruption alone must not require resubmission of an already queued action.

Initial owner: Sprint 07. Initial verification: Sprint 09. Local drafts/outbox persistence first; automatic replay, cancellation, preflight and uncertain-outcome handling expand from issues/comments in 07 to reviews/merges in 08-09.

### MERGE-01

Users MUST be able to request an eligible merge while offline using a locally cached target PR, intended head commit, and chosen supported method. Durably queue that intent. Actual merge execution requires connectivity and fresh checks of PR state, permissions, allowed methods, and server requirements. A cached green check or connection indicator is not authoritative. Send the originally intended expected head commit identity; never silently substitute the current head. [S10]

Initial owner: Sprint 09. Initial verification: Sprint 09. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### MERGE-02

Offer merge, squash, or rebase only when supported and permitted. Respect required checks, reviews, and branch rules. Never silently use administrative bypasses or change repository policy.

Initial owner: Sprint 09. Initial verification: Sprint 09. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### MERGE-03

The first release supports ordinary permitted direct merges and explicitly excludes GitHub merge-queue enqueue, management, and withdrawal flows. GitHub auto-merge is optional future scope. Distinguish a merge waiting in the local Outbox, a request that may have reached or been accepted by GitHub, and a confirmed merged PR; reconcile uncertain or accepted-but-unconfirmed results before claiming success or retrying. If fresh preflight or a server rejection reveals that the repository requires a GitHub merge queue, retain the submitted intent as Needs attention with a clear unsupported-capability reason and a link to the PR when useful. Do not enqueue it remotely, enable auto-merge, bypass a rule, or change policy. Local cancellation stops eligible local attempts and never implies that a remote merge was undone.

Initial owner: Sprint 09. Initial verification: Sprint 09. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### MERGE-04

If the intended head changed, permissions or policy no longer permit the action, or current validation conflicts with queued intent, retain the action as Needs attention, preserve the review context, and require a fresh user decision after updating the view. Do not silently retry against a different head. A safe retry of the same intent after temporary transport failure or throttling is automatic under OUTBOX-04/05; an uncertain outcome is reconciled before another attempt.

Initial owner: Sprint 09. Initial verification: Sprint 09. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### OUTBOX-01

Route every explicit remote mutation through a durable SQLite outbox before sending, online as well as offline. Commit a stable operation ID, immutable submitted payload/version, original account and target identities, base values or commit revision, dependency references, draft snapshot, and locally pending projection atomically. Acknowledging Queued requires a successful durable commit; if storage is unavailable or full, keep the editor recoverable and clearly report that the action was not queued. Draft autosave does not authorize dispatch. The explicit action submission is authorization for its automatic later execution, without a routine second confirmation on reconnect.

Initial owner: Sprint 02. Initial verification: Sprint 09. Durable schema and operation/comment/review step identities in 02; offline UI foundation in 04; full lifecycle, replay, dependencies and step receipts across writers in 07-09.

### OUTBOX-02

Use a recoverable operation lifecycle: Queued (including waiting for connection, dependency, pause or retry time), Validating (fresh read/preflight), Sending (a mutation may reach GitHub), Outcome unknown (automatic reconciliation), Waiting for GitHub (accepted asynchronous work), Needs attention (blocked by auth, conflict, unsupported capability or permanent validation failure), Confirmed, and Cancelled. Store a machine-readable reason independently of the state. Unsubmitted Draft is not an outbox entry. Confirmed and Cancelled are terminal only when justified by evidence; an ambiguous or remotely pending action remains inspectable. Exhausted transient retries wait in Queued and resume automatically when eligible.

Initial owner: Sprint 02. Initial verification: Sprint 09. Durable schema and operation/comment/review step identities in 02; offline UI foundation in 04; full lifecycle, replay, dependencies and step receipts across writers in 07-09.

### OUTBOX-03

Provide an Outbox/Pending actions panel that works offline. Show all unresolved submitted actions for the active account, with type, repository, existing target item/comment/thread, payload preview, intended revision/method, state and reason, dependencies, creation/attempt times, and next eligible attempt. Support filters, opening the originating item, individual cancellation, Pause/Resume queued actions, and Retry now when safe. Explain any unavailable control. Pending counts and row states update across panes; confirmed/cancelled history remains inspectable under a documented retention policy. Returning cancelled content to a draft is supported; deleting drafts is separate.

Initial owner: Sprint 07. Initial verification: Sprint 09. Offline panel shell in 04; real inspection/cancel controls in 07; verify all supported action types in 09.

### OUTBOX-04

Automatically validate and dispatch eligible submitted actions after reconnection, application restart/wake, dependency completion, authorization repair under the same user, or expiry of a retry delay. Apply bounded exponential backoff with jitter, mutation pacing, shared API budgets, and relevant server headers. Off-hours read schedules and notification quiet hours do not delay already submitted actions. Respect Pause queued actions and Work offline. Definitely unapplied temporary failures remain queued; unrecoverable or content conflicts require attention; uncertain mutation outcomes follow OUTBOX-05. Only the running application/tray can dispatch. [S11] [S12]

Initial owner: Sprint 07. Initial verification: Sprint 09. Scheduler signals and pause semantics in 05; automatic eligible dispatch and pacing for each writer in 07-09.

### OUTBOX-05

Prevent duplicate local submission with stable operation IDs, transactional claims, and per-attempt evidence; do not assume this creates server idempotency. For a connection loss, timeout, crash, or ambiguous server error after dispatch, preserve Outcome unknown and reconcile before another mutation. Automatically retry only if reliable evidence shows no remote effect, or verified endpoint idempotency makes it safe. Persist known remote object/operation IDs and confirm through server state where possible. Matching body text or a brief absence from a list is not proof. If still unresolved, retain the action for attention rather than silently replaying or marking success. No unconditional exactly-once delivery guarantee is made.

Initial owner: Sprint 07. Initial verification: Sprint 09. Storage/dispatcher contracts precede actual issue/comment replay in 07; finish uncertain-outcome, cancellation and current-intent validation for reviews and merges by 09.

### OUTBOX-06

Preserve dependency order for related supported operations while allowing unrelated eligible work to progress under the shared budget. Retain stable local operation/draft identities and record confirmed server identities for comment, review, and thread steps where a later supported operation requires them. Preserve review/thread sequencing and meaningful per-target edit order, including proven predecessor results in later preflight comparisons. Persist step identities and receipts for multi-request workflows; resume eligible unfinished steps rather than repeating completed steps. Show partially applied remote effects explicitly. When a predecessor is cancelled, rejected, or uncertain, hold dependent actions with a clear reason; never guess a remote target or drop descendants silently. Cancelling a parent must show its dependent impact, and users may inspect/cancel each remaining dependent. New-issue placeholders, local issue creation IDs, and create-issue dependency chains are not part of this contract.

Initial owner: Sprint 02. Initial verification: Sprint 09. Durable schema and operation/comment/review step identities in 02; offline UI foundation in 04; full lifecycle, replay, dependencies and step receipts across writers in 07-09.

### OUTBOX-07

Cancellation of an unsent queued or validating action MUST durably win against dispatch through an atomic state transition checked immediately before the mutation is sent. If cancellation wins, no request for that operation may later be sent, including after restart. If dispatch has already won, show Cancellation requested/May already be sent, stop further mutation attempts, and continue permitted read-only reconciliation. Cancelling an HTTP task is not proof GitHub did nothing. Do not claim remote rollback: a confirmed comment remains a comment and a remotely completed merge remains merged. GitHub merge-queue withdrawal is outside first-release scope. Keep original text, dispatch evidence, and dependent actions recoverable even if sign-out delays reconciliation.

Initial owner: Sprint 07. Initial verification: Sprint 09. Storage/dispatcher contracts precede actual issue/comment replay in 07; finish uncertain-outcome, cancellation and current-intent validation for reviews and merges by 09.

### OUTBOX-08

Freshly validate the original target, permission, edit base values, review anchors, intended commit, merge method, and applicable policy immediately before mutation dispatch. Automatically execute unchanged valid intent without routine reconfirmation. A conflicting remote edit, changed reviewed/merge head, missing target, or changed permission moves the action to Needs attention. Never silently rebase a queued edit, re-anchor a review, approve a new revision, substitute a merge head, or change the chosen merge method. Show the current-versus-intended context so the user can correct or cancel it. Live server enforcement remains authoritative. [S09] [S10]

Initial owner: Sprint 07. Initial verification: Sprint 09. Storage/dispatcher contracts precede actual issue/comment replay in 07; finish uncertain-outcome, cancellation and current-intent validation for reviews and merges by 09.

### OUTBOX-09

Bind each action permanently to the original host, user/account context, repository/resource identity, and credential owner scope; resolve credentials by protected reference at dispatch, never from a stored token in the payload. Switching users pauses the inactive account's work and isolates its queue contents. Reauthorization may resume the original user's eligible work; another user cannot inherit it. Removing a repository selection preserves its pending work and marks it paused for scope review instead of deleting or secretly sending it. Confirmed locator changes use verified identity and preserve the queue's stable local target.

Initial owner: Sprint 03. Initial verification: Sprint 09. Original-user credential/scope binding begins in 03; panel isolation in 04; deferred issue/review/merge dispatch verified through 09.

### OUTBOX-10

Treat unresolved outbox operations, payloads, cancellation flags, dependency mappings, base snapshots, anchors, and remote reconciliation evidence as durable user data. Protect them from cache eviction, repair resets, account sign-out, and migration. Recover interrupted claims conservatively after a crash; an interrupted Sending state becomes Outcome unknown unless evidence proves it was never dispatched. Reconcile a successful remote write whose local completion transaction failed. Pending actions have no silent age expiry. Bound and virtualize the outbox UI, redact routine diagnostics, and preserve essential local content until completion/cancellation and any dependent work are settled.

Initial owner: Sprint 02. Initial verification: Sprint 12. Persist/protect queue data in 02, integrate all writer snapshots/receipts in 07-09, then verify full upgrade/cleanup/crash/backlog matrix in 12.

### DATA-01

Apply migrations transactionally where supported and test upgrading representative databases. Back up user-authored drafts and unresolved outbox work before risky transformations. Detect corruption or incompatible schema versions without silently resetting user work. Preserve operation identity, cancellation decisions, and reconciliation evidence across migration and recovery.

Initial owner: Sprint 02. Initial verification: Sprint 12. Migration and recovery mechanisms in 02; validate representative upgrade databases after all draft types and release schema exist.

### DATA-02

Use UTC for persisted event timestamps and timezone-aware local scheduling. Use monotonic timing for in-process delays. Do not use a local wall-clock jump as evidence that remote content changed.

Initial owner: Sprint 02. Initial verification: Sprint 05. UTC/monotonic conventions in 02; timezone/DST scheduling exercised in 05.

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

### UX-01

Support platform-appropriate keyboard navigation, focus indication, copy and paste, high-DPI displays, light and dark appearance, and sensible resizing. Test real text editors and review controls with the accessibility features used by the target audience on each published OS. Windows is verified first; macOS/Linux parity closes in their release phases.

Initial owner: Sprint 04. Initial verification: Sprint 12. Keyboard, accessibility, DPI and appearance checked as native controls arrive; integrated release verification in 12.

### UX-02

Preserve user context while refreshing. Avoid replacing a useful cached view with a global spinner, jumping scroll positions, or losing a draft when selecting another tab. Loading and errors belong to the affected resource or pane; a slow request in one pane must not block navigation or editing in another.

Initial owner: Sprint 04. Initial verification: Sprint 10. Context-preserving loading/error behavior must hold through all reader, writer, diff and history views.

### INSTALL-01

Each published platform MUST offer an end-user installation and rerunnable update path requiring one to three documented commands in its normal shell. Install prebuilt release artifacts; do not require Rust, Cargo, a compiler, Git checkout, or local source compilation. Use a per-user install by default without administrator privileges where the supported platform permits. Normal OS trust/permission prompts and the app's later PAT setup are allowed and must be explained; one to three commands does not promise zero interaction. The flow assumes the explicitly documented supported OS/system prerequisites, not hidden manual toolchain setup.

Initial owner: Sprint 01. Initial verification: Sprint 12. Define short-command/prebuilt/compatibility/trust contracts in 01; deliver actual Windows artifacts, exact published command tests and release verification in 12. macOS/Linux counterparts close in their own rows.

### INSTALL-02

Before changing the installed application, detect and validate OS, architecture, minimum supported version and required runtime/graphics/desktop prerequisites. Select the exact compatible artifact from a versioned release manifest; reject unsupported combinations with an actionable explanation. Avoid silent architecture substitution, undocumented compatibility assumptions, or downloading a different channel/version than requested. App dependencies should be bundled where practical; unavailable system prerequisites produce precise repair guidance instead of silent system modifications.

Initial owner: Sprint 01. Initial verification: Sprint 12. Define short-command/prebuilt/compatibility/trust contracts in 01; deliver actual Windows artifacts, exact published command tests and release verification in 12. macOS/Linux counterparts close in their own rows.

### INSTALL-03

Verify artifact integrity and publisher authenticity before execution or installation using the platform's appropriate signing/trust mechanism or a verified signed release manifest with an established trust root. A checksum alone establishes integrity, not publisher authenticity. Download over authenticated transport, validate the selected version/manifest/artifact relationship, fail closed on mismatch or failed verification, and clean up failed staging files. Do not require weakening OS security, disabling certificate checks, execution-policy bypass, or unauthenticated remote-script execution. The documented short command flow must preserve this trust chain.

Initial owner: Sprint 01. Initial verification: Sprint 12. Define short-command/prebuilt/compatibility/trust contracts in 01; deliver actual Windows artifacts, exact published command tests and release verification in 12. macOS/Linux counterparts close in their own rows.

### INSTALL-04

Stage and verify an update before replacing the installed binary. Handle a running instance and file locks safely, prevent simultaneous migration/Outbox dispatch during replacement, and preserve SQLite data, drafts, tabs/layout, settings, offline pins, credential references/protected tokens, sync checkpoints, baselines, and pending-action identities/receipts. Back up user data before risky migrations, and recover from interrupted install/update without falsely confirming actions or losing local work. Roll back binaries only when schema compatibility is safe; do not blindly restore an old Outbox snapshot that could replay already applied mutations. Explain recovery if migration cannot safely continue.

Initial owner: Sprint 02. Initial verification: Sprint 12. Storage backup/migration and durable operation invariants begin in 02; real all-state installed upgrades close in 12, then repeat per later platform.

### INSTALL-05

Rerunning the same install command MUST detect an installed matching version and finish successfully without duplicating shortcuts, startup entries, instances, data directories or credentials. Support a documented stable channel and explicit version selection; upgrades preserve stable app identity and state. Reject unsafe downgrades or schema incompatibility with clear recovery guidance. Startup-on-login remains opt-in; installation must not silently activate background networking or override offline/pause settings.

Initial owner: Sprint 12. Initial verification: Sprint 12. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### INSTALL-06

Failures MUST provide a short actionable explanation, a nonzero exit code when applicable, and a safe rerun path without asking users to compile or inspect developer logs for routine problems. Test denied permissions, blocked downloads, insufficient disk space, interrupted downloads, signature/hash failure, incompatible OS/architecture, missing prerequisite, and running-app locks. Do not request an application PAT for a public installer or expose credentials in commands/logs. If a chosen private release distribution requires separate download authorization, document that boundary without reusing the app's GitHub PAT implicitly.

Initial owner: Sprint 12. Initial verification: Sprint 12. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### INSTALL-07

Provide a documented uninstall path using native platform conventions or an installed uninstaller. Uninstall removes app binaries and registrations while preserving local user data by default; removing data or protected credentials is an explicit separate choice with pending-work consequences shown. Reinstall must recover the retained profile, workspace and progress instead of forcing a full archive. Respect the already specified distinction between ordinary exit, explicit sign-out and cache deletion.

Initial owner: Sprint 12. Initial verification: Sprint 12. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### INSTALL-08

Release deliverables MUST include the real published binaries/packages, version/architecture manifest, verification metadata, install/update/uninstall documentation, and tested one-to-three-command installation examples for that released platform. No release repository, URL or binary is available in this specification package: all illustrative commands are labeled templates until replaced with actual published endpoints and tested. Record clean-machine install, rerun, upgrade with seeded drafts/Outbox, failure recovery and uninstall/reinstall evidence on each claimed platform. A script file, placeholder URL or successful source build is not completed installer acceptance. [S35]

Initial owner: Sprint 12. Initial verification: Sprint 12. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

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

### TEST-07

Live-provider validation MUST use an explicitly designated disposable repository and appropriate test identities with no credentials embedded in code or results. Read-only live tests are the default; mutation tests require explicit --allow-live-writes and a configured allowlisted owner/repository, and cover only supported edits/comments/reviews/merges. Use existing issue/PR fixtures and do not add an app issue/PR-creation flow for test setup. Preserve ordinary SSO and permission enforcement. Keep destructive cases isolated, identify any effects and cleanup, and mark unavailable credentials/permissions Blocked rather than inventing successful integration evidence.

Initial owner: Sprint 03. Initial verification: Sprint 12. Guarded live read setup in 03; explicitly allowed supported mutations in 07-09; required live evidence closes at each applicable platform release without inventing credentials.

### TEST-08

A sprint report MUST list exact commands, commit/build identity, OS/version/architecture, fixture/seed, test layer and scope, results, failure artifacts, limitations, and requirement/AC/scenario links. Distinguish Pass, Fail, Blocked and Not run; skipped or mocked behavior never counts as a live/native pass. Maintain requirement, acceptance and scenario evidence per release platform, identifying simulator/test builds separately from the exact installed published package and its digest/signature. Every phase gate requires applicable unit/integration/native E2E/install tests, published-package smoke evidence, and targeted performance/accessibility evidence with no unexplained required failures. Rerun affected regression tests when shared code changes; do not claim all-platform completion from a Windows report or from validation of this planning package.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

## Verification due for this phase

FND-01, FND-02, FND-03, FND-04, FND-05, FND-06, FND-07, PORT-01, PORT-02, PORT-03, PORT-04, PORT-05, PORT-06, PORT-08, AUTH-01, AUTH-02, AUTH-03, AUTH-04, AUTH-05, AUTH-06, AUTH-07, AUTH-08, AUTH-09, AUTH-10, AUTH-11, BROWSE-01, BROWSE-02, BROWSE-03, BROWSE-04, BROWSE-05, WORKSPACE-01, WORKSPACE-02, WORKSPACE-03, WORKSPACE-04, WORKSPACE-05, WORKSPACE-06, WORKSPACE-07, WORKSPACE-08, WORKSPACE-09, WORKSPACE-10, WORKSPACE-11, WORKSPACE-12, SEARCH-01, SEARCH-02, SEARCH-03, SEARCH-04, CACHE-01, CACHE-02, CACHE-03, CACHE-04, CACHE-05, CACHE-06, CACHE-07, CACHE-08, CACHE-09, CACHE-10, CACHE-11, CACHE-12, CACHE-13, OFFLINE-01, OFFLINE-02, OFFLINE-03, OFFLINE-04, SYNC-01, SYNC-02, SYNC-03, SYNC-04, SYNC-05, SYNC-06, SYNC-07, SYNC-08, SYNC-09, SYNC-10, SYNC-11, CONNECT-01, CONNECT-02, CONNECT-03, ISSUE-01, ISSUE-02, ISSUE-03, ISSUE-04, ISSUE-05, ISSUE-06, PR-01, PR-02, PR-03, PR-04, PR-05, PR-06, PR-07, REVIEW-01, REVIEW-02, REVIEW-03, REVIEW-04, DIFF-01, DIFF-02, DIFF-03, DIFF-04, DIFF-05, DIFF-06, DIFF-07, GRAPH-01, GRAPH-02, GRAPH-03, GRAPH-04, GRAPH-05, GRAPH-06, GRAPH-07, NOTIFY-01, NOTIFY-02, NOTIFY-03, NOTIFY-04, NOTIFY-05, NOTIFY-06, NOTIFY-07, NOTIFY-08, NOTIFY-09, NOTIFY-10, NOTIFY-11, NOTIFY-12, NOTIFY-13, NOTIFY-14, NOTIFY-15, NOTIFY-16, NOTIFY-17, NOTIFY-18, NOTIFY-19, NOTIFY-20, WRITE-01, WRITE-02, WRITE-03, WRITE-04, MERGE-01, MERGE-02, MERGE-03, MERGE-04, OUTBOX-01, OUTBOX-02, OUTBOX-03, OUTBOX-04, OUTBOX-05, OUTBOX-06, OUTBOX-07, OUTBOX-08, OUTBOX-09, OUTBOX-10, DATA-01, DATA-02, DATA-03, DATA-04, DATA-05, DATA-06, DATA-07, DATA-08, DATA-09, DATA-10, UX-01, UX-02, INSTALL-01, INSTALL-02, INSTALL-03, INSTALL-04, INSTALL-05, INSTALL-06, INSTALL-07, INSTALL-08, TEST-01, TEST-02, TEST-03, TEST-04, TEST-05, TEST-06, TEST-07, TEST-08.

Use requirement_platform_traceability.csv for applicability and release-gate evidence. Initial ledger statuses refer only to their initial phase; no single Windows result closes all platforms. Keep future options and unavailable services explicit.

## Later scope and handoff

Maintain regression and packaging gates for all released targets. Additional distributions, architectures, desktop environments and OS versions remain explicit compatibility work.

## Coding LLM instruction

Implement Sprint 14 against v1.7. Keep the app buildable and preserve the current product constraints. Complete listed implementation and test work, use actual native controls for E2E, protect original targets/drafts/Outbox state, and add only the named platform integrations. Do not add issue/PR creation, merge queues or browser engines. Keep secrets out of code/results; live writes require explicit allowlisted authorization. Implement the short verified prebuilt install/update contract for the relevant publication phase and never claim placeholder commands runnable. Update initial/per-platform/scenario ledgers with commands, environment, package identity and evidence, then use SPRINT_REPORT_TEMPLATE.md. Continue only within the assigned sprint/Windows/all-platform scope without new routine approvals.

## Technical sources

- [S01 GPUI framework and Windows rendering](https://gpui.rs/examples/)
- [S02 GPUI Kit native diff component](https://gpui-kit.com/component/diff/)
- [S03 GPUI Kit selectable rich text](https://gpui-kit.com/component/text-view/)
- [S04 SQLite write ahead logging](https://www.sqlite.org/wal.html)
- [S05 SQLite FTS5 full text search](https://www.sqlite.org/fts5.html)
- [S06 GitHub issue and PR conversation comments](https://docs.github.com/en/rest/issues/comments)
- [S07 GitHub REST pagination](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api)
- [S08 GitHub REST search](https://docs.github.com/en/rest/search/search)
- [S09 GitHub pull request reviews](https://docs.github.com/en/rest/pulls/reviews)
- [S10 GitHub pull request reads and merges](https://docs.github.com/en/rest/pulls/pulls)
- [S11 GitHub REST best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)
- [S12 GitHub REST rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)
- [S13 GitHub inline review comments](https://docs.github.com/en/rest/pulls/comments)
- [S14 GitHub notifications API compatibility](https://docs.github.com/en/rest/activity/notifications)
- [S15 GitHub GraphQL pull request and review types](https://docs.github.com/en/graphql/reference/pulls)
- [S16 GitHub branch comparison semantics](https://docs.github.com/en/pull-requests/reference/branches)
- [S17 Git branch and commit relationships](https://git-scm.com/book/en/v2/Git-Branching-Branches-in-a-Nutshell)
- [S18 GitHub commit history and parent identities](https://docs.github.com/en/rest/commits/commits)
- [S19 Git clone and partial clone behavior](https://git-scm.com/docs/git-clone)
- [S20 GitHub personal access token capabilities](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens)
- [S21 GitHub REST authentication and SSO responses](https://docs.github.com/en/rest/authentication/authenticating-to-the-rest-api)
- [S22 Organization approval of fine grained tokens](https://docs.github.com/en/organizations/managing-programmatic-access-to-your-organization/managing-requests-for-personal-access-tokens-in-your-organization)
- [S23 GitHub OAuth app authorization and organization access](https://docs.github.com/en/enterprise-cloud%40latest/apps/oauth-apps/using-oauth-apps/authorizing-oauth-apps)
- [S24 Windows desktop notification APIs](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/)
- [S25 GitHub Events API limitations](https://docs.github.com/en/rest/activity/events)
- [S26 GitHub branch and tag references](https://docs.github.com/en/rest/git/refs)
- [S27 GitHub shared issue and PR numbering](https://docs.github.com/en/enterprise-cloud%40latest/rest/using-the-rest-api/issue-event-types)
- [S28 GitHub issue representations identifiers and updates](https://docs.github.com/en/rest/issues/issues)
- [S29 GitHub linking pull requests and issues](https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/linking-a-pull-request-to-an-issue)
- [S30 GitHub repository auto-close settings](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/managing-repository-settings/managing-auto-closing-issues)
- [S31 GPUI Kit installation and target prerequisites](https://gpui-kit.com/docs/installation/)
- [S32 GPUI Kit testing facilities and limits](https://gpui-kit.com/docs/test/)
- [S33 GPUI Kit accessibility](https://gpui-kit.com/docs/accessibility/)
- [S34 GPUI Kit system notification capabilities](https://gpui-kit.com/docs/system-notification/)
- [S35 GPUI Kit packaging](https://gpui-kit.com/docs/packaging/)
- [S36 Keyring native protected credential stores](https://docs.rs/keyring/latest/keyring/v1/index.html)

Additional current primary sources and platform/install/test constraints are in the three companion plans. Verify pinned tool/API behavior during implementation.
