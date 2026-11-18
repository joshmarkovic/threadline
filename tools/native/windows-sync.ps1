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

function Open-NotificationSchedule([IntPtr]$Window) {
    Wait-For 'the process-owned Activity action' {
        Invoke-OwnedFixtureControl $Window 'reader-activity' $null 'Activity'
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

function Invoke-NotificationScheduleProof([IntPtr]$Window, [string]$Profile) {
    Wait-SeededFixtureAccount
    $null = Wait-Sync 'the normal authenticated issue and comment polling before pauses' {
        param($sync)
        $sync.connection -ceq 'Reachable' -and $sync.cache_generation -ge 2 -and !$sync.controls.work_offline -and
            !$sync.controls.pause_background -and !$sync.controls.pause_queued_actions -and !$sync.controls.notification_quiet
    }
    Open-NotificationSchedule $Window
    Wait-For 'the native Enable quiet hours control' {
        Invoke-OwnedFixtureControl $Window 'notifications-quiet' $null 'Enable quiet hours'
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
    Wait-For 'the native feed and popup default' {
        Invoke-OwnedFixtureControl $Window 'notifications-default-2' $null 'Feed and popup'
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
    $restarted = Restart-Reader $Profile -SeedQuietEvent
    Wait-For 'the exact seeded event stays in native Activity because quiet hours suppress popup and sound' {
        $observed = Read-Observation
        if ($null -ne $observed -and $null -ne $observed.cached_reader -and
            $observed.cached_reader.notification.isolated_quiet_event_feed_only) { return $true }
        $null = Invoke-OwnedFixtureControl $restarted 'reader-activity' $null 'Activity'
        return $false
    } 8000 300
    Add-Evidence 'Observe the isolated event in the native feed with the exact Quiet Hours decision' ([ordered]@{
        delivery = 'FeedOnly'; popup_suppressed_by = 'QuietHours'; sound = $false
    })
    Open-NotificationSchedule $restarted
    $restored = Wait-FixtureQuietSchedule $startMinute $endMinute
    $controls = $restored.cached_reader.shared_sync.controls
    if (!$controls.work_offline -or !$controls.pause_background -or !$controls.pause_queued_actions -or $controls.notification_quiet) {
        throw 'The independent quiet-hours and synchronization preferences did not survive a separate process.'
    }
    Add-Evidence 'Restore the quiet schedule and each independent pause after restart' $controls

    Open-SyncSettings $restarted
    Set-AccessPhase 'sync-live'
    Click-AccountControl $restarted 'sync-work-offline'
    $null = Wait-Sync 'network resume retains both distinct pauses' {
        param($sync) !$sync.controls.work_offline -and $sync.controls.pause_background -and $sync.controls.pause_queued_actions
    }
    Wait-SeededFixtureAccount
    $null = Wait-Reader 'the eligible queued fixture is retained while the queue is specifically paused' {
        param($reader) !$reader.offline -and $reader.outbox -eq 1
    }
    Set-AccessPhase 'sync-service'
    Click-AccountControl $restarted 'sync-check-now'
    $null = Wait-Sync 'explicit provider check still executes with both default pauses and quiet hours' {
        param($sync) $sync.connection -ceq 'Reachable' -and $sync.reason -ceq 'ServiceFailure' -and
            $sync.controls.pause_background -and $sync.controls.pause_queued_actions -and !$sync.controls.work_offline
    }
    Open-NotificationSchedule $restarted
    $null = Wait-FixtureQuietSchedule $startMinute $endMinute
    Wait-For 'restore the native feed-only delivery default' {
        Invoke-OwnedFixtureControl $restarted 'notifications-default-1' $null 'Feed only'
    }
    $null = Wait-Reader 'future fixture events use the safe feed-only default' {
        param($reader) $reader.notification.saved -and !$reader.notification.popup_default
    }
    Wait-For 'the exact account quiet-hours toggle' {
        Invoke-OwnedFixtureControl $restarted 'notifications-quiet' $null 'Enable quiet hours'
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
    Inspect-WriteFixtureAction $restarted 0 'Confirmed'
    $null = Wait-Reader 'the original eligible fixture action left the pending count after acknowledgement' {
        param($reader) $reader.outbox -eq 0 -and !$reader.offline
    }
    Add-Evidence 'Resume one queued action independently of paused background polling' 'Confirmed'
    Stop-ThroughUi
    $script:report.detail = 'Windows quiet hours kept the seeded native activity in the feed while independent native synchronization controls held network requests and the queued action. Explicit checking and queue resumption were each verified against saved preferences and loopback requests.'
}
