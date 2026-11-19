[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$Report, [ValidateRange(1,12)][int]$Samples=6)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (Test-Path -LiteralPath $Report) { throw 'Choose a new receipt file.' }
if (-not [Environment]::Is64BitProcess) { throw 'Run the reference in 64-bit Windows PowerShell.' }
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,UIAutomationProvider,WindowsBase,System.Windows.Forms,System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class ThreadlineRefOwner {
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
}
'@
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('Threadline-invoke-' + [Guid]::NewGuid().ToString('N'))
$null = New-Item -Path $scratch -ItemType Directory
$source = Join-Path $PSScriptRoot 'InvokeReference.cs'
$program = Join-Path $scratch 'InvokeReference.exe'
$target = $null
$samplesTaken = New-Object 'System.Collections.Generic.List[object]'
$receipt = [ordered]@{ diagnostic='owned-native-uia-invoke';status='Fail';frequency=[Diagnostics.Stopwatch]::Frequency;
    os_build=[Environment]::OSVersion.Version.Build;session=[Diagnostics.Process]::GetCurrentProcess().SessionId;
    shell_version=$PSVersionTable.PSVersion.ToString();provider_source_sha256=(Get-FileHash -LiteralPath $source).Hash.ToLowerInvariant();
    warmup=1;samples=$samplesTaken }
function Wait-Record([string]$Name) {
    $path = Join-Path $scratch $Name
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while (!(Test-Path -LiteralPath $path -PathType Leaf)) {
        if ($target.HasExited -or $clock.ElapsedMilliseconds -gt 10000) { throw 'The reference window did not publish the expected record.' }
        Start-Sleep -Milliseconds 20
    }
    return [IO.File]::ReadAllText($path)
}
function Delta([long]$A,[long]$B) { [Math]::Round(1000.0*($B-$A)/$receipt.frequency,4) }
try {
    $refs = @([Windows.Automation.Provider.AutomationInteropProvider],[Windows.Automation.ControlType],
        [Windows.Threading.Dispatcher],[Windows.Forms.Form],[Drawing.Size]) | ForEach-Object { '/r:' + $_.Assembly.Location } | Select-Object -Unique
    $csc = Join-Path ([Runtime.InteropServices.RuntimeEnvironment]::GetRuntimeDirectory()) 'csc.exe'
    & $csc /nologo /warnaserror /target:winexe /platform:x64 ("/out:"+$program) @refs $source
    if ($LASTEXITCODE -ne 0) { throw 'Failed to compile the owned UI Automation provider.' }
    $target = Start-Process -FilePath $program -ArgumentList ('"'+$scratch+'"') -PassThru
    $ready = (Wait-Record 'ready').Split(',')
    $window = [IntPtr]([long]$ready[0])
    if ([long]$ready[1] -ne $receipt.frequency) { throw 'The provider and client clocks differ.' }
    for ($i=0; $i -le $Samples; $i++) {
        [uint32]$ownerId = 0
        $null = [ThreadlineRefOwner]::GetWindowThreadProcessId($window,[ref]$ownerId)
        if ($ownerId -ne $target.Id -or $target.HasExited) { throw 'The reference window changed owner.' }
        $element = [Windows.Automation.AutomationElement]::FromHandle($window)
        $pattern = $null
        if ($element.Current.ProcessId -ne $target.Id -or $element.Current.AutomationId -cne 'threadline-reference-invoke' -or
            $element.Current.Name -cne 'Threadline Invoke reference' -or
            -not $element.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)) {
            throw 'The expected owned provider did not expose Invoke.'
        }
        $begin = [Diagnostics.Stopwatch]::GetTimestamp()
        $pattern.Invoke()
        $returned = [Diagnostics.Stopwatch]::GetTimestamp()
        $ticks = (Wait-Record ('sample-'+($i+1))).Split(',')
        $entered = [long]$ticks[0]
        $handled = [long]$ticks[1]
        if ($entered -lt $begin -or $handled -lt $entered -or $returned -lt $begin) { throw 'The clocks did not correlate.' }
        if ($i -gt 0) {
            $samplesTaken.Add([ordered]@{sample=$i;client_begin_qpc=$begin;provider_entry_qpc=$entered;
                ui_handler_qpc=$handled;client_return_qpc=$returned;call_ms=(Delta $begin $returned);
                before_provider_ms=(Delta $begin $entered);provider_to_ui_ms=(Delta $entered $handled);
                total_to_ui_ms=(Delta $begin $handled)})
        }
    }
    $receipt.status = 'Pass'
} catch {
    $receipt.detail = $_.Exception.Message
    throw
} finally {
    if ($target) {
        [IO.File]::WriteAllText((Join-Path $scratch 'stop'),'stop')
        if (-not $target.WaitForExit(8000)) { $target.Kill(); $target.WaitForExit(); $receipt.status='Fail' }
        $target.Dispose()
    }
    $receipt | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $Report -Encoding UTF8
    Remove-Item -LiteralPath $scratch -Recurse -Force
}
if ($receipt.status -ne 'Pass') { throw 'The reference window did not exit normally.' }
