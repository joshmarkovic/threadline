# Native GitHub Client Requirements

Implementation specification for a coding LLM

Version 1.7 | November 17 2026

### Changes in version 1.7

- The product is cross-platform, with Windows as the first and highest-priority release. Sprints 01-12 deliver Windows; Sprint 13 delivers macOS and Sprint 14 delivers Linux. Shared architecture and early build/core tests keep the later ports practical.
- Unit, integration, native-process end-to-end, authorized live-provider, and installation/upgrade tests are explicit deliverables. Each sprint specifies its relevant test work, reproducible commands, failures to exercise, and evidence. A native launch or test fixture is not proof of a completed user workflow.
- Each published platform must support a documented installation or update path of one to three commands using prebuilt binaries, with platform checks, integrity/authenticity verification, and preservation of local data. Examples in this planning pack are templates; no release repository, published binary, or working installation URL has been supplied.
- The sprint ZIP now includes self-contained testing, installation, cross-platform, scenario, and per-platform traceability guides. Windows, macOS, and Linux acceptance remain separate so a Windows release is not presented as all-platform completion.
- Preserve the code-first PR context layout, sidebar badges, PAT path, retained offline cache and Outbox, incremental restart recovery, direct merges, and exclusions of issue/PR creation and GitHub merge queues.

## 1 Product objective

Build a fast cross-platform native desktop application for working with GitHub.com repositories protected by organizational single sign-on. Prioritize Windows for the first release and deliver macOS and Linux in later release phases. The application must make repeated visits to issues, pull requests, comments, reviews, commits, and diffs substantially faster by rendering downloaded content from local storage while synchronization runs independently.

Users choose the repositories they want to access. All accessible open, closed, and merged issues and pull requests remain discoverable regardless of age. Download content when it is needed, retain it locally, and update relevant content according to configurable work-hours and off-hours schedules. A complete historical download is not required before the app becomes useful. The app must remain fully usable offline for downloaded content and local work. Submitted actions are saved to a durable, inspectable, cancellable outbox and automatically attempted when connectivity and their prerequisites permit. A persistent green/red connection indicator with text shows GitHub reachability separately from authorization or service problems.

The first release authenticates with a personal access token and includes editing existing issues, comments, PR review submission, approvals, requests for changes, ordinary permitted direct merges, native diff viewing, a branch and commit graph, and configurable Windows notifications with suppression of automated review activity. A tabbed viewing workspace lets users keep multiple issues and PRs open at once and arrange them in resizable side-by-side panes. Within each PR review tab, code receives most of the space beside one smaller context pane that switches between the PR body and a linked issue. Issue creation is outside the current release.

The application must run without Electron, an embedded browser, or a webview. Rust is the preferred implementation language. The proposed stack is Rust, GPUI, GPUI Kit, SQLite, SQLx, Tokio, and direct GitHub REST and GraphQL integration.

## 2 Instructions for the implementing LLM

Treat this specification as the implementation contract. MUST identifies a release requirement. SHOULD identifies a recommended behavior that may change for a documented technical reason. Proposed defaults are decisions to use until the owner changes them; they are not claims that the owner explicitly selected every implementation detail.

Inspect the existing repository and its instructions before changing code. If no application exists, create a runnable Rust project. Implement working vertical slices in the sprint order in Section 18, preserve unrelated work, and keep the application buildable. Use current primary documentation to verify dependency compatibility and GitHub endpoint behavior before relying on a specific version or capability.

Do not replace the native interface with a website, Electron application, Tauri application, or hidden webview. Do not stop at screenshots, mocked buttons, a plan, or a fixture-only demonstration. Fixtures are required for development and testing, but the delivered application must support live GitHub integration.

Separate domain behavior from GUI code. Keep networking, large diff parsing, database work, and expensive graph layout off the UI thread. Implement persistent storage and resource completeness before relying on cache-first screens. Use explicit states for pending writes, partial downloads, and unavailable resources. The offline outbox contract remains required: explicit action submission authorizes automatic outbox execution after reconnection, while merely saving or reopening a draft never submits it. Version 1.7 requires cross-platform architecture, staged operating-system releases, explicit testing layers, and a simple published installation path. It preserves the bound PR context/code layout, sidebar counts, and exclusion of issue creation. Preserve confirmed PAT authentication, exclusion of GitHub merge-queue flows, and local session retention with incremental restart catch-up.

Proceed with the documented defaults instead of repeatedly asking about routine implementation choices. Ask a focused question only when missing information materially changes compatibility, organization authorization, data handling, or a requested feature. Never invent credentials, bypass SSO or organization policy, or perform test writes against an unrelated real repository. Integration tests that edit existing issues, create comments or reviews, or merge PRs require an explicitly designated disposable test repository.

For each sprint, report the requirement IDs implemented or partially implemented, commands and tests actually run, measured results, remaining limitations, and the next sprint. Keep primary implementation ownership separate from final verification for requirements spanning sprints, using the companion traceability files. Keep a requirement checklist and short architecture decision record in the repository. Record unit, integration, native-process end-to-end, installation, and live-provider evidence separately for the relevant sprint. Do not claim that a native integration or performance target passed on one operating system because it was only inspected on another. Implement the testing commands in TEST-01; these development commands are not an end-user installation prerequisite.

For a one-sprint assignment, complete that sprint and report its result. A Windows-release assignment covers Sprints 01-12; an all-platform assignment also covers Sprints 13-14. Continue through the authorized phase without routine reapproval unless genuinely blocked. Later macOS/Linux release gates stay pending until their phases and must not be presented as either completed Windows work or a reason to delay an otherwise complete Windows phase. If an integration is blocked, complete the independent work, preserve a useful runnable state, and identify the exact missing prerequisite. Do not silently remove features or relax the no-browser constraint.

## 3 Confirmed scope and implementation defaults

### Confirmed requirements

| Area | Requirement |
| --- | --- |
| Platform | Cross-platform native desktop. Windows is the first and highest-priority release; macOS and Linux follow as required later milestones. |
| Rendering | No Electron, embedded browser, or webview engine. |
| Language | Rust preferred. |
| Service | GitHub.com with organizational SSO. |
| Authentication | Personal access tokens are permitted and are the first-release login path. |
| Repository coverage | Repositories explicitly selected by the user. |
| Historical coverage | All accessible issues and PRs, including closed and merged history. |
| Download behavior | On demand; no mandatory full initial archive. |
| Persistence | Retain local content, drafts, workspace/view state, settings, outbox, and synchronization progress on the PC; normal restart resumes them. |
| Sign-out | Remove local credentials and hold that identity's remote work while its local content remains readable. |
| Offline operation | Cached reading and local work remain usable; submitted actions queue durably, resume automatically, and can be inspected or cancelled while pending. |
| Connection status | Persistent green/red indicator with an accessible text label, last-check time, and separate access/service status. |
| Refresh | User-configurable work-hours and off-hours schedules. |
| Writing | Comments, editing existing issues, PR reviews, approvals, and ordinary permitted merges. New issue creation is excluded. |
| GitHub merge queues | No enqueue, management, or withdrawal flows in the first release. The local action Outbox remains required. |
| Review interface | Native diffs and branch or commit visualization. PR review shows code prominently beside one context pane toggling View PR body and View issue. |
| Viewing workspace | Multiple issues and PRs open concurrently, with tabs and side-by-side viewing panes. |
| Notifications | Native pop-ups on each released OS, with Windows first, selectable events and automated-review suppression. |
| Sidebar totals | Review requested, Assigned to me, and Outbox use dark blue circular badges with white digits for positive totals; zero hides the badge and number. |
| Testing | Practical unit, integration, native-process end-to-end, and install/upgrade tests, with reproducible commands and evidence per sprint and platform. |
| Installation | One to three documented commands per published platform, using prebuilt binaries without Rust build tools and preserving local data on rerun/update. |

### Proposed defaults

| Decision | Default until changed |
| --- | --- |
| Native UI | GPUI with GPUI Kit Rust components. |
| Workspace layout | One window with resizable item panes and a separate item-tab strip in each. A PR review tab contains one coordinated context/code pair, initially about 33% context and 67% code when width permits. Begin a new workspace with one item pane; restore saved layout later. |
| Local data | SQLite through SQLx, WAL mode, and FTS5 search. |
| Issue and PR storage | One common items table using the shared repository number, plus a one-to-one PR details table. Preserve distinct API object IDs. |
| Reference platforms | Windows 11 x64 first; macOS 15 or later on Apple Silicon for the macOS phase; Ubuntu 24.04 x64 with a documented supported desktop/session and graphics configuration for the Linux phase. These version/architecture defaults require verification before publication. |
| Account model | One active GitHub identity initially; storage and credential profiles remain account-aware. |
| PAT choice | Prefer a fine-grained PAT when required endpoints and organization policy support it; use an approved classic PAT when needed. |
| Git history | Remote GitHub branches and commits, loaded through APIs initially. |
| Background operation | Optional native tray/menu-bar operation where supported; startup with the operating system is opt-in. Preserve explicit Quit and offline/pause behavior. |
| Offline behavior | Read and search downloaded content, prepare all supported work, and queue submitted actions for automatic execution after reconnection. Unsubmitted drafts remain drafts. |
| Hosting | No hosted server required for the first release. |
| Retention | Keep fetched text and diffs until the user clears them; expose storage usage. |
| Delivery model | Sprints 01-12 produce the complete Windows first release; Sprint 13 macOS and Sprint 14 Linux provide product parity with platform-specific integrations and evidence. |

The exact OS versions, CPU architectures, Linux desktop/session matrix, fine-grained PAT preference, and single-active-account choice are implementation defaults. Windows priority and later macOS/Linux releases are confirmed. GitHub.com, SSO, PAT authentication, retained local data, and exclusion of GitHub merge-queue flows are confirmed decisions.

### Boundaries for the first release

The release focuses on GitHub issues, PRs, reviews, history visualization, and activity monitoring. Local staging, committing, rebasing, conflict editing, and general working-tree management are later features. Creating new issues, issue-creation forms/templates, creating PRs from local code, full GitHub Projects parity, CI log or artifact archiving, and attachment uploads are also later features unless separately requested.

GitHub merge-queue enqueue, management, and withdrawal are outside the first release. Direct merge is the baseline; GitHub auto-merge and OAuth/device authorization are optional future capabilities and must not be introduced by assuming that distribution requires them.

Existing images and downloadable attachments referenced by discussions should open or download on demand. The interface must state when a resource is unavailable or not downloaded. A full repository clone, complete source checkout, and archive of every historical PR revision are not prerequisites.

## 4 Architecture and technology

### Native application foundation

**FND-01** The shipped application MUST have a native interface and no runtime dependency on a browser engine. Rust with GPUI is the implementation default. A different native approach requires a documented concrete feasibility reason and must preserve the no-browser constraint. An explicit sign-in or external-link action may open the user's existing default browser; this does not embed a browser in the application.

**FND-02** Use GPUI and GPUI Kit as the initial GUI choice. Validate selectable rich text, a large annotated diff, two simultaneously visible panes with independent tabs and focus, a PR review fixture with a smaller switchable context surface beside prominent code, Windows notifications, tray behavior, and click activation before building the entire UI. Pin a tested compatible dependency set and commit the lockfile. GPUI Kit provides native text and diff primitives, but GitHub workflow integration remains application work. [S01] [S02] [S03]

**FND-03** Use SQLite with migrations, foreign keys, suitable indexes, and bounded connections. WAL allows reads during writes; FTS5 supports local full-text search. Keep transactions short and perform database work outside the UI thread. [S04] [S05]

**FND-04** Use Tokio for asynchronous application work and reqwest for HTTP. Use typed request and response models, explicit cancellation, structured errors, and a centralized request scheduler. A different library is acceptable only with a recorded compatibility reason.

**FND-05** Use direct GitHub APIs. Do not scrape rendered GitHub pages or depend on a user's browser session. Keep REST and GraphQL behind a provider interface with explicit capabilities and version handling.

### Module boundaries

These are logical modules. They may begin in one application crate with a reusable core library; separate Cargo crates only when the boundaries are useful.

| Module | Responsibility |
| --- | --- |
| domain | Stable models, validation, permissions, and application rules. |
| storage | SQLite migrations, transactions, drafts, download progress, and journals. |
| queries | Local views, filtering, pagination, search, and cache coverage. |
| github | REST, GraphQL, pagination, capability checks, and remote errors. |
| auth | Identity, credential profiles, SSO state, and credential-store access. |
| sync | Durable jobs, priorities, schedules, retries, and reconciliation. |
| commands | Issue, comment, review, and merge intents; durable outbox, dependency ordering, cancellation, replay, and reconciliation. |
| connectivity | Shared API reachability state, freshness, reconnect signals, and rate-aware probes. |
| review | Comparison identity, patches, source locations, and draft reviews. |
| history | Commits, ordered parents, branch refs, and graph layout. |
| notifications | Activity observation, filtering, grouping, and delivery decisions. |
| platform | Small typed interfaces for protected credentials, notifications/activation, application-data paths, lifecycle/startup, system timezone/theme, and other required desktop capabilities. |
| platform_windows | Windows implementations and packaging, delivered first. |
| platform_macos | macOS implementations and packaging, delivered in Sprint 13. |
| platform_linux | Linux implementations and packaging for the declared supported environments, delivered in Sprint 14. |
| workspace | Item-pane layout, item-tab identity, bound PR context/code layout, focus, per-tab view state, session restore, and draft-editor ownership. |
| ui | GPUI views, keyboard interaction, presentation state, and navigation. |

**FND-06** UI events MUST enqueue or invoke application operations without performing blocking work. Results MUST carry resource identity and revision. View-specific work must also retain a tab-instance identity and selection generation so a late response cannot replace another tab's content or a newer selection. Shared resource results may update the cache and notify all matching views without changing their selection or focus.

**FND-07** The cache is the normal read source. Successful reads and writes update persistent records, then notify subscribed views. Preserve scroll position, selected file, and unfinished input when background refresh completes.

### Cross platform architecture and release phases

**PORT-01** Windows is the highest-priority first release, completed through Sprints 01-12. macOS in Sprint 13 and Linux in Sprint 14 are required later product milestones, not optional replacements for Windows. Publish an explicit support matrix covering OS version, CPU architecture, packaging and, for Linux, supported distribution/desktop/session/graphics prerequisites. Use the provisional targets in Section 3 until validated. Keep Windows completion distinct from later-platform readiness; no supported-platform claim is earned by compiling alone, and no later-platform release gate silently moves into the Windows phase.

**PORT-02** Share domain models, GitHub integration, cache/search, synchronization, Outbox, review/diff logic, history, notification rules, and workspace state across platforms. Isolate operating-system APIs behind small capability interfaces with conditional dependencies inside platform implementations. Common code must not depend on Windows handles, registry calls, COM types, fixed Windows paths, or platform-specific credential objects. Reuse portable GPUI facilities where suitable rather than wrapping the entire toolkit in a new framework. Test interface contracts with fakes and actual adapters; do not duplicate the product's business logic for each OS.

**PORT-03** Prove the actual native renderer and necessary GPUI/GPUI Kit components early with selectable Markdown, text input/copy, large annotated diffs, the bound PR context/code layout, resizing, focus, and a graph fixture. Windows proof is a Sprint 01 gate. Configure compilation/core-test jobs for all intended targets and attempt macOS/Linux native smoke launches early on real target desktops; record gaps honestly and resolve them before that platform's release. In-process/headless tests or a successful cross-compile are not native GUI proof. No phase may add an embedded browser engine as a portability shortcut. [S31] [S32] [S33]

**PORT-04** Implement protected credential and application-data adapters per published OS. Use Windows Credential Manager or equivalent on Windows, the platform keychain on macOS, and a documented supported secure credential service on Linux. Store only credential references in SQLite; never fall back silently to plaintext tokens when a store is unavailable or locked. Preserve offline cached access while affected network work is held. Use appropriate per-user data/config/cache locations, stable app identity and relative managed asset paths rather than Windows-specific absolute paths. Test filesystem permissions, file locking, SQLite WAL/FTS availability, path/Unicode handling, and stable location across upgrades. Credentials and caches are per-device; this does not add cross-device synchronization. [S36]

**PORT-05** Keep notification eligibility, filtering, grouping, delivery history and item routing shared, with per-OS notification/activation implementations. Verify permissions, native delivery, actions, cold/warm launch, single-instance routing, and any supported identity/replacement semantics in the packaged target app. Unsupported OS capabilities must have truthful capability reporting and usable in-app alternatives for actions such as Snooze and Mark read; no silent loss of activity or false replacement/retraction promise. Define supported desktop behavior explicitly before publishing, preserving the persistent feed and correct Open behavior. A toolkit method or Windows test does not prove another OS's integration. [S34]

**PORT-06** Implement and test platform-appropriate keyboard shortcuts, text input, accessibility, clipboard, high-DPI/scaling, theme changes, timezone defaults, external-link opening, startup registration, close versus Quit, background residence, sleep/wake and network-change hints. Feed normalized lifecycle events into the shared scheduler. Restore offline/pause settings before probes or dispatch. If a supported environment lacks a tray/menu-bar capability, keep the app reachable and explain background behavior rather than hiding an inaccessible running instance. Closing/quit must preserve drafts, workspace, Outbox and credentials according to the shared lifecycle contract. [S33] [S35]

**PORT-07** Sprint 13 MUST deliver a tested macOS package with the same applicable product workflows, data model, cache/offline behavior, PR context layout, writes, Outbox safeguards, graph and notification rules as the Windows release. Complete macOS secure storage, native interaction/accessibility, menu-bar/lifecycle and notification/activation behavior, packaging trust requirements, one-to-three-command installation/update, and native-process end-to-end/performance/upgrade evidence. Keep unsupported architectures or OS versions explicit. Windows completion remains valid while this milestone is pending; macOS completion requires its own evidence.

**PORT-08** Sprint 14 MUST deliver a tested Linux package for the declared support matrix with the same applicable product workflows and correctness safeguards. Verify the selected credential service, application-data paths, graphics renderer, X11/Wayland sessions claimed as supported, input/accessibility, notification/activation and background behavior. Bundle application dependencies where appropriate and give precise prerequisite/unsupported-environment guidance where system components are required. Complete one-to-three-command installation/update and native-process end-to-end/performance/upgrade evidence on each claimed configuration. Do not label untested distributions, sessions or architectures supported or require a repository clone/source build for users.

## 5 Authentication and GitHub organization access

**AUTH-01** The application MUST support GitHub.com accounts accessing SSO-protected organization repositories. Use `https://api.github.com` for REST, `https://api.github.com/graphql` for GraphQL, and `https://github.com` as the authorization web origin. Validate credential routing and any authorized redirects. GitHub Enterprise Server compatibility is not a first-release requirement.

**AUTH-02** Authenticate as the user for issue changes, reviews, approvals, and merges. Show the active identity and relevant organization or credential profile. Do not submit the user's reviews as an installation bot identity.

**AUTH-03** Use a personal access token as the confirmed first-release authentication path. Prefer a fine-grained PAT when all required endpoints and organization policy support it; support an organization-approved classic PAT when endpoint coverage or organization compatibility requires that alternative. Do not switch to OAuth or device authorization as an unrequested first-release dependency. A fine-grained token can be limited to one resource owner and selected repositories, so credential profiles must route appropriate PATs for different selected owners without sending an unrelated token. [S20]

**AUTH-04** For organizations using SAML SSO, account for the distinction between token creation, SSO authorization, and organization approval. Fine-grained tokens receive SSO authorization during creation; classic tokens require separate SSO authorization. Fine-grained tokens may also require an organization owner's approval before private resources are accessible. [S21] [S22]

**AUTH-05** Handle expired or revoked tokens, insufficient permissions, pending approval when identifiable, organization restrictions, and required SSO authorization as distinct actionable states. Interpret `X-GitHub-SSO` when present, including partial results. A 403 or 404 caused by authorization MUST NOT be treated automatically as resource deletion. [S21]

**AUTH-06** Provide a user-initiated action to open an appropriate GitHub PAT or SSO authorization page in the external default browser. Validate returned authorization URLs before opening them. Preserve the current screen, cached content, drafts, outbox, and synchronization checkpoints while authorization is repaired. After verifying that a replacement PAT belongs to the same GitHub user and restores the required access, reuse the existing account/resource identities and progress, retry eligible reads, and automatically resume eligible submitted actions under the persisted pause/offline settings. A different user must receive a separate account context; replacing a token must not create a new cache for the same user.

**AUTH-07** Store PATs through the platform credential adapter in Windows Credential Manager or an equivalent native protected store; published macOS/Linux builds follow PORT-04. Persist only credential references in SQLite. Normal window close or application exit retains the protected token so the next launch can resume without re-entry while it remains valid. Explicit sign-out follows AUTH-11 and clears the relevant local credential material. Never include tokens in logs, URLs, diagnostics, exports, or source control.

**AUTH-08** Request and document the permissions actually required by the chosen endpoints. Issue writes, PR review writes, commit access, and merging must each be validated against current endpoint documentation. Check access to an explicitly selected private repository, not merely whether `/user` succeeds. Preserve readable functions when a write permission is missing. [S06] [S09] [S10]

**AUTH-09** Custom activity notifications MUST work independently of GitHub's own notifications inbox. Do not require a classic token solely to show the app's custom pop-ups. Synchronizing GitHub notification read/unread state is optional and has separate token compatibility constraints. [S14]

**AUTH-10** OAuth or device authorization is optional future scope and is inactive for the confirmed PAT-based first release. Retain a provider boundary that permits a separately authorized future registered GitHub OAuth or GitHub App integration. If that scope is explicitly activated later, verify its SSO, organization approval, and user-identity behavior and keep login secrets out of the distributed binary. Distribution alone does not activate this requirement. [S23]

**AUTH-11** Explicit sign-out MUST durably mark the affected account profile as signed out, prevent new network dispatch under that profile, and remove or invalidate its locally stored PAT material. Preserve readable private cached content, drafts, saved workspace, settings, synchronization checkpoints, activity baselines, and outbox records on the PC. Users may reopen that signed-out local profile for offline work without a renewed login; show its signed-out status and hold its remote actions until a PAT for the same verified user restores access. Keep that hold across restart. Ordinary exit is not sign-out. Expired or revoked PATs likewise preserve offline use and progress while holding affected remote work. A sign-out during an in-flight mutation does not prove remote cancellation; retain its evidence for reconciliation after access is restored. Do not expose one profile's data through a different active account or assume sign-out revokes the PAT at GitHub.

## 6 Repository browsing workspace and search

**BROWSE-01** Users MUST be able to select, remove, and revisit accessible repositories. Show organization, repository name, access state, last successful check, and cached storage usage. Removing a monitored repository stops its polling; deleting its cached data is a separate explicit action.

**BROWSE-02** Repository lists MUST support issues and PRs in all states, with no hidden historical date cutoff. Fetch pages as users browse. Cache returned summaries immediately, and provide clear loading, empty, partial, offline, and error states. Identify PR records returned by issue endpoints, upsert them into the common item store, and apply the requested issue-only, PR-only, or combined view without duplicating an item. [S28]

**BROWSE-03** Provide saved views for authored items, assignments, mentions or participation where supported, and requested reviews. Provide repository, state, author, label, assignee, reviewer, and date filters when supported by the selected query mode. Distinguish filtering downloaded data from querying GitHub.

**BROWSE-04** Support recent items, pinned items, back and forward navigation, and opening an accessible GitHub issue or PR URL through the workspace rules below. Keyboard navigation must reach repository lists, item lists, pane controls, item tabs, PR subviews, search, and the review editor.

**BROWSE-05** Review requested, Assigned to me, and Outbox MUST show a positive total in a right-aligned dark blue filled circular badge with centered white digits. Use a suitable accessible color, provisionally #173E70, and preserve a true circle, label clearance, and legibility for multi-digit totals and high-DPI scaling. A known zero draws neither a circle nor a number. Counts are totals for the current account and each view's stated repository/filter scope, not unread-notification counts or just the visible page length. Count distinct matching items for saved views. Use a trustworthy complete-query total when available; otherwise label the cached/known count and incomplete coverage, without presenting unknown or partial coverage as a confirmed empty result. Outbox counts all unresolved submitted actions, including paused, blocked, and outcome-unknown actions, and excludes drafts, download jobs, and confirmed/cancelled history. Refresh counts from shared state after ingestion, edits, scope changes, enqueue, cancellation, and confirmation without per-pane polling. Accessible names and count details must expose scope, freshness, and any coverage limitation.

### Tabbed workspace and side by side viewing

A workspace is the collection of viewing panes in the application window. Each pane has an item-tab strip, and each item tab displays one issue or PR. PR sections such as Conversation, Commits, and Files are subviews inside an item tab. They must remain visually distinct from the tabs used to switch between different issues and PRs.

General item panes own their own item-tab strips. The two coordinated context/code surfaces inside a PR review tab belong to that single active PR; they are not two independently targeted item panes and do not require a third always-visible issue pane. Preserve general standalone issue navigation, multiple PR tabs, duplicate views, and Open beside. PR-05 through PR-07 define the default review arrangement.

**WORKSPACE-01** The application MUST support multiple open issue and PR tabs and at least two simultaneously visible side-by-side panes in one window. Users must be able to mix issues and PRs from the same or different selected repositories. Allow additional side-by-side panes while usable minimum widths can be maintained. A new workspace starts with one pane; a split action adds another without closing the existing content.

**WORKSPACE-02** Each pane MUST maintain its own ordered item tabs and active tab. Show the item type, repository, item number, a readable title, and relevant draft or loading indicators through the tab label and accessible details. Provide tab overflow navigation and access to the full label when space is limited. Opening another item must not silently replace an existing tab. Provide explicit commands to open in a new tab, open beside the current pane, close a tab, and reopen the most recently closed tab.

**WORKSPACE-03** Ordinary item navigation reuses an existing matching tab in the active pane or adds a tab there. Open beside must add or focus a tab in an adjacent pane without replacing its other tabs. Provide an explicit duplicate-view action to show the same issue or PR in different panes, for example a PR conversation beside its Files subview. If several tabs in the target pane match, reuse its active matching tab or otherwise its most recently active match.

**WORKSPACE-04** Users MUST be able to resize panes, reorder tabs, and move tabs between panes. Provide visible or keyboard-accessible commands as well as any drag gestures. Show the focused pane clearly. A focus or maximize action MUST enlarge one pane temporarily and restore the previous layout. On a narrow window, preserve all tabs and drafts and offer a usable focused-pane layout instead of shrinking content beyond its minimum width.

**WORKSPACE-05** Persist view state independently for every tab instance: selected subview, selected file or commit, diff display mode, scroll anchors, expanded discussion or diff regions, and relevant navigation history. For a PR review tab also retain its context mode, selected verified linked issue, context/code proportions, collapsed/focused surface, and separate context scroll anchors. Switching tabs, resizing panes, or refreshing one resource MUST NOT reset another tab's position, selection, or focus. Anchor diff state to its comparison revision and handle unavailable anchors after a changed revision explicitly.

**WORKSPACE-06** Bind comments, issue edits, review submission, approvals, and merges to the originating tab's account, repository, item, and relevant revision. Changing focus while an operation is pending MUST NOT retarget it. Global navigation shortcuts apply to the focused pane; controls inside a pane apply to that pane's displayed item. Show the target repository and item in write confirmations where needed to avoid ambiguous actions.

**WORKSPACE-07** Multiple views of the same resource MUST share cached domain data, operation outcomes, and persistent draft identities. Reopening the same compose or review draft must reuse its draft ID. Use one active editor per draft by default; other views show the current draft preview and a Focus editor action. Do not allow independent unsynchronized copies to overwrite the same draft or submit the same pending operation twice. Separate intentional drafts remain separate records.

**WORKSPACE-08** Save and restore the workspace per account, including pane order and proportions, open tab order, active tabs, focused pane, per-tab view state, scroll position or anchors, navigation context, and draft references. Normal exit MUST retain this state. On restart, render cached visible content before waiting for any API response or renewed login; then start eligible background work through the shared scheduler. Load inactive tabs lazily and expose missing or inaccessible content without discarding its navigation target. Restoring a workspace MUST NOT submit unsubmitted drafts, create duplicate operations, or expose another account's private tabs. The independent outbox worker resumes already submitted eligible actions under OUTBOX-04 and the restored authentication/pause settings; view restoration is not a dispatch trigger.

**WORKSPACE-09** Closing a tab or pane MUST preserve its saved drafts, cached content, and already submitted operation tracking. Persist editor changes before closing; if that fails, keep the editor recoverable and explain the failure. Closing a pane closes its tab views and focuses a remaining pane; closing the final tab leaves a usable empty pane. Discarding a draft is a separate explicit action. Reopening a closed tab restores its saved view state when available.

**WORKSPACE-10** Share request scheduling and deduplicate in-flight fetches across views of the same resource and revision. Opening additional panes MUST NOT create independent polling loops or duplicate alerts for that resource. Give visible tabs priority within the existing schedule and rate-limit budget. Inactive tabs must not retain every large diff or continuously redraw; evict reconstructible rendering state while preserving navigation state and drafts. Closing one view must not cancel work still needed by another view or the shared write journal.

**WORKSPACE-11** Repository lists, search results, in-app links, and notification activation MUST route through the workspace's open-item behavior. For notification or external URL activation, prefer a matching tab in the focused pane, otherwise the most recently active matching tab elsewhere, otherwise a new tab in the focused pane. Apply WORKSPACE-03 when choosing among matches within a pane. Navigate only the chosen tab to the requested comment or diff location. Other panes retain their positions and unsent input.

**WORKSPACE-12** Provide documented keyboard commands for opening a new tab or side pane, cycling tabs within a pane, moving focus between panes, closing and reopening a tab, and moving a tab between panes. Expose tab and pane names, selection, and focus to the published operating system's accessibility features, with Windows tested first. Test these commands from real text editors without intercepting ordinary editing shortcuts unexpectedly.

### Local and remote search

**SEARCH-01** Provide fast local full-text search of downloaded titles, descriptions, and comments. Include available downloaded review text. Search results MUST identify the repository and item, highlight the match where practical, and navigate to the matching discussion or review location.

**SEARCH-02** Provide a separate Search GitHub action or clearly labeled remote result group to discover uncached items. Local results may appear immediately while a remote request runs. An empty local result means no matches in downloaded content, not no matches anywhere in the selected repositories.

**SEARCH-03** Handle unsupported remote query filters, pagination, API limits, and incomplete results explicitly. GitHub REST search can cap a query at 1,000 results and can return incomplete results. Do not use one search request as proof that every historical item was enumerated. Repository browsing must have its own paginated path. [S07] [S08]

**SEARCH-04** Offline search MUST remain usable and clearly indicate its downloaded-content scope. Opening an uncached remote result while offline must explain which content is missing and preserve the navigation target.

## 7 Persistent cache and demand loading

**CACHE-01** Opening downloaded content MUST render it immediately without waiting for GitHub. Refresh stale resources independently. The first visit to uncached content may require network access and must expose useful incremental progress.

**CACHE-02** Fetch resource groups independently: metadata, ordinary comments, timeline events, reviews, review threads, commits, changed-file lists, patches, source context, and media. Loading a PR's Conversation tab must not wait for every patch or image.

**CACHE-03** Persist completeness separately from freshness. Resource state must distinguish missing, downloading, partial, complete, unavailable, and failed. Freshness records include last successful fetch, last validation, validators, and applicable revision. A complete but stale resource remains readable.

**CACHE-04** Only show an empty-content message when the relevant resource is known to be complete and empty. A cached issue summary does not establish that its comments, timeline, or attachments have been downloaded.

**CACHE-05** Commit downloaded pages and their progress atomically. Retry and resume reads without duplicating records. Persist enough progress to resume interrupted pagination. If a cursor or page boundary becomes unreliable, safely re-enumerate the affected range using stable IDs and deduplication. [S07]

**CACHE-06** Retain fetched text, review data, commit metadata, and diffs across restarts. Do not impose an undocumented age cutoff. Bound memory and render state independently of disk retention. Expose per-repository usage and explicit cleanup controls.

**CACHE-07** Existing-issue edit drafts, comment drafts, review drafts, and submitted outbox payloads are user data. Cache clearing, repair, failed downloads, migrations, reconciliation, and closing tabs or panes MUST preserve them unless the user explicitly deletes the drafts or cancels the actions. Drafts and outbox entries exist independently of any viewing tab. Preserve source anchors, edit base values, dependency mappings, and the minimal referenced content needed by unresolved actions; apply OUTBOX-10 to cleanup and recovery.

**CACHE-08** Namespace records by host and account access context, with stable repository and resource identities. Issues and PRs share one repository-local number namespace. Map their API representations to a common local item while preserving distinct issue and pull-request API object IDs. Follow DATA-05 through DATA-09 for keys and ingestion. Never expose another account's private cached content after switching identities. [S27] [S28]

**CACHE-09** Associate comparison data with immutable commit identities and comparison options. Associate mutable branch labels, status checks, and review state with their relevant revisions and timestamps. A branch name alone is not an adequate diff cache key.

**CACHE-10** A 404, authentication failure, or temporary API error MUST NOT trigger bulk cache deletion. Record the reason when known, stop inappropriate retries, and reconcile after access is restored. Distinguish an unknown resource from a confirmed deletion.

**CACHE-11** Existing images and accessible attachments SHOULD be fetched on demand, stored with cache metadata, and reused. Source code outside a patch is a separate resource. Show explicit placeholders for missing, inaccessible, binary, or unsupported content.

**CACHE-12** All history means all currently accessible historical items and their available data. It does not promise reconstruction of deleted comments or every overwritten PR revision. Optional future archival capture must be a separately defined feature.

**CACHE-13** Normal window close and application exit MUST retain downloaded content and resource coverage, all draft types, saved tabs/layout/view positions, local preferences, offline pins, pending actions and receipts, synchronization checkpoints/validators, and notification baselines/delivery history on the PC. Persist settings and user changes as they occur and safely flush remaining local state on orderly exit; if a user-data write fails, keep it recoverable and report the failure. Retain the protected credential according to AUTH-07. Restart MUST reuse these records and render the saved local workspace before networking completes. It must not clear the cache, reset baselines, or redownload all history merely because the process was restarted. Sign-out preserves local content under AUTH-11. Explicit cache cleanup remains a separate action and continues to protect drafts and unresolved work under CACHE-07 and OUTBOX-10.

### Full offline operation

**OFFLINE-01** Once an account and content have been established, the application MUST start and remain usable without a network check or renewed login blocking local access. Users can browse/search downloaded issues, PRs, discussions, reviews, commits, diffs, graph history, and the activity feed; use tabs and panes; edit local settings and drafts; and inspect or cancel pending work. State the cached coverage and last fetch honestly. Unseen remote content and new server events cannot be available until fetched. First-time authentication and initial acquisition of private content still require access; do not require a full repository download.

**OFFLINE-02** Offline mode MUST allow preparation and explicit submission of every implemented remote mutation: existing-issue editing/state/metadata, ordinary comments and edits, review comments/replies/submission/approval/request changes, permitted thread actions, and merges. Keep their controls available when cached target data and capability information are sufficient, and queue intent locally. Unknown permissions or missing required target/source details are explicit prerequisites; never invent a target, bypass a known denial, or silently choose a newer revision. Local settings, drafts, viewed-file markers, and app-feed read state apply locally immediately.

**OFFLINE-03** Provide Download for offline use for a selected item, with the resource scope and revision shown before downloading. Include its accessible discussion and, for a selected PR revision, review/thread data, commit/file metadata and available patches; let users choose optional media or additional source context. Track progress and per-resource completion, expose caps or unavailable content, and resume interrupted downloads. An offline request to fetch missing data becomes a visible, cancellable pending download that starts when eligible. Download jobs are distinct from remote mutation actions. Never present partially fetched data as fully available or start a mandatory full-repository archive.

**OFFLINE-04** Show submitted local work immediately as clearly pending in all matching views: for example a pending comment, a review action, or an edit to an existing issue awaiting confirmation. Keep pending projections separate from confirmed cached server records; a refresh cannot erase them, and cancellation removes only the pending projection while preserving recoverable user text. A pending merge or approval is not a confirmed merge or approval. Offline pins and minimal draft/outbox dependencies must survive ordinary cache cleanup until explicitly released or the action is resolved.

## 8 Synchronization and scheduling

**SYNC-01** Users MUST configure workdays, work windows, timezone, work-hours interval, off-hours interval, and pause behavior. Support daylight-saving changes through timezone-aware scheduling. Default to the operating system timezone through the platform adapter and allow an explicit override.

**SYNC-02** Use these provisional defaults: Monday through Friday, 09:00 to 17:00; work-hours checks every 120 seconds; off-hours checks every 30 minutes; stale items in the selected tab of each visible pane eligible for a 60-second check. Expose these values in settings. Treat them as requested intervals subject to API constraints and actual workload. Multiple views of one item share its refresh schedule.

**SYNC-03** Give priority to explicit user actions and their eligible submitted outbox work, then the selected item in each visible pane, enabled activity monitoring, recently used or pinned content, and slower reconciliation. Apply a shared budget and fair scheduling so a busy pane or outbox cannot starve other work. Fetch older history on demand. Do not launch a mandatory full historical backfill. Keep dependency order and mutation pacing even when draining the outbox.

**SYNC-04** A user-triggered open or refresh may run outside work hours. Explicitly submitted outbox actions also resume outside work hours when eligible; quiet hours control notification delivery and read-refresh schedules control background reads, not authorization to send queued actions. Provide a separately labeled Pause queued actions control, and distinguish it from Pause background refresh and Work offline. Persist these settings and restore them before any startup network work. Work offline blocks all networking until disabled; Pause queued actions blocks new mutation dispatch, and Pause background refresh blocks scheduled catch-up reads. After sleep or reconnection, coalesce overdue read jobs into a bounded catch-up pass and resume eligible actions without replaying every missed timer tick or sending a mutation burst. Required outbox preflight/reconciliation reads remain governed by the action and connectivity controls, not the background-read schedule.

**SYNC-05** Centralize pagination, cancellation, request deduplication, retry limits, exponential backoff, and server budget handling. Honor relevant `Retry-After`, rate-limit, and polling headers. Authenticated conditional requests that return 304 can preserve the primary REST quota, but do not make polling unlimited. [S11] [S12]

**SYNC-06** Maintain independent synchronization progress for resource families. Use repository-level updated-comment feeds where available, but do not assume the parent issue or PR timestamp is a complete change feed for every child object. Use overlapping update windows and periodic reconciliation where required. [S06] [S13]

**SYNC-07** Include all states when checking for relevant updates. A new comment on an old closed issue or merged PR can be relevant. Do not permanently freeze previously downloaded historical items merely because they are closed.

**SYNC-08** Expose last successful synchronization, pending work, throttling, authentication problems, and resumable failures. Background failures must not replace usable cached screens with blocking error pages.

**SYNC-09** Run synchronization, connectivity checks, and the outbox worker in the application or optional tray process. A fully exited application or sleeping machine cannot perform local polling or send actions. Normal exit preserves local state and the protected PAT; it does not sign the user out. On launch or wake, restore the local workspace and persisted control settings first, then resume eligible actions and bounded incremental read catch-up under SYNC-11 when connectivity and authorization permit. No hosted service is required for the initial release.

**SYNC-10** The highest downloaded issue or PR number MUST NOT be the sole synchronization cursor. Updates to old items and their comments, reviews, or state must still be discovered. Use supported update timestamps or cursors, overlapping windows, pagination, and resource-family reconciliation. Enumerate through supported list APIs instead of requesting every integer in a number range. Missing numbers do not establish deletion or complete cache coverage. [S28]

**SYNC-11** Resume from persisted per-account, per-repository, per-resource-family synchronization state on restart, reconnection, and same-user PAT renewal. Retain the last successful checkpoint, overlapping update window or supported cursor, pagination progress, conditional-request validators, resource completeness/freshness, and notification baselines. Advance content and its successful progress transactionally after each completed unit; failures or partial pages must not skip unseen changes. Prioritize visible resources and run bounded, rate-aware incremental catch-up, using supported changed-since/update feeds, conditional reads, and targeted reconciliation. Do not blindly redownload the entire cache, re-enumerate every historical discussion, reset notification baselines, or use the largest issue/PR number as the only cursor. If a cursor expires or an endpoint has no reliable delta interface, re-enumerate the necessary resource/range with stable-ID deduplication and preserved coverage while keeping other completed families intact. An update-feed page one, overlap, unchanged revalidation, or targeted repair may be necessary; no universal exact-delta or zero-unchanged-bytes guarantee is made. Preserve old-item update discovery and honor restored sign-out, offline, and pause state. [S06] [S07] [S11] [S13]

### Connection indicator and recovery signals

**CONNECT-01** Show a persistent, prominent green/red connection indicator in the application chrome, visible across panes, with an accessible icon and text label; color alone is insufficient. Green means recently verified GitHub API reachability. Red means Offline or Cannot reach GitHub after a current transport failure or an explicit Work offline choice. Use a neutral Checking or Status stale state before an initial result and when evidence is too old, rather than asserting a connection indefinitely. Show last checked time, last successful contact, reason, and Check now in its details, plus a pending-action count linking to the Outbox.

**CONNECT-02** Derive reachability from verified HTTPS responses from the configured GitHub API and current transport errors, not from cached data or a network adapter icon alone. Successful responses, including 304, establish contact. A genuine API 401, authorization 403/404, 429, or 5xx also demonstrates a reachable API endpoint, while separate status explains Sign-in required, SSO approval, Throttled, or Service error and blocks affected writes as appropriate. DNS, connection, TLS, and timeout failures indicate inability to reach the service securely; they do not prove that every Internet destination is down. Request cancellation caused by tab navigation is not an offline signal. Suppress older/out-of-order observations so they cannot overwrite newer connectivity evidence; scope authorization errors separately by account/repository.

**CONNECT-03** Maintain one shared connectivity observer. Reuse ordinary API traffic; issue a lightweight non-mutating probe at startup, wake, network-change hints, or Check now when permitted. Use a provisional 120-second freshness window, then show stale evidence instead of increasing idle background polling beyond the configured refresh schedule. While submitted work is waiting on connectivity and the app/tray is active, probe automatically with bounded backoff and jitter, provisionally 5, 15, 30, 60, then at most every 120 seconds. Respect shared request budgets and longer server Retry-After/rate-limit delays. A new reachable result wakes the dispatcher once; many panes or queued items must not each probe or drain independently. Work offline disables probes and sends until explicitly turned off.

## 9 Issue reading and writing

**ISSUE-01** Display title, body, author, timestamps, state, labels, assignees, milestone, available reactions, linked items, and supported timeline events. Display the full accessible paginated conversation, with explicit partial-download states.

**ISSUE-02** Render Markdown natively, including headings, emphasis, links, lists, task lists, tables, quotations, code blocks, and images supported by the implementation. Text must be selectable and copyable. Use a readable source or unsupported-content fallback for features requiring additional rendering, such as complex embedded diagrams or HTML. Do not add a webview to improve parity.

**ISSUE-03** Edit existing issue titles, descriptions, labels, assignees, milestones, and open/closed state where authorized. Show available choices from the repository instead of inventing labels or identities. Bind edits to an already established issue identity and preserve its base values and draft; do not create a new issue as a fallback for a failed edit.

**ISSUE-04** Add comments and edit the user's editable comments. Provide a Markdown editor, native preview, and persistent drafts. On explicit submission, durably enqueue the action before attempting delivery, whether online or offline, and show the OUTBOX-02 lifecycle. Preserve text after errors or cancellation, and publish confirmed results to all views without duplicating a locally pending comment.

**ISSUE-05** Preserve the base values used to begin an edit. Immediately before dispatch, including delayed offline replay, refresh the edited fields and detect conflicting changes. Keep a conflict in the outbox as Needs attention while preserving local intent. Do not overwrite another author's changes silently. Do not assume the endpoint provides an atomic conditional-write guarantee unless it has been verified.

**ISSUE-06** New issue creation is excluded from the current release. Do not expose a New issue button, creation form/template workflow, shortcut, command, issue-creation draft type, create-issue outbox operation, or provider mutation flow. Existing-issue reading, comments, and authorized editing remain available. PR creation is also outside scope; this restriction does not remove comment creation, review submission, or their durable dependency identities.

## 10 Pull request viewing and review

**PR-01** Provide Conversation, Commits, Files, and Checks sections. Display author, source and target branches, draft status, open/closed/merged state, requested reviewers, review results, relevant timestamps, and linked issues.

**PR-02** Retrieve ordinary conversation comments, formal reviews, and inline review threads as separate resources. Preserve thread replies and available resolved or outdated status. GitHub exposes ordinary PR comments through issue-comment APIs and review comments separately. [S06] [S09] [S13] [S15]

**PR-03** The commit list must show commit identity, message, author, and available timestamps. The file list must show paths, status, additions, deletions, and renames where available. Identify missing or incomplete information.

**PR-04** Show checks and commit statuses associated with the relevant PR head. A cached result must show its freshness. Missing permissions or unavailable checks must appear as unknown or inaccessible, never as success.

**PR-05** The default PR review arrangement MUST show the native file list/diff prominently beside exactly one smaller context pane bound to the same active PR. At a usable window width, start with approximately 33% of the review content width for context and 67% for code, within a 30-35% / 65-70% range, excluding global navigation chrome. Context may be resized or collapsed; restore its last proportion. Provide View PR body and View issue controls in that one pane, with PR body as the initial mode for a newly opened review tab. They replace the context content rather than adding a simultaneous third pane. Preserve Conversation, Commits, Files, and Checks navigation and multiple PR tabs. At narrow widths, show a usable focused surface with an explicit way to switch context/code and restore the wider arrangement without losing either surface's state.

**PR-06** Derive linked-issue candidates from closing-style references in the current PR body. Support case-insensitive close/closes/closed, fix/fixes/fixed, and resolve/resolves/resolved with optional colon and a same-repository #number, qualified owner/repository#number, or GitHub.com issue URL. Parse native Markdown safely; ignore fenced/inline code examples and do not treat arbitrary numbers as references. Resolve local #number against the PR's repository, normalize and deduplicate targets, and verify repository identity, access, and ordinary issue type before displaying a candidate as an issue; a shared-number target can be a PR. Multiple verified targets require a picker, with a deterministic default from body order and a retained explicit selection. Cross-repository targets use the appropriate credential scope and load on demand without silently enabling repository-wide monitoring. Preserve reference provenance and body revision under DATA-10. A body reference is reading context, not proof that GitHub linked, will close, or already closed the issue: default-branch rules, permissions, and repository auto-close settings can differ. [S27] [S28] [S29] [S30]

**PR-07** View issue MUST display the selected verified issue's native body and available metadata/conversation in the context pane, with its repository, number, and coverage visible. View PR body restores that PR's body and context scroll state. Switching modes or linked issues must retain the code comparison, selected file, scroll/selection, review draft identity/anchors, and intended mutation target; background loads must not steal code-editor focus. Deliberate keyboard/pointer navigation may move focus normally. Use account, PR, body revision, selected issue, tab instance, and selection generation to discard stale context responses. Switching PR tabs must never show the previous PR's issue while loading. Distinguish no reference, verification pending, target is a PR, not downloaded/offline, inaccessible, failed, and removed or changed reference states; none is a confirmed empty issue. Cached PR bodies, linkage evidence, verified issues, and per-resource coverage remain usable offline with last-validation information. Preserve local drafts if body refresh removes a reference, and never silently attach them to a replacement issue.

**REVIEW-01** Users MUST create inline comments, replies, and a draft review, then submit a comment-only review, approval, or request for changes. Support resolving and reopening review threads where the API and permissions allow it. [S09] [S15]

**REVIEW-02** Persist each inline draft with repository, PR, file path, original or modified side, source line range, reviewed commit identity, and available context. Store source coordinates rather than visible row numbers.

**REVIEW-03** New commits and force-pushes MUST NOT silently move a draft or queued review onto different code. Preserve the original context and intended reviewed commit, indicate that the PR changed, and provide an explicit refresh or re-anchor workflow. Revalidate queued reviews, approvals, requests for changes, and thread actions before dispatch. A changed reviewed revision or invalid anchor requires attention instead of automatically approving different code. GitHub's review API accepts a commit identity; use the intended reviewed revision. [S09]

**REVIEW-04** Preserve unfinished reviews and submitted review actions across restart and connection loss. Offline users MUST be able to read cached review data, edit drafts, submit them to the local outbox, inspect pending actions, and cancel eligible entries. Actual GitHub submission and thread mutations execute automatically after connectivity returns and current permissions, source anchors, and revision checks pass. An unsent saved draft remains a draft.

## 11 Native diffs and review navigation

**DIFF-01** Provide unified and split views, a changed-file sidebar, syntax highlighting, additions and deletions, selection and copying, file collapsing, next/previous change navigation, and keyboard shortcuts. Reuse GPUI Kit components where compatible. [S02]

**DIFF-02** Provide inline thread display, comment-entry controls, unresolved-thread navigation, and persistent per-revision viewed-file markers. A file changed by a subsequent PR revision must not remain silently marked fully reviewed.

**DIFF-03** Distinguish PR comparison, individual-commit comparison, and arbitrary two-commit comparison. The default PR view must match the intended GitHub comparison. GitHub PRs use the merge base for a three-dot comparison; endpoint comparisons can differ. Label the comparison and use the correct source identities in the cache. [S16]

**DIFF-04** Parse large patches and prepare highlighting outside the UI thread. Virtualize long file lists and diff rows. Keep scroll and selection stable when annotations or background data change. Ignore stale background results for an earlier comparison.

**DIFF-05** Support an ignore-whitespace viewing option when backed by an accurate comparison. Preserve original source coordinates for review submission; do not derive comment positions from a visually filtered row index.

**DIFF-06** Distinguish a patch from complete source content. Expanding outside supplied context may require another fetch. Cache fetched file versions separately and preserve the current review view while additional context loads.

**DIFF-07** Detect API caps, missing pages, truncated patches, binary changes, unavailable objects, and unsupported formats. The PR-files endpoint has a documented 3,000-file limit, and the PR-commits endpoint a 250-commit limit. Provide an alternate retrieval path where practical and explicitly label any remaining partial result. Never label capped output complete. [S10]

## 12 Branch and commit graph

**GRAPH-01** Include a native history view with branch labels, commits, parent connections, and visible merge relationships. The initial scope is remote GitHub branches. Branches are movable references to commits, and merge commits can have multiple parents; draw that ancestry rather than inventing permanent parent-child relationships between branch names. [S17]

**GRAPH-02** Show a bounded recent history for the default branch and selected branches first. Load older commits on scroll or explicit request, and cache them. Display messages, authors, dates, abbreviated commit identities, and relevant branch or tag labels.

**GRAPH-03** Selecting a commit opens its changed files and diff. Selecting two commits opens a labeled comparison. For merge commits, identify the parent used for a single-commit comparison and permit selection of another available parent.

**GRAPH-04** Lay out parent relationships consistently, preserve visible lane and scroll stability while expanding history, and mark unloaded parents as continuation boundaries. A missing fetched parent is not proof of a root commit.

**GRAPH-05** Update moved or deleted branch labels independently from cached immutable commit data. Handle force-pushes without corrupting the graph or discarding review drafts.

**GRAPH-06** Start with GitHub references and commit APIs. These expose the identities and relationships needed for a focused graph. A full clone must not be required. [S18] [S26]

**GRAPH-07** A bare Git object cache is an optional future optimization for deep history or complete comparisons. Bare repositories avoid a checkout but do not inherently minimize transfer; partial clone filtering can defer file contents when supported. Any shallow history must expose its incomplete boundary. [S19]

## 13 Notifications and activity rules

### Monitoring and event categories

**NOTIFY-01** Provide native desktop notifications and a persistent in-app activity feed, with Windows integration first and macOS/Linux integration in their release phases under PORT-05. The native integration must support opening the correct item when clicked, single-instance behavior, and useful notification identity or grouping. Validate the chosen Rust and native API integration early, with Windows first and actual target evidence before each later release. [S24]

**NOTIFY-02** Activity monitoring MUST run independently of demand-loaded full content. Enabled events must be detectable on selected repositories even for issues and PRs the user has never opened. Monitoring must not require a complete historical download.

**NOTIFY-03** Provide independent controls for new commits or observed branch updates, ordinary comments, inline comments, review requests, approvals, requests for changes, comment-only reviews, PR opening or readiness, merges, issue assignments, issue state changes, and direct mentions. Check-status notifications are included where the credential and API support them.

**NOTIFY-04** Commit monitoring must support the default branch, chosen branches, branch patterns, or PRs the user follows. Identify observed new commits accurately. Do not infer who pushed a commit from its author if push-actor information is unavailable.

**NOTIFY-05** Support per-user global defaults and per-repository overrides. Relevant-item filters include all monitored activity, items involving the user, and explicitly followed or pinned items. Store preferences locally.

### Delivery and suppression

**NOTIFY-06** Each enabled category must support pop-up plus activity feed, activity feed only, or off. Sound is independent and off by default. Provide repository mute, thread snooze, quiet hours, and suppression of the user's own actions.

**NOTIFY-07** Suppression rules match stable actor identity, event category, and optional repository scope. Use bot or app metadata where reliable and allow explicit account rules. Do not promise perfect automatic recognition of every automated review.

**NOTIFY-08** A rule for automated review activity must cover the selected actor's formal reviews and inline review comments. Ordinary PR comments from that account are separately configurable. Human replies, unrelated events, and all activity on a bot-authored PR must not be suppressed merely because the PR author is a bot.

**NOTIFY-09** Muting notifications MUST NOT remove or hide the underlying review or comment in its issue or PR view. Provide a convenient Mute review activity from this account action and a settings preview explaining the resulting rule.

**NOTIFY-10** Resolve notification categories independently. An Off setting disables that category's delivery path, not every other category matched by the same activity. Comments Off with Direct mentions Pop-up therefore permits one mention notification. Resolve per-repository overrides before global category defaults, then combine eligible paths into one notification using the strongest permitted delivery mode.

Apply global disablement and repository mute first. Apply own-action suppression and actor rules to the underlying activity across its eligible category paths. An explicitly enabled mention exception can restore only an enabled mention path suppressed by an actor rule. It cannot override global disablement, repository mute, own-action suppression, an Off mention category, snooze, or quiet hours. Apply snooze and quiet-hour delivery restrictions before grouping and deduplication. Keep enough decision information to explain the result.

**NOTIFY-11** Quiet hours and snooze suppress pop-ups while retaining otherwise eligible activity-feed entries. Synchronization continues according to its independent schedule. An explicit category Off setting disables that category's feed and pop-up path; a separately enabled matching category may still qualify under NOTIFY-10. Global disablement and repository mute suppress all notification paths in their scope. Underlying repository content remains available normally.

**NOTIFY-12** Group related bursts by repository and issue, PR, or branch. A review with many inline comments should normally produce one alert. An example is 3 new commits on feature/search, with one action opening the corresponding comparison.

### Newness and reliable delivery

**NOTIFY-13** Establish a baseline for each repository and event category when monitoring begins. Downloading old content for the first time MUST NOT itself create a new-activity notification. A genuinely new comment on an old unopened issue may still qualify based on its trustworthy event identity and timestamp.

**NOTIFY-14** Where no trustworthy earlier state or event time exists, establish state without inventing a transition. Deduplicate events across polling endpoints and multiple notification categories. A comment that mentions the user should not produce duplicate comment and mention banners by default.

**NOTIFY-15** Persist event keys, eligibility decisions, grouping keys, and delivery state. Reuse stable native notification identities where supported and report unsupported replacement/retraction capabilities truthfully. Prevent ordinary duplicate delivery across retries and restart; do not claim an unconditional exactly-once guarantee across application crashes and OS delivery.

**NOTIFY-16** Distinguish comment creation from editing. Record the user's successful writes so a subsequent poll does not echo them as unsolicited new activity when own-action suppression is enabled.

**NOTIFY-17** After a long offline period, group eligible catch-up events into a readable summary. Do not replay a storm of old banners. Use current notification preferences when delivering queued activity.

**NOTIFY-18** Clicking a notification must navigate to the correct repository, item, and available thread or diff using the tab routing in WORKSPACE-11. Show cached summary content immediately and fetch missing details on demand while preserving unrelated panes. Support useful actions such as Open, Snooze, and Mark read.

**NOTIFY-19** Respect the published operating system's notification preferences. Local monitoring runs only while the application or tray process is active. Polling detects observed changes but cannot guarantee every transient push or state transition between checks. GitHub's general Events API has retention limits and documented latency; do not use it as the sole reliable event source. [S25]

**NOTIFY-20** A hosted webhook relay and stronger always-on event delivery are later capabilities. Keep the event-ingestion interface extensible, but do not add required infrastructure to the first release.

### Initial preference examples

Use these as editable setup defaults, not fixed behavior: direct mentions and review requests produce pop-ups; comments on followed items produce pop-ups; other comments and merges enter the activity feed; commit alerts begin off until branches are chosen; own actions are suppressed; quiet hours begin disabled. Automated-review suppression requires the user's chosen actor rules.

## 14 Writes and merge correctness

**WRITE-01** Keep unsubmitted drafts separate from submitted operations. Model submitted operations using queued, validating, sending, outcome unknown, waiting for remote completion, blocked/needs attention, confirmed, and cancelled states as defined in OUTBOX-02. Persist intent before delivery; show pending local changes distinctly until GitHub confirms them. Share operation state and server results across all views, independently of the originating tab's lifetime.

**WRITE-02** Save a draft snapshot and durably enqueue the user's explicit submission before attempting a request. Preserve them after authorization errors, validation failures, offline transitions, server errors, and closing their tab. Enforce shared draft ownership and one operation identity across panes. Merely saving, editing, or reopening an unsubmitted draft MUST NOT enqueue it. Submitted eligible actions MUST resume automatically after restart or reconnection without asking the user to submit them again, subject to the outbox rules.

**WRITE-03** A timed-out issue, comment, review, or merge operation may have succeeded remotely. Keep the submitted operation in the outbox and automatically reconcile reliable identities or server state before considering another attempt. Retry automatically only when the action is known not to have been applied or the provider has a verified safe idempotency mechanism. If the result remains ambiguous, show Outcome unknown and allow inspection or stopping further attempts without claiming remote undo. Do not blindly replay a non-idempotent request; matching text alone is not proof of success. A deliberate retry of an unresolved action must explain the possible duplicate and cannot be triggered by reconnection alone.

**WRITE-04** Preserve concurrent remote edits, submitted intent, and local input. Revalidate affected fields, permissions, and revisions immediately before dispatch, including automatic replay; explain conflicts and keep the action visible as Needs attention. Retain the draft for correction. Document unavoidable races where GitHub supplies no atomic conditional operation. A temporary connectivity interruption alone must not require resubmission of an already queued action.

**MERGE-01** Users MUST be able to request an eligible merge while offline using a locally cached target PR, intended head commit, and chosen supported method. Durably queue that intent. Actual merge execution requires connectivity and fresh checks of PR state, permissions, allowed methods, and server requirements. A cached green check or connection indicator is not authoritative. Send the originally intended expected head commit identity; never silently substitute the current head. [S10]

**MERGE-02** Offer merge, squash, or rebase only when supported and permitted. Respect required checks, reviews, and branch rules. Never silently use administrative bypasses or change repository policy.

**MERGE-03** The first release supports ordinary permitted direct merges and explicitly excludes GitHub merge-queue enqueue, management, and withdrawal flows. GitHub auto-merge is optional future scope. Distinguish a merge waiting in the local Outbox, a request that may have reached or been accepted by GitHub, and a confirmed merged PR; reconcile uncertain or accepted-but-unconfirmed results before claiming success or retrying. If fresh preflight or a server rejection reveals that the repository requires a GitHub merge queue, retain the submitted intent as Needs attention with a clear unsupported-capability reason and a link to the PR when useful. Do not enqueue it remotely, enable auto-merge, bypass a rule, or change policy. Local cancellation stops eligible local attempts and never implies that a remote merge was undone.

**MERGE-04** If the intended head changed, permissions or policy no longer permit the action, or current validation conflicts with queued intent, retain the action as Needs attention, preserve the review context, and require a fresh user decision after updating the view. Do not silently retry against a different head. A safe retry of the same intent after temporary transport failure or throttling is automatic under OUTBOX-04/05; an uncertain outcome is reconciled before another attempt.

### Durable action outbox

**OUTBOX-01** Route every explicit remote mutation through a durable SQLite outbox before sending, online as well as offline. Commit a stable operation ID, immutable submitted payload/version, original account and target identities, base values or commit revision, dependency references, draft snapshot, and locally pending projection atomically. Acknowledging Queued requires a successful durable commit; if storage is unavailable or full, keep the editor recoverable and clearly report that the action was not queued. Draft autosave does not authorize dispatch. The explicit action submission is authorization for its automatic later execution, without a routine second confirmation on reconnect.

**OUTBOX-02** Use a recoverable operation lifecycle: Queued (including waiting for connection, dependency, pause or retry time), Validating (fresh read/preflight), Sending (a mutation may reach GitHub), Outcome unknown (automatic reconciliation), Waiting for GitHub (accepted asynchronous work), Needs attention (blocked by auth, conflict, unsupported capability or permanent validation failure), Confirmed, and Cancelled. Store a machine-readable reason independently of the state. Unsubmitted Draft is not an outbox entry. Confirmed and Cancelled are terminal only when justified by evidence; an ambiguous or remotely pending action remains inspectable. Exhausted transient retries wait in Queued and resume automatically when eligible.

**OUTBOX-03** Provide an Outbox/Pending actions panel that works offline. Show all unresolved submitted actions for the active account, with type, repository, existing target item/comment/thread, payload preview, intended revision/method, state and reason, dependencies, creation/attempt times, and next eligible attempt. Support filters, opening the originating item, individual cancellation, Pause/Resume queued actions, and Retry now when safe. Explain any unavailable control. Pending counts and row states update across panes; confirmed/cancelled history remains inspectable under a documented retention policy. Returning cancelled content to a draft is supported; deleting drafts is separate.

**OUTBOX-04** Automatically validate and dispatch eligible submitted actions after reconnection, application restart/wake, dependency completion, authorization repair under the same user, or expiry of a retry delay. Apply bounded exponential backoff with jitter, mutation pacing, shared API budgets, and relevant server headers. Off-hours read schedules and notification quiet hours do not delay already submitted actions. Respect Pause queued actions and Work offline. Definitely unapplied temporary failures remain queued; unrecoverable or content conflicts require attention; uncertain mutation outcomes follow OUTBOX-05. Only the running application/tray can dispatch. [S11] [S12]

**OUTBOX-05** Prevent duplicate local submission with stable operation IDs, transactional claims, and per-attempt evidence; do not assume this creates server idempotency. For a connection loss, timeout, crash, or ambiguous server error after dispatch, preserve Outcome unknown and reconcile before another mutation. Automatically retry only if reliable evidence shows no remote effect, or verified endpoint idempotency makes it safe. Persist known remote object/operation IDs and confirm through server state where possible. Matching body text or a brief absence from a list is not proof. If still unresolved, retain the action for attention rather than silently replaying or marking success. No unconditional exactly-once delivery guarantee is made.

**OUTBOX-06** Preserve dependency order for related supported operations while allowing unrelated eligible work to progress under the shared budget. Retain stable local operation/draft identities and record confirmed server identities for comment, review, and thread steps where a later supported operation requires them. Preserve review/thread sequencing and meaningful per-target edit order, including proven predecessor results in later preflight comparisons. Persist step identities and receipts for multi-request workflows; resume eligible unfinished steps rather than repeating completed steps. Show partially applied remote effects explicitly. When a predecessor is cancelled, rejected, or uncertain, hold dependent actions with a clear reason; never guess a remote target or drop descendants silently. Cancelling a parent must show its dependent impact, and users may inspect/cancel each remaining dependent. New-issue placeholders, local issue creation IDs, and create-issue dependency chains are not part of this contract.

**OUTBOX-07** Cancellation of an unsent queued or validating action MUST durably win against dispatch through an atomic state transition checked immediately before the mutation is sent. If cancellation wins, no request for that operation may later be sent, including after restart. If dispatch has already won, show Cancellation requested/May already be sent, stop further mutation attempts, and continue permitted read-only reconciliation. Cancelling an HTTP task is not proof GitHub did nothing. Do not claim remote rollback: a confirmed comment remains a comment and a remotely completed merge remains merged. GitHub merge-queue withdrawal is outside first-release scope. Keep original text, dispatch evidence, and dependent actions recoverable even if sign-out delays reconciliation.

**OUTBOX-08** Freshly validate the original target, permission, edit base values, review anchors, intended commit, merge method, and applicable policy immediately before mutation dispatch. Automatically execute unchanged valid intent without routine reconfirmation. A conflicting remote edit, changed reviewed/merge head, missing target, or changed permission moves the action to Needs attention. Never silently rebase a queued edit, re-anchor a review, approve a new revision, substitute a merge head, or change the chosen merge method. Show the current-versus-intended context so the user can correct or cancel it. Live server enforcement remains authoritative. [S09] [S10]

**OUTBOX-09** Bind each action permanently to the original host, user/account context, repository/resource identity, and credential owner scope; resolve credentials by protected reference at dispatch, never from a stored token in the payload. Switching users pauses the inactive account's work and isolates its queue contents. Reauthorization may resume the original user's eligible work; another user cannot inherit it. Removing a repository selection preserves its pending work and marks it paused for scope review instead of deleting or secretly sending it. Confirmed locator changes use verified identity and preserve the queue's stable local target.

**OUTBOX-10** Treat unresolved outbox operations, payloads, cancellation flags, dependency mappings, base snapshots, anchors, and remote reconciliation evidence as durable user data. Protect them from cache eviction, repair resets, account sign-out, and migration. Recover interrupted claims conservatively after a crash; an interrupted Sending state becomes Outcome unknown unless evidence proves it was never dispatched. Reconcile a successful remote write whose local completion transaction failed. Pending actions have no silent age expiry. Bound and virtualize the outbox UI, redact routine diagnostics, and preserve essential local content until completion/cancellation and any dependent work are settled.

## 15 Data model and lifecycle

The following are logical entities. The implementation default is one common items table for issues and PRs, with a one-to-one table for PR-specific details. Exact SQL column names may vary, but the identity and ingestion rules below are required. Use stable internal keys and retain provider identifiers. Scope access by host and account profile. Use indexes based on actual local query patterns.

| Entity group | Required information |
| --- | --- |
| Accounts and credentials | GitHub identity, credential reference, owner scope, access and SSO state. |
| Repository selections | Stable repository ID, display names, monitoring preferences, access state. |
| Items shared by issues and PRs | Local item_id, scoped repository_id, shared repository number, kind, title, body, author, open/closed state, timestamps, and typed provider identity mappings. |
| PR details | item_id as primary and foreign key, pull-request API identity, base and head references, draft status, merge state, and detail completeness. |
| Conversation comments and timelines | Stable provider IDs, common parent item_id, author, body or event type, timestamps, and availability. Keep comments and events distinguishable. |
| Reviews and threads | Review identity, state, thread identity, replies, resolution, source anchors. |
| Commits and parents | Commit identity, metadata, ordered parent relationships, retrieval coverage. |
| Branches and tags | Ref names, target identities, last validation, deleted or moved state. |
| Comparisons and files | Comparison kind, source identities, options, changed files, patches, completeness. |
| Resource fetch state | Resource family, progress, cursor or page, validator, timestamps, failure details. |
| Local drafts | Supported existing-item edit/comment/review draft type, target identity, base values, body, anchors, revision, last save, ownership. No new-issue creation draft. |
| Workspace sessions and views | Account, item-pane IDs and proportions, tab-instance IDs, item references, order, active/focused IDs, view-state version, scroll anchors, selected subviews, draft references, and per-PR context mode/selected issue/context-code proportions. |
| PR body issue references | Source PR item_id and body revision/hash, raw reference and order, normalized target locator, provenance, verification/access/type state, verified issue item_id when known, and last validation. |
| Durable action outbox | Operation UUID and payload schema version; account/host/repository/local and remote targets; submitted snapshot and base revision; dependency IDs and per-target order; lifecycle, block reason, retry time, lease/claim, cancellation flag, dispatch/response evidence, remote IDs, and reconciliation state. No credentials in payloads. |
| Connectivity observations | Reachability result, observation/request generation, reason, observed time and freshness; separate per-scope authorization, service, and rate-limit status. |
| Offline availability | Explicit resource/revision coverage, requested downloads, progress, offline pins, and minimal content protected by unresolved user actions. |
| Activity and delivery | Event identity, category, actor, grouping, rule result, delivery state. |
| Notification rules | Scope, category, actors, delivery mode, priority, exception and schedule fields. |
| Assets and search | Cached file metadata, storage identity, content references, derived search indexes. |

**DATA-01** Apply migrations transactionally where supported and test upgrading representative databases. Back up user-authored drafts and unresolved outbox work before risky transformations. Detect corruption or incompatible schema versions without silently resetting user work. Preserve operation identity, cancellation decisions, and reconciliation evidence across migration and recovery.

**DATA-02** Use UTC for persisted event timestamps and timezone-aware local scheduling. Use monotonic timing for in-process delays. Do not use a local wall-clock jump as evidence that remote content changed.

**DATA-03** Keep secrets and private content out of routine diagnostic output. Export diagnostics only through an explicit user action with appropriate redaction. Treat remote Markdown, links, and filenames as data rather than instructions or executable paths.

**DATA-04** Credential switching must isolate private cached data, drafts, and queued actions. Bind each action to its original GitHub user/account access context and credential owner scope. Pause dispatch for inactive identities, signed-out profiles, or known revoked access; resume eligible work under the same verified identity after PAT repair and subject to persisted controls. Replace credentials for the same user without recreating the account, item identities, cache, outbox, or synchronization baselines. Never reroute queued work through another signed-in user. Failed authorization and explicit sign-out preserve local content under AUTH-11; a signed-out local profile remains readable without exposing it through another active account.

### Shared issue and PR identity

**DATA-05** Store the visible GitHub number separately from the local primary key and API object IDs. GitHub issues and PRs use the same increasing number sequence within a repository: an issue numbered 41 can be followed by PR 42 and issue 43. There cannot be two distinct items consisting of issue 42 and PR 42 in that repository. Enforce UNIQUE(repository_id, number), without kind in that constraint. Here repository_id identifies a local repository record already scoped to host and account access context. The same number in a different repository or account context must not collide. [S27]

**DATA-06** Use an items table for shared fields and a pull_request_details table whose item_id is both its primary key and a foreign key to items. Only confirmed PR items may have this extension. The item kind distinguishes an ordinary issue from a PR once classified. PR summaries may exist before PR details are fetched; a navigation placeholder may remain unclassified until metadata arrives. Track missing detail separately from known false or null values. Keep PR merge state and source references in the extension, and derive the displayed merged state from confirmed PR data. A common closed-state update must not erase known merge information.

**DATA-07** Issue endpoints can return PRs, identified by their pull_request property. For that representation, the REST id is an issue ID; the pull-request endpoint supplies the separate PR ID. Store these in distinct typed fields or identity mappings and retain node IDs as opaque provider identifiers. If PR data arrives first, its issue-endpoint ID may remain unknown until fetched. Upsert both representations to the same item using the scoped repository number and verified provider identities. Resolve concurrent ingestion transactionally. Distinguish absent fields from explicit nulls and reject stale overwrites using applicable revision or freshness evidence. Never substitute one ID for the other, create a duplicate row for the second representation, downgrade a known PR because a partial payload omits its marker, or mark unrelated resource families complete. [S28]

**DATA-08** Store ordinary issue and PR conversation comments in a shared conversation-comments model keyed to item_id. Formal reviews and inline review comments remain distinct resources with their own provider identity namespaces and source anchors. Share common labels, assignees, search, drafts, and workspace references through the item identity. Route each remote operation by its API capability; a shared table does not make PR review or merge operations valid for ordinary issues. [S06] [S09] [S10] [S13]

**DATA-09** Use the stable local item_id for relationships, including drafts, tabs, comments, fetch state, and search records. The repository number is the current locator. Resolve a confirmed issue transfer or redirect through verified provider identity before changing that locator; preserve local relationships and the previous locator as an alias where appropriate. Never combine unrelated items merely because their numbers match, and never treat repository names or URLs as immutable database identity. If identity cannot be confirmed, preserve the existing record and surface the unresolved relocation. [S28]

**DATA-10** Store derived PR-body issue references separately from confirmed item identity and server relationship facts. Key a candidate set by source PR item_id and body revision/hash, preserve raw/reference position and provenance, and retain verification state and the verified target issue item_id when available. Recompute when the source body changes; retain unchanged verified targets and selection where still valid, but never associate a prior PR's or prior body's removed target with the current context. Cache reference evidence for offline use with its last validation, without marking current remote access or auto-closing behavior confirmed. Use the existing account/repository identity and transfer rules; do not create a new ordinary issue record or change a known PR kind merely to satisfy a parsed #number. Drafts and outbox actions keep their original stable targets even if a reference disappears. [S27] [S28] [S29] [S30]

## 16 Performance and usability acceptance budgets

These are proposed engineering targets to validate in a release build on a documented reference machine for the platform being released, with Windows first. Repeat actual measurements on macOS and Linux before those phases publish; Windows measurements are not evidence for another target. They are not measured claims about an existing implementation. Record CPU, RAM, storage, GPU, Windows version, display scaling, and dataset size with results.

| Interaction | Initial target |
| --- | --- |
| Open an ordinary already-downloaded issue or PR | First useful cached content at p95 under 100 ms. |
| Switch to an already-downloaded item tab | First useful cached content at p95 under 100 ms while another pane remains usable. |
| Local search | First result page at p95 under 200 ms on the reference fixture. |
| Warm startup | Usable cached screen in under 2 seconds. |
| Long discussion and diff scrolling | Smooth scrolling without synchronous parsing or large database work on the UI thread. |
| Idle behavior | No continuous redraw or busy polling when no work is due. |
| Memory | Bounded visible content and background work; measure working set and GPU memory separately. |
| Background synchronization | Cached navigation remains responsive while reads and writes are processed. |

Use a reproducible fixture containing approximately 50,000 issue/PR summaries, 250,000 comments, a long discussion, a large multi-file diff, and a branching history with merges. These are test inputs, not an estimate of the user's repositories. Include a smaller fixture for fast automated tests. Add an outbox fixture with 1,000 mixed pending/blocked/confirmed actions and dependencies; measure offline startup, queue inspection/cancellation, and cached navigation while eligible work drains. The queue must not make the UI wait for network or database batches.

Include a workspace fixture with two visible panes and twenty open item tabs, mixing issues, long discussions, and PR diffs. Measure tab switching, pane resizing, cached startup restoration, draft editing, and memory while background synchronization runs. Twenty tabs is a benchmark workload, not a product limit. Verify that inactive tabs do not multiply polling work or eagerly materialize every large diff. Include a PR with its default context/code pair, several linked issues, rapid mode and PR-tab switches, a collapsed/narrow context surface, and a live review draft; context loading must not block code navigation or input.

Before completing Sprint 01 native feasibility, record explicit numeric budgets for idle RAM, peak RAM, package size, cold startup, and scrolling frame times on the reference machine. Use these alongside the targets above as release acceptance budgets. Report pass or fail against every budget. A measured failure remains unresolved until fixed or an explicit product decision accepts a revised budget; measurement alone is not a pass. First-time network loading is measured separately from cached opening.

**UX-01** Support platform-appropriate keyboard navigation, focus indication, copy and paste, high-DPI displays, light and dark appearance, and sensible resizing. Test real text editors and review controls with the accessibility features used by the target audience on each published OS. Windows is verified first; macOS/Linux parity closes in their release phases.

**UX-02** Preserve user context while refreshing. Avoid replacing a useful cached view with a global spinner, jumping scroll positions, or losing a draft when selecting another tab. Loading and errors belong to the affected resource or pane; a slow request in one pane must not block navigation or editing in another.

### Installation and update delivery

**INSTALL-01** Each published platform MUST offer an end-user installation and rerunnable update path requiring one to three documented commands in its normal shell. Install prebuilt release artifacts; do not require Rust, Cargo, a compiler, Git checkout, or local source compilation. Use a per-user install by default without administrator privileges where the supported platform permits. Normal OS trust/permission prompts and the app's later PAT setup are allowed and must be explained; one to three commands does not promise zero interaction. The flow assumes the explicitly documented supported OS/system prerequisites, not hidden manual toolchain setup.

**INSTALL-02** Before changing the installed application, detect and validate OS, architecture, minimum supported version and required runtime/graphics/desktop prerequisites. Select the exact compatible artifact from a versioned release manifest; reject unsupported combinations with an actionable explanation. Avoid silent architecture substitution, undocumented compatibility assumptions, or downloading a different channel/version than requested. App dependencies should be bundled where practical; unavailable system prerequisites produce precise repair guidance instead of silent system modifications.

**INSTALL-03** Verify artifact integrity and publisher authenticity before execution or installation using the platform's appropriate signing/trust mechanism or a verified signed release manifest with an established trust root. A checksum alone establishes integrity, not publisher authenticity. Download over authenticated transport, validate the selected version/manifest/artifact relationship, fail closed on mismatch or failed verification, and clean up failed staging files. Do not require weakening OS security, disabling certificate checks, execution-policy bypass, or unauthenticated remote-script execution. The documented short command flow must preserve this trust chain.

**INSTALL-04** Stage and verify an update before replacing the installed binary. Handle a running instance and file locks safely, prevent simultaneous migration/Outbox dispatch during replacement, and preserve SQLite data, drafts, tabs/layout, settings, offline pins, credential references/protected tokens, sync checkpoints, baselines, and pending-action identities/receipts. Back up user data before risky migrations, and recover from interrupted install/update without falsely confirming actions or losing local work. Roll back binaries only when schema compatibility is safe; do not blindly restore an old Outbox snapshot that could replay already applied mutations. Explain recovery if migration cannot safely continue.

**INSTALL-05** Rerunning the same install command MUST detect an installed matching version and finish successfully without duplicating shortcuts, startup entries, instances, data directories or credentials. Support a documented stable channel and explicit version selection; upgrades preserve stable app identity and state. Reject unsafe downgrades or schema incompatibility with clear recovery guidance. Startup-on-login remains opt-in; installation must not silently activate background networking or override offline/pause settings.

**INSTALL-06** Failures MUST provide a short actionable explanation, a nonzero exit code when applicable, and a safe rerun path without asking users to compile or inspect developer logs for routine problems. Test denied permissions, blocked downloads, insufficient disk space, interrupted downloads, signature/hash failure, incompatible OS/architecture, missing prerequisite, and running-app locks. Do not request an application PAT for a public installer or expose credentials in commands/logs. If a chosen private release distribution requires separate download authorization, document that boundary without reusing the app's GitHub PAT implicitly.

**INSTALL-07** Provide a documented uninstall path using native platform conventions or an installed uninstaller. Uninstall removes app binaries and registrations while preserving local user data by default; removing data or protected credentials is an explicit separate choice with pending-work consequences shown. Reinstall must recover the retained profile, workspace and progress instead of forcing a full archive. Respect the already specified distinction between ordinary exit, explicit sign-out and cache deletion.

**INSTALL-08** Release deliverables MUST include the real published binaries/packages, version/architecture manifest, verification metadata, install/update/uninstall documentation, and tested one-to-three-command installation examples for that released platform. No release repository, URL or binary is available in this specification package: all illustrative commands are labeled templates until replaced with actual published endpoints and tested. Record clean-machine install, rerun, upgrade with seeded drafts/Outbox, failure recovery and uninstall/reinstall evidence on each claimed platform. A script file, placeholder URL or successful source build is not completed installer acceptance. [S35]

## 17 Acceptance scenarios

Each scenario must be traced to implemented behavior and meaningful unit, integration, native-process end-to-end, install/upgrade, or authorized live evidence as appropriate. Windows is the first release target; rerun applicable scenarios on macOS and Linux in their later phases. Use the per-platform traceability ledgers so later work remains pending without falsifying Windows completion. All tests in this specification package are Not run until application code and actual evidence exist.

### Required testing layers and evidence

**TEST-01** Implement a documented development test runner and relevant tests in every sprint, not only at release. The required command contract is cargo xtask test unit; cargo xtask test integration; cargo xtask test ui; cargo xtask test e2e --suite smoke or --suite full; cargo xtask test install --artifact <path>; and cargo xtask test live --repo OWNER/REPO with an explicit --allow-live-writes opt-in for approved mutations. Implement these commands or a documented equivalent wrapper before claiming their gates passed. They are planned commands, not tools already provided by this requirements ZIP. Each command returns a meaningful exit status, reports selected/skipped/blocked tests and environment, and writes machine-readable results plus concise logs. End users install prebuilt binaries and need none of this development tooling.

**TEST-02** Unit tests MUST cover nontrivial domain and state-machine decisions introduced by each sprint: shared item identities, absent-versus-null updates, reference parsing/type guards, completeness, schedule/DST rules, notification precedence, draft ownership, dependencies, cancellation, retry eligibility, conflict/revision checks and diff source anchors. Use controlled clocks, seeded fixtures and small independently checkable expectations. Test meaningful boundaries and regressions rather than mirroring implementation lines or pursuing an arbitrary coverage percentage. Add the relevant tests with the feature; future feature tests remain pending.

**TEST-03** Integration tests MUST exercise real SQLite databases/migrations/FTS/transactions and actual provider serialization/HTTP paths against a controllable local API server, not only mocked repository methods. Cover pagination/resumption, SSO/permissions/partial data, rate limits, cache publication, persisted checkpoints, signed-out identity holds, Outbox claiming/reconciliation, and upgrade cleanup protection. Verify production interfaces with deterministic simulated remote responses and OS adapter contract tests; use real protected-store/notification integration checks on the applicable native host. Keep live GitHub tests separate from deterministic integration suites.

**TEST-04** End-to-end tests MUST launch and drive the actual native application process through its user-visible controls using supported native automation/accessibility/input, with real local persistence and the implemented HTTP adapter connected to a deterministic test server where appropriate. Start with launch/focus/quit smoke coverage in Sprint 01 and expand to cache-first reading, tab/context switching, offline drafts, queued submissions/cancellation/reconnect, review/merge revision guards, notifications and restoration as those features arrive. Screenshots, GPUI in-process UI tests, callbacks, domain function calls, or a web browser test are not substitutes for native-process E2E. Keep GPUI UI integration tests as an additional layer. Distinguish simulator/test-build native E2E from smoke and release checks of the exact installed published package; the former does not prove packaging, release feature flags or publisher trust. A native automation gap is a visible blocked gate, not a reason to relabel a unit test E2E; record reproducible manual evidence for any inherently manual OS prompt and automate the remaining journey. [S32]

**TEST-05** Provide isolated reproducible fixtures and fault injection for delayed/out-of-order responses, partial pagination, 401/403/404/429/5xx, offline/online changes, sleep/wake, interrupted writes/migrations, full or unwritable storage, process crash before/after dispatch, timeout after remote success, cancelled predecessor chains, changed PR heads and removed links. Exercise exact data/state invariants and request traces, including no blind replay or retargeting. Use temporary per-test profiles, synthetic secrets/data and controlled test servers; reset deterministically without accessing a user's profile or emitting real notifications/writes accidentally. Preserve failure artifacts with secret redaction.

**TEST-06** Configure native-target build and portable-core unit/integration CI jobs for Windows, macOS and Linux from Sprint 01, adding actual suites as modules become available. Native Windows smoke E2E is required as part of each relevant Sprint 01-12 increment; full Windows suites gate Sprint 12. Attempt and record early native macOS/Linux smoke evidence, then require complete applicable native/E2E/install/performance coverage at Sprints 13/14. A later-platform runner or graphics limitation remains visibly Blocked/Not run for that target and must be resolved before publishing it. It does not silently prevent an otherwise complete Windows phase, while a discovered defect in shared correctness still requires repair. Headless rendering, cross-compilation and fake providers must be labeled accurately.

**TEST-07** Live-provider validation MUST use an explicitly designated disposable repository and appropriate test identities with no credentials embedded in code or results. Read-only live tests are the default; mutation tests require explicit --allow-live-writes and a configured allowlisted owner/repository, and cover only supported edits/comments/reviews/merges. Use existing issue/PR fixtures and do not add an app issue/PR-creation flow for test setup. Preserve ordinary SSO and permission enforcement. Keep destructive cases isolated, identify any effects and cleanup, and mark unavailable credentials/permissions Blocked rather than inventing successful integration evidence.

**TEST-08** A sprint report MUST list exact commands, commit/build identity, OS/version/architecture, fixture/seed, test layer and scope, results, failure artifacts, limitations, and requirement/AC/scenario links. Distinguish Pass, Fail, Blocked and Not run; skipped or mocked behavior never counts as a live/native pass. Maintain requirement, acceptance and scenario evidence per release platform, identifying simulator/test builds separately from the exact installed published package and its digest/signature. Every phase gate requires applicable unit/integration/native E2E/install tests, published-package smoke evidence, and targeted performance/accessibility evidence with no unexplained required failures. Rerun affected regression tests when shared code changes; do not claim all-platform completion from a Windows report or from validation of this planning package.

### Access and navigation

- **AC-01** A valid account with access to an SSO-protected private GitHub.com repository can select it and browse issues and PRs.
- **AC-02** A token requiring SSO authorization or organization approval produces an actionable access state without claiming that private repositories were deleted.
- **AC-03** Partial SSO results are identified; the UI does not claim that the visible repository list is complete.
- **AC-04** A user can browse a closed issue or merged PR older than 90 days without enabling a bulk archive.
- **AC-05** Rapidly switching between PRs cannot display a delayed response or diff belonging to the previously selected PR.

### Caching and synchronization

- **AC-06** With networking disabled, reopening a downloaded issue shows its content and unfinished draft.
- **AC-07** Failure on comment page two shows a partial discussion, not an empty or complete one; a retry resumes safely.
- **AC-08** Restarting during pagination does not lose completed records or duplicate them after recovery.
- **AC-09** Local search clearly limits coverage to downloaded content; remote search can discover an uncached item.
- **AC-10** Clearing downloaded content preserves all user drafts and unresolved submitted actions with their required snapshots and anchors, and explains which offline content is removed. At each earlier sprint, test the draft/queue types then implemented; final outbox integration is verified again in AC-68.
- **AC-11** Work-hour changes, timezone changes, sleep, wake, and offline recovery produce the intended scheduling behavior without a burst of overdue requests.
- **AC-12** A rate-limit response pauses appropriate work while cached navigation and draft editing remain usable.

### Issues and reviews

- **AC-13** Editing an existing issue title/body/metadata or state updates every matching local view only as confirmed or clearly pending, preserves input on failure, and never falls back to creating a new issue.
- **AC-14** A comment-creation timeout retains the submitted action and automatically reconciles it or shows Outcome unknown, without blind duplicate creation. A definitely unsent action resumes automatically when eligible.
- **AC-15** A conflicting remote issue edit is surfaced while preserving the local draft.
- **AC-16** A review with multiple inline comments can be saved, restarted, reopened, and submitted against the intended commit.
- **AC-17** A force-push preserves the original draft anchors and requires explicit handling of changed context.
- **AC-18** Approval and request-changes actions appear under the authenticated user's identity.
- **AC-19** A stale expected merge head is rejected and shown accurately; the app does not retry against a different head.
- **AC-20** A direct merge request that may have reached GitHub, or whose result is accepted but not yet confirmed, remains pending or Outcome unknown until authoritative evidence establishes the outcome. A locally queued merge is never displayed as already merged; no GitHub merge-queue flow is required or offered.

### Diffs and history

- **AC-21** Unified and split views show the same underlying change and keep inline comments on the correct source side and lines.
- **AC-22** Large diffs remain navigable while parsing and highlighting run in the background.
- **AC-23** Truncated, binary, unavailable, and API-capped content is explicitly identified.
- **AC-24** Loading older graph history preserves ancestry and scroll context; unloaded parents are shown as continuation boundaries.
- **AC-25** Moved or deleted branch labels update correctly without changing immutable commit records or losing drafts.
- **AC-26** PR comparison and two-commit comparison are labeled and use the intended merge-base or endpoint semantics.

### Notifications

- **AC-27** A new comment on an unopened monitored issue can produce the configured notification without downloading all repository history.
- **AC-28** Downloading an old PR for the first time produces no historical activity banners.
- **AC-29** Muting a selected review bot suppresses its configured review alerts while preserving human replies and all PR content.
- **AC-30** Notification rule precedence is deterministic, including direct-mention exceptions, mute, snooze, and quiet hours.
- **AC-31** A grouped review produces one sensible notification, and ordinary restart does not repeat an already delivered event.
- **AC-32** Reconnection after a long offline period produces grouped catch-up activity rather than a banner flood.
- **AC-33** Clicking a Windows notification opens the correct cached item or thread, including when activation launches the app.
- **AC-34** A notification for a comment containing a mention does not produce duplicate banners for two categories by default.

### Release and performance

- **AC-35** Release-build measurements report pass or fail against all recorded performance budgets, including cached opening, local search, startup, working set, GPU memory, scrolling, and behavior during background sync. Failed budgets remain visible acceptance failures.
- **AC-36** A clean installation of the supported platform being released can install, launch, authenticate, receive a notification, follow its activation, and uninstall cleanly. Windows closes first; macOS/Linux repeat with their own packages.
- **AC-37** The shipped dependency and process inventory contains no embedded browser or webview runtime.
- **AC-38** Platform-appropriate keyboard focus, text selection, high-DPI behavior, accessibility and draft recovery pass the documented native checks, with Windows first and separate macOS/Linux evidence at their milestones.
- **AC-39** A watched branch gaining commits produces its configured grouped alert; an unmonitored branch does not. Commits already present at the monitoring baseline produce no historical alert.
- **AC-40** An observed unmerged-to-merged PR transition produces the configured delivery without duplicates in the normal retry/restart path. A PR already merged at the initial baseline produces no merge alert merely because it was discovered.
- **AC-41** Comments Off with Direct mentions Pop-up produces one eligible mention alert. Actor suppression blocks it unless the explicit mention exception applies; own-action suppression, repository mute, snooze, and quiet hours still apply.

### Workspace tabs and panes

- **AC-42** An issue and a PR from different selected repositories remain simultaneously visible in resizable side-by-side panes, each with multiple item tabs. Switching a tab in one pane leaves the other pane's content, scroll position, and draft intact.
- **AC-43** Opening beside, reordering or moving a tab, maximizing and restoring a pane, and narrowing the window preserve all open items and their view state. A loading or failed request in one pane does not block the other pane.
- **AC-44** The same PR can appear in two panes with Conversation selected in one and Files selected in the other. Shared data updates both views without changing their subview or scroll state, and concurrent requests for the same resource and revision are deduplicated.
- **AC-45** Starting a comment, review, or merge from one pane and immediately focusing another pane cannot change the request's target. Multiple views of the same draft use the same draft identity and cannot silently overwrite it or duplicate a pending submission.
- **AC-46** Closing and reopening a tab, closing a pane, and restarting offline restore saved drafts and applicable view state. The saved pane layout, tab order, and selected tabs return without downloading inactive content or automatically submitting unsubmitted drafts.
- **AC-47** Opening a search result or following a notification focuses or creates the intended item tab and navigates to the correct target without replacing an unrelated tab or changing another pane's input. Account switching isolates saved tabs and private content.
- **AC-48** The two-pane, twenty-tab fixture meets the recorded tab-switching and responsiveness budgets while syncing. Inactive tabs do not each create polling loops or retain full large-diff render trees, and closing one view does not cancel work needed by another.
- **AC-49** Tab opening, selection, closing, reopening, pane focus, and moving tabs between panes are operable with the documented keyboard commands and meaningful accessibility names. Ordinary text editing remains usable in both panes.

### Shared issue and PR storage

- **AC-50** Issue 41, PR 42, and issue 43 in one repository occupy three common item rows. A second representation of PR 42 updates the same row. Another repository's issue 42 remains a separate item, and the uniqueness constraint cannot be bypassed by changing kind.
- **AC-51** Fetching a PR through the issue endpoint and then the pull-request endpoint, in the reverse order, or concurrently produces one item and at most one PR extension with correctly distinguished API IDs. PR-first loading works while the issue-endpoint ID is unknown. Partial or older payloads do not downgrade PR kind, overwrite newer fields, clear known PR details, or mark unrelated resources complete.
- **AC-52** A merged PR remains displayed as merged after a common issue-shaped response reports state closed without merge details. A PR whose merge information is not downloaded is not falsely classified as merged or definitely unmerged.
- **AC-53** Ordinary conversation comments for an issue and a PR use the common item linkage without duplicate search results. Review comments retain their distinct identities and file anchors, and review or merge controls are unavailable for ordinary issues.
- **AC-54** With item 900 cached, a later comment or state change on item 12 is still found through the applicable update path. Gaps in repository numbers do not cause integer-by-integer probing, false deletions, or a false claim that all history is downloaded.
- **AC-55** A fixture representing a confirmed issue transfer preserves the local item identity, drafts, and tab references while updating its current repository locator. An unrelated destination item with the same number is not merged into it; unresolved identity remains explicit.

### Offline operation connectivity and queued actions

- **AC-56** After a cold start with networking disabled, downloaded issues/PRs, discussions, review data, diffs, fetched graph history, local search, activity feed, settings, drafts, and the outbox remain usable without a blocking login or connection check. Unfetched content has an explicit placeholder and retained target.
- **AC-57** The global indicator shows green for current verified GitHub responses and red after current transport failure or Work offline. Startup/stale evidence uses a neutral state. 401/403/429/5xx show separate auth/throttle/service status rather than falsely claiming no Internet. A late response, cancelled view request, multiple panes, sleep/wake, and network change do not produce false status or probe storms.
- **AC-58** Editing an existing issue and submitting an ordinary comment while offline immediately produces durable pending entries and local previews. They survive restart, submit automatically after reconnection without a second Submit, and reconcile pending previews with the existing issue and confirmed comment without duplication.
- **AC-59** Saving, editing, reopening, or restoring an unsubmitted draft never adds it to the outbox or sends it on reconnection. Explicitly submitted queued work resumes automatically when eligible, including after an online restart.
- **AC-60** Cancelling an unsent queued/validating action persists its cancelled state and prevents dispatch, including when cancellation races with reconnection or the worker claim. The queue, counts, and all panes update; original text remains recoverable after restart.
- **AC-61** Cancelling after a mutation may have been sent stops further attempts and retains honest cancellation-requested/outcome-unknown status. Reconciliation can report that the action already succeeded; the UI never promises that stopping an HTTP request undid a remote effect.
- **AC-62** For existing-issue edits, comment creation, review submission, and merge, inject server acceptance followed by a dropped response, process crash, or failed local completion transaction. Recovery reconciles known identities/results without blind replay, duplicate local operations, or false success/cancellation; unresolved ambiguity remains visible.
- **AC-63** Queue a supported review/thread workflow whose later step requires the confirmed predecessor identity, plus meaningful ordered edits on an existing target. Persist operation IDs and successful step receipts across restart, run unfinished steps in order without repeating completed mutations, and include proven predecessor changes in later conflict checks. A cancelled, failed, or uncertain predecessor holds its dependents with a visible reason while unrelated eligible actions continue. Test actual review/thread adapters, not only storage fixtures; no new-issue creation chain is used.
- **AC-64** An offline issue edit detects a conflicting remote edit before dispatch and remains Needs attention with both versions preserved. Invalid input, revoked/SSO-pending credentials, and rate limits retain work. Exhaust a transient retry burst, then reconnect or repair same-user access: eligible actions resume automatically without another Submit or retry storm.
- **AC-65** A queued inline review, approval, or request for changes retains its intended commit/anchors across offline restart. An unchanged valid revision submits automatically; a changed head or invalid anchor requires attention and cannot silently approve or comment on different code. Permitted queued thread actions follow the same intent checks.
- **AC-66** An offline direct-merge intent retains the selected method and expected head. Reconnection dispatches it automatically only after fresh eligibility checks; a changed head is held for a new decision. Local pending, remotely accepted or uncertain, and actually merged states remain distinct. Local cancellation never claims to undo a remote merge. A repository requiring a GitHub merge queue follows AC-77.
- **AC-67** Switching accounts, repairing credentials, or removing a selected repository cannot send pending issue, review, or merge actions as another user or against another target. Inactive/deselected scope work is held visibly in its proper account context and survives restart.
- **AC-68** Crash recovery, upgrade, sign-out, and ordinary cache cleanup preserve unresolved operations, cancellation tombstones, dependency maps, payload versions, base snapshots, and review anchors. Inject a full/unwritable database during enqueue: no Queued success is shown and the input remains recoverable.
- **AC-69** Submitted eligible actions resume outside work hours and during notification quiet hours; Pause background refresh does not pause them. Pause queued actions stops new mutation dispatch, Work offline blocks all networking, and a fully exited/sleeping application sends nothing. Restart/wake coalesces work and respects rate limits and mutation pacing.
- **AC-70** Download for offline use exposes selected resource/revision coverage and progress, survives interrupted pagination, and can be requested/cancelled while offline. Reconnection completes eligible downloads without archiving the whole repository; capped, missing, or inaccessible PR data is never shown as fully available.
- **AC-71** The Outbox works offline with existing-issue edit, comment, review, and merge entries. Users can inspect payload, original target/revision/method, status/reason, dependencies, attempt times, and eligible controls. Counts and state agree across panes, filters and item navigation work, and cancelling a parent exposes dependent impact without erasing its children.


### Persistent sessions, PAT renewal, and incremental restart

- **AC-72** With an expired or revoked PAT, reopen the established local profile and read/edit cached local work without a login wall. Issue, review, and merge operations remain retained and held. Supply a permitted replacement PAT for the same verified GitHub user: retain account/item identities, cache, drafts, outbox, and synchronization baselines, then resume eligible reads and unchanged submitted actions under restored pause/offline controls. A PAT belonging to another user cannot inherit the work. Validate the chosen fine-grained or approved classic PAT against the actual required endpoints and SSO-protected repository.
- **AC-73** Seed every implemented content/draft type, panes/tabs with non-default view positions, local settings and offline/pause flags, pending actions, sync checkpoints, and notification history. Quit normally and relaunch online and offline. Restore the same local workspace before a deliberately delayed API response, retain the protected PAT without asking for re-entry while valid, and preserve all local data. Verify that Work offline prevents startup probes, paused actions do not dispatch, unsent drafts remain drafts, and restarting does not recreate old notification alerts. This is the full-release lifecycle matrix, not a fixture-only storage test.
- **AC-74** Explicitly sign out with cached private content and pending issue/review/merge work. Verify local PAT material is removed or invalidated, no new requests dispatch under that profile, the hold survives restart, and its local content, drafts, workspace, settings, sync checkpoints, and outbox remain readable in that signed-out profile. A different active account cannot see or send its data. In-flight effects remain uncertain when appropriate; same-user PAT repair reconciles them before safe replay. Sign-out performs no implicit purge or claimed remote undo.
- **AC-75** Seed completed and partially downloaded issue/PR resource families plus persisted checkpoints and validators, stop the app, then change an old issue comment, PR review/check state, and other mutable fixture data while it is stopped. On restart, display cached content before networking finishes and use a recorded request trace to demonstrate bounded catch-up from saved progress, discovery of the old-item changes, safe partial-page resumption, and unchanged-resource revalidation. Do not trigger a full historical redownload or reset baselines. Overlapping reads, an update-feed first page, and endpoints without exact deltas are valid when targeted and documented; no zero-unchanged-payload assertion is required.
- **AC-76** Expire a stored cursor or remove delta support for one fixture resource family during restart catch-up. Re-enumerate only the necessary resource/range with stable-ID deduplication, preserving readable data, partial coverage, completed unrelated families, and draft/outbox references. Crash or fail between a returned page and its transaction, then resume without skipped changes, duplicate records, or a falsely advanced success checkpoint.
- **AC-77** Inject a repository rule requiring a GitHub merge queue before dispatch, and separately a server rejection revealing that requirement after preflight. Keep the original merge intent visible as Needs attention with an unsupported-capability reason. The request trace contains no merge-queue enqueue/manage/withdraw request, no auto-merge enablement, and no administrative bypass. Ordinary permitted direct merges and the local Outbox still work.

### PR context layout and sidebar totals

- **AC-78** At a usable width, a PR review tab opens with one 30-35% context / 65-70% code pair and PR body selected. Resize/collapse the context, toggle View PR body / View issue, select another verified issue, and use a narrow focused surface while an inline review draft is open. Preserve the code file/comparison, scroll/selection, draft identity/anchors, and deliberate editor focus. Restore the layout, context mode, selected issue, and separate scroll anchors after tab switching and offline restart. Multiple PR tabs and general Open beside remain usable without a required third issue pane.
- **AC-79** Parse fixtures covering Resolves #41, uppercase/colon variants, the supported closing keywords, owner/repository#41, GitHub.com issue URLs, duplicate references, multiple issues, code examples, and a shared-number target that is actually a PR. Verify actual issue type and scoped identity, offer a picker for multiple issues, and never label a PR as an issue or infer an auto-close outcome. A reference on a non-default base branch or with auto-close disabled can remain reading context without a false server-relationship claim.
- **AC-80** With delayed context responses, switch PRs and body revisions and change linked-issue selection. No response shows the previous PR's issue or a removed reference under the new PR. Exercise no link, unresolved verification, unavailable permissions, missing/offline content, failed fetch, and cached verified linkage states. Cached bodies/issues remain readable offline; a lost link preserves the original-target draft and does not retarget it. A cross-repository reference does not enable unrelated repository monitoring or leak credentials.
- **AC-81** Review requested, Assigned to me, and Outbox show right-aligned dark blue true circular badges with centered white digits for positive totals, including multi-digit and high-DPI fixtures. A known zero shows no circle and no number. Counts follow the active account and documented scope, deduplicate items, are not just visible-page or unread counts, and expose incomplete/unknown coverage honestly. Outbox counts unresolved submitted actions including blocked/unknown states, excludes drafts/downloads/terminal history, and updates after enqueue/cancel/confirm and across duplicate panes without extra polling.
- **AC-82** Inspect navigation, shortcuts/commands, draft types, mutation capabilities, and request traces: the current release exposes no New issue form or create-issue action/endpoint flow and no PR-creation workflow. Reading issues, authorized edits to existing issues, ordinary comment creation/editing, offline drafts, and their durable submitted actions still work. An issue edit failure never creates a replacement issue.

### Platform testing and installation

- **AC-83** Sprint 01 establishes the platform interfaces, native-target build/core-test CI matrix and a recorded support plan without importing Windows-only APIs into the shared core. Record actual job results per target and clearly pending later-platform gates; do not infer runtime support from compilation.
- **AC-84** Launch the real native feasibility app on Windows and drive text input/copy, focus, pane/context resizing and quit through native controls. Record renderer/process evidence and early macOS/Linux smoke attempts separately. No browser engine or in-process UI snapshot is presented as native-process E2E proof. Each later released OS repeats this proof on its own target.
- **AC-85** Exercise the actual supported platform's protected credential store and per-user data path, including unavailable/locked store, Unicode paths, restart and same-user PAT renewal. No plaintext fallback or new duplicate profile appears; offline cached reading and original outbox identity survive. Windows closes initially in Sprint 03; later OS adapters repeat it in their phases.
- **AC-86** The packaged target app delivers a native notification, opens the exact item through cold/warm single-instance activation, honors notification preferences, and exposes unsupported actions/replacement semantics honestly with in-app alternatives. Close/Quit, optional background residence, wake and startup behavior preserve pause/offline settings and user data. Windows closes initially in Sprint 11, with clean-machine packaging repeated at release; later OS phases require their own integration evidence.
- **AC-87** The macOS milestone passes applicable product/credential/lifecycle/notification/native E2E/performance/installation gates for its published support matrix. It preserves the Windows feature contract and no-browser constraint. Windows completion is not reused as macOS evidence, and Windows regressions from shared changes are checked.
- **AC-88** The Linux milestone passes applicable product/credential/lifecycle/notification/native E2E/performance/installation gates for every claimed distribution/desktop/session/architecture combination. Missing graphics, secure-store or system prerequisites produce truthful guidance; untested variants are not declared supported. Previously released platform regressions are checked.
- **AC-89** Implement and run the unit-test command on the Sprint 01 core and subsequently introduced domain behavior, with deterministic fixtures, meaningful exit status and machine-readable results. The initial gate proves the runner and current tests, not future feature coverage; every sprint extends its own relevant units.
- **AC-90** Use actual SQLite storage and the real HTTP provider adapter against a deterministic local server to verify transactional identity/cache behavior, pagination, partial failures and authentication/error mapping. Storage foundations are tested in Sprint 02 and the initial complete adapter scenario closes in Sprint 03. Later integration scenarios extend this foundation rather than being claimed complete early.
- **AC-91** Run the full native application E2E journey for the completed phase: cached startup, search, PR body/issue context switching, preserved inline draft, offline submission, Outbox inspection/cancellation, reconnect/reconciliation, changed-head guards, notification activation and restart. Evidence identifies the actual app process, controls, local database and provider requests; unit calls or GPUI snapshots cannot satisfy it.
- **AC-92** Deterministically inject response loss after server success, a crash around dispatch, disk/transaction failure, cancel-versus-reconnect races and revision changes. Native/integration evidence shows retained input, stable target/operation identities, safe dependency holds and no blind duplicate send, false cancellation or lost local data. Run applicable cases throughout the feature sprints and the complete matrix at phase release.
- **AC-93** The phase report links every applicable requirement/AC/scenario to unit, integration, native E2E, installer and approved live evidence or an explicit failure/block. Commands, versions, OS/architecture and artifacts are recorded; later-platform statuses stay separate. A skipped or unavailable required native test prevents that platform's publication, not an unsupported all-platform pass.
- **AC-94** On a clean supported target with declared system prerequisites and no Rust/compiler checkout, follow the real published install instructions in one to three commands. The per-user app installs and launches; normal OS trust prompts are explained. A placeholder URL or source-build command fails this release gate.
- **AC-95** A wrong OS/architecture/version, incompatible prerequisite, truncated artifact, altered checksum/signature or unauthenticated payload is rejected before replacing the installed app. The previous install and user data remain usable, and the explanation plus safe retry path is actionable.
- **AC-96** Seed cached content, drafts, a non-default workspace, credentials, pause/offline flags, checkpoints and unresolved Outbox work, then rerun the installer and upgrade from the previous supported version. State and operation identity survive, registrations are not duplicated, and networking respects restored controls. Test interruptions, running-app locks and safe migration recovery without replaying a restored stale Outbox snapshot.
- **AC-97** Exercise denied permission, blocked/interrupted download, full disk, failed trust verification and missing prerequisite. Report a concise reason/nonzero exit and safe repair/rerun guidance without requiring developer compilation, an app PAT, security weakening, or silent system changes.
- **AC-98** Uninstall removes the application and registrations while retaining local data by default. Explicit data/credential removal is separate and explains pending-work impact. Reinstall restores the retained local profile and incremental progress without an implicit full archive or duplicate queued actions.
- **AC-99** The released platform's binaries, version/architecture manifest, authenticity metadata, tested install/update/uninstall commands and clean-machine evidence are actually published. Documentation contains no unresolved placeholder presented as runnable; unreleased platform examples remain clearly labeled templates and do not imply that their packages exist.

## 18 Sprint implementation plan

Execute fourteen scope-based increments in three release phases. Sprints 01-12 deliver the complete Windows first release. Sprint 13 delivers macOS parity and Sprint 14 Linux parity. Windows remains the highest priority. These are work scopes, not fourteen equal calendar periods; split large scopes into ordered subtasks without dropping requirements or gates.

The first useful cached reader arrives in Sprint 04, scheduled refreshing in 05, PR review reading/diffs in 06, existing-issue/comment writes and the durable Outbox in 07, reviews/merges in 08-09, graph/history in 10, notifications in 11 and Windows release verification/simple installation in 12. Later OS phases reuse shared behavior and add actual native/package evidence. Platform interfaces, build/core-test visibility, test runners and native Windows smoke tests begin in Sprint 01.

The pack maps all 175 requirement IDs and 99 acceptance scenarios to an initial implementation/verification owner. The per-platform ledgers separately record applicability, phase gates and evidence. A Windows Verified/Pass does not verify macOS or Linux. Later-phase gaps remain visible and must be resolved before publishing that target; they do not silently block an otherwise complete Windows phase. A defect in shared correctness must still be fixed. AUTH-10 remains Inactive future scope.

Every sprint includes unit/integration/native E2E test work appropriate to its implemented features, reproducible commands and evidence. Add tests with the feature and preserve prior behavior. Native in-process UI tests, native-process test-build E2E, live-provider tests and exact installed published-package smoke are different evidence classes. Current statuses are Planned or Not run: this is a specification pack, not application code or a claim that app tests passed.

An assigned Windows release ends at Sprint 12; an assigned all-platform release includes Sprints 13-14. Continue within the authorized scope without routine reapproval. Missing runners, publisher configuration, signing access or approved GitHub credentials blocks the affected validation/publication, not independent implementation. Never invent a pass, published URL or binary. Use TESTING_PLAN.md, INSTALLATION_PLAN.md, CROSS_PLATFORM_PLAN.md and the scenario/phase ledgers alongside each sprint file.

### Sprint map

| Sprint | Phase | Outcome |
| --- | --- | --- |
| 01 | Windows first release | Native feasibility and project foundation. A runnable native Windows shell proves the chosen stack can support the hardest interface and operating-system integrations. |
| 02 | Windows first release | Shared data model, SQLite cache, drafts, and outbox. A tested local domain and database can retain remote content and user drafts without losing identity or confusing partial data with complete data. |
| 03 | Windows first release | GitHub authentication, SSO, provider, and repository selection. The app can securely access an explicitly selected SSO-protected GitHub.com repository as the user. |
| 04 | Windows first release | Cached reading, search, tabs, and offline workspace. The first useful reader opens downloaded conversations instantly, searches locally, and keeps multiple issues and PRs visible with saved local drafts. |
| 05 | Windows first release | Synchronization, connectivity, and activity observation. Relevant cached content stays fresh on configurable schedules without blocking reading or forcing an archive; trustworthy activity observation exists for later notifications. |
| 06 | Windows first release | PR context, commits, files, checks, and native diffs. A PR review tab presents prominent code beside one smaller body/verified-issue context pane, with cached discussions, reviews, commits, files, and checks. |
| 07 | Windows first release | Existing-issue edits, comments, and the action outbox. Users submit existing-issue edits and comment work online or offline, inspect or cancel pending actions, and have eligible work automatically delivered after reconnection. |
| 08 | Windows first release | Inline PR reviews, approvals, and review drafts. Users can review exact source revisions, retain unfinished reviews, and submit comments, approvals, or requests for changes as themselves. |
| 09 | Windows first release | Merge workflows. Permitted PRs can be directly merged with fresh server checks, expected-head protection, and accurate locally queued, uncertain, or merged outcomes. |
| 10 | Windows first release | Branch graph and commit comparisons. Users can navigate remote branch ancestry and compare commits through the same native diff system without cloning the repository. |
| 11 | Windows first release | Notification rules and native delivery. Users receive only the selected activity, including unopened items, with reliable local history, bot suppression, grouping, and correct workspace activation. |
| 12 | Windows first release | Windows testing, simple installation, and first release. A tested Windows-first release satisfies the complete Windows product contract and offers a verified one-to-three-command prebuilt installation/update path. |
| 13 | macOS release | macOS parity, native testing, and simple installation. A published macOS release preserves the Windows product workflows while implementing and testing its native integrations and installation. |
| 14 | Linux release | Linux parity, native testing, and simple installation. A published Linux release supports a bounded tested desktop/session matrix with full product parity, native services, and a simple verified prebuilt installation. |

### Sprint 01 Native feasibility and project foundation

A runnable native Windows shell proves the chosen stack can support the hardest interface and operating-system integrations.

**Release phase:** Windows first release.

**Prerequisites:** None; inspect the repository first.

**Code areas:** Cargo project, domain, github provider interface, ui, platform_windows, tests/fixtures, docs/architecture.

**Primary requirement IDs:** FND-01, FND-02, FND-04, PORT-01, PORT-02, PORT-03, PORT-06, TEST-01, TEST-02, TEST-04, TEST-05, TEST-06, TEST-08, INSTALL-01, INSTALL-02, INSTALL-03.

**Required implementation work:**

- **S01-T01:** Create a buildable Rust application and reusable core with the logical module boundaries in Section 4. Pin a tested GPUI/GPUI Kit dependency set and commit Cargo.lock; document any justified native alternative.
- **S01-T02:** Define typed provider, operation, resource-identity, cancellation, and structured-error interfaces. Supply a controllable fake provider; establish Tokio background execution and a minimal shared scheduler boundary.
- **S01-T03:** Build a Windows feasibility screen with long selectable Markdown, a large annotated native diff, two resizable item panes with separate tabs and keyboard focus, a PR-tab fixture with a 33/67 switchable-context/code pair, and a branching graph fixture. Exercise text selection, copying, high-DPI rendering, and focus.
- **S01-T04:** Prove native Windows notification delivery, click activation, single-instance routing, and opt-in tray lifecycle. This is the early platform contribution to NOTIFY-01, not completion of the notification product.
- **S01-T05:** Document the Windows reference machine and record numeric idle/peak RAM, GPU-memory, package-size, cold-startup, and scrolling-frame-time budgets alongside Section 16 targets. Add reproducible measurement entry points and inspect the dependency/process inventory for browser engines.
- **S01-T06:** Define small platform service contracts and isolate Windows APIs/dependency features from shared core/UI behavior. Record Windows-first, macOS Sprint 13 and Linux Sprint 14 targets, native capability gaps, support-matrix defaults and one-to-three-command prebuilt installation constraints. Design artifact/version/architecture/trust metadata now; later package delivery remains in the relevant release sprint.
- **S01-T07:** Implement the development test-runner entry points and deterministic fixture/clock/API-simulator boundaries in TESTING_PLAN.md. Configure native-target build/core-test CI for all three OS families, start real Windows process launch/focus/quit E2E through native controls, and record early macOS/Linux native smoke results or blockers. Separate GPUI in-process integration, native test-build E2E, exact installed-package smoke and live GitHub evidence.

**Required test work:**

- Unit: test initial typed identities/errors and controlled-clock/fixture helpers with the implemented runner. Configure build/core CI jobs on all three OS families.
- Native E2E: launch the real Windows process, use actual text/focus/resize controls and quit. Record other-target native smoke attempts separately; no future feature journey is claimed complete.
- Packaging design: check manifest/target/trust decisions and prototype the short-command contract as templates; no installer publication pass occurs here.

**Deliverables:** Runnable Windows feasibility build; Cargo.lock; provider fake; architecture decisions; Windows integration notes; baseline measurements and numeric budget file.

**Exit gates:**

- Run the native slice on the supported Windows target and record commands, machine details, observed behavior, and measurements. A Linux-only build does not satisfy this gate.
- Demonstrate selectable text, independent pane focus, responsive background work, notification activation into one instance, and a documented tray lifecycle without a browser engine.
- Resolve any platform feasibility failure before depending on it. Record the requested numeric budgets; a screenshot alone is insufficient.
- Pass AC-83, AC-84 and AC-89 for the initial Windows/core slice. Show implemented unit/native-smoke commands and actual results; later feature journeys and other-platform release gates remain pending, not incorrectly passed.

**Initial acceptance owner:** AC-83, AC-84, AC-89.

**Later integration:** Production authentication starts in Sprint 03; graph semantics in Sprint 10; notification rules, feed, and full activation in Sprint 11. The scheduler contract is completed in Sprint 05. Repeat the no-browser inventory for the shipped build in Sprint 12.


### Sprint 02 Shared data model, SQLite cache, drafts, and outbox

A tested local domain and database can retain remote content and user drafts without losing identity or confusing partial data with complete data.

**Release phase:** Windows first release.

**Prerequisites:** Sprint 01.

**Code areas:** domain, storage, queries/FTS schema, review identity types, workspace persistence, tests/fixtures.

**Primary requirement IDs:** FND-03, DATA-01, DATA-02, DATA-03, DATA-05, DATA-06, DATA-07, DATA-08, DATA-09, DATA-10, CACHE-03, CACHE-07, CACHE-08, CACHE-09, CACHE-10, CACHE-13, OUTBOX-01, OUTBOX-02, OUTBOX-06, OUTBOX-10, TEST-03, INSTALL-04.

**Required implementation work:**

- **S02-T01:** Implement SQLite migrations, foreign keys, indexes, WAL, short transactions, bounded connections, and FTS5 foundations. Add account-scoped repositories, common items, one-to-one PR details, and distinct typed issue/PR provider IDs.
- **S02-T02:** Enforce UNIQUE(repository_id, number), excluding kind. Implement transactional upserts for issue-first, PR-first, reversed, and concurrent arrival; preserve known merged state, absent-versus-null distinctions, and newer fields. Keep stable local item IDs through verified transfers and aliases.
- **S02-T03:** Persist resource-family completeness, freshness, validators, pagination progress, immutable comparison identities, commit parents, mutable refs, and revision-bound state. Establish storage contracts for sync jobs, activity baselines, event keys, delivery state, and write outcomes.
- **S02-T04:** Persist account-scoped workspace layouts and drafts independently of tabs and cache rows. Existing-issue edits and comment/review drafts retain their established target identity. Provide shared editor-ownership and operation-identity primitives; do not add a new-issue draft type. Persist per-PR context mode, selected verified issue, body-reference revision/provenance and layout fields for Sprint 06 integration.
- **S02-T05:** Implement draft-safe cleanup, repair and migration behavior, schema incompatibility handling, UTC timestamp conventions, diagnostic redaction boundaries, and safe handling of remote paths and content.
- **S02-T06:** Create small deterministic fixtures and a reproducible large-data generator for the Section 16 workload, including 1,000 mixed outbox entries. Test migration upgrades, account isolation, corruption/error handling, and draft/outbox survival during cache cleanup.
- **S02-T07:** Implement durable submitted-operation identities and immutable payload snapshots separate from editable drafts. Atomically persist enqueue, local pending projection, original identity/target/base revision, dependency IDs, per-target ordering, attempts, claims, retry times, cancellation decisions, and remote step receipts.
- **S02-T08:** Add crash-recovery and atomic cancel-versus-dispatch storage primitives, confirmed comment/review/thread step identity mapping, and payload schema migration. Protect required base snapshots and review anchors; a failed/full-disk enqueue must never return Queued success.
- **S02-T09:** Persist the complete normal-exit session contract: cache and coverage, workspace/view state, drafts, preferences and pause/offline settings, outbox evidence, per-family progress/validators, and notification baselines. Keep account identity independent of PAT values. Establish durable signed-out holds without deleting readable profile content; complete UI and writer integration in later sprints.
- **S02-T10:** Test actual temporary SQLite/WAL/FTS databases, migrations and disk/crash failures across configured core runners. Define stable per-user path and safe backup/migration boundaries for installed upgrades; do not place durable drafts or Outbox state in disposable cache. Deliver the storage subset of AC-90 now, with the real HTTP adapter portion closing in Sprint 03.

**Required test work:**

- Unit/integration: exercise actual temporary SQLite migrations, uniqueness/upserts, partial coverage, FTS, draft/Outbox durability, claims and cancel races, including disk/transaction failure.
- Native smoke: verify the build still launches/quits with an isolated profile; storage fixtures are not user-workflow E2E. Run available storage suites on all core targets.
- Upgrade foundation: test valid SQLite backup/recovery and schema compatibility using synthetic prior databases, preserving user-authored state.

**Deliverables:** Migrations and repositories; typed domain models; transactional ingestion; FTS foundations; separate drafts and durable outbox; dependency/claim/cancellation contracts; fixture generator; storage and migration tests.

**Exit gates:**

- Pass AC-50, AC-51, and AC-55 with database fixtures, including concurrent ingestion and an unknown issue-endpoint ID on a PR-first load.
- Prove cleanup cannot cascade into user drafts, including existing-issue edit base snapshots, comment drafts, and review drafts with source anchors.
- Demonstrate that missing, partial, complete, stale, unavailable, and failed resources survive restart distinctly. Authorization failures preserve cached content.

**Initial acceptance owner:** AC-50, AC-51, AC-55.

**Later integration:** Later sprints integrate these contracts with real provider data, panes, reviews, and merges. Storage tests do not complete their UI or live acceptance scenarios. Full draft-type integration closes in Sprint 08; all comparison kinds in Sprint 10.


### Sprint 03 GitHub authentication, SSO, provider, and repository selection

The app can securely access an explicitly selected SSO-protected GitHub.com repository as the user.

**Release phase:** Windows first release.

**Prerequisites:** Sprint 01, Sprint 02.

**Code areas:** auth, github, platform_windows credential store, storage/account profiles, ui/repository selection.

**Primary requirement IDs:** FND-05, AUTH-01, AUTH-02, AUTH-03, AUTH-04, AUTH-05, AUTH-06, AUTH-07, AUTH-08, AUTH-11, BROWSE-01, DATA-04, CONNECT-02, OUTBOX-09, PORT-04, TEST-07.

**Required implementation work:**

- **S03-T01:** Implement the confirmed PAT login path, protected native credential storage, visible active identity, and owner-scoped credential profiles. Prefer a fine-grained PAT where actual endpoint and organization compatibility permit; support an approved classic PAT when needed. Validate host and redirect routing; never send one owner's credential to an unrelated destination. OAuth/device authorization is future optional scope.
- **S03-T02:** Implement typed REST/GraphQL provider methods, pagination envelopes, capability reporting, read cancellation, structured API errors, and explicit API version handling. Verify current primary endpoint documentation and record required permissions.
- **S03-T03:** Represent SSO authorization, pending organization approval when detectable, partial SSO results, expired/revoked credentials, and missing permissions separately. Provide a validated external authorization action while preserving local context.
- **S03-T04:** Implement repository discovery, selection, revisit/removal, access status, last successful check, and cached-usage display. Removing selection must mark monitoring disabled; Sprint 05 connects that state to the running scheduler.
- **S03-T05:** Validate a designated private repository, not only the user endpoint. Test account switching, cached-content isolation, revoked access, and readable functions remaining available when write permissions are absent. Establish the permissions matrix for later issue/review/merge integrations.
- **S03-T06:** Emit typed API reachability evidence separately from authentication, SSO, service errors, and rate limiting. Bind queued work to its original identity and credential scope; same-user authorization repair may wake eligible work, while account switching or repository deselection holds affected actions.
- **S03-T07:** Distinguish ordinary exit from explicit sign-out. Retain the protected PAT across ordinary exit; sign-out persists a network/action hold and removes or invalidates local PAT material while retaining readable cache, drafts, workspace, checkpoints, and outbox. Reuse the same local account and resource identities after verified same-user PAT replacement. Expiry or revocation never creates a local login wall or a full resync. Exercise provider/storage cases now; full cache UI, writer, and release cases follow in Sprints 04, 09, and 12.
- **S03-T08:** Implement and test the Windows protected credential adapter and portable path contracts, including locked/denied/unavailable storage, Unicode user paths and same-user renewal. Keep macOS/Linux adapters as explicit phase tasks. Complete AC-85 and AC-90 with real local storage, actual HTTP serialization/pagination/error mapping, and controlled server failures; live read/write validation remains separately allowlisted.

**Required test work:**

- Integration: drive actual HTTP serialization/pagination/SSO/errors against the local simulator plus real SQLite; include partial/private access and credential-routing checks.
- Native E2E: use Windows account/repository controls with isolated fake credentials/server; separately test the real protected store and locked/denied conditions.
- Live: run allowlisted read-only repository access when credentials exist, mark missing SSO/approval blocked, and keep mutation tests off by default.

**Deliverables:** Live read-capable provider; protected token profiles; SSO/access diagnostics; selected-repository settings; endpoint/permission matrix; authentication and routing fixtures.

**Exit gates:**

- Demonstrate real authenticated access to the designated private repository and record the environment without exposing secrets.
- Pass AC-02 and AC-03 with controlled SSO/approval/partial-result cases. Verify 403/404 handling never assumes bulk deletion.
- Show account isolation and credential routing tests. If credentials or organization approval are unavailable, mark the live check blocked and continue independent work; do not mark it passed.

**Initial acceptance owner:** AC-02, AC-03, AC-85, AC-90.

**Later integration:** User-identity, endpoint permission, and same-user PAT renewal across all writers finish in Sprints 07-09. Signed-out local reading begins in Sprint 04; the all-content lifecycle matrix closes in Sprint 12. AUTH-10 remains inactive future scope. Repository removal is exercised against polling in Sprint 05.


### Sprint 04 Cached reading, search, tabs, and offline workspace

The first useful reader opens downloaded conversations instantly, searches locally, and keeps multiple issues and PRs visible with saved local drafts.

**Release phase:** Windows first release.

**Prerequisites:** Sprint 02, Sprint 03.

**Code areas:** queries, storage/FTS ingestion, github/list and discussion reads, workspace, ui/Markdown and search, local draft editor.

**Primary requirement IDs:** FND-06, FND-07, BROWSE-02, BROWSE-03, BROWSE-04, BROWSE-05, SEARCH-01, SEARCH-02, SEARCH-03, SEARCH-04, WORKSPACE-01, WORKSPACE-02, WORKSPACE-03, WORKSPACE-04, WORKSPACE-05, WORKSPACE-08, WORKSPACE-09, WORKSPACE-11, WORKSPACE-12, CACHE-01, CACHE-02, CACHE-04, CACHE-05, CACHE-06, CACHE-11, CACHE-12, ISSUE-01, ISSUE-02, UX-01, UX-02, OFFLINE-01, OFFLINE-03, OFFLINE-04, CONNECT-01.

**Required implementation work:**

- **S04-T01:** Implement paginated all-state repository browsing with no age cutoff, common issue/PR ingestion, filters, saved views, recent/pinned items, and URL navigation. Persist each page and its progress atomically; distinguish partial results and authorization failures.
- **S04-T02:** Render issue metadata, accessible timeline and ordinary conversations, including ordinary PR comments, from the cache. Fetch missing resource groups independently. Provide native selectable Markdown, on-demand images/attachments, and explicit unsupported/missing-content placeholders.
- **S04-T03:** Implement FTS search over downloaded content and clearly separate remote GitHub discovery. Handle remote caps, incomplete results, unsupported filters, and offline targets without asserting exhaustive historical coverage.
- **S04-T04:** Implement resizable side-by-side panes with separate tab strips, independent view state, duplicate views, tab movement/reordering, open-beside routing, maximize/restore, narrow-window behavior, close/reopen, and account-scoped lazy session restoration.
- **S04-T05:** Provide a local Markdown draft composer and preview now, including existing-issue edit and comment drafts, one active editor per draft, focus-editor actions, autosave, recovery, and offline editing. Submission remains unavailable until Sprint 07; closing a tab does not discard its drafts.
- **S04-T06:** Wire resource/tab identity and selection generations into asynchronous results. Preserve other panes' focus, selection, input, and scroll. Add keyboard/accessibility names, high-DPI and theme behavior, usage/cleanup controls, and early cached-open/search/tab-switch measurements.
- **S04-T07:** Start offline without a blocking network/login check for an established account. Add persistent green/red connectivity chrome with accessible text, checking/stale state, last-check details, and a pending-count link; integrate real status signals in Sprint 05.
- **S04-T08:** Build an offline-accessible outbox shell backed by persisted fixture entries and distinct pending local projections. Add Download for offline use for issue content with resumable visible/cancellable download jobs and truthful resource coverage. Actual mutation submission/dispatch/cancellation is completed in Sprint 07; PR offline-download integration follows in Sprint 06.
- **S04-T09:** Restore the normal-exit workspace from retained local state before any API response, including tab/pane order, focus, positions, selections, drafts, and settings. Permit an explicitly selected signed-out local profile to remain readable while marking remote work held. Prove the current slice with delayed networking and expired credentials; later graph, notification, and full-writer state closes in Sprint 12.
- **S04-T10:** Implement the sidebar count component for Review requested, Assigned to me, and Outbox: right-aligned dark blue true circles, centered white digits, adequate label clearance, and no badge or number for a known zero. Bind counts to shared account/scope queries, deduplicate item totals, and disclose cached or incomplete coverage. Use persisted outbox fixtures for the UI now; actual cross-action count closure is AC-81 in Sprint 09.
- **S04-T11:** Add native Windows E2E for cached cold start, search, tabs, saved drafts and offline coverage using a real temporary profile and the HTTP simulator. Keep keyboard/input/accessibility paths portable and run the available common tests/builds on macOS/Linux without claiming later-platform GUI parity.

**Required test work:**

- Unit/integration: cache completeness, URL routing, FTS coverage, per-tab state and draft ownership with delayed/partial data.
- Native E2E: cold offline startup, search, tabs/panes, saved draft restoration, scope counts and explicit missing-content states through actual controls.
- Regression: restart during pagination, close/reopen and account isolation; capture native focus/accessibility evidence and current cached-navigation timings.

**Deliverables:** Usable cache-first reader; native discussion renderer; local/remote search; pane/tab workspace; offline draft composer; restoration and cleanup UI; initial performance evidence.

**Exit gates:**

- Pass the assigned AC scenarios with live reads where needed and deterministic failures. Disable networking and reopen downloaded discussions and unsent drafts.
- Exercise page-two failure and process restart during pagination. A partial conversation must remain partial and resume without duplicate rows.
- Show an issue and PR in separate item panes, keyboard navigation from real editors, duplicate-view draft ownership, narrow-window recovery, and independent loading/errors. Keep these general item panes distinct from the bound context/code surfaces added to a PR tab in Sprint 06.
- Measure Section 16 cached-open, local-search, and tab-switch targets on the Windows fixture. Record and fix failures affecting this slice before calling it complete.

**Initial acceptance owner:** AC-01, AC-04, AC-06, AC-07, AC-08, AC-09, AC-10, AC-42, AC-43, AC-46, AC-49.

**Later integration:** Real connectivity closes in Sprint 05. PR resources and their offline-download scope arrive in Sprint 06. The working action outbox arrives in Sprint 07, reviews in Sprint 08, merges in Sprint 09, graph-specific offline views in Sprint 10, and activity-feed integration in Sprint 11. Full offline acceptance closes in Sprint 12. Re-run cleanup for each new draft/action type.


### Sprint 05 Synchronization, connectivity, and activity observation

Relevant cached content stays fresh on configurable schedules without blocking reading or forcing an archive; trustworthy activity observation exists for later notifications.

**Release phase:** Windows first release.

**Prerequisites:** Sprint 02, Sprint 03, Sprint 04.

**Code areas:** sync, github/conditional reads and update feeds, notifications/observation contracts, storage/jobs and baselines, settings, workspace subscriptions.

**Primary requirement IDs:** SYNC-01, SYNC-02, SYNC-03, SYNC-04, SYNC-05, SYNC-06, SYNC-07, SYNC-08, SYNC-09, SYNC-10, SYNC-11, WORKSPACE-10, NOTIFY-02, NOTIFY-13, NOTIFY-14, NOTIFY-15, CONNECT-03.

**Required implementation work:**

- **S05-T01:** Implement workdays, work windows, timezone, work/off-hours intervals, pause, and provisional defaults from SYNC-02. Use a controllable clock, timezone-aware DST behavior, monotonic delays, and coalesced sleep/reconnect catch-up.
- **S05-T02:** Complete one durable, fair scheduler with visible-pane priority, activity monitoring, recent/pinned content, and slower reconciliation under a shared budget. Deduplicate same-resource/revision reads across panes, including cancellation reference tracking.
- **S05-T03:** Centralize read pagination, bounded retries, conditional requests, rate-limit/polling headers, backoff, and restart resumption. Remove monitored repositories from active polling. Never apply a generic retry rule to non-idempotent writes.
- **S05-T04:** Track independent resource-family progress and overlapping update windows. Discover old closed/merged item changes and comment edits without relying on the largest item number or only parent timestamps. Keep full older content on demand.
- **S05-T05:** Implement lightweight item and ordinary-comment observations for unopened items, repository/category baselines, stable event identities, creation-versus-edit distinction, and journal persistence. Define explicit observer contracts for reviews/checks in Sprint 06 and branches in Sprint 10.
- **S05-T06:** Expose last success, pending jobs, throttling, auth failures, and resumable errors without replacing cached content. Confirm application/tray/exit behavior and bounded rendering retention for inactive tabs.
- **S05-T07:** Implement one shared reachability observer with API-based green/red status, separate auth/service/throttle reasons, neutral initial/stale state, observation generations, startup/wake/network-change probes, and bounded retry probes while work awaits connectivity. Respect server budgets and the provisional CONNECT-03 timings.
- **S05-T08:** Publish reconnect/startup/dependency eligibility signals for the outbox. Separate Pause background refresh, Pause queued actions, notification quiet hours, and Work offline. Restore their persisted values before startup probes or dispatch. Scheduled reads keep their work/off-hours policy; submitted actions become eligible promptly when allowed. Prove signals with fixtures now and actual dispatch in Sprint 07.
- **S05-T09:** Resume bounded incremental catch-up from persisted resource-family progress, validators, overlapping update windows, partial pages, and notification baselines. Advance content and successful progress atomically. Use conditional reads and targeted reconciliation where delta support is absent or cursors expire; do not reset the cache, replay old notification baselines, or use a largest-number-only cursor. Record request traces for issue/comment families now; AC-75/76 close after PR-family integration in Sprint 06, and the full lifecycle closes in Sprint 12.
- **S05-T10:** Add deterministic unit/integration tests for schedules, timezone/DST, rate budgets, partial pages, stale/out-of-order connectivity evidence and wake/reconnect. Drive the implemented Windows settings/connectivity controls in a real process and verify persisted pause/offline flags and bounded request traces.

**Required test work:**

- Unit/integration: controlled DST/timezone/work hours, rate limits, coalesced wake, checkpoint recovery and notification baselines.
- Native E2E: manipulate real offline/refresh controls and observe API-based connection evidence; prove stale responses and cancellation do not overwrite current state.
- Request traces: shared polling across panes, old-item updates and bounded reconnect work; actual future mutation replay remains for its feature sprint.

**Deliverables:** Durable shared scheduler; schedule settings; refresh status; conditional/reconciled reads; baselines and event journal; controlled-clock and failure tests.

**Exit gates:**

- Pass AC-11, AC-12, and AC-54, including item 900 already cached while item 12 changes later.
- Show that two views share one in-flight fetch and schedule, closing one does not cancel another's work, and a busy pane cannot starve monitoring.
- Prove initial historical hydration records a baseline without inventing activity. Store a genuinely new comment on an unopened issue as an observation without fetching the full discussion.
- Demonstrate pause, DST/timezone change, sleep/wake, reconnect, throttling, and repository removal. Pop-up delivery acceptance remains open until Sprint 11.
- Pass AC-57 including verified API errors versus transport failure, stale evidence, cancelled requests, out-of-order results, and single-observer behavior across panes. This does not yet close actual outbox replay.

**Initial acceptance owner:** AC-11, AC-12, AC-54, AC-57.

**Later integration:** Sprint 06 connects formal review/request/thread/check observers; Sprint 10 connects branch observers. Full event rule decisions and native delivery close NOTIFY requirements in Sprint 11. The twenty-tab integrated performance gate closes in Sprint 12.


### Sprint 06 PR context, commits, files, checks, and native diffs

A PR review tab presents prominent code beside one smaller body/verified-issue context pane, with cached discussions, reviews, commits, files, and checks.

**Release phase:** Windows first release.

**Prerequisites:** Sprint 04, Sprint 05.

**Code areas:** github/PR resource families, review/comparisons and source mapping, storage/PR ingestion, ui/PR subviews and diff, notifications/review and check observers.

**Primary requirement IDs:** PR-01, PR-02, PR-03, PR-04, PR-05, PR-06, PR-07, DIFF-01, DIFF-04, DIFF-06, DIFF-07.

**Required implementation work:**

- **S06-T01:** Build Conversation, Commits, Files, and Checks subviews with author, requested reviewers, source/target refs, draft/merge state, review results, linked issues, and independent resource completeness.
- **S06-T02:** Read formal reviews and inline threads separately from ordinary comments; preserve replies and resolved/outdated status. Add review text to local search and connect review-request, review, inline-comment, and supported check observation to Sprint 05 baselines.
- **S06-T03:** Implement immutable comparison identity and correct PR merge-base semantics as the PR portion of DIFF-03. Persist source-side and source-line mappings that Sprint 08 will use for review anchors; rendered row numbers are never source identities.
- **S06-T04:** Implement native unified/split diffs, file sidebar, syntax highlighting, selection/copy, collapsed files, next/previous change, keyboard navigation, and early viewed-file storage. Parse/highlight off-thread and virtualize long lists and diff rows.
- **S06-T05:** Fetch missing source context separately, detect truncated/binary/unavailable/API-capped data, and retain an explicit partial state when no practical alternate path exists. Bind checks and statuses to their head revision and expose unknown permissions.
- **S06-T06:** Deduplicate PR resources across panes, preserve independent Conversation/Files selection, ignore stale comparison results, and avoid retaining full render trees for inactive tabs. Measure large-diff responsiveness on Windows.
- **S06-T07:** Complete Download for offline use for a selected PR revision, including discussions/reviews, file/commit metadata, available patches, and chosen optional media/context. Expose partial/unavailable coverage, preserve offline pins and anchors, and pass AC-70 including offline request/cancellation and interrupted download resumption.
- **S06-T08:** Integrate PR resource families with persisted restart checkpoints and conditional/overlapping catch-up. Complete AC-75 and AC-76 using request traces, old-item changes, expired cursors, absent delta support, and interrupted page transactions. Preserve unrelated completed families and distinguish targeted repair from an unjustified full historical download.
- **S06-T09:** Implement the default PR-tab review arrangement: one 30-35% context surface beside 65-70% code, initial View PR body, View issue toggle, resizable/collapsible context, and narrow-window surface switching. Preserve top-level multiple PR tabs and general Open beside without requiring a third issue pane. Persist per-tab mode, selected issue, proportions and independent context/code scroll anchors.
- **S06-T10:** Parse closing-style references from the current native Markdown body, excluding code examples. Support the specified keyword, local/qualified/URL forms, verify actual issue type and scoped identity, and cache body-revision provenance. Offer a picker for multiple verified issues. Fetch cross-repository context under the proper credential profile without enabling repository monitoring or implying an auto-close relationship.
- **S06-T11:** Bind context responses to account/PR/body revision/issue selection/tab generation. Preserve code file, comparison and position while switching context; expose no-link, verification, PR-target, partial, offline, denied, failed, and removed-reference states truthfully. Complete AC-79/80 with delayed responses and cached linkage; use the reader subset of AC-78 now and close actual inline-draft/focus behavior in Sprint 08.
- **S06-T12:** Extend native-process E2E through real PR/body/linked-issue and diff controls. Cover stale context responses, multiple targets, offline cached context, large-diff responsiveness and per-tab state; distinguish simulator/test-build evidence from later exact packaged release smoke.

**Required test work:**

- Unit/integration: comparison/source mapping, PR body references/type verification, review/check observers and partial/API-capped data.
- Native E2E: select PR/file, switch body/verified issue and PR tabs with delayed responses, restore offline context and inspect a large diff without focus/position loss.
- Regression/performance: unified/split diff correctness, requested-review counts, download/cursor recovery and current Windows rendering budgets.

**Deliverables:** Complete PR read screens; native diff viewer; comparison/source mapping model; PR-family cache and observers; large-diff and partial-data fixtures.

**Exit gates:**

- Pass AC-05, AC-22, AC-23, AC-44, and AC-52. Demonstrate the same real PR in Conversation and Files panes without shared UI state or duplicate fetches.
- Exercise delayed responses after a PR switch, missing source context, unknown checks, truncated/binary files, and endpoint caps. Do not mark incomplete content complete.
- Verify unified/split views share correct source mapping and that cached merged state survives a later common issue-shaped closed response.
- Record large-diff measurements and review/check observer baseline tests. Full inline-submission AC-21 remains for Sprint 08.
- Pass AC-75 and AC-76 for the implemented issue/PR reader families. A necessary update-feed first page, overlap, or unchanged revalidation is permitted; verify targeted bounded work instead of asserting a universal exact-delta API.
- Pass AC-79 and AC-80 with multiple and cross-repository references, shared numbers resolving to PRs, code examples, disabled auto-close/non-default branches, missing/offline/denied content, and rapid PR/body/selection changes. Show one bound context/code pair with persistent multiple PR tabs. Actual review-draft stability and complete AC-78 close in Sprint 08.

**Initial acceptance owner:** AC-05, AC-22, AC-23, AC-44, AC-52, AC-70, AC-75, AC-76, AC-79, AC-80.

**Later integration:** Sprint 08 adds inline entry/submission, re-anchoring, full viewed-file/whitespace behavior, and AC-78 context-switch stability with actual review drafts. Sprint 10 completes individual-commit and two-commit comparisons. Notifications consume observers in Sprint 11.


### Sprint 07 Existing-issue edits, comments, and the action outbox

Users submit existing-issue edits and comment work online or offline, inspect or cancel pending actions, and have eligible work automatically delivered after reconnection.

**Release phase:** Windows first release.

**Prerequisites:** Sprint 03, Sprint 04, Sprint 05.

**Code areas:** commands, storage/write journal, github/issue and common-comment mutations, workspace/draft ownership, ui/editors and confirmations.

**Primary requirement IDs:** ISSUE-03, ISSUE-04, ISSUE-05, ISSUE-06, WRITE-01, WRITE-02, WRITE-03, WRITE-04, WORKSPACE-06, WORKSPACE-07, NOTIFY-16, OFFLINE-02, OUTBOX-03, OUTBOX-04, OUTBOX-05, OUTBOX-07, OUTBOX-08.

**Required implementation work:**

- **S07-T01:** Implement the OUTBOX-02 lifecycle and one durable enqueue path for online/offline submissions, separate from unsent drafts. Atomically persist immutable intent and pending projection before acknowledging Queued. Claim work transactionally and retain original account, target, revision, and operation identity independently of tab lifetime.
- **S07-T02:** Enable authorized edits to existing issue titles/bodies, labels, assignees, milestones, and state with actual repository metadata choices. Require the established issue identity and edit base values; a failed edit never creates a replacement issue.
- **S07-T03:** Enable ordinary comment creation and permitted edits on both issues and PRs. Share one editor per draft and pending operation across duplicate views; focus changes and closed tabs cannot retarget or duplicate a submission.
- **S07-T04:** Preserve base values and refresh edited fields before updates. Surface conflicts and unavoidable API races. On a comment-creation or issue-edit timeout, reconcile reliable server evidence or retain outcome unknown; never blindly replay or treat matching text alone as proof.
- **S07-T05:** Exclude New issue forms/templates, shortcuts/commands, creation draft types, create-issue operations, and endpoint flows. Keep ordinary comments and existing-issue editors. Verify AC-82 and preserve supported drafts on all failures. Reconnection automatically resumes eligible submitted actions, while unsent saved drafts remain drafts.
- **S07-T06:** Record successful own actions and distinguish created from edited comments for later notification suppression. Extend capability/identity checks and add authorized live mutation validation in the disposable repository.
- **S07-T07:** Complete the offline Outbox panel with payload/target/revision previews, state/reason/dependencies, attempt times, filters, pending counts, item navigation, Pause/Resume, safe Retry now, and individual cancellation. Keep cancelled text recoverable and synchronize all duplicate views.
- **S07-T08:** Implement meaningful ordered edits on existing targets and comment-create/comment-edit sequencing using confirmed predecessor identity and values, mutation pacing, bounded retries, original-identity authorization repair, and prompt reconnect eligibility independent of off-hours reads. Preserve generic dependencies and per-step receipts for actual review/thread integration and full AC-63 in Sprint 08. Hold affected chains on conflict while unrelated eligible work proceeds.
- **S07-T09:** Implement atomic cancellation before dispatch and honest cancellation-requested handling afterward. Stop further attempts and reconcile possible remote effects. Persist per-step receipts, recover interrupted Sending as unknown when necessary, and test lost responses, crashes, disk errors, and cancel/reconnect races.
- **S07-T10:** Run native offline edit/comment submission, Outbox inspection/cancel and reconnect journeys against the actual provider adapter plus simulator. Inject post-acceptance response loss and cancellation races. Run permitted live edit/comment tests only with the explicit repository and write opt-in; do not add issue-creation setup endpoints.

**Required test work:**

- Unit/integration: existing-issue/comment intents, preflight, dependency ordering, leases, cancellation and unknown-result reconciliation.
- Native E2E: submit offline edits/comments, inspect/cancel pending work, reconnect and verify original target and no duplicate operation after restart.
- Fault/live: post-success lost response, disk failure, full retry burst and cancel races; explicitly approved allowlisted live edit/comment tests only.

**Deliverables:** Offline-capable issue/comment writes; durable outbox dispatcher and inspection/cancellation UI; pending local projections; dependency and reconciliation engine; conflict UI; own-action records; failure/restart/cancellation evidence.

**Exit gates:**

- Pass AC-13, AC-14, and AC-15. Include fake timeout-after-server-success, validation failure, revoked access, concurrent remote edit, and offline cases.
- Demonstrate live permitted existing-issue edits and comment creation/editing in the designated disposable repository. Preserve drafts on failure and after tab closure.
- Switch panes immediately after submission and verify the request target and shared operation identity remain unchanged. Duplicate views cannot send the same pending operation twice.
- Re-run offline draft restoration and cleanup checks. The comment portion of AC-45 passes here; full AC-45 closes only after review and merge integrations.
- Pass AC-58, AC-59, AC-60, AC-61, AC-64, AC-69, and AC-82 using real issue/comment adapters and deterministic failures. Queued submissions resume without a second Submit; drafts remain unsent; cancellation prevents unsent dispatch and never falsely promises remote undo.
- Exercise partial AC-62/67/71 with issue/comment actions; their full cross-action closure remains in Sprint 09. The final integrated durability/offline checks close in Sprint 12.

**Initial acceptance owner:** AC-13, AC-14, AC-15, AC-58, AC-59, AC-60, AC-61, AC-64, AC-69, AC-82.

**Later integration:** Review operations reuse this engine and close actual dependency sequencing AC-63 in Sprint 08; merges reuse it in Sprint 09. Own-action delivery suppression closes in Sprint 11. No generic transport retry may bypass operation-specific reconciliation.


### Sprint 08 Inline PR reviews, approvals, and review drafts

Users can review exact source revisions, retain unfinished reviews, and submit comments, approvals, or requests for changes as themselves.

**Release phase:** Windows first release.

**Prerequisites:** Sprint 06, Sprint 07.

**Code areas:** review, commands/reviews and threads, github/review mutations, workspace/review drafts, ui/inline review and navigation.

**Primary requirement IDs:** REVIEW-01, REVIEW-02, REVIEW-03, REVIEW-04, DIFF-02, DIFF-05.

**Required implementation work:**

- **S08-T01:** Implement inline comment entry, replies, a multi-comment draft review, comment-only submission, approval, request changes, and permitted thread resolve/reopen actions.
- **S08-T02:** Persist path, original/modified side, source line range, commit identity, and original context for each inline draft. Submit against the intended reviewed commit through the shared write engine.
- **S08-T03:** Detect new heads and force-pushes, preserve original anchors, show changed context, and require explicit refresh/re-anchor choices. Never silently attach the draft to different code.
- **S08-T04:** Complete inline thread display, unresolved-thread navigation, and persistent per-revision viewed-file markers. Invalidate reviewed status when the file changes; keep layout and selection stable as annotations arrive.
- **S08-T05:** Support accurate ignore-whitespace viewing when available while preserving unfiltered source coordinates. Provide an explicit limitation if accuracy cannot be guaranteed; never submit a filtered row index as a source location.
- **S08-T06:** Complete shared review-editor ownership, offline restart recovery, draft/outbox-safe cleanup, permissions/identity checks, and keyboard/accessibility behavior in actual review controls.
- **S08-T07:** Queue inline comments, replies, review submissions, approvals, requests for changes, and permitted thread actions offline through the common outbox. Retain the reviewed commit, source coordinates and per-step receipts; automatically execute unchanged valid intent and block changed context. Integrate pending previews, cancellation, dependencies and unknown-outcome reconciliation.
- **S08-T08:** Complete AC-63 using actual supported review/thread steps, such as a pending review requiring its returned server identity before submission. Persist successful receipts, hold uncertain/cancelled predecessors and dependents, and let unrelated actions proceed. A restart resumes unfinished steps without repeating completed mutations; no create-issue chain exists.
- **S08-T09:** Complete AC-78 with an actual inline review draft: toggle PR body/verified issue, pick another linked issue, resize/collapse context, narrow/restore, and switch or restore PR tabs offline. Keep the code comparison/file/scroll, draft identity/anchors and intended target unchanged; asynchronous context responses do not steal editor focus. Retain general multi-item panes and keyboard navigation.
- **S08-T10:** Add actual review-control E2E for anchored drafts, body/issue context switches, submit/approve/request changes, dependency receipts and changed-head holds. Verify source mapping with unit/integration assertions and approved live review cases separately.

**Required test work:**

- Unit/integration: anchors, source sides, changed-head validation, review/thread step receipts and whitespace source mapping.
- Native E2E: actual inline draft, body/issue toggle, restore, submit/approve/request changes, and dependency cancellation/reconnect while preserving intended revision.
- Fault/live: force-push and ambiguous review results plus explicitly approved live review identity/permissions; no automatic approval of new code.

**Deliverables:** End-to-end PR review workflow; durable anchored review drafts; thread actions; viewed-file navigation; revision-change handling; review integration evidence.

**Exit gates:**

- Pass AC-16, AC-17, AC-18, and AC-21, including multiple inline comments, restart, offline recovery, force-push, both diff modes, and source-side correctness.
- Validate approval/request-changes identity in an authorized test PR with suitable authorship and permissions. Missing permission is an explicit unavailable capability, not a successful review.
- Test timeout reconciliation, duplicate-view submission prevention, focus switches, and account isolation for actual review drafts.
- Re-run draft restoration, cleanup, local search of review text, editor keyboard checks, and source-context expansion. Preserve all prior issue-write behavior.
- Pass AC-63, AC-65, and AC-78 and repeat outbox cancellation, identity, lost-response and dependent-step tests for actual review workflows. Full multi-action AC-62/67/71 remains assigned to Sprint 09.

**Initial acceptance owner:** AC-16, AC-17, AC-18, AC-21, AC-63, AC-65, AC-78.

**Later integration:** The review portion of AC-45 is covered here; final multi-action target verification remains in Sprint 09. Repository-history comparisons remain in Sprint 10.


### Sprint 09 Merge workflows

Permitted PRs can be directly merged with fresh server checks, expected-head protection, and accurate locally queued, uncertain, or merged outcomes.

**Release phase:** Windows first release.

**Prerequisites:** Sprint 06, Sprint 07, Sprint 08.

**Code areas:** commands/merge and reconciliation, github/merge capabilities, ui/merge controls, workspace/operation targeting.

**Primary requirement IDs:** MERGE-01, MERGE-02, MERGE-03, MERGE-04.

**Required implementation work:**

- **S09-T01:** Allow an offline merge intent bound to the selected expected head and method. Persist it in the common outbox, then run fresh connected preflight before automatic dispatch. Cached checks or a green connection indicator cannot authorize server execution.
- **S09-T02:** Offer merge, squash, and rebase only when allowed. Send the expected head identity and respect required checks, reviews, branch rules, and organization policy without administrative bypass.
- **S09-T03:** Persist and reconcile intent waiting in the local Outbox, a direct merge request that may have reached or been accepted by GitHub, and a confirmed merge as distinct states. Local cancellation never claims remote undo. Record successful own merges for later suppression.
- **S09-T04:** Reject changed-head results, preserve review context, refresh the displayed state, and require a fresh deliberate user action. Do not retry automatically against a newer head.
- **S09-T05:** Exclude GitHub merge-queue enqueue, management, and withdrawal, and leave auto-merge as future optional scope. If preflight or rejection reveals a required queue, retain the original intent as Needs attention with an unsupported-capability reason. Never bypass the rule or silently enable a different workflow.
- **S09-T06:** Finish cross-pane target tests for comments, reviews, and merges, and verify ordinary issues never receive review/merge controls or PR mutation capabilities.
- **S09-T07:** Complete same-user PAT expiry/replacement tests across issue, review, and merge writers. Preserve cache/profile/item identities, drafts, outbox and sync baselines, retain signed-out/offline/pause holds, and automatically resume only unchanged eligible intent after access repair. Keep in-flight uncertainty visible until reconciliation proves its outcome.
- **S09-T08:** Complete AC-81 for sidebar totals with actual issue/comment/review/merge actions. Outbox includes every unresolved submitted state and excludes drafts, downloads and terminal history. Verify live count changes on enqueue/cancel/confirm, saved-view account/filter scope and distinct items, zero suppression, truthful partial coverage, multi-digit true circles, high-DPI/keyboard accessibility, and shared subscriptions without extra polling.
- **S09-T09:** Run native direct-merge intent/confirmation and changed-head/queue-required scenarios, with lost responses and cross-pane/account switching. Use guarded disposable PRs for explicitly enabled live merges and retain evidence of their effects. Re-run affected cancellation/reconciliation tests and pending-count transitions.

**Required test work:**

- Unit/integration: direct-merge preflight/method/head, queue-required rejection, credentials and Outbox total calculation.
- Native E2E: offline merge intent, changed head, unknown result, cross-pane targeting and all three sidebar badge transitions.
- Fault/live: complete cross-action crash/timeout/cancel/identity matrix; approved disposable live merges record effects and Windows phase evidence remains separate.

**Deliverables:** Direct-merge UI and provider integration; fresh preflight; expected-head protection; uncertain-outcome reconciliation; unsupported merge-queue reporting; full write-target and PAT-renewal tests.

**Exit gates:**

- Pass AC-19, AC-20, AC-45, and AC-53. Use controlled stale-head, locally queued, rejected, ambiguous, and delayed-confirmation fixtures.
- Validate permitted direct merge methods in the designated disposable repository. Pass AC-77 with required-queue rules detected both before dispatch and on rejection; verify no queue enqueue/manage/withdraw, auto-merge enablement, or bypass requests.
- Prove focusing a different pane cannot change any comment/review/merge target, and all writes remain under the active authorized user identity.
- Show remotely unconfirmed or locally queued operations as pending until actual merge confirmation and preserve context after failures. Pass AC-81 across all three sidebar rows, including zero, positive and multi-digit totals and actual pending-state transitions.
- Pass AC-62, AC-66, AC-67, AC-71, and AC-72 across all supported mutation types, covering offline merge replay, unchanged versus changed head, same-user PAT renewal without identity/cache resets, account/scope isolation, Outbox inspection/cancellation, remote acceptance followed by lost response, and failed local completion.

**Initial acceptance owner:** AC-19, AC-20, AC-45, AC-53, AC-62, AC-66, AC-67, AC-71, AC-72, AC-77, AC-81.

**Later integration:** GitHub merge-queue and auto-merge execution are outside this release; required-queue detection and retention of blocked local intent are mandatory. Full notification suppression follows in Sprint 11. Full normal-exit/sign-out lifecycle acceptance closes in Sprint 12.


### Sprint 10 Branch graph and commit comparisons

Users can navigate remote branch ancestry and compare commits through the same native diff system without cloning the repository.

**Release phase:** Windows first release.

**Prerequisites:** Sprint 05, Sprint 06, Sprint 08.

**Code areas:** history, github/refs and commits, storage/commit graph, review/comparison reuse, ui/history, notifications/branch observer.

**Primary requirement IDs:** GRAPH-01, GRAPH-02, GRAPH-03, GRAPH-04, GRAPH-05, GRAPH-06, GRAPH-07, DIFF-03, NOTIFY-04.

**Required implementation work:**

- **S10-T01:** Implement remote branch/tag refs, immutable commits, ordered parents, and a native ancestry graph with branch labels and visible merge relationships. Do not invent a permanent hierarchy of branch names.
- **S10-T02:** Load bounded recent history for the default and selected branches; page older history on demand. Cache records, preserve lanes/scroll/selection, and show unfetched parents as continuation boundaries.
- **S10-T03:** Open selected-commit file changes and comparisons between two commits. For a merge commit, label the selected parent and allow another available parent. Reuse cached diff rendering and explicit comparison semantics.
- **S10-T04:** Reconcile moved/deleted refs and force-pushes independently of immutable commits and review drafts. Maintain revision-aware view state and ignore stale graph/comparison work.
- **S10-T05:** Connect default/chosen branches, branch patterns, and followed PRs to lightweight commit monitoring. Establish per-scope baselines and known new-commit identities without inventing an unavailable push actor.
- **S10-T06:** Measure lazy-history layout and comparison responsiveness. Record GRAPH-07 as an optional future object-cache optimization; API-based history remains sufficient and no clone is required.
- **S10-T07:** Preserve graph selection/scroll, immutable commit content, mutable-ref validation, and branch observation baselines through normal exit and same-user PAT renewal. Integrate targeted restart ref checks without redownloading already retained ancestry; contribute these cases to the final lifecycle matrix.
- **S10-T08:** Add graph/comparison unit and integration fixtures plus native selection, history expansion and comparison navigation E2E. Run cross-target core/build checks and record actual Windows rendering/performance evidence; untested graphics environments remain pending.

**Required test work:**

- Unit/integration: graph parents/lanes, unloaded boundaries, ref changes and exact comparison identities.
- Native E2E: graph selection, history expansion and single/two-commit comparisons with delayed data and offline restart.
- Regression/performance: large graph/diff behavior and remote-branch baseline detection without a full clone; run common CI matrix.

**Deliverables:** Native remote-history graph; lazy ancestry cache; commit and two-commit comparisons; ref reconciliation; branch observer; graph performance evidence.

**Exit gates:**

- Pass AC-24, AC-25, and AC-26 with merges, partial ancestry, moved/deleted refs, and force-push fixtures plus representative live reads.
- Verify PR comparisons, individual commits, and two-commit comparisons retain distinct labels, source identities, options, and cache keys.
- Demonstrate expanding older history without resetting context or losing review drafts. Unloaded parents cannot be shown as proven roots.
- Test watched/unwatched branch observations and historical baselines. End-to-end branch pop-ups remain an acceptance gate in Sprint 11.

**Initial acceptance owner:** AC-24, AC-25, AC-26.

**Later integration:** A bare object cache, local working-tree management, and mandatory cloning are outside the initial scope. Sprint 11 adds rules, grouping, and native delivery for branch observations.


### Sprint 11 Notification rules and native delivery

Users receive only the selected activity, including unopened items, with reliable local history, bot suppression, grouping, and correct workspace activation.

**Release phase:** Windows first release.

**Prerequisites:** Sprint 05, Sprint 06, Sprint 07, Sprint 09, Sprint 10.

**Code areas:** notifications, storage/rules and delivery journal, platform_windows/toasts and activation, ui/activity feed and settings, workspace/deep links.

**Primary requirement IDs:** AUTH-09, NOTIFY-01, NOTIFY-03, NOTIFY-05, NOTIFY-06, NOTIFY-07, NOTIFY-08, NOTIFY-09, NOTIFY-10, NOTIFY-11, NOTIFY-12, NOTIFY-17, NOTIFY-18, NOTIFY-19, NOTIFY-20, PORT-05.

**Required implementation work:**

- **S11-T01:** Integrate all required event categories from the existing observers, including commits/branch updates, ordinary and inline comments, review requests/results, PR opening/readiness/merges, assignments/state, mentions, and supported checks. Monitor unopened items without downloading their full history.
- **S11-T02:** Implement global defaults, per-repository overrides, relevance/followed-item filters, feed-plus-pop-up/feed-only/off, sound off by default, mute, snooze, quiet hours, and own-action suppression.
- **S11-T03:** Implement actor/category/repository rules with stable identities, bot/app metadata, an explicit automated-review rule covering formal and inline reviews, separately configurable ordinary bot comments, and human-reply preservation.
- **S11-T04:** Implement the complete NOTIFY-10 precedence including its second paragraph. Resolve independent category paths, mention exceptions, mute, own actions, quiet hours, and snooze before grouping and deduplication; provide a human-readable rule preview.
- **S11-T05:** Complete the persistent feed and decision/delivery journal, baseline safety, burst grouping, current-preference catch-up, and stable Windows notification identity. Do not promise exactly-once delivery or discovery of every transient state.
- **S11-T06:** Implement native Open/Snooze/Mark read actions and click activation into the correct existing/new tab, item, thread, or comparison, including cold launch. Preserve unrelated panes and drafts, respect Windows notification preferences, and keep the ingestion interface extensible without a hosted relay.
- **S11-T07:** Restore rule settings, event-category baselines, feed read state, deduplication keys, and delivery history after normal exit, sign-out, and same-user PAT renewal. Catch up only eligible observed new activity without recreating historical alerts; complete notification contributions to AC-73/74 for Sprint 12.
- **S11-T08:** Complete the platform notification capability contract and real Windows native delivery/activation/lifecycle tests, including permissions, cold launch, single instance, stable target routing and in-app alternatives for unsupported OS actions. Pass AC-86 on the current packaged development slice and repeat exact published-package checks in Sprint 12.

**Required test work:**

- Unit/integration: every notification category, precedence/mentions/actor rules, baseline/newness, grouping and durable delivery decisions.
- Native E2E/integration: real Windows notifications, permissions, cold/warm activation, single instance and native actions with an isolated profile.
- Lifecycle regression: sleep/wake, close/Quit, optional background operation and exact target/draft preservation; repeat packaged release smoke in Sprint 12.

**Deliverables:** Persistent activity feed; complete notification settings/rule engine; native pop-ups and actions; correct activation; category-by-category baseline and delivery tests.

**Exit gates:**

- Pass AC-27 through AC-34, AC-39 through AC-41, and AC-47. Run real Windows activation tests plus deterministic rule and event fixtures.
- Test each supported category for baseline versus new activity, duplicate polling/restart, own actions, grouping, and capability limits. An old first-time download must never create historical banners.
- Verify bot formal/inline-review suppression preserves ordinary comments when separately enabled, human replies, and the underlying conversation. Test Comments Off with Direct mentions Pop-up and all exception/mute/snooze combinations.
- Demonstrate an unopened old issue receiving a genuinely new comment, a watched branch gaining commits, and an unmerged-to-merged transition, without an initial full archive or a classic token solely for custom pop-ups.

**Initial acceptance owner:** AC-27, AC-28, AC-29, AC-30, AC-31, AC-32, AC-33, AC-34, AC-39, AC-40, AC-41, AC-47, AC-86.

**Later integration:** GitHub inbox read/unread synchronization and hosted always-on delivery remain optional/later capabilities. A fully exited or sleeping computer does not perform local polling. Installer activation is repeated on a clean machine in Sprint 12.


### Sprint 12 Windows testing, simple installation, and first release

A tested Windows-first release satisfies the complete Windows product contract and offers a verified one-to-three-command prebuilt installation/update path.

**Release phase:** Windows first release.

**Prerequisites:** Sprint 01, Sprint 02, Sprint 03, Sprint 04, Sprint 05, Sprint 06, Sprint 07, Sprint 08, Sprint 09, Sprint 10, Sprint 11.

**Code areas:** all modules, platform_windows/installer and lifecycle, diagnostics, docs, tests/performance and acceptance.

**Primary requirement IDs:** AUTH-10, INSTALL-05, INSTALL-06, INSTALL-07, INSTALL-08.

**Required implementation work:**

- **S12-T01:** Measure every Section 16 budget in a release build on the documented Windows machine, including 50,000 summaries, 250,000 comments, large diffs/history, and two visible panes with twenty tabs while syncing. Record p95 cached opening/tab switching and search, startup, RAM/GPU memory, frame times, and package size.
- **S12-T02:** Resolve measured regressions, excessive retained inactive-tab state, redundant polling, UI-thread blocking, and idle redraw. A failed budget stays a failure until fixed or explicitly revised by the product owner.
- **S12-T03:** Complete Windows keyboard/accessibility, real-editor selection/copy, focus, high-DPI, resizing, appearance, crash/draft recovery, and account-isolation checks. Verify migrations on representative previous databases and redact opt-in diagnostics.
- **S12-T04:** Produce a runnable Windows build and locally usable installer. Test install, first launch, credential setup, notification activation into a single instance, optional tray/startup behavior, upgrade, and clean uninstall on a clean supported Windows environment. Preserve user data unless deletion is explicitly requested.
- **S12-T05:** Ship the confirmed PAT login path and record AUTH-10 as inactive future scope. Distribution does not activate OAuth/device authorization. Implement the required package authenticity, simple install/rerun update and publication pipeline without a new login dependency. A hosted automatic updater remains optional; trusted manual update delivery is mandatory.
- **S12-T06:** Run the full applicable acceptance set, preserve prior evidence, and resolve all required open items. Supply source, lockfile, migrations, fixture/test commands without live credentials, build/install/auth instructions, performance results, architecture decisions, and the final requirement/acceptance ledger.
- **S12-T07:** Verify the final cold-start offline matrix for all cached views, drafts, settings, activity feed, and Outbox. Run a 1,000-action backlog while navigating/editing; inject connection flapping, rate limits, crashes before/after possible remote success, full disk, account switching, upgrade and cleanup with pending work. Verify accessible connection announcements and cancellation controls on Windows.
- **S12-T08:** Run AC-73 and AC-74 across the completed product: normal exit retains the protected PAT and every local content/session/progress family; explicit sign-out clears local credentials while retaining readable private data and a durable remote-work hold. Restore the saved workspace before a delayed API response, honor offline/pause controls, reuse same-user identities after PAT repair, and verify bounded incremental request traces with no restart-triggered full archive or baseline reset.
- **S12-T09:** Implement the real published Windows install/update/uninstall path from INSTALLATION_PLAN.md. Deliver a signed/native per-user prebuilt package, OS/architecture/version manifest and verification metadata, one-to-three-command normal path, safe rerun/staged upgrade, data retention and actionable failures. Replace all release placeholders with the actual approved publisher/repository/URLs before publication; missing configuration blocks publication, not independent local packaging/tests.
- **S12-T10:** Run unit, integration, GPUI UI integration, full native-process E2E, installer/failure/upgrade tests and approved live checks using the implemented runner. Test exact published command text and the installed release artifact, with digest/signature/build identity recorded separately from simulator/test builds. Pass AC-91 through AC-99; later macOS/Linux gates AC-87/88 remain pending.

**Required test work:**

- Run all applicable Windows unit, integration, GPUI UI integration, full native-process E2E and approved live tests; require exact command/build/fixture evidence.
- Installer: run exact published commands on clean standard-user Windows, verify authenticity/target checks, rerun, seeded-state update, interruption, safe failure, uninstall/reinstall and installed release smoke.
- Release: performance/accessibility/offline/backlog/crash matrices and Windows phase ledgers must pass; macOS/Linux future rows remain pending, and no plan/ZIP check is an app test.

**Deliverables:** Published installable Windows release; real verified one-to-three-command install/update instructions; release manifest and trust metadata; complete source/lockfile/migrations; implemented test runners/fixtures; native and installer evidence; Windows phase ledgers and performance report.

**Exit gates:**

- Pass AC-35 through AC-38, AC-48, AC-56, AC-68, AC-73, and AC-74, and rerun all Windows-applicable AC-01 through AC-99 across the integrated Windows build. AC-87 and AC-88 belong only to their later platform phases.
- Report pass/fail against every recorded budget, including cached open/tab p95 under 100 ms, local-search p95 under 200 ms, and warm usable startup under 2 seconds.
- Inspect the shipped dependency/process inventory for embedded browser runtimes and complete clean-Windows installation/activation/uninstallation tests.
- No required failure, unimplemented scope, or blocked Windows/live validation may be silently marked complete. Record conditional requirements as inactive only with their condition and reason.
- Pass AC-91 through AC-99 with reproducible commands, per-layer/per-platform evidence, actual native process controls and the exact installed published package. Placeholders, unsigned development artifacts, snapshots and plan/ZIP validation cannot satisfy these release gates.
- Close the Windows rows of the requirement/acceptance/test platform ledgers only when evidence is complete. Do not wait for full macOS/Linux parity to declare a valid Windows release, and do not mark those later rows Pass from Windows evidence.

**Initial acceptance owner:** AC-35, AC-36, AC-37, AC-38, AC-48, AC-56, AC-68, AC-73, AC-74, AC-91, AC-92, AC-93, AC-94, AC-95, AC-96, AC-97, AC-98, AC-99.

**Later integration:** Sprint 13 delivers macOS and Sprint 14 Linux with the same shared product contract and their own native/install gates. Extra architectures/OS variants, attachment uploads, local Git working-tree operations and hosted auto-update remain separately defined scope. The Windows phase requires trusted simple install and rerun updates.


### Sprint 13 macOS parity, native testing, and simple installation

A published macOS release preserves the Windows product workflows while implementing and testing its native integrations and installation.

**Release phase:** macOS release.

**Prerequisites:** Sprint 12.

**Code areas:** platform_macos, shared UI/core regressions, packaging/macOS, tests/native and installer, release documentation.

**Primary requirement IDs:** PORT-07.

**Required implementation work:**

- **S13-T01:** Revalidate the macOS version/architecture default against the pinned toolkit and publish the actual support matrix. Build the shared native app without browser features; implement and exercise renderer, keyboard conventions, text input/clipboard, accessibility, scaling and native preferences on a real target desktop.
- **S13-T02:** Implement Keychain/protected credentials, per-user persistent paths, application identity, single-instance/cold activation, menu-bar/background and close-versus-Quit/startup/wake behavior. Preserve local-first startup, pause/offline settings, signed-out profiles, cache, drafts and Outbox identities.
- **S13-T03:** Complete native notifications and capability-aware actions/replacement behavior in a real correctly packaged app with actual permission handling. Preserve shared filters, bot suppression, feed and exact item routing; no toolkit stub or Windows result is macOS proof.
- **S13-T04:** Verify product parity for reading/search, tabs and PR context/code review, shared item identity, diffs/history, existing-issue edits/comments, reviews and direct merges, offline submission/cancellation/reconciliation and incremental restart. Do not add issue/PR creation or GitHub merge queues.
- **S13-T05:** Produce the real signed/notarized package and verified prebuilt one-to-three-command per-user install/update flow. Test trust checks, unsupported targets, rerun, interrupted upgrade, seeded data migration, uninstall/reinstall, safe failures and exact published commands using INSTALLATION_PLAN.md.
- **S13-T06:** Run applicable unit/integration/native-process E2E/install/live/accessibility/performance scenarios on macOS and record exact package/build/environment evidence. Re-run Windows regressions for changed shared code and complete macOS ledger rows without relabeling earlier Windows evidence.

**Required test work:**

- Rerun all applicable unit/integration/native E2E/live and regression scenarios on actual supported macOS, including keyboard/accessibility, Keychain, lifecycle and notification permissions/activation.
- Installer: exact signed/notarized published artifact and one-to-three-command install/update on a clean user target; seed drafts/Outbox for interruption/migration/uninstall/reinstall cases.
- Measure macOS budgets, close macOS phase rows with package evidence and rerun Windows regressions for shared changes; Linux gates stay pending.

**Deliverables:** Published macOS package and quick-install/update/uninstall documentation; native adapters; macOS phase ledgers; test/package/performance evidence and supported-target statement.

**Exit gates:**

- Pass AC-87 and every macOS-applicable existing product/test/install scenario, including AC-36/38, AC-73/74 and AC-83 through AC-99 except Linux-only AC-88. This is a macOS rerun, not reassignment of original Windows closure owners.
- Drive the real installed macOS app through native E2E, verify protected credentials and notification cold launch/permissions, and pass exact package install/upgrade/trust checks. A headless smoke or development-only bundle cannot close publication.
- Record pass/fail for macOS performance/accessibility budgets and Windows shared-code regressions. Publish only actual validated support combinations; missing release credentials/runner evidence remains a visible blocker for macOS.

**Initial acceptance owner:** AC-87.

**Later integration:** Linux completes in Sprint 14. Keep macOS regression gates active for later shared changes; optional architecture or older-OS expansion needs explicit validation.


### Sprint 14 Linux parity, native testing, and simple installation

A published Linux release supports a bounded tested desktop/session matrix with full product parity, native services, and a simple verified prebuilt installation.

**Release phase:** Linux release.

**Prerequisites:** Sprint 12, Sprint 13.

**Code areas:** platform_linux, shared UI/core regressions, packaging/Linux, tests/native/session and installer, release documentation.

**Primary requirement IDs:** PORT-08.

**Required implementation work:**

- **S14-T01:** Validate and publish the actual Linux distribution/version/architecture/desktop/session and graphics prerequisites. Start from Ubuntu 24.04 x64 GNOME with separately tested Wayland/X11 configurations; do not imply support for every Linux desktop or require a source build.
- **S14-T02:** Implement the supported secure credential service, persistent XDG paths, native input/accessibility/clipboard/scaling, single-instance and external activation, startup/background/close/Quit/wake behavior. If required services are absent or locked, retain offline reading and provide exact guidance without plaintext tokens or silent system modifications.
- **S14-T03:** Verify renderer, large diffs, tabs/context switching and notification delivery/actions in real graphical sessions. Report replacement/retraction/tray limitations explicitly and supply required in-app alternatives while preserving feed, rules and correct item routing.
- **S14-T04:** Verify the same cache/search/data model, review/diff/graph, existing-issue/comment/review/direct-merge, offline Outbox and incremental restart workflows as Windows/macOS. Maintain one shared domain implementation and no browser engine.
- **S14-T05:** Publish verified prebuilt Linux artifacts, a versioned manifest and the one-to-three-command user install/update route. Safely stage/extract/install, check target/runtime prerequisites, preserve user data on rerun/update, and document uninstall/reinstall without silent elevation or package-source changes.
- **S14-T06:** Run all applicable unit/integration/native-process E2E/install/live/performance/accessibility cases in each claimed session/configuration, including exact published-package smoke and failure recovery. Re-run Windows/macOS shared-code regressions and complete Linux phase evidence before any all-platform completion claim.

**Required test work:**

- Rerun all applicable unit/integration/native E2E/live cases on each claimed Linux desktop/session configuration, including secure store, graphics/input, lifecycle and notification capabilities.
- Installer: exact verified published artifact and quick install/update on clean user targets; prove safe prerequisite errors, staging/extraction, data retention and uninstall/reinstall.
- Measure Linux budgets, close Linux phase rows and recheck already released Windows/macOS regressions before declaring all-platform completion.

**Deliverables:** Published Linux package and verified quick-install/update/uninstall documentation; supported service/session matrix; Linux phase ledgers; native/package/performance evidence and all-published-platform regression report.

**Exit gates:**

- Pass AC-88 and every Linux-applicable existing product/test/install scenario, including AC-36/38, AC-73/74 and AC-83 through AC-99 except macOS-only AC-87, on each claimed session/configuration.
- Use real installed native processes for GUI journeys and test exact published commands on clean standard-user targets without Rust tools. Missing graphics/credential/notification prerequisites must fail truthfully without weakening OS security.
- Close Linux rows only with their own evidence; verify Windows/macOS regressions for shared changes. All-platform completion requires all three published-phase ledgers, not one build or a shared-source claim.

**Initial acceptance owner:** AC-88.

**Later integration:** Maintain regression and packaging gates for all released targets. Additional distributions, architectures, desktop environments and OS versions remain explicit compatibility work.

### Phase scope and verification rules

- Preserve PAT authentication, direct merges without GitHub merge queues or auto-merge, retained local state, safe offline Outbox replay, PR context layout and sidebar badge behavior. New issue/PR creation remains excluded.
- Original product acceptance retains its initial closure owner. AC-63/78 close in Sprint 08, AC-79/80 in 06, AC-81 in 09 and AC-82 in 07. Full product behavior is rerun per released OS.
- AC-83/84/89 initially close in Sprint 01; AC-85/90 in 03; AC-86 in 11; AC-91 through AC-99 in the Windows release at 12. AC-87 is macOS-only at 13 and AC-88 Linux-only at 14. Earlier tests cover implemented portions, not future features.
- Native target tests must exercise actual process controls and persistence. Exact installed-package smoke, trust and installer evidence are required before publication even if simulator/test-build E2E passed.
- Simple install/rerun update, compatibility and authenticity checks are required for every published platform. Hosted automatic update infrastructure remains optional. No placeholder installation command is a published result.
- All required Windows rows close before Windows publication. Applicable macOS/Linux rows close at their milestones. Inactive future options retain recorded conditions; a missing runner or credential is Blocked/Not run, not Pass.
- Recheck affected common behavior on already released platforms whenever shared code changes. Measure performance/accessibility on each target rather than reusing Windows evidence.

### Pack contents and evidence

The ZIP contains the master, README, fourteen self-contained sprint files, the report template, testing/installation/cross-platform plans, test_scenarios.csv, initial requirement/acceptance ledgers, and per-platform requirement/acceptance/test ledgers. Use the scenario IDs and fixture catalog for execution details; no application source, binary or implemented test runner is implied by these documents.

Initial ledger status records the first owning phase only. Per-platform rows are authoritative for release claims. Requirement states are Planned, In progress, Partial, Blocked, Verified, or Inactive for documented future scope; acceptance/scenario states are Not run, Pass, Fail or Blocked. Not applicable rows include a reason. Attach exact commands/build/package/OS evidence before changing status.

### Release deliverables

For each released platform provide the shared Rust source, lockfile, target build/packaging recipes, database migrations, implemented test runners and fixtures, prebuilt trusted artifacts, version/architecture manifest, real one-to-three-command install/update instructions, uninstall/recovery documentation, PAT setup guidance, platform-specific native/install/live evidence, ledgers and measured budgets. Preserve state across updates. Missing publisher/URL/signing information must be resolved before public publication; it does not authorize invented credentials or URLs.

## 19 Remaining decisions

These decisions may refine implementation. Cross-platform scope with Windows first and macOS/Linux later, explicit unit/integration/native E2E testing, simple installation, GitHub.com SSO, no embedded browser, PAT authentication, exclusion of GitHub merge queues, retained local data, incremental restart, the PR context/code layout, sidebar badges and exclusion of new issue creation are confirmed and must not be re-asked.

| Question | Provisional assumption and consequence |
| --- | --- |
| Where should release binaries and the installer be published, and who is the publisher? | No release repository/URL or signing identity is supplied. Implement the documented install pipeline with explicit templates now; publication and verified real commands are required release deliverables. Do not invent a live URL or claim installation works today. PAT remains the app login path. |
| Approximately how many repositories and how large is the largest? | Use the reference fixture for engineering tests; tune monitoring and storage after real scale is known. |
| Should history include local branches and working copies? | Remote GitHub branches only initially. |
| Are additional OS versions, architectures or Linux desktops needed? | Windows 11 x64 first, macOS 15+ Apple Silicon later, and a verified Ubuntu 24.04 x64 desktop/session matrix later. Other variants need explicit support validation; they do not expand silently. |
| Are multiple GitHub identities needed simultaneously? | One active identity; account-aware data and credential profiles from the start. |
| Are attachment uploads needed in a later release? | Existing attachments load on demand; uploads remain future scope. Direct merge is the first-release baseline; GitHub merge queues are excluded and auto-merge is optional future work. |

## 20 Primary technical references

These references support platform and API constraints. Product behavior and acceptance budgets above are requirements, not claims that a framework automatically supplies them. Verify current endpoint and dependency details when implementation begins.

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

## 21 Starter prompt for the coding LLM

Use Native GitHub Client Requirements version 1.7 and the complete sprint pack as the contract. Build a native Rust/GPUI application with no embedded browser. Windows is first and highest priority through Sprints 01-12; macOS Sprint 13 and Linux Sprint 14 are required later releases. Keep one shared GitHub/cache/search/Outbox/review core and small platform adapters. Preserve PAT/SSO, offline reading and queued actions, incremental restart, tabs and code-first PR context, sidebar badges, comments/edits/reviews/direct merges, graph and notification behavior. Do not add issue/PR creation, GitHub merge queues or auto-merge.

Read the current sprint, TESTING_PLAN.md, INSTALLATION_PLAN.md, CROSS_PLATFORM_PLAN.md and all relevant traceability/scenario ledgers. Inspect existing instructions and verify current dependency/API compatibility. Implement working slices, relevant unit and real SQLite/HTTP integration tests, native-process E2E, and guarded live tests with reproducible commands/evidence. GPUI snapshots and domain calls are not native E2E; simulator test-build results are separate from exact installed published-package smoke. Do not invent Windows/macOS/Linux or live results.

Deliver a verified one-to-three-command prebuilt install/update route per published platform without end-user Rust tools. Preserve data, credentials, drafts and pending actions across rerun/update. Example URLs in the pack are templates until actual publisher/repository/artifacts exist and exact commands pass clean-machine tests. Hosted auto-update is optional; safe manual update delivery is required.

Update initial and per-platform ledgers plus the sprint report with actual commands, build/package identity, fixtures, OS/architecture, results and remaining work. Later OS rows remain pending until verified. A single-sprint assignment stops after its report; a Windows-release assignment finishes at 12; an all-platform assignment continues through 14 without routine reapproval. Missing prerequisites block only the affected work, never justify a false pass or silent scope deletion.
