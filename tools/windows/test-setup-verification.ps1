# Exercise the exact commands embedded in setup without installing an application.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (![Environment]::Is64BitProcess -or [Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'Run this check in 64-bit PowerShell on Windows.'
}

$source = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'threadline-installer.nsi')) -replace '\\\r?\n\s*', ' '
$commands = [regex]::Matches($source, '-Command "(?<code>[^"\r\n]+)"''')
$checks = @{}
$quoteEscape = '$\' + [char]39
foreach ($match in $commands) {
    $command = $match.Groups['code'].Value.Replace('$$', '$').Replace($quoteEscape, [string][char]39)
    $key = [regex]::Match($command, '\$env:(THREADLINE_SETUP_(?:INSTANCE|PAYLOAD))').Groups[1].Value
    if (!$key) { continue }
    if ($checks.ContainsKey($key)) { throw 'Found duplicate setup verification commands.' }
    $checks[$key] = $command
}
if ($checks.Count -ne 2) { throw 'Expected both setup verification commands.' }

$root = Join-Path ([IO.Path]::GetTempPath()) ('threadline-setup-check-' + [guid]::NewGuid().ToString('N'))
$null = [IO.Directory]::CreateDirectory($root)
$modules = [IO.Directory]::CreateDirectory((Join-Path $root 'empty-modules')).FullName
$powershell = Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'

function Assert-SetupExit($Command, $Variable, $Value, [int]$Expected) {
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $powershell
    $info.Arguments = '-NoLogo -NoProfile -NonInteractive -Command "' + $Command + '"'
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.EnvironmentVariables['PSModulePath'] = $modules
    $info.EnvironmentVariables[$Variable] = $Value
    $child = [Diagnostics.Process]::Start($info)
    try {
        if (!$child.WaitForExit(15000)) { throw 'Setup verification timed out.' }
        if ($child.ExitCode -ne $Expected) {
            throw "$Variable returned $($child.ExitCode), expected $Expected."
        }
    } finally {
        if (!$child.HasExited) { $child.Kill(); $null = $child.WaitForExit(3000) }
        $child.Dispose()
    }
}

$running = $null
try {
    $pin = 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'
    $verify = $checks.THREADLINE_SETUP_PAYLOAD.Replace('${PAYLOAD_SHA256}', $pin)
    $payload = Join-Path $root ('payload [test] O''Brien $.exe' + [char]0x03bb)
    [IO.File]::WriteAllBytes($payload, [Text.Encoding]::ASCII.GetBytes('abc'))
    Assert-SetupExit $verify THREADLINE_SETUP_PAYLOAD $payload 0
    Assert-SetupExit ($verify.Replace($pin, ('0' * 64))) THREADLINE_SETUP_PAYLOAD $payload 43
    $held = [IO.File]::Open($payload, 'Open', 'ReadWrite', 'None')
    try { Assert-SetupExit $verify THREADLINE_SETUP_PAYLOAD $payload 44 }
    finally { $held.Dispose() }
    Assert-SetupExit $verify THREADLINE_SETUP_PAYLOAD (Join-Path $root 'missing.exe') 44

    $preflight = $checks.THREADLINE_SETUP_INSTANCE
    $instance = Join-Path $root 'threadline.exe'
    Assert-SetupExit $preflight THREADLINE_SETUP_INSTANCE $instance 0
    $missingModule = $preflight.Replace('Microsoft.PowerShell.Management\Get-Process', 'Missing.ThreadlineFixture\Get-Process')
    Assert-SetupExit $missingModule THREADLINE_SETUP_INSTANCE $instance 44

    # The renamed system command interpreter waits on this private input pipe.
    [IO.File]::Copy((Join-Path $env:WINDIR 'System32\cmd.exe'), $instance)
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $instance
    $info.Arguments = '/D /Q /K'
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $running = [Diagnostics.Process]::Start($info)
    Assert-SetupExit $preflight THREADLINE_SETUP_INSTANCE $instance 42
    Assert-SetupExit $preflight THREADLINE_SETUP_INSTANCE (Join-Path $root 'other\threadline.exe') 0
} finally {
    if ($null -ne $running) {
        if (!$running.HasExited) { $running.Kill(); $null = $running.WaitForExit(3000) }
        $running.Dispose()
    }
    [IO.Directory]::Delete($root, $true)
}
Write-Output "PowerShell $($PSVersionTable.PSVersion) passed staged digest, unreadable file and exact running-path checks with an overridden parent module path."
