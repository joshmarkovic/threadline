function Select-FixtureReplacement([IntPtr]$Window, [long]$LocalAccount) {
    if ($LocalAccount -le 0) { throw 'Missing original fixture account identity.' }
    Wait-For 'the owned Replace token action for the original repository owner' {
        Invoke-OwnedFixtureControl $Window "account-renew-$LocalAccount-fixture-org" $null 'Replace token'
    }
    $null = Wait-Account 'the original owner credential replacement form' {
        param($account)
        $account.active_local_id -eq $LocalAccount -and $account.fixture_replacement_active
    }
}

function Invoke-CredentialBoundaryProof([IntPtr]$Window, [string]$Profile) {
    Set-AccessPhase 'credential-ready'
    Connect-FixtureAccount $Window -FromVisibleControl
    $initial = Wait-Account 'the original verified local account' {
        param($account) $account.active_local_id -gt 0 -and $account.active_user_id -eq 41
    }
    [long]$original = $initial.account_access.active_local_id
    Click-AccountControl $Window 'account-close'
    $item = Open-WriteFixtureIssue $Window -UserConnected
    Set-WriteFixtureOffline $Window $true
    Click-AccountControl $Window 'reader-draft-comment-0'
    $text = 'Threadline S07 explicitly queued comment.'
    Type-WriteFixtureField $Window 'reader-draft-input-0' $text
    Wait-WriteFixtureDraft $item $text
    Queue-WriteFixtureDraft $Window $item 1

    Click-AccountControl $Window 'account-open'
    Click-AccountControl $Window 'account-sign-out'
    $null = Wait-Account 'the original account is signed out with its selected repository retained' {
        param($account)
        !$account.busy -and $account.active_local_id -eq $original -and
            $account.profile_access -ceq 'SignedOut' -and $account.selected_count -eq 1
    }
    Set-AccessPhase 'credential-offline'
    $ready = Restart-Reader $Profile
    $null = Wait-Reader 'cached original work and queued operation survive sign-out and a separate offline process' {
        param($reader)
        $reader.active_item -eq $item -and $reader.cached_content_ready -and $reader.signed_out -and
            $reader.offline -and $reader.outbox -eq 1
    }
    Add-Evidence 'Keep the original item and one queued action after sign-out and offline restart' ([ordered]@{
        original_user = 41; selected_repository_retained = $true; outbox = 1; offline = $true
    })

    Click-AccountControl $ready 'account-open'
    Set-AccessPhase 'credential-other'
    Click-AccountControl $ready 'account-offline'
    $null = Wait-Reader 'online mode with no usable original account credential' {
        param($reader) !$reader.offline -and $reader.signed_out -and $reader.outbox -eq 1
    }
    Select-FixtureReplacement $ready $original
    Enter-FixtureAccountForm $ready
    Click-AccountControl $ready 'account-connect'
    $null = Wait-Account 'replacement credential rejected for a different authenticated GitHub user' {
        param($account)
        !$account.busy -and !$account.fixture_token_entered -and $account.fixture_identity_rejected -and
            $account.active_local_id -eq $original -and $account.profile_access -ceq 'SignedOut'
    }
    Add-Evidence 'Reject a replacement credential that authenticates the wrong GitHub user' ([ordered]@{
        original_user = 41; replacement_rejected = $true
    })

    Enter-FixtureAccountForm $ready
    Click-AccountControl $ready 'account-connect'
    $alternate = Wait-Account 'the same repository is authenticated in a separate local account' {
        param($account)
        !$account.busy -and $account.active_local_id -gt 0 -and $account.active_local_id -ne $original -and
            $account.active_user_id -eq 42 -and $account.profile_access -ceq 'Readable'
    }
    if ($alternate.account_access.selected_count -eq 0) {
        Click-AccountControl $ready 'account-select-0'
    }
    Click-AccountControl $ready 'account-close'
    $null = Wait-Reader 'the second account cannot see or dispatch the original account queue' {
        param($reader) !$reader.signed_out -and !$reader.offline -and $reader.outbox -eq 0
    }
    Add-Evidence 'Connect the same repository under a distinct account without carrying the queued action' ([ordered]@{
        alternate_user = 42; outbox = 0
    })

    Click-AccountControl $ready 'account-open'
    Wait-For 'the owned original account switch' {
        Invoke-OwnedFixtureControl $ready "account-switch-$original" $null 'fixture-user'
    }
    $null = Wait-Account 'the original owner is active and still signed out' {
        param($account)
        !$account.busy -and $account.active_local_id -eq $original -and $account.active_user_id -eq 41 -and
            $account.profile_access -ceq 'SignedOut'
    }
    $null = Wait-Reader 'the original account still owns the cached queued action' {
        param($reader) $reader.signed_out -and $reader.outbox -eq 1
    }
    Set-AccessPhase 'credential-return'
    Select-FixtureReplacement $ready $original
    Enter-FixtureAccountForm $ready
    Click-AccountControl $ready 'account-connect'
    $null = Wait-Account 'the renewed credential was verified for the original account and repository' {
        param($account)
        !$account.busy -and $account.active_local_id -eq $original -and $account.active_user_id -eq 41 -and
            $account.profile_access -ceq 'Readable' -and $account.selected_count -eq 1
    }
    Click-AccountControl $ready 'account-close'
    Click-AccountControl $ready 'reader-outbox'
    Click-AccountControl $ready 'reader-outbox-filter-history'
    Inspect-WriteFixtureAction $ready 0 'Confirmed'
    Click-AccountControl $ready 'reader-outbox-open'
    $null = Wait-Reader 'the confirmed action returns to the original cached item' {
        param($reader) $reader.active_item -eq $item -and $reader.outbox -eq 0
    }
    Add-Evidence 'Renew the original credential and confirm the same scoped queued action' ([ordered]@{
        original_user = 41; outbox = 'Confirmed'; alternate_account_sent = $false
    })
    Stop-ThroughUi
    $script:report.detail = 'Sign-out and offline restart retained the original scoped action. Wrong-user renewal was rejected, another account could not see or send it, and the original verified account renewed and confirmed it once.'
}
