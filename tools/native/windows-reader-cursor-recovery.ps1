function Read-RecoveryFixture([string]$Profile, [string]$Stage, [string]$ReceiptName) {
    if ($ReaderProof -cne 'CursorRecovery' -or $Stage -cnotin @('arm','snapshot','release') -or
        $ReceiptName -cnotmatch '^reader-recovery-[a-z]+\.json$') {
        throw 'The receipt is outside the isolated discussion recovery case.'
    }
    $receipt = Join-Path $script:ReportDir $ReceiptName
    $process = Start-Threadline $Profile $receipt -ReaderRecovery $Stage
    try {
        if (!$process.WaitForExit(10000)) {
            $process.Kill()
            [void]$process.WaitForExit(3000)
            throw 'The isolated discussion checkpoint inspection timed out.'
        }
        if ($process.ExitCode -ne 0 -or !(Test-Path -LiteralPath $receipt -PathType Leaf)) {
            throw "The isolated discussion checkpoint rejected stage $Stage."
        }
    } finally {
        $process.Dispose()
    }
    return [IO.File]::ReadAllText($receipt) | ConvertFrom-Json
}

function Wait-RecoveryDiscussion([long]$Item, [int]$Count, [string]$Coverage, [bool]$Offline, [switch]$Failed) {
    return Wait-Reader "the original fixture issue with $Count comments and $Coverage coverage" {
        param($reader)
        $reader.cached_content_ready -and $reader.active_item -eq $Item -and $reader.active_number -eq 41 -and
            $reader.offline -eq $Offline -and $reader.comment_count -eq $Count -and
            $reader.comments_coverage -ceq $Coverage -and $reader.outbox -eq 1 -and
            (!$Failed -or $reader.comment_fetch_state -ceq 'Failed')
    }
}

function Confirm-RecoveryState($Original, $Current, [string]$Ids, [bool]$Edited) {
    if ($Current.item -ne $Original.item -or $Current.completed_other_resources -ne 2 -or
        ($Current.other_resource_hashes -join ',') -cne ($Original.other_resource_hashes -join ',') -or
        ($Current.draft_hashes -join ',') -cne ($Original.draft_hashes -join ',') -or
        ($Current.outbox_hashes -join ',') -cne ($Original.outbox_hashes -join ',') -or
        ($Current.comments.id -join ',') -cne $Ids -or $Current.comments[0].edited_original -ne $Edited -or
        $false -cin $Current.comments.metadata_retained) {
        throw 'Discussion recovery changed unrelated committed content, a draft, an Outbox action, or a stable comment.'
    }
}

function Confirm-RecoveryCheckpoint($Before, $After, [int]$ErrorCode) {
    if ($After.coverage -ne 1 -or $After.cursor -cne '2' -or $After.error -ne $ErrorCode -or
        $After.last_page -ne $Before.last_page -or $After.last_success -ne $Before.last_success -or
        $After.last_validation -ne $Before.last_validation) {
        throw 'The failed discussion page changed its committed checkpoint or lost Partial coverage.'
    }
}

function Invoke-ReaderCursorRecoveryProof([IntPtr]$Window, [string]$Profile) {
    Wait-SeededFixtureAccount
    Open-SyncSettings $Window
    Click-AccountControl $Window 'sync-pause-background'
    $null = Wait-Sync 'background refresh paused' { param($sync) $sync.controls.pause_background }
    Click-AccountControl $Window 'sync-pause-outbox'
    $null = Wait-Sync 'background refresh and queued actions paused' {
        param($sync) $sync.controls.pause_background -and $sync.controls.pause_queued_actions
    }
    Click-AccountControl $Window 'reader-browse'
    $item = Open-WriteFixtureIssue $Window -UserConnected
    Click-AccountControl $Window 'reader-fetch-timeline-0'
    $null = Wait-Reader 'the committed original timeline' {
        param($reader) $reader.active_item -eq $item -and $reader.timeline_coverage -ceq 'Complete'
    }
    Click-AccountControl $Window 'reader-fetch-comments-0'
    $null = Wait-Reader 'the committed first discussion page' {
        param($reader) $reader.active_item -eq $item -and $reader.comments_coverage -ceq 'Partial' -and $reader.comment_count -eq 1
    }

    Set-WriteFixtureOffline $Window $true
    Click-AccountControl $Window 'reader-draft-comment-0'
    $draft = 'Threadline S07 explicitly queued comment.'
    Type-WriteFixtureField $Window 'reader-draft-input-0' $draft
    Wait-WriteFixtureDraft $item $draft
    Queue-WriteFixtureDraft $Window $item 1 'Queue comment'

    $window = Restart-Reader $Profile -FixturePhaseAfterQuit 'recover-offline' -RecoveryStage 'arm'
    $original = [IO.File]::ReadAllText((Join-Path $script:ReportDir 'reader-recovery-arm.json')) | ConvertFrom-Json
    $null = Wait-RecoveryDiscussion $item 1 'Partial' $true
    Add-Evidence 'Restore one cached discussion page and the paused Outbox before network access' $original

    Set-AccessPhase 'recover-expired'
    Set-WriteFixtureOffline $window $false
    Click-AccountControl $window 'reader-fetch-comments-0'
    $null = Wait-RecoveryDiscussion $item 1 'Partial' $false -Failed
    $expired = Read-RecoveryFixture $Profile 'snapshot' 'reader-recovery-expired.json'
    Confirm-RecoveryState $original $expired '00000000000000005101' $false
    Confirm-RecoveryCheckpoint $original.discussion $expired.discussion 9
    Add-Evidence 'Keep the previously committed page when its saved continuation returns HTTP 410' $expired

    Set-AccessPhase 'recover-rebuild'
    Click-AccountControl $window 'reader-fetch-comments-0'
    $null = Wait-RecoveryDiscussion $item 2 'Partial' $false
    $repaired = Read-RecoveryFixture $Profile 'snapshot' 'reader-recovery-repaired.json'
    Confirm-RecoveryState $original $repaired '00000000000000005101,00000000000000005102' $true
    if ($repaired.discussion.coverage -ne 1 -or $repaired.discussion.cursor -cne '2' -or
        $null -ne $repaired.discussion.error -or $repaired.discussion.last_page -le $original.discussion.last_page) {
        throw 'The first replacement page did not commit the edited comment and new continuation.'
    }
    Add-Evidence 'Re-enumerate the affected discussion and retain the old comment identity after its edit' $repaired

    Click-AccountControl $window 'reader-fetch-comments-0'
    $null = Wait-RecoveryDiscussion $item 2 'Partial' $false -Failed
    $rolledBack = Read-RecoveryFixture $Profile 'snapshot' 'reader-recovery-rollback.json'
    Confirm-RecoveryState $original $rolledBack '00000000000000005101,00000000000000005102' $true
    Confirm-RecoveryCheckpoint $repaired.discussion $rolledBack.discussion 1
    Add-Evidence 'Roll back a returned discussion page while retaining the complete body and timeline' $rolledBack

    Set-WriteFixtureOffline $window $true
    $window = Restart-Reader $Profile -FixturePhaseAfterQuit 'recover-offline' -RecoveryStage 'release'
    $released = [IO.File]::ReadAllText((Join-Path $script:ReportDir 'reader-recovery-release.json')) | ConvertFrom-Json
    Confirm-RecoveryState $original $released '00000000000000005101,00000000000000005102' $true
    Confirm-RecoveryCheckpoint $repaired.discussion $released.discussion 1
    $null = Wait-RecoveryDiscussion $item 2 'Partial' $true

    Set-AccessPhase 'recover-resume'
    Set-WriteFixtureOffline $window $false
    Click-AccountControl $window 'reader-fetch-comments-0'
    $null = Wait-RecoveryDiscussion $item 3 'Complete' $false
    $completed = Read-RecoveryFixture $Profile 'snapshot' 'reader-recovery-completed.json'
    Confirm-RecoveryState $original $completed '00000000000000005101,00000000000000005102,00000000000000005103' $true
    if ($completed.discussion.coverage -ne 2 -or $null -ne $completed.discussion.cursor -or
        $null -ne $completed.discussion.error -or $completed.discussion.last_success -le $repaired.discussion.last_page) {
        throw 'The retried saved page did not advance the successful discussion checkpoint.'
    }
    Add-Evidence 'Resume the exact uncommitted page once and publish three stable comments' $completed
    Stop-ThroughUi
    $script:report.detail = 'The native reader repaired one expired continuation, preserved unrelated checkpoints and local work across a returned-page rollback, then completed the saved page after a separate-process restart.'
}
