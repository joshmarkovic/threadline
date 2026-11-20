# Exercise the setup notification helper's move from the former Start menu name.
# The helper is compiled with its test switch, so it works on scratch folders and a
# scratch registry key instead of the real Start menu and notification registration.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (![Environment]::Is64BitProcess -or [Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'Run this check in 64-bit Windows PowerShell.'
}
$csc = Join-Path ([Runtime.InteropServices.RuntimeEnvironment]::GetRuntimeDirectory()) 'csc.exe'
if (!(Test-Path -LiteralPath $csc -PathType Leaf)) { throw 'The .NET Framework C# compiler is unavailable.' }

$tag = [guid]::NewGuid().ToString('N')
$root = Join-Path ([IO.Path]::GetTempPath()) "threadline-helper-rename-$tag"
$data = Join-Path $root 'data'
$programs = Join-Path $root 'programs'
$keyName = "Software\Threadline.HelperTest.$tag"
$key = "HKCU:\$keyName"
$class = '{df3bb9a2-0794-44a6-b130-a35197093afe}'
$sender = "$key\AppUserModelId\Threadline.InstallTest"
$server = "$key\CLSID\$class\LocalServer32"
$app = Join-Path $data 'Programs\Threadline Installer Test\threadline.exe'
$command = '"' + $app + '" --notification-com-server'
$current = Join-Path $programs 'Threadline Preview.lnk'
$former = Join-Path $programs 'Threadline Installer Test.lnk'
$helper = Join-Path $root 'Threadline.NotificationRegistration.exe'
$runs = 0

function Assert-That([bool]$Condition, [string]$Message) {
    if (!$Condition) { throw "Notification rename check failed: $Message" }
}

# Each setup run unpacks the helper into its own empty folder, where it keeps its snapshot.
function New-SetupRun {
    $script:runs++
    $folder = Join-Path $root "run-$script:runs"
    $null = [IO.Directory]::CreateDirectory($folder)
    Copy-Item -LiteralPath $helper -Destination $folder
    Join-Path $folder 'Threadline.NotificationRegistration.exe'
}

function Invoke-Helper([string]$Executable, [string]$Mode, [int]$Expected = 0) {
    $child = Start-Process -FilePath $Executable -ArgumentList $Mode, 'Local' -Wait -PassThru -WindowStyle Hidden
    Assert-That ($child.ExitCode -eq $Expected) "$Mode exited with $($child.ExitCode), expected $Expected."
}

function Read-Value([string]$Path, [string]$Name) {
    $item = Get-Item -LiteralPath $Path -ErrorAction SilentlyContinue
    if ($null -eq $item) { return $null }
    $item.GetValue($Name, $null)
}

function Set-Shortcut([string]$Path, [string]$Target) {
    $shell = New-Object -ComObject WScript.Shell
    $shortcut = $shell.CreateShortcut($Path)
    $shortcut.TargetPath = $Target
    $shortcut.Save()
}

function Get-ShortcutTarget([string]$Path) {
    (New-Object -ComObject WScript.Shell).CreateShortcut($Path).TargetPath
}

function Reset-State([switch]$FormerInstall) {
    foreach ($path in $current, $former) {
        if (Test-Path -LiteralPath $path) { [IO.File]::Delete($path) }
    }
    if (Test-Path -LiteralPath $key) { Remove-Item -LiteralPath $key -Recurse -Force }
    if ($FormerInstall) {
        Set-Shortcut $former $app
        $null = New-Item -Path $server -Force
        Set-ItemProperty -LiteralPath $server -Name '(default)' -Value $command
        $null = New-Item -Path $sender -Force
        Set-ItemProperty -LiteralPath $sender -Name 'CustomActivator' -Value $class
        Set-ItemProperty -LiteralPath $sender -Name 'DisplayName' -Value 'Threadline Installer Test'
    }
}

function Assert-FormerInstall([string]$When) {
    Assert-That (Test-Path -LiteralPath $former) "the former shortcut is missing $When."
    Assert-That (!(Test-Path -LiteralPath $current)) "the new shortcut exists $When."
    Assert-That ((Read-Value $sender 'DisplayName') -ceq 'Threadline Installer Test') "the former sender name is missing $When."
    Assert-That ((Read-Value $server '') -ceq $command) "the activation command changed $When."
}

function Assert-CurrentInstall([string]$When) {
    Assert-That (Test-Path -LiteralPath $current) "the new shortcut is missing $When."
    Assert-That ((Get-ShortcutTarget $current) -ieq $app) "the new shortcut has another target $When."
    Assert-That (!(Test-Path -LiteralPath $former)) "the former shortcut remains $When."
    Assert-That ((Read-Value $sender 'DisplayName') -ceq 'Threadline Preview') "the sender name was not updated $When."
    Assert-That ((Read-Value $sender 'CustomActivator') -ieq $class) "the activator is wrong $When."
    Assert-That ((Read-Value $server '') -ceq $command) "the activation command is wrong $When."
}

try {
    $null = [IO.Directory]::CreateDirectory((Split-Path -Parent $app))
    $null = [IO.Directory]::CreateDirectory($programs)
    [IO.File]::WriteAllBytes($app, [byte[]](0x4d, 0x5a))
    $sources = (Join-Path (Split-Path -Parent $PSScriptRoot) 'NotificationRegistration.cs'),
        (Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'native\ThreadlineToastShortcut.cs')
    & $csc /nologo /target:winexe /platform:x64 /optimize+ /warnaserror+ /define:THREADLINE_HELPER_TEST "/out:$helper" $sources
    if ($LASTEXITCODE -ne 0) { throw 'The notification registration helper did not compile.' }
    $env:THREADLINE_HELPER_TEST_DATA = $data
    $env:THREADLINE_HELPER_TEST_PROGRAMS = $programs
    $env:THREADLINE_HELPER_TEST_KEY = $keyName

    # A first installation publishes only the new name.
    Reset-State
    $run = New-SetupRun
    Invoke-Helper $run 'Prepare'
    Invoke-Helper $run 'Apply'
    Assert-CurrentInstall 'after a first installation'

    # Repairing that installation keeps it.
    $run = New-SetupRun
    Invoke-Helper $run 'Prepare'
    Invoke-Helper $run 'Apply'
    Assert-CurrentInstall 'after a repair'

    # An installation made under the former name moves to the new one.
    Reset-State -FormerInstall
    $run = New-SetupRun
    Invoke-Helper $run 'Prepare'
    Assert-FormerInstall 'before setup commits'
    Invoke-Helper $run 'Apply'
    Assert-CurrentInstall 'after an upgrade'

    # A failed upgrade puts the former name back, before and after the change was applied.
    Reset-State -FormerInstall
    $run = New-SetupRun
    Invoke-Helper $run 'Prepare'
    Invoke-Helper $run 'Restore'
    Assert-FormerInstall 'after rolling back an unapplied upgrade'
    $run = New-SetupRun
    Invoke-Helper $run 'Prepare'
    Invoke-Helper $run 'Apply'
    Invoke-Helper $run 'Restore'
    Assert-FormerInstall 'after rolling back an applied upgrade'

    # Uninstalling removes both names, and a failed uninstall restores what was there.
    $run = New-SetupRun
    Invoke-Helper $run 'Prepare'
    Invoke-Helper $run 'Apply'
    $run = New-SetupRun
    Invoke-Helper $run 'Prepare'
    Invoke-Helper $run 'Remove'
    Invoke-Helper $run 'Restore'
    Assert-CurrentInstall 'after rolling back an uninstall'
    $run = New-SetupRun
    Invoke-Helper $run 'Prepare'
    Invoke-Helper $run 'Remove'
    Assert-That (!(Test-Path -LiteralPath $current) -and !(Test-Path -LiteralPath $former)) 'a shortcut remains after uninstall.'
    Assert-That (!(Test-Path -LiteralPath $sender) -and !(Test-Path -LiteralPath $server)) 'a registration remains after uninstall.'

    # A former shortcut that opens another program, or a sender with another name, is left alone.
    Reset-State -FormerInstall
    Set-Shortcut $former (Join-Path $env:WINDIR 'System32\notepad.exe')
    $run = New-SetupRun
    Invoke-Helper $run 'Prepare' 22
    Assert-That (!(Test-Path -LiteralPath $current)) 'setup published a shortcut beside a foreign one.'
    Reset-State -FormerInstall
    Set-ItemProperty -LiteralPath $sender -Name 'DisplayName' -Value 'Another application'
    $run = New-SetupRun
    Invoke-Helper $run 'Prepare' 22

    'Notification rename checks passed.'
} finally {
    foreach ($name in 'DATA', 'PROGRAMS', 'KEY') {
        [Environment]::SetEnvironmentVariable("THREADLINE_HELPER_TEST_$name", $null)
    }
    if (Test-Path -LiteralPath $key) { Remove-Item -LiteralPath $key -Recurse -Force }
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}
