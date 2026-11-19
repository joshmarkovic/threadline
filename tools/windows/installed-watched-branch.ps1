# Invoked after verifying the app, helper, registered shortcut, shell identity and owned profile.
function Test-InstalledWatchedBranch {
    $initial = Read-Fixture seed-watched
    $base = 'a' * 40
    $head = 'd' * 40
    if ($initial.branch -cne 'feature/fixture-search' -or $initial.base -cne $base -or $initial.head -cne $head -or
        $initial.events -ne 3 -or $initial.published -ne 0 -or $initial.groups -ne 0 -or $initial.comparison_saved) {
        throw 'The initial monitored branch, observed successors or comparison state did not match.'
    }
    Add-Evidence 'Observe a watched baseline and three proven successor commits with both network queues paused' $initial
    Start-ActivityFixtureProxy
    $script:toastHistory = [Windows.UI.Notifications.ToastNotificationManager,Windows.UI.Notifications,ContentType=WindowsRuntime]::History
    $script:child = Start-ActivityFixtureProcess
    $report.initial_pid = $child.Id
    $delivery = Wait-Activity 'one delivered three-commit group bound to the expected branch and endpoints' {
        if ($child.HasExited) { throw 'The isolated installed app exited before submission.' }
        $state = Read-Fixture inspect-watched
        if ($state.outcome -ceq 'delivered' -and $state.groups -eq 1 -and $state.events -eq 3 -and
            $state.grouped -eq 3 -and $state.published -eq 3 -and $state.native_id -cmatch '^[a-f0-9]{32}$') { return $state }
    } 65000
    $script:group = $delivery.native_id
    $null = Wait-Activity 'the exact registered three-update card' { Get-ExactHistory }
    # The shell exposes card text. Refuse ambiguous text even across groups belonging to this test sender.
    $matching = @($toastHistory.GetHistory($identity) | Where-Object {
        [xml]$xml = $_.Content.GetXml()
        @($xml.toast.visual.binding.text | Where-Object { $_ -ceq $fixtureBody }).Count -eq 1
    })
    if ($matching.Count -ne 1) { Stop-Blocked 'Another test-sender card has the same text.' }
    Add-Evidence 'Verify the single native card and its durable watched-branch destination' $delivery
    $script:focusForm = New-Object Windows.Forms.Form
    $focusForm.Text = 'Threadline watched branch test'
    $focusForm.Width = 320
    $focusForm.Height = 100
    $focusForm.Show()
    $focusForm.Activate()
    [Windows.Forms.Application]::DoEvents()
    if ([ThreadlineWinInput]::GetForegroundWindow() -ne $focusForm.Handle) { Stop-Blocked 'The owned test form did not receive foreground.' }
    [ThreadlineWinInput]::TargetWindow = $focusForm.Handle
    if (!(Invoke-FixtureCenterClick $focusForm.Handle -InvokeOnly)) { Stop-Blocked 'The signed shell did not offer default Invoke for the unique fixture card.' }
    $label = 'Direct comparison ' + [char]0x00b7 + ' ' + $base + ' ' + [char]0x2192 + ' ' + $head
    $null = Wait-Activity 'the full base and head in the installed history view' {
        $control = Find-OwnedId (Get-OwnedWindow $child) $child.Id 'history-comparison-mode'
        $control -and $control.Current.Name -ceq $label
    }
    if (@(Get-InstalledProcesses).Count -ne 1) { throw 'The warm activation left multiple installed processes.' }
    $routed = Wait-Activity 'the acknowledged exact comparison and read group' {
        $s = Read-Fixture inspect-watched
        if ($s.activated -and $s.comparison_saved -and $s.read -eq 3) { return $s }
    }
    Add-Evidence 'Open the exact cached branch and full base/head in the original process' $routed
    Close-Installed $child
    $after = Read-Fixture inspect-watched
    if (!$after.comparison_saved -or !$after.activated -or $after.native_id -cne $group -or $after.read -ne 3) {
        throw 'The installed comparison route did not survive normal exit.'
    }
    $report.comparison_label = $label
    $report.loopback_proxy_received_connection = $activityProxy.Pending()
    if ($report.loopback_proxy_received_connection) { throw 'The installed fixture attempted a network request.' }
}
