function Test-OwnedFixtureWindow([IntPtr]$Window) {
    $script:app.Refresh()
    return $Window -ne [IntPtr]::Zero -and !$script:app.HasExited -and
        $script:app.MainWindowHandle -eq $Window -and
        [ThreadlineWinInput]::WindowProcessId($Window) -eq $script:app.Id -and
        [ThreadlineWinInput]::IsWindowVisible($Window) -and ![ThreadlineWinInput]::IsIconic($Window)
}

function Test-OwnedAutomationProperties($Properties, [string]$Id, [int]$Owner, [string]$ExpectedName, [switch]$AllowDisabled) {
    return $Properties.ProcessId -eq $Owner -and $Properties.AutomationId -ceq $Id -and
        (!$ExpectedName -or $Properties.Name -ceq $ExpectedName) -and
        !$Properties.IsOffscreen -and ($AllowDisabled -or $Properties.IsEnabled) -and
        $Properties.BoundingRectangle.Width -gt 0 -and $Properties.BoundingRectangle.Height -gt 0
}

function Test-OwnedFixtureEditProperties($Properties) {
    $type = $Properties.PSObject.Properties['ControlType']
    $password = $Properties.PSObject.Properties['IsPassword']
    if ($null -eq $type -or $null -eq $password -or $password.Value -isnot [bool] -or $password.Value) {
        return $false
    }
    $kind = $type.Value
    return $kind -is [Windows.Automation.ControlType] -and $kind.Id -eq [Windows.Automation.ControlType]::Edit.Id
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

function Select-UniqueOwnedFixtureControl($Controls, [string]$Id, [int]$Owner, [string]$ExpectedName, [switch]$Edit, [switch]$AllowDisabled) {
    $match = $null
    foreach ($control in $Controls) {
        $properties = $control.Current
        if (!(Test-OwnedAutomationProperties $properties $Id $Owner $ExpectedName -AllowDisabled:$AllowDisabled)) { continue }
        if ($Edit -and !(Test-OwnedFixtureEditProperties $properties)) { continue }
        if (!$Edit -and !$AllowDisabled -and $ExpectedName -and $null -eq (Get-OwnedFixtureActivation $control $Id)) { continue }
        if ($null -ne $match) {
            Stop-Blocked 'The owned accessibility tree returned multiple eligible controls for the exact identity.'
        }
        $match = $control
    }
    return $match
}

function Find-OwnedFixtureControl([IntPtr]$Window, [string]$Id, [string]$ExpectedName, [switch]$Edit, [switch]$AllowDisabled) {
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
        return Select-UniqueOwnedFixtureControl $controls $Id $script:app.Id $ExpectedName -Edit:$Edit -AllowDisabled:$AllowDisabled
    } catch [Windows.Automation.ElementNotAvailableException] {
        return $null
    }
}

function Invoke-OwnedFixtureControl([IntPtr]$Window, [string]$Id, [Diagnostics.Stopwatch]$InputClock, [string]$ExpectedName, [hashtable]$InputTiming) {
    $element = Find-OwnedFixtureControl $Window $Id $ExpectedName
    if ($null -eq $element) { return $false }
    try {
        $activation = Get-OwnedFixtureActivation $element $Id
        if ($null -eq $activation) { return $false }
        $properties = $element.Current
    } catch [Windows.Automation.ElementNotAvailableException] {
        return $false
    }
    if (!(Test-OwnedFixtureWindow $Window)) {
        Stop-Blocked 'The isolated Threadline window changed before Invoke.'
    }
    if (!(Test-OwnedAutomationProperties $properties $Id $script:app.Id $ExpectedName)) {
        if ($ExpectedName) { return $false }
        Stop-Blocked 'The owned accessibility control changed before Invoke.'
    }
    if ($null -ne $InputClock) { $InputClock.Restart() }
    if ($InputTiming) { $InputTiming.begin = [Diagnostics.Stopwatch]::GetTimestamp() }
    if ($activation.Invoke) {
        $activation.Pattern.Invoke()
        $kind = 'UIA InvokePattern'
    } else {
        $activation.Pattern.Select()
        $kind = 'UIA SelectionItemPattern'
    }
    if ($InputTiming) {
        $InputTiming.returned = [Diagnostics.Stopwatch]::GetTimestamp()
        $InputTiming.method = $kind
    }
    Add-Evidence 'Activate an owned accessibility control' ([ordered]@{ automation_id = $Id; input = $kind })
    return $true
}

function Get-OwnedFixtureEditPattern([IntPtr]$Window, [string]$Id, [string]$Name) {
    $element = Find-OwnedFixtureControl $Window $Id $Name -Edit
    if ($null -eq $element) { return $null }
    try {
        $properties = $element.Current
        $pattern = $null
        if (!(Test-OwnedAutomationProperties $properties $Id $script:app.Id $Name) -or
            !(Test-OwnedFixtureEditProperties $properties) -or
            !$element.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$pattern) -or
            $pattern.Current.IsReadOnly) {
            return $null
        }
    } catch [Windows.Automation.ElementNotAvailableException] {
        return $null
    }
    if (!(Test-OwnedFixtureWindow $Window)) { Stop-Blocked 'The isolated window changed before the editor action.' }
    return $pattern
}

function Set-OwnedFixtureEditValue([IntPtr]$Window, [string]$Id, [string]$Name, [string]$Value) {
    $pattern = Get-OwnedFixtureEditPattern $Window $Id $Name
    if ($null -eq $pattern) { return $false }
    $pattern.SetValue($Value)
    Add-Evidence 'Edit an owned synthetic field' ([ordered]@{ automation_id = $Id; input = 'UIA ValuePattern' })
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

function Assert-OwnedMaskedFixtureField([IntPtr]$Window, [switch]$ObserveFocus) {
    $element = Find-OwnedFixtureControl $Window 'account-token'
    if ($null -eq $element -or !$element.Current.IsPassword) {
        Stop-Blocked 'The exact masked fixture field is unavailable in the owned accessibility tree.'
    }
    $pattern = $null
    if ($element.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {
        throw 'Unexpected readable ValuePattern for the masked fixture credential.'
    }
    if ($ObserveFocus) {
        try {
            if ($element.Current.HasKeyboardFocus) { return 'masked-editor' }
            $condition = [Windows.Automation.AndCondition]::new([Windows.Automation.Condition[]]@(
                [Windows.Automation.PropertyCondition]::new(
                    [Windows.Automation.AutomationElement]::ProcessIdProperty, [int]$script:app.Id)
                [Windows.Automation.PropertyCondition]::new(
                    [Windows.Automation.AutomationElement]::HasKeyboardFocusProperty, $true)
            ))
            if ($null -ne $element.FindFirst([Windows.Automation.TreeScope]::Descendants, $condition)) {
                return 'masked-descendant'
            }
            if (!(Test-OwnedFixtureWindow $Window)) {
                Stop-Blocked 'The isolated window changed while checking the masked field.'
            }
            $root = [Windows.Automation.AutomationElement]::FromHandle($Window)
            if ($null -eq $root -or $root.Current.ProcessId -ne $script:app.Id) {
                Stop-Blocked 'The isolated accessibility root changed while checking the masked field.'
            }
            if ($null -ne $root.FindFirst([Windows.Automation.TreeScope]::Descendants, $condition)) {
                return 'other-owned-descendant'
            }
            if ($root.Current.HasKeyboardFocus) { return 'owned-window' }
            return 'not-reported'
        } catch [Windows.Automation.ElementNotAvailableException] {
            Stop-Blocked 'The owned masked fixture field was replaced before input.'
        }
    }
}
