[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Provision','PublicKey','Sign')][string]$Action,
    [string]$Installer,
    [string]$OutputDirectory,
    [string]$ExpectedPublicKey,
    [int]$OldestSupportedSchema,
    [ValidatePattern('^Threadline\.PrivateRelease\.(Manifest\.v1|Test\.[a-f0-9]{32})$')]
    [string]$KeyName = 'Threadline.PrivateRelease.Manifest.v1'
)

Set-StrictMode -Version 3.0
$ErrorActionPreference = 'Stop'
$provider = [Security.Cryptography.CngProvider]::MicrosoftSoftwareKeyStorageProvider
$userKey = [Security.Cryptography.CngKeyOpenOptions]::None
$algorithm = [Security.Cryptography.CngAlgorithm]::ECDsaP256
$key = $null
$signer = $null
$sha = $null
$staging = $null
try {
    if ($Action -eq 'Provision' -and ![Security.Cryptography.CngKey]::Exists($KeyName, $provider, $userKey)) {
        $options = [Security.Cryptography.CngKeyCreationParameters]::new()
        $options.Provider = $provider
        $options.KeyUsage = [Security.Cryptography.CngKeyUsages]::Signing
        $options.ExportPolicy = [Security.Cryptography.CngExportPolicies]::None
        $key = [Security.Cryptography.CngKey]::Create($algorithm, $KeyName, $options)
    } else {
        $key = [Security.Cryptography.CngKey]::Open($KeyName, $provider, $userKey)
    }
    if ($key.Algorithm -ne $algorithm -or $key.IsMachineKey -or
        $key.ExportPolicy -ne [Security.Cryptography.CngExportPolicies]::None -or
        $key.KeyUsage -ne [Security.Cryptography.CngKeyUsages]::Signing) {
        throw 'Select a nonexportable current-user P-256 manifest signing key.'
    }
    $blob = $key.Export([Security.Cryptography.CngKeyBlobFormat]::EccPublicBlob)
    if ($blob.Length -ne 72 -or [Text.Encoding]::ASCII.GetString($blob, 0, 4) -cne 'ECS1' -or
        [BitConverter]::ToUInt32($blob, 4) -ne 32) { throw 'Unexpected P-256 public key format.' }
    $publicKey = '04' + [BitConverter]::ToString($blob, 8, 64).Replace('-', '').ToLowerInvariant()
    if ($Action -ne 'Sign') {
        $publicKey
        return
    }
    if ($ExpectedPublicKey -notmatch '^04[0-9a-f]{128}$' -or $ExpectedPublicKey -ine $publicKey) {
        throw 'The signing key differs from the expected app public key.'
    }
    if (!$Installer -or !$OutputDirectory -or $OldestSupportedSchema -le 0) {
        throw 'Supply the installer, a new output directory, and the oldest supported database schema.'
    }
    $setup = Get-Item -LiteralPath $Installer
    $meta = Get-Content -LiteralPath ($setup.FullName + '.json') -Raw | ConvertFrom-Json
    $build = [int]$meta.package_build
    $version = [string]$meta.package_version
    $name = "Threadline-Setup-$build.exe"
    if ($build -lt 1 -or $build -gt 999999999 -or $setup.Name -cne $name -or
        $version -cnotmatch ("^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-local\." + $build + '$') -or
        $meta.channel -cne 'local-fixture' -or $meta.fixture_fault -cne 'none' -or
        $meta.target -cne 'x86_64-pc-windows-msvc' -or $meta.minimum_os -cne 'Windows 11 x64 build 22000' -or
        $meta.restart_protocol -ne 1 -or $meta.private_update_public_key -ine $publicKey -or
        $meta.current_schema -lt $OldestSupportedSchema -or $meta.app_sha256 -notmatch '^[a-f0-9]{64}$') {
        throw 'The installer metadata does not describe an eligible private Windows update.'
    }
    $destination = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
    if (Test-Path -LiteralPath $destination) { throw 'Use a new output directory for the signed release.' }
    $parent = (Get-Item -LiteralPath ([IO.Path]::GetDirectoryName($destination))).FullName
    $staging = Join-Path $parent ('.threadline-release-' + [guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($staging)
    $copy = Join-Path $staging $name
    [IO.File]::Copy($setup.FullName, $copy)
    $size = (Get-Item -LiteralPath $copy).Length
    $digest = (Get-FileHash -LiteralPath $copy -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($size -le 0 -or $size -ne $meta.size -or $digest -ine $meta.sha256) {
        throw 'The staged installer differs from its build metadata.'
    }
    $manifest = [ordered]@{
        version = $version
        package_build = $build
        channel = 'private-preview'
        target = $meta.target
        minimum_os = $meta.minimum_os
        current_schema = [int]$meta.current_schema
        oldest_supported_schema = $OldestSupportedSchema
        restart_protocol = 1
        artifact_url = "https://github.com/joshmarkovic/threadline/releases/download/v$version/$name"
        size = $size
        sha256 = $digest
    }
    $bytes = [Text.Encoding]::UTF8.GetBytes(($manifest | ConvertTo-Json -Compress) + [char]10)
    $sha = [Security.Cryptography.SHA256]::Create()
    $hash = $sha.ComputeHash($bytes)
    $signer = [Security.Cryptography.ECDsaCng]::new($key)
    $signature = $signer.SignHash($hash)
    if ($signature.Length -ne 64 -or !$signer.VerifyHash($hash, $signature)) {
        throw 'Windows could not verify the release manifest signature.'
    }
    $manifestPath = Join-Path $staging 'threadline-private-windows-x64.json'
    [IO.File]::WriteAllBytes($manifestPath, $bytes)
    [IO.File]::WriteAllBytes(($manifestPath + '.sig'), $signature)
    [IO.Directory]::Move($staging, $destination)
    $staging = $null
    $destination
} finally {
    if ($sha) { $sha.Dispose() }
    if ($signer) { $signer.Dispose() }
    if ($key) { $key.Dispose() }
    if ($staging -and [IO.Directory]::Exists($staging)) { [IO.Directory]::Delete($staging, $true) }
}
