# Native foundation

Threadline uses GPUI Kit 0.10.0 and its matching GPUI snapshot, `gpui-pre` 0.3.12. The application uses the native platform backend. GPUI renders the interface using DirectX on Windows, Metal on macOS, and wgpu on Linux. GPUI Kit supplies the rich text, editor, diff, and resize primitives. The native dependency inventory is recorded with feasibility results. [GPUI rendering and examples](https://gpui.rs/examples/) · [GPUI Kit installation and version alignment](https://gpui-kit.com/docs/installation/)

## Workspace boundaries

| Crate | Ownership |
| --- | --- |
| `threadline-core` | Portable domain and resource identities, provider and error contracts, cancellable background work, and deterministic test boundaries |
| `threadline-platform` | Portable desktop capability contracts and target-specific adapters |
| `threadline-app` | GPUI application, workspace and view state, fixture rendering, and native event handling |
| `xtask` | Test selection, evidence reports, and external native driver invocation |

The core defines request identity separately from the current view. A response carries the requested resource and revision information. Tab-specific work also carries the view generation so a late response can be ignored after a tab or selection changes. Future storage, queries, authentication, synchronization, Outbox, review, history, and notification-rule modules belong in the portable layer. Platform handles and operating-system APIs stay in the platform crate.

The feasibility UI uses a synthetic repository and pinned fixtures. Tests distinguish pure units, real loopback HTTP integration, in-process GPUI interaction, and a separate native process driven by the operating system. Release testing will add a clean-machine run of the exact published artifact. The [testing plan](../spec/TESTING_PLAN.md) defines each layer and the approved scope for live checks.

## Target defaults

| Target | First release gate | Local distribution design |
| --- | --- | --- |
| Windows 11 x64, first priority | Sprint 12 | Authenticode-signed current-user installer and checked publisher identity |
| macOS 15 or later on Apple Silicon | Sprint 13 | Developer ID signed and notarized bundle in a verified archive or disk image |
| Ubuntu 24.04 x64 on GNOME | Sprint 14 | Verified archive and per-user launcher. Test Wayland and X11 separately with a working graphics driver. |

Keep credential storage, application paths, desktop notifications, lifecycle, and preferences behind platform contracts. Windows uses its native adapter first. Later releases supply Keychain or Secret Service and their own native notification and lifecycle integrations. Durable user data and drafts belong in persistent per-user data directories. A credential store error holds remote work. Discardable cache and runtime files have separate locations. See the [cross-platform contract](../spec/CROSS_PLATFORM_PLAN.md).

With a matching per-user Start shortcut and COM registration, Threadline sends a Windows Runtime notification. On the Windows 11 reference desktop, the Notification Center card opened PR 72 in the running process. Without a registered identity, the development app uses a notification-area balloon. Its physical click remains unverified. Sprint 11 owns persistent activity and cold activation. Sprint 12 verifies the installed package. [Windows reference evidence](../reports/evidence/windows-11-release/registered-notification.json) · [Windows notification-area API](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyiconw)

## Distribution contract

Use the application version and target triple from the build. Each stable or preview manifest entry records `version`, `channel`, `target`, `minimum_os`, `artifact_url`, `size_bytes`, `sha256`, `publisher` or `signed_metadata`, and the local database schema compatibility range. The stable channel excludes prereleases. The installer selects and verifies the target and version before execution. It stages and verifies an update before replacing the working app and preserves local data by default.

A Windows install or update will take one to three ordinary PowerShell commands. The template downloads the prebuilt installer from the project's HTTPS origin, checks Authenticode status and the configured publisher, then launches the current-user installer and checks its exit code. A macOS or Linux bootstrap downloads to a private directory, verifies signed release metadata against a configured trust root, and installs the selected prebuilt artifact. The [installation plan](../spec/INSTALLATION_PLAN.md) contains command templates and the clean-machine tests. Publisher identities, release URLs, final script text, and published package digests are release deliverables.

## Windows feasibility budgets

The ceilings in [windows-budgets.json](windows-budgets.json) are engineering targets. The reference environment inspected for Sprint 01 is Windows 11 Enterprise 23H2 x64, build 22631.7652, on a Hyper-V virtual machine with eight logical Xeon Platinum 8370C processors, 32 GiB RAM, a Microsoft virtual SAS disk, and a 2560 × 1440 Microsoft Remote Display Adapter. Record the active DPI, refresh rate, renderer adapter, and session with each run. A virtual display adapter makes GPU results specific to that machine and session.

| Measurement | Ceiling | Sample |
| --- | ---: | --- |
| Idle working set | 200 MiB | After the feasibility fixture settles |
| Peak working set | 450 MiB | Across startup and the native fixture interaction |
| GPU memory | 256 MiB | Peak process-attributed GPU memory when counters are available |
| Distribution package | 50 MiB | Exact candidate package bytes |
| Cold startup | 3,000 ms | Process launch to a usable first window, with cold conditions recorded |
| Scroll frame time | p95 20 ms | Long discussion and large diff scroll intervals, with sample count |

Record executable size separately from package size. Record numbers for the feasibility fixture separately from Section 16's cached-opening, search, warm-startup, and larger workload tests. Missing measurements stay open along with their cause.
