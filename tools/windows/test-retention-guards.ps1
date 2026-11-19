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
    $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -cin @('Assert-Ordinary','New-RetentionRoot')
},$false)
if ($helpers.Count -ne 2) { throw 'The retention preflight functions did not load.' }
. ([scriptblock]::Create(($helpers | ForEach-Object {$_.Extent.Text}) -join [Environment]::NewLine))

$scratch = Join-Path ([IO.Path]::GetTempPath()) ('threadline-retention-guard-'+[guid]::NewGuid().ToString('N'))
Assert-Ordinary ([IO.Path]::GetTempPath())
$null = New-Item -ItemType Directory -Path $scratch
$real = Join-Path $scratch 'ordinary'
$link = Join-Path $scratch 'junction'
$created = Join-Path $real 'threadline-package-retention-fixture'
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
    [ordered]@{status='Pass';redirected_local_refused=$true;redirected_temp_refused=$true;ordinary_created=$true} |
        ConvertTo-Json -Compress
} finally {
    if (Test-Path -LiteralPath $link) { [IO.Directory]::Delete($link) }
    if (Test-Path -LiteralPath $created) { [IO.Directory]::Delete($created) }
    if (Test-Path -LiteralPath $real) { [IO.Directory]::Delete($real) }
    [IO.Directory]::Delete($scratch)
}
