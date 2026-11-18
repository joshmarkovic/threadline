# Sprint 01 native foundation

November 18, 2026 · Threadline 0.1.0

The sprint pack starts a new implementation at Sprint 01. Threadline has a runnable Rust and GPUI workspace, and the native Windows and Linux smoke tests passed. The Windows notification click, high-DPI desktop run, scroll budget, and hosted build matrix still need evidence or resolution.

## Delivered behavior

| Ticket | Result |
| --- | --- |
| S01-T01 | The Rust workspace builds with Cargo.lock, GPUI Kit 0.10.0 and GPUI 0.3.12. The Windows and Linux feasibility binaries ran as native processes. |
| S01-T02 | The portable core verifies typed repository and item identities, revisions, structured errors, cancellation and stale responses. A seeded fake, bounded Tokio scheduler and loopback HTTP simulator exercise background work. |
| S01-T03 | The workspace has selectable Markdown, a virtualized annotated diff, two resizable panes with independently focused tabs and drafts, a branching history fixture, and a 33/67 PR context and code split. Both splitters and OS clipboard behavior passed on Windows at 96 DPI. Simulated 2× geometry passed in the GPUI harness. Native high-DPI evidence remains open. |
| S01-T04 | Windows opted in to tray residence, minimized on Close, and restored the original process through the actual tray icon. A second launch forwarded to the active review in the original process. The shell emitted its banner-shown callback. The Windows automation could not locate the banner's native click target. |
| S01-T05 | The native benchmark and numeric ceilings are in place. Startup and working set passed on the reference VM. Both scrolling surfaces exceeded the 20 ms budget. |
| S01-T06 | Portable service contracts, platform ownership, a support matrix, artifact metadata and short, verified installation templates are documented. |
| S01-T07 | Cargo commands validate selection and seed, report JSON and JUnit, and run isolated tests. The Windows and X11 input drivers exercised real windows. Windows, Apple Silicon macOS and Ubuntu x64 build and core-test jobs are configured. Hosted job execution and macOS desktop access remain open. |

The app opens synthetic station-labs/relay content, fixture revision `s01-feasibility-v1`. SQLite, live GitHub authentication, durable drafts and the Outbox have their own implementation sprints in the pack.

## Environment and build

- **Windows** ran in Windows 11 Enterprise 23H2 x64 build 22631.7652, active Remote Desktop session 2. The VM has eight logical Xeon Platinum 8370C processors, 32 GiB RAM and a Microsoft virtual SAS disk. The driver observed 2544 × 1353 pixels and 96 DPI. The loaded modules included Direct3D 11, DirectWrite and the Windows software renderer `d3d10warp.dll`.
- **Linux** ran in Debian 13.7 x86-64 on WSL2 kernel 6.18.40.1 with an X11 window through WSLg. The Linux release target is Ubuntu 24.04 GNOME with separate tests for Wayland and X11.
- **Toolchain** used Rust 1.98.1 and lockfile SHA-256 `00d232dc36582a00dddaddfaf4f4225b9be0af05c50dd26803bd7054e8f30f86`. Windows binaries were cross-built with LLVM 19.1.7 and Microsoft SDK 10.0.26100, then executed on the Windows desktop. For this cross-build the release profile kept debug assertions on in `gpui-pre-windows` and mapped its HLSL source path to the equivalent Windows-accessible WSL path. The Windows-native release build will compile shaders using the SDK.

The final Windows and Linux app smoke and the optimized Windows benchmark used `d7d6702`, including the asynchronous telemetry writer. The final Windows smoke used the guarded input helper in `52edeec`. Evidence records the SHA-256 of each executable. The [architecture decisions](../architecture/foundation.md) record the target matrix and distribution contract.

## Test evidence

Commands use deterministic seed 12345. The Windows host used the same Cargo runner binary via `xtask.exe` with a staged local workspace, `--binary` pointing to the instrumented executable, and Windows PowerShell 5.1 with Win32 input. Linux used `cargo xtask` from the repository with the X11 tools listed in the [driver instructions](../../tools/native/README.md).

| Layer | Command or execution | Result and evidence |
| --- | --- | --- |
| Unit | `cargo xtask test unit --seed 12345 --report-dir artifacts/verification` | Pass. 15 tests including UT-021, core behavior and diff fixture parsing. [JSON](evidence/linux-unit/result.json) · [JUnit](evidence/linux-unit/junit.xml) |
| Loopback HTTP and platform contracts | `cargo xtask test integration --seed 12345 --report-dir artifacts/verification` | Pass. Five HTTP simulator and two platform contract tests. Real SQLite starts in Sprint 02. [JSON](evidence/linux-integration/result.json) · [JUnit](evidence/linux-integration/junit.xml) |
| GPUI in-process | `cargo xtask test ui --seed 12345 --report-dir artifacts/verification` | Pass. Three tests of selection, focus and state, and splitter geometry. [JSON](evidence/linux-ui/result.json) |
| Native Windows Rust harnesses | `cargo test --locked --offline --target x86_64-pc-windows-msvc -p threadline-app -p threadline-core -p threadline-platform -p xtask --no-run`, then each applicable executable on Windows with `--test-threads=1` | Pass. 26 tests spanning portable code, Win32 contracts, runner checks and the GPUI integration harness. [Console results](evidence/windows-cargo-tests.txt) |
| Windows separate process | `xtask.exe test e2e --suite smoke --binary <windows-exe> --seed 12345 --report-dir <reports>` | Pass, E2E-001 and AC-84. Verified Markdown copy, typing while preview loaded, pane focus, stable draft on tab change, linked issue context, both pointer splitters, single-instance forwarding, Quit and relaunch. [Native actions](evidence/windows-e2e/native.json) · [Screenshot](evidence/windows-e2e/interaction.png) · [JUnit](evidence/windows-e2e/junit.xml) |
| Windows tray and banner | `xtask.exe test e2e --suite full --case S01-WINDOWS-SHELL --binary <windows-exe> --seed 12345 --report-dir <reports>` | Blocked. The app remained in the tray after Close, restored from a click on its icon, and received `NIN_BALLOONSHOW`. UI Automation could not identify the banner click target. [Native actions](evidence/windows-shell/platform-native.json) |
| Linux separate process | `cargo xtask test e2e --suite smoke --binary <linux-exe> --seed 12345 --report-dir artifacts/verification` | Pass for early WSLg X11 feasibility. Launch, selection, typing, background work, tab and context switches, both pointer splitters and Quit ran against the actual window. [Native actions](evidence/linux-e2e/native.json) · [Screenshot](evidence/linux-e2e/interaction.png) |
| macOS desktop | Native runner or manual process run on an Apple Silicon macOS 15 desktop | Blocked. A macOS desktop was unavailable and the driver supports Windows and X11. |
| Unknown selector | `cargo xtask test unit --case UT-unknown --report-dir artifacts/verification` | Expected exit 1, with Fail in JSON and JUnit. [JSON](evidence/runner-invalid/result.json) |
| Installer and upgrade | `cargo xtask test install --artifact docs/architecture/windows-budgets.json --report-dir artifacts/verification` | Expected exit 78 from the development guard. An inert file exercised reporting. Exact installer and retained-data verification are assigned to Sprint 12. [JSON](evidence/installer-guard/result.json) |
| Approved live GitHub | `cargo xtask test live --repo test-fixture/not-authorized --allow-live-writes --report-dir artifacts/verification` with authorization variables unset | Expected exit 78. The runner blocked all live cases before accessing any repository. [JSON](evidence/live-guard/result.json) |

`cargo fmt --all --check` and `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` passed. The observer's real-file shutdown test passed with `cargo test --locked -p threadline-app --features e2e --lib`.

The dependency inventory used `cargo tree --locked --offline -p threadline-app --target <triple> -e normal,build --prefix none` for `x86_64-pc-windows-msvc`, `aarch64-apple-darwin` and `x86_64-unknown-linux-gnu`. The native target trees had zero browser runtime matches. The [Windows loaded module and child-process inventory](evidence/windows-e2e/native.json) and the [Linux mapped module inventory](evidence/linux-e2e/native-modules.txt) record the separate process evidence.

## Windows performance

`xtask.exe measure windows --binary <optimized-windows-exe> --seed 12345 --report-dir <reports>` used an isolated profile and the larger patch fixture. It recorded startup from a fresh process with the OS disk cache left in its ordinary state. Frame time covers GPUI dirty-to-present submission during actual wheel events. Compositor latency requires a separate display measurement.

| Metric | Optimized run at 96 DPI | Ceiling | Result |
| --- | ---: | ---: | --- |
| Idle working set | 90.25 MiB | 200 MiB | Pass |
| Peak working set | 92.32 MiB | 450 MiB | Pass |
| GPU process-memory counters | 0 bytes | 256 MiB | Pass for the observed software-rendered session. Hardware GPU evidence remains open. |
| Fresh process startup | 2,728 ms | 3,000 ms | Pass |
| Large diff scroll p95 | 64.30 ms over 46 samples | 20 ms | Fail |
| Long discussion scroll p95 | 83.44 ms over 40 samples | 20 ms | Fail |
| Candidate package | Pending | 50 MiB | Blocked |

The optimized executable was 26,760,192 bytes before packaging. The earlier [debug run](evidence/windows-debug-measure/measurements.json) measured a worst p95 of 126.70 ms. The [first optimized run](evidence/windows-optimized-measure/measurements.json) measured 71.75 ms. The [final optimized result](evidence/windows-final-measure/measurements.json) remains above budget after moving test report writes off the GUI thread. A Windows desktop with hardware rendering and a high-DPI display is needed to assess the outstanding native feasibility cases and determine the remaining performance work.

## Installation and traceability

The [installation plan](../spec/INSTALLATION_PLAN.md) contains the proposed one-to-three-command routines, including prebuilt target selection and publisher checks. The Windows installer, trusted publisher identity, exact published command tests and retention evidence belong to Sprint 12. The later platform install gates remain in Sprints 13 and 14.

AC-84 and AC-89 passed for the initial slice. AC-83 remains open for actual hosted job results. The [initial requirements](../spec/requirements_traceability.csv), [acceptance cases](../spec/acceptance_traceability.csv), and [test scenarios](../spec/test_scenarios.csv) carry the initial-phase updates. The [per-platform requirements](../spec/requirement_platform_traceability.csv), [acceptance](../spec/acceptance_platform_traceability.csv), and [test evidence](../spec/test_platform_traceability.csv) retain their own release gates.

Sprint 02 takes SQLite migrations, resource completeness, durable drafts and the Outbox after the outstanding Sprint 01 feasibility gates are resolved.
