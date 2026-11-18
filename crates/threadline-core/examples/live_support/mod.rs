use serde::Serialize;
use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, IsTerminal, Read},
    path::{Path, PathBuf},
};
use threadline_core::github::SecretToken;

#[derive(Clone, Copy)]
pub struct Scope {
    pub repository: &'static str,
    pub actor: u64,
    pub profile: &'static str,
}

pub fn checkpoint(directory: &Path, name: &str, data: &impl Serialize) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join(name))?;
    serde_json::to_writer_pretty(&file, data)?;
    file.sync_all()
}

pub struct Options {
    pub stdin: bool,
    oauth: bool,
    pub journal: PathBuf,
}
impl Options {
    pub fn parse(
        args: impl IntoIterator<Item = String>,
        expected: Scope,
    ) -> Result<Self, &'static str> {
        let mut args = args.into_iter();
        let (mut repo, mut actor, mut journal, mut source) = (None, None, None, None);
        let mut profile = None;
        let (mut allow, mut oauth) = (false, false);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--repo" if repo.is_none() => repo = args.next(),
                "--expected-user-id" if actor.is_none() => {
                    actor = args.next().and_then(|id| id.parse::<u64>().ok())
                }
                "--journal-dir" if journal.is_none() => {
                    journal = args
                        .next()
                        .filter(|path| !path.is_empty())
                        .map(PathBuf::from)
                }
                "--allow-live-writes" if !allow => allow = true,
                "--profile" if profile.is_none() => profile = args.next(),
                "--credential-stdin" if source.is_none() => source = Some(true),
                "--credential-env" if source.is_none() => source = Some(false),
                "--allow-existing-cli-oauth" if !oauth => oauth = true,
                _ => return Err("invalid_request"),
            }
        }
        if repo.as_deref() != Some(expected.repository)
            || actor != Some(expected.actor)
            || profile.as_deref() != Some(expected.profile)
            || !allow
        {
            return Err("write_scope_or_opt_in_missing");
        }
        let stdin = source.ok_or("credential_source_missing")?;
        if oauth && !stdin {
            return Err("cli_oauth_requires_explicit_pipe");
        }
        Ok(Self {
            stdin,
            oauth,
            journal: journal.ok_or("journal_location_missing")?,
        })
    }

    pub fn credential(&self) -> Result<(&'static str, SecretToken), &'static str> {
        let raw = if self.stdin {
            let stdin = io::stdin();
            if stdin.is_terminal() {
                return Err("credential_stdin_requires_a_pipe");
            }
            let mut raw = String::new();
            stdin
                .lock()
                .take(8195)
                .read_to_string(&mut raw)
                .map_err(|_| "credential_read_failed")?;
            raw
        } else {
            env::var("THREADLINE_LIVE_PAT").map_err(|_| "dedicated_pat_unavailable")?
        };
        let raw = raw
            .strip_suffix("\r\n")
            .or_else(|| raw.strip_suffix('\n'))
            .unwrap_or(&raw);
        let class = if raw.starts_with("github_pat_") {
            "fine_grained_pat"
        } else if raw.starts_with("ghp_") {
            "classic_pat"
        } else if self.oauth && self.stdin && raw.starts_with("gho_") {
            "existing_cli_oauth"
        } else {
            return Err("credential_type_not_authorized");
        };
        Ok((
            class,
            SecretToken::new(raw).map_err(|_| "credential_invalid")?,
        ))
    }
}
