# Sprint 03 GitHub access

Threadline 0.1.0 · Windows first release and portable GitHub provider · November 18, 2026

The development app can verify a GitHub user, select a private repository, replace an expired credential for the same account and sign out while retaining local work. The provider and protected-store tests used synthetic personal access tokens (PATs). A read-only check also reached the designated private repository on GitHub with an existing GitHub CLI OAuth credential. Live PAT authentication and a real organization single sign-on (SSO) challenge remain unverified. [Sprint requirements](../spec/sprints/sprint_03_github_access.md) · [Endpoint and permission decisions](../architecture/github-api.md) · [Live receipt](evidence/sprint-03/github-dogfood/github-read.json)

## Implementation

The app verifies the user and the private repository separately. It binds each credential to its original GitHub account and owner. Repository selection, removal, discovery completeness, last successful access check and scoped failure state persist in SQLite. The Windows adapter stores the secret in Credential Manager. Ordinary quit retains the credential. Sign-out releases the protected entry and blocks networking and queued actions while retaining cached work. A replacement credential must verify the original user and selected repository before affected work can proceed. [Account controller](../../crates/threadline-app/src/accounts.rs) · [SQLite access contract](../../crates/threadline-storage/src/access.rs)

The provider checks GitHub origins and owner scope, validates repository identity, follows paginated responses and supports cancellation. It separates API reachability from invalid credentials, SSO restrictions, rate limits and service failures. Partial SSO discovery and partial GraphQL results retain their warnings. The app can offer a validated GitHub authorization link. [Provider tests](../../crates/threadline-core/tests/github_access.rs)

## Verification

| Lane and scope | Command or evidence | Result |
| --- | --- | --- |
| Linux x64 unit | `cargo xtask test unit --seed 12345 --report-dir <report-dir>` | Pass. Nine cases ran 43 tests, including credential routing and secret redaction. |
| Linux x64 integration | `cargo xtask test integration --seed 12345 --report-dir <report-dir>` | Pass. Twelve cases ran 45 tests against loopback HTTP, SQLite and platform contracts. Five IT-004 cases and one IT-005 case cover provider identity, conditional reads, SSO, pagination, GraphQL, routing and cancellation. |
| Linux X11 development app, isolated profile | `cargo xtask test e2e --suite smoke --case E2E-001 --binary <binary-path> --seed 12345 --report-dir <report-dir>` | Pass. The external X11 driver completed nine actions and verified that quit and relaunch retained the same SQLite digest. |
| Windows 11 x64 | Native provider, protected credential and account checks | All six loopback HTTP checks passed. Windows exercised the native protected store. Three corrected account integration tests passed in each of three runs, including identity substitution. The corrected app test executable had SHA-256 `356da8c7074a93bb5540296f677d5f3f5cd1c94ecc73b61ed5e99877cab52892`. |
| Live read-only dogfood | Authenticated GitHub.com user, exact private repository and GraphQL repository identity | Read passed. All three requests received HTTP 200. GitHub reported repository metadata access. The credential came from an existing CLI session. The user-owned repository returned no SSO challenge. LIVE-001 remains Blocked pending a PAT with authorized organization SSO coverage. |

The Linux runs used clean revision `43a1cead5e3b39bcdcf12c09ea6948aaa16109ef` and the new GitHub cases used fixture `s03-github-access-v1`. The curated receipts retain source, lock digest, fixture, seed, host, commands and case outcomes. [Development verification](evidence/sprint-03/verification.json) · [Live runner result](evidence/sprint-03/github-dogfood/result.json)

The first hosted run passed Ubuntu, failed the Windows portable unit step and Windows account GUI step, and cancelled the macOS build/core job. The Windows socket fixture needed blocking mode on accepted connections. The subsequent Windows 11 account checks above exercised the fix. [Original hosted run](https://github.com/joshmarkovic/threadline/actions/runs/44373987834) · [Corrective hosted run](https://github.com/joshmarkovic/threadline/actions/runs/44377465944)

The corrective run at 7de6b4c passed Ubuntu. The Windows and macOS integration jobs exposed two further portability defects in the test fixtures. Revision `cb74155` corrected the fixtures and its Windows, macOS and Linux build/core jobs all passed. The Windows account GUI journey failed before its first authenticated user request. The external E2E-002 gate remains open. [First correction](https://github.com/joshmarkovic/threadline/actions/runs/44377465944) · [Cross-platform fixture correction](https://github.com/joshmarkovic/threadline/actions/runs/44380442934)

The hosted run at `4884eca` started zero steps in all four attempted jobs. GitHub reported an account billing or spending-limit block. Those jobs supplied no test results. [Hosted job status](https://github.com/joshmarkovic/threadline/actions/runs/44385602312)

## Handoff

Sprint 04 uses the persisted account and repository selection to show the cached workspace. Sprint 05 connects selection, sign-out and authorization to scheduled reads. Later writer sprints verify permissions for each mutation. The installed Windows release and macOS/Linux release reruns retain separate gates. [Acceptance ledger](../spec/acceptance_traceability.csv) · [Platform scenarios](../spec/test_platform_traceability.csv)
