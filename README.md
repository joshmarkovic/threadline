# Threadline

Threadline is a native desktop client for GitHub issues and pull requests. It keeps downloaded conversations, reviews, tabs, and drafts in a local SQLite profile. You can reopen cached work offline and inspect queued changes in the Outbox.

## Build and run

Use Rust 1.98 or later. The dependency set is pinned in `Cargo.lock`. On Windows, use the x64 MSVC toolchain, Visual Studio 2022 Build Tools with the Desktop development with C++ workload, a Windows SDK that includes `fxc.exe`, and CMake. See the [GPUI Kit installation guide](https://gpui-kit.com/docs/installation/) for the development prerequisites on other hosts.

```sh
cargo run --locked -p threadline-app
```

Open **Accounts**, enter the private repository as `owner/repository`, paste its GitHub personal access token into the masked field, and choose **Check access**. Threadline verifies the account and repository with GitHub. Select the repositories you want to keep in the reader. On Windows, the native credential store protects the token. The token's repository selection and permissions govern which reads and writes GitHub allows. [Account setup and endpoint permissions](docs/architecture/github-api.md)

The per-user Windows 11 x64 development installer has been tested locally. The signed public package and its end-user download command remain pending. Developers can build and exercise the local installer. [Windows packaging guide](tools/windows/README.md) · [Package evidence](docs/reports/sprint-12.md)

The reports record which configurations and scenarios have actually run. The sprint pack contains the acceptance criteria and traceability ledgers. [Sprint reports](docs/reports/) · [Architecture](docs/architecture/foundation.md) · [Version 1.7 sprint pack](docs/spec/README.md)

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

To open either dataset in the cached reader, use `reader-small` or `reader-section16` and write `threadline.sqlite3` inside a new, empty test profile directory. These modes select two synthetic repositories for a signed-out account. The reader can browse and search the generated records offline.

```sh
cargo run --locked -p threadline-storage --example generate_fixture -- /path/to/test-profile/threadline.sqlite3 reader-small 12345
```
