[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$Bootstrap,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9A-Fa-f]{64}$')][string]$BootstrapSha256,
    [Parameter(Mandatory=$true)][string]$OfflineFixture,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9A-Fa-f]{64}$')][string]$ManifestSha256,
    [Parameter(Mandatory=$true)][string]$RetainedFixture,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9A-Fa-f]{64}$')][string]$RetainedSha256,
    [Parameter(Mandatory=$true)][string]$Report,
    [switch]$ExpectDisconnected
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not [Environment]::Is64BitProcess -or $env:PROCESSOR_ARCHITECTURE -cne 'AMD64') {
    throw 'Run these local fixture checks in 64-bit Windows PowerShell on native Windows x64.'
}
$utf8 = New-Object Text.UTF8Encoding($false)
function Digest([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Assert-Digest([string]$Path, [string]$Expected) {
    if ((Digest $Path) -ine $Expected) { throw 'A fixture file differs from its independent digest.' }
}
$Bootstrap = (Get-Item -LiteralPath $Bootstrap).FullName
$manifestName = 'threadline-windows-x64.json'
$sourceManifest = Join-Path $OfflineFixture $manifestName
Assert-Digest $Bootstrap $BootstrapSha256
Assert-Digest $sourceManifest $ManifestSha256
$manifest = Get-Content -LiteralPath $sourceManifest -Raw | ConvertFrom-Json
$assetUrl = [Uri]$manifest.artifact_url
if ($assetUrl.Scheme -cne 'https' -or -not $assetUrl.Host.EndsWith('.invalid') -or
    $manifest.target -cne 'x86_64-pc-windows-msvc' -or
    $manifest.version -cnotmatch '^[0-9]+[.][0-9]+[.][0-9]+(?:[-+][A-Za-z0-9.-]+)?$' -or
    $manifest.channel -cnotin @('stable','preview') -or $manifest.size -lt 2 -or
    $manifest.size -gt 16777216 -or $assetUrl.Segments[-1] -cnotmatch '^[a-zA-Z0-9_-]+[.]exe$') {
    throw 'Supply an independently pinned synthetic offline release with a reserved .invalid origin.'
}
$leaf = $assetUrl.Segments[-1]
$signed = Join-Path $OfflineFixture $leaf
Assert-Digest $signed $manifest.sha256
if (-not (Test-Path -LiteralPath (Join-Path $RetainedFixture 'threadline-migration-local10.owner') -PathType Leaf)) {
    throw 'Supply the archived test-owned build 10 migration profile.'
}
$database = Join-Path $RetainedFixture 'threadline.sqlite3'
Assert-Digest $database $RetainedSha256
if (Test-Path -LiteralPath $Report) { throw 'Choose a new report file.' }

function Installed-Fixture {
    $app = Join-Path $env:LOCALAPPDATA 'Programs\Threadline Installer Test\threadline.exe'
    $appDigest = if (Test-Path -LiteralPath $app -PathType Leaf) { Digest $app } else { $null }
    $history = Get-ItemProperty -LiteralPath 'HKCU:\Software\Threadline.InstallTest' -Name HighestBuild -ErrorAction SilentlyContinue
    $highest = if ($history) { $history.HighestBuild } else { $null }
    [ordered]@{
        registered = Test-Path -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Threadline.InstallTest'
        app_sha256 = $appDigest
        highest_build = $highest
        retained_database_sha256 = Digest $database
    }
}
$before = Installed-Fixture
$work = Join-Path ([IO.Path]::GetTempPath()) ('Threadline-bootstrap-faults-' + [Guid]::NewGuid().ToString('N'))
$null = New-Item -Path $work -ItemType Directory -ErrorAction Stop
$stage = Join-Path $work 'staging'
$offline = Join-Path $work 'offline'
$null = New-Item -Path $stage,$offline -ItemType Directory
$currentManifest = Join-Path $offline $manifestName
$candidate = Join-Path $offline $leaf
$cases = New-Object 'System.Collections.Generic.List[object]'

function Verify-Case([string]$Name, [int]$Exit, [string]$Expected, [switch]$InstallPreflight) {
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $Bootstrap
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.EnvironmentVariables['TMP'] = $stage
    $info.EnvironmentVariables['TEMP'] = $stage
    $info.WorkingDirectory = $work
    $pin = Digest $currentManifest
    if ($InstallPreflight) {
        $pin = '0' * 64
        if ($pin -eq (Digest $currentManifest)) { throw 'The guard pin must differ from the actual manifest.' }
    }
    $info.Arguments = '--version "{0}" --channel "{1}" --manifest-sha256 {2} --offline-dir "{3}"' -f $manifest.version,$manifest.channel,$pin,$offline
    if (-not $InstallPreflight) { $info.Arguments += ' --verify-only' }
    $process = [Diagnostics.Process]::Start($info)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(60000)) { $process.Kill(); $process.WaitForExit(); throw 'Fixture verification timed out.' }
        $message = ($stdout.Result + $stderr.Result).Trim()
        if ($process.ExitCode -ne $Exit -or $message -cnotlike ('*' + $Expected + '*')) {
            throw ("Unexpected bootstrap result in {0} (exit {1}): {2}" -f $Name,$process.ExitCode,$message)
        }
        $unchanged = ((Installed-Fixture | ConvertTo-Json -Compress) -ceq ($before | ConvertTo-Json -Compress))
        $left = @(Get-ChildItem -LiteralPath $stage -Force).Count
        if (-not $unchanged -or $left -ne 0) { throw 'A bootstrap case changed the installed fixture or left staging files.' }
        $cases.Add([ordered]@{case=$Name;exit_code=$process.ExitCode;message=$message;supplied_manifest_sha256=$pin;verify_only=(-not $InstallPreflight);
            asset_bytes=(Get-Item -LiteralPath $candidate).Length;staging_entries_after=$left;retained_unchanged=$unchanged})
    } finally { $process.Dispose() }
}

try {
    Copy-Item -LiteralPath $sourceManifest -Destination $currentManifest
    Copy-Item -LiteralPath $signed -Destination $candidate
    Verify-Case 'valid_signed_fixture' 0 'Verified Threadline'
    if ($ExpectDisconnected) {
        Verify-Case 'normal_setup_requires_desktop' 1 'Run setup in an interactive Windows desktop session.' -InstallPreflight
    }

    $wrong = Get-Content -LiteralPath $currentManifest -Raw | ConvertFrom-Json
    $wrong.target = 'aarch64-pc-windows-msvc'
    [IO.File]::WriteAllText($currentManifest, ($wrong | ConvertTo-Json -Depth 5), $utf8)
    Verify-Case 'arm64_manifest_on_native_x64' 1 'does not match the exact requested version, channel and Windows target'
    Copy-Item -LiteralPath $sourceManifest -Destination $currentManifest -Force

    $truncated = [IO.File]::OpenWrite($candidate)
    try { $truncated.SetLength([int64][Math]::Floor($manifest.size / 2)) } finally { $truncated.Dispose() }
    Verify-Case 'interrupted_offline_payload' 1 'download was interrupted or had the wrong size'
    Copy-Item -LiteralPath $signed -Destination $candidate -Force

    $previousAcl = Get-Acl -LiteralPath $stage
    $restrictedAcl = Get-Acl -LiteralPath $stage
    $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
    $deny = New-Object Security.AccessControl.FileSystemAccessRule($sid,'CreateDirectories','Deny')
    $restrictedAcl.AddAccessRule($deny)
    try {
        Set-Acl -LiteralPath $stage -AclObject $restrictedAcl
        try {
            $null = [IO.Directory]::CreateDirectory((Join-Path $stage 'should-be-denied'))
            throw 'The temporary directory permission did not deny creating a child directory.'
        } catch [UnauthorizedAccessException] {}
        Verify-Case 'denied_private_staging_directory' 1 'Could not create a private temporary staging directory'
    } finally { Set-Acl -LiteralPath $stage -AclObject $previousAcl }
    Verify-Case 'fresh_retry_with_intact_payload' 0 'Verified Threadline'

    $receipt = [ordered]@{status='Pass';os_build=[Environment]::OSVersion.Version.Build;architecture=$env:PROCESSOR_ARCHITECTURE;
        shell_version=$PSVersionTable.PSVersion.ToString();bootstrap_sha256=$BootstrapSha256;fixture_manifest_sha256=$ManifestSha256;
        fixture_payload_sha256=$manifest.sha256;expected_asset_bytes=$manifest.size;retained_fixture=$before;cases=$cases}
    [IO.File]::WriteAllText($Report, ($receipt | ConvertTo-Json -Depth 7), $utf8)
} finally {
    if (Test-Path -LiteralPath $work) { Remove-Item -LiteralPath $work -Recurse -Force }
}
