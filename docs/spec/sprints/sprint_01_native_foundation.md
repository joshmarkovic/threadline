# Sprint 01: Native feasibility and project foundation

Requirements version 1.7 | Initial sprint status Planned

## Outcome

A runnable native Windows shell proves the chosen stack can support the hardest interface and operating-system integrations.

## Phase and prerequisites

- Release phase: Windows first release.
- Prerequisites: None; inspect existing instructions.
- Code areas: Cargo project, domain, github provider interface, ui, platform_windows, tests/fixtures, docs/architecture.
- Read the master, TESTING_PLAN.md, INSTALLATION_PLAN.md, CROSS_PLATFORM_PLAN.md and all relevant scenario/platform ledgers.
- Use actual prior evidence; a missing dependency remains blocked rather than passed. Later-platform release work does not silently move into the Windows phase.

## Implementation tickets

### S01-T01

Create a buildable Rust application and reusable core with the logical module boundaries in Section 4. Pin a tested GPUI/GPUI Kit dependency set and commit Cargo.lock; document any justified native alternative.

### S01-T02

Define typed provider, operation, resource-identity, cancellation, and structured-error interfaces. Supply a controllable fake provider; establish Tokio background execution and a minimal shared scheduler boundary.

### S01-T03

Build a Windows feasibility screen with long selectable Markdown, a large annotated native diff, two resizable item panes with separate tabs and keyboard focus, a PR-tab fixture with a 33/67 switchable-context/code pair, and a branching graph fixture. Exercise text selection, copying, high-DPI rendering, and focus.

### S01-T04

Prove native Windows notification delivery, click activation, single-instance routing, and opt-in tray lifecycle. This is the early platform contribution to NOTIFY-01, not completion of the notification product.

### S01-T05

Document the Windows reference machine and record numeric idle/peak RAM, GPU-memory, package-size, cold-startup, and scrolling-frame-time budgets alongside Section 16 targets. Add reproducible measurement entry points and inspect the dependency/process inventory for browser engines.

### S01-T06

Define small platform service contracts and isolate Windows APIs/dependency features from shared core/UI behavior. Record Windows-first, macOS Sprint 13 and Linux Sprint 14 targets, native capability gaps, support-matrix defaults and one-to-three-command prebuilt installation constraints. Design artifact/version/architecture/trust metadata now; later package delivery remains in the relevant release sprint.

### S01-T07

Implement the development test-runner entry points and deterministic fixture/clock/API-simulator boundaries in TESTING_PLAN.md. Configure native-target build/core-test CI for all three OS families, start real Windows process launch/focus/quit E2E through native controls, and record early macOS/Linux native smoke results or blockers. Separate GPUI in-process integration, native test-build E2E, exact installed-package smoke and live GitHub evidence.

## Required tests and commands

- Unit: test initial typed identities/errors and controlled-clock/fixture helpers with the implemented runner. Configure build/core CI jobs on all three OS families.
- Native E2E: launch the real Windows process, use actual text/focus/resize controls and quit. Record other-target native smoke attempts separately; no future feature journey is claimed complete.
- Packaging design: check manifest/target/trust decisions and prototype the short-command contract as templates; no installer publication pass occurs here.

Implement and use the canonical runner from TESTING_PLAN.md: unit, integration, ui, native e2e smoke/full, install with an artifact, and separately guarded live tests. These are development commands to build, not commands already supplied by this ZIP. Record actual OS/build/fixture evidence and mark unavailable checks Blocked/Not run. Native test-build E2E and exact published-package smoke remain separate.

Scenario IDs introduced or closing here: UT-021, E2E-001.

Live-provider cases are shared gates on an explicitly authorized host. A later platform can reference existing evidence only with a recorded applicability assessment; rerun changed provider/credential-policy or target-specific behavior. Missing authorization remains Blocked/Not run and never permits unapproved writes.

Use test_scenarios.csv for fixture IDs, expected results, first/closure ownership and evidence. Release sprints rerun all scenarios applicable to their target; earlier sprint scenario lists do not waive regression checks.

## Deliverables

Runnable Windows feasibility build; Cargo.lock; provider fake; architecture decisions; Windows integration notes; baseline measurements and numeric budget file.

## Exit gates

- Run the native slice on the supported Windows target and record commands, machine details, observed behavior, and measurements. A Linux-only build does not satisfy this gate.
- Demonstrate selectable text, independent pane focus, responsive background work, notification activation into one instance, and a documented tray lifecycle without a browser engine.
- Resolve any platform feasibility failure before depending on it. Record the requested numeric budgets; a screenshot alone is insufficient.
- Pass AC-83, AC-84 and AC-89 for the initial Windows/core slice. Show implemented unit/native-smoke commands and actual results; later feature journeys and other-platform release gates remain pending, not incorrectly passed.

## Initial acceptance scenarios owned here

- **AC-83** Sprint 01 establishes the platform interfaces, native-target build/core-test CI matrix and a recorded support plan without importing Windows-only APIs into the shared core. Record actual job results per target and clearly pending later-platform gates; do not infer runtime support from compilation.
- **AC-84** Launch the real native feasibility app on Windows and drive text input/copy, focus, pane/context resizing and quit through native controls. Record renderer/process evidence and early macOS/Linux smoke attempts separately. No browser engine or in-process UI snapshot is presented as native-process E2E proof. Each later released OS repeats this proof on its own target.
- **AC-89** Implement and run the unit-test command on the Sprint 01 core and subsequently introduced domain behavior, with deterministic fixtures, meaningful exit status and machine-readable results. The initial gate proves the runner and current tests, not future feature coverage; every sprint extends its own relevant units.

## Full owned requirements

### FND-01

The shipped application MUST have a native interface and no runtime dependency on a browser engine. Rust with GPUI is the implementation default. A different native approach requires a documented concrete feasibility reason and must preserve the no-browser constraint. An explicit sign-in or external-link action may open the user's existing default browser; this does not embed a browser in the application.

Initial verification owner: Sprint 12. Native feasibility in 01; shipped dependency/process inventory in 12.

### FND-02

Use GPUI and GPUI Kit as the initial GUI choice. Validate selectable rich text, a large annotated diff, two simultaneously visible panes with independent tabs and focus, a PR review fixture with a smaller switchable context surface beside prominent code, Windows notifications, tray behavior, and click activation before building the entire UI. Pin a tested compatible dependency set and commit the lockfile. GPUI Kit provides native text and diff primitives, but GitHub workflow integration remains application work. [S01] [S02] [S03]

Initial verification owner: Sprint 01. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### FND-04

Use Tokio for asynchronous application work and reqwest for HTTP. Use typed request and response models, explicit cancellation, structured errors, and a centralized request scheduler. A different library is acceptable only with a recorded compatibility reason.

Initial verification owner: Sprint 05. Background/provider contracts begin in 01; complete central scheduler in 05.

### PORT-01

Windows is the highest-priority first release, completed through Sprints 01-12. macOS in Sprint 13 and Linux in Sprint 14 are required later product milestones, not optional replacements for Windows. Publish an explicit support matrix covering OS version, CPU architecture, packaging and, for Linux, supported distribution/desktop/session/graphics prerequisites. Use the provisional targets in Section 3 until validated. Keep Windows completion distinct from later-platform readiness; no supported-platform claim is earned by compiling alone, and no later-platform release gate silently moves into the Windows phase.

Initial verification owner: Sprint 12. Define phased scope and support matrix in 01; verify Windows phase statement in 12. macOS/Linux publication closes their separate platform rows in 13/14.

### PORT-02

Share domain models, GitHub integration, cache/search, synchronization, Outbox, review/diff logic, history, notification rules, and workspace state across platforms. Isolate operating-system APIs behind small capability interfaces with conditional dependencies inside platform implementations. Common code must not depend on Windows handles, registry calls, COM types, fixed Windows paths, or platform-specific credential objects. Reuse portable GPUI facilities where suitable rather than wrapping the entire toolkit in a new framework. Test interface contracts with fakes and actual adapters; do not duplicate the product's business logic for each OS.

Initial verification owner: Sprint 12. Introduce platform boundaries in 01 and keep shared modules portable throughout; verify the full Windows implementation boundary in 12 and parity in later platform rows.

### PORT-03

Prove the actual native renderer and necessary GPUI/GPUI Kit components early with selectable Markdown, text input/copy, large annotated diffs, the bound PR context/code layout, resizing, focus, and a graph fixture. Windows proof is a Sprint 01 gate. Configure compilation/core-test jobs for all intended targets and attempt macOS/Linux native smoke launches early on real target desktops; record gaps honestly and resolve them before that platform's release. In-process/headless tests or a successful cross-compile are not native GUI proof. No phase may add an embedded browser engine as a portability shortcut. [S31] [S32] [S33]

Initial verification owner: Sprint 12. Windows native proof begins in 01; expand actual controls as built. Early other-target smoke is tracked separately; full native release evidence closes at each platform gate.

### PORT-06

Implement and test platform-appropriate keyboard shortcuts, text input, accessibility, clipboard, high-DPI/scaling, theme changes, timezone defaults, external-link opening, startup registration, close versus Quit, background residence, sleep/wake and network-change hints. Feed normalized lifecycle events into the shared scheduler. Restore offline/pause settings before probes or dispatch. If a supported environment lacks a tray/menu-bar capability, keep the app reachable and explain background behavior rather than hiding an inaccessible running instance. Closing/quit must preserve drafts, workspace, Outbox and credentials according to the shared lifecycle contract. [S33] [S35]

Initial verification owner: Sprint 12. Keep native interaction/lifecycle boundaries explicit from 01; complete Windows accessibility, restart and installed behavior in 12, and repeat for later targets.

### TEST-01

Implement a documented development test runner and relevant tests in every sprint, not only at release. The required command contract is cargo xtask test unit; cargo xtask test integration; cargo xtask test ui; cargo xtask test e2e --suite smoke or --suite full; cargo xtask test install --artifact <path>; and cargo xtask test live --repo OWNER/REPO with an explicit --allow-live-writes opt-in for approved mutations. Implement these commands or a documented equivalent wrapper before claiming their gates passed. They are planned commands, not tools already provided by this requirements ZIP. Each command returns a meaningful exit status, reports selected/skipped/blocked tests and environment, and writes machine-readable results plus concise logs. End users install prebuilt binaries and need none of this development tooling.

Initial verification owner: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### TEST-02

Unit tests MUST cover nontrivial domain and state-machine decisions introduced by each sprint: shared item identities, absent-versus-null updates, reference parsing/type guards, completeness, schedule/DST rules, notification precedence, draft ownership, dependencies, cancellation, retry eligibility, conflict/revision checks and diff source anchors. Use controlled clocks, seeded fixtures and small independently checkable expectations. Test meaningful boundaries and regressions rather than mirroring implementation lines or pursuing an arbitrary coverage percentage. Add the relevant tests with the feature; future feature tests remain pending.

Initial verification owner: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### TEST-04

End-to-end tests MUST launch and drive the actual native application process through its user-visible controls using supported native automation/accessibility/input, with real local persistence and the implemented HTTP adapter connected to a deterministic test server where appropriate. Start with launch/focus/quit smoke coverage in Sprint 01 and expand to cache-first reading, tab/context switching, offline drafts, queued submissions/cancellation/reconnect, review/merge revision guards, notifications and restoration as those features arrive. Screenshots, GPUI in-process UI tests, callbacks, domain function calls, or a web browser test are not substitutes for native-process E2E. Keep GPUI UI integration tests as an additional layer. Distinguish simulator/test-build native E2E from smoke and release checks of the exact installed published package; the former does not prove packaging, release feature flags or publisher trust. A native automation gap is a visible blocked gate, not a reason to relabel a unit test E2E; record reproducible manual evidence for any inherently manual OS prompt and automate the remaining journey. [S32]

Initial verification owner: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### TEST-05

Provide isolated reproducible fixtures and fault injection for delayed/out-of-order responses, partial pagination, 401/403/404/429/5xx, offline/online changes, sleep/wake, interrupted writes/migrations, full or unwritable storage, process crash before/after dispatch, timeout after remote success, cancelled predecessor chains, changed PR heads and removed links. Exercise exact data/state invariants and request traces, including no blind replay or retargeting. Use temporary per-test profiles, synthetic secrets/data and controlled test servers; reset deterministically without accessing a user's profile or emitting real notifications/writes accidentally. Preserve failure artifacts with secret redaction.

Initial verification owner: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### TEST-06

Configure native-target build and portable-core unit/integration CI jobs for Windows, macOS and Linux from Sprint 01, adding actual suites as modules become available. Native Windows smoke E2E is required as part of each relevant Sprint 01-12 increment; full Windows suites gate Sprint 12. Attempt and record early native macOS/Linux smoke evidence, then require complete applicable native/E2E/install/performance coverage at Sprints 13/14. A later-platform runner or graphics limitation remains visibly Blocked/Not run for that target and must be resolved before publishing it. It does not silently prevent an otherwise complete Windows phase, while a discovered defect in shared correctness still requires repair. Headless rendering, cross-compilation and fake providers must be labeled accurately.

Initial verification owner: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### TEST-08

A sprint report MUST list exact commands, commit/build identity, OS/version/architecture, fixture/seed, test layer and scope, results, failure artifacts, limitations, and requirement/AC/scenario links. Distinguish Pass, Fail, Blocked and Not run; skipped or mocked behavior never counts as a live/native pass. Maintain requirement, acceptance and scenario evidence per release platform, identifying simulator/test builds separately from the exact installed published package and its digest/signature. Every phase gate requires applicable unit/integration/native E2E/install tests, published-package smoke evidence, and targeted performance/accessibility evidence with no unexplained required failures. Rerun affected regression tests when shared code changes; do not claim all-platform completion from a Windows report or from validation of this planning package.

Initial verification owner: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### INSTALL-01

Each published platform MUST offer an end-user installation and rerunnable update path requiring one to three documented commands in its normal shell. Install prebuilt release artifacts; do not require Rust, Cargo, a compiler, Git checkout, or local source compilation. Use a per-user install by default without administrator privileges where the supported platform permits. Normal OS trust/permission prompts and the app's later PAT setup are allowed and must be explained; one to three commands does not promise zero interaction. The flow assumes the explicitly documented supported OS/system prerequisites, not hidden manual toolchain setup.

Initial verification owner: Sprint 12. Define short-command/prebuilt/compatibility/trust contracts in 01; deliver actual Windows artifacts, exact published command tests and release verification in 12. macOS/Linux counterparts close in their own rows.

### INSTALL-02

Before changing the installed application, detect and validate OS, architecture, minimum supported version and required runtime/graphics/desktop prerequisites. Select the exact compatible artifact from a versioned release manifest; reject unsupported combinations with an actionable explanation. Avoid silent architecture substitution, undocumented compatibility assumptions, or downloading a different channel/version than requested. App dependencies should be bundled where practical; unavailable system prerequisites produce precise repair guidance instead of silent system modifications.

Initial verification owner: Sprint 12. Define short-command/prebuilt/compatibility/trust contracts in 01; deliver actual Windows artifacts, exact published command tests and release verification in 12. macOS/Linux counterparts close in their own rows.

### INSTALL-03

Verify artifact integrity and publisher authenticity before execution or installation using the platform's appropriate signing/trust mechanism or a verified signed release manifest with an established trust root. A checksum alone establishes integrity, not publisher authenticity. Download over authenticated transport, validate the selected version/manifest/artifact relationship, fail closed on mismatch or failed verification, and clean up failed staging files. Do not require weakening OS security, disabling certificate checks, execution-policy bypass, or unauthenticated remote-script execution. The documented short command flow must preserve this trust chain.

Initial verification owner: Sprint 12. Define short-command/prebuilt/compatibility/trust contracts in 01; deliver actual Windows artifacts, exact published command tests and release verification in 12. macOS/Linux counterparts close in their own rows.

## Integration work owned elsewhere

### PORT-05

Keep notification eligibility, filtering, grouping, delivery history and item routing shared, with per-OS notification/activation implementations. Verify permissions, native delivery, actions, cold/warm launch, single-instance routing, and any supported identity/replacement semantics in the packaged target app. Unsupported OS capabilities must have truthful capability reporting and usable in-app alternatives for actions such as Snooze and Mark read; no silent loss of activity or false replacement/retraction promise. Define supported desktop behavior explicitly before publishing, preserving the persistent feed and correct Open behavior. A toolkit method or Windows test does not prove another OS's integration. [S34]

Initial owner: Sprint 11. Initial verification: Sprint 12. Windows notification feasibility in 01, actual product delivery in 11, and exact installed-package activation in 12; later OS parity closes at its release.

### PR-05

The default PR review arrangement MUST show the native file list/diff prominently beside exactly one smaller context pane bound to the same active PR. At a usable window width, start with approximately 33% of the review content width for context and 67% for code, within a 30-35% / 65-70% range, excluding global navigation chrome. Context may be resized or collapsed; restore its last proportion. Provide View PR body and View issue controls in that one pane, with PR body as the initial mode for a newly opened review tab. They replace the context content rather than adding a simultaneous third pane. Preserve Conversation, Commits, Files, and Checks navigation and multiple PR tabs. At narrow widths, show a usable focused surface with an explicit way to switch context/code and restore the wider arrangement without losing either surface's state.

Initial owner: Sprint 06. Initial verification: Sprint 08. Native layout feasibility in 01 and workspace primitives in 04; bound context/code PR reader in 06; full AC-78 with actual inline drafts, focus, narrow layout and offline restore closes in 08.

### NOTIFY-01

Provide native desktop notifications and a persistent in-app activity feed, with Windows integration first and macOS/Linux integration in their release phases under PORT-05. The native integration must support opening the correct item when clicked, single-instance behavior, and useful notification identity or grouping. Validate the chosen Rust and native API integration early, with Windows first and actual target evidence before each later release. [S24]

Initial owner: Sprint 11. Initial verification: Sprint 11. Native notification/tray/activation feasibility in 01; persistent feed, rules and production activation in 11.

### UX-01

Support platform-appropriate keyboard navigation, focus indication, copy and paste, high-DPI displays, light and dark appearance, and sensible resizing. Test real text editors and review controls with the accessibility features used by the target audience on each published OS. Windows is verified first; macOS/Linux parity closes in their release phases.

Initial owner: Sprint 04. Initial verification: Sprint 12. Keyboard, accessibility, DPI and appearance checked as native controls arrive; integrated release verification in 12.

## Verification due for this phase

FND-02.

Use requirement_platform_traceability.csv for applicability and release-gate evidence. Initial ledger statuses refer only to their initial phase; no single Windows result closes all platforms. Keep future options and unavailable services explicit.

## Later scope and handoff

Production authentication starts in Sprint 03; graph semantics in Sprint 10; notification rules, feed, and full activation in Sprint 11. The scheduler contract is completed in Sprint 05. Repeat the no-browser inventory for the shipped build in Sprint 12.

## Coding LLM instruction

Implement Sprint 01 against v1.7. Keep the app buildable and preserve the current product constraints. Complete listed implementation and test work, use actual native controls for E2E, protect original targets/drafts/Outbox state, and add only the named platform integrations. Do not add issue/PR creation, merge queues or browser engines. Keep secrets out of code/results; live writes require explicit allowlisted authorization. Implement the short verified prebuilt install/update contract for the relevant publication phase and never claim placeholder commands runnable. Update initial/per-platform/scenario ledgers with commands, environment, package identity and evidence, then use SPRINT_REPORT_TEMPLATE.md. Continue only within the assigned sprint/Windows/all-platform scope without new routine approvals.

## Technical sources

- [S01 GPUI framework and Windows rendering](https://gpui.rs/examples/)
- [S02 GPUI Kit native diff component](https://gpui-kit.com/component/diff/)
- [S03 GPUI Kit selectable rich text](https://gpui-kit.com/component/text-view/)
- [S24 Windows desktop notification APIs](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/)
- [S31 GPUI Kit installation and target prerequisites](https://gpui-kit.com/docs/installation/)
- [S32 GPUI Kit testing facilities and limits](https://gpui-kit.com/docs/test/)
- [S33 GPUI Kit accessibility](https://gpui-kit.com/docs/accessibility/)
- [S34 GPUI Kit system notification capabilities](https://gpui-kit.com/docs/system-notification/)
- [S35 GPUI Kit packaging](https://gpui-kit.com/docs/packaging/)

Additional current primary sources and platform/install/test constraints are in the three companion plans. Verify pinned tool/API behavior during implementation.
