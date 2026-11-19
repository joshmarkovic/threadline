[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Predecessor, [Parameter(Mandatory)][string]$PredecessorSha256,
    [Parameter(Mandatory)][string]$Current, [Parameter(Mandatory)][string]$CurrentSha256,
    [Parameter(Mandatory)][string]$StageFailure, [Parameter(Mandatory)][string]$StageFailureSha256,
    [Parameter(Mandatory)][string]$Fixture, [Parameter(Mandatory)][string]$FixtureSha256,
    [Parameter(Mandatory)][string]$Bootstrap, [Parameter(Mandatory)][string]$BootstrapSha256,
    [string]$PredecessorFixture, [string]$PredecessorFixtureSha256, [uint32]$FromSchema, [uint32]$ToSchema,
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
function Get-Snapshot([string[]]$Arguments,[string]$Helper=$Fixture,[string]$Pin=$FixtureSha256) {
    if ((Get-Digest $Helper) -ine $Pin) { throw 'The pinned fixture helper changed.' }
    $output = & $Helper @Arguments
    if ($LASTEXITCODE -ne 0) { throw 'The package fixture check failed. Preserve its workspace for inspection.' }
    $snapshot = $output | ConvertFrom-Json
    if ($snapshot.status -cne 'Pass') { throw 'The helper returned an incomplete fixture receipt.' }
    return $snapshot
}
function Get-ClosedDigest([string]$Directory,[string]$BackupName='') {
    Assert-Ordinary $Directory
    $count = 2
    if ($BackupName) {
        if ($BackupName -cnotmatch ('^threadline-schema-'+$FromSchema+'-[A-Za-z0-9]{6}\.sqlite3$')) { throw 'Unexpected backup name.' }
        Assert-Ordinary (Join-Path $Directory $BackupName)
        $count++
    }
    if (@(Get-ChildItem -LiteralPath $Directory -Force).Count -ne $count) { throw 'Unknown profile entries were preserved.' }
    $marker = Join-Path $Directory 'threadline-package-retention.owner'
    Assert-Ordinary $marker
    if ([IO.File]::ReadAllText($marker) -cne ('package-retention-v1/'+$nonce)) { throw 'The owner marker changed.' }
    return Get-Digest (Join-Path $Directory 'threadline.sqlite3')
}
function Copy-CheckedFixture([string]$Source,[string]$Database,[string]$Destination,[string]$Digest) {
    $null = New-Item -ItemType Directory -Path $Destination -ErrorAction Stop
    $marker = 'threadline-package-retention.owner'
    [IO.File]::Copy((Join-Path $Source $marker),(Join-Path $Destination $marker),$false)
    [IO.File]::Copy((Join-Path $Source $Database),(Join-Path $Destination 'threadline.sqlite3'),$false)
    if ((Get-ClosedDigest $Destination) -cne $Digest) { throw 'The fresh fixture copy differs.' }
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
$previous = Get-Content -Raw -LiteralPath ($Predecessor+'.json') | ConvertFrom-Json
if ($metadata.channel -cne 'local-fixture' -or $metadata.sha256 -ine $CurrentSha256 -or $metadata.fixture_fault -cne 'none' -or
    $previous.channel -cne 'local-fixture' -or $previous.sha256 -ine $PredecessorSha256 -or $previous.fixture_fault -cne 'none' -or
    $previous.current_schema -lt 1 -or $previous.current_schema -gt $metadata.current_schema) {
    throw 'Use the scoped local fixture package.'
}
$migrates = $previous.current_schema -ne $metadata.current_schema
if ($migrates) {
    if ($FromSchema -ne $previous.current_schema -or $ToSchema -ne $metadata.current_schema -or !$PredecessorFixture -or
        $PredecessorFixtureSha256 -cnotmatch '^[A-Fa-f0-9]{64}$' -or $PredecessorFixtureSha256 -ieq $FixtureSha256 -or
        (Get-Digest $PredecessorFixture) -ine $PredecessorFixtureSha256) { throw 'Pin the predecessor seeder and both package schemas.' }
} elseif ($PredecessorFixture -or $PredecessorFixtureSha256 -or $FromSchema -or $ToSchema) {
    throw 'The declared migration does not match the package schemas.'
}
$parent = Split-Path -Parent $data
$nonce = [guid]::NewGuid().ToString('N')
$root = New-RetentionRoot $local ([IO.Path]::GetTempPath()) $nonce
$seed = Join-Path $root 'profile'
$archive = Join-Path $root 'uninstalled-owned-data'
$before = $null
$enteredDefault = $false
$backupName = ''
$migration = $null
$stage = 'seed'
try {
    $before = if ($migrates) { Get-Snapshot @('seed',$nonce) $PredecessorFixture $PredecessorFixtureSha256 }
              else { Get-Snapshot @('seed',$nonce) }
    if ($before.schema -ne $previous.current_schema) { throw 'The generated database does not match the predecessor schema.' }
    Assert-Seed $before
    $baseline = Get-ClosedDigest $seed
    $staging = Join-Path $root 'enter-default'
    Copy-CheckedFixture $seed 'threadline.sqlite3' $staging $baseline
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
    if ($migrates) {
        $backups = @(Get-ChildItem -LiteralPath $data -Force -File -Filter ('threadline-schema-'+$FromSchema+'-*.sqlite3'))
        if ($backups.Count -ne 1) { throw 'Expected one pre-migration database backup from the installed app.' }
        $backupName = $backups[0].Name
    }
    $closed = Get-ClosedDigest $data $backupName
    $openedState = Get-Snapshot @('inspect-default',$nonce)
    if ($openedState.schema -ne $metadata.current_schema) { throw 'Installed application did not open the advertised schema.' }
    Assert-Seed $openedState
    if ($migrates) {
        $backupHash = Get-Digest (Join-Path $data $backupName)
        Copy-CheckedFixture $data $backupName (Join-Path $root 'migration-backup-check') $backupHash
        $backupState = Get-Snapshot @('inspect-backup-copy',$nonce)
        if ($backupState.schema -ne $FromSchema) { throw 'The backup did not retain the predecessor schema.' }
        Assert-Seed $backupState
        $migration = [ordered]@{predecessor_fixture_sha256=$PredecessorFixtureSha256;backup_sha256=$backupHash;backup_snapshot=$backupState}
    }
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
    if ((Get-ClosedDigest $data $backupName) -cne $closed) { throw 'Default uninstall changed the closed workspace.' }
    $retained = Get-Snapshot @('inspect-default',$nonce)
    Assert-Seed $retained
    [IO.Directory]::Move($data,$archive)
    $checkpointSource = $archive
    if ($migrates) {
        $migration.backup_after_uninstall_sha256 = Get-Digest (Join-Path $archive $backupName)
        if ($migration.backup_after_uninstall_sha256 -cne $backupHash) { throw 'Default uninstall changed the migration backup.' }
        $copyRoot = Join-Path $root 'checkpoint-input'
        $null = New-Item -ItemType Directory -Path $copyRoot -ErrorAction Stop
        $checkpointSource = Join-Path $copyRoot 'uninstalled-owned-data'
        Copy-CheckedFixture $archive 'threadline.sqlite3' $checkpointSource $closed
    }
    $key = Get-Snapshot @('cleanup-key',$nonce)
    $stage = 'copied checkpoint resume'
    $resume = Get-Snapshot @('resume',$checkpointSource,$nonce,$closed)
    if ((Get-Digest $Bootstrap) -ine $BootstrapSha256) { throw 'The pinned bootstrap changed.' }
    $stage = 'separate confirmed cleanup'
    $confirmed = Get-Snapshot @('confirm',$Bootstrap,$BootstrapSha256,$checkpointSource,$nonce,$closed)
    if ($migrates) {
        $migration.backup_after_cleanup_sha256 = Get-Digest (Join-Path $archive $backupName)
        if ($migration.backup_after_cleanup_sha256 -cne $backupHash) { throw 'The archived migration backup changed.' }
    }
    if (!$key.protected_fixture_key_absent -or !$resume.archived_bytes_unchanged -or !$confirmed.default_profile_absent_before_and_after -or
        (Get-ClosedDigest $archive $backupName) -cne $closed -or (Test-Path -LiteralPath $data)) { throw 'Fixture cleanup or archive validation failed.' }
    Write-LocalReceipt 'windows-retained-package.json' ([ordered]@{status='Pass';fixture_sha256=$FixtureSha256;bootstrap_sha256=$BootstrapSha256;
        current_sha256=$CurrentSha256;retained_at_steps=$inspectedSteps.ToArray();native=$opened;
        initial_database_sha256=$baseline;closed_database_sha256=$closed;before=$before;after_default_uninstall=$retained;
        migration=$migration;copied_checkpoint=$resume;separate_confirmed_removal=$confirmed;default_data_absent=$true})
    $matrixReport | ConvertTo-Json -Depth 4
} catch {
    $failure = $_
    if ($enteredDefault -and (Test-Path -LiteralPath $data)) {
        try {
            if (!(Get-Process -Name threadline -ErrorAction SilentlyContinue)) {
                $null = Get-ClosedDigest $data $backupName
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
