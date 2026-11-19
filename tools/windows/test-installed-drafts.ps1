[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$LifecycleExecutable,
    [Parameter(Mandatory)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$LifecycleSha256,
    [Parameter(Mandatory)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$InstalledSha256,
    [Parameter(Mandatory)][ValidateRange(1,65535)][int]$Schema,
    [Parameter(Mandatory)][string]$ReportDirectory,
    [switch]$DelayedOnline
)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
function Assert-Ordinary([string]$Path) {
    $item=Get-Item -Force -LiteralPath $Path
    while ($item) {
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'A generated workspace path is redirected.' }
        $item=if ($item -is [IO.FileInfo]) { $item.Directory } else { $item.Parent }
    }
}
function Get-Digest([string]$Path) {
    Assert-Ordinary $Path
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}
function Assert-ClosedFixture([string]$Directory) {
    Assert-Ordinary $Directory
    $entries=@(Get-ChildItem -LiteralPath $Directory -Force)
    $expected=@('installed-drafts.owner','native-lifecycle.owner','threadline.sqlite3')
    if ($entries.Count -ne 3) { throw 'Preserving unexpected workspace contents.' }
    foreach ($entry in $entries) {
        if ($entry.PSIsContainer -or $entry.Name -cnotin $expected) { throw 'Preserving an unexpected file.' }
        Assert-Ordinary $entry.FullName
    }
    if ([IO.File]::ReadAllText((Join-Path $Directory $expected[0])) -cne $nonce -or
        [IO.File]::ReadAllText((Join-Path $Directory $expected[1])) -cne 's12-native-lifecycle-v2') {
        throw 'The fixture markers differ. Preserve the workspace.'
    }
    $stream=[IO.File]::Open((Join-Path $Directory $expected[2]),'Open','Read','None')
    try {
        $header=New-Object byte[] 64
        if ($stream.Read($header,0,64) -ne 64 -or [Text.Encoding]::ASCII.GetString($header,0,16) -cne ("SQLite format 3"+[char]0)) {
            throw 'Expected the closed fixture database.'
        }
        $version=[byte[]]$header[60..63]
        if ([BitConverter]::IsLittleEndian) { [Array]::Reverse($version) }
        if ([BitConverter]::ToUInt32($version,0) -ne $Schema) { throw 'The profile is outside the selected same-schema check.' }
    } finally { $stream.Dispose() }
}
function Invoke-Fixture([string]$Stage,[string]$Name) {
    if ((Get-Digest $LifecycleExecutable) -ine $LifecycleSha256) { throw 'The pinned fixture executable changed.' }
    $path=Join-Path $ReportDirectory ($Name+'.json')
    if (Test-Path -LiteralPath $path) { throw 'Preserve the earlier fixture receipt.' }
    $info=[Diagnostics.ProcessStartInfo]::new()
    $info.UseShellExecute=$false
    $info.CreateNoWindow=$true
    $info.FileName=$LifecycleExecutable
    $info.Arguments="--e2e-native-lifecycle $Stage --e2e-report `"$path`""
    foreach ($key in @($info.EnvironmentVariables.Keys)) {
        if ($key -like 'THREADLINE_*') { $info.EnvironmentVariables.Remove($key) }
    }
    $info.EnvironmentVariables['THREADLINE_PROFILE_DIR']=$profileDirectory
    $info.EnvironmentVariables['THREADLINE_TEST_SEED']='12345'
    $info.EnvironmentVariables['THREADLINE_E2E_NATIVE_LIFECYCLE']='1'
    $info.EnvironmentVariables['THREADLINE_E2E_GITHUB_ADDR']=$loopback.LocalEndpoint.ToString()
    $child=[Diagnostics.Process]::Start($info)
    try {
        if (!$child.WaitForExit(30000)) { $child.Kill(); $child.WaitForExit(); throw 'The owned fixture helper timed out.' }
        if ($child.ExitCode -ne 0) { throw "The owned fixture check failed at $Stage." }
    } finally { $child.Dispose() }
    return [IO.File]::ReadAllText($path) | ConvertFrom-Json
}

$local=[Environment]::GetFolderPath('LocalApplicationData')
$temp=[IO.Path]::GetTempPath()
$ReportDirectory=(Get-Item -LiteralPath $ReportDirectory).FullName
$LifecycleExecutable=(Get-Item -LiteralPath $LifecycleExecutable).FullName
$parent=Join-Path $local 'Threadline'
$data=Join-Path $parent 'Data'
$installed=Join-Path $local 'Programs\Threadline Installer Test'
$exe=Join-Path $installed 'threadline.exe'
$registered=Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Threadline.InstallTest'
foreach ($base in @($temp,$local,$ReportDirectory)) { Assert-Ordinary $base }
if (Test-Path -LiteralPath $parent) { Assert-Ordinary $parent }
if ($registered.InstallLocation -ine $installed -or (Get-Digest $exe) -ine $InstalledSha256 -or
    (Get-Digest $LifecycleExecutable) -ine $LifecycleSha256 -or $LifecycleSha256 -ieq $InstalledSha256 -or
    (Test-Path -LiteralPath $data) -or (Get-Process -Name threadline -ErrorAction SilentlyContinue) -or
    @(Get-ChildItem -LiteralPath $ReportDirectory -Force).Count -gt 0) {
    throw 'Use the exact fixture installation, an empty report directory and an unused default profile.'
}
$nonce=[guid]::NewGuid().ToString('N')
$root=Join-Path $temp ('threadline-e2e-'+[guid]::NewGuid().ToString('N'))
$profileDirectory=Join-Path $root ('initial-'+$nonce)
$seedArchive=Join-Path $root 'original-seed'
$moved=$false
$complete=$false
$delayed=$null
$stage='create isolated fixture'
$loopback=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0)
if ($DelayedOnline) { . (Join-Path $PSScriptRoot 'installed-drafts-delay.ps1') }
try {
    $null=New-Item -ItemType Directory -Path $root -ErrorAction Stop
    $null=New-Item -ItemType Directory -Path $profileDirectory
    [IO.File]::WriteAllText((Join-Path $root 'access-fixture-phase.txt'),'lifecycle-offline')
    $loopback.Start()
    $seed=Invoke-Fixture 'seed-offline-outbox' 'before'
    if (!$seed.work_offline -or !$seed.pause_queued_actions -or !$seed.protected_token_present -or
        !$seed.protected_reference_matches -or ($seed.drafts.kind -join ',') -cne 'IssueEdit,Comment,Review,Reply' -or
        ($seed.actions -join ',') -cne 'Queued,Queued,Queued,Queued' -or $seed.sync_checkpoint.cursor -cne '2' -or
        ($seed.tab_counts -join ',') -cne '10,10' -or !$seed.review_anchor_preserved -or $seed.first_send_starts -ne 0) {
        throw 'The generated prior-version profile did not contain the required offline work.'
    }
    [IO.File]::WriteAllText((Join-Path $profileDirectory 'installed-drafts.owner'),$nonce)
    Assert-ClosedFixture $profileDirectory
    $initialHash=Get-Digest (Join-Path $profileDirectory 'threadline.sqlite3')
    $staged=Join-Path $root 'install-profile'
    $null=New-Item -ItemType Directory -Path $staged
    foreach ($file in @(Get-ChildItem -LiteralPath $profileDirectory -Force)) {
        [IO.File]::Copy($file.FullName,(Join-Path $staged $file.Name),$false)
    }
    Assert-ClosedFixture $staged
    if ((Get-Digest (Join-Path $staged 'threadline.sqlite3')) -cne $initialHash) { throw 'The staged copy differs.' }
    if (!(Test-Path -LiteralPath $parent)) { $null=New-Item -ItemType Directory -Path $parent }
    Assert-Ordinary $parent
    [IO.Directory]::Move($staged,$data)
    $moved=$true
    $stage='installed launches'
    $opened=@()
    if ($DelayedOnline) {
        $stage='installed offline and delayed online launches'
        $delayed=Invoke-InstalledDraftDelay
    } else {
        $smoke=Join-Path $PSScriptRoot 'smoke-installed.ps1'
        $opened=foreach ($tab in @('reader-tab-0-1','reader-tab-1-1')) {
            $result=(& $smoke -ExpectedExecutableSha256 $InstalledSha256 -TabId $tab | Out-String) | ConvertFrom-Json
            if (!$result.owned_window -or !$result.tab_activated -or !$result.orderly_exit) { throw 'The installed tab was not activated and closed normally.' }
            $result
        }
    }
    Assert-ClosedFixture $data
    [IO.Directory]::Move($profileDirectory,$seedArchive)
    [IO.Directory]::Move($data,$profileDirectory)
    $moved=$false
    $stage='retained mixed draft verification'
    $after=Invoke-Fixture 'inspect' 'after'
    if ($after.work_offline -ne !$DelayedOnline) { throw 'The installed fixture did not retain the tested connectivity setting.' }
    foreach ($field in @('drafts','actions','action_ids','sync_checkpoint','content','schedule','tab_counts','pane_proportions',
        'pause_queued_actions','protected_reference_matches','protected_token_present','review_anchor_preserved',
        'feed_event','feed_count','prior_delivery','delivery_groups','delivery_unchanged_at_ms','first_attempts','first_send_starts')) {
        $beforeValue=$seed.PSObject.Properties[$field].Value | ConvertTo-Json -Depth 8 -Compress
        $afterValue=$after.PSObject.Properties[$field].Value | ConvertTo-Json -Depth 8 -Compress
        if ($beforeValue -cne $afterValue) { throw "The installed reader changed this retained field: $field." }
    }
    if ((Get-Digest (Join-Path $seedArchive 'threadline.sqlite3')) -cne $initialHash) { throw 'The original archive changed.' }
    $complete=$true
} finally {
    try {
        if ($moved -and (Test-Path -LiteralPath $data) -and !(Get-Process -Name threadline -ErrorAction SilentlyContinue)) {
            Assert-ClosedFixture $data
            [IO.Directory]::Move($data,(Join-Path $root 'interrupted-owned-data'))
        }
    } finally {
        try {
            if (!(Test-Path -LiteralPath $profileDirectory) -and (Test-Path -LiteralPath $seedArchive)) {
                [IO.Directory]::Move($seedArchive,$profileDirectory)
            }
            if (Test-Path -LiteralPath (Join-Path $profileDirectory 'native-lifecycle.owner') -PathType Leaf) {
                $key=Invoke-Fixture 'cleanup' 'protected-key-cleanup'
                if (!$key.protected_fixture_key_removed) { throw 'The synthetic credential was not removed.' }
            }
        } finally { $loopback.Stop() }
    }
}
if (!$complete -or (Test-Path -LiteralPath $data)) { throw "The mixed-draft check did not finish: $stage." }
$receipt=[ordered]@{status='Pass';installed_app_sha256=$InstalledSha256.ToLowerInvariant();fixture_sha256=$LifecycleSha256.ToLowerInvariant();
    schema=$Schema;before_database_sha256=$initialHash;seed_archive_unchanged=$true;native_launches=$opened;
    drafts=$after.drafts;work_offline=$after.work_offline;outbox_paused=$after.pause_queued_actions;queued_states=$after.actions;
    provider_page_cursor=$after.sync_checkpoint.cursor;prior_delivery=$after.prior_delivery;review_anchor_preserved=$after.review_anchor_preserved;
    protected_credential_retained=$after.protected_token_present;protected_fixture_key_removed=$key.protected_fixture_key_removed;
    default_data_absent=$true}
if ($DelayedOnline) { $receipt['delayed_online']=$delayed }
$receipt | ConvertTo-Json -Depth 8
