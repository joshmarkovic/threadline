param(
    [Parameter(Mandatory = $true)][int]$OwnerId,
    [Parameter(Mandatory = $true)][long]$OwnerStartTicks,
    [Parameter(Mandatory = $true)][long]$WindowHandle
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
function Stop-Blocked([string]$Message) { throw $Message }
function Add-Evidence([string]$Action, $Observed) {}

try {
    if ($OwnerId -le 0 -or $OwnerStartTicks -le 0 -or $WindowHandle -eq 0) { exit 78 }
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
    Add-Type -Path (Join-Path $PSScriptRoot 'ThreadlineWinInput.cs')
    . (Join-Path $PSScriptRoot 'windows-owned-uia.ps1')
    $script:app = [Diagnostics.Process]::GetProcessById($OwnerId)
    $window = [IntPtr]$WindowHandle
    if ($script:app.StartTime.ToUniversalTime().Ticks -ne $OwnerStartTicks -or
        !(Test-OwnedFixtureWindow $window)) { exit 78 }
    if (!(Invoke-OwnedFixtureControl $window 'reader-activity' $null 'Activity')) { exit 3 }
    exit 0
} catch {
    exit 78
}
