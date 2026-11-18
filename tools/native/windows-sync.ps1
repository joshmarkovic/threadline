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
    Set-AccessPhase 'sync-live'
    Connect-FixtureAccount $Window
    Click-AccountControl $Window 'account-close'
    $null = Wait-Sync 'actual GitHub availability and both scheduled resource families' {
        param($sync) $sync.connection -ceq 'Reachable' -and $sync.cache_generation -ge 2
    }
    Add-Evidence 'Observe authenticated issue and comment updates in the native reader' 'Reachable'

    Open-SyncSettings $Window
    Click-AccountControl $Window 'sync-timezone'
    [ThreadlineWinInput]::Chord(@(0x11, 0x41))
    [ThreadlineWinInput]::TypeText('UTC')
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
    Connect-FixtureAccount $restarted
    Click-AccountControl $restarted 'account-close'
    Open-SyncSettings $restarted

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
