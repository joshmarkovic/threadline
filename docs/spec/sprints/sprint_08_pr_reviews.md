# Sprint 08: Inline PR reviews, approvals, and review drafts

Requirements version 1.7 | Initial sprint status Planned

## Outcome

Users can review exact source revisions, retain unfinished reviews, and submit comments, approvals, or requests for changes as themselves.

## Phase and prerequisites

- Release phase: Windows first release.
- Prerequisites: Sprint 06, Sprint 07.
- Code areas: review, commands/reviews and threads, github/review mutations, workspace/review drafts, ui/inline review and navigation.
- Read the master, TESTING_PLAN.md, INSTALLATION_PLAN.md, CROSS_PLATFORM_PLAN.md and all relevant scenario/platform ledgers.
- Use actual prior evidence; a missing dependency remains blocked rather than passed. Later-platform release work does not silently move into the Windows phase.

## Implementation tickets

### S08-T01

Implement inline comment entry, replies, a multi-comment draft review, comment-only submission, approval, request changes, and permitted thread resolve/reopen actions.

### S08-T02

Persist path, original/modified side, source line range, commit identity, and original context for each inline draft. Submit against the intended reviewed commit through the shared write engine.

### S08-T03

Detect new heads and force-pushes, preserve original anchors, show changed context, and require explicit refresh/re-anchor choices. Never silently attach the draft to different code.

### S08-T04

Complete inline thread display, unresolved-thread navigation, and persistent per-revision viewed-file markers. Invalidate reviewed status when the file changes; keep layout and selection stable as annotations arrive.

### S08-T05

Support accurate ignore-whitespace viewing when available while preserving unfiltered source coordinates. Provide an explicit limitation if accuracy cannot be guaranteed; never submit a filtered row index as a source location.

### S08-T06

Complete shared review-editor ownership, offline restart recovery, draft/outbox-safe cleanup, permissions/identity checks, and keyboard/accessibility behavior in actual review controls.

### S08-T07

Queue inline comments, replies, review submissions, approvals, requests for changes, and permitted thread actions offline through the common outbox. Retain the reviewed commit, source coordinates and per-step receipts; automatically execute unchanged valid intent and block changed context. Integrate pending previews, cancellation, dependencies and unknown-outcome reconciliation.

### S08-T08

Complete AC-63 using actual supported review/thread steps, such as a pending review requiring its returned server identity before submission. Persist successful receipts, hold uncertain/cancelled predecessors and dependents, and let unrelated actions proceed. A restart resumes unfinished steps without repeating completed mutations; no create-issue chain exists.

### S08-T09

Complete AC-78 with an actual inline review draft: toggle PR body/verified issue, pick another linked issue, resize/collapse context, narrow/restore, and switch or restore PR tabs offline. Keep the code comparison/file/scroll, draft identity/anchors and intended target unchanged; asynchronous context responses do not steal editor focus. Retain general multi-item panes and keyboard navigation.

### S08-T10

Add actual review-control E2E for anchored drafts, body/issue context switches, submit/approve/request changes, dependency receipts and changed-head holds. Verify source mapping with unit/integration assertions and approved live review cases separately.

## Required tests and commands

- Unit/integration: anchors, source sides, changed-head validation, review/thread step receipts and whitespace source mapping.
- Native E2E: actual inline draft, body/issue toggle, restore, submit/approve/request changes, and dependency cancellation/reconnect while preserving intended revision.
- Fault/live: force-push and ambiguous review results plus explicitly approved live review identity/permissions; no automatic approval of new code.

Implement and use the canonical runner from TESTING_PLAN.md: unit, integration, ui, native e2e smoke/full, install with an artifact, and separately guarded live tests. These are development commands to build, not commands already supplied by this ZIP. Record actual OS/build/fixture evidence and mark unavailable checks Blocked/Not run. Native test-build E2E and exact published-package smoke remain separate.

Scenario IDs introduced or closing here: IT-011, IT-013, UI-004, E2E-007, E2E-008, LIVE-003.

Live-provider cases are shared gates on an explicitly authorized host. A later platform can reference existing evidence only with a recorded applicability assessment; rerun changed provider/credential-policy or target-specific behavior. Missing authorization remains Blocked/Not run and never permits unapproved writes.

Use test_scenarios.csv for fixture IDs, expected results, first/closure ownership and evidence. Release sprints rerun all scenarios applicable to their target; earlier sprint scenario lists do not waive regression checks.

## Deliverables

End-to-end PR review workflow; durable anchored review drafts; thread actions; viewed-file navigation; revision-change handling; review integration evidence.

## Exit gates

- Pass AC-16, AC-17, AC-18, and AC-21, including multiple inline comments, restart, offline recovery, force-push, both diff modes, and source-side correctness.
- Validate approval/request-changes identity in an authorized test PR with suitable authorship and permissions. Missing permission is an explicit unavailable capability, not a successful review.
- Test timeout reconciliation, duplicate-view submission prevention, focus switches, and account isolation for actual review drafts.
- Re-run draft restoration, cleanup, local search of review text, editor keyboard checks, and source-context expansion. Preserve all prior issue-write behavior.
- Pass AC-63, AC-65, and AC-78 and repeat outbox cancellation, identity, lost-response and dependent-step tests for actual review workflows. Full multi-action AC-62/67/71 remains assigned to Sprint 09.

## Initial acceptance scenarios owned here

- **AC-16** A review with multiple inline comments can be saved, restarted, reopened, and submitted against the intended commit.
- **AC-17** A force-push preserves the original draft anchors and requires explicit handling of changed context.
- **AC-18** Approval and request-changes actions appear under the authenticated user's identity.
- **AC-21** Unified and split views show the same underlying change and keep inline comments on the correct source side and lines.
- **AC-63** Queue a supported review/thread workflow whose later step requires the confirmed predecessor identity, plus meaningful ordered edits on an existing target. Persist operation IDs and successful step receipts across restart, run unfinished steps in order without repeating completed mutations, and include proven predecessor changes in later conflict checks. A cancelled, failed, or uncertain predecessor holds its dependents with a visible reason while unrelated eligible actions continue. Test actual review/thread adapters, not only storage fixtures; no new-issue creation chain is used.
- **AC-65** A queued inline review, approval, or request for changes retains its intended commit/anchors across offline restart. An unchanged valid revision submits automatically; a changed head or invalid anchor requires attention and cannot silently approve or comment on different code. Permitted queued thread actions follow the same intent checks.
- **AC-78** At a usable width, a PR review tab opens with one 30-35% context / 65-70% code pair and PR body selected. Resize/collapse the context, toggle View PR body / View issue, select another verified issue, and use a narrow focused surface while an inline review draft is open. Preserve the code file/comparison, scroll/selection, draft identity/anchors, and deliberate editor focus. Restore the layout, context mode, selected issue, and separate scroll anchors after tab switching and offline restart. Multiple PR tabs and general Open beside remain usable without a required third issue pane.

## Full owned requirements

### REVIEW-01

Users MUST create inline comments, replies, and a draft review, then submit a comment-only review, approval, or request for changes. Support resolving and reopening review threads where the API and permissions allow it. [S09] [S15]

Initial verification owner: Sprint 08. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### REVIEW-02

Persist each inline draft with repository, PR, file path, original or modified side, source line range, reviewed commit identity, and available context. Store source coordinates rather than visible row numbers.

Initial verification owner: Sprint 08. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### REVIEW-03

New commits and force-pushes MUST NOT silently move a draft or queued review onto different code. Preserve the original context and intended reviewed commit, indicate that the PR changed, and provide an explicit refresh or re-anchor workflow. Revalidate queued reviews, approvals, requests for changes, and thread actions before dispatch. A changed reviewed revision or invalid anchor requires attention instead of automatically approving different code. GitHub's review API accepts a commit identity; use the intended reviewed revision. [S09]

Initial verification owner: Sprint 08. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### REVIEW-04

Preserve unfinished reviews and submitted review actions across restart and connection loss. Offline users MUST be able to read cached review data, edit drafts, submit them to the local outbox, inspect pending actions, and cancel eligible entries. Actual GitHub submission and thread mutations execute automatically after connectivity returns and current permissions, source anchors, and revision checks pass. An unsent saved draft remains a draft.

Initial verification owner: Sprint 08. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### DIFF-02

Provide inline thread display, comment-entry controls, unresolved-thread navigation, and persistent per-revision viewed-file markers. A file changed by a subsequent PR revision must not remain silently marked fully reviewed.

Initial verification owner: Sprint 08. Read threads and establish viewed-file state in 06; inline entry, navigation and revision invalidation completed in 08.

### DIFF-05

Support an ignore-whitespace viewing option when backed by an accurate comparison. Preserve original source coordinates for review submission; do not derive comment positions from a visually filtered row index.

Initial verification owner: Sprint 08. Source coordinates established in 06; accurate filtering and review-submission mapping verified in 08.

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

### AUTH-02

Authenticate as the user for issue changes, reviews, approvals, and merges. Show the active identity and relevant organization or credential profile. Do not submit the user's reviews as an installation bot identity.

Initial owner: Sprint 03. Initial verification: Sprint 09. Verify user identity and actual endpoint permissions for issue, review, and merge actions as each becomes available.

### AUTH-06

Provide a user-initiated action to open an appropriate GitHub PAT or SSO authorization page in the external default browser. Validate returned authorization URLs before opening them. Preserve the current screen, cached content, drafts, outbox, and synchronization checkpoints while authorization is repaired. After verifying that a replacement PAT belongs to the same GitHub user and restores the required access, reuse the existing account/resource identities and progress, retry eligible reads, and automatically resume eligible submitted actions under the persisted pause/offline settings. A different user must receive a separate account context; replacing a token must not create a new cache for the same user.

Initial owner: Sprint 03. Initial verification: Sprint 09. Authorization repair restores the same identity in 03; reconnect signals in 05 and automatic eligible outbox resumption across all writers in 07-09.

### AUTH-08

Request and document the permissions actually required by the chosen endpoints. Issue writes, PR review writes, commit access, and merging must each be validated against current endpoint documentation. Check access to an explicitly selected private repository, not merely whether `/user` succeeds. Preserve readable functions when a write permission is missing. [S06] [S09] [S10]

Initial owner: Sprint 03. Initial verification: Sprint 09. Verify user identity and actual endpoint permissions for issue, review, and merge actions as each becomes available.

### AUTH-11

Explicit sign-out MUST durably mark the affected account profile as signed out, prevent new network dispatch under that profile, and remove or invalidate its locally stored PAT material. Preserve readable private cached content, drafts, saved workspace, settings, synchronization checkpoints, activity baselines, and outbox records on the PC. Users may reopen that signed-out local profile for offline work without a renewed login; show its signed-out status and hold its remote actions until a PAT for the same verified user restores access. Keep that hold across restart. Ordinary exit is not sign-out. Expired or revoked PATs likewise preserve offline use and progress while holding affected remote work. A sign-out during an in-flight mutation does not prove remote cancellation; retain its evidence for reconciliation after access is restored. Do not expose one profile's data through a different active account or assume sign-out revokes the PAT at GitHub.

Initial owner: Sprint 03. Initial verification: Sprint 12. Signed-out credential/dispatch holds begin in 03; readable local-profile UI in 04 and all-writer PAT repair in 09; complete private-data retention and in-flight lifecycle matrix in 12.

### BROWSE-04

Support recent items, pinned items, back and forward navigation, and opening an accessible GitHub issue or PR URL through the workspace rules below. Keyboard navigation must reach repository lists, item lists, pane controls, item tabs, PR subviews, search, and the review editor.

Initial owner: Sprint 04. Initial verification: Sprint 08. Navigation begins in 04; verify access to actual PR subviews and review editor in 06/08.

### BROWSE-05

Review requested, Assigned to me, and Outbox MUST show a positive total in a right-aligned dark blue filled circular badge with centered white digits. Use a suitable accessible color, provisionally #173E70, and preserve a true circle, label clearance, and legibility for multi-digit totals and high-DPI scaling. A known zero draws neither a circle nor a number. Counts are totals for the current account and each view's stated repository/filter scope, not unread-notification counts or just the visible page length. Count distinct matching items for saved views. Use a trustworthy complete-query total when available; otherwise label the cached/known count and incomplete coverage, without presenting unknown or partial coverage as a confirmed empty result. Outbox counts all unresolved submitted actions, including paused, blocked, and outcome-unknown actions, and excludes drafts, download jobs, and confirmed/cancelled history. Refresh counts from shared state after ingestion, edits, scope changes, enqueue, cancellation, and confirmation without per-pane polling. Accessible names and count details must expose scope, freshness, and any coverage limitation.

Initial owner: Sprint 04. Initial verification: Sprint 09. Count component and shared query totals in 04; requested-review metadata in 06 and actual pending states in 07-09. Full AC-81 validates all supported writers, scope/coverage, circular styling and zero suppression in 09.

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

### WORKSPACE-12

Provide documented keyboard commands for opening a new tab or side pane, cycling tabs within a pane, moving focus between panes, closing and reopening a tab, and moving a tab between panes. Expose tab and pane names, selection, and focus to the published operating system's accessibility features, with Windows tested first. Test these commands from real text editors without intercepting ordinary editing shortcuts unexpectedly.

Initial owner: Sprint 04. Initial verification: Sprint 08. Workspace shortcuts and accessibility names in 04; test conflicts from real review text editors in 08.

### SEARCH-01

Provide fast local full-text search of downloaded titles, descriptions, and comments. Include available downloaded review text. Search results MUST identify the repository and item, highlight the match where practical, and navigate to the matching discussion or review location.

Initial owner: Sprint 04. Initial verification: Sprint 08. FTS schema in 02 and conversations in 04; formal review ingestion in 06 and confirmed submitted reviews in 08.

### CACHE-07

Existing-issue edit drafts, comment drafts, review drafts, and submitted outbox payloads are user data. Cache clearing, repair, failed downloads, migrations, reconciliation, and closing tabs or panes MUST preserve them unless the user explicitly deletes the drafts or cancels the actions. Drafts and outbox entries exist independently of any viewing tab. Preserve source anchors, edit base values, dependency mappings, and the minimal referenced content needed by unresolved actions; apply OUTBOX-10 to cleanup and recovery.

Initial owner: Sprint 02. Initial verification: Sprint 09. Draft/outbox-safe storage in 02; recheck cleanup and recovery for every draft and queued-action type through 09; repeat final migration matrix in 12.

### CACHE-09

Associate comparison data with immutable commit identities and comparison options. Associate mutable branch labels, status checks, and review state with their relevant revisions and timestamps. A branch name alone is not an adequate diff cache key.

Initial owner: Sprint 02. Initial verification: Sprint 10. Typed immutable comparison identity in 02; actual PR/review and repository comparison integration later.

### CACHE-13

Normal window close and application exit MUST retain downloaded content and resource coverage, all draft types, saved tabs/layout/view positions, local preferences, offline pins, pending actions and receipts, synchronization checkpoints/validators, and notification baselines/delivery history on the PC. Persist settings and user changes as they occur and safely flush remaining local state on orderly exit; if a user-data write fails, keep it recoverable and report the failure. Retain the protected credential according to AUTH-07. Restart MUST reuse these records and render the saved local workspace before networking completes. It must not clear the cache, reset baselines, or redownload all history merely because the process was restarted. Sign-out preserves local content under AUTH-11. Explicit cache cleanup remains a separate action and continues to protect drafts and unresolved work under CACHE-07 and OUTBOX-10.

Initial owner: Sprint 02. Initial verification: Sprint 12. Complete persistence schema in 02, PAT lifecycle in 03, local-first restoration in 04, sync in 05/06, all writer/history/notification types through 11, and the complete quit/sign-out/reopen matrix in 12.

### OFFLINE-01

Once an account and content have been established, the application MUST start and remain usable without a network check or renewed login blocking local access. Users can browse/search downloaded issues, PRs, discussions, reviews, commits, diffs, graph history, and the activity feed; use tabs and panes; edit local settings and drafts; and inspect or cancel pending work. State the cached coverage and last fetch honestly. Unseen remote content and new server events cannot be available until fetched. First-time authentication and initial acquisition of private content still require access; do not require a full repository download.

Initial owner: Sprint 04. Initial verification: Sprint 12. Offline startup/workspace in 04; each new view and action integrates as built; complete all-feature cold-start offline matrix in 12.

### OFFLINE-02

Offline mode MUST allow preparation and explicit submission of every implemented remote mutation: existing-issue editing/state/metadata, ordinary comments and edits, review comments/replies/submission/approval/request changes, permitted thread actions, and merges. Keep their controls available when cached target data and capability information are sufficient, and queue intent locally. Unknown permissions or missing required target/source details are explicit prerequisites; never invent a target, bypass a known denial, or silently choose a newer revision. Local settings, drafts, viewed-file markers, and app-feed read state apply locally immediately.

Initial owner: Sprint 07. Initial verification: Sprint 09. Local preparation begins in 04; all supported mutation intents become offline-submittable in 07-09 with honest prerequisites.

### OFFLINE-04

Show submitted local work immediately as clearly pending in all matching views: for example a pending comment, a review action, or an edit to an existing issue awaiting confirmation. Keep pending projections separate from confirmed cached server records; a refresh cannot erase them, and cancellation removes only the pending projection while preserving recoverable user text. A pending merge or approval is not a confirmed merge or approval. Offline pins and minimal draft/outbox dependencies must survive ordinary cache cleanup until explicitly released or the action is resolved.

Initial owner: Sprint 04. Initial verification: Sprint 09. Storage protection in 02, pending-view foundation in 04, actual issue/comment/review/merge projections and cancellation integration in 07-09.

### SYNC-03

Give priority to explicit user actions and their eligible submitted outbox work, then the selected item in each visible pane, enabled activity monitoring, recently used or pinned content, and slower reconciliation. Apply a shared budget and fair scheduling so a busy pane or outbox cannot starve other work. Fetch older history on demand. Do not launch a mandatory full historical backfill. Keep dependency order and mutation pacing even when draining the outbox.

Initial owner: Sprint 05. Initial verification: Sprint 09. Read scheduling and lifecycle in 05; prove eligible outbox dispatch outside read schedules and on restart across issue/review/merge actions in 07-09.

### SYNC-04

A user-triggered open or refresh may run outside work hours. Explicitly submitted outbox actions also resume outside work hours when eligible; quiet hours control notification delivery and read-refresh schedules control background reads, not authorization to send queued actions. Provide a separately labeled Pause queued actions control, and distinguish it from Pause background refresh and Work offline. Persist these settings and restore them before any startup network work. Work offline blocks all networking until disabled; Pause queued actions blocks new mutation dispatch, and Pause background refresh blocks scheduled catch-up reads. After sleep or reconnection, coalesce overdue read jobs into a bounded catch-up pass and resume eligible actions without replaying every missed timer tick or sending a mutation burst. Required outbox preflight/reconciliation reads remain governed by the action and connectivity controls, not the background-read schedule.

Initial owner: Sprint 05. Initial verification: Sprint 09. Read scheduling and lifecycle in 05; prove eligible outbox dispatch outside read schedules and on restart across issue/review/merge actions in 07-09.

### SYNC-09

Run synchronization, connectivity checks, and the outbox worker in the application or optional tray process. A fully exited application or sleeping machine cannot perform local polling or send actions. Normal exit preserves local state and the protected PAT; it does not sign the user out. On launch or wake, restore the local workspace and persisted control settings first, then resume eligible actions and bounded incremental read catch-up under SYNC-11 when connectivity and authorization permit. No hosted service is required for the initial release.

Initial owner: Sprint 05. Initial verification: Sprint 12. Lifecycle scheduling begins in 05, all writers integrate by 09, graph/feed families by 11, and retained-credential plus local-first incremental restart acceptance closes in 12.

### PR-05

The default PR review arrangement MUST show the native file list/diff prominently beside exactly one smaller context pane bound to the same active PR. At a usable window width, start with approximately 33% of the review content width for context and 67% for code, within a 30-35% / 65-70% range, excluding global navigation chrome. Context may be resized or collapsed; restore its last proportion. Provide View PR body and View issue controls in that one pane, with PR body as the initial mode for a newly opened review tab. They replace the context content rather than adding a simultaneous third pane. Preserve Conversation, Commits, Files, and Checks navigation and multiple PR tabs. At narrow widths, show a usable focused surface with an explicit way to switch context/code and restore the wider arrangement without losing either surface's state.

Initial owner: Sprint 06. Initial verification: Sprint 08. Native layout feasibility in 01 and workspace primitives in 04; bound context/code PR reader in 06; full AC-78 with actual inline drafts, focus, narrow layout and offline restore closes in 08.

### PR-07

View issue MUST display the selected verified issue's native body and available metadata/conversation in the context pane, with its repository, number, and coverage visible. View PR body restores that PR's body and context scroll state. Switching modes or linked issues must retain the code comparison, selected file, scroll/selection, review draft identity/anchors, and intended mutation target; background loads must not steal code-editor focus. Deliberate keyboard/pointer navigation may move focus normally. Use account, PR, body revision, selected issue, tab instance, and selection generation to discard stale context responses. Switching PR tabs must never show the previous PR's issue while loading. Distinguish no reference, verification pending, target is a PR, not downloaded/offline, inaccessible, failed, and removed or changed reference states; none is a confirmed empty issue. Cached PR bodies, linkage evidence, verified issues, and per-resource coverage remain usable offline with last-validation information. Preserve local drafts if body refresh removes a reference, and never silently attach them to a replacement issue.

Initial owner: Sprint 06. Initial verification: Sprint 08. Reference/cache/error and stale-response reading behavior closes AC-80 in 06; actual review-draft/focus stability and AC-78 close in 08.

### DIFF-04

Parse large patches and prepare highlighting outside the UI thread. Virtualize long file lists and diff rows. Keep scroll and selection stable when annotations or background data change. Ignore stale background results for an earlier comparison.

Initial owner: Sprint 06. Initial verification: Sprint 08. Virtualized async native diff in 06; prove annotation stability with actual review editing in 08.

### NOTIFY-16

Distinguish comment creation from editing. Record the user's successful writes so a subsequent poll does not echo them as unsolicited new activity when own-action suppression is enabled.

Initial owner: Sprint 07. Initial verification: Sprint 11. Creation/edit distinction in 05; own successful issue/comment/review/merge writes recorded in 07-09; suppression verified in 11.

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

### DATA-04

Credential switching must isolate private cached data, drafts, and queued actions. Bind each action to its original GitHub user/account access context and credential owner scope. Pause dispatch for inactive identities, signed-out profiles, or known revoked access; resume eligible work under the same verified identity after PAT repair and subject to persisted controls. Replace credentials for the same user without recreating the account, item identities, cache, outbox, or synchronization baselines. Never reroute queued work through another signed-in user. Failed authorization and explicit sign-out preserve local content under AUTH-11; a signed-out local profile remains readable without exposing it through another active account.

Initial owner: Sprint 03. Initial verification: Sprint 09. Identity isolation in 03; verify cached workspace, drafts and deferred issue/review/merge execution remain bound to the original account.

### DATA-08

Store ordinary issue and PR conversation comments in a shared conversation-comments model keyed to item_id. Formal reviews and inline review comments remain distinct resources with their own provider identity namespaces and source anchors. Share common labels, assignees, search, drafts, and workspace references through the item identity. Route each remote operation by its API capability; a shared table does not make PR review or merge operations valid for ordinary issues. [S06] [S09] [S10] [S13]

Initial owner: Sprint 02. Initial verification: Sprint 09. Shared comments and typed resources in storage/reads; review/merge guards complete in 09.

### UX-01

Support platform-appropriate keyboard navigation, focus indication, copy and paste, high-DPI displays, light and dark appearance, and sensible resizing. Test real text editors and review controls with the accessibility features used by the target audience on each published OS. Windows is verified first; macOS/Linux parity closes in their release phases.

Initial owner: Sprint 04. Initial verification: Sprint 12. Keyboard, accessibility, DPI and appearance checked as native controls arrive; integrated release verification in 12.

### UX-02

Preserve user context while refreshing. Avoid replacing a useful cached view with a global spinner, jumping scroll positions, or losing a draft when selecting another tab. Loading and errors belong to the affected resource or pane; a slow request in one pane must not block navigation or editing in another.

Initial owner: Sprint 04. Initial verification: Sprint 10. Context-preserving loading/error behavior must hold through all reader, writer, diff and history views.

### INSTALL-04

Stage and verify an update before replacing the installed binary. Handle a running instance and file locks safely, prevent simultaneous migration/Outbox dispatch during replacement, and preserve SQLite data, drafts, tabs/layout, settings, offline pins, credential references/protected tokens, sync checkpoints, baselines, and pending-action identities/receipts. Back up user data before risky migrations, and recover from interrupted install/update without falsely confirming actions or losing local work. Roll back binaries only when schema compatibility is safe; do not blindly restore an old Outbox snapshot that could replay already applied mutations. Explain recovery if migration cannot safely continue.

Initial owner: Sprint 02. Initial verification: Sprint 12. Storage backup/migration and durable operation invariants begin in 02; real all-state installed upgrades close in 12, then repeat per later platform.

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

BROWSE-04, WORKSPACE-12, SEARCH-01, PR-05, PR-07, REVIEW-01, REVIEW-02, REVIEW-03, REVIEW-04, DIFF-02, DIFF-04, DIFF-05.

Use requirement_platform_traceability.csv for applicability and release-gate evidence. Initial ledger statuses refer only to their initial phase; no single Windows result closes all platforms. Keep future options and unavailable services explicit.

## Later scope and handoff

The review portion of AC-45 is covered here; final multi-action target verification remains in Sprint 09. Repository-history comparisons remain in Sprint 10.

## Coding LLM instruction

Implement Sprint 08 against v1.7. Keep the app buildable and preserve the current product constraints. Complete listed implementation and test work, use actual native controls for E2E, protect original targets/drafts/Outbox state, and add only the named platform integrations. Do not add issue/PR creation, merge queues or browser engines. Keep secrets out of code/results; live writes require explicit allowlisted authorization. Implement the short verified prebuilt install/update contract for the relevant publication phase and never claim placeholder commands runnable. Update initial/per-platform/scenario ledgers with commands, environment, package identity and evidence, then use SPRINT_REPORT_TEMPLATE.md. Continue only within the assigned sprint/Windows/all-platform scope without new routine approvals.

## Technical sources

- [S06 GitHub issue and PR conversation comments](https://docs.github.com/en/rest/issues/comments)
- [S09 GitHub pull request reviews](https://docs.github.com/en/rest/pulls/reviews)
- [S10 GitHub pull request reads and merges](https://docs.github.com/en/rest/pulls/pulls)
- [S11 GitHub REST best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)
- [S12 GitHub REST rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)
- [S13 GitHub inline review comments](https://docs.github.com/en/rest/pulls/comments)
- [S15 GitHub GraphQL pull request and review types](https://docs.github.com/en/graphql/reference/pulls)
- [S31 GPUI Kit installation and target prerequisites](https://gpui-kit.com/docs/installation/)
- [S32 GPUI Kit testing facilities and limits](https://gpui-kit.com/docs/test/)
- [S33 GPUI Kit accessibility](https://gpui-kit.com/docs/accessibility/)
- [S35 GPUI Kit packaging](https://gpui-kit.com/docs/packaging/)

Additional current primary sources and platform/install/test constraints are in the three companion plans. Verify pinned tool/API behavior during implementation.
