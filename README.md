# Threadline

Threadline is a native desktop client for GitHub issues and pull requests. It keeps downloaded conversations, reviews, tabs, and drafts in a local SQLite profile. You can reopen cached work offline and inspect queued changes in the Outbox.

## Install from source

You need Git, Rust 1.98 or later, and a GitHub account with access to the private Threadline repository. Make sure Cargo's bin directory is on your `PATH`.

Sign in to GitHub when Git prompts. If you're already signed in with GitHub CLI, you can run `gh auth setup-git` once before cloning. [GitHub CLI credential setup](https://cli.github.com/manual/gh_auth_setup-git)

On Windows, use the x64 MSVC toolchain, Visual Studio 2022 Build Tools with the Desktop development with C++ workload, a Windows SDK that includes `fxc.exe`, and CMake. See the [GPUI Kit installation guide](https://gpui-kit.com/docs/installation/) for the development prerequisites on other hosts.

Copy these commands into PowerShell or a POSIX shell.

```sh
git clone https://github.com/joshmarkovic/threadline.git
cd threadline
cargo install --locked --path crates/threadline-app
threadline
```

From an existing checkout, use `cargo run --locked -p threadline-app` to build and run a development copy.

Open **Accounts**, enter the private repository as `owner/repository`, paste its GitHub personal access token into the masked field, and choose **Check access**. Threadline verifies the account and repository with GitHub. Select the repositories you want to keep in the reader. On Windows, the native credential store protects the token. The token's repository selection and permissions govern which reads and writes GitHub allows. [Account setup and endpoint permissions](docs/architecture/github-api.md)

In **Accounts → Display time zone**, enter an IANA name such as `America/New_York` or `UTC` and choose **Save time zone**. Enter `system` to follow the device. Displayed dates update immediately. Applying a calendar-day filter uses the selected time zone. Saved filters keep their existing UTC cutoffs. Work and quiet hours use the selected time zone when they have no override. You can also set the preference before opening a workspace with `threadline --display-timezone America/New_York`. [Time-zone and storage behavior](docs/architecture/synchronization.md#time-zones)

The per-user Windows 11 x64 development installer has been tested locally. The signed public package and its end-user download command remain pending. Developers can build and exercise the local installer. [Windows packaging guide](tools/windows/README.md) · [Package evidence](docs/reports/sprint-12.md)

Uninstall preserves your local workspace and sign-in credentials. To preview or permanently remove the retained data, use the separate Windows cleanup command. [Remove a retained local workspace](tools/windows/README.md#remove-a-retained-local-workspace)

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
