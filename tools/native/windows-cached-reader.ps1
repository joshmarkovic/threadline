function Wait-Reader([string]$Description, [scriptblock]$ReaderCheck, [int]$PollInterval = 45, [int]$Timeout = 8000) {
    return Wait-State $Description {
        param($state)
        $cached = $state.PSObject.Properties['cached_reader']
        $null -ne $cached -and $null -ne $cached.Value -and (& $ReaderCheck $cached.Value)
    } $PollInterval $Timeout
}

function Initialize-ReaderProfile([string]$Profile, [string]$Scale) {
    $path = Join-Path $script:ReportDir 'reader-fixture.json'
    $seeder = Start-Threadline $Profile $path -SeedReader $Scale
    try {
        if (!$seeder.WaitForExit(180000)) {
            $seeder.Kill()
            [void]$seeder.WaitForExit(3000)
            throw 'The isolated reader fixture did not finish seeding.'
        }
        if ($seeder.ExitCode -ne 0) { throw 'The app could not seed its isolated reader profile.' }
    } finally {
        $seeder.Dispose()
    }
    $receipt = [IO.File]::ReadAllText($path) | ConvertFrom-Json
    $revision = switch ($Scale) {
        'section-16-backlog' { 's12-large-workspace-v2' }
        'small-pane-positions' { 's12-pane-positions-v1' }
        'small-comment-links' { 's12-cached-comment-links-v1' }
        default { 's04-reader-v1' }
    }
    if ($receipt.fixture_revision -cne $revision -or $receipt.scale -cne $Scale -or
        $receipt.seed -ne $script:Seed -or !$receipt.offline) {
        throw 'The reader fixture does not match the requested isolated workload.'
    }
    if ($Scale -in @('section-16', 'section-16-backlog') -and ($receipt.items -ne 50000 -or $receipt.comments -ne 250000 -or
        $receipt.open_tabs -ne 20 -or $receipt.outbox_count -ne 1000)) {
        throw 'The reader fixture does not match the Section 16 workload.'
    }
    if ($Scale -eq 'section-16-backlog') {
        if ($receipt.typed_outbox_count -ne 12 -or $receipt.unresolved_outbox -ne 901) {
            throw 'The large workspace needs its exact typed actions and retained historical queue.'
        }
        $script:BacklogSeed = $receipt
    }
    if ($Scale -eq 'small-pane-positions') {
        $script:PanePositionsSeed = $receipt
        if ($receipt.pane_positions.archive -eq $receipt.repository_id -or $receipt.pane_positions.comments -ne 28 -or $receipt.open_tabs -ne 20) {
            throw 'The isolated pane-position fixture must retain two repositories and both tab strips.'
        }
    }
    if ($Scale -eq 'small-comment-links') {
        $script:CachedCommentLinksSeed = $receipt
        if ($receipt.open_tabs -ne 3 -or $receipt.comment_links.query -cne 'QuasarLedger' -or
            $receipt.comment_links.issue.item -eq $receipt.comment_links.pull_request.item) {
            throw 'The cached comment fixture must have three tabs and independent issue and PR targets.'
        }
    }
    Add-Evidence 'Seed the isolated profile with the application and bundled SQLite' $receipt
}

function Restart-Reader([string]$Profile, [hashtable]$Startup, [switch]$SeedQuietEvent, [switch]$Backlog, [string]$FixturePhaseAfterQuit, [ValidateSet('arm','release')][string]$RecoveryStage) {
    if ($RecoveryStage -and ($ReaderProof -cne 'CursorRecovery' -or $FixturePhaseAfterQuit -cne 'recover-offline')) {
        throw 'The recovery fault may change only while the isolated reader is stopped and offline.'
    }
    Stop-ThroughUi
    if ($FixturePhaseAfterQuit) { Set-AccessPhase $FixturePhaseAfterQuit }
    $script:app.Dispose()
    $script:app = $null
    $before = Get-IsolatedSqlite $Profile
    if ($RecoveryStage) {
        $null = Read-RecoveryFixture $Profile $RecoveryStage "reader-recovery-$RecoveryStage.json"
    }
    if ($SeedQuietEvent) {
        if ($ReaderProof -cne 'NotificationSchedule') { throw 'Only the isolated notification schedule can seed this event.' }
        $receipt = Join-Path $script:ReportDir 'notification-schedule-fixture.json'
        $seeder = Start-Threadline $Profile $receipt -SeedQuietEvent
        try {
            if (!$seeder.WaitForExit(10000)) {
                $seeder.Kill()
                [void]$seeder.WaitForExit(3000)
                throw 'The notification schedule fixture did not finish.'
            }
            if ($seeder.ExitCode -ne 0) { throw 'The app rejected the isolated quiet-event profile.' }
            $seeded = [IO.File]::ReadAllText($receipt) | ConvertFrom-Json
            if ($seeded.fixture_revision -cne 's11-native-notification-schedule-v1' -or
                $seeded.queued -ne 1 -or $seeded.events -ne 1) {
                throw 'The isolated schedule fixture did not report one event and one queued action.'
            }
        } finally {
            $seeder.Dispose()
        }
    }
    $observationName = if ($RecoveryStage) {
        "reader-recovery-$RecoveryStage-observation.json"
    } elseif ($Backlog) {
        'reader-backlog-observation.json'
    } elseif ($FixturePhaseAfterQuit) {
        'reader-dependent-restart-observation.json'
    } else {
        'reader-restart-observation.json'
    }
    $script:reportFile = Join-Path $script:ReportDir $observationName
    if ($Startup) {
        $launchBegin = [Diagnostics.Stopwatch]::GetTimestamp()
        $startupClock = [Diagnostics.Stopwatch]::StartNew()
    }
    $script:app = Start-Threadline $Profile $script:reportFile -ReaderIdle:([bool]$Startup) -ReaderBacklog:$Backlog
    if ($Startup) { $launchReturned = [Diagnostics.Stopwatch]::GetTimestamp() }
    $script:report.restarted_pid = $script:app.Id
    $window = Get-ReadyWindow
    if ($Startup) {
        $ready = Wait-LargeWorkspaceReady $window
        $Startup.warm_start_ms = $startupClock.Elapsed.TotalMilliseconds
        $Startup.warm_start_timing = Get-LargeStartupTiming $ready $launchBegin $launchReturned ([Diagnostics.Stopwatch]::GetTimestamp())
    }
    if (!$script:UseOwnedUiAutomation) { Set-WindowFocus $window }
    $after = Get-IsolatedSqlite $Profile -Live
    if ($before.creation_time_ticks -ne $after.creation_time_ticks) {
        throw 'The isolated reader database changed identity after Quit and relaunch.'
    }
    Add-Evidence 'Relaunch the same SQLite profile in a separate process' ([ordered]@{
        pid = $script:report.restarted_pid
        sqlite_retained = $true
    })
    return $window
}

function Test-OfflineDraftFocusReceipt($Snapshot, [int]$Owner, [string]$Id, [long]$Item, [int]$Pane, [switch]$NativeFocus) {
    if ($null -eq $Snapshot) { return $false }
    $ownerPid = $Snapshot.PSObject.Properties['pid']
    if ($null -eq $ownerPid -or $ownerPid.Value -ne $Owner) { return $false }
    $cached = $Snapshot.PSObject.Properties['cached_reader']
    if ($null -eq $cached -or $null -eq $cached.Value) { return $false }
    $reader = $cached.Value
    $focus = $reader.PSObject.Properties['active_draft_focus']
    return $reader.active_item -eq $Item -and $reader.focused_pane -eq $Pane -and
        $reader.offline -and $reader.signed_out -and
        ($NativeFocus -or ($null -ne $focus -and $focus.Value -ceq $Id))
}

function Get-OwnedOfflineDraftFocus([IntPtr]$Window, [string]$Id, [long]$Item, [int]$Pane) {
    if (!(Test-OwnedFixtureWindow $Window) -or [ThreadlineWinInput]::GetForegroundWindow() -ne $Window) {
        Stop-Blocked 'The isolated offline reader lost its verified foreground before typing.'
    }
    $editor = Find-OwnedFixtureControl $Window $Id 'Local Markdown draft' -Edit
    if ($null -eq $editor) { return $null }
    try {
        $focused = [Windows.Automation.AndCondition]::new([Windows.Automation.Condition[]]@(
            [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty, [int]$script:app.Id)
            [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::HasKeyboardFocusProperty, $true)
        ))
        $nativeFocus = $editor.Current.HasKeyboardFocus -or
            $null -ne $editor.FindFirst([Windows.Automation.TreeScope]::Descendants, $focused)
    } catch [Windows.Automation.ElementNotAvailableException] { return $null }
    if (Test-OfflineDraftFocusReceipt (Read-Observation) $script:app.Id $Id $Item $Pane -NativeFocus:$nativeFocus) {
        if ($nativeFocus) { return 'UI Automation' }
        return 'GPUI FocusHandle'
    }
    return $null
}

function Invoke-OfflineReaderProof([IntPtr]$Window, [string]$Profile) {
    $initial = Wait-Reader 'downloaded workspaces while signed out and offline' {
        param($reader) $reader.offline -and $reader.signed_out -and $reader.cached_content_ready -and $reader.pane_count -eq 2
    }
    $pane = $initial.cached_reader.focused_pane
    $item = $initial.cached_reader.active_item
    $tabCounts = $initial.cached_reader.tab_counts -join ','
    Click-AccountControl $Window "reader-tab-$pane-0"
    # The seeded PR starts in Files. Ordinary comment drafts are visible in Conversation.
    Click-AccountControl $Window "pr-$pane-conversation"
    $conversation = Wait-Reader 'the same downloaded pull request in Conversation' {
        param($reader)
        $pr = $reader.PSObject.Properties['pull_request']
        $reader.active_item -eq $item -and $reader.focused_pane -eq $pane -and
            $reader.offline -and $reader.signed_out -and $null -ne $pr -and $null -ne $pr.Value -and
            $pr.Value.section -ceq 'Conversation'
    }
    $editor = "reader-draft-input-$pane"
    if ($null -eq $conversation.cached_reader.active_draft_chars) {
        [ThreadlineWinInput]::Chord(@(0x11, 0x12, 0x44))
    } else {
        Click-AccountControl $Window $editor
    }
    Wait-For 'the visible exact owned local Markdown draft editor' {
        $null -ne (Find-OwnedFixtureControl $Window $editor 'Local Markdown draft' -Edit)
    }
    Wait-For 'keyboard focus on the exact owned local Markdown draft editor' {
        Get-OwnedOfflineDraftFocus $Window $editor $item $pane
    }
    $draft = 'Offline synthetic draft 04'
    $hash = [Security.Cryptography.SHA256]::Create()
    try {
        $digest = [BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes($draft))).Replace('-', '').ToLowerInvariant()
    } finally { $hash.Dispose() }
    $focus = Get-OwnedOfflineDraftFocus $Window $editor $item $pane
    if (!$focus) { Stop-Blocked 'The owned draft editor lost keyboard focus before typing.' }
    [ThreadlineWinInput]::ReplaceFixtureText($Window, $script:app.Id, $draft)
    $null = Wait-Reader 'the exact native draft saved in the offline profile' {
        param($reader)
        $reader.active_item -eq $item -and $reader.focused_pane -eq $pane -and
            $reader.active_draft_saved -and $reader.active_draft_sha256 -ceq $digest
    }
    Add-Evidence 'Type and save a local Markdown draft offline' ([ordered]@{
        item = $item; draft_sha256 = $digest; focus_provider = $focus; input = 'guarded Win32 SendInput'
    })
    $null = Restart-Reader $Profile
    $null = Wait-Reader 'the original item, pane layout and saved draft without login' {
        param($reader)
        $reader.offline -and $reader.signed_out -and $reader.cached_content_ready -and
            $reader.active_item -eq $item -and $reader.focused_pane -eq $pane -and
            ($reader.tab_counts -join ',') -ceq $tabCounts -and
            $reader.active_draft_saved -and $reader.active_draft_sha256 -ceq $digest
    }
    Add-Evidence 'Restore cached content, tab layout and the exact local draft offline' ([ordered]@{
        item = $item
        pane = $pane
        draft_sha256 = $digest
    })
    Stop-ThroughUi
    $script:report.detail = 'Cached content, panes and the local Markdown draft survived a separate-process restart while signed out and offline.'
}

function Invoke-ReaderResumeProof([IntPtr]$Window, [string]$Profile) {
    Set-AccessPhase 'reader-failure'
    Wait-SeededFixtureAccount
    # Keep the shared request budget available for this explicit pin and continuation.
    Open-SyncSettings $Window
    Click-AccountControl $Window 'sync-pause-background'
    $null = Wait-Sync 'background refresh paused before the explicit offline download' {
        param($sync) $sync.controls.pause_background -and !$sync.controls.work_offline
    }
    Click-AccountControl $Window 'reader-browse'
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-fetch-repository'
    Click-AccountControl $Window 'reader-result-0'
    $start = Wait-Reader 'the downloaded issue from the repository list' {
        param($reader) $reader.cached_content_ready -and $null -ne $reader.active_item
    }
    $item = $start.cached_reader.active_item
    Click-AccountControl $Window 'reader-fetch-comments-0'
    $null = Wait-Reader 'one saved page with Partial discussion coverage' {
        param($reader) $reader.active_item -eq $item -and $reader.comments_coverage -ceq 'Partial' -and $reader.comment_count -eq 1
    }
    Click-AccountControl $Window 'reader-download-0'
    $null = Wait-Reader 'a failed later page preserving the first page and the download request' {
        param($reader)
        $reader.active_item -eq $item -and $reader.comments_coverage -ceq 'Partial' -and
            $reader.comment_count -eq 1 -and $reader.comment_fetch_state -ceq 'Failed' -and
            $reader.download_state -cin @('Active', 'Pending')
    }
    Add-Evidence 'Keep Partial discussion and an offline request after a failed page two' ([ordered]@{
        item = $item
        discussion = 'Partial'
        downloaded_comments = 1
    })

    Open-SyncSettings $Window
    Click-AccountControl $Window 'sync-work-offline'
    $null = Wait-Sync 'the saved download held while Work Offline is enabled' {
        param($sync) $sync.controls.work_offline -and $sync.connection -ceq 'Offline'
    }
    Set-AccessPhase 'sync-offline'
    $restarted = Restart-Reader $Profile
    $null = Wait-Reader 'restored Partial coverage with the same pending download' {
        param($reader)
        $reader.offline -and $reader.cached_content_ready -and $reader.active_item -eq $item -and
            $reader.comments_coverage -ceq 'Partial' -and $reader.comment_count -eq 1 -and
            $reader.download_state -cin @('Active', 'Pending')
    }
    Open-SyncSettings $restarted
    $null = Wait-Sync 'the original background pause and Work Offline setting after restart' {
        param($sync) $sync.controls.pause_background -and $sync.controls.work_offline
    }
    Set-AccessPhase 'reader-resume'
    Click-AccountControl $restarted 'sync-work-offline'
    $null = Wait-Sync 'leave Work Offline while background refresh stays paused' {
        param($sync) !$sync.controls.work_offline -and $sync.controls.pause_background
    }
    Wait-SeededFixtureAccount
    $null = Wait-Reader 'the worker resumes the original download with two unique comments' {
        param($reader)
        $reader.active_item -eq $item -and $reader.comments_coverage -ceq 'Complete' -and
            $reader.comment_count -eq 2 -and $reader.download_state -ceq 'Complete'
    } -Timeout 45000
    Add-Evidence 'Automatically resume the saved download after reconnecting the isolated account' ([ordered]@{
        item = $item
        discussion = 'Complete'
        downloaded_comments = 2
    })
    Stop-ThroughUi
    $script:report.detail = 'The native reader showed Partial discussion, restored the pending download offline and resumed it automatically after reconnecting.'
}

function Get-ReaderBudget([double[]]$Samples, [double]$Limit) {
    $p95 = $null
    $status = 'Blocked'
    if ($Samples.Count -eq 20) {
        $ordered = $Samples.Clone()
        [Array]::Sort($ordered)
        $p95 = $ordered[[int][Math]::Ceiling(0.95 * $ordered.Count) - 1]
        $status = if ($p95 -lt $Limit) { 'Pass' } else { 'Fail' }
    }
    return [ordered]@{
        sample_count = $Samples.Count
        samples_ms = $Samples
        p95_ms = $p95
        limit_ms = $Limit
        status = $status
    }
}

function Set-ReaderPerformanceQuery([IntPtr]$Window, [int]$Sample, [switch]$Physical) {
    if ($Sample -lt 0 -or $Sample -ge 20) { throw 'Unknown Section 16 search fixture index.' }
    $targetItem = 1000 + 3 * $Sample
    $query = "Investigate relay behavior $targetItem"
    $id = 'reader-search'
    $name = 'Search downloaded content'
    $null = Wait-AccountControl $id
    if ($Physical) {
        Click-AccountControl $Window $id
        Assert-ReaderPerformanceSearchFocus $Window
        [ThreadlineWinInput]::ReplaceFixtureText($Window, [uint32]$script:app.Id, $query)
    } else {
        Wait-For 'the exact editable benchmark search field' {
            Set-OwnedFixtureEditValue $Window $id $name $query
        }
    }
    Wait-For 'the synthetic benchmark query in the native search field' {
        $pattern = Get-OwnedFixtureEditPattern $Window $id $name
        try {
            $null -ne $pattern -and $pattern.Current.Value -ceq $query
        } catch [Windows.Automation.ElementNotAvailableException] {
            $false
        }
    }
    return $targetItem
}

function Assert-ReaderPerformanceSearchFocus([IntPtr]$Window) {
    $id = 'reader-search'
    $name = 'Search downloaded content'
    if ([ThreadlineWinInput]::GetForegroundWindow() -ne $Window -or !(Test-OwnedFixtureWindow $Window)) {
        Stop-Blocked 'Physical reader timing requires the exact Threadline window in the foreground.'
    }
    try {
        Wait-For 'keyboard focus on the exact owned search editor' {
            $element = Find-OwnedFixtureControl $Window $id $name -Edit
            $null -ne $element -and $element.Current.HasKeyboardFocus
        } 1500 25
    } catch {
        Stop-Blocked 'The exact owned search editor did not confirm keyboard focus.'
    }
    if ([ThreadlineWinInput]::GetForegroundWindow() -ne $Window) {
        Stop-Blocked 'The physical reader timing window lost foreground before key input.'
    }
}

function Get-ReaderTimingAttribution([hashtable]$InputTiming, [object]$State, [string]$Kind, [long]$Target, [long]$Observed) {
    $frequency = [Diagnostics.Stopwatch]::Frequency
    $details = [ordered]@{ input = $InputTiming.method; status = 'Unavailable' }
    $begin = $InputTiming.begin
    $returned = $InputTiming.returned
    if (![Diagnostics.Stopwatch]::IsHighResolution -or $begin -le 0 -or $returned -lt $begin -or $Observed -lt $returned) {
        return $details
    }
    $details.input_call_ms = 1000.0 * ($returned - $begin) / $frequency
    $details.return_to_observation_ms = 1000.0 * ($Observed - $returned) / $frequency
    if ($null -eq $State) { return $details }
    $reader = $State.PSObject.Properties['cached_reader']
    if ($null -eq $reader -or $null -eq $reader.Value) { return $details }
    $native = $reader.Value.PSObject.Properties['performance_timing']
    if ($null -eq $native -or $null -eq $native.Value) { return $details }
    $native = $native.Value
    $handler = $native.handler_qpc
    $paint = $native.painted_qpc
    if ($native.frequency -ne $frequency -or $native.kind -cne $Kind -or $native.target -ne $Target -or
        $handler -lt $begin -or $paint -lt $handler -or $Observed -lt $paint) {
        $details.status = 'Uncorrelated'
        return $details
    }
    $details.input_to_handler_ms = 1000.0 * ($handler - $begin) / $frequency
    $details.handler_to_useful_paint_ms = 1000.0 * ($paint - $handler) / $frequency
    $details.paint_to_observation_ms = 1000.0 * ($Observed - $paint) / $frequency
    $details.paint_before_input_return = $paint -le $returned
    $details.status = 'Correlated'
    return $details
}

function Get-ConcurrentFeedState([int]$Page) {
    $directory = [IO.Path]::GetDirectoryName($script:AccessPhaseFile)
    foreach ($state in @('served','response-failed','disconnected','expired','stopped','held')) {
        if ([IO.File]::Exists([IO.Path]::Combine($directory,"reader-feed-$Page-$state"))) {
            return $state
        }
    }
    return 'waiting'
}

function Wait-ConcurrentFeed([int]$Page, [switch]$Served) {
    $wanted = if ($Served) { 'served' } else { 'held' }
    # Waiting for a scheduled request happens outside the client input-to-observation clock.
    $deadline = if ($Served) { 4000 } else { 30000 }
    $wait = [Diagnostics.Stopwatch]::StartNew()
    do {
        if ($script:app.HasExited) { throw 'Threadline exited during concurrent feed verification.' }
        $state = Get-ConcurrentFeedState $Page
        if ($state -ceq $wanted) { return }
        if ($state -cnotin @('waiting','held')) {
            Stop-Blocked "Scheduled loopback feed page $Page ended in state $state while waiting for $wanted."
        }
        Start-Sleep -Milliseconds 25
    } while ($wait.ElapsedMilliseconds -lt $deadline)
    Stop-Blocked "Scheduled loopback feed page $Page stayed in state $state while waiting for $wanted."
}

function Complete-ConcurrentFeed([int]$Page, $Attribution) {
    $directory = [IO.Path]::GetDirectoryName($script:AccessPhaseFile)
    $state = Get-ConcurrentFeedState $Page
    if ($state -cne 'held' -or
        $Attribution.status -cne 'Correlated' -or $Attribution.input -cne 'UIA InvokePattern') {
        Stop-Blocked "Scheduled loopback feed page $Page could not be correlated through the useful native paint. Request $state, timing $($Attribution.status), input $($Attribution.input)."
    }
    $ack = [IO.Path]::Combine($directory,"reader-feed-$Page-painted")
    [IO.File]::Open($ack, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None).Dispose()
    Wait-ConcurrentFeed $Page -Served
    $Attribution.feed_page = $Page
    $Attribution.feed_response_written = $true
}

function Wait-ReaderPerformanceState([string]$Description, [scriptblock]$Ready, [string]$Kind,
    [bool]$Concurrent, [int]$Page, [hashtable]$InputTiming) {
    if (!$Concurrent) { return Wait-Reader $Description $Ready 5 }
    $pending = [ordered]@{ page=$Page; action=$Kind; useful_view=$false; paint='Unavailable'; request='held' }
    $deadline = [Diagnostics.Stopwatch]::StartNew()
    do {
        $pending.request = Get-ConcurrentFeedState $Page
        if ($pending.request -cne 'held') { break }
        $state = Read-Observation
        if ($null -ne $state -and $state.PSObject.Properties['cached_reader'] -and
            $null -ne $state.cached_reader -and (& $Ready $state.cached_reader)) {
            $pending.useful_view = $true
            $target = if ($Kind -ceq 'search') { $state.cached_reader.search_generation } else { $state.cached_reader.active_item }
            $timing = Get-ReaderTimingAttribution $InputTiming $state $Kind $target ([Diagnostics.Stopwatch]::GetTimestamp())
            $pending.paint = $timing.status
            if ($timing.status -ceq 'Correlated') { return $state }
        }
        Start-Sleep -Milliseconds 5
    } while ($deadline.ElapsedMilliseconds -lt 8000)
    $script:report.concurrent_pending = $pending
    Stop-Blocked "Scheduled feed page $Page did not cover $Description. Request $($pending.request), useful view $($pending.useful_view), paint $($pending.paint)."
}

function Measure-ReaderPerformance([IntPtr]$Window, $Samples, $Attribution, [switch]$Physical, [switch]$Concurrent) {
    $initial = Wait-Reader 'twenty restored tabs across two cached panes' {
        param($reader)
        $reader.offline -eq (!$Concurrent) -and $reader.signed_out -eq (!$Concurrent) -and $reader.cached_content_ready -and
            $reader.pane_count -eq 2 -and ($reader.tab_counts -join ',') -ceq '10,10'
    }
    if ($null -eq $initial.bounds.PSObject.Properties['reader-tab-0-1'] -or
        $null -eq $initial.bounds.PSObject.Properties['reader-tab-1-0']) {
        Stop-Blocked 'The Section 16 reader workload needs enough logical display area for both native panes.'
    }
    $clock = [Diagnostics.Stopwatch]::new()
    $inputTiming = @{ method = $null; begin = 0L; returned = 0L }
    $generation = 0
    $feedPage = 1
    for ($sample = 0; $sample -lt 20; $sample++) {
        # These numbers identify distinct cached issues outside the twenty restored tabs.
        $targetItem = Set-ReaderPerformanceQuery $Window $sample -Physical:$Physical
        if ($Concurrent) { Wait-ConcurrentFeed $feedPage }
        if ($Physical) {
            Assert-ReaderPerformanceSearchFocus $Window
            $clock.Restart()
            $inputTiming.begin = [Diagnostics.Stopwatch]::GetTimestamp()
            [ThreadlineWinInput]::Chord(@(0x0D))
            $inputTiming.returned = [Diagnostics.Stopwatch]::GetTimestamp()
            $inputTiming.method = 'Win32 Enter'
        } else {
            Click-AccountControl $Window 'reader-search-submit' $clock $inputTiming
        }
        $found = Wait-ReaderPerformanceState 'the first local result page for the current query' {
            param($reader)
            $null -ne $reader.search_generation -and $reader.search_generation -gt $generation -and
                $reader.search_results -gt 0 -and $reader.first_search_item -eq $targetItem
        } 'search' $Concurrent $feedPage $inputTiming
        $sampleMs = $clock.Elapsed.TotalMilliseconds
        $observed = [Diagnostics.Stopwatch]::GetTimestamp()
        $generation = $found.cached_reader.search_generation
        $timing = Get-ReaderTimingAttribution $inputTiming $found 'search' $generation $observed
        if ($Concurrent) { Complete-ConcurrentFeed $feedPage $timing; $feedPage++ }
        $Samples.local_search.Add($sampleMs)
        $Attribution.local_search.Add($timing)

        if ($Concurrent) { Wait-ConcurrentFeed $feedPage }
        Click-AccountControl $Window 'reader-result-0' $clock $inputTiming
        $opened = Wait-ReaderPerformanceState 'useful cached content in the newly opened item' {
            param($reader) $reader.active_item -eq $targetItem -and $reader.cached_content_ready
        } 'open' $Concurrent $feedPage $inputTiming
        $sampleMs = $clock.Elapsed.TotalMilliseconds
        $observed = [Diagnostics.Stopwatch]::GetTimestamp()
        $timing = Get-ReaderTimingAttribution $inputTiming $opened 'open' $targetItem $observed
        if ($Concurrent) { Complete-ConcurrentFeed $feedPage $timing; $feedPage++ }
        $Samples.cached_open.Add($sampleMs)
        $Attribution.cached_open.Add($timing)
        Click-AccountControl $Window 'reader-close-tab'
        $null = Wait-Reader 'the original twenty tabs after closing the measured item' {
            param($reader) ($reader.tab_counts -join ',') -ceq '10,10'
        }
    }
    Click-AccountControl $Window 'reader-tab-0-0'
    $before = Wait-Reader 'the first cached tab and two visible panes' {
        param($reader) $reader.focused_pane -eq 0 -and $reader.cached_content_ready -and $reader.pane_count -eq 2
    }
    $previous = $before.cached_reader.active_item
    for ($sample = 0; $sample -lt 20; $sample++) {
        $position = ($sample + 1) % 2
        if ($Concurrent) { Wait-ConcurrentFeed $feedPage }
        Click-AccountControl $Window "reader-tab-0-$position" $clock $inputTiming
        $after = Wait-ReaderPerformanceState 'useful cached content in the other tab with both panes retained' {
            param($reader)
            $reader.active_item -ne $previous -and $reader.focused_pane -eq 0 -and
                $reader.cached_content_ready -and ($reader.tab_counts -join ',') -ceq '10,10'
        } 'tab' $Concurrent $feedPage $inputTiming
        $sampleMs = $clock.Elapsed.TotalMilliseconds
        $observed = [Diagnostics.Stopwatch]::GetTimestamp()
        $previous = $after.cached_reader.active_item
        $timing = Get-ReaderTimingAttribution $inputTiming $after 'tab' $previous $observed
        if ($Concurrent) { Complete-ConcurrentFeed $feedPage $timing; $feedPage++ }
        $Samples.cached_tab_switch.Add($sampleMs)
        $Attribution.cached_tab_switch.Add($timing)
    }
}

function Invoke-ReaderPerformance([IntPtr]$Window, [switch]$KeepOpen, [switch]$Physical, [switch]$Concurrent) {
    $limits = [ordered]@{ cached_open = 100; local_search = 200; cached_tab_switch = 100 }
    $samples = @{}
    $attribution = @{}
    foreach ($metric in $limits.Keys) {
        $samples[$metric] = [System.Collections.Generic.List[double]]::new()
        $attribution[$metric] = [System.Collections.Generic.List[object]]::new()
    }
    $script:report.timing = if ($Concurrent) {
        'Owned UI Automation input to useful app observation. Each correlated paint overlaps a different scheduled loopback feed page.'
    } elseif ($Physical) {
        'Owned physical Enter or Win32 click to useful app observation. Query entry verified before timing. Polling requested at 5 ms.'
    } else {
        'Verified native UI Automation or owned Win32 activation to useful app observation. Query entry verified before timing. Polling requested at 5 ms.'
    }
    $script:report.attribution = [ordered]@{
        source = 'Windows QueryPerformanceCounter. App paint callback precedes platform presentation.'
    }
    if ($null -eq $script:report['budgets']) { $script:report['budgets'] = [ordered]@{} }
    try {
        Measure-ReaderPerformance $Window $samples $attribution -Physical:$Physical -Concurrent:$Concurrent
    } finally {
        foreach ($metric in $limits.Keys) {
            $script:report.budgets[$metric] = Get-ReaderBudget ($samples[$metric].ToArray()) $limits[$metric]
            $script:report.attribution[$metric] = $attribution[$metric].ToArray()
        }
    }
    if (!$KeepOpen) { Stop-ThroughUi }
    if ($Concurrent) { $script:report.concurrent_checks_passed = $true }
    if (!$KeepOpen) {
        if ($script:report.budgets.Values.status -contains 'Fail') {
            throw 'One or more Section 16 cached-reader p95 budgets failed.'
        }
        if ($script:report.budgets.Values.status -contains 'Blocked') {
            Stop-Blocked 'One or more Section 16 cached-reader timings did not finish.'
        }
        $script:report.detail = 'Cached opening, local search and tab switching passed their Section 16 p95 budgets.'
    }
}

function Invoke-ConcurrentReaderPerformance([IntPtr]$Window) {
    $null = Wait-Account 'the seeded and reauthorized benchmark account' {
        param($account) $account.active_user_id -eq 41 -and $account.profile_access -ceq 'Readable'
    }
    Open-SyncSettings $Window
    $null = Wait-Sync 'the seeded offline background pause' {
        param($sync) $sync.controls.work_offline -and $sync.controls.pause_background -and !$sync.controls.pause_queued_actions
    }
    Click-AccountControl $Window 'sync-pause-outbox'
    $null = Wait-Sync 'queued actions remain paused for the scoped read workload' {
        param($sync) $sync.controls.pause_queued_actions
    }
    Set-AccessPhase 'backlog-concurrent'
    Click-AccountControl $Window 'sync-work-offline'
    $null = Wait-Sync 'the authorized online session keeps both pauses' {
        param($sync) !$sync.controls.work_offline -and $sync.controls.pause_queued_actions -and $sync.controls.pause_background
    }
    Click-AccountControl $Window 'sync-pause-background'
    $null = Wait-Sync 'the read worker is enabled with writes paused' {
        param($sync) !$sync.controls.work_offline -and $sync.controls.pause_queued_actions -and !$sync.controls.pause_background
    }
    Click-AccountControl $Window 'reader-browse'
    Invoke-ReaderPerformance $Window -Concurrent
}

function Wait-LargeWorkspaceReady([IntPtr]$Window) {
    $state = Wait-Reader 'both visible cached panes and all twenty offline tabs' {
        param($reader)
        $panes = $reader.PSObject.Properties['cached_visible_panes']
        $null -ne $panes -and $panes.Value -eq 2 -and $reader.offline -and $reader.signed_out -and
            ($reader.tab_counts -join ',') -ceq '10,10'
    } 5
    if (![ThreadlineWinInput]::IsWindowVisible($Window) -or $state.pid -ne $script:app.Id -or
        $null -eq $state.bounds.PSObject.Properties['reader-tab-0-0'] -or
        $null -eq $state.bounds.PSObject.Properties['reader-tab-1-0']) {
        Stop-Blocked 'Both cached panes need to be visible in the owned Threadline window.'
    }
    return $state
}

function Get-LargeStartupTiming($State, [long]$LaunchBegin, [long]$LaunchReturned, [long]$Observed) {
    $ticksToMs = 1000.0 / [Diagnostics.Stopwatch]::Frequency
    $milestones = $State.PSObject.Properties['startup_milestones_ms']
    $result = [ordered]@{
        status = 'Unavailable'
        process_start_call_ms = ($LaunchReturned - $LaunchBegin) * $ticksToMs
        app_ready_ms = $State.startup_ms
        app_milestones_ms = if ($null -ne $milestones) { $milestones.Value } else { $null }
        launch_to_cached_panes_paint_ms = $null
        app_start_to_cached_panes_paint_ms = $null
        paint_to_verified_receipt_ms = $null
    }
    $paint = $State.PSObject.Properties['first_cached_workspace_paint']
    if ($null -eq $paint -or $null -eq $paint.Value) { return $result }
    $stamp = $paint.Value
    if ($stamp.frequency -ne [Diagnostics.Stopwatch]::Frequency -or
        $stamp.qpc -le $LaunchBegin -or $stamp.qpc -gt $Observed -or $stamp.since_app_start_ms -lt 0) {
        return $result
    }
    $result.status = 'Correlated'
    $result.launch_to_cached_panes_paint_ms = ($stamp.qpc - $LaunchBegin) * $ticksToMs
    $result.app_start_to_cached_panes_paint_ms = $stamp.since_app_start_ms
    $result.paint_to_verified_receipt_ms = ($Observed - $stamp.qpc) * $ticksToMs
    return $result
}

function Get-LargeIdleStatus($Idle, [int]$ProcessId) {
    if ($null -eq $Idle) { return 'Blocked' }
    if ($Idle.pid -ne $ProcessId -or $Idle.settle_ms -ne 2000 -or $Idle.sample_ms -ne 3000 -or
        @($Idle.draws_per_second).Count -ne 3 -or @($Idle.presents_per_second).Count -ne 3) {
        throw 'The passive idle capture does not identify the owned window and bounded interval.'
    }
    if (@($Idle.draws_per_second | Where-Object { $_ -gt 0 }).Count -eq 3 -or
        @($Idle.presents_per_second | Where-Object { $_ -gt 0 }).Count -eq 3) { return 'Fail' }
    if (@($Idle.draws_per_second + $Idle.presents_per_second | Where-Object { $_ -ne 0 }).Count -ne 0) { return 'Blocked' }
    return 'Pass'
}

function Read-LargeIdleSample {
    $path = [IO.Path]::ChangeExtension($script:reportFile, 'idle.json')
    try {
        Wait-For 'the app-owned passive renderer sample' { [IO.File]::Exists($path) } 8500
    } catch {
        if ($script:app.HasExited) { throw 'Threadline exited before the idle sample completed.' }
        return $null
    }
    return [IO.File]::ReadAllText($path) | ConvertFrom-Json
}

function Invoke-LargeWorkspaceProof([IntPtr]$Window, [string]$Profile, [double]$InitialStartupMs, $InitialStartupTiming) {
    $script:app.Refresh()
    $startup = @{ warm_start_ms = $null }
    $runtime = [ordered]@{
        initial_offline_start_ms = $InitialStartupMs
        initial_offline_start_timing = $InitialStartupTiming
        initial_offline_peak_working_set_bytes = $script:app.PeakWorkingSet64
        warm_start_ms = $null
        warm_start_timing = $null
        warm_process_id = $null
        idle_frames = $null
        idle_working_set_bytes = $null
        peak_working_set_bytes = $null
        gpu_memory = [ordered]@{ idle = $null; after_actions = $null; after_drain = $null }
    }
    $script:report.runtime = $runtime
    $Window = Restart-Reader $Profile $startup
    $runtime.warm_start_ms = $startup.warm_start_ms
    $runtime.warm_start_timing = $startup.warm_start_timing
    $runtime.warm_process_id = $script:app.Id
    $runtime.idle_frames = Read-LargeIdleSample
    $idleStatus = Get-LargeIdleStatus $runtime.idle_frames $script:app.Id
    $script:app.Refresh()
    $runtime.idle_working_set_bytes = $script:app.WorkingSet64
    $runtime.gpu_memory.idle = Get-GpuProcessMemory $script:app.Id
    $limits = Get-Content -LiteralPath $BudgetFile -Raw | ConvertFrom-Json
    $script:report.budgets = [ordered]@{
        warm_start_ms = [ordered]@{
            value = $runtime.warm_start_ms
            limit = 2000
            status = if ($null -eq $runtime.warm_start_ms) { 'Blocked' }
                elseif ($runtime.warm_start_ms -gt 0 -and $runtime.warm_start_ms -lt 2000) { 'Pass' }
                else { 'Fail' }
        }
        idle_redraw = [ordered]@{ status = $idleStatus }
        idle_working_set_bytes = [ordered]@{
            value = $runtime.idle_working_set_bytes
            limit = $limits.idle_working_set_bytes
            status = Compare-Measurement $runtime.idle_working_set_bytes $limits.idle_working_set_bytes
        }
    }
    Invoke-ReaderPerformance $Window -KeepOpen
    Click-AccountControl $Window 'reader-outbox'
    $expectedUnresolved = $script:BacklogSeed.unresolved_outbox
    $null = Wait-Reader 'the signed-out native queue shows the complete unresolved fixture count' {
        param($reader) $reader.offline -and $reader.outbox_visible -and $reader.outbox -eq $expectedUnresolved
    }
    Click-AccountControl $Window 'reader-outbox-action-0'
    $null = Wait-Reader 'the first seeded action is queued' {
        param($reader) $reader.outbox_action_state -ceq 'Queued'
    }
    $cancel = [Diagnostics.Stopwatch]::new()
    Click-AccountControl $Window 'reader-outbox-cancel' $cancel
    $null = Wait-Reader 'native cancellation committed and only one action left the unresolved count' {
        param($reader)
        $reader.outbox_action_state -ceq 'Cancelled' -and $reader.outbox -eq ($expectedUnresolved - 1) -and $reader.offline
    } 5
    Add-Evidence 'Inspect and cancel the first queued action in the full offline Outbox' ([ordered]@{
        unresolved_before = $expectedUnresolved; unresolved_after = $expectedUnresolved - 1; cancellation_ms = $cancel.Elapsed.TotalMilliseconds
    })
    $script:app.Refresh()
    $runtime.peak_working_set_bytes = [Math]::Max($runtime.initial_offline_peak_working_set_bytes, $script:app.PeakWorkingSet64)
    $runtime.gpu_memory.after_actions = Get-GpuProcessMemory $script:app.Id
    $gpuTotals = @($runtime.gpu_memory.Values | Where-Object { $null -ne $_ } | ForEach-Object {
        $_.dedicated_bytes + $_.shared_bytes
    })
    $gpuMaximum = if ($gpuTotals.Count) { ($gpuTotals | Measure-Object -Maximum).Maximum } else { $null }
    $budgets = $script:report.budgets
    $budgets.peak_working_set_bytes = [ordered]@{
        value = $runtime.peak_working_set_bytes
        limit = $limits.peak_working_set_bytes
        status = Compare-Measurement $runtime.peak_working_set_bytes $limits.peak_working_set_bytes
    }
    $budgets.gpu_memory_bytes = Get-GpuBudget $gpuMaximum $gpuTotals.Count $limits.gpu_memory_bytes
    if ($gpuTotals.Count -ne $runtime.gpu_memory.Count -and $budgets.gpu_memory_bytes.status -eq 'Pass') {
        $budgets.gpu_memory_bytes.status = 'Blocked'
    }
    if ($Artifact) {
        $packageBytes = (Get-Item -LiteralPath $Artifact).Length
        $runtime.package = [ordered]@{ bytes = $packageBytes; sha256 = Get-FileSha256 $Artifact }
        $budgets.package_bytes = [ordered]@{
            value = $packageBytes; limit = $limits.package_bytes
            status = Compare-Measurement $packageBytes $limits.package_bytes
        }
    }
    $script:report.offline_checks_passed = $true
    try {
        Invoke-ReaderBacklog $Profile $runtime
    } catch {
        if ($budgets.Values.status -contains 'Fail' -and $script:blocked) {
            $script:blocked = $false
            $script:report.backlog_interruption = $_.Exception.Message
            throw 'At least one Section 16 budget failed before the backlog journey could finish.'
        }
        throw
    }
    $budgets.peak_working_set_bytes.value = $runtime.peak_working_set_bytes
    $budgets.peak_working_set_bytes.status = Compare-Measurement $runtime.peak_working_set_bytes $limits.peak_working_set_bytes
    $gpuTotals = @($runtime.gpu_memory.Values | Where-Object { $null -ne $_ } | ForEach-Object {
        $_.dedicated_bytes + $_.shared_bytes
    })
    $gpuMaximum = if ($gpuTotals.Count) { ($gpuTotals | Measure-Object -Maximum).Maximum } else { $null }
    $budgets.gpu_memory_bytes = Get-GpuBudget $gpuMaximum $gpuTotals.Count $limits.gpu_memory_bytes
    if ($gpuTotals.Count -ne $runtime.gpu_memory.Count -and $budgets.gpu_memory_bytes.status -eq 'Pass') {
        $budgets.gpu_memory_bytes.status = 'Blocked'
    }
    if ($budgets.Values.status -contains 'Fail') { throw 'At least one Section 16 large-workspace budget failed.' }
    if ($budgets.Values.status -contains 'Blocked') { Stop-Blocked 'The native backlog journey completed. One or more runtime measurements remain unavailable.' }
    $script:report.detail = 'The native workspace met the measured budgets, kept navigation and cancellation available during a held mutation, and confirmed the remaining eligible actions.'
}

function Invoke-ReaderBacklog([string]$Profile, $Runtime) {
    Set-AccessPhase 'backlog-auth'
    $window = Restart-Reader $Profile -Backlog
    $script:report.backlog = [ordered]@{ process_id = $script:app.Id; held_action = 2; confirmed = $null }
    $null = Wait-Account 'the exact reauthorized fixture account remains offline' {
        param($account) $account.active_user_id -eq 41 -and $account.profile_access -ceq 'Readable'
    }
    $null = Wait-Reader 'the same two offline panes and the durable first cancellation' {
        param($reader) $reader.offline -and $reader.outbox -eq 900 -and ($reader.tab_counts -join ',') -ceq '10,10'
    }
    Click-AccountControl $window 'reader-tab-0-0'
    $before = Wait-Reader 'the original cached issue in the left pane' {
        param($reader) $reader.active_number -eq 1 -and $reader.cached_content_ready
    }
    $null = Open-AccountFixture
    Set-AccessPhase 'backlog-hold'
    Click-AccountControl $window 'account-offline'
    Click-AccountControl $window 'account-close'
    $null = Wait-Reader 'the saved workspace running online' { param($reader) !$reader.offline }

    $marker = Join-Path ([IO.Path]::GetDirectoryName($script:AccessPhaseFile)) 'backlog-request-pending.txt'
    Wait-For 'the owned loopback simulator accepted the first eligible request' {
        [IO.File]::Exists($marker)
    } 12000 25
    $navigation = [Diagnostics.Stopwatch]::new()
    $cancellation = [Diagnostics.Stopwatch]::new()
    try {
        Click-AccountControl $window 'reader-tab-0-1' $navigation
        $null = Wait-Reader 'a different cached tab while the write is pending' {
            param($reader) $reader.active_item -ne $before.cached_reader.active_item -and
                $reader.cached_content_ready -and ($reader.tab_counts -join ',') -ceq '10,10'
        } 5
        $script:report.backlog.cached_navigation_ms = $navigation.Elapsed.TotalMilliseconds
        Click-AccountControl $window 'reader-outbox'
        Click-AccountControl $window 'reader-outbox-action-3'
        $null = Wait-Reader 'the next owned backlog action is still queued' {
            param($reader) $reader.outbox_visible -and $reader.outbox_action_state -ceq 'Queued'
        }
        Click-AccountControl $window 'reader-outbox-cancel' $cancellation
        $null = Wait-Reader 'the next action cancels while the first request remains pending' {
            param($reader) $reader.outbox_action_state -ceq 'Cancelled' -and $reader.outbox -eq 899
        } 5
        if (![IO.File]::Exists($marker)) { Stop-Blocked 'The controlled response window ended before the native interaction was verified.' }
        $script:report.backlog.cancellation_ms = $cancellation.Elapsed.TotalMilliseconds
        $script:app.Refresh()
        $script:report.backlog.overlap_working_set_bytes = $script:app.WorkingSet64
    } finally {
        Set-AccessPhase 'backlog-drain'
    }
    Add-Evidence 'Navigate and cancel another queued action while a verified remote operation was in flight' ([ordered]@{
        displayed_tabs = 20; navigation_ms = $script:report.backlog.cached_navigation_ms
        cancellation_ms = $script:report.backlog.cancellation_ms
    })
    Wait-For 'all nine remaining eligible actions confirm in the durable native Outbox' {
        if ($script:app.HasExited) { throw 'Threadline exited before the backlog was drained.' }
        $state = Read-Observation
        $null -ne $state -and $state.pid -eq $script:app.Id -and $state.cached_reader.outbox -eq 890
    } 120000 100
    $script:report.backlog.confirmed = 9
    $script:report.backlog.unresolved_after = 890
    $script:app.Refresh()
    $Runtime.peak_working_set_bytes = [Math]::Max($Runtime.peak_working_set_bytes, $script:app.PeakWorkingSet64)
    $Runtime.gpu_memory.after_drain = Get-GpuProcessMemory $script:app.Id
    Stop-ThroughUi
    $script:report.backlog_checks_passed = $true
}
