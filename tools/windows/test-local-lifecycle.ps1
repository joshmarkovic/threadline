[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Predecessor,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$PredecessorSha256,
    [Parameter(Mandatory = $true)][string]$Current,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$CurrentSha256,
    [Parameter(Mandatory = $true)][string]$StageFailure,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$StageFailureSha256,
    [ValidatePattern('^[0-9a-fA-F]{64}$')][string]$ProfileSha256
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (![Environment]::Is64BitProcess -or $env:PROCESSOR_ARCHITECTURE -cne 'AMD64') {
    throw 'Run the local lifecycle test in 64-bit Windows PowerShell on x64 Windows.'
}

$install = Join-Path $env:LOCALAPPDATA 'Programs\Threadline Installer Test'
$exe = Join-Path $install 'threadline.exe'
$uninstall = Join-Path $install 'Uninstall.exe'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Threadline Installer Test.lnk'
$registration = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Threadline.InstallTest'
$history = 'HKCU:\Software\Threadline.InstallTest'
$fixtureDatabase = Join-Path $env:LOCALAPPDATA 'Threadline\Data\threadline.sqlite3'
$steps = [System.Collections.Generic.List[object]]::new()

function Read-Package([string]$Path, [string]$Digest, [string]$Fault) {
    $file = Get-Item -LiteralPath $Path
    if ($file.PSIsContainer -or $file.Extension -ine '.exe' -or
        ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'Specify an ordinary local fixture executable.'
    }
    $manifest = Get-Content -LiteralPath ($file.FullName + '.json') -Raw | ConvertFrom-Json
    if ($manifest.channel -cne 'local-fixture' -or $manifest.target -cne 'x86_64-pc-windows-msvc' -or
        $manifest.fixture_fault -cne $Fault -or $manifest.app_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
        $manifest.package_version -cnotmatch '^\d+\.\d+\.\d+[-+]local[.][0-9]+$' -or
        $manifest.package_build -lt 1 -or $manifest.size -ne $file.Length -or
        $manifest.sha256 -ine $Digest -or
        (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash -ine $Digest) {
        throw 'The local installer does not match its proof, expected scope, and independent digest.'
    }
    [pscustomobject]@{
        path = $file.FullName
        sha = $Digest
        version = $manifest.package_version
        build = $manifest.package_build
        schema = $manifest.current_schema
        app = $manifest.app_sha256
    }
}

function Assert-ProfileUnchanged {
    if (!$ProfileSha256) {
        if (Test-Path -LiteralPath $fixtureDatabase) { throw 'Setup created an unexpected application database.' }
        return
    }
    if ((Get-FileHash -LiteralPath $fixtureDatabase -Algorithm SHA256).Hash -ine $ProfileSha256) {
        throw 'The offline fixture database changed during a packaging operation.'
    }
}

function Assert-Installed($Package) {
    $entry = Get-ItemProperty -LiteralPath $registration
    if ($entry.InstallLocation -ine $install -or $entry.DisplayName -cne 'Threadline Installer Test' -or
        $entry.DisplayVersion -cne $Package.version -or $entry.InstalledBuild -ne $Package.build -or
        $entry.UninstallString -ine ('"' + $uninstall + '"') -or
        (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ine $Package.app -or
        !(Test-Path -LiteralPath $uninstall -PathType Leaf) -or
        !(Test-Path -LiteralPath $shortcut -PathType Leaf)) {
        throw 'The managed app, version, and integration did not match the expected local fixture.'
    }
    $shell = New-Object -ComObject WScript.Shell
    try {
        $link = $shell.CreateShortcut($shortcut)
        try {
            if ($link.TargetPath -ine $exe -or $link.Arguments) {
                throw 'The existing Start menu shortcut does not launch the expected app.'
            }
        } finally {
            [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($link)
        }
    } finally {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell)
    }
}

function Assert-Uninstalled {
    foreach ($path in @($exe, $uninstall, $shortcut, $registration)) {
        if (Test-Path -LiteralPath $path) { throw 'Uninstall left a managed program or integration entry.' }
    }
}

function Run-Setup([string]$Step, $Package, [int]$ExpectedExit) {
    if ((Get-FileHash -LiteralPath $Package.path -Algorithm SHA256).Hash -ine $Package.sha) {
        throw 'The verified local setup changed before it was started.'
    }
    $process = Start-Process -FilePath $Package.path -ArgumentList '/S' -PassThru
    try {
        if (!$process.WaitForExit(120000)) { throw "Local setup did not finish during $Step." }
        if ($process.ExitCode -ne $ExpectedExit) {
            throw "Local setup returned $($process.ExitCode) during $Step. Expected $ExpectedExit."
        }
        $steps.Add([pscustomobject]@{ step = $Step; exit_code = $process.ExitCode })
    } finally {
        $process.Dispose()
    }
    if (Get-Process -Name threadline -ErrorAction SilentlyContinue) {
        throw 'A Threadline process appeared during a package-only operation.'
    }
    Assert-ProfileUnchanged
}

$old = Read-Package $Predecessor $PredecessorSha256 'none'
$new = Read-Package $Current $CurrentSha256 'none'
$fault = Read-Package $StageFailure $StageFailureSha256 'stage-write'
if ($null -eq $old.schema -or $null -eq $new.schema -or $old.schema -ge $new.schema -or
    $old.build -ge $new.build -or $fault.build -ne $new.build -or $fault.app -ine $new.app -or
    $fault.schema -ne $new.schema -or $fault.version -cne $new.version) {
    throw 'Use documented predecessor/current schemas and a staging-fault build of the exact current payload.'
}
if ((Test-Path -LiteralPath $install) -or (Test-Path -LiteralPath $registration) -or
    (Test-Path -LiteralPath $shortcut) -or
    (Get-Process -Name threadline -ErrorAction SilentlyContinue)) {
    throw 'Uninstall the test fixture and close any existing application before this sequence.'
}
if (Test-Path -LiteralPath $history) {
    $previous = Get-Item -LiteralPath $history
    try {
        $names = @($previous.GetValueNames())
        if ($previous.SubKeyCount -ne 0 -or $names.Count -ne 1 -or $names[0] -cne 'HighestBuild' -or
            $previous.GetValueKind('HighestBuild') -ne [Microsoft.Win32.RegistryValueKind]::DWord) {
            throw 'The local fixture history contains unexpected values.'
        }
        $floor = $previous.GetValue('HighestBuild')
        if ($floor -lt 1 -or $old.build -lt $floor) {
            throw 'Use a predecessor build ordinal at least as high as the retained local fixture history.'
        }
    } finally {
        $previous.Dispose()
    }
}
if ((Test-Path -LiteralPath ($fixtureDatabase + '-wal')) -or (Test-Path -LiteralPath ($fixtureDatabase + '-shm'))) {
    throw 'Close the isolated fixture profile and checkpoint its SQLite sidecars before starting.'
}
Assert-ProfileUnchanged

try {
    Run-Setup 'install_predecessor' $old 0
    Assert-Installed $old
    Run-Setup 'repair_predecessor' $old 0
    Assert-Installed $old

    $lock = [IO.File]::Open($exe, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    try {
        Run-Setup 'refuse_locked_executable' $new 14
        Assert-Installed $old
    } finally {
        $lock.Dispose()
    }
    Run-Setup 'fail_staged_write' $fault 16
    Assert-Installed $old
    Run-Setup 'upgrade_after_failures' $new 0
    Assert-Installed $new
    Run-Setup 'repair_current' $new 0
    Assert-Installed $new
    Run-Setup 'refuse_installed_downgrade' $old 12
    Assert-Installed $new

    $remove = Start-Process -FilePath $uninstall -ArgumentList '/S' -PassThru
    try {
        if (!$remove.WaitForExit(120000) -or $remove.ExitCode -ne 0) {
            throw 'The registered fixture uninstaller did not exit successfully.'
        }
        $watch = [Diagnostics.Stopwatch]::StartNew()
        while (((Test-Path -LiteralPath $registration) -or (Test-Path -LiteralPath $exe) -or
                (Test-Path -LiteralPath $uninstall) -or (Test-Path -LiteralPath $shortcut)) -and
                $watch.ElapsedMilliseconds -lt 15000) {
            Start-Sleep -Milliseconds 100
        }
        Assert-Uninstalled
        $steps.Add([pscustomobject]@{ step = 'uninstall_retains_profile'; exit_code = $remove.ExitCode })
    } finally {
        $remove.Dispose()
    }
    Assert-ProfileUnchanged
    Run-Setup 'refuse_downgrade_after_uninstall' $old 12
    Assert-Uninstalled
    Run-Setup 'reinstall_current' $new 0
    Assert-Installed $new
} finally {
    [ordered]@{
        channel = 'local-fixture'
        target = 'x86_64-pc-windows-msvc'
        windows_build = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion').CurrentBuildNumber
        predecessor_setup_sha256 = $old.sha.ToLowerInvariant()
        current_setup_sha256 = $new.sha.ToLowerInvariant()
        staged_fault_setup_sha256 = $fault.sha.ToLowerInvariant()
        preseeded_profile_checked = [bool]$ProfileSha256
        completed_steps = @($steps.ToArray())
    } | ConvertTo-Json -Depth 4
}
