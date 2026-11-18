# Cross platform delivery plan

Version 1.7 companion specification

## Confirmed direction

The application will support Windows, macOS, and Linux, with Windows taking priority. Sprints 01 through 12 produce the first Windows release. Sprint 13 adds the macOS release and Sprint 14 adds the Linux release. Shared architecture, core testing, and build visibility begin in Sprint 01, so later ports do not require duplicating the cache, GitHub, review, or Outbox implementation.

This plan implements PORT-01 through PORT-08 and complements TESTING_PLAN.md and INSTALLATION_PLAN.md. It preserves the no-browser-engine requirement, PAT authentication, persistent local data, offline behavior, the review context/code layout, and the exclusion of New Issue creation.

## Support matrix and release gates

| Phase | Default support target | Required completion gate | Earlier development expectation |
| --- | --- | --- | --- |
| Windows first release | Windows 11 x64 | Sprint 12, including current-platform requirements and unit/integration/native E2E/install tests | Main development and GUI validation platform from Sprint 01 |
| macOS release | macOS 15 or later on Apple Silicon | Sprint 13, including product parity, native integration, packaged E2E and installation | Core tests and app compile checks from Sprint 01; targeted real GUI proof as the platform foundation is established |
| Linux release | Ubuntu 24.04 x64 on GNOME, with Wayland and X11 recorded as separate desktop test configurations | Sprint 14, including product parity, native integration, packaged E2E and installation | Core tests and app compile checks from Sprint 01; targeted real GUI proof and dependency assessment early |

The OS versions and CPU targets are implementation defaults pending the feasibility evidence. Windows priority and later macOS/Linux support are confirmed. Supporting Windows ARM64, Intel Macs, Linux ARM64, additional distributions/desktops, or older OS versions requires an explicit compatibility decision and its own tests. A successful x64 build cannot certify ARM64. [P1]

The current GPUI Kit installation guide lists macOS 15 or later and verified Linux development dependencies for Ubuntu 24.04. Linux window rendering requires a working graphical session and GPU driver. These framework facts inform the proposed targets but do not prove that this app has passed its target gates. [P2]

After a platform reaches its release gate, future shared changes must pass that platform's required regression gates before publication. Before that milestone, its full product and release checks are tracked as future-phase work, not silently waived and not falsely required for the first Windows release. An early compile/core-test failure must remain visible with an owner and cause; the report must distinguish a shared-code portability regression from a temporarily unavailable platform runner.

## Shared architecture

Keep one implementation of the following behavior:

- Domain models, GitHub repository/item identity, shared issue/PR numbering, permission decisions, and linked-issue resolution.
- GitHub REST/GraphQL clients, PAT profile routing, conditional reads, pagination, rate budgets, and SSO/error interpretation.
- SQLite schema and migrations, FTS search, durable drafts, resource coverage, incremental checkpoints, and notification baselines.
- Outbox state machine, operation identities and receipts, cancellation, dependency sequencing, unknown-outcome reconciliation, and revision protection.
- Schedules, timezone-aware work/off-hours logic, retry policy, offline controls, and reconnection eligibility.
- Tabs, review context/body/issue selection, comparison identity, diff parsing, code mapping, branch graph data/layout, and persisted workspace state.
- Notification observation, bot suppression, filters, grouping, and the persistent activity feed.

The UI uses shared GPUI views and platform-appropriate conventions. GPUI draws the interface without embedding a browser; it does not automatically turn every custom control into an OS-native widget. Keep optional webview and browser-backed capabilities out of the shipped dependency feature set and verify the actual binary/runtime dependency closure. [P3]

Each installation retains its own account-aware local data and resumes incremental synchronization from its own checkpoints. This work does not add database roaming or multi-computer draft synchronization. Do not put live SQLite/WAL files into a shared network folder as a substitute for a designed synchronization protocol.

## Platform service boundaries

| Boundary | Shared contract | OS adapter responsibilities |
| --- | --- | --- |
| Protected credential store | Read/write/remove a named credential for a scoped account/profile; report unavailable/locked/denied distinctly | Windows Credential Manager, macOS Keychain, or the chosen Linux Secret Service backend; no plaintext fallback |
| Application paths | Resolve stable data, state, configuration, disposable cache, log, and install locations | Native known folders and XDG conventions; path encoding, permissions, override validation, and migration from prior app locations |
| System notifications | Deliver a durable event reference, declare action/replacement/retraction capabilities, route activations | OS identity, permission state, daemon/service interaction, action support, and cold-start activation into the existing/single app instance |
| Lifecycle and startup | Distinguish window close, explicit Quit, optional background residence, and startup-at-login | Tray/menu-bar availability, desktop registration, orderly shutdown, wake and network-change hints, and native single-instance primitives |
| Desktop preferences | Obtain system timezone, appearance, scale and keyboard conventions | Native observation and mapping; preserve explicit user overrides |
| Native interaction | Accessible controls and correct user-driven input/focus/navigation | Clipboard, rich-text selection, IME, screen-reader integration, menus, shortcuts and high-DPI behavior |
| Packaging | Stable app identity/version/channel and update/data-retention contract | Artifact format, signatures, installer registration, desktop entries, launch integration and platform upgrade verification |

Use small Rust interfaces or focused modules where an OS boundary exists. Prefer GPUI's appropriate portable facility where it meets the product contract. Avoid a large generic wrapper that duplicates the GUI toolkit. Keep native handles, OS-specific dependency features, and conditional code inside the adapter layer; the domain, storage, sync, and command modules must compile without Windows APIs.

Mock adapters support unit and integration tests. They must not make a native service appear verified. A mock notification click, mock keychain, or simulated GPUI window cannot close a real target integration gate.

## Paths and durable storage

Use platform path types and native directory discovery. Do not construct filesystem paths with fixed backslashes, assume ASCII usernames, or persist a Windows-specific timezone identifier as the only schedule representation. Test spaces, Unicode, long supported paths, user-directory overrides, permission denial, and full-disk failures.

Keep the durable SQLite database, drafts, pending and uncertain Outbox work, receipts, and required recovery state in a persistent data location. They are not disposable cache. On Linux, honor valid XDG_DATA_HOME, XDG_CONFIG_HOME, and XDG_STATE_HOME values; cache/runtime directories have different retention rules. [P4]

The protected credential remains outside SQLite. If the OS credential service is locked, absent, or denied, cached content stays available and remote work remains held with a clear reason. Never migrate the PAT to an unprotected file to make a port appear functional.

## Notification and lifecycle parity

Keep one rule engine and activity feed. The delivery adapter reports supported capabilities so the UI can present meaningful alternatives. Required notification events, bot suppression, and quiet-hour decisions cannot vary accidentally by OS.

Document and test each target's actual delivery conditions. GPUI Kit's system notification documentation identifies permission/bundle conditions on macOS and a session service requirement on Linux; adapter features such as replacement and retraction can vary. Revalidate these details against the pinned toolkit version. [P5]

When a target cannot expose an OS notification action, keep the equivalent action in the in-app feed and disclose the limitation. Use a custom native adapter when needed to meet a release requirement that the pinned toolkit cannot provide. Do not claim exactly-once delivery or treat OS acceptance of a notification request as proof that a banner appeared.

Click activation must restore the correct account/item/tab/thread and preserve other drafts. Test active-window, background-window, no-window, and cold-launch cases. An unavailable tray must not leave the app unexpectedly inaccessible. Startup-at-login remains opt-in. Closing a window and explicit Quit must have documented behavior, and both must preserve durable local state. On wake, run the existing bounded incremental catch-up and eligible queued actions while honoring restored offline and pause controls.

## Testing and sprint ownership

| Sprint | Additional portability deliverables and gates |
| --- | --- |
| 01 | Establish the three-target build/core-test matrix, platform interfaces, target defaults and dependency feature inventory. Complete the Windows native proof for text selection, input, large diff, context toggle, resizing and accessible controls; record early macOS/Linux proof results or exact open feasibility items. |
| 02 | Keep schema, migration, draft/outbox persistence and path contracts portable. Run storage tests against real temporary SQLite databases on the core matrix. |
| 03 | Add the credential-store contract and Windows native credential tests; provide macOS/Linux adapter implementations or explicit owned tasks for their release phases. Keep denied/locked-store behavior testable without live credentials. |
| 04 through 10 | Keep shared UI and domain changes buildable. Exercise deterministic native GUI workflows on Windows and run the shared unit/integration suites on all configured core runners. Track input, rendering and platform gaps for later target completion. |
| 11 | Complete Windows notification/activation/lifecycle behavior and cross-platform capability contracts. Keep each target's delivery and system-service limitations explicit. |
| 12 | Complete Windows release performance, accessibility, installer/upgrade, offline cold-start and installed native E2E gates. Publish the Windows support statement only after evidence passes. |
| 13 | Complete macOS adapters, real desktop behavior, feature parity, signed/notarized distribution, quick install, retained-data upgrade and installed native E2E. Rerun the shared acceptance suite on the supported macOS target. |
| 14 | Complete Linux adapters and dependency handling, GNOME Wayland/X11 verification, feature parity, quick install, retained-data upgrade and installed native E2E. Rerun the shared acceptance suite on the supported Linux target. |

For AC-83 through AC-88 and the target-specific reruns of installation acceptance, the evidence must name the OS version, architecture, display environment, app commit/version, dependency lockfile, commands, driver/service context as relevant, result and unresolved limits. A missing runner is Blocked or Not run, not Pass. Build success is recorded separately from native GUI and package success.

## Final release parity

Before calling a later platform supported, verify the same cached issue/PR reading, search, tabs/context/code review, local drafts, Outbox inspection/cancellation, reconnect replay, direct merges, branch/diff views, notification rules, connection indicator, and retained restart state required on Windows. No new issue creation or merge-queue workflow may appear in a port.

Performance targets remain product targets but must be measured on a documented representative machine for each OS. Do not reuse Windows timing or RAM evidence as proof for another target. Record any target-specific limitation in the release notes and capability UI, with a clear decision on whether it is acceptable for that release.

## Sources

- [P1 GPUI Kit packaging and architecture-specific releases](https://gpui-kit.com/docs/packaging/)
- [P2 GPUI Kit installation prerequisites](https://gpui-kit.com/docs/installation/)
- [P3 GPUI rendering and accessibility](https://gpui.rs/examples/)
- [P4 XDG base directory specification](https://specifications.freedesktop.org/basedir/latest/)
- [P5 GPUI Kit system notifications](https://gpui-kit.com/docs/system-notification/)
