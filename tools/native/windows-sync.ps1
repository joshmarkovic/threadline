function Wait-Sync([string]$Description, [scriptblock]$SyncCheck) {
    return Wait-Reader $Description {
        param($reader)
        $property = $reader.PSObject.Properties['shared_sync']
        $null -ne $property -and $null -ne $property.Value -and (& $SyncCheck $property.Value)
    }
}

function Open-SyncSettings([IntPtr]$Window) {
    Click-AccountControl $Window 'reader-sync-settings'
    $null = Wait-Sync 'the native synchronization settings with persisted values' {
        param($sync) $sync.panel_open -and $null -ne $sync.controls -and $null -ne $sync.schedule
    }
}

function Invoke-SyncProof([IntPtr]$Window, [string]$Profile) {
    Wait-SeededFixtureAccount
    $null = Wait-Sync 'actual GitHub availability and both scheduled resource families' {
        param($sync) $sync.connection -ceq 'Reachable' -and $sync.cache_generation -ge 2
    }
    Add-Evidence 'Observe authenticated issue and comment updates in the native reader' 'Reachable'

    Open-SyncSettings $Window
    $null = Wait-AccountControl 'sync-timezone'
    Wait-For 'the owned timezone field' {
        Set-OwnedFixtureEditValue $Window 'sync-timezone' 'IANA time zone override' 'UTC'
    }
    Click-AccountControl $Window 'sync-save-schedule'
    $null = Wait-Sync 'the saved timezone override' { param($sync) $sync.schedule.timezone -ceq 'UTC' }
    Click-AccountControl $Window 'sync-pause-background'
    $null = Wait-Sync 'background refresh paused independently' { param($sync) $sync.controls.pause_background -and !$sync.controls.work_offline }
    Click-AccountControl $Window 'sync-pause-outbox'
    $null = Wait-Sync 'queued actions paused independently' { param($sync) $sync.controls.pause_queued_actions -and $sync.controls.pause_background }
    Click-AccountControl $Window 'sync-quiet-notifications'
    $null = Wait-Sync 'quiet notifications persisted' { param($sync) $sync.controls.notification_quiet }
    Click-AccountControl $Window 'sync-work-offline'
    $null = Wait-Sync 'offline controls and unavailable network mode' {
        param($sync) $sync.connection -ceq 'Offline' -and $sync.controls.work_offline
    }

    Set-AccessPhase 'sync-offline'
    $restarted = Restart-Reader $Profile
    Open-SyncSettings $restarted
    $restored = Wait-Sync 'offline mode, independent pauses, quiet notifications and timezone restored after Quit' {
        param($sync)
        $sync.connection -ceq 'Offline' -and $sync.controls.work_offline -and
            $sync.controls.pause_background -and $sync.controls.pause_queued_actions -and
            $sync.controls.notification_quiet -and $sync.schedule.timezone -ceq 'UTC'
    }
    Add-Evidence 'Restore the native synchronization controls in a new process' $restored.cached_reader.shared_sync.controls

    Set-AccessPhase 'sync-live'
    Click-AccountControl $restarted 'sync-work-offline'
    $null = Wait-Sync 'online mode while both pauses remain set' {
        param($sync) !$sync.controls.work_offline -and $sync.controls.pause_background -and $sync.controls.pause_queued_actions
    }
    Wait-SeededFixtureAccount

    Set-AccessPhase 'sync-service'
    Click-AccountControl $restarted 'sync-check-now'
    $null = Wait-Sync 'a reachable HTTP service failure distinct from Offline' {
        param($sync) $sync.connection -ceq 'Reachable' -and $sync.reason -ceq 'ServiceFailure'
    }
    Add-Evidence 'Check now reaches GitHub and shows a service failure' 'ServiceFailure'

    Set-AccessPhase 'sync-throttle'
    Click-AccountControl $restarted 'sync-check-now'
    $null = Wait-Sync 'a reachable API response applying the rate floor' {
        param($sync) $sync.connection -ceq 'Reachable' -and $sync.reason -ceq 'RateLimited'
    }
    Add-Evidence 'Display rate limiting separately from connectivity' 'RateLimited'

    Set-AccessPhase 'sync-transport'
    Click-AccountControl $restarted 'sync-check-now'
    $null = Wait-Sync 'current inability to reach GitHub with Work Offline disabled' {
        param($sync) $sync.connection -ceq 'Unreachable' -and !$sync.controls.work_offline
    }
    Add-Evidence 'Display current transport failure separately from Work Offline' 'Unreachable'

    Stop-ThroughUi
    $script:report.detail = 'Native synchronization settings survived restart and distinguished HTTP, throttling and transport outcomes.'
}

function Invoke-RateLimitedCacheProof([IntPtr]$Window) {
    Wait-SeededFixtureAccount
    $item = Open-WriteFixtureIssue $Window -UserConnected
    $null = Wait-Reader 'the downloaded issue with discussion still unfetched' {
        param($reader)
        $reader.active_item -eq $item -and $reader.cached_content_ready -and
            $reader.comments_coverage -ceq 'Missing' -and $reader.outbox -eq 0
    }
    Open-SyncSettings $Window
    $null = Wait-Sync 'online with neither background nor Outbox paused by the operator' {
        param($sync)
        $sync.connection -ceq 'Reachable' -and !$sync.controls.work_offline -and
            !$sync.controls.pause_background -and !$sync.controls.pause_queued_actions
    }

    $clock = [Diagnostics.Stopwatch]::StartNew()
    Set-AccessPhase 'sync-rate-throttle'
    Click-AccountControl $Window 'sync-check-now'
    $null = Wait-Sync 'the reachable service reporting the fixture rate limit' {
        param($sync) $sync.connection -ceq 'Reachable' -and $sync.reason -ceq 'RateLimited' -and !$sync.controls.work_offline
    }
    Set-AccessPhase 'sync-rate-floor'
    Click-AccountControl $Window 'sync-check-now'
    Click-AccountControl $Window 'reader-browse'
    Click-AccountControl $Window 'reader-close-tab'
    $null = Wait-Reader 'the original issue tab is closed before searching the local cache' {
        param($reader) $null -eq $reader.active_item -and $reader.tab_counts[0] -eq 0
    }
    $query = 'Existing issue for offline edits'
    Wait-For 'the owned search field for already downloaded content' {
        Set-OwnedFixtureEditValue $Window 'reader-search' 'Search downloaded content' $query
    }
    Wait-For 'confirmation of the local query in the owned native field' {
        $pattern = Get-OwnedFixtureEditPattern $Window 'reader-search' 'Search downloaded content'
        try { $null -ne $pattern -and $pattern.Current.Value -ceq $query }
        catch [Windows.Automation.ElementNotAvailableException] { $false }
    }
    Click-AccountControl $Window 'reader-search-submit'
    $null = Wait-Reader 'one local search result for the same downloaded issue' {
        param($reader)
        $reader.search_generation -gt 0 -and $reader.search_results -eq 1 -and $reader.first_search_item -eq $item -and !$reader.offline
    }
    Click-AccountControl $Window 'reader-result-0'
    $null = Wait-Reader 'reopen the same cached issue without network or offline mode' {
        param($reader) $reader.active_item -eq $item -and $reader.cached_content_ready -and !$reader.offline
    }
    Click-AccountControl $Window 'reader-maximize-pane'
    Click-AccountControl $Window 'reader-fetch-comments-0'
    $null = Wait-Reader 'the explicit fetch is held while existing content stays available' {
        param($reader)
        $reader.comments_coverage -ceq 'Missing' -and $reader.comment_fetch_state -ceq 'Failed' -and
            $reader.active_item -eq $item -and $reader.cached_content_ready -and !$reader.offline
    }
    Click-AccountControl $Window 'reader-draft-comment-0'
    $body = 'A local draft saved during the rate limit.'
    Type-WriteFixtureField $Window 'reader-draft-input-0' $body
    Wait-WriteFixtureDraft $item $body
    $saved = Wait-Reader 'the exact draft is saved locally and absent from the Outbox' {
        param($reader) $reader.active_item -eq $item -and $reader.active_draft_saved -and !$reader.active_draft_submitted -and $reader.outbox -eq 0
    }
    Stop-ThroughUi
    # The fixture sends Retry-After 180. Start the clock before the first Check now.
    if ($clock.Elapsed.TotalSeconds -ge 180) { throw 'The native controls completed outside the server retry window.' }
    Add-Evidence 'Search and reopen cached content and save the unsubmitted draft while provider reads are held' ([ordered]@{
        item = $item; fixture_http_status = 429; retry_after_seconds = 180; seconds_since_throttle_phase_begin = $clock.Elapsed.TotalSeconds
        search_results = 1; second_probe_requested = $true; discussion_fetch = 'Failed'; work_offline = $false
        draft_sha256 = $saved.cached_reader.active_draft_sha256; outbox = 0
    })
    $script:report.detail = 'After a real HTTP 429 the native reader searched and reopened cached content, held an explicit provider fetch, and saved a local draft before the retry window elapsed.'
}

function Open-NotificationSchedule([IntPtr]$Window, [switch]$FromActivity) {
    if (!$FromActivity) {
        Wait-For 'the process-owned Activity action' {
            Invoke-OwnedFixtureControl $Window 'reader-activity' $null 'Activity'
        }
    }
    Wait-For 'the process-owned Notification settings action' {
        Invoke-OwnedFixtureControl $Window 'activity-toggle-settings' $null 'Notification settings'
    }
    $null = Wait-Reader 'the loaded account notification preferences' {
        param($reader) $reader.notification.preferences -and $reader.notification.saved
    }
}

function Wait-FixtureQuietSchedule([int]$StartMinute, [int]$EndMinute) {
    return Wait-Reader 'the exact independent quiet-hours schedule' {
        param($reader)
        $schedule = $reader.notification.quiet_hours
        $reader.notification.saved -and $null -ne $schedule -and $schedule.weekdays -eq 127 -and
            $schedule.start_minute -eq $StartMinute -and $schedule.end_minute -eq $EndMinute -and $schedule.timezone -ceq 'UTC'
    }
}

function Test-NotificationSyncReady($Snapshot) {
    $controls = $Snapshot.controls
    return $Snapshot.connection -ceq 'Reachable' -and $Snapshot.cache_generation -ge 2 -and
        $null -ne $controls -and !$controls.work_offline -and !$controls.pause_background -and
        !$controls.pause_queued_actions -and !$controls.notification_quiet
}

function Get-NotificationChoiceName([string]$Label, [bool]$Selected) {
    $state = if ($Selected) { 'Selected' } else { 'Unselected' }
    return $Label + ' ' + [char]0x00b7 + ' ' + $state
}

function Add-NotificationCheckpoint([string]$Step) {
    $receipt = [ordered]@{ step = $Step; at_unix_ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() }
    $line = ($receipt | ConvertTo-Json -Compress) + [Environment]::NewLine
    [IO.File]::AppendAllText((Join-Path $script:ReportDir 'notification-schedule-checkpoints.jsonl'), $line)
}

function Open-RestartedFixtureActivity([IntPtr]$Window) {
    if (!(Test-OwnedFixtureWindow $Window)) { Stop-Blocked 'The restarted activity window is no longer owned and visible.' }
    Add-NotificationCheckpoint 'request_restarted_activity'
    $probe = Join-Path $PSScriptRoot 'windows-open-fixture-activity.ps1'
    $started = $script:app.StartTime.ToUniversalTime().Ticks
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = Join-Path $PSHOME 'powershell.exe'
    $info.Arguments = '-NoProfile -NonInteractive -Sta -File "' + $probe + '" -OwnerId ' + $script:app.Id +
        ' -OwnerStartTicks ' + $started + ' -WindowHandle ' + $Window.ToInt64()
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $child = [Diagnostics.Process]::Start($info)
    try {
        if (!$child.WaitForExit(12000)) {
            $child.Kill()
            [void]$child.WaitForExit(2000)
            Add-NotificationCheckpoint 'activity_accessibility_timed_out'
            Stop-Blocked 'The restarted process did not return from the owned Activity accessibility action within 12 seconds.'
        }
        if ($child.ExitCode -ne 0) {
            Add-NotificationCheckpoint 'activity_accessibility_unavailable'
            Stop-Blocked 'The restarted process did not expose the exact owned Activity accessibility action.'
        }
    } finally {
        $child.Dispose()
    }
    Add-NotificationCheckpoint 'restarted_activity_invoked'
}

function Invoke-NotificationScheduleProof([IntPtr]$Window, [string]$Profile) {
    Wait-SeededFixtureAccount
    Open-SyncSettings $Window
    $null = Wait-Sync 'the normal authenticated issue and comment polling before pauses' {
        param($sync) Test-NotificationSyncReady $sync
    }
    Open-NotificationSchedule $Window
    Wait-For 'the native Enable quiet hours control' {
        Invoke-OwnedFixtureControl $Window 'notifications-quiet' $null (Get-NotificationChoiceName 'Enable quiet hours' $false)
    }
    # Choose a bounded UTC window containing this entire isolated test.
    $now = [DateTime]::UtcNow
    $minutes = $now.Hour * 60 + $now.Minute
    $startMinute = ($minutes + 1320) % 1440
    $endMinute = ($minutes + 180) % 1440
    $start = [DateTime]::Today.AddMinutes($startMinute).ToString('HH:mm', [Globalization.CultureInfo]::InvariantCulture)
    $end = [DateTime]::Today.AddMinutes($endMinute).ToString('HH:mm', [Globalization.CultureInfo]::InvariantCulture)
    Wait-For 'the quiet-hours start input' {
        Set-OwnedFixtureEditValue $Window 'notifications-quiet-start' 'Quiet hours start, HH:MM' $start
    }
    Wait-For 'the quiet-hours end input' {
        Set-OwnedFixtureEditValue $Window 'notifications-quiet-end' 'Quiet hours end, HH:MM' $end
    }
    Wait-For 'the quiet-hours time zone input' {
        Set-OwnedFixtureEditValue $Window 'notifications-quiet-zone' 'IANA time zone, blank for local' 'UTC'
    }
    Wait-For 'the native Save quiet hours control' {
        Invoke-OwnedFixtureControl $Window 'notifications-save-quiet' $null 'Save quiet hours'
    }
    $saved = Wait-FixtureQuietSchedule $startMinute $endMinute
    $current = $saved.cached_reader.shared_sync.controls
    if ($current.work_offline -or $current.pause_background -or $current.pause_queued_actions -or $current.notification_quiet) {
        throw 'Saving quiet hours changed an independent synchronization control.'
    }
    Add-Evidence 'Save independent notification quiet hours using native fields' $saved.cached_reader.notification.quiet_hours
    Add-NotificationCheckpoint 'quiet_hours_saved'
    Wait-For 'the native feed and popup default' {
        Invoke-OwnedFixtureControl $Window 'notifications-default-2' $null (Get-NotificationChoiceName 'Feed and popup' $false)
    }
    $null = Wait-Reader 'popup normally allowed while the current quiet schedule is active' {
        param($reader) $reader.notification.saved -and $reader.notification.popup_default
    }

    Open-SyncSettings $Window
    Click-AccountControl $Window 'sync-pause-background'
    $null = Wait-Sync 'reading paused while queued actions and Work Offline stay enabled for network' {
        param($sync) $sync.controls.pause_background -and !$sync.controls.pause_queued_actions -and !$sync.controls.work_offline
    }
    Click-AccountControl $Window 'sync-pause-outbox'
    $null = Wait-Sync 'queued actions separately paused' {
        param($sync) $sync.controls.pause_queued_actions -and $sync.controls.pause_background -and !$sync.controls.work_offline
    }
    Click-AccountControl $Window 'sync-work-offline'
    $null = Wait-Sync 'Work Offline enabled after the two independent pauses' {
        param($sync) $sync.controls.work_offline -and $sync.connection -ceq 'Offline'
    }
    Set-AccessPhase 'sync-offline'
    Add-NotificationCheckpoint 'offline_and_paused_before_quit'
    $restarted = Restart-Reader $Profile -SeedQuietEvent
    Add-NotificationCheckpoint 'offline_restart_and_fixture_verified'
    $null = Wait-Reader 'the restarted profile has its seeded queued item' {
        param($reader) $reader.offline -and $reader.outbox -eq 1
    }
    Open-SyncSettings $restarted
    $restored = Wait-Sync 'restore all three independent synchronization holds after restart' {
        param($sync)
        $sync.controls.work_offline -and $sync.controls.pause_background -and
            $sync.controls.pause_queued_actions -and !$sync.controls.notification_quiet
    }
    Add-Evidence 'Restore Work Offline, background read pause and Outbox pause after restart' $restored.cached_reader.shared_sync.controls

    # Work Offline defers new activity decisions as well as provider requests.
    Set-AccessPhase 'sync-live'
    Click-AccountControl $restarted 'sync-work-offline'
    $null = Wait-Sync 'network resume retains both distinct pauses' {
        param($sync) !$sync.controls.work_offline -and $sync.controls.pause_background -and $sync.controls.pause_queued_actions
    }
    Wait-SeededFixtureAccount
    $null = Wait-Reader 'the eligible queued fixture is retained while the queue is specifically paused' {
        param($reader) !$reader.offline -and $reader.outbox -eq 1
    }
    Add-NotificationCheckpoint 'online_with_separate_pauses'

    Open-RestartedFixtureActivity $restarted
    $null = Wait-Reader 'the exact seeded event is in the real Activity feed because quiet hours suppress popup and sound' {
        param($reader) $reader.notification.isolated_quiet_event_feed_only
    }
    Add-NotificationCheckpoint 'quiet_hours_feed_only_verified'
    Add-Evidence 'Observe the isolated event in the native feed with the exact Quiet Hours decision' ([ordered]@{
        delivery = 'FeedOnly'; popup_suppressed_by = 'QuietHours'; sound = $false
    })
    Open-NotificationSchedule $restarted -FromActivity
    $restoredQuiet = Wait-FixtureQuietSchedule $startMinute $endMinute
    Add-Evidence 'Restore the same quiet hours after restart and reconnection' $restoredQuiet.cached_reader.notification.quiet_hours
    Add-NotificationCheckpoint 'settings_restored'

    Open-SyncSettings $restarted
    Set-AccessPhase 'sync-service'
    Click-AccountControl $restarted 'sync-check-now'
    $null = Wait-Sync 'explicit provider check still executes with both default pauses and quiet hours' {
        param($sync) $sync.connection -ceq 'Reachable' -and $sync.reason -ceq 'ServiceFailure' -and
            $sync.controls.pause_background -and $sync.controls.pause_queued_actions -and !$sync.controls.work_offline
    }
    Open-RestartedFixtureActivity $restarted
    Open-NotificationSchedule $restarted -FromActivity
    $null = Wait-FixtureQuietSchedule $startMinute $endMinute
    Wait-For 'restore the native feed-only delivery default' {
        Invoke-OwnedFixtureControl $restarted 'notifications-default-1' $null (Get-NotificationChoiceName 'Feed only' $false)
    }
    $null = Wait-Reader 'future fixture events use the safe feed-only default' {
        param($reader) $reader.notification.saved -and !$reader.notification.popup_default
    }
    Wait-For 'the exact account quiet-hours toggle' {
        Invoke-OwnedFixtureControl $restarted 'notifications-quiet' $null (Get-NotificationChoiceName 'Enable quiet hours' $true)
    }
    $null = Wait-Reader 'removing quiet hours keeps both other pauses and online mode' {
        param($reader)
        $sync = $reader.shared_sync.controls
        $reader.notification.saved -and $null -eq $reader.notification.quiet_hours -and
            $sync.pause_background -and $sync.pause_queued_actions -and !$sync.work_offline
    }
    Set-AccessPhase 'sync-notification-send'
    Open-SyncSettings $restarted
    Click-AccountControl $restarted 'sync-pause-outbox'
    $null = Wait-Sync 'only queued actions resumed while background reads stay paused' {
        param($sync) !$sync.controls.work_offline -and !$sync.controls.pause_queued_actions -and $sync.controls.pause_background
    }
    Click-AccountControl $restarted 'reader-outbox'
    Click-AccountControl $restarted 'reader-outbox-filter-history'
    Inspect-WriteFixtureAction $restarted 0 'Confirmed' 'Comment'
    $null = Wait-Reader 'the original eligible fixture action left the pending count after acknowledgement' {
        param($reader) $reader.outbox -eq 0 -and !$reader.offline
    }
    Add-Evidence 'Resume one queued action independently of paused background polling' 'Confirmed'
    Add-NotificationCheckpoint 'queued_action_confirmed'
    Stop-ThroughUi
    $script:report.detail = 'Windows quiet hours kept the seeded native activity in the feed while independent native synchronization controls held network requests and the queued action. Explicit checking and queue resumption were each verified against saved preferences and loopback requests.'
}
