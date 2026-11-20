use super::*;

#[test]
fn login_is_a_single_github_account_argument() {
    for candidate in ["cloud-engineer", "developer_learner", "A1"] {
        assert!(GitHubUser::valid_login(candidate));
    }
    for candidate in [
        "",
        "--hostname",
        "-abc",
        "github.com/u",
        "a b",
        "a\nb",
        "a|b",
        "a@",
        "ß",
    ] {
        assert!(!GitHubUser::valid_login(candidate));
    }
}

#[test]
fn environment_only_uses_native_profile_and_required_system_settings() {
    let values = native::environment().unwrap();
    for name in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GH_HOST",
        "GH_CONFIG_DIR",
        "GH_DEBUG",
        "GITHUB_API_URL",
        "GH_BROWSER",
        "BROWSER",
        "GIT_CONFIG_GLOBAL",
        "GIT_CONFIG_COUNT",
        "XDG_CONFIG_HOME",
        "HTTPS_PROXY",
        "LD_PRELOAD",
        "DYLD_INSERT_LIBRARIES",
        "GH_PAGER",
    ] {
        assert!(!values.iter().any(|(key, _)| key == name));
    }
    let paths = &values.iter().find(|(key, _)| key == "PATH").unwrap().1;
    assert!(std::env::split_paths(paths).all(|p| p.is_absolute()));
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    use tempfile::{TempDir, tempdir};

    fn fixture(script: &str) -> (TempDir, GitHubCli) {
        let temp = tempdir().unwrap();
        let executable = temp.path().join("gh fixture");
        fs::write(&executable, format!("#!/bin/sh\nset -eu\n{script}\n")).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let cli = GitHubCli {
            executable,
            environment: vec![
                ("HOME".into(), temp.path().as_os_str().into()),
                ("PATH".into(), "/usr/bin:/bin".into()),
            ],
        };
        (temp, cli)
    }

    #[tokio::test]
    async fn active_account_and_pinned_token_use_fixed_host_in_isolation() {
        let (temp, cli) = fixture(
            r#"
            [ "$PWD" = "$HOME" ] && [ "$GH_PROMPT_DISABLED" = 1 ] || exit 1
            case "$1" in
              api)
                [ "$#" = 6 ] && [ "$2" = --hostname ] && [ "$3" = github.com ] &&
                  [ "$4" = user ] && [ "$5" = --jq ] && [ "$6" = '{id,login}' ] || exit 1
                printf '%s\n' '{"id":7352,"login":"Example-User"}' ;;
              auth)
                [ "$#" = 6 ] && [ "$2" = token ] && [ "$3" = --hostname ] &&
                  [ "$4" = github.com ] && [ "$5" = --user ] && [ "$6" = Stored_User ] || exit 1
                printf '%s\r\n' 'gho_threadlineFixtureSecret' ;;
              *) exit 1 ;;
            esac
        "#,
        );
        let cancel = CancellationToken::new();
        let user = cli.active_user(&cancel).await.unwrap();
        assert_eq!((user.id.get(), user.login.as_str()), (7352, "Example-User"));
        let token = cli.token_for("Stored_User", &cancel).await.unwrap();
        assert_eq!(format!("{token:?}"), "SecretToken([redacted])");
        assert_eq!(
            cli.token_for("other", &cancel).await.unwrap_err(),
            CliError::SignedOut
        );
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn invalid_login_and_pre_cancelled_work_never_invoke_cli() {
        let (temp, cli) = fixture("touch ran; exit 1");
        let cancel = CancellationToken::new();
        assert_eq!(
            cli.token_for("--hostname", &cancel).await.unwrap_err(),
            CliError::InvalidLogin
        );
        cancel.cancel();
        assert_eq!(
            cli.token_for("example", &cancel).await.unwrap_err(),
            CliError::Cancelled
        );
        assert_eq!(
            cli.active_user(&cancel).await.err(),
            Some(CliError::Cancelled)
        );
        assert_eq!(cli.login(&cancel).await, Err(CliError::Cancelled));
        assert!(!temp.path().join("ran").exists());
    }

    #[tokio::test]
    async fn output_is_bounded_and_failures_are_redacted() {
        let cancel = CancellationToken::new();
        for (script, error) in [
            (
                "printf '%s' 'real-token-text-in-error' >&2; exit 4",
                CliError::SignedOut,
            ),
            (
                "printf '%s' 'unexpected output'; exit 0",
                CliError::InvalidResponse,
            ),
            (
                r#"printf '%s\n' '{"id":12,"login":"--web"}'"#,
                CliError::InvalidResponse,
            ),
            (
                "head -c 16384 /dev/zero; exec sleep 10",
                CliError::InvalidResponse,
            ),
        ] {
            let (_temp, cli) = fixture(script);
            let result = timeout(Duration::from_secs(3), cli.active_user(&cancel))
                .await
                .unwrap();
            let actual = result.err().unwrap();
            assert_eq!(actual, error);
            assert!(!actual.to_string().contains("real-token-text"));
        }
        for output in [
            "gho_X\ngho_Y",
            "gho_",
            "gho_X ",
            " ghp_X",
            "ghr_refreshCredential",
        ] {
            let (_temp, cli) = fixture(&format!("printf '%s' '{output}'"));
            assert_eq!(
                cli.token_for("example", &cancel).await.unwrap_err(),
                CliError::InvalidResponse
            );
        }
    }

    #[tokio::test]
    async fn cancellation_ends_a_hung_command() {
        let (temp, cli) = fixture("touch ready; exec sleep 60");
        let cancel = CancellationToken::new();
        let stop = cancel.clone();
        let task = tokio::spawn(async move { cli.active_user(&stop).await });
        timeout(Duration::from_secs(3), async {
            while !temp.path().join("ready").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        cancel.cancel();
        assert_eq!(
            timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap()
                .err(),
            Some(CliError::Cancelled)
        );
    }
}

#[cfg(windows)]
#[test]
#[ignore = "child of native_console_has_interactive_handles"]
fn console_child() {
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for kind in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        let mut mode = 0;
        assert_ne!(unsafe { GetConsoleMode(GetStdHandle(kind), &mut mode) }, 0);
    }
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "requires an installed, signed GitHub CLI"]
async fn installed_cli_version() {
    let cli = GitHubCli::discover().expect("locate and verify official GitHub CLI");
    let result = cli
        .capture(
            &["--version"],
            4096,
            CliError::Unavailable,
            &CancellationToken::new(),
        )
        .await
        .expect("read CLI version");
    assert!(result.starts_with(b"gh version "));
}

#[cfg(windows)]
#[tokio::test]
async fn native_console_has_interactive_handles() {
    fn assert_send(_: impl Send) {}
    // Start only this test binary. The child's console must work even when the parent has redirected input/output.
    let executable = std::env::current_exe().unwrap();
    let cli = GitHubCli {
        _verified_file: std::sync::Arc::new(std::fs::File::open(&executable).unwrap()),
        executable,
        environment: native::environment().unwrap(),
    };
    // The desktop runs this future on its multithreaded account worker.
    assert_send(cli.login(&CancellationToken::new()));
    timeout(
        Duration::from_secs(15),
        native::console(
            &cli,
            &[
                "--exact",
                "github_cli::tests::console_child",
                "--ignored",
                "--nocapture",
            ],
        ),
    )
    .await
    .unwrap()
    .unwrap();
}
