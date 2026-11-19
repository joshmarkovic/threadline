# This script shares the exact registration, profile and binary guards in test-installed-activity.ps1.
function Assert-GroupedReviewEvents($State) {
    $events = @($State.events)
    if ($State.fixture -cne $marker -or !$State.background_and_outbox_paused -or $State.pull_number -ne 70 -or $events.Count -lt 2) {
        throw 'The generated PR fixture is incomplete.'
    }
    $review = $events[0]
    $inline = $events[1]
    if ($review.kind -cne 'review_submitted' -or $inline.kind -cne 'inline_comment_created' -or
        $review.key -cne ("github:pr-reviews:812:review_submitted:" + $review.occurred_at) -or
        $inline.key -cne ("github:pr-inline-comments:813:inline_comment_created:" + $review.occurred_at) -or
        $review.actor_id -ne 70021 -or $inline.actor_id -ne 70021 -or $review.automation -or $inline.automation -or
        (@($review.categories) -join ',') -cne 'CommentOnlyReview' -or
        (@($inline.categories) -join ',') -cne 'InlineComment') {
        throw 'The two observations did not identify the submitted human review and inline comment.'
    }
}
function Get-GroupedHistoryCount {
    return @($toastHistory.GetHistory($identity) | Where-Object {
        $_.Tag -ceq $group.Substring(0,16) -and $_.Group -ceq $group.Substring(16)
    }).Count
}
function Start-GroupedReviewProcess {
    $endpoint = 'http://127.0.0.1:' + $reviewProxy.LocalEndpoint.Port
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $exe
    $info.UseShellExecute = $false
    foreach ($name in @('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY')) { $info.EnvironmentVariables[$name] = $endpoint }
    $info.EnvironmentVariables['NO_PROXY'] = ''
    return [Diagnostics.Process]::Start($info)
}
function Test-InstalledGroupedReview {
    $before = Read-Fixture seed-review
    Assert-GroupedReviewEvents $before
    if ($before.event_count -ne 2 -or $before.group_count -ne 0 -or @($before.events | Where-Object published).Count) {
        throw 'The original observations must be new to the installed application.'
    }
    Add-Evidence 'Commit the two synthetic PR pages through the ordinary observation code with both network queues paused' $before
    $script:reviewProxy = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    $reviewProxy.Start()
    $script:toastHistory = [Windows.UI.Notifications.ToastNotificationManager,Windows.UI.Notifications,ContentType=WindowsRuntime]::History
    $script:child = Start-GroupedReviewProcess
    $report.initial_pid = $child.Id
    $first = Wait-Activity 'the single real native review group' {
        if ($child.HasExited) { throw 'The installed process exited before delivery.' }
        $state = Read-Fixture inspect-review
        if ($state.group_count -eq 0 -and @($state.events | Where-Object published).Count -eq 2) {
            if (@($state.events | Where-Object os_disabled).Count -eq 2) { Stop-Blocked 'Windows disabled the registered sender.' }
            throw 'The registered application routed both events to the feed without native delivery.'
        }
        if ($state.outcome -ceq 'delivered') { return $state }
    } 65000
    Assert-GroupedReviewEvents $first
    if ($first.event_count -ne 2 -or $first.group_count -ne 1 -or $first.native_id -cnotmatch '^[a-f0-9]{32}$' -or
        @($first.events | Where-Object { $_.mode -ceq 'Popup' -and $_.published -and $_.grouped }).Count -ne 2) {
        throw 'The review and inline event did not receive one committed popup.'
    }
    $script:group = $first.native_id
    $null = Wait-Activity 'the registered two-event toast with its exact payloads' { Get-ExactHistory }
    Close-Installed $child
    $child.Dispose(); $script:child = $null
    if (@(Get-InstalledProcesses).Count -ne 0 -or !(Get-ExactHistory)) {
        throw 'The exact toast did not survive a full process exit.'
    }
    Add-Evidence 'Read one two-event card from app-scoped Windows history after the ordinary process exits' $first
    $toastHistory.Remove($group.Substring(0,16),$group.Substring(16),$identity)
    $null = Wait-Activity 'retirement of only the verified fixture card' { (Get-GroupedHistoryCount) -eq 0 }
    $pending = Read-Fixture replay-review
    Assert-GroupedReviewEvents $pending
    if ($pending.event_count -ne 3 -or $pending.events[2].kind -cne 'check_completed' -or $pending.events[2].published) {
        throw 'Replay did not preserve the two source keys and create one pending feed-only check.'
    }
    $script:child = Start-GroupedReviewProcess
    $report.restarted_pid = $child.Id
    if ($child.Id -eq $report.initial_pid) { throw 'A separate process restart was not observed.' }
    $after = Wait-Activity 'the new process publishing its independent feed-only check' {
        if ($child.HasExited) { throw 'The restarted app exited before the new check reached the feed.' }
        $state = Read-Fixture inspect-review
        if ($state.event_count -eq 3 -and $state.events[2].published) { return $state }
    } 65000
    Close-Installed $child
    $after = Read-Fixture inspect-review
    Assert-GroupedReviewEvents $after
    if ($after.event_count -ne 3 -or $after.group_count -ne 1 -or $after.native_id -cne $group -or
        $after.changed_at -ne $first.changed_at -or $after.outcome -cne 'delivered' -or
        @($after.events | Where-Object { $_.mode -ceq 'Popup' -and $_.published -and $_.grouped }).Count -ne 2 -or
        $after.events[2].mode -cne 'FeedOnly' -or !$after.events[2].published -or $after.events[2].grouped -or (Get-GroupedHistoryCount) -ne 0) {
        throw 'Restart changed the original delivery or returned the retired card to Windows history.'
    }
    $report.loopback_proxy_received_connection = $reviewProxy.Pending()
    if ($report.loopback_proxy_received_connection) { throw 'The installed fixture attempted a network request.' }
    Add-Evidence 'Reopen, publish the independent feed-only barrier and retain both original deliveries without a replacement card' $after
}
