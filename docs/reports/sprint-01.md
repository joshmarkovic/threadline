# Sprint 01 native foundation

November 18, 2026 · Threadline 0.1.0

Threadline has a runnable Rust and GPUI workspace. Native desktop smoke passed on Windows 11 at 96 and 144 DPI, hosted Windows Server 2025 at 144 DPI, and isolated X11. Hosted builds and portable tests passed on all three OS families, and the hosted macOS window and AppKit Quit probe passed. Windows notification click, hardware-rendered performance and the 20 ms scrolling budget remain open.

## Delivered behavior

| Ticket | Result |
| --- | --- |
| S01-T01 | The Rust workspace builds with Cargo.lock, GPUI Kit 0.10.0 and GPUI 0.3.12. The Windows and Linux feasibility binaries ran as native processes. |
| S01-T02 | The portable core verifies typed repository and item identities, revisions, structured errors, cancellation and stale responses. A seeded fake, bounded Tokio scheduler and loopback HTTP simulator exercise background work. |
| S01-T03 | The workspace has virtualized selectable Markdown, a virtualized annotated diff, two resizable panes with independently focused tabs and drafts, a branching history fixture, and a 33/67 PR context and code split. Both splitters and OS clipboard behavior passed on Windows 11 at 96 and 144 DPI and on hosted Windows Server at 144 DPI. Simulated 2× geometry passed in the GPUI harness. [Windows 11 release-build smoke](evidence/windows-11-release/native.json) · [Hosted 144-DPI smoke](#hosted-144-dpi-smoke) |
| S01-T04 | Windows opted in to tray residence, minimized on Close, and restored the original process through the actual tray icon. A second launch forwarded to the active review in the original process. The shell emitted its banner-shown callback. The Windows automation could not locate the banner's native click target. |
| S01-T05 | The native benchmark and numeric ceilings are in place. Startup and working set passed on the reference VM. Both surfaces exceeded 20 ms at 96 DPI. At 144 DPI the release build exceeded 20 ms on the discussion and returned too few diff samples for p95. |
| S01-T06 | Portable service contracts, platform ownership, a support matrix, artifact metadata and short, verified installation templates are documented. |
| S01-T07 | Cargo commands validate selection and seed, report JSON and JUnit, and run isolated tests. The Windows and X11 input drivers exercised real windows, including Windows 11 and hosted Windows Server at 150% display scaling. Windows, Apple Silicon macOS, and Ubuntu x64 hosted build and core-test jobs passed. The macOS launch and AppKit Quit probe passed. Full macOS desktop input verification remains open. |

The app opens synthetic station-labs/relay content, fixture revision `s01-feasibility-v1`. SQLite, live GitHub authentication, durable drafts and the Outbox have their own implementation sprints in the pack.

## Environment and build

- **Windows 11** ran in Windows 11 Enterprise 23H2 x64 build 22631.7652, active Remote Desktop session 2. The VM has eight logical Xeon Platinum 8370C processors, 32 GiB RAM and a Microsoft virtual SAS disk. The reference runs observed 2544 × 1324 pixels at 96 DPI and 3840 × 2160 pixels at 144 DPI. GPUI reported Microsoft Basic Render Driver, with software emulation enabled. The process loaded Direct3D 11, DirectWrite and `d3d10warp.dll`. [Renderer and frame evidence](evidence/windows-virtualized-measure/frame-phases.json)
- **Linux** ran in Debian 13.7 x86-64 on WSL2 kernel 6.18.40.1. The latest run used an isolated Xvfb display with XTEST at 1500 × 1000 × 24 and software Vulkan through lavapipe. An earlier run used X11 through WSLg. The release target is Ubuntu 24.04 GNOME with separate tests for Wayland and X11. [Xvfb environment](evidence/linux-virtualized-e2e/environment.json)
- **macOS** ran on a hosted Apple Silicon runner with macOS 15.8.2 build 24H232 and runner image 20261104.0514.1. The logged-in Quartz console session had one active display and an Apple Paravirtual Metal device. [Native window probe](evidence/macos-native-window/macos-window-probe.json)
- **Toolchain** used Rust 1.98.1. Earlier Windows executables were cross-built with LLVM 19.1.7 and Microsoft SDK 10.0.26100. Debug assertions in the cross-built `gpui-pre-windows` dependency selected runtime Direct3D shader compilation with `D3DCOMPILE_SKIP_OPTIMIZATION`. The hosted native release build embedded shaders compiled by the Windows SDK with `/O3`. [GPUI Windows build source](https://docs.rs/crate/gpui-pre-windows/0.3.12/source/build.rs) · [Runtime shader source](https://docs.rs/crate/gpui-pre-windows/0.3.12/source/src/directx_renderer.rs)

The local Windows smoke and benchmark at 96 DPI used the app source in `c3e92b3f34e842a59803f50883f2f551a66fb95c` with the virtualized discussion. Both ran the 26,833,408-byte instrumented executable identified by SHA-256 `657ffefe2b6a6215ecfb7bbfc999d8082b027da58da0b9c96b89de09041adb2b`. Both the Linux rerun and macOS probe recorded source revision `adcffbdde03f774e9863917b7bbee25500d4c80f`. The Linux report records a dirty worktree and the exact executable digest. [Linux build identity](evidence/linux-virtualized-e2e/binary.json) · [Architecture decisions](../architecture/foundation.md)

## Test evidence

Commands use deterministic seed 12345. The Windows host used the same Cargo runner binary via `xtask.exe` with a staged local workspace, `--binary` pointing to the instrumented executable, and Windows PowerShell 5.1 with Win32 input. Linux used `cargo xtask` from the repository with the X11 tools listed in the [driver instructions](../../tools/native/README.md).

| Layer | Command or execution | Result and evidence |
| --- | --- | --- |
| Unit | `cargo xtask test unit --seed 12345 --report-dir artifacts/verification` | Pass. 15 tests including UT-021, core behavior and diff fixture parsing. [JSON](evidence/linux-unit/result.json) · [JUnit](evidence/linux-unit/junit.xml) |
| Loopback HTTP and platform contracts | `cargo xtask test integration --seed 12345 --report-dir artifacts/verification` | Pass. Five HTTP simulator and two platform contract tests. Real SQLite starts in Sprint 02. [JSON](evidence/linux-integration/result.json) · [JUnit](evidence/linux-integration/junit.xml) |
| GPUI in-process | `cargo xtask test ui --seed 12345 --report-dir artifacts/verification` | Pass. Three tests of selection, focus and state, and splitter geometry. [JSON](evidence/linux-ui/result.json) |
| Earlier native Windows Rust harnesses | `cargo test --locked --offline --target x86_64-pc-windows-msvc -p threadline-app -p threadline-core -p threadline-platform -p xtask --no-run`, then each applicable executable on Windows with `--test-threads=1` | Pass. 26 tests spanning portable code, Win32 contracts, runner checks and the GPUI integration harness. [Console results](evidence/windows-cargo-tests.txt) |
| Windows separate process | `xtask.exe test e2e --suite smoke --binary <windows-exe> --seed 12345 --report-dir <reports>` | Pass, E2E-001 and AC-84. Verified Markdown copy, typing while preview loaded, pane focus, stable draft on tab change, linked issue context, both pointer splitters, single-instance forwarding, Quit and relaunch. [96-DPI native actions](evidence/windows-virtualized-e2e/native.json) · [144-DPI release-build native actions](evidence/windows-11-release/native.json) · [JUnit](evidence/windows-virtualized-e2e/junit.xml) |
| Windows tray and banner | `xtask.exe test e2e --suite full --case S01-WINDOWS-SHELL --binary <windows-exe> --seed 12345 --report-dir <reports>` | Blocked. The app remained in the tray after Close, restored from a click on its icon, and received `NIN_BALLOONSHOW`. UI Automation could not identify the banner click target at either scale. [96-DPI native actions](evidence/windows-shell/platform-native.json) · [144-DPI release-build shell test](evidence/windows-11-release/platform-native.json) |
| Windows notification click diagnostic | Staged Windows PowerShell driver on the earlier optimized executable, seed 12345 | Blocked. The shell confirmed display. A bounded search took 28 polls over 2.71 seconds and found zero fresh notification nodes and zero verified click points. [Diagnostic and driver digests](evidence/windows-notification-diagnostic/platform-native.json) |
| Windows Notification Center diagnostic | Fixture-only shell inspection on Windows 11 at 96 DPI, seed 12345 | Blocked. After the shell reported the banner shown, seven polls found no card with the exact fixture title and body. The probe restored the original closed flyout. [Diagnostic](evidence/windows-notification-center/platform-native.json) |
| Hosted Windows shell replay | `windows-smoke.ps1 -PlatformProof` with the verified native release test executable, seed 12345 | Blocked. The window minimized into the tray, the actual tray icon restored the original process, and the shell reported the banner shown. UI Automation could not identify the banner's clickable title. [Native actions](evidence/windows-hosted-replay/platform-native.json) |
| Linux separate process on Xvfb | `xtask test e2e --suite smoke --case E2E-001 --binary <linux-exe> --seed 12345` | Pass on the virtualized build. XTEST drove launch, Markdown copy, typing while preview loaded, tab and context switches, both pointer splitters, Quit and relaunch. [Native actions](evidence/linux-virtualized-e2e/native.json) · [JUnit](evidence/linux-virtualized-e2e/junit.xml) |
| Earlier Linux separate process on WSLg | `cargo xtask test e2e --suite smoke --binary <linux-exe> --seed 12345 --report-dir artifacts/verification` | Pass for early WSLg X11 feasibility. Launch, selection, typing, background work, tab and context switches, both pointer splitters and Quit ran against the actual window. [Native actions](evidence/linux-e2e/native.json) · [Screenshot](evidence/linux-e2e/interaction.png) |
| Early macOS native window | `THREADLINE_PROBE_COMMIT=$(git rev-parse HEAD) xcrun swift tools/native/macos-window-probe.swift target/debug/threadline artifacts/macos-window-probe.json` | Pass for launch and Quit. Quartz observed a 1024 × 681 on-screen window owned by the child. AppKit Quit targeted the verified child and the process exited with status 0. Text, focus, clipboard and splitters remain open. [Probe JSON](evidence/macos-native-window/macos-window-probe.json) · [Hosted macOS job](https://github.com/joshmarkovic/threadline/actions/runs/44233609666/job/134564328594) |
| Unknown selector | `cargo xtask test unit --case UT-unknown --report-dir artifacts/verification` | Expected exit 1, with Fail in JSON and JUnit. [JSON](evidence/runner-invalid/result.json) |
| Installer and upgrade | `cargo xtask test install --artifact docs/architecture/windows-budgets.json --report-dir artifacts/verification` | Expected exit 78 from the development guard. An inert file exercised reporting. Exact installer and retained-data verification are assigned to Sprint 12. [JSON](evidence/installer-guard/result.json) |
| Approved live GitHub | `cargo xtask test live --repo test-fixture/not-authorized --allow-live-writes --report-dir artifacts/verification` with authorization variables unset | Expected exit 78. The runner blocked all live cases before accessing any repository. [JSON](evidence/live-guard/result.json) |

`cargo fmt --all --check` and `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` passed. The observer's real-file shutdown test passed with `cargo test --locked -p threadline-app --features e2e --lib`.

The earlier optimized Windows smoke preserved a cropped view of the fixture and its separate process results. [Earlier Windows native actions](evidence/windows-e2e/native.json) · [Earlier Windows screenshot](evidence/windows-e2e/interaction.png)

The dependency inventory used `cargo tree --locked --offline -p threadline-app --target <triple> -e normal,build --prefix none` for `x86_64-pc-windows-msvc`, `aarch64-apple-darwin` and `x86_64-unknown-linux-gnu`. The native target trees had zero browser runtime matches. [Windows loaded modules](evidence/windows-virtualized-e2e/native.json) · [Linux mapped modules](evidence/linux-virtualized-e2e/native-modules.txt)

### Hosted build and core tests

All three hosted jobs built `37611fc8e839a5d42fa0b365d599dd26b089efa1` with Rust 1.98.1 and passed `cargo build --locked -p threadline-app`, `cargo xtask test unit`, and `cargo xtask test integration`. Each build-core artifact includes the native compiler host, `result.json` and JUnit files, and the runtime dependency inventory. All six test reports used seed 12345 and reported clean source. The Windows lockfile digest reflects CRLF line endings for the same locked dependencies. [GitHub Actions run and artifacts](https://github.com/joshmarkovic/threadline/actions/runs/44231863438)

| Hosted runner | Compiler host and runner version | Result |
| --- | --- | --- |
| Windows Server 2025 x64 | `x86_64-pc-windows-msvc`, Windows 10.0.26100.33674 | Build, 15 unit tests, and eight integration tests passed. [Job](https://github.com/joshmarkovic/threadline/actions/runs/44231863438/job/134558577273) |
| macOS 15 Apple Silicon | `aarch64-apple-darwin`, macOS 15.8.2 | Build, 15 unit tests, and seven integration tests passed. [Job](https://github.com/joshmarkovic/threadline/actions/runs/44231863438/job/134558577229) |
| Ubuntu 24.04 x64 | `x86_64-unknown-linux-gnu`, Linux kernel 7.0.0-1017-azure | Build, 15 unit tests, and seven integration tests passed. [Job](https://github.com/joshmarkovic/threadline/actions/runs/44231863438/job/134558577159) |

These build and core-test results satisfy AC-83 for the initial phase. Native desktop behavior and release support still require target-specific evidence.

The rerun at `acd7ce73e51f3d0ba1e4ada4243779d211d21904` passed the three build and core-test jobs and the Windows GUI job. On Windows Server 2025 Datacenter at 1024 × 768 and 96 DPI, three GPUI in-process tests passed and the Windows driver passed E2E-001 against a separate native window. The private reference artifact contains the same 26,802,688-byte test executable as the hosted smoke. Its manifest records the commit, target, lockfile and executable SHA-256. [Hosted run](https://github.com/joshmarkovic/threadline/actions/runs/44235734818) · [Windows GUI job](https://github.com/joshmarkovic/threadline/actions/runs/44235734818/job/134571305191) · [GPUI JUnit](evidence/windows-hosted/ui/junit.xml) · [Native JUnit](evidence/windows-hosted/e2e/junit.xml) · [Build manifest](evidence/windows-hosted/build.json) · [Private reference test artifact](https://github.com/joshmarkovic/threadline/actions/runs/44235734818/artifacts/14398706880)

## Windows performance

`xtask.exe measure windows --binary <optimized-windows-exe> --seed 12345 --report-dir <reports>` used an isolated profile and the larger patch fixture. It recorded startup from a fresh process with the OS disk cache left in its ordinary state. Frame time covers GPUI dirty-to-present submission during actual wheel events. Compositor latency requires a separate display measurement.

| Metric | Virtualized cross-build at 96 DPI | Ceiling | Result |
| --- | ---: | ---: | --- |
| Idle working set | 80.11 MiB | 200 MiB | Pass |
| Peak working set | 82.54 MiB | 450 MiB | Pass |
| GPU process-memory counters | 0 bytes | 256 MiB | Pass for the observed software-rendered session. Hardware GPU evidence remains open. |
| Fresh process startup | 2,569.04 ms | 3,000 ms | Pass |
| Large diff scroll p95 | 59.35 ms over 48 samples | 20 ms | Fail |
| Long discussion scroll p95 | 69.05 ms over 46 samples | 20 ms | Fail |
| Candidate package | Pending | 50 MiB | Blocked |

GPUI recorded median drawing times of 7.34 ms for the diff and 3.66 ms for the discussion. Wait time before drawing was 21.00 ms on the diff p95 frame and 55.45 ms on the discussion p95 frame. Subtracting the recorded GPUI draw time while holding the measured scheduling and submission durations fixed leaves p95s of 49.11 ms and 61.52 ms. The data identify delays outside drawing on this software-rendered session, alongside occasional paint spikes. Both surfaces still exceed budget. [Measurements](evidence/windows-virtualized-measure/measurements.json) · [Frame phases](evidence/windows-virtualized-measure/frame-phases.json) · [JUnit](evidence/windows-virtualized-measure/junit.xml)

The earlier runs retain their original results. The debug run measured a worst p95 of 126.70 ms. The first optimized run measured 71.75 ms. The optimized run after moving report writes off the GUI thread measured 83.44 ms. Hardware-rendered Windows performance remains open. [Debug measurements](evidence/windows-debug-measure/measurements.json) · [First optimized measurements](evidence/windows-optimized-measure/measurements.json) · [Earlier optimized measurements after the report-writer change](evidence/windows-final-measure/measurements.json)

The first Windows 11 benchmark attempt with the exact hosted test executable returned exit 78 during input desktop readiness. All 50 samples showed the Remote Desktop session disconnected and locked. The guard stopped before sending input. [Attempt summary](evidence/windows-hosted/reference-readiness/summary.json) · [Desktop samples](evidence/windows-hosted/reference-readiness/measure-desktop.json)

### Windows 11 release build at 144 DPI

After the reference desktop reconnected, Windows reported 144 DPI on Threadline’s window and GPUI reported 1.5 at 3840 × 2160. The executable matched the SHA-256 and length in the Windows CI build manifest. The same E2E-001 driver and seed 12345 passed all ten native actions with the test runner’s console hidden. It restored the clipboard, opened PR 72 in the original process on the second launch, and quit and relaunched successfully. Two earlier attempts stopped when the app lost foreground focus at the context-divider drag. [Build manifest](evidence/windows-hosted/build.json) · [Native actions and earlier attempts](evidence/windows-11-release/native.json)

The standard benchmark in the same desktop configuration recorded 1,693.06 ms to start, 103,784,448 bytes of idle working set, 105,050,112 bytes of peak working set and zero bytes in both GPU process-memory counters. The machine used Microsoft Basic Render Driver with software emulation enabled. The candidate package remains due in Sprint 12. [Measurements and budgets](evidence/windows-11-release/measurements.json)

| Windows 11 benchmark | Diff samples | Discussion samples | Discussion p95 | Result against 20 ms |
| --- | ---: | ---: | ---: | --- |
| Standard, hidden test runner console | 4 | 36 | 134.4278 ms | Fail |
| Standard, ordinary test runner console | 3 | 36 | 137.7274 ms | Fail |
| DirectComposition disabled, ordinary test runner console | 3 | 28 | 196.5959 ms | Fail |

All three runs used the same executable, seed and wheel workload. The last run set GPUI’s process-local `GPUI_DISABLE_DIRECT_COMPOSITION=1` switch. All three diff results fell below the ten-frame minimum for reporting p95. Individual frame times include the wait before drawing, the draw itself, and submission through the platform. [Per-run results](evidence/windows-11-release/measurements.json) · [Paired frame samples and phases](evidence/windows-11-release/frame-samples.json)

### Hosted Windows Server replay

The replay used `build_run_id=44235734818` and seed 12345 on hosted Windows Server 2025 Datacenter x64, build 26100, at 1024 × 768 and 96 DPI. It reused the verified native release test executable with `/O3` shaders and SHA-256 `2d6c06948bf3273973ec448c3f78176f87ce9a1dd7c02c20b9d326ee415a7aa4`. GPUI named Microsoft Basic Render Driver and returned `is_software_emulated: false`. The Windows 11 cross-build and Windows Server replay ran on different machines. The Windows 11 tests of the two builds also used different display scales, so their timings do not isolate shader compilation. [Verified build manifest](evidence/windows-hosted/build.json) · [Replay workflow](../../.github/workflows/windows-hosted-replay.yml) · [Hosted replay and raw artifacts](https://github.com/joshmarkovic/threadline/actions/runs/44240601495)

| Metric | Hosted replay | Result |
| --- | ---: | --- |
| Fresh process startup | 1,374.79 ms | Pass |
| Idle working set | 76,935,168 bytes | Pass |
| Peak working set | 84,938,752 bytes | Pass |
| GPU process-memory counters | 0 bytes | Pass in this session |
| Large diff scroll p95 | 154.4225 ms over 31 samples | Fail against 20 ms |
| Long discussion scroll p95 | 140.0184 ms over 38 samples | Fail against 20 ms |
| Candidate package | Pending | Blocked |

The diff p95 frame spent 146.6351 ms in GPUI paint. The discussion p95 frame spent 137.2148 ms in platform submission. The replay preserves the per-frame timings in its workflow artifact. [Measurement results](evidence/windows-hosted-replay/measurements.json) · [Raw replay artifact](https://github.com/joshmarkovic/threadline/actions/runs/44240601495)

## Additional Windows desktop evidence

### Hosted notification and display diagnostics

A separate hosted Windows Server run used the same verified app executable and an isolated diagnostic driver revision. [Driver and build identity](evidence/windows-hosted-diagnostics/diagnostic-driver.json) · [Hosted diagnostic run](https://github.com/joshmarkovic/threadline/actions/runs/44243315426)

- **Notification click** remained Blocked. Local optical character recognition (OCR) passed synthetic checks for wrapped matches, wrong text, and duplicates. After the shell reported the banner shown, the driver made 18 reads of a 512 × 300-pixel notification corner over 2,249 ms. It found no exact fixture title-and-body pair and ended before issuing a notification click. [Notification diagnostic](evidence/windows-hosted-diagnostics/notification-ocr.json)
- **Initial display probe** stopped at 96 DPI. The initial Display selector exposed neither a unique enabled 150% nor 200% choice to this probe. The driver left display settings as found and ended before the smoke. [Initial display diagnostic](evidence/windows-hosted-diagnostics/display-scale.json)

### Hosted 144-DPI smoke

On Windows Server 2025 Datacenter, the supplemental driver at `6c39407` changed the physical resolution from 1024 × 768 to 1920 × 1080. Display Settings then offered 100%, 125%, 150% and 175%. The driver selected 150%, the monitor changed from 96 to 144 DPI, and the native smoke measured 144 DPI on the actual Threadline window. GPUI reported a 1.5 scale factor. The driver restored the original resolution after the smoke. [Display diagnostic](evidence/windows-hosted-high-dpi/display-scale.json) · [GPUI observation excerpt](evidence/windows-hosted-high-dpi/ui-observation.json)

The unchanged `windows-smoke.ps1` from `acd7ce7` passed all ten actions with seed 12345. It launched the window, selected and copied Markdown, typed and copied review input during a pending preview, preserved the draft across tabs, switched to linked issue context, dragged both splitters, forwarded a second launch, quit, and relaunched and quit again. It restored the clipboard. The app SHA-256 matches the verified release test artifact from run `44235734818`. [Native actions](evidence/windows-hosted-high-dpi/native.json) · [Verified build manifest](evidence/windows-hosted/build.json) · [Hosted run and raw artifacts](https://github.com/joshmarkovic/threadline/actions/runs/44244949417)

## Installation and traceability

The [installation plan](../spec/INSTALLATION_PLAN.md) contains the proposed one-to-three-command routines, including prebuilt target selection and publisher checks. The Windows installer, trusted publisher identity, exact published command tests and retention evidence belong to Sprint 12. The later platform install gates remain in Sprints 13 and 14.

AC-83, AC-84, and AC-89 passed for the initial Windows and portable core slice. The [initial requirements](../spec/requirements_traceability.csv), [acceptance cases](../spec/acceptance_traceability.csv), and [test scenarios](../spec/test_scenarios.csv) carry the initial-phase updates. The [per-platform requirements](../spec/requirement_platform_traceability.csv), [acceptance](../spec/acceptance_platform_traceability.csv), and [test evidence](../spec/test_platform_traceability.csv) retain their own release gates.

Sprint 02 takes SQLite migrations, resource completeness, durable drafts and the Outbox after the outstanding Sprint 01 feasibility gates are resolved.
