function Test-SyntheticActiveOfflineAccount([Diagnostics.Process]$Child, [long]$Account) {
    $window = Get-OwnedWindow $Child
    $identity = Find-OwnedId $window $Child.Id "account-switch-$Account" -AllowDisabled
    $offline = Find-OwnedId $window $Child.Id 'account-offline'
    return $null -ne $identity -and $identity.Current.Name -ceq 'fixture-user (active)' -and
        !$identity.Current.IsEnabled -and $null -ne $offline -and $offline.Current.Name -ceq 'Go online'
}

function Confirm-ClosedInstalledSignOut([long]$Account, [long]$PreviousGeneration) {
    Assert-ClosedFixture $data
    $closedHash = Get-Digest (Join-Path $data 'threadline.sqlite3')
    $inspectionRoot = Join-Path $temp ('threadline-e2e-' + [guid]::NewGuid().ToString('N'))
    $inspectionProfile = Join-Path $inspectionRoot ('initial-' + $nonce)
    $null = New-Item -ItemType Directory -Path $inspectionRoot
    $null = New-Item -ItemType Directory -Path $inspectionProfile
    [IO.File]::WriteAllText((Join-Path $inspectionRoot 'access-fixture-phase.txt'),'lifecycle-offline')
    foreach ($file in @(Get-ChildItem -LiteralPath $data -Force)) {
        [IO.File]::Copy($file.FullName, (Join-Path $inspectionProfile $file.Name), $false)
    }
    Assert-ClosedFixture $inspectionProfile
    if ((Get-Digest (Join-Path $inspectionProfile 'threadline.sqlite3')) -cne $closedHash) {
        throw 'The copied closed installed fixture differs.'
    }
    $signedOut = Invoke-Fixture 'inspect' 'signed-out-closed-before-restart' $inspectionProfile
    if ($signedOut.active_account -ne $Account -or $signedOut.access -cne 'SignedOut' -or
        $signedOut.credential_generation -ne ($PreviousGeneration + 1) -or !$signedOut.protected_reference_matches -or
        $signedOut.protected_token_present) {
        throw 'The closed installed workspace did not contain the exact signed-out account and next credential generation.'
    }
    # The pinned probe maps an absent owned CredRead value to CredentialUnavailable and any different value to ProtectedStore.
    $presence = Invoke-Fixture 'probe' 'signed-out-credential-before-restart' $inspectionProfile
    if ($presence.provider_probe -cne 'CredentialUnavailable') { throw 'The exact generated protected key is not absent.' }
    return $closedHash
}

function Invoke-InstalledDraftSignOut([long]$Account, [long]$PreviousGeneration) {
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
    . (Join-Path $PSScriptRoot 'installed-controls.ps1')
    $result = [ordered]@{ sign_out_pid = $null; restart_pid = $null; original_active_identity = $false
        sign_out_invoked = $false; closed_sign_out_database_sha256 = $null
        closed_account_signed_out = $false; fixture_key_absent_before_restart = $false; cached_panes_after_sign_out = $false
        original_cached_panes_after_restart = $false; paused_and_offline_after_restart = $false
        orderly_sign_out_exit = $false; orderly_restart_exit = $false; loopback_connection = $false }
    $guard = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    $pending = $null
    try {
        $guard.Start()
        $pending = $guard.BeginAcceptTcpClient($null, $null)
        foreach ($restart in @($false, $true)) {
            $child = Start-InstalledDraft $guard
            try {
                if (!$restart) { $result.sign_out_pid = $child.Id } else { $result.restart_pid = $child.Id }
                if ($result.restart_pid -eq $result.sign_out_pid) { throw 'Expected a distinct installed restart process.' }
                $panes = Wait-InstalledDraft $child 'the original active issue and review panes' { Read-InstalledDraftPanes $child } -ReturnValue
                if (!$restart) {
                    $before = $panes
                    Wait-InstalledDraft $child 'the installed Settings screen' { Invoke-OwnedId $child 'account-open' 'Settings' }
                    Wait-InstalledDraft $child 'the exact active synthetic account while offline' {
                        (Test-SyntheticActiveOfflineAccount $child $Account) -and
                            $null -ne (Find-OwnedId (Get-OwnedWindow $child) $child.Id 'account-sign-out')
                    }
                    $result.original_active_identity = $true
                    if (!(Invoke-OwnedId $child 'account-sign-out')) { throw 'The verified synthetic account did not support Sign out.' }
                    $result.sign_out_invoked = $true
                    Wait-InstalledDraft $child 'the updated original offline account controls' { Test-SyntheticActiveOfflineAccount $child $Account }
                    Wait-InstalledDraft $child 'return to the installed Home workspace' { Invoke-OwnedId $child 'account-close' 'Home' }
                } else {
                    if ($panes.left -cne $before.left -or $panes.right -cne $before.right) {
                        throw 'The offline restart changed the active cached items.'
                    }
                    Wait-InstalledDraft $child 'the restarted Synchronization settings' { Invoke-OwnedId $child 'reader-sync-settings' }
                    Wait-InstalledDraft $child 'the saved offline and two pause controls' {
                        (Test-InstalledDraftDial $child 'sync-work-offline' 'Work offline' $true) -and
                            (Test-InstalledDraftDial $child 'sync-pause-background' 'Pause background refresh' $true) -and
                            (Test-InstalledDraftDial $child 'sync-pause-outbox' 'Pause queued actions' $true)
                    }
                    $result.paused_and_offline_after_restart = $true
                    Wait-InstalledDraft $child 'the owned return to the cached workspace' { Invoke-OwnedId $child 'reader-browse' }
                }
                Wait-InstalledDraft $child 'the same locally available issue and review' {
                    $after = Read-InstalledDraftPanes $child
                    return $null -ne $after -and $after.left -ceq $before.left -and $after.right -ceq $before.right
                }
                if ($restart) { $result.original_cached_panes_after_restart = $true } else { $result.cached_panes_after_sign_out = $true }
                Close-InstalledDraft $child
                if ($restart) {
                    $result.orderly_restart_exit = $true
                } else {
                    $result.orderly_sign_out_exit = $true
                    $result.closed_sign_out_database_sha256 = Confirm-ClosedInstalledSignOut $Account $PreviousGeneration
                    $result.closed_account_signed_out = $true
                    $result.fixture_key_absent_before_restart = $true
                }
                if ($pending.IsCompleted) { throw 'An installed offline sign-out process attempted a connection.' }
            } finally {
                if (!$child.HasExited -and $null -ne (Get-OwnedWindow $child)) {
                    [void]$child.CloseMainWindow()
                    [void]$child.WaitForExit(10000)
                }
                $child.Dispose()
            }
        }
    } finally {
        try {
            $result.loopback_connection = $null -ne $pending -and $pending.IsCompleted
            if ($result.loopback_connection) {
                $unexpected = $guard.EndAcceptTcpClient($pending)
                $unexpected.Dispose()
            }
        } finally {
            $guard.Stop()
            $result | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $ReportDirectory 'signed-out-offline.json') -Encoding UTF8
        }
    }
    return $result
}
