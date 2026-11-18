[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Prepare','Apply','Restore','Remove')][string]$Mode,
    [Parameter(Mandatory)][ValidateSet('Local','Release')][string]$Channel,
    [Parameter(Mandatory)][string]$App,
    [Parameter(Mandatory)][string]$Link,
    [Parameter(Mandatory)][string]$Scratch
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($Channel -eq 'Local') {
    $displayName = 'Threadline Installer Test'
    $identity = 'Threadline.InstallTest'
    $class = [guid]'df3bb9a2-0794-44a6-b130-a35197093afe'
} else {
    $displayName = 'Threadline'
    $identity = 'Threadline.Desktop'
    $class = [guid]'5a20cea8-a5d8-4b5e-9be3-4121ba0aa753'
}
$expectedExe = Join-Path $env:LOCALAPPDATA "Programs\$displayName\threadline.exe"
$expectedLink = Join-Path ([Environment]::GetFolderPath('Programs')) "$displayName.lnk"
if ([IO.Path]::GetFullPath($App) -ine $expectedExe -or [IO.Path]::GetFullPath($Link) -ine $expectedLink) {
    throw 'The notification registration does not match this per-user install.'
}
$clsid = $class.ToString('B')
$classes = [Microsoft.Win32.Registry]::CurrentUser
$root = "Software\Classes\CLSID\$clsid"
$aumid = "Software\Classes\AppUserModelId\$identity"
$server = "$root\LocalServer32"
$command = '"' + $expectedExe + '" --notification-com-server'
$snapshot = Join-Path $Scratch 'notification-state.json'
$oldLink = Join-Path $Scratch 'previous-notification.lnk'
$newLink = Join-Path $Scratch 'next-notification.lnk'
function Read-Value([string]$Path,[string]$Name) {
    $key = $classes.OpenSubKey($Path)
    if (!$key) { return $null }
    try { return $key.GetValue($Name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) }
    finally { $key.Dispose() }
}
function Set-Value([string]$Path,[string]$Name,$Value) {
    $key = $classes.CreateSubKey($Path)
    try { if ($null -eq $Value) { $key.DeleteValue($Name,$false) } else { $key.SetValue($Name,$Value,[Microsoft.Win32.RegistryValueKind]::String) } }
    finally { $key.Dispose() }
}
function Remove-EmptyKey([string]$Path) {
    $key = $classes.OpenSubKey($Path)
    if (!$key) { return }
    try { $empty = $key.ValueCount -eq 0 -and $key.SubKeyCount -eq 0 } finally { $key.Dispose() }
    if ($empty) { $classes.DeleteSubKey($Path, $false) }
}
function Assert-OwnedOrAbsent {
    $existing = Read-Value $server ''
    $activator = Read-Value $aumid 'CustomActivator'
    $name = Read-Value $aumid 'DisplayName'
    if (($null -ne $existing -and $existing -ine $command) -or ($null -ne $activator -and $activator -ine $clsid) -or ($null -ne $name -and $name -ine $displayName)) {
        throw 'Windows notification ownership differs from the expected registration.'
    }
}
switch ($Mode) {
    'Prepare' {
        Assert-OwnedOrAbsent
        $state = [ordered]@{ server = Read-Value $server ''; activator = Read-Value $aumid 'CustomActivator'; name = Read-Value $aumid 'DisplayName'; hadLink = [IO.File]::Exists($Link) }
        if ($state.hadLink) {
            $shell = New-Object -ComObject WScript.Shell
            try {
                $target = $shell.CreateShortcut($Link).TargetPath
                if ($target -ine $expectedExe) { throw 'The Start menu shortcut belongs to another application.' }
            } finally { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell) }
        }
        if ($state.hadLink) { [IO.File]::Copy($Link,$oldLink,$false) }
        $state | ConvertTo-Json -Compress | Set-Content -LiteralPath $snapshot -Encoding UTF8
        Add-Type -Path (Join-Path $PSScriptRoot 'ThreadlineToastShortcut.cs')
        [ThreadlineToastShortcut]::CreateOwnedLinkWithActivator($newLink,$expectedExe,$identity,$class)
    }
    'Apply' {
        Assert-OwnedOrAbsent
        if (![IO.File]::Exists($snapshot) -or ![IO.File]::Exists($newLink)) { throw 'Notification registration was not staged.' }
        [IO.File]::Copy($newLink,$Link,$true)
        Set-Value $server '' $command
        Set-Value $aumid 'CustomActivator' $clsid
        Set-Value $aumid 'DisplayName' $displayName
    }
    'Restore' {
        if (![IO.File]::Exists($snapshot)) {return}
        Assert-OwnedOrAbsent
        $state = Get-Content -Raw -LiteralPath $snapshot | ConvertFrom-Json
        if ($state.hadLink) { [IO.File]::Copy($oldLink,$Link,$true) }
        elseif ([IO.File]::Exists($Link)) { [IO.File]::Delete($Link) }
        Set-Value $server '' $state.server
        Set-Value $aumid 'CustomActivator' $state.activator
        Set-Value $aumid 'DisplayName' $state.name
        Remove-EmptyKey $server
        Remove-EmptyKey $root
        Remove-EmptyKey $aumid
    }
    'Remove' {
        Assert-OwnedOrAbsent
        Set-Value $server '' $null
        Set-Value $aumid 'CustomActivator' $null
        Set-Value $aumid 'DisplayName' $null
        Remove-EmptyKey $server
        Remove-EmptyKey $root
        Remove-EmptyKey $aumid
    }
}
