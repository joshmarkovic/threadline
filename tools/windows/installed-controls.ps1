function Get-OwnedWindow([Diagnostics.Process]$Process) {
    $Process.Refresh()
    if ($Process.HasExited -or $Process.MainWindowHandle -eq [IntPtr]::Zero -or
        $Process.MainModule.FileName -ine $exe) { return $null }
    $root = [Windows.Automation.AutomationElement]::FromHandle($Process.MainWindowHandle)
    if ($null -ne $root -and $root.Current.ProcessId -eq $Process.Id -and !$root.Current.IsOffscreen) {
        return $root
    }
    return $null
}

function Find-OwnedId($Root, [int]$ChildId, [string]$Id, [switch]$AllowDisabled) {
    if ($null -eq $Root -or $Root.Current.ProcessId -ne $ChildId) { return $null }
    $idCondition = [Windows.Automation.PropertyCondition]::new(
        [Windows.Automation.AutomationElement]::AutomationIdProperty, $Id)
    $ownerCondition = [Windows.Automation.PropertyCondition]::new(
        [Windows.Automation.AutomationElement]::ProcessIdProperty, $ChildId)
    $scope = [Windows.Automation.AndCondition]::new([Windows.Automation.Condition[]]@($idCondition, $ownerCondition))
    $items = $Root.FindAll([Windows.Automation.TreeScope]::Descendants, $scope)
    if ($items.Count -ne 1) { return $null }
    $candidate = $items.Item(0)
    if ($candidate.Current.ProcessId -eq $ChildId -and $candidate.Current.AutomationId -ceq $Id -and
        ($AllowDisabled -or $candidate.Current.IsEnabled) -and !$candidate.Current.IsOffscreen) { return $candidate }
    return $null
}

function Invoke-OwnedId([Diagnostics.Process]$Process, [string]$Id) {
    $root = Get-OwnedWindow $Process
    $control = Find-OwnedId $root $Process.Id $Id
    $invoke = $null
    if ($null -eq $control -or
        !$control.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$invoke) -or
        $null -eq (Get-OwnedWindow $Process) -or $control.Current.ProcessId -ne $Process.Id) { return $false }
    $invoke.Invoke()
    return $true
}
