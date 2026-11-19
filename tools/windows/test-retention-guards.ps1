[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$tokens=$null
$errors=$null
$source = Join-Path $PSScriptRoot 'test-retained-package.ps1'
$ast = [Management.Automation.Language.Parser]::ParseFile($source,[ref]$tokens,[ref]$errors)
if ($errors.Count) { throw 'The retention driver did not parse.' }
$helpers = $ast.FindAll({param($node)
    $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -cin @(
        'Assert-Ordinary','New-RetentionRoot','Get-Digest','Get-ClosedDigest','Copy-CheckedFixture','Assert-SameSchemaSeeder')
},$false)
if ($helpers.Count -ne 6) { throw 'The retention preflight functions did not load.' }
. ([scriptblock]::Create(($helpers | ForEach-Object {$_.Extent.Text}) -join [Environment]::NewLine))

$scratch = Join-Path ([IO.Path]::GetTempPath()) ('threadline-retention-guard-'+[guid]::NewGuid().ToString('N'))
Assert-Ordinary ([IO.Path]::GetTempPath())
$null = New-Item -ItemType Directory -Path $scratch
$real = Join-Path $scratch 'ordinary'
$link = Join-Path $scratch 'junction'
$created = Join-Path $real 'threadline-package-retention-fixture'
$copy = Join-Path $scratch 'backup-copy'
$nonce = [guid]::NewGuid().ToString('N')
$FromSchema = 15
$backup = 'threadline-schema-15-ABC123.sqlite3'
$files = @('threadline-package-retention.owner','threadline.sqlite3',$backup,'unknown.txt')
try {
    $null = New-Item -ItemType Directory -Path $real
    $null = New-Item -ItemType Junction -Path $link -Target $real
    foreach ($test in @(@($link,$real),@($real,$link))) {
        $refused=$false
        try { $null=New-RetentionRoot $test[0] $test[1] 'fixture' } catch { $refused=$_.Exception.Message -like '*reparse point*' }
        if (!$refused -or (Test-Path -LiteralPath $created)) { throw 'Redirected ancestry created a test directory.' }
    }
    $actual = New-RetentionRoot $real $real 'fixture'
    if ($actual -ine $created) { throw 'An ordinary test root was not accepted.' }
    [IO.File]::WriteAllText((Join-Path $created $files[0]),('package-retention-v1/'+$nonce))
    [IO.File]::WriteAllText((Join-Path $created $files[1]),'candidate bytes')
    [IO.File]::WriteAllText((Join-Path $created $backup),'previous bytes')
    $currentFile = Join-Path $created $files[1]
    $priorFile = Join-Path $created $backup
    $oldPin = Get-Digest $priorFile
    Assert-SameSchemaSeeder $currentFile $priorFile $oldPin 0 0
    Assert-SameSchemaSeeder $currentFile '' '' 0 0
    foreach ($invalid in @(
        @($priorFile,(Get-Digest $currentFile),0,0),
        @($currentFile,(Get-Digest $currentFile),0,0),
        @($priorFile,'',0,0),@('',$oldPin,0,0),@($priorFile,$oldPin,18,18)
    )) {
        $rejected = $false
        try { Assert-SameSchemaSeeder $currentFile @invalid } catch {
            $rejected = $_.Exception.Message -in @('Pin a separate seeder from the predecessor package.',
                'The declared migration does not match the package schemas.')
        }
        if (!$rejected) { throw 'A same-schema fixture guard accepted invalid inputs.' }
    }
    if ((Get-ClosedDigest $created $backup) -cne (Get-Digest (Join-Path $created $files[1]))) { throw 'Current database bytes mismatched.' }
    Copy-CheckedFixture $created $backup $copy (Get-Digest (Join-Path $created $backup))
    [IO.File]::WriteAllText((Join-Path $created $files[3]),'preserve')
    $refused = $false
    try { $null = Get-ClosedDigest $created $backup } catch { $refused = $_.Exception.Message -like '*Unknown profile entries*' }
    if (!$refused -or [IO.File]::ReadAllText((Join-Path $created $files[3])) -cne 'preserve') { throw 'Unexpected contents were not preserved.' }
    [ordered]@{status='Pass';redirected_local_refused=$true;redirected_temp_refused=$true;ordinary_created=$true;
        copied_exact_backup=$true;unrecognized_file_preserved=$true;same_schema_seeder_pin_validated=$true;
        invalid_seeder_inputs_rejected=5} |
        ConvertTo-Json -Compress
} finally {
    if (Test-Path -LiteralPath $link) { [IO.Directory]::Delete($link) }
    foreach ($filename in $files) { [IO.File]::Delete((Join-Path $created $filename)) }
    foreach ($filename in $files[0..1]) { [IO.File]::Delete((Join-Path $copy $filename)) }
    if (Test-Path -LiteralPath $copy) { [IO.Directory]::Delete($copy) }
    if (Test-Path -LiteralPath $created) { [IO.Directory]::Delete($created) }
    if (Test-Path -LiteralPath $real) { [IO.Directory]::Delete($real) }
    [IO.Directory]::Delete($scratch)
}
