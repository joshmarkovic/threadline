function Wait-Reader([string]$Description, [scriptblock]$ReaderCheck, [int]$PollInterval = 45) {
    return Wait-State $Description {
        param($state)
        $cached = $state.PSObject.Properties['cached_reader']
        $null -ne $cached -and $null -ne $cached.Value -and (& $ReaderCheck $cached.Value)
    } $PollInterval
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
    if ($receipt.fixture_revision -cne 's04-reader-v1' -or $receipt.scale -cne $Scale -or
        $receipt.seed -ne $script:Seed -or !$receipt.offline) {
        throw 'The reader fixture does not match the requested isolated workload.'
    }
    if ($Scale -eq 'section-16' -and ($receipt.items -ne 50000 -or $receipt.comments -ne 250000 -or
        $receipt.open_tabs -ne 20 -or $receipt.outbox_count -ne 1000)) {
        throw 'The reader fixture does not match the Section 16 workload.'
    }
    Add-Evidence 'Seed the isolated profile with the application and bundled SQLite' $receipt
}

function Restart-Reader([string]$Profile) {
    Stop-ThroughUi
    $script:app.Dispose()
    $script:app = $null
    $before = Get-IsolatedSqlite $Profile
    $script:reportFile = Join-Path $script:ReportDir 'reader-restart-observation.json'
    $script:app = Start-Threadline $Profile $script:reportFile
    $script:report.restarted_pid = $script:app.Id
    $window = Get-ReadyWindow
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

function Invoke-OfflineReaderProof([IntPtr]$Window, [string]$Profile) {
    $initial = Wait-Reader 'downloaded workspaces while signed out and offline' {
        param($reader) $reader.offline -and $reader.signed_out -and $reader.cached_content_ready -and $reader.pane_count -eq 2
    }
    $pane = $initial.cached_reader.focused_pane
    $item = $initial.cached_reader.active_item
    $tabCounts = $initial.cached_reader.tab_counts -join ','
    # The normal shortcut focuses the Markdown editor even when the conversation is tall.
    Click-AccountControl $Window "reader-tab-$pane-0"
    [ThreadlineWinInput]::Chord(@(0x11, 0x12, 0x44))
    $null = Wait-Reader 'the native local Markdown draft editor' {
        param($reader) $reader.active_item -eq $item -and $null -ne $reader.active_draft_chars
    }
    $draft = 'Offline synthetic draft 04'
    $hash = [Security.Cryptography.SHA256]::Create()
    try {
        $digest = [BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes($draft))).Replace('-', '').ToLowerInvariant()
    } finally { $hash.Dispose() }
    [ThreadlineWinInput]::Chord(@(0x11, 0x41))
    [ThreadlineWinInput]::TypeText($draft)
    $null = Wait-Reader 'the exact native draft saved in the offline profile' {
        param($reader) $reader.active_draft_saved -and $reader.active_draft_sha256 -ceq $digest
    }
    Add-Evidence 'Type and save a local Markdown draft offline' ([ordered]@{ item = $item; draft_sha256 = $digest })

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

    $restarted = Restart-Reader $Profile
    $null = Wait-Reader 'restored Partial coverage with the same pending download' {
        param($reader)
        $reader.cached_content_ready -and $reader.active_item -eq $item -and
            $reader.comments_coverage -ceq 'Partial' -and $reader.comment_count -eq 1 -and
            $reader.download_state -cin @('Active', 'Pending')
    }
    Set-AccessPhase 'reader-resume'
    Wait-SeededFixtureAccount
    Click-AccountControl $restarted 'reader-resume-download-0'
    $null = Wait-Reader 'the resumed offline download completed with two unique comments' {
        param($reader)
        $reader.active_item -eq $item -and $reader.comments_coverage -ceq 'Complete' -and
            $reader.comment_count -eq 2 -and $reader.download_state -ceq 'Complete'
    }
    Add-Evidence 'Resume the original download after reconnecting the isolated account' ([ordered]@{
        item = $item
        discussion = 'Complete'
        downloaded_comments = 2
    })
    Stop-ThroughUi
    $script:report.detail = 'The native reader showed a Partial discussion and resumed its saved offline download after a separate-process restart.'
}

function Get-ReaderBudget([double[]]$Samples, [double]$Limit) {
    if ($Samples.Count -ne 20) { throw 'The reader workload did not collect twenty samples.' }
    $ordered = $Samples.Clone()
    [Array]::Sort($ordered)
    $p95 = $ordered[[int][Math]::Ceiling(0.95 * $ordered.Count) - 1]
    return [ordered]@{
        sample_count = $Samples.Count
        samples_ms = $Samples
        p95_ms = $p95
        limit_ms = $Limit
        status = if ($p95 -lt $Limit) { 'Pass' } else { 'Fail' }
    }
}

function Set-ReaderPerformanceQuery([IntPtr]$Window, [int]$Sample) {
    if ($Sample -lt 0 -or $Sample -ge 20) { throw 'Unknown Section 16 search fixture index.' }
    $targetItem = 1000 + 3 * $Sample
    $query = "Investigate relay behavior $targetItem"
    $id = 'reader-search'
    $name = 'Search downloaded content'
    $null = Wait-AccountControl $id
    Wait-For 'the exact editable benchmark search field' {
        Set-OwnedFixtureEditValue $Window $id $name $query
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
    $native = $State.cached_reader.PSObject.Properties['performance_timing']
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

function Invoke-ReaderPerformance([IntPtr]$Window, [switch]$KeepOpen) {
    $initial = Wait-Reader 'twenty restored tabs across two offline panes' {
        param($reader)
        $reader.offline -and $reader.signed_out -and $reader.cached_content_ready -and
            $reader.pane_count -eq 2 -and ($reader.tab_counts -join ',') -ceq '10,10'
    }
    if ($null -eq $initial.bounds.PSObject.Properties['reader-tab-0-1'] -or
        $null -eq $initial.bounds.PSObject.Properties['reader-tab-1-0']) {
        Stop-Blocked 'The Section 16 reader workload needs enough logical display area for both native panes.'
    }
    $clock = [Diagnostics.Stopwatch]::new()
    $search = [System.Collections.Generic.List[double]]::new()
    $open = [System.Collections.Generic.List[double]]::new()
    $switch = [System.Collections.Generic.List[double]]::new()
    $searchTiming = [System.Collections.Generic.List[object]]::new()
    $openTiming = [System.Collections.Generic.List[object]]::new()
    $switchTiming = [System.Collections.Generic.List[object]]::new()
    $inputTiming = @{ method = $null; begin = 0L; returned = 0L }
    $generation = 0
    for ($sample = 0; $sample -lt 20; $sample++) {
        # These numbers identify distinct cached issues outside the twenty restored tabs.
        $targetItem = Set-ReaderPerformanceQuery $Window $sample
        Click-AccountControl $Window 'reader-search-submit' $clock $inputTiming
        $found = Wait-Reader 'the first local result page for the current query' {
            param($reader)
            $null -ne $reader.search_generation -and $reader.search_generation -gt $generation -and
                $reader.search_results -gt 0 -and $reader.first_search_item -eq $targetItem
        } 5
        $search.Add($clock.Elapsed.TotalMilliseconds)
        $observed = [Diagnostics.Stopwatch]::GetTimestamp()
        $generation = $found.cached_reader.search_generation
        $searchTiming.Add((Get-ReaderTimingAttribution $inputTiming $found 'search' $generation $observed))

        Click-AccountControl $Window 'reader-result-0' $clock $inputTiming
        $opened = Wait-Reader 'useful cached content in the newly opened item' {
            param($reader) $reader.active_item -eq $targetItem -and $reader.cached_content_ready
        } 5
        $open.Add($clock.Elapsed.TotalMilliseconds)
        $observed = [Diagnostics.Stopwatch]::GetTimestamp()
        $openTiming.Add((Get-ReaderTimingAttribution $inputTiming $opened 'open' $targetItem $observed))
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
        Click-AccountControl $Window "reader-tab-0-$position" $clock $inputTiming
        $after = Wait-Reader 'useful cached content in the other tab with both panes retained' {
            param($reader)
            $reader.active_item -ne $previous -and $reader.focused_pane -eq 0 -and
                $reader.cached_content_ready -and ($reader.tab_counts -join ',') -ceq '10,10'
        } 5
        $switch.Add($clock.Elapsed.TotalMilliseconds)
        $observed = [Diagnostics.Stopwatch]::GetTimestamp()
        $previous = $after.cached_reader.active_item
        $switchTiming.Add((Get-ReaderTimingAttribution $inputTiming $after 'tab' $previous $observed))
    }
    $script:report.timing = 'Verified native UI Automation or owned Win32 activation to useful app observation. Query entry verified before timing. Polling requested at 5 ms.'
    $script:report.attribution = [ordered]@{
        source = 'Windows QueryPerformanceCounter. App paint callback precedes platform presentation.'
        cached_open = $openTiming.ToArray()
        local_search = $searchTiming.ToArray()
        cached_tab_switch = $switchTiming.ToArray()
    }
    $script:report.budgets = [ordered]@{
        cached_open = Get-ReaderBudget ($open.ToArray()) 100
        local_search = Get-ReaderBudget ($search.ToArray()) 200
        cached_tab_switch = Get-ReaderBudget ($switch.ToArray()) 100
    }
    if (!$KeepOpen) { Stop-ThroughUi }
    if ($script:report.budgets.Values.status -contains 'Fail') {
        throw 'One or more Section 16 cached-reader p95 budgets failed.'
    }
    $script:report.detail = 'Cached opening, local search and tab switching passed their Section 16 p95 budgets.'
}

function Invoke-LargeWorkspaceProof([IntPtr]$Window) {
    Invoke-ReaderPerformance $Window -KeepOpen
    Click-AccountControl $Window 'reader-outbox'
    $null = Wait-Reader 'the signed-out native queue shows 900 unresolved fixture operations' {
        param($reader) $reader.offline -and $reader.outbox_visible -and $reader.outbox -eq 900
    }
    Click-AccountControl $Window 'reader-outbox-action-0'
    $null = Wait-Reader 'the first seeded action is queued' {
        param($reader) $reader.outbox_action_state -ceq 'Queued'
    }
    $cancel = [Diagnostics.Stopwatch]::new()
    Click-AccountControl $Window 'reader-outbox-cancel' $cancel
    $null = Wait-Reader 'native cancellation committed and only one action left the unresolved count' {
        param($reader)
        $reader.outbox_action_state -ceq 'Cancelled' -and $reader.outbox -eq 899 -and $reader.offline
    } 5
    Add-Evidence 'Inspect and cancel the first queued action in the full offline Outbox' ([ordered]@{
        unresolved_before = 900; unresolved_after = 899; cancellation_ms = $cancel.Elapsed.TotalMilliseconds
    })
    Stop-ThroughUi
    $script:report.offline_checks_passed = $true
    Stop-Blocked 'Offline reader budgets, large Outbox inspection and durable cancellation passed. Concurrent eligible online backlog drain still requires a verified workload.'
}
