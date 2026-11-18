[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [Parameter(Mandatory = $true)][string]$SignedFixtureManifest,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$FixturePublisherPin
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Security
$envelope = Get-Item -LiteralPath $SignedFixtureManifest
if ($envelope.PSIsContainer -or $envelope.Length -lt 1 -or $envelope.Length -gt 65536) {
    throw 'The local fixture signature receipt has an unexpected size.'
}
$cms = [Security.Cryptography.Pkcs.SignedCms]::new()
try {
    $cms.Decode([IO.File]::ReadAllBytes($envelope.FullName))
    if ($cms.SignerInfos.Count -ne 1) { throw 'The fixture must have one signer.' }
    $cms.CheckSignature($true)
} catch {
    throw 'The signed fixture metadata did not verify.'
}
$signer = $cms.SignerInfos[0].Certificate
if ($null -eq $signer) { throw 'The fixture signer certificate is absent.' }
$sha256 = [Security.Cryptography.SHA256]::Create()
try {
    $publisher = [BitConverter]::ToString($sha256.ComputeHash($signer.RawData)).Replace('-', '')
} finally {
    $sha256.Dispose()
}
if ($publisher -ine $FixturePublisherPin) { throw 'The signed fixture has the wrong publisher.' }

$metadata = [Text.Encoding]::UTF8.GetString($cms.ContentInfo.Content) | ConvertFrom-Json
if ($metadata.channel -cne 'local-fixture' -or $metadata.target -cne 'x86_64-pc-windows-msvc' -or
    $metadata.minimum_os -cne 'Windows 11 x64 build 22000' -or
    $metadata.app_sha256 -cnotmatch '^[0-9a-f]{64}$' -or $metadata.sha256 -cnotmatch '^[0-9a-f]{64}$' -or
    $metadata.package_build -lt 1 -or $metadata.size -lt 1) {
    throw 'The signed metadata does not describe a Windows local fixture.'
}
$package = Get-Item -LiteralPath $Installer
if ($package.PSIsContainer -or $package.Extension -ine '.exe' -or $package.Length -ne $metadata.size -or
    (Get-FileHash -LiteralPath $package.FullName -Algorithm SHA256).Hash -ine $metadata.sha256) {
    throw 'The installer does not match the signed fixture digest or size.'
}
[ordered]@{
    verified = $true
    channel = 'local-fixture'
    setup_sha256 = $metadata.sha256
    fixture_publisher_certificate_sha256 = $publisher.ToLowerInvariant()
} | ConvertTo-Json -Compress
