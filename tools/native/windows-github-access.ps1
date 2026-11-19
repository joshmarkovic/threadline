function Wait-Account([string]$Description, [scriptblock]$AccountCheck) {
    return Wait-State $Description {
        param($state)
        $account = $state.PSObject.Properties['account_access']
        $null -ne $account -and $null -ne $account.Value -and !$account.Value.busy -and (& $AccountCheck $account.Value)
    }
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

function Click-AccountControl([IntPtr]$Window, [string]$Id, [Diagnostics.Stopwatch]$InputClock, [hashtable]$InputTiming) {
    $state = Wait-AccountControl $Id
    if ($script:UseOwnedUiAutomation) {
        if ($Id -cnotin @('account-token', 'account-repository')) {
            # GPUI observations can publish before Windows receives the matching accessibility tree.
            for ($attempt = 0; $attempt -lt 12; $attempt++) {
                if (Invoke-OwnedFixtureControl $Window $Id $InputClock -InputTiming $InputTiming) { return }
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
    if ($Phase -cnotin @('ready', 'partial', 'sso', 'denied', 'revoked', 'reader-failure', 'reader-resume',
        'sync-live', 'sync-offline', 'sync-service', 'sync-throttle', 'sync-transport', 'sync-notification-send',
        'backlog-auth', 'backlog-hold', 'backlog-drain', 'backlog-concurrent',
        'pr-load', 'pr-delay', 'pr-release', 'review-send', 'review-changed',
        'merge-prepare', 'merge-send', 'merge-wrong-head', 'history', 'write-online', 'write-drop',
        'credential-ready', 'credential-offline', 'credential-other', 'credential-return')) {
        throw 'Unknown access fixture phase.'
    }
    try {
        [IO.File]::WriteAllText($script:AccessPhaseFile, $Phase, [Text.Encoding]::ASCII)
    } catch {
        throw 'Could not advance the isolated access fixture phase.'
    }
}

function Open-AccountFixture {
    if ($script:UseOwnedUiAutomation) {
        Click-AccountControl $script:app.MainWindowHandle 'account-open'
    } else {
        [ThreadlineWinInput]::Chord(@(0x11, 0x10, 0x41))
    }
    return Wait-Account 'open the native account panel' { param($account) $account.panel_open }
}

function Enter-FixtureAccountForm([IntPtr]$Window, [switch]$LoopbackTokenAction) {
    if (!$script:UseOwnedUiAutomation -or !(Set-OwnedFixtureRepository $Window)) {
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
        if (!$script:UseOwnedUiAutomation -or $ReaderProof -cne 'CredentialBoundary') {
            throw 'The synthetic fill action is limited to the scoped credential boundary driver.'
        }
        Assert-OwnedMaskedFixtureField $Window
        Wait-For 'the owned synthetic credential setup action' {
            Invoke-OwnedFixtureControl $Window 'account-fill-synthetic-token' $null 'Fill synthetic test credential'
        }
    } else {
        if ($script:UseOwnedUiAutomation) {
            Assert-OwnedMaskedFixtureField $Window
            if ([ThreadlineWinInput]::GetForegroundWindow() -ne $Window) { Set-WindowFocus $Window }
            [ThreadlineWinInput]::TargetWindow = $Window
        }
        Click-AccountControl $Window 'account-token'
        $null = Wait-Account 'focus in the masked native token field' {
            param($account) $account.panel_open -and $account.fixture_token_focused
        }
        $syntheticPat = 'threadline-test-fixture'
        if ($script:UseOwnedUiAutomation) { Assert-OwnedMaskedFixtureField $Window }
        [ThreadlineWinInput]::ReplaceFixtureText($Window, [uint32]$script:app.Id, $syntheticPat)
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

function Connect-FixtureAccount([IntPtr]$Window, [switch]$FromVisibleControl, [switch]$LoopbackTokenAction) {
    if ($FromVisibleControl) {
        Click-AccountControl $Window 'account-open'
        $null = Wait-Account 'open the native account panel through its visible control' {
            param($account) $account.panel_open
        }
    } else {
        $null = Open-AccountFixture
    }
    Enter-FixtureAccountForm $Window -LoopbackTokenAction:$LoopbackTokenAction
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
    Add-Evidence 'Select the verified repository' 1
}

function Invoke-GitHubAccessProof([IntPtr]$Window, [string]$Profile) {
    Connect-FixtureAccount $Window
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
    $null = Open-AccountFixture
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
