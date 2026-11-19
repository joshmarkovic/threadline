[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Predecessor, [Parameter(Mandatory)][string]$PredecessorSha256,
    [Parameter(Mandatory)][string]$Current, [Parameter(Mandatory)][string]$CurrentSha256,
    [Parameter(Mandatory)][string]$StageFailure, [Parameter(Mandatory)][string]$StageFailureSha256,
    [Parameter(Mandatory)][string]$Fixture, [Parameter(Mandatory)][string]$FixtureSha256,
    [Parameter(Mandatory)][string]$Bootstrap, [Parameter(Mandatory)][string]$BootstrapSha256,
    [Parameter(Mandatory)][string]$EvidenceDirectory
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (![Environment]::Is64BitProcess -or $env:PROCESSOR_ARCHITECTURE -cne 'AMD64') { throw 'Use native x64 Windows.' }
$local = [Environment]::GetFolderPath('LocalApplicationData')
$data = Join-Path $local 'Threadline\Data'
$installed = Join-Path $local 'Programs\Threadline Installer Test'
$app = Join-Path $installed 'threadline.exe'
$uninstaller = Join-Path $installed 'Uninstall.exe'
$record = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Threadline.InstallTest'
$server = 'HKCU:\Software\Classes\CLSID\{df3bb9a2-0794-44a6-b130-a35197093afe}\LocalServer32'
$sender = 'HKCU:\Software\Classes\AppUserModelId\Threadline.InstallTest'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Threadline Installer Test.lnk'

function Assert-Ordinary([string]$Path) {
    $item = Get-Item -LiteralPath $Path -Force
    while ($item) {
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'A test path traverses a reparse point.' }
        $item = if ($item -is [IO.FileInfo]) { $item.Directory } else { $item.Parent }
    }
}
function Get-Digest([string]$Path) {
    Assert-Ordinary $Path
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}
function Get-Snapshot([string[]]$Arguments) {
    if ((Get-Digest $Fixture) -ine $FixtureSha256) { throw 'The pinned fixture helper changed.' }
    $output = & $Fixture @Arguments
    if ($LASTEXITCODE -ne 0) { throw 'The package fixture check failed. Preserve its workspace for inspection.' }
    $snapshot = $output | ConvertFrom-Json
    if ($snapshot.status -cne 'Pass') { throw 'The helper returned an incomplete fixture receipt.' }
    return $snapshot
}
function Get-ClosedDigest([string]$Directory) {
    Assert-Ordinary $Directory
    if (@(Get-ChildItem -LiteralPath $Directory -Force).Count -ne 2) { throw 'Unknown profile entries were preserved.' }
    $marker = Join-Path $Directory 'threadline-package-retention.owner'
    Assert-Ordinary $marker
    if ([IO.File]::ReadAllText($marker) -cne ('package-retention-v1/'+$nonce)) { throw 'The owner marker changed.' }
    return Get-Digest (Join-Path $Directory 'threadline.sqlite3')
}
function Assert-Seed($snapshot) {
    if (!$snapshot.protected_credential_present -or !$snapshot.profile_bound -or !$snapshot.work_offline -or
        !$snapshot.outbox_paused -or $snapshot.saved_comment_page_cursor -cne '2' -or $snapshot.queued_operations -ne 8 -or
        $snapshot.step_receipts -ne 1 -or $snapshot.drafts -ne 1 -or $snapshot.durable_projection_sha256 -cne $before.durable_projection_sha256) {
        throw 'The protected profile, pending operations, receipt or cursor changed.'
    }
}
function Write-LocalReceipt([string]$Name,$Document) {
    $stream = [IO.File]::Open((Join-Path $EvidenceDirectory $Name),[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try {
        $bytes = [Text.Encoding]::UTF8.GetBytes(($Document | ConvertTo-Json -Depth 10))
        $stream.Write($bytes,0,$bytes.Length)
    } finally { $stream.Dispose() }
}
function New-RetentionRoot([string]$LocalDirectory,[string]$TempDirectory,[string]$RunId) {
    Assert-Ordinary $LocalDirectory
    Assert-Ordinary $TempDirectory
    $profileParent = Join-Path $LocalDirectory 'Threadline'
    if (Test-Path -LiteralPath $profileParent) { Assert-Ordinary $profileParent }
    $directory = Join-Path $TempDirectory ('threadline-package-retention-'+$RunId)
    $null = New-Item -ItemType Directory -Path $directory -ErrorAction Stop
    Assert-Ordinary $directory
    return $directory
}

Assert-Ordinary $EvidenceDirectory
if (@($data,$installed,$record,$shortcut,$server,$sender | Where-Object {Test-Path -LiteralPath $_}).Count -gt 0 -or
    (Get-Process -Name threadline -ErrorAction SilentlyContinue)) { throw 'Use a dedicated Windows test user with no existing data or test install.' }
foreach ($asset in @(@($Fixture,$FixtureSha256),@($Bootstrap,$BootstrapSha256),@($Predecessor,$PredecessorSha256),
    @($Current,$CurrentSha256),@($StageFailure,$StageFailureSha256))) {
    if ($asset[1] -cnotmatch '^[0-9A-Fa-f]{64}$' -or (Get-Digest $asset[0]) -ine $asset[1]) { throw 'A fixture asset differs from its independent pin.' }
}
$metadata = Get-Content -Raw -LiteralPath ($Current+'.json') | ConvertFrom-Json
if ($metadata.channel -cne 'local-fixture' -or $metadata.sha256 -ine $CurrentSha256 -or $metadata.fixture_fault -cne 'none') {
    throw 'Use the scoped local fixture package.'
}
$parent = Split-Path -Parent $data
$nonce = [guid]::NewGuid().ToString('N')
$root = New-RetentionRoot $local ([IO.Path]::GetTempPath()) $nonce
$seed = Join-Path $root 'profile'
$archive = Join-Path $root 'uninstalled-owned-data'
$before = $null
$enteredDefault = $false
$stage = 'seed'
try {
    $before = Get-Snapshot @('seed',$nonce)
    Assert-Seed $before
    $baseline = Get-ClosedDigest $seed
    $staging = Join-Path $root 'enter-default'
    $null = New-Item -ItemType Directory -Path $staging
    foreach ($file in @('threadline-package-retention.owner','threadline.sqlite3')) {
        [IO.File]::Copy((Join-Path $seed $file),(Join-Path $staging $file),$false)
    }
    if ((Get-ClosedDigest $staging) -cne $baseline) { throw 'The fresh fixture copy differs.' }
    Assert-Ordinary $local
    if (!(Test-Path -LiteralPath $parent)) {
        $null = New-Item -ItemType Directory -Path $parent -ErrorAction Stop
    }
    Assert-Ordinary $parent
    [IO.Directory]::Move($staging,$data)
    $enteredDefault = $true
    $stage = 'ordinary package matrix'
    $inspectedSteps = [Collections.Generic.List[string]]::new()
    $matrix = & (Join-Path $PSScriptRoot 'test-local-lifecycle.ps1') -Predecessor $Predecessor -PredecessorSha256 $PredecessorSha256 `
        -Current $Current -CurrentSha256 $CurrentSha256 -StageFailure $StageFailure -StageFailureSha256 $StageFailureSha256 `
        -ProfileSha256 $baseline -AfterRetainedStep {
            param($step)
            Assert-Seed (Get-Snapshot @('inspect-default',$nonce))
            $inspectedSteps.Add($step)
        }
    $matrixReport = ($matrix -join [Environment]::NewLine) | ConvertFrom-Json
    if ($matrixReport.completed_steps.Count -ne 10 -or
        ($inspectedSteps -join '/') -cne ($matrixReport.completed_steps.step -join '/') -or
        (Get-ClosedDigest $data) -cne $baseline) { throw 'Incomplete normal lifecycle or changed database.' }
    $stage = 'installed native reader'
    $native = & (Join-Path $PSScriptRoot 'smoke-installed.ps1') -ExpectedExecutableSha256 $metadata.app_sha256
    $opened = ($native -join [Environment]::NewLine) | ConvertFrom-Json
    if (!$opened.owned_window -or !$opened.tab_activated -or $opened.selected_before_invocation -or !$opened.orderly_exit) { throw 'The installed fixture did not activate its inactive tab.' }
    $closed = Get-ClosedDigest $data
    Assert-Seed (Get-Snapshot @('inspect-default',$nonce))
    $registered = Get-ItemProperty -LiteralPath $record
    $stage = 'final ordinary uninstall'
    if ($registered.InstallLocation -ine $installed -or $registered.UninstallString -ine ('"'+$uninstaller+'"') -or
        (Get-Digest $app) -ine $metadata.app_sha256 -or (Get-Process -Name threadline -ErrorAction SilentlyContinue)) { throw 'The installed fixture identity changed.' }
    Assert-Ordinary $uninstaller
    $remove = Start-Process -FilePath $uninstaller -ArgumentList '/S' -PassThru
    try {
        if (!$remove.WaitForExit(120000) -or $remove.ExitCode -ne 0) { throw 'The final fixture uninstall failed.' }
    } finally { $remove.Dispose() }
    $watch = [Diagnostics.Stopwatch]::StartNew()
    while (@($installed,$record,$shortcut,$server,$sender | Where-Object {Test-Path -LiteralPath $_}).Count -gt 0) {
        if ($watch.ElapsedMilliseconds -gt 20000) { throw 'Installed fixture registration remains.' }
        Start-Sleep -Milliseconds 100
    }
    if ((Get-ClosedDigest $data) -cne $closed) { throw 'Default uninstall changed the closed workspace.' }
    $retained = Get-Snapshot @('inspect-default',$nonce)
    Assert-Seed $retained
    [IO.Directory]::Move($data,$archive)
    $key = Get-Snapshot @('cleanup-key',$nonce)
    $stage = 'copied checkpoint resume'
    $resume = Get-Snapshot @('resume',$archive,$nonce,$closed)
    if ((Get-Digest $Bootstrap) -ine $BootstrapSha256) { throw 'The pinned bootstrap changed.' }
    $stage = 'separate confirmed cleanup'
    $confirmed = Get-Snapshot @('confirm',$Bootstrap,$BootstrapSha256,$archive,$nonce,$closed)
    if (!$key.protected_fixture_key_absent -or !$resume.archived_bytes_unchanged -or !$confirmed.default_profile_absent_before_and_after -or
        (Get-ClosedDigest $archive) -cne $closed -or (Test-Path -LiteralPath $data)) { throw 'Fixture cleanup or archive validation failed.' }
    Write-LocalReceipt 'windows-retained-package.json' ([ordered]@{status='Pass';fixture_sha256=$FixtureSha256;bootstrap_sha256=$BootstrapSha256;
        current_sha256=$CurrentSha256;retained_at_steps=$inspectedSteps.ToArray();native=$opened;
        initial_database_sha256=$baseline;closed_database_sha256=$closed;before=$before;after_default_uninstall=$retained;
        copied_checkpoint=$resume;separate_confirmed_removal=$confirmed;default_data_absent=$true})
    $matrixReport | ConvertTo-Json -Depth 4
} catch {
    $failure = $_
    if ($enteredDefault -and (Test-Path -LiteralPath $data)) {
        try {
            if (!(Get-Process -Name threadline -ErrorAction SilentlyContinue)) {
                $null = Get-ClosedDigest $data
                Assert-Seed (Get-Snapshot @('inspect-default',$nonce))
                [IO.Directory]::Move($data,(Join-Path $root 'retained-after-interruption'))
            }
        } catch {
            Write-Warning 'The default workspace was left in place because ownership or durable state could not be confirmed.'
        }
    }
    $defaultAbsent = !(Test-Path -LiteralPath $data)
    try {
        $seedMarker=Join-Path $seed 'threadline-package-retention.owner'
        if (($defaultAbsent -or !$enteredDefault) -and (Test-Path -LiteralPath $seedMarker -PathType Leaf)) {
            Assert-Ordinary $seedMarker
            if ([IO.File]::ReadAllText($seedMarker) -ceq ('package-retention-v1/'+$nonce) -and
                (Test-Path -LiteralPath (Join-Path $seed 'threadline.sqlite3') -PathType Leaf)) {
                $null = Get-Snapshot @('cleanup-key',$nonce)
            }
        }
    } finally {
        Write-LocalReceipt 'windows-retained-package-failure.json' ([ordered]@{status='Fail';stage=$stage;default_data_absent=$defaultAbsent})
    }
    throw $failure
}
