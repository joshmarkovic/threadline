function Test-OwnedFixtureWindow([IntPtr]$Window) {
    $script:app.Refresh()
    return $Window -ne [IntPtr]::Zero -and !$script:app.HasExited -and
        $script:app.MainWindowHandle -eq $Window -and
        [ThreadlineWinInput]::WindowProcessId($Window) -eq $script:app.Id -and
        [ThreadlineWinInput]::IsWindowVisible($Window) -and ![ThreadlineWinInput]::IsIconic($Window)
}

function Test-OwnedAutomationProperties($Properties, [string]$Id, [int]$Owner, [string]$ExpectedName) {
    return $Properties.ProcessId -eq $Owner -and $Properties.AutomationId -ceq $Id -and
        (!$ExpectedName -or $Properties.Name -ceq $ExpectedName) -and
        !$Properties.IsOffscreen -and $Properties.IsEnabled -and
        $Properties.BoundingRectangle.Width -gt 0 -and $Properties.BoundingRectangle.Height -gt 0
}

function Get-OwnedFixtureActivation($Element, [string]$Id) {
    $pattern = $null
    if ($Element.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
        return [pscustomobject]@{ Pattern = $pattern; Invoke = $true }
    }
    if ($Id -cmatch '^reader-tab-[0-9]+-[0-9]+$' -and
        $Element.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) {
        return [pscustomobject]@{ Pattern = $pattern; Invoke = $false }
    }
    return $null
}

function Select-UniqueOwnedFixtureControl($Controls, [string]$Id, [int]$Owner, [string]$ExpectedName) {
    $match = $null
    foreach ($control in $Controls) {
        if (!(Test-OwnedAutomationProperties $control.Current $Id $Owner $ExpectedName)) { continue }
        if ($ExpectedName -and $null -eq (Get-OwnedFixtureActivation $control $Id)) { continue }
        if ($null -ne $match) {
            Stop-Blocked 'The owned accessibility tree returned multiple eligible controls for the exact identity.'
        }
        $match = $control
    }
    return $match
}

function Find-OwnedFixtureControl([IntPtr]$Window, [string]$Id, [string]$ExpectedName) {
    if (!(Test-OwnedFixtureWindow $Window)) { Stop-Blocked 'The isolated Threadline window is no longer visible and owned.' }
    try {
        $root = [Windows.Automation.AutomationElement]::FromHandle($Window)
        if ($null -eq $root -or $root.Current.ProcessId -ne $script:app.Id) { return $null }
        $conditions = @(
            [Windows.Automation.PropertyCondition]::new(
                [Windows.Automation.AutomationElement]::ProcessIdProperty, [int]$script:app.Id)
            [Windows.Automation.PropertyCondition]::new(
                [Windows.Automation.AutomationElement]::AutomationIdProperty, $Id)
        )
        if ($ExpectedName) {
            $conditions += [Windows.Automation.PropertyCondition]::new(
                [Windows.Automation.AutomationElement]::NameProperty, $ExpectedName)
        }
        $condition = [Windows.Automation.AndCondition]::new([Windows.Automation.Condition[]]$conditions)
        $controls = $root.FindAll([Windows.Automation.TreeScope]::Descendants, $condition)
        return Select-UniqueOwnedFixtureControl $controls $Id $script:app.Id $ExpectedName
    } catch [Windows.Automation.ElementNotAvailableException] {
        return $null
    }
}

function Invoke-OwnedFixtureControl([IntPtr]$Window, [string]$Id, [Diagnostics.Stopwatch]$InputClock, [string]$ExpectedName) {
    $element = Find-OwnedFixtureControl $Window $Id $ExpectedName
    if ($null -eq $element) { return $false }
    $activation = Get-OwnedFixtureActivation $element $Id
    if ($null -eq $activation) { return $false }
    if (!(Test-OwnedFixtureWindow $Window) -or
        !(Test-OwnedAutomationProperties $element.Current $Id $script:app.Id $ExpectedName)) {
        Stop-Blocked 'The owned accessibility control changed before Invoke.'
    }
    if ($null -ne $InputClock) { $InputClock.Restart() }
    if ($activation.Invoke) {
        $activation.Pattern.Invoke()
        $kind = 'UIA InvokePattern'
    } else {
        $activation.Pattern.Select()
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
