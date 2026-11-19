[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$driver = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'ThreadlineWinInput.cs'))
Add-Type -TypeDefinition ($driver + @'

public static class ThreadlineWheelChecks
{
    private static void Check(bool condition, string description)
    {
        if (!condition) throw new InvalidOperationException(description);
    }

    private static void Reject<T>(Action action, string message) where T : Exception
    {
        try { action(); }
        catch (T error)
        {
            Check(error.Message.Contains(message), error.Message);
            return;
        }
        throw new InvalidOperationException("Expected rejection: " + message);
    }

    private static void MapsToPixel(int origin, int extent, int expected)
    {
        int absolute = ThreadlineWinInput.NormalizePixel(expected, origin, extent);
        Check(absolute >= 0 && absolute <= 65535, "Absolute coordinate is outside Win32's range.");
        Check(origin + (long)absolute * extent / 65536 == expected, "Absolute coordinate selected the wrong physical pixel.");
    }

    public static void Run()
    {
        MapsToPixel(0, 3840, 0);
        MapsToPixel(0, 3840, 3839);
        MapsToPixel(-1600, 5440, -1600);
        MapsToPixel(-1600, 5440, -1);
        MapsToPixel(-1600, 5440, 0);
        MapsToPixel(-1600, 5440, 3839);
        MapsToPixel(-500, 2660, 300);
        MapsToPixel(-27000, 50000, -27000);
        MapsToPixel(-27000, 50000, -26999);
        MapsToPixel(-27000, 50000, 22998);
        MapsToPixel(-27000, 50000, 22999);
        MapsToPixel(0, 65536, 65535);
        Reject<InvalidOperationException>(() => ThreadlineWinInput.NormalizePixel(-1601, -1600, 5440), "outside");
        Reject<InvalidOperationException>(() => ThreadlineWinInput.NormalizePixel(3840, -1600, 5440), "outside");
        Reject<InvalidOperationException>(() => ThreadlineWinInput.NormalizePixel(1, 0, 65537), "outside");

        var window = new IntPtr(1);
        var other = new IntPtr(2);
        ThreadlineWinInput.RequireWheelOwner(window, window, window, 10, 10, true);
        Reject<ThreadlineWinInput.TargetUnavailableException>(() => ThreadlineWinInput.RequireWheelOwner(window, window, window, 20, 10, true), "child window");
        Reject<ThreadlineWinInput.TargetUnavailableException>(() => ThreadlineWinInput.RequireWheelOwner(window, window, window, 10, 10, false), "child window");
        Reject<ThreadlineWinInput.TargetUnavailableException>(() => ThreadlineWinInput.RequireWheelOwner(window, other, window, 10, 10, true), "focus");
        Reject<ThreadlineWinInput.TargetUnavailableException>(() => ThreadlineWinInput.RequireWheelOwner(window, window, other, 10, 10, true), "covered");

        ThreadlineWinInput.RequireAllEvents(2, 2);
        Reject<Win32Exception>(() => ThreadlineWinInput.RequireAllEvents(1, 2), "1 of 2");
        Reject<Win32Exception>(() => ThreadlineWinInput.RequireAllEvents(0, 2), "0 of 2");
        var evidence = new ThreadlineWinInput.WheelEvidence { Calibrated = true, AcceptedDown = 25, AcceptedUp = 25, PhysicalReadbacks = 49 };
        Check(!evidence.Complete, "A missing physical readback must prevent a valid measurement.");
        evidence.PhysicalReadbacks++;
        Check(evidence.Complete, "A complete calibrated gesture must permit measurement.");

        var screen = new ThreadlineWinInput.Point { X = -1250, Y = 360 };
        long packed = ThreadlineWinInput.PackScreenPoint(screen).ToInt64();
        Check((short)packed == screen.X && (short)(packed >> 16) == screen.Y,
            "Synthetic HWND scroll coordinates must retain their signed Win32 values.");
        screen.Y = short.MinValue;
        packed = ThreadlineWinInput.PackScreenPoint(screen).ToInt64();
        Check((short)(packed >> 16) == screen.Y, "South/north monitor coordinates must round trip.");
        screen.X = short.MaxValue + 1;
        Reject<ThreadlineWinInput.TargetUnavailableException>(() => ThreadlineWinInput.PackScreenPoint(screen), "coordinate range");
    }
}
'@)

[ThreadlineWheelChecks]::Run()

$tokens = $null
$errors = $null
$scriptPath = Join-Path $PSScriptRoot 'windows-smoke.ps1'
$syntax = [System.Management.Automation.Language.Parser]::ParseFile($scriptPath, [ref]$tokens, [ref]$errors)
if ($errors.Count -gt 0) { throw 'Could not parse the Windows driver.' }
$checks = @('Assert-ScrollProcess', 'Stop-ScrollBlocked', 'Stop-Blocked', 'Compare-Measurement', 'Get-GpuBudget')
foreach ($definition in $syntax.FindAll({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -in $checks
}, $false)) {
    . ([scriptblock]::Create($definition.Extent.Text))
}

if ((Get-GpuBudget $null 0 20).status -ne 'Blocked' -or
    (Get-GpuBudget 10 1 20).status -ne 'Blocked' -or
    (Get-GpuBudget 25 1 20).status -ne 'Fail' -or
    (Get-GpuBudget 10 2 20).status -ne 'Pass') {
    throw 'A GPU measurement used the wrong sample requirement.'
}
$script:app = [pscustomobject]@{ HasExited = $false; ExitCode = 0 }
Assert-ScrollProcess
foreach ($code in @(0, 13)) {
    $script:app.HasExited = $true
    $script:app.ExitCode = $code
    $script:blocked = $false
    $reason = $null
    try { Stop-ScrollBlocked 'Test focus interruption.' } catch { $reason = $_.Exception.Message }
    if ($script:blocked -or $reason -notlike "*code $code before the driver requested Quit*") {
        throw 'An unexpected process exit was not reported as a failure.'
    }
}

Write-Output 'Wheel mapping, target rejection, input evidence, GPU budgets and unexpected exit checks passed.'
