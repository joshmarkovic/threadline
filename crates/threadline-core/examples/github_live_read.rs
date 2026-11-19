//! Explicit read-only verification for the user-designated dogfood repository.
use serde_json::{Value, json};
use std::{
    env,
    io::{self, IsTerminal, Read},
    num::NonZeroU64,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};
use threadline_core::{
    domain::{ItemKind, ItemNumber, RepositoryApiId, RepositoryName},
    github::{
        AccessError, AccessKind, CancellationToken, GitHubClient, GraphIssue, OwnerScope,
        SecretToken,
        reader::{GitHubReader, IssueSummary, Page, refresh::ReadValidators},
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
    first_issue_pr_page: bool,
    pr_2_activity: bool,
}
impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, &'static str> {
        let mut args = args.into_iter();
        let (mut repo, mut id, mut read_only, mut source, mut allow_cli) =
            (None, None, false, None, false);
        let mut first_issue_pr_page = false;
        let mut pr_2_activity = false;
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
                "--first-issue-pr-page" if !first_issue_pr_page => first_issue_pr_page = true,
                "--pr-2-activity" if !pr_2_activity => pr_2_activity = true,
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
        Ok(Self {
            source,
            allow_cli,
            first_issue_pr_page,
            pr_2_activity,
        })
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
    let (repo, reader) = match session.private_reader(&route, id, &cancel).await {
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
    let page_failed = if options.first_issue_pr_page {
        let observed = match reader.list(1, &cancel).await {
            Ok(page) => page_receipt(&page, SystemTime::now()),
            Err(error) => {
                if error.kind == AccessKind::SsoRequired {
                    receipt["sso_challenge"] = json!("observed");
                }
                json!({"status":"Fail", "page":1, "error":observed_error(&error)})
            }
        };
        let failed = observed["status"] == "Fail";
        receipt["issue_pr_page"] = observed;
        failed
    } else {
        false
    };
    let activity_failed = if options.pr_2_activity {
        let mut activity = json!({"pull_request":2,"page_limit":1,"lookback_hours":24});
        let result = monitor_page(&reader, &cancel, &mut activity).await;
        activity["status"] = json!(if result.is_ok() { "Pass" } else { "Fail" });
        if let Err(reason) = result {
            activity["error"] = reason;
        }
        let failed = activity["status"] == "Fail";
        receipt["pr_activity"] = activity;
        failed
    } else {
        false
    };
    if credential == Credential::ExistingCliOAuth {
        receipt["reason"] = json!("read_proven_with_cli_oauth_pat_unverified");
        BLOCKED
    } else if activity_failed {
        receipt["reason"] = json!("pr_activity_read_incomplete");
        1
    } else if page_failed {
        receipt["reason"] = json!("issue_pr_page_failed");
        1
    } else {
        0
    }
}

async fn monitor_page(
    reader: &GitHubReader<'_>,
    cancel: &CancellationToken,
    receipt: &mut Value,
) -> Result<(), Value> {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|clock| i64::try_from(clock.as_millis()).ok())
        .and_then(|now| now.checked_sub(86_400_000))
        .ok_or_else(|| json!({"kind":"ClockUnavailable"}))?;
    let validators = ReadValidators::default();
    let number = ItemNumber::new(NonZeroU64::new(2).expect("fixed PR 2"));
    let no_page = || json!({"kind":"UnexpectedNotModified","http_status":304});

    receipt["route"] = json!("GET /repositories/{id}/pulls");
    let index = reader
        .open_pull_index(1, &validators, cancel)
        .await
        .map_err(|error| observed_error(&error))?
        .value
        .ok_or_else(no_page)?;
    let indexed = index.records.iter().find(|row| row.number == number);
    receipt["open_index"] = json!({"http_status":200,"records":index.records.len(),"next_page":index.next_page.is_some(),
        "partial":index.sso_limited,"target_present":indexed.is_some()});

    receipt["route"] = json!("GET /repositories/{id}/pulls/comments");
    let inline = reader
        .updated_inline_comments(since, 1, &validators, cancel)
        .await
        .map_err(|error| observed_error(&error))?
        .value
        .ok_or_else(no_page)?;
    receipt["inline_comments"] = json!({"http_status":200,"records":inline.records.len(),"next_page":inline.next_page.is_some(),
        "partial":inline.sso_limited,"target_records":inline.records.iter().filter(|row|row.issue==number).count()});

    receipt["route"] = json!("POST /graphql (ThreadlinePullActivity)");
    let page = reader
        .pull_activity(number, since, None, cancel)
        .await
        .map_err(|error| observed_error(&error))?;
    let matched = indexed
        .map(|candidate| candidate.id == page.identity.id && candidate.node == page.identity.node);
    receipt["timeline"] = json!({"http_status":200,"events":page.events.len(),"next_page":page.next.is_some(),
        "partial":page.partial,"permission_limited":page.permission_limited,"index_identity_matched":matched});
    if matched == Some(false) {
        return Err(json!({"kind":"TargetIdentityMismatch"}));
    }
    if index.sso_limited || inline.sso_limited || page.partial {
        return Err(json!({"kind":"PartialPage"}));
    }
    Ok(())
}

fn page_receipt(page: &Page<IssueSummary>, now: SystemTime) -> Value {
    const MAX_PAGE: usize = 20;
    if page.records.len() > MAX_PAGE {
        return json!({"status":"Fail", "page":1, "error":{"kind":"MalformedResponse","http_status":200,"reachability":"ReceivedResponse"}});
    }
    let pull_requests = page
        .records
        .iter()
        .filter(|record| record.kind == ItemKind::PullRequest)
        .count();
    let records = page.records.len();
    let oldest = page
        .records
        .iter()
        .filter_map(|record| record.created_at_ms)
        .min();
    let age_days = now
        .duration_since(UNIX_EPOCH)
        .ok()
        .zip(oldest)
        .and_then(|(now, created)| now.as_millis().checked_sub(u128::try_from(created).ok()?))
        .and_then(|age| u64::try_from(age / 86_400_000).ok());
    let coverage = if page.sso_limited {
        "partial_sso"
    } else if page.next_page.is_some() {
        "partial_pagination"
    } else {
        "complete"
    };
    json!({"status":"Pass", "page":1, "http_status":200, "coverage":coverage,
        "records":records, "issues":records - pull_requests, "pull_requests":pull_requests,
        "has_next_page":page.next_page.is_some(), "oldest_age_days":age_days})
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
    let parsed = Options::parse(env::args().skip(1));
    if parsed
        .as_ref()
        .is_ok_and(|options| options.first_issue_pr_page)
    {
        receipt["issue_pr_page"] = json!("not_attempted");
    }
    let code = match parsed.and_then(|options| options.credential().map(|c| (options, c))) {
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
        assert!(
            parse(&format!("{expected} --pr-2-activity"))
                .unwrap()
                .pr_2_activity
        );
        for bad in [
            expected.replace("joshmarkovic/threadline", "outsider/example"),
            expected.replace("52184130", "42"),
            expected.replace("--read-only", ""),
            expected.replace("--credential-stdin", ""),
            format!("{expected} --credential-env"),
            format!("{expected} --token synthetic-secret"),
            format!("{expected} --pr-2-activity --pr-2-activity"),
            format!("{expected} --pr-2-activity --pr 3"),
        ] {
            assert!(parse(&bad).is_err());
        }
        assert!(parse(&format!("{expected} --allow-existing-cli-oauth")).is_ok());
        assert!(
            parse(&format!("{expected} --first-issue-pr-page"))
                .unwrap()
                .first_issue_pr_page
        );
        assert!(
            parse(&format!(
                "{expected} --first-issue-pr-page --first-issue-pr-page"
            ))
            .is_err()
        );
        assert!(parse(&format!("{expected} --first-issue-pr-page --page 2")).is_err());
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
