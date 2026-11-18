# GitHub access and permissions

Threadline binds each session to the numeric user ID returned by GitHub and the resource owner selected for that credential. Private repository verification checks the owner, repository name, private flag and any previously known numeric repository ID. The production client sends its credential to `https://api.github.com` in an authorization header. It rejects redirects and uses a five-second connection timeout and a 15-second request timeout. REST requests identify the application and pin API version `2026-03-10`. [GitHub REST API versions](https://docs.github.com/en/rest/about-the-rest-api/api-versions)

| Sprint 03 request | Evidence and fine-grained personal access token requirements |
| --- | --- |
| `GET /user` | Reads the authenticated identity. This endpoint requires no additional fine-grained permissions. [Get the authenticated user](https://docs.github.com/en/rest/users/users#get-the-authenticated-user) |
| `GET /repos/{owner}/{repo}` | Requires Metadata read for a private repository. An entity tag can revalidate the same user's cached repository by route and immutable ID. [Get a repository](https://docs.github.com/en/rest/repos/repos#get-a-repository) |
| `GET /user/repos` | Requires Metadata read for private results. Discovery exposes only the selected owner's repositories, including forks. A personal account can be the selected owner. Each next-page link must advance on the same endpoint. [List repositories for the authenticated user](https://docs.github.com/en/rest/repos/repos#list-repositories-for-the-authenticated-user) · [REST pagination](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api) |
| `POST /graphql` | The fixed read-only query uses owner and repository name as variables. It returns the node ID, owner, name, visibility and viewer role. Private access requires visibility of that repository. A partial response preserves the available fields and typed errors. [Forming calls with GraphQL](https://docs.github.com/en/graphql/guides/forming-calls-with-graphql) |

A successful metadata read confirms that read for the active credential. Repository roles and GraphQL viewer permissions describe the user's access. Issue, review and merge grants remain unverified until the corresponding endpoint establishes them. Prefer a fine-grained token for the selected owner and repositories. Organization policy and endpoint compatibility govern use of a classic token. [Managing personal access tokens](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens)

| Later operation | Fine-grained repository permission accepted by the endpoint |
| --- | --- |
| Update issue | Issues write or Pull requests write. [Update an issue](https://docs.github.com/en/rest/issues/issues#update-an-issue) |
| Post an issue or pull-request conversation comment | Issues write or Pull requests write. [Create an issue comment](https://docs.github.com/en/rest/issues/comments#create-an-issue-comment) |
| Create or submit a pull-request review | Pull requests write. [Create a review](https://docs.github.com/en/rest/pulls/reviews#create-a-review-for-a-pull-request) · [Submit a review](https://docs.github.com/en/rest/pulls/reviews#submit-a-review-for-a-pull-request) |
| Merge a pull request | Contents write. Branch rules, the current head and merge eligibility still apply. [Merge a pull request](https://docs.github.com/en/rest/pulls/pulls#merge-a-pull-request) |

The provider records HTTP reachability independently of the access decision. A 401 indicates a rejected credential. A 403 can indicate missing permission, a rate limit, or an explicit organization approval or single sign-on requirement. A 404 preserves the ambiguity between a missing resource and a hidden private one. A `partial-results` SSO advisory keeps discovery incomplete across later pages. User-initiated SSO actions accept only a GitHub HTTPS organization authorization URL for the active owner. The browser receives no API credential. [REST authentication](https://docs.github.com/en/rest/authentication/authenticating-to-the-rest-api) · [REST rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)

## Live read receipt

The read-only harness checks the designated private repository `joshmarkovic/threadline` (ID `1462273296`) after verifying GitHub user `joshmarkovic` (ID `52184130`). It reads that one repository through REST and GraphQL. It reports the credential type, identity match, private metadata result, GraphQL completeness and any explicit SSO challenge separately. GitHub's token prefixes distinguish fine-grained and classic personal access tokens from OAuth and installation credentials. [GitHub credential types](https://docs.github.com/en/organizations/managing-programmatic-access-to-your-organization/github-credential-types)

With the approved PAT already provisioned in `THREADLINE_LIVE_PAT`, run the following command. A protected credential helper may instead pipe the token to `--credential-stdin`. The optional `--allow-existing-cli-oauth` flag accepts an existing piped OAuth credential for a separately labeled read observation. A successful OAuth read leaves LIVE-001 blocked. A repository owned by a personal account cannot establish that an organization's SSO challenge and authorization flow works.

Add `--first-issue-pr-page` to make one all-state issue and pull request list request after repository identity checks. The receipt contains counts, coverage and the oldest reported creation age in days for that page.

```sh
cargo run --locked -p threadline-core --example github_live_read -- \
  --read-only --repo joshmarkovic/threadline --expected-user-id 52184130 --credential-env
```
