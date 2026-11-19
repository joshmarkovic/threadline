# Run with Windows PowerShell 5.1. Every executable and rendezvous belongs to this temporary fixture.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Harness)

$ErrorActionPreference = 'Stop'
$harnessPath = (Resolve-Path -LiteralPath $Harness).Path
$root = Join-Path ([IO.Path]::GetTempPath()) ("ThreadlineHandoff-" + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($root)
try {
    $peer = Join-Path $root 'peer.exe'
    Add-Type -OutputAssembly $peer -OutputType ConsoleApplication -TypeDefinition @'
using System;
using System.Diagnostics;
using System.IO;
using System.Threading;

class HandoffFixture {
    static int Main(string[] args) {
        string setup = args[0], mode = args[1];
        int pid = Process.GetCurrentProcess().Id;
        string prefix = @"Local\Threadline.Update." + pid + "." + new DirectoryInfo(Path.GetDirectoryName(setup)).Name;
        bool newReady, newProceed;
        using (var ready = new EventWaitHandle(false, EventResetMode.ManualReset, prefix + ".Ready", out newReady))
        using (var proceed = new EventWaitHandle(false, EventResetMode.ManualReset, prefix + ".Proceed", out newProceed)) {
            if (!newReady || !newProceed) return 2;
            using (var child = Process.Start(new ProcessStartInfo(setup, "/S /THREADLINEUPDATE=" + pid) {UseShellExecute = false})) {
                if (mode == "WrongImage") return child.WaitForExit(10000) && child.ExitCode == 21 ? 0 : 3;
                if (!ready.WaitOne(10000)) return 4;
                if (mode == "Decline") return 0;
                proceed.Set();
                if (mode == "StillRunning") return child.WaitForExit(10000) && child.ExitCode == 21 ? 0 : 5;
                return 0;
            }
        }
    }
}
'@
    foreach ($mode in 'Manual', 'Accept', 'Decline', 'StillRunning', 'WrongImage') {
        $caseDir = Join-Path $root $mode
        $stage = Join-Path $caseDir 'Setup'
        [void][IO.Directory]::CreateDirectory($stage)
        $setup = Join-Path $stage 'Handoff.exe'
        Copy-Item -LiteralPath $harnessPath -Destination $setup
        if ($mode -eq 'Manual') {
            $p = Start-Process -FilePath $setup -ArgumentList '/S' -Wait -PassThru
        } else {
            $name = if ($mode -eq 'WrongImage') { 'wrong.exe' } else { 'threadline.exe' }
            $app = Join-Path $caseDir $name
            Copy-Item -LiteralPath $peer -Destination $app
            $p = Start-Process -FilePath $app -ArgumentList @(('"' + $setup + '"'), $mode) -Wait -PassThru
        }
        if ($p.ExitCode -ne 0) { throw "$mode fixture failed with exit code $($p.ExitCode)" }
        $result = Join-Path $stage 'result.ini'
        $deadline = [DateTime]::UtcNow.AddSeconds(10)
        while (!(Test-Path -LiteralPath $result) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
        $lines = Get-Content -LiteralPath $result
        $expected = if ($mode -in 'Manual','Accept') { 0 } else { 21 }
        if ($lines -notcontains "code=$expected" -or ($mode -eq 'Accept' -and $lines -notcontains 'restart=1') -or
            ($mode -eq 'Manual' -and $lines -contains 'restart=1')) { throw "$mode returned an unexpected handoff result." }
        Write-Output "$mode passed"
    }
} finally {
    Remove-Item -LiteralPath $root -Recurse -Force
}
