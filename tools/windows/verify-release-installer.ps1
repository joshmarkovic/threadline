[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [Parameter(Mandatory = $true)][string]$TrustedManifest
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$metadata = Get-Content -LiteralPath $TrustedManifest -Raw | ConvertFrom-Json
if ($metadata.target -cne 'x86_64-pc-windows-msvc' -or
    $metadata.minimum_os -cne 'Windows 11 x64 build 22000' -or
    $metadata.channel -cnotin @('stable', 'preview') -or
    $metadata.version -cnotmatch '^\d+\.\d+\.\d+([-+][A-Za-z0-9.-]+)?$' -or
    ($metadata.channel -ceq 'stable' -and $metadata.version -match '-') -or
    $metadata.sha256 -cnotmatch '^[0-9a-f]{64}$' -or
    $metadata.publisher_certificate_sha256 -cnotmatch '^[0-9a-f]{64}$' -or
    $metadata.size -le 0) {
    throw 'The trusted release metadata is missing required Windows identity or signing information.'
}

$artifact = Get-Item -LiteralPath $Installer
if ($artifact.PSIsContainer -or $artifact.Extension -ine '.exe' -or $artifact.Length -ne $metadata.size) {
    throw 'The installer does not match the trusted release size.'
}
if ((Get-FileHash -LiteralPath $artifact.FullName -Algorithm SHA256).Hash -ine $metadata.sha256) {
    throw 'The installer does not match the trusted release digest.'
}

$signature = Get-AuthenticodeSignature -LiteralPath $artifact.FullName
if ($signature.Status -ne 'Valid' -or $null -eq $signature.SignerCertificate) {
    throw 'Windows did not verify a trusted Authenticode signature for the installer.'
}
$hash = [Security.Cryptography.SHA256]::Create()
try {
    $publisher = [BitConverter]::ToString($hash.ComputeHash($signature.SignerCertificate.RawData)).Replace('-', '')
} finally {
    $hash.Dispose()
}
if ($publisher -ine $metadata.publisher_certificate_sha256) {
    throw 'The verified installer signer does not match the trusted publisher pin.'
}
[pscustomobject]@{
    verified = $true
    version = $metadata.version
    channel = $metadata.channel
    target = $metadata.target
    sha256 = $metadata.sha256
    publisher_certificate_sha256 = $metadata.publisher_certificate_sha256
}
