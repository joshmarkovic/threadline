use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use serde::Serialize;
use serde_json::{Value, json};

mod activity;
mod backlog;
mod comment_dependency;
mod concurrent;
mod historical_reader;
mod history;
mod lifecycle;
mod merge;
mod pr;
mod rate_limit;
mod review_decisions;
mod review_threads;
mod writes;

const MAX_REQUEST: u64 = 16 * 1024;
const FIXTURE_AUTHORIZATION: &str = "Bearer threadline-test-fixture";

pub fn contains_fixture_credential(bytes: &[u8]) -> bool {
    let credential = FIXTURE_AUTHORIZATION
        .strip_prefix("Bearer ")
        .expect("fixed scheme")
        .as_bytes();
    bytes
        .windows(credential.len())
        .any(|window| window == credential)
}

const USER: &str = "/user";
const DISCOVERY: &str = "/user/repos?per_page=100&page=1";
const REPOSITORY: &str = "/repos/fixture-org/private";
const ITEMS: &str =
    "/repos/fixture-org/private/issues?state=all&sort=created&direction=desc&per_page=20&page=1";
const PR_STATE_ITEMS_NEXT: &str =
    "/repos/fixture-org/private/issues?state=all&sort=created&direction=desc&per_page=20&page=2";
const ITEM: &str = "/repos/fixture-org/private/issues/41";
const SCHEDULED_ISSUE: &str = "/repositories/71/issues/41";
const COMMENTS_FIRST: &str = "/repos/fixture-org/private/issues/41/comments?per_page=20&page=1";
const COMMENTS_NEXT: &str = "/repos/fixture-org/private/issues/41/comments?per_page=20&page=2";
const TIMELINE: &str = "/repos/fixture-org/private/issues/41/timeline?per_page=20&page=1";
const SYNC_ITEMS: &str = "/repositories/71/issues?state=all&sort=updated&direction=asc&since=";
const SYNC_COMMENTS: &str = "/repositories/71/issues/comments?sort=updated&direction=asc&since=";
const SYNC_INLINE: &str = "/repositories/71/pulls/comments?sort=updated&direction=asc&since=";
const AUTHORIZATION_URL: &str =
    "https://github.com/orgs/fixture-org/sso?authorization_request=fixture123";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Ready,
    Partial,
    Sso,
    Denied,
    Revoked,
    HistoricalLocal,
    HistoricalRemote,
    ReaderFailure,
    ReaderResume,
    SyncLive,
    SyncOffline,
    SyncService,
    SyncThrottle,
    SyncRateReady,
    SyncRateThrottle,
    SyncRateFloor,
    SyncTransport,
    SyncNotificationSend,
    PrLoad,
    PrMergeState,
    PrDelay,
    PrRelease,
    PrSharedHold,
    PrSharedReady,
    PrSharedRelease,
    ReviewSend,
    ReviewChanged,
    ReviewDecisions,
    ReviewPredecessor,
    ReviewResume,
    ThreadQueue,
    ThreadOutdated,
    MergePrepare,
    MergeSend,
    MergeWrongHead,
    MergeQueuePreflight,
    MergeQueueRejected,
    CredentialReady,
    CredentialOffline,
    CredentialOther,
    CredentialReturn,
    History,
    HistoryMoved,
    HistoryDeleted,
    WriteOnline,
    WriteDrop,
    IssueEditBaseline,
    IssueEditRemoteChange,
    IssueEditCommit,
    BacklogAuth,
    BacklogHold,
    BacklogDrain,
    BacklogConcurrent,
    LifecycleOffline,
    LifecycleReady,
    LifecycleDelay,
    LifecycleCrash,
    LifecycleRecovery,
    LifecycleSignedOut,
    LifecycleReturn,
    ActivityBaseline,
    ActivityFresh,
    ActivityOffline,
    ActivityCatchup,
    ActivityReplay,
    CommentDependencyOffline,
    CommentDependencyCreate,
    CommentDependencyResume,
    CommentDependencyLost,
}

impl Phase {
    fn read(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path)?.trim() {
            "ready" => Ok(Self::Ready),
            "partial" => Ok(Self::Partial),
            "sso" => Ok(Self::Sso),
            "denied" => Ok(Self::Denied),
            "revoked" => Ok(Self::Revoked),
            "historical-local" => Ok(Self::HistoricalLocal),
            "historical-remote" => Ok(Self::HistoricalRemote),
            "reader-failure" => Ok(Self::ReaderFailure),
            "reader-resume" => Ok(Self::ReaderResume),
            "sync-live" => Ok(Self::SyncLive),
            "sync-offline" => Ok(Self::SyncOffline),
            "sync-service" => Ok(Self::SyncService),
            "sync-throttle" => Ok(Self::SyncThrottle),
            "sync-rate-ready" => Ok(Self::SyncRateReady),
            "sync-rate-throttle" => Ok(Self::SyncRateThrottle),
            "sync-rate-floor" => Ok(Self::SyncRateFloor),
            "sync-transport" => Ok(Self::SyncTransport),
            "sync-notification-send" => Ok(Self::SyncNotificationSend),
            "pr-load" => Ok(Self::PrLoad),
            "pr-merge-state" => Ok(Self::PrMergeState),
            "pr-delay" => Ok(Self::PrDelay),
            "pr-release" => Ok(Self::PrRelease),
            "pr-shared-hold" => Ok(Self::PrSharedHold),
            "pr-shared-ready" => Ok(Self::PrSharedReady),
            "pr-shared-release" => Ok(Self::PrSharedRelease),
            "review-send" => Ok(Self::ReviewSend),
            "review-changed" => Ok(Self::ReviewChanged),
            "review-decisions" => Ok(Self::ReviewDecisions),
            "review-predecessor" => Ok(Self::ReviewPredecessor),
            "review-resume" => Ok(Self::ReviewResume),
            "thread-queue" => Ok(Self::ThreadQueue),
            "thread-outdated" => Ok(Self::ThreadOutdated),
            "merge-prepare" => Ok(Self::MergePrepare),
            "merge-send" => Ok(Self::MergeSend),
            "merge-wrong-head" => Ok(Self::MergeWrongHead),
            "merge-queue-preflight" => Ok(Self::MergeQueuePreflight),
            "merge-queue-rejected" => Ok(Self::MergeQueueRejected),
            "credential-ready" => Ok(Self::CredentialReady),
            "credential-offline" => Ok(Self::CredentialOffline),
            "credential-other" => Ok(Self::CredentialOther),
            "credential-return" => Ok(Self::CredentialReturn),
            "history" => Ok(Self::History),
            "history-moved" => Ok(Self::HistoryMoved),
            "history-deleted" => Ok(Self::HistoryDeleted),
            "write-online" => Ok(Self::WriteOnline),
            "write-drop" => Ok(Self::WriteDrop),
            "issue-edit-baseline" => Ok(Self::IssueEditBaseline),
            "issue-edit-remote-change" => Ok(Self::IssueEditRemoteChange),
            "issue-edit-commit" => Ok(Self::IssueEditCommit),
            "backlog-auth" => Ok(Self::BacklogAuth),
            "backlog-hold" => Ok(Self::BacklogHold),
            "backlog-drain" => Ok(Self::BacklogDrain),
            "backlog-concurrent" => Ok(Self::BacklogConcurrent),
            "lifecycle-offline" => Ok(Self::LifecycleOffline),
            "lifecycle-ready" => Ok(Self::LifecycleReady),
            "lifecycle-delay" => Ok(Self::LifecycleDelay),
            "lifecycle-crash" => Ok(Self::LifecycleCrash),
            "lifecycle-recovery" => Ok(Self::LifecycleRecovery),
            "lifecycle-signed-out" => Ok(Self::LifecycleSignedOut),
            "lifecycle-return" => Ok(Self::LifecycleReturn),
            "activity-baseline" => Ok(Self::ActivityBaseline),
            "activity-fresh" => Ok(Self::ActivityFresh),
            "activity-offline" => Ok(Self::ActivityOffline),
            "activity-catchup" => Ok(Self::ActivityCatchup),
            "activity-replay" => Ok(Self::ActivityReplay),
            "comment-dependency-offline" => Ok(Self::CommentDependencyOffline),
            "comment-dependency-create" => Ok(Self::CommentDependencyCreate),
            "comment-dependency-resume" => Ok(Self::CommentDependencyResume),
            "comment-dependency-lost" => Ok(Self::CommentDependencyLost),
            _ => Err(io::Error::other("invalid access fixture phase")),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Route {
    User,
    Discovery,
    Repository,
    Graphql,
    Items,
    PrStateItemsNext,
    Item,
    ScheduledIssue,
    HistoricalSearch,
    HistoricalIssue,
    CommentsFirst,
    CommentsNext,
    Timeline,
    UpdatedItems,
    UpdatedComments,
    UpdatedInlineComments,
    OpenPullIndex,
    PullActivityA,
    PullActivityB,
    PullA,
    PullB,
    CompareA,
    CompareB,
    ReviewContextIssue,
    ReviewList,
    SharedPrReview,
    ReviewRead,
    ReviewCreated,
    ReviewSubmitted,
    ReviewDecision(review_decisions::DecisionStep),
    QueuedThreadList,
    QueuedThreadReviewList,
    QueuedThreadTarget,
    QueuedThreadReply,
    QueuedThreadResolve,
    MergePolicyA,
    MergePolicyB,
    MergePut,
    MergeQueueDeniedPut,
    MergeReadback,
    HistoryBranches,
    HistoryTags,
    HistoryBranch,
    HistoryFirst,
    HistoryOlder,
    HistoryParentDiff,
    HistoryPairDiff,
    HistoryDivergentPair,
    HistoryExactObject(history::ExactObject),
    CommentCreated,
    CommentReplyLost,
    IssueEditCommitted,
    BacklogIssue,
    BacklogPermission,
    BacklogComment(u8),
    BacklogFeed(u8),
    BacklogFeedRetry(u8),
    BacklogFeedMetadata(u16),
    BacklogCachedPr,
    BacklogArchive,
    BacklogAuxiliaryFeed,
    LifecycleComment,
    ActivityCheckRuns,
    ActivityReviews,
    ActivityThreads,
    ActivityCommitStatuses,
    ActivityBranches,
    ActivityWatched,
    ActivityBaselineTip,
    ActivityMovedTip,
    ActivityUnwatched,
    DependencyCommentCreated,
    DependencyCommentRead,
    DependencyCommentEdited,
    DependencyCommentLost,
    DependencyUnexpectedRead,
    DependencyUnexpectedEdit,
    DependencyWrongCommentId,
    DependencyUnexpectedCommentQuery,
    DependencyEditBodyMismatch,
    DependencyRepeatedOriginalCreate,
    DependencyCreateUsedForEdit,
    DependencyUnexpectedCreate,
    DependencyUnexpectedGraphql,
    DependencyUnexpectedPost,
    DependencyUnexpectedMethod,
    Unexpected,
}

#[derive(Serialize)]
struct RequestEvidence {
    phase: Phase,
    route: Route,
    fixture_authorization: bool,
    status: u16,
}

pub struct AccessFixture {
    address: SocketAddr,
    phase_file: PathBuf,
    stopping: Arc<AtomicBool>,
    worker: Option<JoinHandle<FixtureRun>>,
}

#[derive(Default)]
struct FixtureRun {
    records: Vec<RequestEvidence>,
    error: Option<io::Error>,
}

pub enum TraceCheck {
    HistoricalReader,
    Observe,
    AccountJourney,
    OfflineReader,
    LargeWorkspaceBacklog,
    ConcurrentReader,
    NativeLifecycle,
    NativeActivity,
    CommentDependency,
    IssueEditConflict,
    IssueEditPropagation,
    ReaderResume,
    SyncJourney,
    RateLimitedCache,
    NotificationSchedule,
    PullRequestJourney,
    PullRequestMergeState,
    SharedPrPanes,
    ReviewContextJourney,
    ReviewDraftJourney,
    ReviewDecisionsJourney,
    ReviewPredecessorJourney,
    ReviewThreadsJourney,
    MergeJourney,
    MergeQueueJourney,
    CredentialBoundary,
    HistoryJourney,
    WritesWithLoss,
    WriteSurface,
}

impl AccessFixture {
    pub fn start(profile: &Path) -> io::Result<Self> {
        let phase_file = profile.join("access-fixture-phase.txt");
        fs::write(&phase_file, "ready\n")?;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let stopping = Arc::new(AtomicBool::new(false));
        let worker = thread::Builder::new()
            .name("access-fixture".into())
            .spawn({
                let phase_file = phase_file.clone();
                let stopping = stopping.clone();
                move || {
                    let mut records = Vec::new();
                    let error = serve(listener, &phase_file, &stopping, &mut records).err();
                    FixtureRun { records, error }
                }
            })?;
        Ok(Self {
            address,
            phase_file,
            stopping,
            worker: Some(worker),
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn phase_file(&self) -> &Path {
        &self.phase_file
    }

    pub fn finish(mut self, report_dir: &Path, check: TraceCheck) -> io::Result<()> {
        let FixtureRun { records, error } = self.stop()?;
        let trace = serde_json::to_vec_pretty(&records)?;
        fs::write(report_dir.join("github-access-trace.json"), trace)?;
        if let Some(error) = error {
            let reason = match error.to_string().as_str() {
                "too many requests to the access fixture" => "request limit reached",
                "incomplete access fixture request" => "incomplete request",
                "invalid access fixture phase" => "invalid fixture phase",
                "paint acknowledged before the feed request" => "paint preceded its request",
                _ => "request or fixture I/O failed",
            };
            let diagnostic = json!({
                "reason": reason,
                "error_kind": format!("{:?}", error.kind()),
                "os_error": error.raw_os_error(),
                "completed_requests": records.len(),
            });
            fs::write(
                report_dir.join("github-access-trace-error.json"),
                serde_json::to_vec_pretty(&diagnostic)?,
            )?;
            return Err(error);
        }
        let valid = match check {
            TraceCheck::Observe => true,
            TraceCheck::AccountJourney => complete_journey(&records),
            TraceCheck::OfflineReader => records.is_empty(),
            TraceCheck::LargeWorkspaceBacklog => backlog::trace(&records),
            TraceCheck::ConcurrentReader => concurrent::trace(&records),
            TraceCheck::NativeLifecycle => lifecycle::trace(&records),
            TraceCheck::NativeActivity => activity::trace(&records),
            TraceCheck::CommentDependency => comment_dependency::trace(&records),
            TraceCheck::IssueEditConflict => writes::issue_conflict_trace(&records),
            TraceCheck::IssueEditPropagation => writes::issue_propagation_trace(&records),
            TraceCheck::HistoricalReader => historical_reader::trace(&records),
            TraceCheck::ReaderResume => complete_reader_resume(&records),
            TraceCheck::SyncJourney => complete_sync_journey(&records),
            TraceCheck::RateLimitedCache => rate_limit::trace(&records),
            TraceCheck::NotificationSchedule => complete_notification_schedule(&records),
            TraceCheck::PullRequestJourney => pr::trace(&records),
            TraceCheck::PullRequestMergeState => pr::trace_merge_state(&records),
            TraceCheck::SharedPrPanes => pr::trace_shared(&records),
            TraceCheck::ReviewContextJourney => pr::trace_review_context(&records),
            TraceCheck::ReviewDraftJourney => pr::trace_review_draft(&records),
            TraceCheck::ReviewDecisionsJourney => review_decisions::trace(&records),
            TraceCheck::ReviewPredecessorJourney => review_decisions::predecessor_trace(&records),
            TraceCheck::ReviewThreadsJourney => review_threads::trace(&records),
            TraceCheck::MergeJourney => merge::trace(&records),
            TraceCheck::MergeQueueJourney => merge::trace_queue(&records),
            TraceCheck::CredentialBoundary => complete_credential_boundary(&records),
            TraceCheck::HistoryJourney => history::trace(&records),
            TraceCheck::WritesWithLoss => writes::trace(&records, true),
            TraceCheck::WriteSurface => writes::trace(&records, false),
        };
        if !valid {
            return Err(io::Error::other(
                "the synthetic GitHub trace did not match the expected journey",
            ));
        }
        Ok(())
    }

    fn stop(&mut self) -> io::Result<FixtureRun> {
        self.stopping.store(true, Ordering::Release);
        match self.worker.take() {
            Some(worker) => worker
                .join()
                .map_err(|_| io::Error::other("access fixture worker stopped unexpectedly")),
            None => Ok(FixtureRun::default()),
        }
    }
}

fn complete_reader_resume(records: &[RequestEvidence]) -> bool {
    let discussion: Vec<_> = records
        .iter()
        .filter(|entry| matches!(entry.route, Route::CommentsFirst | Route::CommentsNext))
        .map(|entry| (entry.phase, entry.route, entry.status))
        .collect();
    records
        .iter()
        .all(|entry| entry.route != Route::Unexpected && entry.fixture_authorization)
        && records
            .iter()
            .any(|entry| entry.route == Route::Items && entry.status == 200)
        && discussion
            == [
                (Phase::ReaderFailure, Route::CommentsFirst, 200),
                (Phase::ReaderFailure, Route::CommentsNext, 502),
                (Phase::ReaderResume, Route::CommentsNext, 200),
            ]
}

fn complete_notification_schedule(records: &[RequestEvidence]) -> bool {
    let writes: Vec<_> = records
        .iter()
        .filter(|entry| matches!(entry.route, Route::CommentCreated | Route::CommentReplyLost))
        .map(|entry| (entry.phase, entry.route, entry.status))
        .collect();
    records.iter().all(|entry| {
        entry.fixture_authorization
            && entry.route != Route::Unexpected
            && entry.phase != Phase::SyncOffline
    }) && [Route::UpdatedItems, Route::UpdatedComments]
        .into_iter()
        .all(|route| {
            records.iter().any(|entry| {
                entry.phase == Phase::SyncLive && entry.route == route && entry.status == 200
            })
        })
        && writes == [(Phase::SyncNotificationSend, Route::CommentCreated, 201)]
        && !records.iter().any(|entry| {
            entry.phase == Phase::SyncNotificationSend
                && matches!(
                    entry.route,
                    Route::UpdatedItems
                        | Route::UpdatedComments
                        | Route::UpdatedInlineComments
                        | Route::OpenPullIndex
                )
        })
        && records.iter().any(|entry| {
            entry.phase == Phase::SyncService && entry.route == Route::User && entry.status == 503
        })
}

fn complete_sync_journey(records: &[RequestEvidence]) -> bool {
    let promised = |phase, route, status| {
        records
            .iter()
            .any(|entry| entry.phase == phase && entry.route == route && entry.status == status)
    };
    records.iter().all(|entry| {
        entry.route != Route::Unexpected
            && entry.phase != Phase::SyncOffline
            && entry.fixture_authorization
    }) && promised(Phase::SyncLive, Route::UpdatedItems, 200)
        && promised(Phase::SyncLive, Route::UpdatedComments, 200)
        && promised(Phase::SyncService, Route::User, 503)
        && promised(Phase::SyncThrottle, Route::User, 429)
        && promised(Phase::SyncTransport, Route::User, 0)
}

impl Drop for AccessFixture {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn complete_journey(records: &[RequestEvidence]) -> bool {
    let observed = |phase, route, status| {
        records.iter().any(|entry| {
            entry.phase == phase
                && entry.route == route
                && entry.status == status
                && entry.fixture_authorization
        })
    };
    records
        .iter()
        .all(|entry| entry.route != Route::Unexpected && entry.fixture_authorization)
        && observed(Phase::Ready, Route::User, 200)
        && observed(Phase::Ready, Route::Repository, 200)
        && observed(Phase::Partial, Route::Discovery, 200)
        && observed(Phase::Sso, Route::Repository, 403)
        && observed(Phase::Denied, Route::Repository, 403)
        && observed(Phase::Revoked, Route::User, 401)
}

fn complete_credential_boundary(records: &[RequestEvidence]) -> bool {
    let successful = |phase, route| {
        records
            .iter()
            .filter(|row| row.phase == phase && row.route == route && row.status == 200)
            .count()
    };
    let effects: Vec<_> = records
        .iter()
        .filter(|row| matches!(row.route, Route::CommentCreated | Route::CommentReplyLost))
        .map(|row| (row.phase, row.route, row.status))
        .collect();
    records.iter().all(|row| {
        row.fixture_authorization
            && row.route != Route::Unexpected
            && row.phase != Phase::CredentialOffline
    }) && successful(Phase::CredentialReady, Route::User) >= 1
        && successful(Phase::CredentialOther, Route::User) >= 2
        && successful(Phase::CredentialOther, Route::Repository) >= 1
        && successful(Phase::CredentialReturn, Route::User) >= 1
        && successful(Phase::CredentialReturn, Route::Repository) >= 1
        && effects == [(Phase::CredentialReturn, Route::CommentCreated, 201)]
}

fn serve(
    listener: TcpListener,
    phase_file: &Path,
    stopping: &AtomicBool,
    evidence: &mut Vec<RequestEvidence>,
) -> io::Result<()> {
    while !stopping.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut socket, _)) => {
                let phase = Phase::read(phase_file)?;
                if evidence.len() >= if activity::active(phase) { 512 } else { 128 } {
                    return Err(io::Error::other("too many requests to the access fixture"));
                }
                let request = answer(&mut socket, phase, phase_file, stopping, evidence)?;
                evidence.push(request);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(12));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn answer(
    socket: &mut TcpStream,
    phase: Phase,
    phase_file: &Path,
    stopping: &AtomicBool,
    previous_requests: &[RequestEvidence],
) -> io::Result<RequestEvidence> {
    let timeout = Some(Duration::from_secs(2));
    socket.set_nonblocking(false)?;
    socket.set_read_timeout(timeout)?;
    socket.set_write_timeout(timeout)?;
    let (route, authorized, binding) = read_request(socket, phase)?;
    if authorized
        && route == Route::ReviewDecision(review_decisions::DecisionStep::ApprovalReadInterrupted)
    {
        review_decisions::interrupt_read(phase_file, stopping)?;
        return Ok(RequestEvidence {
            phase,
            route,
            fixture_authorization: true,
            status: 0,
        });
    }
    if authorized && let Some(status) = lifecycle::hold(phase, route, phase_file, stopping)? {
        return Ok(RequestEvidence {
            phase,
            route,
            fixture_authorization: true,
            status,
        });
    }
    if phase == Phase::PrDelay && route == Route::Item && authorized {
        pr::wait_for_release(phase_file, stopping)?;
    }
    if phase == Phase::PrSharedHold && route == Route::SharedPrReview && authorized {
        pr::wait_for_shared_review(phase_file, stopping)?;
    }
    if authorized
        && ((phase == Phase::SyncTransport && route == Route::User)
            || matches!(
                route,
                Route::CommentReplyLost | Route::DependencyCommentLost
            ))
    {
        return Ok(RequestEvidence {
            phase,
            route,
            fixture_authorization: true,
            status: 0,
        });
    }
    if authorized
        && phase == Phase::BacklogHold
        && matches!(route, Route::BacklogComment(_))
        && !backlog::await_release(phase_file, stopping)?
    {
        return Ok(RequestEvidence {
            phase,
            route,
            fixture_authorization: true,
            status: 0,
        });
    }
    let mut evidence_route = route;
    let mut held_feed = None;
    let mut next_feed_page = None;
    if let (true, Route::BacklogFeed(page), Some(target)) = (authorized, route, binding.as_deref())
    {
        match concurrent::hold(phase_file, stopping, page, socket)? {
            concurrent::FeedAttempt::Painted(request) => held_feed = Some(request),
            concurrent::FeedAttempt::AlreadyServed => {
                evidence_route = Route::BacklogFeedRetry(page)
            }
            concurrent::FeedAttempt::Interrupted => {
                return Ok(RequestEvidence {
                    phase,
                    route,
                    fixture_authorization: true,
                    status: 0,
                });
            }
        }
        next_feed_page = concurrent::next(target, page);
    }
    let succeeded = |expected| {
        previous_requests
            .iter()
            .any(|record| record.route == expected && record.status == 200)
    };
    let committed_issue =
        authorized && phase == Phase::IssueEditCommit && succeeded(Route::IssueEditCommitted);
    let reply = (authorized.then(|| review_threads::response(phase, route, binding.as_deref())))
        .flatten()
        .or_else(|| {
            committed_issue
                .then(|| writes::committed_read(route))
                .flatten()
        })
        .unwrap_or_else(|| response(phase, route, authorized, succeeded(Route::MergePut)));
    let body = serde_json::to_vec(&reply.body)?;
    write!(
        socket,
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        reply.status,
        reply.reason,
        body.len()
    )?;
    if let Some(sso) = reply.sso {
        write!(socket, "X-GitHub-SSO: {sso}\r\n")?;
    }
    if let Some(next) = next_feed_page {
        let address = socket.local_addr()?;
        write!(socket, "Link: <http://{address}{next}>; rel=\"next\"\r\n")?;
    }
    if phase == Phase::History && route == Route::HistoryFirst && authorized && reply.status == 200
    {
        let next = history::older_target();
        let address = socket.local_addr()?;
        write!(socket, "Link: <http://{address}{next}>; rel=\"next\"\r\n")?;
    }
    if route == Route::CommentCreated && reply.status == 201 {
        socket.write_all(b"X-GitHub-Request-Id: SYNTHETIC:4101:07\r\n")?;
    }
    if route == Route::DependencyCommentCreated && reply.status == 201 {
        socket
            .write_all(b"X-GitHub-Request-Id: SYNTHETIC:4101:08:CREATE\r\nRetry-After: 25\r\n")?;
    }
    if route == Route::DependencyCommentEdited && reply.status == 200 {
        socket.write_all(b"X-GitHub-Request-Id: SYNTHETIC:4101:08:EDIT\r\n")?;
    }
    if route == Route::QueuedThreadReply && reply.status == 201 {
        socket.write_all(b"X-GitHub-Request-Id: SYNTHETIC:772:08\r\n")?;
    }
    if let Route::BacklogComment(index) = route
        && reply.status == 201
    {
        write!(socket, "X-GitHub-Request-Id: SYNTHETIC:71:{index:02}\r\n")?;
    }
    if reply.status == 200 {
        match route {
            Route::ReviewCreated => {
                socket.write_all(b"X-GitHub-Request-Id: SYNTHETIC:4201:08\r\n")?
            }
            Route::ReviewSubmitted => {
                socket.write_all(b"X-GitHub-Request-Id: SYNTHETIC:4202:08\r\n")?
            }
            Route::ReviewDecision(step) => {
                write!(socket, "X-GitHub-Request-Id: SYNTHETIC:08:{step:?}\r\n")?
            }
            Route::QueuedThreadResolve => {
                socket.write_all(b"X-GitHub-Request-Id: SYNTHETIC:771:08\r\n")?
            }
            Route::MergePut => socket.write_all(b"X-GitHub-Request-Id: SYNTHETIC:4201:09\r\n")?,
            _ => (),
        }
    }
    if phase == Phase::SyncThrottle && route == Route::User && reply.status == 429 {
        socket.write_all(b"Retry-After: 2\r\n")?;
    }
    if phase == Phase::SyncRateThrottle && route == Route::User && reply.status == 429 {
        write!(socket, "Retry-After: {}\r\n", rate_limit::RETRY_SECONDS)?;
    }
    if phase == Phase::ReaderFailure && route == Route::CommentsFirst && reply.status == 200 {
        let local = socket.local_addr()?;
        write!(
            socket,
            "Link: <http://{local}{COMMENTS_NEXT}>; rel=\"next\"\r\n"
        )?;
    }
    if phase == Phase::PrMergeState && route == Route::Items && reply.status == 200 {
        let local = socket.local_addr()?;
        write!(
            socket,
            "Link: <http://{local}{PR_STATE_ITEMS_NEXT}>; rel=\"next\"\r\n"
        )?;
    }
    socket.write_all(b"\r\n")?;
    socket.write_all(&body)?;
    if let Some(request) = held_feed {
        request.response_written()?;
    }
    Ok(RequestEvidence {
        phase,
        route: evidence_route,
        fixture_authorization: authorized,
        status: reply.status,
    })
}

fn read_request(socket: &TcpStream, phase: Phase) -> io::Result<(Route, bool, Option<String>)> {
    let mut reader = BufReader::new(socket.take(MAX_REQUEST));
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_ascii_whitespace();
    let request = (parts.next(), parts.next(), parts.next());
    let is_get = request.0 == Some("GET");
    let post_path = match request {
        (Some("POST"), Some(path), Some("HTTP/1.1")) => Some(path.to_owned()),
        _ => None,
    };
    let put_path = match request {
        (Some("PUT"), Some(path), Some("HTTP/1.1")) => Some(path.to_owned()),
        _ => None,
    };
    let patch_path = match request {
        (Some("PATCH"), Some(path), Some("HTTP/1.1")) => Some(path.to_owned()),
        _ => None,
    };
    let mut route = match request {
        (Some("GET"), Some(target), Some("HTTP/1.1")) if phase == Phase::BacklogConcurrent => {
            concurrent::get(target)
        }
        (Some("GET"), Some(USER), Some("HTTP/1.1")) => Route::User,
        (Some("GET"), Some(DISCOVERY), Some("HTTP/1.1")) => Route::Discovery,
        (Some("GET"), Some(REPOSITORY), Some("HTTP/1.1")) => Route::Repository,
        (Some("GET"), Some(ITEMS), Some("HTTP/1.1")) => Route::Items,
        (Some("GET"), Some(PR_STATE_ITEMS_NEXT), Some("HTTP/1.1"))
            if phase == Phase::PrMergeState =>
        {
            Route::PrStateItemsNext
        }
        (Some("GET"), Some(ITEM), Some("HTTP/1.1")) => Route::Item,
        (Some("GET"), Some(SCHEDULED_ISSUE), Some("HTTP/1.1"))
            if matches!(
                phase,
                Phase::WriteOnline
                    | Phase::WriteDrop
                    | Phase::SyncRateReady
                    | Phase::SyncRateThrottle
                    | Phase::IssueEditBaseline
                    | Phase::IssueEditRemoteChange
                    | Phase::IssueEditCommit
                    | Phase::SyncNotificationSend
                    | Phase::CredentialReady
                    | Phase::CredentialOther
                    | Phase::CredentialReturn
            ) =>
        {
            Route::ScheduledIssue
        }
        (Some("GET"), Some(COMMENTS_FIRST), Some("HTTP/1.1")) => Route::CommentsFirst,
        (Some("GET"), Some(COMMENTS_NEXT), Some("HTTP/1.1")) => Route::CommentsNext,
        (Some("GET"), Some(TIMELINE), Some("HTTP/1.1")) => Route::Timeline,
        (Some("GET"), Some(target), Some("HTTP/1.1")) if sync_since(target, SYNC_ITEMS) => {
            Route::UpdatedItems
        }
        (Some("GET"), Some(target), Some("HTTP/1.1")) if sync_since(target, SYNC_COMMENTS) => {
            Route::UpdatedComments
        }
        (Some("GET"), Some(target), Some("HTTP/1.1")) if sync_since(target, SYNC_INLINE) => {
            Route::UpdatedInlineComments
        }
        (
            Some("GET"),
            Some("/repositories/71/pulls?state=open&sort=created&direction=asc&per_page=20&page=1"),
            Some("HTTP/1.1"),
        ) => Route::OpenPullIndex,
        (Some("POST"), Some("/graphql"), Some("HTTP/1.1")) => Route::Graphql,
        (Some("GET"), Some(target), Some("HTTP/1.1")) => historical_reader::get(phase, target)
            .or_else(|| review_decisions::get(phase, target))
            .or_else(|| comment_dependency::get(phase, target))
            .or_else(|| activity::read_route(phase, target))
            .or_else(|| lifecycle::read_route(phase, target))
            .or_else(|| backlog::read_route(phase, target))
            .or_else(|| history::route(phase, target))
            .or_else(|| pr::route(target))
            .unwrap_or(Route::Unexpected),
        _ => Route::Unexpected,
    };
    let mut binding = match route {
        Route::BacklogFeed(_) => request.1.map(str::to_owned),
        _ => None,
    };
    let dependency_failure =
        comment_dependency::classify_unexpected_request(phase, request.0, request.1);
    let mut authorized = false;
    let mut content_length = 0usize;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Err(io::Error::other("incomplete access fixture request"));
        }
        if line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("authorization") {
                authorized = value.trim() == FIXTURE_AUTHORIZATION;
            }
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .filter(|length| *length <= MAX_REQUEST as usize)
                    .ok_or_else(|| io::Error::other("invalid access fixture body length"))?;
            }
        }
    }
    if phase == Phase::BacklogConcurrent && !is_get {
        return Ok((Route::Unexpected, authorized, None));
    }
    if let Some(patch_path) = patch_path {
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body)?;
        route = serde_json::from_slice::<Value>(&body)
            .ok()
            .map_or(Route::Unexpected, |body| {
                writes::patch_issue(phase, &patch_path, &body)
                    .unwrap_or_else(|| comment_dependency::patch(phase, &patch_path, &body))
            });
    } else if let Some(put_path) = put_path {
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body)?;
        route = serde_json::from_slice::<Value>(&body)
            .ok()
            .map_or(Route::Unexpected, |body| {
                merge::put(phase, &put_path, &body)
            });
    } else if let Some(post_path) = post_path {
        let mut body = vec![0; content_length];
        reader.read_exact(&mut body)?;
        let payload = serde_json::from_slice::<Value>(&body).ok();
        if route != Route::Graphql {
            route = payload.as_ref().map_or(Route::Unexpected, |body| {
                lifecycle::post(phase, &post_path, body)
                    .or_else(|| comment_dependency::post(phase, &post_path, body))
                    .or_else(|| review_threads::post(phase, &post_path, body))
                    .or_else(|| review_decisions::post(phase, &post_path, body))
                    .or_else(|| backlog::post(phase, &post_path, body))
                    .or_else(|| pr::post(phase, &post_path, body))
                    .unwrap_or_else(|| writes::post(phase, &post_path, body))
            });
        } else if let Some(operation) = payload.as_ref().and_then(|body| {
            review_threads::graphql(phase, body)
                .or_else(|| pr::shared_review_query(phase, body))
                .or_else(|| activity::graphql(phase, body))
                .or_else(|| merge::graphql(phase, body))
                .or_else(|| pr::activity_query(phase, body))
        }) {
            route = operation;
            if route == Route::QueuedThreadResolve {
                binding = payload
                    .as_ref()
                    .and_then(|body| body.pointer("/variables/input/clientMutationId"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
        } else if !payload.is_some_and(|body| {
            body.pointer("/variables/owner").and_then(Value::as_str) == Some("fixture-org")
                && body.pointer("/variables/name").and_then(Value::as_str) == Some("private")
                && body
                    .get("query")
                    .and_then(Value::as_str)
                    .is_some_and(|query| query.contains("RepositoryAccess"))
        }) {
            route = Route::Unexpected;
        }
    }
    if authorized && route == Route::Unexpected {
        route = dependency_failure.unwrap_or(Route::Unexpected);
    }
    Ok((route, authorized, binding))
}

fn sync_since(target: &str, prefix: &str) -> bool {
    let Some(value) = target
        .strip_prefix(prefix)
        .and_then(|s| s.strip_suffix("&per_page=20&page=1"))
    else {
        return false;
    };
    let bytes = value.as_bytes();
    // Match the fixed encoded UTC form without accepting additional query clauses.
    let structure = b"0000-00-00T00%3A00%3A00Z";
    bytes.len() == structure.len()
        && bytes.iter().zip(structure).all(|(value, pattern)| {
            if *pattern == b'0' {
                value.is_ascii_digit()
            } else {
                value == pattern
            }
        })
}

struct Response {
    status: u16,
    reason: &'static str,
    body: Value,
    sso: Option<String>,
}

fn response(phase: Phase, route: Route, authorized: bool, merge_sent: bool) -> Response {
    let result = |status, reason, body| Response {
        status,
        reason,
        body,
        sso: None,
    };
    if !authorized || phase == Phase::Revoked {
        return result(401, "Unauthorized", json!({"message":"Bad credentials"}));
    }
    if let Some(reply) = review_decisions::response(route) {
        return reply;
    }
    if let Some(response) = lifecycle::response(phase, route) {
        return response;
    }
    if let Some(response) = activity::response(phase, route) {
        return response;
    }
    if let Some(response) = comment_dependency::response(phase, route) {
        return response;
    }
    if let Some(response) = rate_limit::response(phase, route) {
        return response;
    }
    if matches!(route, Route::UpdatedInlineComments | Route::OpenPullIndex) {
        return result(200, "OK", json!([]));
    }
    if let Some(reply) = concurrent::response(phase, route) {
        return reply;
    }
    if let Some(backlog) = backlog::response(phase, route) {
        return backlog;
    }
    if let Some(reply) = historical_reader::response(phase, route) {
        return reply;
    }
    if let Some(history) = history::response(phase, route) {
        return history;
    }
    if let Some(merge) = merge::response(phase, route, merge_sent) {
        return merge;
    }
    if let Some(pr) = pr::response(phase, route) {
        return pr;
    }
    if let Some(write) = writes::response(phase, route) {
        return write;
    }
    let repository = json!({
        "id":71, "node_id":"R_71", "name":"private", "owner":{"login":"fixture-org"},
        "default_branch":"main","private":true,
        "permissions":{"pull":true,"push":matches!(phase, Phase::ReviewSend | Phase::ReviewChanged) || review_decisions::active(phase) || merge::applies(phase),"admin":false}
    });
    let item = || {
        json!({
            "id":410, "node_id":"I_410", "number":41, "title":"Reader pagination fixture",
            "state":"closed", "body":"A downloaded fixture issue.", "user":{"login":"fixture-user"},
            "created_at":"2018-01-01T00:00:00Z", "updated_at":"2018-01-01T00:00:00Z"
        })
    };
    let comment = |id, day, body| {
        json!({
            "id":id, "node_id":format!("IC_{id}"), "body":body, "user":{"login":"fixture-user"},
            "created_at":format!("2018-01-{day:02}T00:00:00Z"),
            "updated_at":format!("2018-01-{day:02}T00:00:00Z")
        })
    };
    match (phase, route) {
        (_, Route::Unexpected) => {
            result(404, "Not Found", json!({"message":"Unknown fixture route"}))
        }
        (Phase::SyncService, Route::User) => result(
            503,
            "Service Unavailable",
            json!({"message":"Fixture service unavailable"}),
        ),
        (Phase::SyncThrottle, Route::User) => result(
            429,
            "Too Many Requests",
            json!({"message":"Fixture throttled"}),
        ),
        (Phase::CredentialOther, Route::User) => {
            result(200, "OK", json!({"id":42,"login":"alternate-fixture-user"}))
        }
        (_, Route::User) => result(200, "OK", json!({"id":41,"login":"fixture-user"})),
        (Phase::Partial, Route::Discovery) => Response {
            sso: Some("partial-results; organizations=123".into()),
            ..result(200, "OK", json!([repository]))
        },
        (_, Route::Discovery) => result(200, "OK", json!([repository])),
        (Phase::Sso, Route::Repository) => Response {
            sso: Some(format!("required; url={AUTHORIZATION_URL}")),
            ..result(
                403,
                "Forbidden",
                json!({"message":"Organization SSO authorization required"}),
            )
        },
        (Phase::Denied, Route::Repository) => {
            result(403, "Forbidden", json!({"message":"Access denied"}))
        }
        (_, Route::Repository) => result(200, "OK", repository),
        (Phase::Denied | Phase::Sso, Route::Graphql) => result(
            200,
            "OK",
            json!({"data":{"repository":null},"errors":[{"type":"FORBIDDEN"}]}),
        ),
        (_, Route::Graphql) => result(
            200,
            "OK",
            json!({"data":{"repository":{
                "id":"R_71","name":"private","owner":{"login":"fixture-org"},"isPrivate":true,"viewerPermission":"READ"
            }}}),
        ),
        (phase, Route::UpdatedItems | Route::UpdatedComments)
            if merge::applies(phase)
                || review_decisions::active(phase)
                || review_threads::active(phase) =>
        {
            result(200, "OK", json!([]))
        }
        (Phase::ReaderFailure | Phase::ReaderResume, Route::Items) => {
            result(200, "OK", json!([item()]))
        }
        (Phase::ReaderFailure | Phase::ReaderResume, Route::Item) => result(200, "OK", item()),
        (Phase::ReaderFailure, Route::CommentsFirst) => result(
            200,
            "OK",
            json!([comment(5101, 2, "Downloaded first page.")]),
        ),
        (Phase::ReaderFailure, Route::CommentsNext) => result(
            502,
            "Bad Gateway",
            json!({"message":"Fixture page two failed"}),
        ),
        (Phase::ReaderResume, Route::CommentsNext) => result(
            200,
            "OK",
            json!([comment(5102, 3, "Recovered second page.")]),
        ),
        (Phase::ReaderFailure | Phase::ReaderResume, Route::Timeline) => {
            result(200, "OK", json!([]))
        }
        (Phase::SyncLive, Route::UpdatedItems) => {
            let mut current = item();
            current["repository_url"] = json!("https://api.github.com/repos/fixture-org/private");
            result(200, "OK", json!([current]))
        }
        (Phase::SyncLive, Route::UpdatedComments) => {
            let mut current = comment(5151, 4, "Observed ordinary conversation.");
            current["issue_url"] =
                json!("https://api.github.com/repos/fixture-org/private/issues/41");
            result(200, "OK", json!([current]))
        }
        _ => result(
            404,
            "Not Found",
            json!({"message":"Unexpected fixture phase"}),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immutable_compare_routes_to_the_active_native_fixture() {
        let temp = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(temp.path()).unwrap();
        let base = "a".repeat(40);
        let head = "b".repeat(40);
        let fetch = |phase, target| {
            fs::write(fixture.phase_file(), phase).unwrap();
            let mut socket = TcpStream::connect(fixture.address()).unwrap();
            write!(
                socket,
                "GET {target} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\n\r\n"
            )
            .unwrap();
            let mut reply = String::new();
            socket.read_to_string(&mut reply).unwrap();
            assert!(reply.starts_with("HTTP/1.1 200"));
            reply
        };
        let pair = format!("/repositories/71/compare/{base}...{head}?per_page=20&page=1");
        assert!(fetch("history", pair.as_str()).contains("pair.txt"));
        assert!(fetch("pr-load", pair.as_str()).contains("src/first.rs"));
        let branches = "/repositories/71/branches?per_page=20&page=1";
        assert!(fetch("history", branches).contains(&head));
        let moved = fetch("history-moved", branches);
        assert!(moved.contains(&"c".repeat(40)));
        assert!(!moved.contains(&head));
        assert!(fetch("history-deleted", branches).ends_with("\r\n[]"));
        let tags = "/repositories/71/tags?per_page=20&page=1";
        assert!(fetch("history-deleted", tags).contains("fixture-v1"));
        fixture.finish(temp.path(), TraceCheck::Observe).unwrap();
    }

    #[test]
    fn fixture_binds_loopback_and_exports_only_redacted_access_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(temp.path()).unwrap();
        assert!(fixture.address().ip().is_loopback());
        fs::write(fixture.phase_file(), "partial").unwrap();
        let mut socket = TcpStream::connect(fixture.address()).unwrap();
        write!(
            socket,
            "GET {DISCOVERY} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\n\r\n"
        )
        .unwrap();
        let mut reply = String::new();
        socket.read_to_string(&mut reply).unwrap();
        assert!(reply.contains("X-GitHub-SSO: partial-results; organizations=123"));
        fixture.finish(temp.path(), TraceCheck::Observe).unwrap();
        let trace = fs::read_to_string(temp.path().join("github-access-trace.json")).unwrap();
        let records: Value = serde_json::from_str(&trace).unwrap();
        assert_eq!(
            records[0],
            json!({"phase":"partial","route":"discovery","fixture_authorization":true,"status":200})
        );
        assert!(!trace.contains("threadline-test-fixture"));
    }

    #[test]
    fn reader_fixture_rejects_repeating_the_first_comment_page_on_resume() {
        let temp = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(temp.path()).unwrap();
        let fetch = |path: &str| {
            let mut socket = TcpStream::connect(fixture.address()).unwrap();
            write!(
                socket,
                "GET {path} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\n\r\n"
            )
            .unwrap();
            let mut reply = String::new();
            socket.read_to_string(&mut reply).unwrap();
            reply
        };
        fs::write(fixture.phase_file(), "reader-failure").unwrap();
        assert!(fetch(ITEMS).contains("Reader pagination fixture"));
        assert!(
            fetch(COMMENTS_FIRST).contains(&format!("http://{}{COMMENTS_NEXT}", fixture.address()))
        );
        assert!(fetch(COMMENTS_NEXT).starts_with("HTTP/1.1 502"));
        fs::write(fixture.phase_file(), "reader-resume").unwrap();
        assert!(fetch(COMMENTS_NEXT).contains("Recovered second page"));
        fixture
            .finish(temp.path(), TraceCheck::ReaderResume)
            .unwrap();

        let mut records: Vec<RequestEvidence> = [
            (Phase::ReaderFailure, Route::Items, 200),
            (Phase::ReaderFailure, Route::CommentsFirst, 200),
            (Phase::ReaderFailure, Route::CommentsNext, 502),
            (Phase::ReaderResume, Route::CommentsNext, 200),
        ]
        .into_iter()
        .map(|(phase, route, status)| RequestEvidence {
            phase,
            route,
            status,
            fixture_authorization: true,
        })
        .collect();
        assert!(complete_reader_resume(&records));
        records.push(RequestEvidence {
            phase: Phase::ReaderResume,
            route: Route::CommentsFirst,
            status: 404,
            fixture_authorization: true,
        });
        assert!(!complete_reader_resume(&records));
    }

    #[test]
    fn sync_journey_replays_concrete_http_failures_and_rejects_offline_traffic() {
        let dir = tempfile::tempdir().unwrap();
        let mut fixture = AccessFixture::start(dir.path()).unwrap();
        let since = "2026-11-18T20%3A11%3A12Z&per_page=20&page=1";
        let items = format!("{SYNC_ITEMS}{since}");
        let comments = format!("{SYNC_COMMENTS}{since}");
        assert!(!sync_since(&format!("{items}&unexpected=1"), SYNC_ITEMS));
        for (phase, route, expected) in [
            ("sync-live", items.as_str(), "200 OK"),
            ("sync-live", comments.as_str(), "200 OK"),
            ("sync-service", USER, "503 Service Unavailable"),
            ("sync-throttle", USER, "429 Too Many Requests"),
            ("sync-transport", USER, ""),
        ] {
            fs::write(fixture.phase_file(), phase).unwrap();
            let mut socket = TcpStream::connect(fixture.address()).unwrap();
            write!(
                socket,
                "GET {route} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\n\r\n"
            )
            .unwrap();
            let mut reply = String::new();
            socket.read_to_string(&mut reply).unwrap();
            if expected.is_empty() {
                assert!(reply.is_empty());
            } else {
                assert!(reply.starts_with(&format!("HTTP/1.1 {expected}")));
            }
            if phase == "sync-throttle" {
                assert!(reply.contains("Retry-After: 2\r\n"));
            }
        }
        let mut run = fixture.stop().unwrap();
        assert!(run.error.is_none());
        let records = &mut run.records;
        assert!(complete_sync_journey(records));
        records.push(RequestEvidence {
            phase: Phase::SyncOffline,
            route: Route::User,
            status: 200,
            fixture_authorization: true,
        });
        assert!(!complete_sync_journey(records));
    }
}
