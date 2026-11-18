# Sprint 01 native foundation

## Scope

The sprint pack assigns a new implementation to Sprint 01. This sprint builds a Windows-first feasibility workspace and starts the portable application and test architecture. The app version is 0.1.0. The workspace uses synthetic station-labs/relay content with seed 12345. Use the [foundation decisions](../architecture/foundation.md) for the support targets and [Sprint 01](../spec/sprints/sprint_01_native_foundation.md) for the gates.

## Delivered behavior

- Two independently focused and resizable panes with their own tabs and drafts. The left pane shows a long selectable Markdown discussion and branching history. The right pane shows a native annotated diff beside a switchable PR body or related issue. The context starts at 33% of the review split.
- A portable Rust core with resource IDs, request revisions, cancellable work, a bounded Tokio scheduler, a controllable fake, and a reqwest integration boundary tested against a real loopback HTTP simulator. The synthetic response carries the repository ID and item number. GitHub authentication and production repository selection start in Sprint 03.
- A Windows adapter for single-instance activation and opt-in tray residence. The development notification uses the Windows shell notification area and routes a clicked fixture notification to the review. The adapter distinguishes a queued request from a shell delivery callback.
- A Cargo test interface with case filters, replay seeds, JSON and JUnit results, guarded live selection, Windows and X11 native drivers, and a Windows measurement command. CI declares native build and portable test jobs for Windows x64, macOS Apple Silicon, and Ubuntu x64.

## Environment and build

Rust 1.98.1. GPUI Kit 0.10.0 and GPUI snapshot 0.3.12. The Windows development executable was cross-built from WSL with LLVM 19.1.7 and Windows SDK 10.0.26100, then executed in Windows 11 Enterprise 23H2 x64 build 22631.7652, active RDP session 2. The development renderer compiles HLSL from source at launch. The cross-build mapped that compile-time path to the same pinned sources through WSL's Windows-accessible filesystem. A native release build uses the Windows SDK shader compiler during the build.

The Linux development test host is Debian 13.7 x86-64 in WSL2, with an X11 window reached through WSLg. This is early Linux feasibility evidence. The Ubuntu 24.04 GNOME Wayland and X11 matrix remains the Sprint 14 release target. A macOS desktop and an executed three-target CI workflow were unavailable in this local repository.

## Test evidence

The test commands below run from the repository root. The native commands use the instrumented feasibility executable and a temporary synthetic profile. GPUI in-process tests exercise production views in the framework harness. Native smoke launches a separate process and drives the actual controls through the operating system.

## Installation and publication

Sprint 01 records the target, artifact metadata, trust, and update design in [foundation decisions](../architecture/foundation.md). The [installation plan](../spec/INSTALLATION_PLAN.md) contains provisional one-to-three-command templates. The Windows installer, publisher identity, stable URLs, exact command tests, and data retention checks belong to Sprint 12. The macOS and Linux equivalents belong to Sprints 13 and 14.

## Phase traceability

The [initial requirements](../spec/requirements_traceability.csv), [initial acceptance cases](../spec/acceptance_traceability.csv), [test scenarios](../spec/test_scenarios.csv), and [per-platform requirements](../spec/requirement_platform_traceability.csv), [acceptance](../spec/acceptance_platform_traceability.csv), and [test evidence](../spec/test_platform_traceability.csv) track the assigned owners separately for each platform.

## Measurements

The [numeric Windows ceilings](../architecture/windows-budgets.json) cover working set, GPU memory, package bytes, cold start and scroll time. The development test measures the synthetic feasibility workspace. Section 16's cached startup, search and larger workload budgets remain part of later release verification.

## Handoff

Sprint 02 introduces SQLite migrations, shared issue and PR identity, cache coverage, drafts and the durable Outbox. Sprint 01 native gates and associated open evidence must be resolved before the foundation is treated as verified.
