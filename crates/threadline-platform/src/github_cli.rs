//! Native GitHub CLI sign-in. Threadline only reads credentials for the selected github.com login.
use std::{ffi::OsString, fmt, path::PathBuf, process::Stdio, time::Duration};

use threadline_core::github::{CancellationToken, GitHubUser, SecretToken};
use tokio::{io::AsyncReadExt, process::Command, time::timeout};
use zeroize::Zeroizing;

#[cfg(unix)]
#[path = "github_cli/unix.rs"]
mod native;
#[cfg(windows)]
#[path = "github_cli/windows.rs"]
mod native;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const LOGIN_ARGS: &[&str] = &[
    "auth",
    "login",
    "--hostname",
    "github.com",
    "--web",
    "--skip-ssh-key",
];

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CliError {
    Missing,
    Unavailable,
    SignedOut,
    InvalidResponse,
    InvalidLogin,
    Timeout,
    Cancelled,
    LoginFailed,
    TerminalRequired,
}

impl CliError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Missing => "Install GitHub CLI on this computer, then try again.",
            Self::Unavailable => {
                "GitHub CLI could not reach GitHub or access its profile. Check your connection and try again."
            }
            Self::SignedOut => "Sign in to github.com with GitHub CLI, then try again.",
            Self::InvalidResponse => {
                "GitHub CLI returned an unreadable response. Update GitHub CLI and try again."
            }
            Self::InvalidLogin => "Choose a valid GitHub username.",
            Self::Timeout => "GitHub CLI timed out. Try again.",
            Self::Cancelled => "GitHub sign-in cancelled.",
            Self::LoginFailed => "GitHub CLI sign-in did not finish. Try signing in again.",
            Self::TerminalRequired => {
                "Open a terminal and run gh auth login --hostname github.com --web, then return to Threadline."
            }
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}
impl std::error::Error for CliError {}

#[derive(Clone)]
pub struct GitHubCli {
    executable: PathBuf,
    environment: Vec<(OsString, OsString)>,
    #[cfg(windows)]
    _verified_file: std::sync::Arc<std::fs::File>,
}

impl GitHubCli {
    /// Locate the native executable in known installation directories without searching the working directory or PATH.
    pub fn discover() -> Result<Self, CliError> {
        let mut error = CliError::Missing;
        for executable in native::candidates()
            .into_iter()
            .filter(|p| p.is_absolute() && p.is_file())
        {
            #[cfg(windows)]
            let Some(verified) = crate::publisher::github_cli(&executable) else {
                error = CliError::Unavailable;
                continue;
            };
            #[cfg(unix)]
            if !native::trusted(&executable) {
                error = CliError::Unavailable;
                continue;
            }
            return Ok(Self {
                executable,
                environment: native::environment()?,
                #[cfg(windows)]
                _verified_file: std::sync::Arc::new(verified),
            });
        }
        Err(error)
    }

    /// Ask GitHub for the active CLI identity. The UI can display this name before linking the account.
    pub async fn active_user(&self, cancel: &CancellationToken) -> Result<GitHubUser, CliError> {
        let response = self
            .capture(
                &[
                    "api",
                    "--hostname",
                    "github.com",
                    "user",
                    "--jq",
                    "{id,login}",
                ],
                4096,
                CliError::Unavailable,
                cancel,
            )
            .await?;
        let user: GitHubUser =
            serde_json::from_slice(&response).map_err(|_| CliError::InvalidResponse)?;
        if !GitHubUser::valid_login(&user.login) {
            return Err(CliError::InvalidResponse);
        }
        Ok(user)
    }

    /// The caller must compare the resulting API session's immutable user ID with its bound account.
    pub async fn token_for(
        &self,
        login: &str,
        cancel: &CancellationToken,
    ) -> Result<SecretToken, CliError> {
        if !GitHubUser::valid_login(login) {
            return Err(CliError::InvalidLogin);
        }
        let response = self
            .capture(
                &["auth", "token", "--hostname", "github.com", "--user", login],
                8194,
                CliError::SignedOut,
                cancel,
            )
            .await?;
        let line = std::str::from_utf8(&response).map_err(|_| CliError::InvalidResponse)?;
        let value = line
            .strip_suffix("\r\n")
            .or_else(|| line.strip_suffix('\n'))
            .unwrap_or(line);
        let valid = ["gho_", "ghu_", "ghp_", "github_pat_"]
            .into_iter()
            .any(|prefix| {
                value.strip_prefix(prefix).is_some_and(|tail| {
                    !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                })
            })
            || (value.len() == 40 && value.bytes().all(|b| b.is_ascii_hexdigit()));
        if !valid {
            return Err(CliError::InvalidResponse);
        }
        SecretToken::new(value).map_err(|_| CliError::InvalidResponse)
    }

    /// Starts an interactive command only in response to the user's sign-in action.
    pub async fn login(&self, cancel: &CancellationToken) -> Result<(), CliError> {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(CliError::Cancelled),
            result = timeout(LOGIN_TIMEOUT, native::login(self)) => result.map_err(|_| CliError::Timeout)?,
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.executable);
        cmd.args(args)
            .env_clear()
            .envs(self.environment.iter().cloned())
            .current_dir(self.executable.parent().expect("absolute executable"))
            .kill_on_drop(true);
        cmd
    }

    async fn capture(
        &self,
        args: &[&str],
        limit: usize,
        failure: CliError,
        cancel: &CancellationToken,
    ) -> Result<Zeroizing<Vec<u8>>, CliError> {
        let run = async {
            let mut cmd = self.command(args);
            cmd.env("GH_PROMPT_DISABLED", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            #[cfg(windows)]
            cmd.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
            let mut child = cmd.spawn().map_err(|_| CliError::Unavailable)?;
            let mut stdout = child
                .stdout
                .take()
                .ok_or(CliError::Unavailable)?
                .take(limit as u64 + 1);
            let read = async {
                // Preallocation also prevents a secret-bearing buffer being left behind by a resize.
                let mut bytes = Zeroizing::new(Vec::with_capacity(limit + 1));
                stdout
                    .read_to_end(&mut bytes)
                    .await
                    .map_err(|_| CliError::Unavailable)?;
                if bytes.len() > limit {
                    return Err(CliError::InvalidResponse);
                }
                Ok(bytes)
            };
            let wait = async { child.wait().await.map_err(|_| CliError::Unavailable) };
            let (bytes, status) = tokio::try_join!(read, wait)?;
            match status.code() {
                Some(0) => Ok(bytes),
                Some(4) => Err(CliError::SignedOut),
                _ => Err(failure),
            }
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(CliError::Cancelled),
            result = timeout(COMMAND_TIMEOUT, run) => result.map_err(|_| CliError::Timeout)?,
        }
    }
}

#[cfg(test)]
mod tests;
