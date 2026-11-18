function Wait-Account([string]$Description, [scriptblock]$AccountCheck) {
    return Wait-State $Description {
        param($state)
        $null -ne $state.account_access -and !$state.account_access.busy -and (& $AccountCheck $state.account_access)
    }
}

function Click-AccountControl([IntPtr]$Window, [string]$Id) {
    $state = Wait-State ('account control ' + $Id) {
        param($snapshot)
        $property = $snapshot.bounds.PSObject.Properties[$Id]
        if ($null -eq $property) { return $false }
        $rect = $property.Value
        return $rect.Count -eq 4 -and $rect[2] -gt 0 -and $rect[3] -gt 0
    }
    $bounds = $state.bounds.PSObject.Properties[$Id].Value
    $center = To-ClientPoint ($bounds[0] + $bounds[2] / 2) ($bounds[1] + $bounds[3] / 2) $state.scale_factor
    [ThreadlineWinInput]::ClickClient($Window, $center, [uint32]$script:app.Id, [uint32][Math]::Round(96 * $state.scale_factor))
}

function Set-AccessPhase([string]$Phase) {
    if ($Phase -cnotin @('ready', 'partial', 'sso', 'denied', 'revoked')) {
        throw 'Unknown access fixture phase.'
    }
    try {
        [IO.File]::WriteAllText($script:AccessPhaseFile, $Phase, [Text.Encoding]::ASCII)
    } catch {
        throw 'Could not advance the isolated access fixture phase.'
    }
}

function Open-AccountFixture {
    [ThreadlineWinInput]::Chord(@(0x11, 0x10, 0x41))
    return Wait-Account 'open the native account panel' { param($account) $account.panel_open }
}

function Invoke-GitHubAccessProof([IntPtr]$Window, [string]$Profile) {
    $null = Open-AccountFixture
    Click-AccountControl $Window 'account-repository'
    [ThreadlineWinInput]::TypeText('fixture-org/private')
    Click-AccountControl $Window 'account-token'
    $syntheticPat = 'threadline-test-fixture'
    [ThreadlineWinInput]::TypeText($syntheticPat)
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
    Set-WindowFocus $restarted
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
