function Test-OwnedFixtureWindow([IntPtr]$Window) {
    $script:app.Refresh()
    return $Window -ne [IntPtr]::Zero -and !$script:app.HasExited -and
        $script:app.MainWindowHandle -eq $Window -and
        [ThreadlineWinInput]::WindowProcessId($Window) -eq $script:app.Id -and
        [ThreadlineWinInput]::IsWindowVisible($Window) -and ![ThreadlineWinInput]::IsIconic($Window)
}

function Test-OwnedAutomationProperties($Properties, [string]$Id, [int]$Owner) {
    return $Properties.ProcessId -eq $Owner -and $Properties.AutomationId -ceq $Id -and
        !$Properties.IsOffscreen -and $Properties.IsEnabled -and
        $Properties.BoundingRectangle.Width -gt 0 -and $Properties.BoundingRectangle.Height -gt 0
}

function Find-OwnedFixtureControl([IntPtr]$Window, [string]$Id) {
    if (!(Test-OwnedFixtureWindow $Window)) { Stop-Blocked 'The isolated Threadline window is no longer visible and owned.' }
    try {
        $root = [Windows.Automation.AutomationElement]::FromHandle($Window)
        if ($null -eq $root -or $root.Current.ProcessId -ne $script:app.Id) { return $null }
        $owner = [Windows.Automation.PropertyCondition]::new(
            [Windows.Automation.AutomationElement]::ProcessIdProperty, [int]$script:app.Id)
        $identifier = [Windows.Automation.PropertyCondition]::new(
            [Windows.Automation.AutomationElement]::AutomationIdProperty, $Id)
        $condition = [Windows.Automation.AndCondition]::new([Windows.Automation.Condition[]]@($owner, $identifier))
        $controls = $root.FindAll([Windows.Automation.TreeScope]::Descendants, $condition)
        if ($controls.Count -eq 0) { return $null }
        if ($controls.Count -eq 1 -and !(Test-OwnedAutomationProperties $controls[0].Current $Id $script:app.Id)) { return $null }
    } catch [Windows.Automation.ElementNotAvailableException] {
        return $null
    }
    if ($controls.Count -ne 1) { Stop-Blocked 'The owned accessibility tree returned an ambiguous control identity.' }
    return $controls[0]
}

function Invoke-OwnedFixtureControl([IntPtr]$Window, [string]$Id, [Diagnostics.Stopwatch]$InputClock) {
    $element = Find-OwnedFixtureControl $Window $Id
    if ($null -eq $element) { return $false }
    $pattern = $null
    $invoke = $element.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)
    if (!$invoke -and ($Id -cnotmatch '^reader-tab-[0-9]+-[0-9]+$' -or
        !$element.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern))) {
        return $false
    }
    if (!(Test-OwnedFixtureWindow $Window) -or !(Test-OwnedAutomationProperties $element.Current $Id $script:app.Id)) {
        Stop-Blocked 'The owned accessibility control changed before Invoke.'
    }
    if ($null -ne $InputClock) { $InputClock.Restart() }
    if ($invoke) {
        $pattern.Invoke()
        $kind = 'UIA InvokePattern'
    } else {
        $pattern.Select()
        $kind = 'UIA SelectionItemPattern'
    }
    Add-Evidence 'Activate an owned accessibility control' ([ordered]@{ automation_id = $Id; input = $kind })
    return $true
}

function Set-OwnedFixtureRepository([IntPtr]$Window) {
    $id = 'account-repository'
    $element = Find-OwnedFixtureControl $Window $id
    $pattern = $null
    if ($null -eq $element -or $element.Current.IsPassword -or
        !$element.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$pattern) -or
        $pattern.Current.IsReadOnly) {
        return $false
    }
    if (!(Test-OwnedFixtureWindow $Window) -or !(Test-OwnedAutomationProperties $element.Current $id $script:app.Id)) {
        Stop-Blocked 'The isolated repository field changed before SetValue.'
    }
    $pattern.SetValue('fixture-org/private')
    Add-Evidence 'Set the verified synthetic repository field' ([ordered]@{ automation_id = $id; input = 'UIA ValuePattern' })
    return $true
}

function Assert-OwnedMaskedFixtureField([IntPtr]$Window) {
    $element = Find-OwnedFixtureControl $Window 'account-token'
    if ($null -eq $element -or !$element.Current.IsPassword) {
        Stop-Blocked 'The exact masked fixture field is unavailable in the owned accessibility tree.'
    }
    $pattern = $null
    if ($element.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
        throw 'Unexpected readable ValuePattern for the masked fixture credential.'
    }
}
