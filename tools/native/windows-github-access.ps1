function Wait-Account([string]$Description, [scriptblock]$AccountCheck) {
    return Wait-State $Description {
        param($state)
        $account = $state.PSObject.Properties['account_access']
        $null -ne $account -and $null -ne $account.Value -and !$account.Value.busy -and (& $AccountCheck $account.Value)
    }
}

function Test-FixtureTokenFocus($Snapshot, [int]$Owner) {
    if ($null -eq $Snapshot -or $Owner -le 0) { return $false }
    $pidProperty = $Snapshot.PSObject.Properties['pid']
    $account = $Snapshot.PSObject.Properties['account_access']
    if ($null -eq $pidProperty -or $pidProperty.Value -ne $Owner -or $null -eq $account -or $null -eq $account.Value) {
        return $false
    }
    return $account.Value.panel_open -and !$account.Value.busy -and
        $account.Value.fixture_route_entered -and !$account.Value.fixture_route_focused -and
        $account.Value.fixture_token_focused
}

function Wait-SeededFixtureAccount {
    if (!$script:UseSeededFixtureAuth) { throw 'This case must use normal native credential entry.' }
    $observed = Wait-Account 'the authorized synthetic fixture and selected repository' {
        param($account)
        $account.active_user_id -eq 41 -and $account.active_login -ceq 'fixture-user' -and
            $account.profile_access -ceq 'Readable' -and $account.repository_count -eq 1 -and
            $account.selected_count -eq 1
    }
    Add-Evidence 'Load the isolated verified account and scope' ([ordered]@{
        user = $observed.account_access.active_user_id
        repository_count = $observed.account_access.selected_count
    })
}

function Wait-AccountControl([string]$Id) {
    return Wait-State ('account control ' + $Id) {
        param($snapshot)
        $property = $snapshot.bounds.PSObject.Properties[$Id]
        if ($null -eq $property) { return $false }
        $rect = $property.Value
        return $rect.Count -eq 4 -and $rect[2] -gt 0 -and $rect[3] -gt 0
    }
}

function Click-AccountControl([IntPtr]$Window, [string]$Id, [Diagnostics.Stopwatch]$InputClock, [hashtable]$InputTiming, [string]$ExpectedName) {
    $state = Wait-AccountControl $Id
    if ($script:UseOwnedUiAutomation) {
        if ($Id -cnotin @('account-token', 'account-repository')) {
            # GPUI observations can publish before Windows receives the matching accessibility tree.
            for ($attempt = 0; $attempt -lt 12; $attempt++) {
                if (Invoke-OwnedFixtureControl $Window $Id $InputClock -InputTiming $InputTiming -ExpectedName $ExpectedName) { return }
                Start-Sleep -Milliseconds 50
            }
        }
        if ((Get-Variable ReaderProof -ValueOnly -ErrorAction SilentlyContinue) -eq 'PerformanceConcurrent') {
            Stop-Blocked 'The concurrent benchmark needs the exact native UI Automation action for a matched input source.'
        }
        if (!(Test-OwnedFixtureWindow $Window)) { Stop-Blocked 'The physical fallback lost the owned Threadline window.' }
        if ([ThreadlineWinInput]::GetForegroundWindow() -ne $Window) { Set-WindowFocus $Window }
        [ThreadlineWinInput]::TargetWindow = $Window
        $state = Wait-AccountControl $Id
        Add-Evidence 'Use the guarded physical fallback for an owned control' ([ordered]@{ automation_id = $Id })
    }
    $bounds = $state.bounds.PSObject.Properties[$Id].Value
    $center = To-ClientPoint ($bounds[0] + $bounds[2] / 2) ($bounds[1] + $bounds[3] / 2) $state.scale_factor
    if ($null -ne $InputClock) { $InputClock.Restart() }
    if ($InputTiming) { $InputTiming.begin = [Diagnostics.Stopwatch]::GetTimestamp() }
    [ThreadlineWinInput]::ClickClient($Window, $center, [uint32]$script:app.Id, [uint32][Math]::Round(96 * $state.scale_factor))
    if ($InputTiming) {
        $InputTiming.returned = [Diagnostics.Stopwatch]::GetTimestamp()
        $InputTiming.method = 'Win32 Click'
    }
}

function Set-AccessPhase([string]$Phase) {
    if ($Phase -cnotin @('ready', 'partial', 'sso', 'denied', 'revoked', 'reader-failure', 'reader-resume', 'historical-local', 'historical-remote', 'scope-hold-online',
        'sync-live', 'sync-offline', 'sync-service', 'sync-throttle', 'sync-transport', 'sync-notification-send',
        'sync-rate-ready', 'sync-rate-throttle', 'sync-rate-floor',
        'backlog-auth', 'backlog-hold', 'backlog-drain', 'backlog-concurrent',
        'mixed-before', 'mixed-offline', 'mixed-after', 'pr-load', 'pr-merge-state', 'pr-pin-load', 'pr-pin-offline', 'pr-pin-interrupt', 'pr-pin-continue', 'pr-delay', 'pr-release', 'pr-shared-ready', 'pr-shared-hold', 'pr-shared-release', 'review-send', 'review-changed', 'review-decisions', 'review-predecessor', 'review-resume', 'thread-queue', 'thread-outdated',
        'merge-prepare', 'merge-send', 'merge-wrong-head', 'merge-queue-preflight', 'merge-queue-rejected',
        'merge-cancel-hold', 'merge-cancel-read', 'merge-cancel-finish',
        'history', 'history-moved', 'history-deleted', 'write-online', 'write-drop',
         'issue-edit-baseline', 'issue-edit-remote-change', 'issue-edit-commit',
        'credential-ready', 'credential-offline', 'credential-other', 'credential-return',
        'lifecycle-offline', 'lifecycle-ready', 'lifecycle-delay', 'lifecycle-crash',
        'lifecycle-recovery', 'lifecycle-signed-out', 'lifecycle-return', 'activity-baseline', 'activity-fresh',
        'activity-offline', 'activity-catchup', 'activity-replay',
        'comment-dependency-offline', 'comment-dependency-create', 'comment-dependency-resume',
        'comment-dependency-lost', 'thread-placement', 'recover-baseline', 'recover-offline',
        'recover-expired', 'recover-rebuild', 'recover-resume')) {
        throw 'Unknown access fixture phase.'
    }
    try {
        if ($Phase -ceq 'mixed-offline') {
            if ($ReaderProof -cne 'MixedCatchup' -or !$script:app.HasExited) {
                throw 'Mixed provider edits require the original fixture process to have exited.'
            }
            $path = Join-Path ([IO.Path]::GetDirectoryName($script:AccessPhaseFile)) 'mixed-edit-time.txt'
            $stamp = [DateTime]::UtcNow.ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'",[Globalization.CultureInfo]::InvariantCulture)
            $stream = [IO.File]::Open($path,[IO.FileMode]::CreateNew)
            try {
                $bytes = [Text.Encoding]::ASCII.GetBytes($stamp)
                $stream.Write($bytes,0,$bytes.Length)
            } finally { $stream.Dispose() }
        }
        [IO.File]::WriteAllText($script:AccessPhaseFile, $Phase, [Text.Encoding]::ASCII)
    } catch {
        throw 'Could not advance the isolated access fixture phase.'
    }
}

function Open-FixtureSettings([IntPtr]$Window) {
    Click-AccountControl $Window 'account-open' -ExpectedName 'Settings'
    $null = Wait-Account 'the Settings screen' { param($account) $account.panel_open }
}

function Open-FixtureHome([IntPtr]$Window) {
    Click-AccountControl $Window 'account-close' -ExpectedName 'Home'
    $null = Wait-Account 'the Home workspace' { param($account) !$account.panel_open }
}

function Enter-FixtureAccountForm([IntPtr]$Window, [switch]$LoopbackTokenAction) {
    Click-AccountControl $Window 'account-use-pat' -ExpectedName 'Use a personal access token'
    if ($LoopbackTokenAction) {
        $port = [regex]::Match($script:AccessAddress, '^127\.0\.0\.1:([1-9][0-9]{0,4})$')
        if (!$script:UseOwnedUiAutomation -or !$port.Success -or [int]$port.Groups[1].Value -gt 65535 -or
            $ReaderProof -cnotin @('CredentialBoundary', 'NativeLifecycle') -or
            ($ReaderProof -ceq 'NativeLifecycle' -and [IO.File]::ReadAllText($script:AccessPhaseFile).Trim() -cne 'lifecycle-return')) {
            throw 'The synthetic fill action requires an isolated native credential scenario.'
        }
        Wait-For 'the owned repository field in the isolated credential scenario' {
            Set-OwnedFixtureRepository $Window
        }
    } elseif (!$script:UseOwnedUiAutomation -or !(Set-OwnedFixtureRepository $Window)) {
        if ($script:UseOwnedUiAutomation) { Set-WindowFocus $Window }
        Click-AccountControl $Window 'account-repository'
        $null = Wait-Account 'focus in the native repository field' {
            param($account) $account.panel_open -and $account.fixture_route_focused
        }
        [ThreadlineWinInput]::ReplaceFixtureText($Window, [uint32]$script:app.Id, 'fixture-org/private')
    }
    $null = Wait-Account 'the exact synthetic repository in the native form' {
        param($account) $account.fixture_route_entered
    }
    if ($LoopbackTokenAction) {
        Assert-OwnedMaskedFixtureField $Window
        Wait-For 'the owned synthetic credential setup action' {
            Invoke-OwnedFixtureControl $Window 'account-fill-synthetic-token' $null 'Fill synthetic test credential'
        }
    } else {
        if ($script:UseOwnedUiAutomation) {
            Assert-OwnedMaskedFixtureField $Window
            if ([ThreadlineWinInput]::GetForegroundWindow() -ne $Window) { Set-WindowFocus $Window }
            [ThreadlineWinInput]::TargetWindow = $Window
            $form = Wait-Account 'focus in the owned native credential form' {
                param($account)
                $account.panel_open -and $account.fixture_route_entered -and
                    ($account.fixture_route_focused -or $account.fixture_token_focused)
            }
            $focused = $form.account_access
            if (!$focused.panel_open -or !$focused.fixture_route_entered -or
                (!$focused.fixture_route_focused -and !$focused.fixture_token_focused)) {
                Stop-Blocked 'The owned credential form changed before keyboard focus navigation.'
            }
            if (!$focused.fixture_token_focused) {
                [ThreadlineWinInput]::Chord(@(0x09))
                Add-Evidence 'Move focus from the verified repository field to the masked field' ([ordered]@{
                    automation_id = 'account-token'; input = 'Guarded Tab'
                })
            }
        } else {
            Click-AccountControl $Window 'account-token'
        }
        $null = Wait-Account 'focus in the masked native token field' {
            param($account) $account.panel_open -and $account.fixture_token_focused
        }
        $uiaFocus = if ($script:UseOwnedUiAutomation) {
            Assert-OwnedMaskedFixtureField $Window -ObserveFocus
        } else { 'not-used' }
        if (!(Test-FixtureTokenFocus (Read-Observation) $script:app.Id)) {
            Stop-Blocked 'The isolated app no longer reports focus in the exact masked editor.'
        }
        [ThreadlineWinInput]::ReplaceFixtureText($Window, [uint32]$script:app.Id, 'threadline-test-fixture')
        Add-Evidence 'Type the synthetic credential in the owned masked editor' ([ordered]@{
            automation_id = 'account-token'; native_editor_focused = $true; uia_focus = $uiaFocus
        })
    }
    $null = Wait-Account 'the exact synthetic credential entered into the masked native form' {
        param($account) $account.fixture_route_entered -and $account.fixture_token_entered
    }
    Add-Evidence 'Verify the synthetic settings reached the native authorization form' ([ordered]@{
        repository_value_confirmed = $true
        synthetic_credential_confirmed = $true
        input = if ($LoopbackTokenAction) { 'Fixture-only owned setup action' } else { 'Guarded keyboard' }
    })
}

function Test-FixtureAccountRouteCorrection([IntPtr]$Window) {
    $repository = 'account-repository'
    $name = 'GitHub repository'
    $message = 'Enter a repository as owner/repository, for example octocat/hello-world.'

    Wait-For 'the owned repository editor to accept the synthetic owner alone' {
        Set-OwnedFixtureEditValue $Window $repository $name 'fixture-org'
    }
    Wait-For 'the owner alone to appear in the repository editor' {
        $edit = Get-OwnedFixtureEditPattern $Window $repository $name
        $null -ne $edit -and $edit.Current.Value -ceq 'fixture-org'
    }
    Wait-For 'Check access on the owned account form' {
        Invoke-OwnedFixtureControl $Window 'account-connect' -ExpectedName 'Check access'
    }
    Wait-For 'visible inline advice to enter both the owner and repository' {
        $null -ne (Find-OwnedFixtureControl $Window 'account-repository-error' $message)
    }
    Assert-OwnedMaskedFixtureField $Window
    $null = Wait-Account 'the synthetic token to remain in the masked editor after local validation' {
        param($account)
        $account.panel_open -and !$account.fixture_route_entered -and $account.fixture_token_entered -and !$account.active_user_id
    }
    Add-Evidence 'Check a username alone and keep the masked credential for correction' ([ordered]@{
        route_error = $message
        password_field = $true
        readable_value_pattern = $false
        synthetic_credential_retained = $true
    })
    Wait-For 'the same owned editor to accept a full owner/repository' {
        Set-OwnedFixtureEditValue $Window $repository $name 'fixture-org/private'
    }
    $null = Wait-Account 'the corrected route and the original synthetic credential' {
        param($account) $account.fixture_route_entered -and $account.fixture_token_entered
    }
}

function Connect-FixtureAccount([IntPtr]$Window, [switch]$LoopbackTokenAction, [switch]$VerifyRouteCorrection) {
    Click-AccountControl $Window 'display-timezone-new-york' -ExpectedName 'New York (Eastern Time)'
    Click-AccountControl $Window 'onboarding-zone-continue' -ExpectedName 'Continue to GitHub'
    Enter-FixtureAccountForm $Window -LoopbackTokenAction:$LoopbackTokenAction
    if ($VerifyRouteCorrection) { Test-FixtureAccountRouteCorrection $Window }
    Click-AccountControl $Window 'account-connect'
    $identity = Wait-Account 'verified fixture identity and repository access' {
        param($account)
        $account.active_user_id -eq 41 -and $account.active_login -ceq 'fixture-user' -and
            $account.profile_access -ceq 'Readable' -and $account.repository_count -eq 1
    }
    Add-Evidence 'Connect through native repository and password controls' ([ordered]@{
        active_user_id = $identity.account_access.active_user_id
        active_login = $identity.account_access.active_login
        access = $identity.account_access.profile_access
        credential = 'synthetic, entry omitted'
    })

    if ($identity.account_access.selected_count -eq 0) {
        Click-AccountControl $Window 'account-select-0'
    }
    $null = Wait-Account 'repository selected in the native account panel' {
        param($account) $account.selected_count -eq 1 -and $account.profile_access -ceq 'Readable'
    }
    Click-AccountControl $Window 'onboarding-continue' -ExpectedName 'Continue to Home'
    $null = Wait-Account 'a verified account and monitored repository on Home' {
        param($account) !$account.panel_open -and $account.active_user_id -eq 41 -and $account.selected_count -eq 1
    }
    Add-Evidence 'Complete Welcome and reach Home with the verified repository' 1
}

function Invoke-GitHubAccessProof([IntPtr]$Window, [string]$Profile) {
    Connect-FixtureAccount $Window -VerifyRouteCorrection
    Wait-OwnedFixtureLabel $Window 'home-repository-summary' 'Monitoring 1 of 1 repositories'
    Click-AccountControl $Window 'home-refresh-all' -ExpectedName 'Refresh all'
    Wait-OwnedFixtureLabel $Window 'home-refresh-status' 'GitHub repositories refreshed.'
    Add-Evidence 'Refresh the monitored repository from Home' 'GitHub repositories refreshed.'
    Open-FixtureSettings $Window
    $zoneLabel = 'Display time zone ' + [char]0xb7 + ' America/New_York'
    Wait-OwnedFixtureLabel $Window 'display-timezone-effective' $zoneLabel
    Add-Evidence 'Read the time zone selected during Welcome from Settings' 'America/New_York'
    Set-AccessPhase 'partial'
    Click-AccountControl $Window 'account-refresh'
    $null = Wait-Account 'partial SSO repository discovery with retained direct access' {
        param($account) $account.discovery -ceq 'RestrictedBySso' -and $account.profile_access -ceq 'Readable' -and $account.selected_count -eq 1
    }
    Add-Evidence 'Show incomplete discovery while retaining direct read access' 'RestrictedBySso'

    Set-AccessPhase 'sso'
    Click-AccountControl $Window 'account-refresh'
    $null = Wait-Account 'SSO authorization required for the selected repository' {
        param($account) $account.profile_access -ceq 'SsoRequired' -and $account.authorization_available -and $account.selected_count -eq 1
    }
    Add-Evidence 'Offer a validated SSO authorization action' 'SsoRequired'

    Set-AccessPhase 'denied'
    Click-AccountControl $Window 'account-refresh'
    $null = Wait-Account 'insufficient permission without changing the selected repository' {
        param($account) $account.profile_access -ceq 'InsufficientPermission' -and !$account.authorization_available -and $account.selected_count -eq 1
    }
    Add-Evidence 'Show insufficient permission separately from SSO' 'InsufficientPermission'

    Set-AccessPhase 'revoked'
    Click-AccountControl $Window 'account-refresh'
    $null = Wait-Account 'revoked credential with retained selected repository' {
        param($account) $account.profile_access -ceq 'Unauthorized' -and $account.selected_count -eq 1
    }
    Add-Evidence 'Keep the selected repository on token revocation' 'Unauthorized'

    Stop-ThroughUi
    $script:app.Dispose()
    $script:app = $null
    $before = Get-IsolatedSqlite $Profile
    $script:reportFile = Join-Path $script:ReportDir 'github-access-restart-observation.json'
    $script:app = Start-Threadline $Profile $script:reportFile
    $script:report.restarted_pid = $script:app.Id
    $restarted = Get-ReadyWindow
    if (!$script:UseOwnedUiAutomation) { Set-WindowFocus $restarted }
    Open-FixtureSettings $restarted
    $null = Wait-Account 'readable same-profile selection restored after Quit' {
        param($account) $account.active_user_id -eq 41 -and $account.selected_count -eq 1
    }
    Stop-ThroughUi
    $after = Get-IsolatedSqlite $Profile
    if ($before.creation_time_ticks -ne $after.creation_time_ticks) {
        throw 'The isolated account database changed identity during restart.'
    }
    Add-Evidence 'Restart with the same isolated profile and retained selection' ([ordered]@{
        pid = $script:report.restarted_pid
        sqlite_header = $true
        sqlite_retained = $true
        selected_count = 1
    })
}
