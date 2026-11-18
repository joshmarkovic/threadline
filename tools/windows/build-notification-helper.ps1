[CmdletBinding()]
param([Parameter(Mandatory)][string]$Output)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$target = [IO.Path]::GetFullPath($Output)
if (Test-Path -LiteralPath $target) { throw 'Choose a new output file for the notification helper.' }
$csc = Join-Path ([Runtime.InteropServices.RuntimeEnvironment]::GetRuntimeDirectory()) 'csc.exe'
if (![Environment]::Is64BitProcess -or !(Test-Path -LiteralPath $csc -PathType Leaf)) {
    throw 'Use 64-bit Windows PowerShell with the .NET Framework C# compiler to build this installer component.'
}
$shortcut = Join-Path (Split-Path -Parent $PSScriptRoot) 'native\ThreadlineToastShortcut.cs'
& $csc /nologo /target:winexe /platform:x64 /optimize+ /warnaserror+ "/out:$target" (Join-Path $PSScriptRoot 'NotificationRegistration.cs') $shortcut
if ($LASTEXITCODE -ne 0) { throw 'The notification registration helper did not compile.' }
(Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant()
