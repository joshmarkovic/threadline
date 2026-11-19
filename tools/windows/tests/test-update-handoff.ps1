# Run with Windows PowerShell 5.1. Every executable and rendezvous belongs to this temporary fixture.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Harness)

$ErrorActionPreference = 'Stop'
$harnessPath = (Resolve-Path -LiteralPath $Harness).Path
$root = Join-Path ([IO.Path]::GetTempPath()) ("ThreadlineHandoff-" + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($root)
try {
    $peer = Join-Path $root 'peer.exe'
    Add-Type -OutputAssembly $peer -OutputType WindowsApplication -TypeDefinition @'
using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

class HandoffFixture {
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
    struct StartupInfo {
        public int size;
        public string reserved, desktop, title;
        public int x, y, width, height, cx, cy, fill, flags;
        public short show, bytes;
        public IntPtr reserved2, input, output, error;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct ProcessInfo { public IntPtr process, thread; public uint pid, tid; }
    [DllImport("user32", CharSet=CharSet.Unicode)]
    static extern IntPtr CreateDesktop(string name, IntPtr device, IntPtr mode, uint flags, uint access, IntPtr security);
    [DllImport("user32")] static extern bool CloseDesktop(IntPtr desktop);
    [DllImport("kernel32", CharSet=CharSet.Unicode)]
    static extern bool CreateProcess(string app, StringBuilder command, IntPtr ps, IntPtr ts, bool inherit, uint flags,
        IntPtr env, string cwd, ref StartupInfo startup, out ProcessInfo process);
    [DllImport("kernel32")] static extern uint WaitForSingleObject(IntPtr h, uint ms);
    [DllImport("kernel32")] static extern bool GetExitCodeProcess(IntPtr h, out int result);
    [DllImport("kernel32")] static extern bool TerminateProcess(IntPtr h, uint code);
    [DllImport("kernel32")] static extern bool CloseHandle(IntPtr h);

    static int RunOnPrivateDesktop(string setup, string mode) {
        string name = "ThreadlineSetupTest" + Guid.NewGuid().ToString("N");
        IntPtr desktop = CreateDesktop(name, IntPtr.Zero, IntPtr.Zero, 0, 0x1ff, IntPtr.Zero);
        if (desktop == IntPtr.Zero) return 8;
        try {
            StartupInfo startup = new StartupInfo { size = Marshal.SizeOf(typeof(StartupInfo)), desktop = name };
            ProcessInfo child;
            var args = new StringBuilder("\"" + setup + "\"" + (mode == "InteractiveFailure" ? " /fixture-fail" : ""));
            if (!CreateProcess(setup, args, IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero,
                    Path.GetDirectoryName(setup), ref startup, out child)) return 9;
            try {
                if (WaitForSingleObject(child.process, 12000) != 0) {
                    TerminateProcess(child.process, 10);
                    WaitForSingleObject(child.process, 2000);
                    return 10;
                }
                int code;
                return GetExitCodeProcess(child.process, out code) && code == (mode == "InteractiveFailure" ? 16 : 0) ? 0 : 11;
            } finally { CloseHandle(child.thread); CloseHandle(child.process); }
        } finally { CloseDesktop(desktop); }
    }
    static int Main(string[] args) {
        if (args.Length == 0) {
            string dir = AppDomain.CurrentDomain.BaseDirectory;
            File.AppendAllText(Path.Combine(dir, "launched.txt"), "launch\r\n");
            return 0;
        }
        string setup = args[0], mode = args[1];
        if (mode.StartsWith("Interactive")) return RunOnPrivateDesktop(setup, mode);
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
    foreach ($mode in 'Manual', 'Interactive', 'InteractiveFailure', 'Accept', 'Decline', 'StillRunning', 'WrongImage') {
        $caseDir = Join-Path $root $mode
        $stage = Join-Path $caseDir 'Setup'
        [void][IO.Directory]::CreateDirectory($stage)
        $setup = Join-Path $stage 'Handoff.exe'
        Copy-Item -LiteralPath $harnessPath -Destination $setup
        $app = Join-Path $caseDir 'threadline.exe'
        Copy-Item -LiteralPath $peer -Destination $app
        if ($mode -eq 'Manual') {
            $p = Start-Process -FilePath $setup -ArgumentList '/S' -Wait -PassThru
        } else {
            if ($mode -eq 'WrongImage') { $app = $peer }
            $p = Start-Process -FilePath $app -ArgumentList @(('"' + $setup + '"'), $mode) -Wait -PassThru
        }
        if ($p.ExitCode -ne 0) { throw "$mode fixture failed with exit code $($p.ExitCode)" }
        $result = Join-Path $stage 'result.ini'
        $deadline = [DateTime]::UtcNow.AddSeconds(10)
        while (!(Test-Path -LiteralPath $result) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
        $lines = Get-Content -LiteralPath $result
        $expected = if ($mode -in 'Manual','Interactive','Accept') { 0 } elseif ($mode -eq 'InteractiveFailure') { 16 } else { 21 }
        if ($lines -notcontains "code=$expected" -or ($mode -eq 'Accept' -and $lines -notcontains 'restart=1') -or
            ($mode -eq 'Manual' -and $lines -contains 'restart=1')) { throw "$mode returned an unexpected handoff result." }
        $launchReceipt = Join-Path $caseDir 'launched.txt'
        if ($mode -in 'Interactive','Accept') {
            while (!(Test-Path -LiteralPath $launchReceipt) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
            if (@(Get-Content -LiteralPath $launchReceipt).Count -ne 1) { throw "$mode must launch the app exactly once." }
        } elseif (Test-Path -LiteralPath $launchReceipt) { throw "$mode unexpectedly launched the app." }
        Write-Output "$mode passed"
    }
} finally {
    Remove-Item -LiteralPath $root -Recurse -Force
}
