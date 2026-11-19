[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Installer,
    [Parameter(Mandatory)][string]$StagedMetadata,
    [Parameter(Mandatory)][ValidatePattern('^[0-9A-Fa-f]{64}$')][string]$ExpectedStageMetadataSha256,
    [Parameter(Mandatory)][ValidatePattern('^[0-9A-Fa-f]{64}$')][string]$ExpectedSignedSetupSha256,
    [Parameter(Mandatory)][ValidatePattern('^[0-9A-Fa-f]{64}$')][string]$ExpectedPublisherSha256,
    [Parameter(Mandatory)][string]$OutputDirectory
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$stageFile = Get-Item -LiteralPath $StagedMetadata
$stageBytes = [IO.File]::ReadAllBytes($stageFile.FullName)
$hash = [Security.Cryptography.SHA256]::Create()
try {
    $stagePin = [BitConverter]::ToString($hash.ComputeHash($stageBytes)).Replace('-', '')
} finally {
    $hash.Dispose()
}
if ($stagePin -ine $ExpectedStageMetadataSha256) { throw 'The staged metadata differs from the reviewed package build.' }
$stage = [Text.Encoding]::UTF8.GetString($stageBytes) | ConvertFrom-Json
if ($stage.status -cne 'publication_pending' -or $stage.restart_protocol -ne 1 -or $stage.publisher_certificate_sha256 -ine $ExpectedPublisherSha256) {
    throw 'The release staging record does not match the approved publisher configuration.'
}
$artifact = Get-Item -LiteralPath $Installer
$directory = Get-Item -LiteralPath $OutputDirectory
if (!$directory.PSIsContainer) { throw 'Choose an existing directory for the verified metadata.' }
$final = Join-Path $directory.FullName 'threadline-windows-x64.json'
$temporary = Join-Path $directory.FullName ('.threadline-manifest-' + [Guid]::NewGuid().ToString('N') + '.tmp')
$manifest = [ordered]@{
    version = $stage.version
    channel = $stage.channel
    target = $stage.target
    minimum_os = $stage.minimum_os
    artifact_url = $stage.artifact_url
    package_build = $stage.package_build
    current_schema = $stage.current_schema
    restart_protocol = $stage.restart_protocol
    oldest_supported_schema = $stage.oldest_supported_schema
    publisher_certificate_sha256 = $ExpectedPublisherSha256.ToLowerInvariant()
    size = $artifact.Length
    sha256 = $ExpectedSignedSetupSha256.ToLowerInvariant()
}
$json = ($manifest | ConvertTo-Json) + [Environment]::NewLine
$bytes = [Text.UTF8Encoding]::new($false).GetBytes($json)
$file = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
try {
    try { $file.Write($bytes, 0, $bytes.Length) } finally { $file.Dispose() }
    $proof = & (Join-Path $PSScriptRoot 'verify-release-installer.ps1') -Installer $artifact.FullName -TrustedManifest $temporary
    if ($proof.verified -ne $true) { throw 'The signed installer failed release verification.' }
    [IO.File]::Move($temporary, $final)
} finally {
    if ([IO.File]::Exists($temporary)) { [IO.File]::Delete($temporary) }
}
(Get-FileHash -LiteralPath $final -Algorithm SHA256).Hash.ToLowerInvariant()
