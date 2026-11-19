[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$release = Join-Path (Split-Path -Parent $PSScriptRoot) 'private-release.ps1'
$keyName = 'Threadline.PrivateRelease.Test.' + [guid]::NewGuid().ToString('N')
$root = Join-Path ([IO.Path]::GetTempPath()) ('ThreadlineSigning-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($root)
$provider = [Security.Cryptography.CngProvider]::MicrosoftSoftwareKeyStorageProvider
$public = $null
$verifier = $null
$hasher = $null
try {
    $pin = & $release -Action Provision -KeyName $keyName
    if ($pin -cnotmatch '^04[0-9a-f]{128}$' -or (& $release -Action PublicKey -KeyName $keyName) -cne $pin -or
        (& $release -Action Provision -KeyName $keyName) -cne $pin) { throw 'Unexpected public key or key replacement.' }
    $setup = Join-Path $root 'Threadline-Setup-21.exe'
    [IO.File]::WriteAllBytes($setup, [Text.Encoding]::ASCII.GetBytes('MZ-synthetic-payload'))
    $meta = @{
        package_version = '0.1.0-local.21'
        package_build = 21
        channel = 'local-fixture'
        fixture_fault = 'none'
        target = 'x86_64-pc-windows-msvc'
        minimum_os = 'Windows 11 x64 build 22000'
        restart_protocol = 1
        current_schema = 18
        app_sha256 = 'a' * 64
        private_update_public_key = $pin
        sha256 = (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash.ToLowerInvariant()
        size = (Get-Item -LiteralPath $setup).Length
    }
    $meta | ConvertTo-Json | Set-Content -Encoding ASCII -LiteralPath ($setup + '.json')
    $output = Join-Path $root 'release'
    $arguments = @{Action='Sign'; KeyName=$keyName; Installer=$setup; ExpectedPublicKey=$pin; OldestSupportedSchema=18}
    [void](& $release @arguments -OutputDirectory $output)
    $manifest = Join-Path $output 'threadline-private-windows-x64.json'
    $bytes = [IO.File]::ReadAllBytes($manifest)
    $sig = [IO.File]::ReadAllBytes(($manifest + '.sig'))
    $data = [Text.Encoding]::UTF8.GetString($bytes) | ConvertFrom-Json
    $packaged = Get-FileHash -LiteralPath (Join-Path $output 'Threadline-Setup-21.exe') -Algorithm SHA256
    if (@(Get-ChildItem -LiteralPath $output).Count -ne 3 -or $bytes[0] -ne 123 -or $bytes[-1] -ne 10 -or
        $sig.Length -ne 64 -or $data.channel -cne 'private-preview' -or $data.version -cne $meta.package_version -or
        $data.size -ne $meta.size -or $data.package_build -ne 21 -or $data.restart_protocol -ne 1 -or
        $data.current_schema -ne 18 -or $data.oldest_supported_schema -ne 18 -or $packaged.Hash -ine $data.sha256 -or
        $data.artifact_url -cne 'https://github.com/joshmarkovic/threadline/releases/download/v0.1.0-local.21/Threadline-Setup-21.exe') {
        throw 'The release assets differ from the private updater protocol.'
    }
    $blob = [byte[]]::new(72)
    [Text.Encoding]::ASCII.GetBytes('ECS1').CopyTo($blob, 0)
    $blob[4] = 32
    for ($i = 0; $i -lt 64; $i++) { $blob[8 + $i] = [Convert]::ToByte($pin.Substring(2 + 2 * $i, 2), 16) }
    $public = [Security.Cryptography.CngKey]::Import($blob, [Security.Cryptography.CngKeyBlobFormat]::EccPublicBlob)
    $verifier = [Security.Cryptography.ECDsaCng]::new($public)
    $hasher = [Security.Cryptography.SHA256]::Create()
    if (!$verifier.VerifyHash($hasher.ComputeHash($bytes), $sig)) { throw 'Independent public-key verification failed.' }
    $bytes[0] = 0
    if ($verifier.VerifyHash($hasher.ComputeHash($bytes), $sig)) { throw 'A changed manifest passed verification.' }
    $reject = Join-Path $root 'invalid'
    foreach ($reason in 'wrong key','modified installer') {
        if ($reason -eq 'wrong key') { $arguments.ExpectedPublicKey = '04' + '0' * 128 }
        else {
            $arguments.ExpectedPublicKey = $pin
            [IO.File]::WriteAllBytes($setup, [Text.Encoding]::ASCII.GetBytes('MZ-synthetic-payloaX'))
        }
        $failed = $false
        try { [void](& $release @arguments -OutputDirectory $reject) } catch { $failed = $true }
        if (!$failed -or (Test-Path -LiteralPath $reject)) { throw "Signing accepted $reason." }
    }
    Write-Output 'Private release signing, public-key verification and tamper checks passed.'
} finally {
    if ($verifier) { $verifier.Dispose() }
    if ($public) { $public.Dispose() }
    if ($hasher) { $hasher.Dispose() }
    if ([Security.Cryptography.CngKey]::Exists($keyName, $provider)) {
        $testKey = [Security.Cryptography.CngKey]::Open($keyName, $provider)
        try { $testKey.Delete() } finally { $testKey.Dispose() }
    }
    [IO.Directory]::Delete($root, $true)
}
