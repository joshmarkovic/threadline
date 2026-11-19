function Read-NativeLifecycle([string]$Profile, [string]$Stage, [string]$Name) {
    if ($script:ReaderProof -cne 'NativeLifecycle') { throw 'The protected fixture is scoped to the separate lifecycle case.' }
    $receipt = Join-Path $script:ReportDir $Name
    if (Test-Path -LiteralPath $receipt) { throw 'Refusing to reuse the protected lifecycle receipt.' }
    $child = Start-Threadline $Profile $receipt -Lifecycle $Stage
    try {
        if (!$child.WaitForExit(15000)) {
            $child.Kill()
            [void]$child.WaitForExit(3000)
            throw 'The isolated lifecycle checkpoint timed out.'
        }
        if ($child.ExitCode -ne 0) { throw "The isolated lifecycle checkpoint '$Stage' exited $($child.ExitCode)." }
    } finally {
        $child.Dispose()
    }
    return [IO.File]::ReadAllText($receipt) | ConvertFrom-Json
}

function Initialize-NativeLifecycle([string]$Profile) {
    Set-AccessPhase 'lifecycle-offline'
    $script:lifecycleSeed = Read-NativeLifecycle $Profile 'seed' 'native-lifecycle-seeded.json'
    if ($lifecycleSeed.fixture_revision -cne 's12-native-lifecycle-v1' -or
        !$lifecycleSeed.work_offline -or !$lifecycleSeed.pause_queued_actions -or !$lifecycleSeed.protected_token_present -or
        ($lifecycleSeed.tab_counts -join ',') -cne '10,10' -or ($lifecycleSeed.actions -join ',') -cne 'Queued,Queued,Queued,Queued' -or
        !$lifecycleSeed.review_anchor_preserved -or $lifecycleSeed.prior_delivery -cne 'Delivered' -or $lifecycleSeed.feed_count -ne 1) {
        throw 'The lifecycle fixture did not seed its exact retained layout, drafts, queue and delivery.'
    }
    Add-Evidence 'Seed mixed private cached work and install one unique protected fixture key' $lifecycleSeed
}

function Test-LifecycleWorkspace($Reader, [long]$Item, [bool]$Offline, [bool]$SignedOut) {
    return $null -ne $Reader -and $Reader.cached_content_ready -and $Reader.cached_visible_panes -eq 2 -and
        $Reader.focused_pane -eq 1 -and $Reader.active_item -eq $Item -and
        ($Reader.tab_counts -join ',') -ceq '10,10' -and $Reader.outbox -eq 4 -and
        $Reader.offline -eq $Offline -and $Reader.signed_out -eq $SignedOut
}

function Test-LifecycleSync($Sync, [bool]$Offline, [bool]$OutboxPaused, [switch]$Reachable) {
    if ($null -eq $Sync) { return $false }
    $saved = $Sync.PSObject.Properties['controls']
    if ($null -eq $saved -or $null -eq $saved.Value) { return $false }
    $controls = $saved.Value
    return $controls.work_offline -eq $Offline -and $controls.pause_background -and
        $controls.pause_queued_actions -eq $OutboxPaused -and
        (!$Reachable -or $Sync.connection -ceq 'Reachable')
}

function Wait-LifecycleWorkspace([bool]$Offline, [bool]$SignedOut) {
    return Wait-Reader 'the established native cached workspace and four retained actions' {
        param($reader)
        Test-LifecycleWorkspace $reader $script:lifecycleSeed.review_item $Offline $SignedOut
    }
}

function Start-LifecycleWindow([string]$Profile, [string]$Observation) {
    if ($null -ne $script:app) {
        if (!$script:app.HasExited) { throw 'The previous owned process is still running.' }
        $script:app.Dispose()
    }
    $script:reportFile = Join-Path $script:ReportDir $Observation
    $script:app = Start-Threadline $Profile $script:reportFile
    $script:report.restarted_pid = $script:app.Id
    return Get-ReadyWindow
}

function Wait-LifecycleMarker([string]$Name) {
    $marker = Join-Path $script:ProfileDir $Name
    Wait-For "the exact held loopback request $Name" {
        (Test-Path -LiteralPath $marker -PathType Leaf) -and [IO.File]::ReadAllText($marker).Trim() -ceq 'held'
    }
}

function Invoke-NativeLifecycleProof([IntPtr]$Window, [string]$Profile) {
    $initial = Wait-LifecycleWorkspace $true $false
    Add-Evidence 'Restore the useful two-pane native workspace with Work Offline and saved credential' ([ordered]@{
        pid = $initial.pid; panes = $initial.cached_reader.cached_visible_panes; outbox = $initial.cached_reader.outbox
    })
    Open-SyncSettings $Window
    $null = Wait-Sync 'separate offline, read and Outbox holds' {
        param($sync) Test-LifecycleSync $sync $true $true
    }
    Set-AccessPhase 'lifecycle-ready'
    Click-AccountControl $Window 'sync-work-offline'
    $null = Wait-Sync 'online mode and both workload pauses committed' {
        param($sync) Test-LifecycleSync $sync $false $true
    }
    Click-AccountControl $Window 'sync-check-now'
    try {
        $null = Wait-Sync 'reachability using the native protected credential while submissions are paused' {
            param($sync) Test-LifecycleSync $sync $false $true -Reachable
        }
    } catch {
        Stop-ThroughUi
        $diagnostic = Read-NativeLifecycle $Profile 'probe' 'native-lifecycle-probe-diagnostic.json'
        $script:report.probe_diagnostic = $diagnostic.provider_probe
        throw
    }
    Stop-ThroughUi

    $retained = Read-NativeLifecycle $Profile 'inspect' 'native-lifecycle-before-delayed-start.json'
    if (!$retained.protected_token_present -or $retained.work_offline -or !$retained.pause_queued_actions -or
        $retained.first_attempts -ne 0 -or $retained.feed_count -ne 1) {
        throw 'Normal close did not preserve the protected token, controls and delivered activity.'
    }
    Set-AccessPhase 'lifecycle-delay'
    $online = Start-LifecycleWindow $Profile 'native-lifecycle-online-observation.json'
    Wait-LifecycleMarker 'lifecycle-probe-held'
    $ready = Wait-LifecycleWorkspace $false $false
    Add-Evidence 'Observe both restored panes and four actions while the authenticated startup response is held' ([ordered]@{
        pid = $ready.pid; cached_panes = $ready.cached_reader.cached_visible_panes
        queued_actions = $ready.cached_reader.outbox; protected_credential_reentered = $false
    })
    Set-AccessPhase 'lifecycle-ready'
    Open-SyncSettings $online
    $null = Wait-Sync 'the released startup probe reaches the actual loopback server' {
        param($sync) Test-LifecycleSync $sync $false $true -Reachable
    }

    Set-AccessPhase 'lifecycle-crash'
    Click-AccountControl $online 'sync-pause-outbox'
    Wait-LifecycleMarker 'lifecycle-comment-held'
    if (!(Test-OwnedFixtureWindow $online)) {
        Stop-Blocked 'The deliberate interruption lost the verified owned fixture window.'
    }
    $interruptedId = $script:app.Id
    $script:app.Kill()
    if (!$script:app.WaitForExit(4000)) { throw 'The owned fixture did not exit after the deliberate interruption.' }
    Set-AccessPhase 'lifecycle-recovery'
    Add-Evidence 'Interrupt only the owned process after the exact first comment reached the server before its reply' $interruptedId

    $interrupted = Read-NativeLifecycle $Profile 'inspect' 'native-lifecycle-after-crash.json'
    if ($interrupted.actions[0] -cne 'Sending' -or $interrupted.first_send_starts -ne 1 -or
        $null -eq $interrupted.next_recovery_at_ms) {
        throw 'The deliberate process interruption did not leave a real send lease in SQLite.'
    }
    $recovery = Start-LifecycleWindow $Profile 'native-lifecycle-recovery-observation.json'
    $null = Wait-LifecycleWorkspace $false $false
    Open-SyncSettings $recovery
    $null = Wait-Sync 'the replacement process reconnects without repeating the uncertain mutation' {
        param($sync) Test-LifecycleSync $sync $false $false -Reachable
    }
    $recoveryDeadline = [long]$interrupted.next_recovery_at_ms
    if ($recoveryDeadline - [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() -gt 180000) {
        throw 'The interrupted operation has an unexpected recovery lease.'
    }
    Add-Evidence 'Wait for normal dispatch lease expiry in the restarted application' $recoveryDeadline
    Wait-For 'the normal interrupted-send lease deadline' {
        [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() -ge ($recoveryDeadline + 1500)
    } 182000 200
    Click-AccountControl $recovery 'reader-outbox'
    Click-AccountControl $recovery 'reader-outbox-filter-attention'
    Wait-For 'the original unresolved comment in the process-owned Outbox' {
        $name = 'Comment ' + [char]0x00b7 + ' Outcome unknown'
        Invoke-OwnedFixtureControl $recovery 'reader-outbox-action-0' $null $name
    }
    $null = Wait-Reader 'the reopened original comment shows an unknown native outcome' {
        param($reader) $reader.outbox_visible -and $reader.outbox_action_state -ceq 'OutcomeUnknown'
    }
    Stop-ThroughUi
    $recovered = Read-NativeLifecycle $Profile 'inspect' 'native-lifecycle-recovered.json'
    if (!$recovered.protected_token_present -or $recovered.actions[0] -cne 'OutcomeUnknown' -or
        $recovered.first_send_starts -ne 1 -or $recovered.feed_count -ne 1 -or
        $recovered.dependents -ne 3) {
        throw 'The interrupted mutation was not durably recovered as uncertain with its dependents and prior delivery.'
    }
    Add-Evidence 'Verify the same durable operation, original inline anchor, dependent intents and one prior delivery on restart' $recovered

    $signOut = Start-LifecycleWindow $Profile 'native-lifecycle-signout-observation.json'
    $null = Wait-LifecycleWorkspace $false $false
    Click-AccountControl $signOut 'account-open'
    Click-AccountControl $signOut 'account-sign-out'
    $null = Wait-Account 'the original private account signed out with its repository selection preserved' {
        param($account)
        $account.active_local_id -eq $script:lifecycleSeed.account -and
            $account.profile_access -ceq 'SignedOut' -and $account.selected_count -eq 2
    }
    Set-AccessPhase 'lifecycle-signed-out'
    Stop-ThroughUi
    $signedOut = Read-NativeLifecycle $Profile 'inspect' 'native-lifecycle-signed-out.json'
    if ($signedOut.protected_token_present -or $signedOut.access -cne 'SignedOut' -or
        $signedOut.first_send_starts -ne 1 -or $signedOut.actions[0] -cne 'OutcomeUnknown') {
        throw 'Native sign-out did not revoke the fixture entry and preserve uncertain local work.'
    }
    Add-Evidence 'Verify that normal sign-out removed the actual protected entry and retained the same mixed queue' $signedOut

    $isolated = Start-LifecycleWindow $Profile 'native-lifecycle-isolation-observation.json'
    $null = Wait-LifecycleWorkspace $false $true
    Click-AccountControl $isolated 'account-open'
    Wait-For 'the exact alternate local account control' {
        Invoke-OwnedFixtureControl $isolated ("account-switch-" + $script:lifecycleSeed.alternate) $null 'fixture-other'
    }
    $null = Wait-Account 'the alternate account has no selected private repositories' {
        param($account) $account.active_local_id -eq $script:lifecycleSeed.alternate -and $account.selected_count -eq 0
    }
    $null = Wait-Reader 'the alternate account does not see the original cache or Outbox' {
        param($reader) $reader.outbox -eq 0 -and $null -eq $reader.active_item
    }
    Wait-For 'switch back to the exact signed-out original account' {
        Invoke-OwnedFixtureControl $isolated ("account-switch-" + $script:lifecycleSeed.account) $null 'fixture-user'
    }
    Click-AccountControl $isolated 'account-close'
    $null = Wait-LifecycleWorkspace $false $true
    Stop-ThroughUi
    $final = Read-NativeLifecycle $Profile 'inspect' 'native-lifecycle-final.json'
    if ($final.protected_token_present -or $final.active_account -ne $lifecycleSeed.account -or
        $final.alternate_queue -ne 0 -or $final.feed_count -ne 1 -or $final.first_send_starts -ne 1 -or
        $final.delivery_groups -ne 1 -or
        $final.delivery_unchanged_at_ms -ne $lifecycleSeed.delivery_unchanged_at_ms) {
        throw 'Account switching did not preserve the isolated durable signed-out state.'
    }
    Add-Evidence 'Restore the original signed-out workspace after visiting the isolated account' $final
    $script:report.detail = 'The mixed workspace restored before an authenticated startup response. An interrupted mutation remained uncertain across restart and sign-out. The protected key was removed, the other profile saw no private work, three dependents and the original draft stayed intact, and the prior delivery was not repeated.'
}
