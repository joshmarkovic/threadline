# Threadline

Threadline is a native desktop client for reading and reviewing GitHub work. This repository starts with the Sprint 01 feasibility workspace. It uses synthetic issues, pull requests, and history and opens without a GitHub account.

## Run the feasibility workspace

Use Rust 1.98 or later. The dependency set is pinned in `Cargo.lock`. On Windows, use the x64 MSVC toolchain, Visual Studio 2022 Build Tools with the Desktop development with C++ workload, a Windows SDK that includes `fxc.exe`, and CMake. See the [GPUI Kit installation guide](https://gpui-kit.com/docs/installation/) for the development prerequisites on other hosts.

```sh
cargo run --locked -p threadline-app
```

The [Sprint 01 report](docs/reports/sprint-01.md) records which configurations have actually run and which gates remain open. The [architecture notes](docs/architecture/foundation.md) cover the workspace boundaries, platform targets, and distribution design. The complete version 1.7 [sprint pack](docs/spec/README.md), including its traceability ledgers, lives in `docs/spec`.

## Test the workspace

```sh
cargo xtask test unit
cargo xtask test integration
cargo xtask test ui
cargo xtask test e2e --suite smoke
```

The native smoke command needs an interactive desktop with the driver prerequisites for the host. Use `--report-dir` to choose where the runner writes its machine-readable results. See the [testing contract](docs/spec/TESTING_PLAN.md) for the full runner, artifact, and live commands.

## Generate a storage fixture

Use an unused database path in an existing directory. The generator fills a temporary database and publishes it at that path when it finishes.

```sh
cargo run --locked -p threadline-storage --example generate_fixture -- section16.sqlite section16 12345
```

Use `small` for a fast fixture. The `section16` fixture includes 50,000 issue and PR summaries, 250,000 comments, a multi-file diff with a review draft, branching history, two panes with twenty tabs, and 1,000 mixed Outbox entries. Runner results for these storage cases include their fixture revision.
