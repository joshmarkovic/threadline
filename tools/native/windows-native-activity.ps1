function Read-NativeActivityFixture([string]$Profile, [string]$Stage, [string]$Name) {
    if ($script:ReaderProof -cne 'NativeActivity') { throw 'The activity receipt belongs to the explicit fixture case.' }
    $path = Join-Path $script:ReportDir $Name
    $process = Start-Threadline $Profile $path -ActivityFixture $Stage
    try {
        if (!$process.WaitForExit(10000)) {
            $process.Kill()
            [void]$process.WaitForExit(3000)
            throw 'The isolated activity fixture tool did not finish.'
        }
        if ($process.ExitCode -ne 0) { throw "The activity fixture tool rejected stage '$Stage'." }
    } finally {
        $process.Dispose()
    }
    return [IO.File]::ReadAllText($path) | ConvertFrom-Json
}

function Initialize-NativeActivity([string]$Profile, [string]$Receipt) {
    $seeded = Read-NativeActivityFixture $Profile 'seed' $Receipt
    if ($seeded.fixture_revision -cne 's11-native-activity-v4' -or @($seeded.feed).Count -ne 0 -or
        $seeded.delivered_groups -ne 0 -or $seeded.opened_tabs -ne 0) {
        throw 'The initial repository fixture contained current delivery, activity, or opened-item state.'
    }
    Add-Evidence 'Seed only a prior authorized repository, old monitoring checkpoint and bot rule' $seeded
}

function Read-NativeActivityCapture([string]$Profile) {
    $path = Join-Path $Profile 'native-activity-groups.jsonl'
    if (!(Test-Path -LiteralPath $path -PathType Leaf)) { return }
    $content = [IO.File]::ReadAllText($path)
    if (!$content.EndsWith("`n")) { return }
    foreach ($line in $content -split "`n") {
        if ($line) { $line | ConvertFrom-Json }
    }
}

function Start-NativeActivityWindow([string]$Profile, [string]$Name) {
    if ($null -ne $script:app) {
        if (!$script:app.HasExited) { throw 'The previous owned activity process is still open.' }
        $script:app.Dispose()
    }
    $script:reportFile = Join-Path $script:ReportDir $Name
    $script:app = Start-Threadline $Profile $script:reportFile
    $script:report.restarted_pid = $script:app.Id
    return Get-ReadyWindow
}

function Start-NativeActivitySync([IntPtr]$Window, [string]$Phase) {
    Wait-SeededFixtureAccount
    Open-SyncSettings $Window
    $null = Wait-Sync 'the restored offline scope before activity synchronization' {
        param($sync) $sync.controls.work_offline
    }
    Set-AccessPhase $Phase
    Click-AccountControl $Window 'sync-work-offline'
    $null = Wait-Sync 'the committed online mode' { param($sync) !$sync.controls.work_offline }
    Click-AccountControl $Window 'sync-check-now'
    $null = Wait-Sync 'verified connectivity from the selected repository profile' {
        param($sync) $sync.connection -ceq 'Reachable'
    }
    Open-FixtureHome $Window
}

function Wait-NativeActivityDelivery([string]$Profile, [string]$Stage, [string]$Description, [int]$Timeout) {
    try {
        Wait-For $Description { @((Read-NativeActivityCapture $Profile)).Count -ge 2 } $Timeout 200
    } catch {
        $failure = $_
        try {
            $observed = Read-NativeActivityFixture $Profile 'inspect' "native-activity-$Stage-timeout.json"
            Add-Evidence 'Inspect the durable fixture activity after the delivery wait' ([ordered]@{
                stage = $Stage
                feed_count = @($observed.feed).Count
                suppressed = $observed.suppressed
                delivered_groups = $observed.delivered_groups
                capture_count = @((Read-NativeActivityCapture $Profile)).Count
            })
        } finally {
            throw $failure
        }
    }
}

function Test-NativeActivityBaselineComparison($State) {
    if ($null -eq $State -or !$State.PSObject.Properties['cached_reader'] -or
        $null -eq $State.cached_reader) { return $false }
    $reader = $State.cached_reader
    if (!$reader.PSObject.Properties['pull_request'] -or $null -eq $reader.pull_request) {
        return $false
    }
    $pr = $reader.pull_request
    return $reader.active_number -eq 43 -and $pr.merged -eq $true -and
        $pr.head -ceq ('b' * 40) -and $pr.section -ceq 'Files' -and
        $pr.files_coverage -ceq 'Complete' -and $pr.files_on_page -eq 1
}

function Invoke-NativeActivityProof([IntPtr]$Window, [string]$Profile) {
    Start-NativeActivitySync $Window 'activity-baseline'
    Wait-For 'the bounded background activity baseline' {
        $state = Read-Observation
        $null -ne $state -and $null -ne $state.cached_reader -and
            $null -ne $state.cached_reader.shared_sync -and $state.cached_reader.shared_sync.cache_generation -ge 8
    } 30000 150

    Click-AccountControl $Window 'reader-browse'
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-fetch-repository'
    Activate-FixturePr $Window 'reader-result-' 43 'Activity fixture pull request 43' -Result
    Wait-For 'the old already-merged pull request and its downloaded comparison' {
        Test-NativeActivityBaselineComparison (Read-Observation)
    } 30000 150
    $baseline = Read-NativeActivityFixture $Profile 'inspect' 'native-activity-baseline.json'
    if (@($baseline.feed).Count -ne 0 -or $baseline.delivered_groups -ne 0 -or @((Read-NativeActivityCapture $Profile)).Count -ne 0) {
        throw 'Historical activity produced an alert or feed row at the monitoring baseline.'
    }
    $baseline | Add-Member -NotePropertyName comparison_complete -NotePropertyValue $true
    Add-Evidence 'Download the old pull request and retain a zero-alert baseline' $baseline

    Set-AccessPhase 'activity-fresh'
    Wait-NativeActivityDelivery $Profile 'fresh' 'the native activity worker groups the human review and emits the unopened issue alert' 70000
    $fresh = Read-NativeActivityFixture $Profile 'inspect' 'native-activity-fresh.json'
    $grouped = @(Read-NativeActivityCapture $Profile)
    $issue = @($fresh.feed | Where-Object { $_.key -cmatch '^github:comments:6101:comment_created:' })
    $review = @($fresh.feed | Where-Object { $_.key -cmatch '^github:pr-reviews:6302:' })
    $reply = @($fresh.feed | Where-Object { $_.key -cmatch '^github:pr-inline-comments:6202:' })
    if ($grouped.Count -ne 2 -or (($grouped.events | Sort-Object) -join ',') -cne '1,2' -or
        $issue.Count -ne 1 -or $review.Count -ne 1 -or $reply.Count -ne 1 -or $fresh.suppressed -ne 2 -or
        @($fresh.feed | Where-Object { $_.key -match ':6201:|:6301:' }).Count -ne 0 -or $fresh.opened_tabs -ne 1) {
        throw 'Expected one unopened-item alert, one grouped human review/inline alert and exactly two muted bot events.'
    }
    $reviewTarget = $review[0].target.Item.item
    $issueTarget = $issue[0].target.Item.item
    if ($reviewTarget -eq $issueTarget -or $reply[0].target.Item.item -ne $reviewTarget -or
        @($grouped | Where-Object { $_.events -eq 2 -and $_.destination.target.Item.item -eq $reviewTarget }).Count -ne 1 -or
        @($grouped | Where-Object { $_.events -eq 1 -and $_.destination.target.Item.item -eq $issueTarget }).Count -ne 1) {
        throw 'The activity groups did not retain their distinct exact targets.'
    }
    Add-Evidence 'Observe one unopened issue and one two-event human review group with no bot alert' $fresh

    Click-AccountControl $Window 'reader-browse'
    Activate-FixturePr $Window 'reader-result-' 42 'Activity fixture pull request 42' -Result
    Click-AccountControl $Window 'pr-0-conversation'
    Click-AccountControl $Window 'pr-0-threads-fetch'
    $null = Wait-PrReader 'the exact pull request shows its complete downloaded review thread' {
        param($pr, $reader)
        $reader.active_item -eq $reviewTarget -and $reader.active_number -eq 42 -and
            $pr.threads_coverage -ceq 'Complete' -and $pr.threads_on_page -eq 1
    }
    Click-AccountControl $Window 'pr-0-reviews-fetch'
    Wait-For 'both formal reviews and the linked human reply remain in the downloaded PR' {
        $probe = Read-NativeActivityFixture $Profile 'inspect' ('native-activity-content-' + [guid]::NewGuid().ToString('N') + '.json')
        $thread = $probe.retained_thread
        $probe.bot_content -eq 1 -and $probe.human_content -eq 1 -and $null -ne $thread -and
            $thread.item -eq $reviewTarget -and $thread.thread -ceq 'PRRT_6201' -and
            $thread.root -eq 6201 -and $thread.reply -eq 6202 -and $thread.reply_to -eq 6201 -and
            $thread.root_author -eq 91 -and $thread.reply_author -eq 92 -and
            $thread.root_body_retained -and $thread.reply_body_retained
    } 15000 750
    Stop-ThroughUi
    $firstClosed = Read-NativeActivityFixture $Profile 'inspect' 'native-activity-first-closed.json'
    if ($firstClosed.delivered_groups -ne 2 -or $firstClosed.suppressed -ne 2 -or
        $firstClosed.retained_thread.reply_to -ne 6201 -or
        !$firstClosed.retained_thread.root_body_retained -or !$firstClosed.retained_thread.reply_body_retained) {
        throw 'Downloading the verified thread changed its content or repeated delivery.'
    }
    Add-Evidence 'Preserve the muted bot root, its linked human reply and both formal review bodies after normal exit' $firstClosed

    Set-AccessPhase 'activity-offline'
    $catchupProfile = Join-Path $script:ProfileDir ('initial-' + [guid]::NewGuid().ToString('N'))
    $null = New-Item -ItemType Directory -Path $catchupProfile
    Initialize-NativeActivity $catchupProfile 'native-activity-catchup-seeded.json'
    $catchup = Start-NativeActivityWindow $catchupProfile 'native-activity-catchup-observation.json'
    Start-NativeActivitySync $catchup 'activity-catchup'
    Wait-NativeActivityDelivery $catchupProfile 'catchup' 'one catch-up digest and the separately watched branch comparison' 60000
    $catchupResult = Read-NativeActivityFixture $catchupProfile 'inspect' 'native-activity-catchup.json'
    $catchupGroups = @(Read-NativeActivityCapture $catchupProfile)
    $merge = @($catchupResult.feed | Where-Object { $_.kind -ceq 'pr_merged' })
    $branch = @($catchupResult.feed | Where-Object { $_.kind -ceq 'branch_commit_observed' })
    if ($catchupGroups.Count -ne 2 -or $merge.Count -ne 1 -or $branch.Count -ne 1 -or
        @($catchupGroups | Where-Object { $_.events -eq 3 -and $_.destination.target -ceq 'ActivityFeed' }).Count -ne 1 -or
        @($catchupGroups | Where-Object { $_.events -eq 1 -and $_.destination.target.Comparison.reference -ceq 'refs/heads/main' }).Count -ne 1) {
        throw 'Catch-up did not group exactly two ordinary comments and the one real merge, alongside only the watched branch.'
    }
    Add-Evidence 'Deliver one catch-up digest and one exact watched-branch update with no baseline or unwatched alert' $catchupResult

    Set-AccessPhase 'activity-replay'
    Stop-ThroughUi
    $replay = Start-NativeActivityWindow $catchupProfile 'native-activity-replay-observation.json'
    $null = Wait-Sync 'same checkpoint replay runs after restart' {
        param($sync) $sync.connection -ceq 'Reachable' -and $sync.cache_generation -ge 3
    }
    Open-RestartedFixtureActivity $replay
    $mergeAction = 'activity-open-' + $merge[0].id
    $mergeItem = $merge[0].target.Item.item
    Wait-For 'the exact reopened merge activity action in the owned window' {
        Invoke-OwnedFixtureControl $replay $mergeAction $null 'Open'
    }
    $null = Wait-Reader 'the original cached pull request opened from its restored activity row' {
        param($reader) $reader.cached_content_ready -and $reader.active_number -eq 42 -and $reader.active_item -eq $mergeItem
    }
    Add-Evidence 'Open the exact merged pull request from its restored native Activity row' ([ordered]@{
        activity_id = $merge[0].id; item = $mergeItem; number = 42
    })
    Stop-ThroughUi
    $final = Read-NativeActivityFixture $catchupProfile 'inspect' 'native-activity-replay.json'
    if ($final.opened_tabs -ne 1 -or $final.delivered_groups -ne 2 -or
        (@($final.feed.key | Sort-Object) -join ',') -cne (@($catchupResult.feed.key | Sort-Object) -join ',') -or
        @((Read-NativeActivityCapture $catchupProfile)).Count -ne 2) {
        throw 'Restarted provider observations duplicated a delivery or feed entry.'
    }
    Add-Evidence 'Replay overlapping provider updates after restart without repeating activity' $final
    $script:report.detail = 'Normal native synchronization and reader controls verified a muted bot root with its linked human reply and retained review bodies, baseline silence, unopened activity, human review grouping, offline catch-up, watched-ref movement and a merge transition. The isolated delivery adapter captured requests at the app boundary.'
}
