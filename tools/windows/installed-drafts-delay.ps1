function Wait-InstalledDraft([Diagnostics.Process]$Child, [string]$Description, [scriptblock]$Check,
    [int]$Milliseconds = 15000, [switch]$ReturnValue) {
    $watch = [Diagnostics.Stopwatch]::StartNew()
    while ($watch.ElapsedMilliseconds -lt $Milliseconds) {
        if ($Child.HasExited) { throw "The installed fixture exited before $Description." }
        try {
            $value = & $Check
            if ($value) {
                if ($ReturnValue) { return $value }
                return
            }
        } catch [Windows.Automation.ElementNotAvailableException] { }
        Start-Sleep -Milliseconds 60
    }
    throw "The installed fixture did not reach $Description."
}

function Start-InstalledDraft([Net.Sockets.TcpListener]$Listener) {
    Assert-ClosedFixture $data
    if ((Get-Digest $exe) -ine $InstalledSha256) { throw 'The installed fixture changed.' }
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $exe
    $start.WorkingDirectory = $installed
    $start.UseShellExecute = $false
    $proxy = 'http://127.0.0.1:' + $Listener.LocalEndpoint.Port
    foreach ($name in @($start.EnvironmentVariables.Keys)) {
        if ($name -like 'THREADLINE_*' -or $name -in @('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','NO_PROXY')) {
            $start.EnvironmentVariables.Remove($name)
        }
    }
    foreach ($name in @('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY')) { $start.EnvironmentVariables[$name] = $proxy }
    $start.EnvironmentVariables['NO_PROXY'] = ''
    return [Diagnostics.Process]::Start($start)
}

function Close-InstalledDraft([Diagnostics.Process]$Child) {
    if ($null -eq (Get-OwnedWindow $Child) -or !$Child.CloseMainWindow() -or
        !$Child.WaitForExit(10000) -or $Child.ExitCode -ne 0) {
        throw 'The owned installed fixture did not close normally.'
    }
    Assert-ClosedFixture $data
}

function Read-ActiveDraftTab([Diagnostics.Process]$child, $window, [string]$Id) {
    $tab = Find-OwnedId $window $child.Id $Id
    if ($tab -and $tab.Current.Name.EndsWith((' ' + [char]0x00b7 + ' Active tab'), [StringComparison]::Ordinal)) {
        return $tab.Current.Name
    }
    return $null
}

function Read-InstalledDraftPanes([Diagnostics.Process]$child) {
    $window = Get-OwnedWindow $child
    $issue = Read-ActiveDraftTab $child $window 'reader-tab-0-0'
    $review = Read-ActiveDraftTab $child $window 'reader-tab-1-0'
    if ($issue -and $issue.StartsWith('Issue ', [StringComparison]::Ordinal) -and
        $review -and $review.StartsWith('Pull request ', [StringComparison]::Ordinal) -and
        (Find-OwnedId $window $child.Id 'reader-draft-comment-0') -and
        (Find-OwnedId $window $child.Id 'pr-1-file-0')) {
        return [pscustomobject]@{ left = $issue; right = $review }
    }
    return $null
}

function Test-InstalledDraftDial([Diagnostics.Process]$child, [string]$Id, [string]$Label, [bool]$On) {
    $node = Find-OwnedId (Get-OwnedWindow $child) $child.Id $Id
    $suffix = if ($On) { 'On' } else { 'Off' }
    return $null -ne $node -and $node.Current.Name -ceq ($Label + ' ' + [char]0x00b7 + ' ' + $suffix)
}

function Test-InstalledDraftSocket([Diagnostics.Process]$Child, [Net.Sockets.TcpClient]$Client) {
    if ($Child.HasExited) { return $false }
    $origin = $Client.Client.RemoteEndPoint
    $listener = $Client.Client.LocalEndPoint
    if (!$origin.Address.Equals([Net.IPAddress]::Loopback) -or !$listener.Address.Equals([Net.IPAddress]::Loopback)) {
        return $false
    }
    $matching = @(Get-NetTCPConnection -LocalAddress '127.0.0.1' -RemoteAddress '127.0.0.1' `
        -LocalPort $origin.Port -RemotePort $listener.Port -OwningProcess $Child.Id -State Established -ErrorAction SilentlyContinue)
    return $matching.Count -eq 1
}

function Test-InstalledGitHubConnect([Net.Sockets.NetworkStream]$Stream) {
    $expected = [Text.Encoding]::ASCII.GetBytes("CONNECT api.github.com:443 HTTP/1.1`r`n")
    $watch = [Diagnostics.Stopwatch]::StartNew()
    try {
        foreach ($octet in $expected) {
            $left = 1000 - $watch.ElapsedMilliseconds
            if ($left -le 0) { return $false }
            $Stream.ReadTimeout = [int]$left
            if ($Stream.ReadByte() -ne $octet) { return $false }
        }
    } catch [IO.IOException] { return $false }
    return $true
}

function Invoke-InstalledDraftDelay {
    $null = Get-Command Get-NetTCPConnection -ErrorAction Stop
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
    . (Join-Path $PSScriptRoot 'installed-controls.ps1')
    $result = [ordered]@{ offline_no_connection = $false; pause_background = $false; pause_outbox = $false
        offline_pid = $null; orderly_offline_exit = $false; online_pid = $null; online_connect = $false; owned_connect = $false
        online_pause_background = $false; online_pause_outbox = $false; online_work_offline_off = $false; restored_after_settings = $false
        matching_cached_panes_while_held = $false; orderly_online_exit = $false; forwarded = $false; provider_response = $false }
    foreach ($online in @($false,$true)) {
        $proxy = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
        $child = $null
        $held = $null
        try {
            $proxy.Start()
            $child = Start-InstalledDraft $proxy
            if (!$online) {
                $result.offline_pid = $child.Id
                foreach ($id in @('reader-tab-0-0','reader-tab-1-0')) {
                    Wait-InstalledDraft $child 'activation of the original cached tab' {
                        if (Read-ActiveDraftTab $child (Get-OwnedWindow $child) $id) { return $true }
                        [void](Invoke-OwnedId $child $id)
                        return $false
                    }
                }
                $before = Wait-InstalledDraft $child 'the original issue and review panes' { Read-InstalledDraftPanes $child } -ReturnValue
                Open-InstalledSyncSettings $child
                Wait-InstalledDraft $child 'the three original offline settings' {
                    (Test-InstalledDraftDial $child 'sync-work-offline' 'Work offline' $true) -and
                    (Test-InstalledDraftDial $child 'sync-pause-background' 'Pause background refresh' $true) -and
                    (Test-InstalledDraftDial $child 'sync-pause-outbox' 'Pause queued actions' $true)
                }
                $result.pause_background = $true
                $result.pause_outbox = $true
                $result.offline_no_connection = !$proxy.Pending()
                if (!$result.offline_no_connection) { throw 'The offline installed fixture attempted a connection.' }
                Wait-InstalledDraft $child 'the owned Work offline control' { Invoke-OwnedId $child 'sync-work-offline' }
                Wait-InstalledDraft $child 'the saved online setting with both queues paused' {
                    (Test-InstalledDraftDial $child 'sync-work-offline' 'Work offline' $false) -and
                    (Test-InstalledDraftDial $child 'sync-pause-background' 'Pause background refresh' $true) -and
                    (Test-InstalledDraftDial $child 'sync-pause-outbox' 'Pause queued actions' $true)
                }
            } else {
                $result.online_pid = $child.Id
                if ($result.online_pid -eq $result.offline_pid) { throw 'Expected a distinct installed process.' }
                Wait-InstalledDraft $child 'the loopback connection from normal online startup' { $proxy.Pending() }
                $held = $proxy.AcceptTcpClient()
                $result.owned_connect = Test-InstalledDraftSocket $child $held
                if (!$result.owned_connect) { throw 'The loopback connection did not belong to the installed fixture.' }
                $result.online_connect = Test-InstalledGitHubConnect ($held.GetStream())
                if (!$result.online_connect) { throw 'The installed fixture did not request the expected GitHub HTTPS tunnel.' }
                $after = Wait-InstalledDraft $child 'the cached issue and review before a provider response' {
                    Read-InstalledDraftPanes $child
                } 4000 -ReturnValue
                $result.matching_cached_panes_while_held = $null -ne $after -and $after.left -ceq $before.left -and
                    $after.right -ceq $before.right -and (Test-InstalledDraftSocket $child $held)
                if (!$result.matching_cached_panes_while_held) { throw 'Both original cached panes did not return while the exact tunnel was held open.' }
                Open-InstalledSyncSettings $child
                Wait-InstalledDraft $child 'both saved pauses and online mode in the restarted process' {
                    $result.online_pause_background = Test-InstalledDraftDial $child 'sync-pause-background' 'Pause background refresh' $true
                    $result.online_pause_outbox = Test-InstalledDraftDial $child 'sync-pause-outbox' 'Pause queued actions' $true
                    $result.online_work_offline_off = Test-InstalledDraftDial $child 'sync-work-offline' 'Work offline' $false
                    return $result.online_pause_background -and $result.online_pause_outbox -and $result.online_work_offline_off
                }
                Wait-InstalledDraft $child 'the owned return to the cached workspace' { Invoke-OwnedId $child 'account-close' 'Home' }
                Wait-InstalledDraft $child 'the same pane selections after opening settings' {
                    $reopened = Read-InstalledDraftPanes $child
                    return $null -ne $reopened -and $reopened.left -ceq $before.left -and $reopened.right -ceq $before.right
                }
                $result.restored_after_settings = $true
            }
            Close-InstalledDraft $child
            if ($online) { $result.orderly_online_exit = $true } else { $result.orderly_offline_exit = $true }
        } finally {
            try {
                if ($null -ne $child) {
                    if (!$child.HasExited -and $null -ne (Get-OwnedWindow $child)) {
                        [void]$child.CloseMainWindow()
                        [void]$child.WaitForExit(10000)
                    }
                    $child.Dispose()
                }
            } finally {
                if ($null -ne $held) { $held.Dispose() }
                $proxy.Stop()
                $result | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $ReportDirectory 'delayed-online.json') -Encoding UTF8
            }
        }
    }
    return $result
}
