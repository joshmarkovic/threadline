//! Explicit read-only verification for the user-designated dogfood repository.
use serde_json::{Value, json};
use std::{
    env,
    io::{self, IsTerminal, Read},
    num::NonZeroU64,
    process::ExitCode,
};
use threadline_core::{
    domain::{RepositoryApiId, RepositoryName},
    github::{
        AccessError, AccessKind, CancellationToken, GitHubClient, GraphIssue, OwnerScope,
        SecretToken,
    },
};

const REPOSITORY: &str = "joshmarkovic/threadline";
const USER_ID: u64 = 52_184_130;
const REPO_ID: u64 = 1_462_273_296;
const OWNER: &str = "joshmarkovic";
const NAME: &str = "threadline";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Stdin,
    Environment,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Credential {
    FineGrainedPat,
    ClassicPat,
    ExistingCliOAuth,
}
impl Credential {
    fn name(self) -> &'static str {
        match self {
            Self::FineGrainedPat => "fine_grained_pat",
            Self::ClassicPat => "classic_pat",
            Self::ExistingCliOAuth => "existing_cli_oauth",
        }
    }
    fn from_token(value: &str, allow_cli: bool) -> Option<Self> {
        if value.starts_with("github_pat_") {
            Some(Self::FineGrainedPat)
        } else if value.starts_with("ghp_") {
            Some(Self::ClassicPat)
        } else if allow_cli && value.starts_with("gho_") {
            Some(Self::ExistingCliOAuth)
        } else {
            None
        }
    }
}
struct Options {
    source: Source,
    allow_cli: bool,
}
impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, &'static str> {
        let mut args = args.into_iter();
        let (mut repo, mut id, mut read_only, mut source, mut allow_cli) =
            (None, None, false, None, false);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--repo" if repo.is_none() => repo = args.next(),
                "--expected-user-id" if id.is_none() => {
                    id = args.next().and_then(|v| v.parse::<u64>().ok())
                }
                "--read-only" if !read_only => read_only = true,
                "--credential-stdin" if source.is_none() => source = Some(Source::Stdin),
                "--credential-env" if source.is_none() => source = Some(Source::Environment),
                "--allow-existing-cli-oauth" if !allow_cli => allow_cli = true,
                _ => return Err("invalid_request"),
            }
        }
        if repo.as_deref() != Some(REPOSITORY) || id != Some(USER_ID) || !read_only {
            return Err("read_only_scope_or_expected_identity_missing");
        }
        let source = source.ok_or("explicit_credential_source_required")?;
        if allow_cli && source != Source::Stdin {
            return Err("cli_oauth_requires_explicit_pipe");
        }
        Ok(Self { source, allow_cli })
    }
    fn credential(&self) -> Result<(Credential, SecretToken), &'static str> {
        let mut raw = match self.source {
            Source::Environment => {
                env::var("THREADLINE_LIVE_PAT").map_err(|_| "dedicated_pat_unavailable")?
            }
            Source::Stdin => {
                let stdin = io::stdin();
                if stdin.is_terminal() {
                    return Err("credential_stdin_requires_a_pipe");
                }
                let mut bytes = String::new();
                stdin
                    .lock()
                    .take(8195)
                    .read_to_string(&mut bytes)
                    .map_err(|_| "credential_read_failed")?;
                bytes
            }
        };
        if raw.ends_with('\n') {
            raw.pop();
            if raw.ends_with('\r') {
                raw.pop();
            }
        }
        if raw.is_empty() || raw.len() > 8192 {
            return Err("credential_missing_or_invalid");
        }
        let kind =
            Credential::from_token(&raw, self.allow_cli).ok_or("credential_type_not_authorized")?;
        Ok((
            kind,
            SecretToken::new(&raw).map_err(|_| "credential_missing_or_invalid")?,
        ))
    }
}

fn observed_error(error: &AccessError) -> Value {
    // Keep the HTTP result and classification. Never serialize upstream URLs, headers or bodies.
    json!({"kind":format!("{:?}",error.kind),"http_status":error.http_status,
        "reachability":format!("{:?}",error.reachability)})
}

const BLOCKED: u8 = 78;
async fn probe(
    options: Options,
    credential: Credential,
    token: SecretToken,
    receipt: &mut Value,
) -> u8 {
    receipt["credential_kind"] = json!(credential.name());
    receipt["credential_source"] = json!(match options.source {
        Source::Stdin => "stdin_pipe",
        Source::Environment => "dedicated_environment",
    });
    let Ok(client) = GitHubClient::production() else {
        receipt["reason"] = json!("https_client_unavailable");
        return BLOCKED;
    };
    let cancel = CancellationToken::new();
    let owner = OwnerScope::new(OWNER).expect("fixed owner");
    let session = match client
        .authenticate(token, owner, NonZeroU64::new(USER_ID), &cancel)
        .await
    {
        Ok(s) => s,
        Err(e) => return endpoint_error(receipt, "authenticated_user", &e, credential),
    };
    // The numeric provider ID survives account renames. This fixture additionally fixes today's owner route.
    if !session.user().login.eq_ignore_ascii_case(OWNER) {
        receipt["authenticated_user"] = json!({"matched":false,"http_status":200});
        receipt["reason"] = json!("expected_login_mismatch");
        return failure_code(credential);
    }
    receipt["authenticated_user"] = json!({"matched":true,"http_status":200});
    let route = RepositoryName::new(OWNER, NAME).expect("fixed repository");
    let id = RepositoryApiId::try_from(REPO_ID).expect("fixed repository ID");
    let repo = match session
        .verify_private_repository(&route, Some(id), &cancel)
        .await
    {
        Ok(r) => r,
        Err(e) => return endpoint_error(receipt, "private_repository_read", &e, credential),
    };
    receipt["private_repository_read"] = json!({"http_status":200,"private":repo.private,"immutable_id_matched":repo.id==id,
        "repository_metadata_read":"verified","write_grants":"unverified"});
    let graph = match session.graphql_repository(&route, &cancel).await {
        Ok(r) => r,
        Err(e) => return endpoint_error(receipt, "graphql_repository_read", &e, credential),
    };
    let graph_match = graph
        .repository
        .as_ref()
        .is_some_and(|r| r.private && r.node_id == repo.node_id);
    let complete = graph.complete() && graph_match;
    if graph.errors.contains(&GraphIssue::SsoRequired) {
        receipt["sso_challenge"] = json!("observed")
    }
    receipt["graphql_repository_read"] = json!({"http_status":200,"immutable_node_matched":graph_match,
        "complete":complete,"errors":graph.errors.iter().map(|e|format!("{e:?}")).collect::<Vec<_>>()});
    if !complete {
        receipt["reason"] = json!("graphql_access_incomplete");
        return failure_code(credential);
    }
    receipt["dogfood_read"] = json!("Pass");
    if credential == Credential::ExistingCliOAuth {
        receipt["reason"] = json!("read_proven_with_cli_oauth_pat_unverified");
        BLOCKED
    } else {
        0
    }
}
fn failure_code(credential: Credential) -> u8 {
    if credential == Credential::ExistingCliOAuth {
        BLOCKED
    } else {
        1
    }
}
fn endpoint_error(
    receipt: &mut Value,
    endpoint: &str,
    error: &AccessError,
    credential: Credential,
) -> u8 {
    receipt[endpoint] = observed_error(error);
    if error.kind == AccessKind::SsoRequired {
        receipt["sso_challenge"] = json!("observed")
    }
    receipt["dogfood_read"] = json!("Fail");
    failure_code(credential)
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut receipt = json!({"scenario":"LIVE-001","repository":REPOSITORY,"expected_github_user_id":USER_ID,
        "expected_github_repository_id":REPO_ID,"authenticated_user":"not_attempted","private_repository_read":"not_attempted",
        "graphql_repository_read":"not_attempted","credential_kind":"unverified","dogfood_read":"not_run","sso_challenge":"not_tested"});
    let code = match Options::parse(env::args().skip(1))
        .and_then(|options| options.credential().map(|c| (options, c)))
    {
        Ok((options, (credential, token))) => {
            receipt["sso_challenge"] = json!("no_explicit_challenge_observed");
            probe(options, credential, token, &mut receipt).await
        }
        Err(reason) => {
            receipt["reason"] = json!(reason);
            BLOCKED
        }
    };
    receipt["status"] = json!(match code {
        0 => "Pass",
        BLOCKED => "Blocked",
        _ => "Fail",
    });
    println!("{receipt}");
    ExitCode::from(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scope_and_credential_options_are_explicit() {
        let parse = |s: &str| Options::parse(s.split_ascii_whitespace().map(String::from));
        let expected = "--repo joshmarkovic/threadline --expected-user-id 52184130 --read-only --credential-stdin";
        assert!(parse(expected).is_ok());
        for bad in [
            expected.replace("joshmarkovic/threadline", "outsider/example"),
            expected.replace("52184130", "42"),
            expected.replace("--read-only", ""),
            expected.replace("--credential-stdin", ""),
            format!("{expected} --credential-env"),
            format!("{expected} --token synthetic-secret"),
        ] {
            assert!(parse(&bad).is_err());
        }
        assert!(parse(&format!("{expected} --allow-existing-cli-oauth")).is_ok());
        assert!(
            parse(&expected.replace(
                "--credential-stdin",
                "--credential-env --allow-existing-cli-oauth"
            ))
            .is_err()
        );
    }
    #[test]
    fn fixture_prefixes_cannot_turn_oauth_or_actions_into_pat_evidence() {
        assert!(matches!(
            Credential::from_token("ghp_synthetic", false),
            Some(Credential::ClassicPat)
        ));
        assert!(matches!(
            Credential::from_token("github_pat_synthetic", false),
            Some(Credential::FineGrainedPat)
        ));
        assert!(Credential::from_token("gho_synthetic", false).is_none());
        assert!(matches!(
            Credential::from_token("gho_synthetic", true),
            Some(Credential::ExistingCliOAuth)
        ));
        for prefix in ["", "ghs_synthetic", "ghu_synthetic", "unknown"] {
            assert!(Credential::from_token(prefix, true).is_none());
        }
    }
}
