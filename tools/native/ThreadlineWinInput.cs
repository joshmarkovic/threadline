using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class ThreadlineWinInput
{
    private const uint KeyUp = 2;
    private const uint LeftDown = 2;
    private const uint LeftUp = 4;
    private const int HostedCaptureFreshnessMs = 500;
    private static readonly int InputSize = Marshal.SizeOf(typeof(Input));

    public static IntPtr TargetWindow { get; set; }

    [StructLayout(LayoutKind.Sequential)]
    public struct Point { public int X, Y; }

    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left, Top, Right, Bottom; }

    [StructLayout(LayoutKind.Sequential)]
    private struct MonitorInfo
    {
        public int Size;
        public Rect Monitor, WorkArea;
        public uint Flags;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct KeyboardInput
    {
        public ushort VirtualKey, ScanCode;
        public uint Flags, Time;
        public UIntPtr ExtraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MouseInput
    {
        public int X, Y;
        public uint Data, Flags, Time;
        public UIntPtr ExtraInfo;
    }

    [StructLayout(LayoutKind.Explicit)]
    private struct InputData
    {
        [FieldOffset(0)] public KeyboardInput Keyboard;
        [FieldOffset(0)] public MouseInput Mouse;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct Input { public uint Kind; public InputData Data; }

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool IsIconic(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr window, int command);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetProcessDpiAwarenessContext(IntPtr context);

    [DllImport("user32.dll")]
    public static extern uint GetDpiForWindow(IntPtr window);

    [DllImport("user32.dll")]
    public static extern uint GetClipboardSequenceNumber();

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetClientRect(IntPtr window, out Rect bounds);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool ClientToScreen(IntPtr window, ref Point point);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool GetCursorPos(out Point point);

    [DllImport("user32.dll")]
    private static extern IntPtr WindowFromPoint(Point point);

    [DllImport("user32.dll")]
    private static extern IntPtr GetAncestor(IntPtr window, uint flags);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetClassName(IntPtr window, StringBuilder name, int capacity);

    [DllImport("user32.dll")]
    private static extern bool GetWindowRect(IntPtr window, out Rect bounds);

    [DllImport("user32.dll")]
    private static extern IntPtr MonitorFromWindow(IntPtr window, uint flags);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern bool GetMonitorInfo(IntPtr monitor, ref MonitorInfo info);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags);

    [DllImport("dwmapi.dll")]
    private static extern int DwmFlush();

    [DllImport("user32.dll", SetLastError = true)]
    private static extern uint SendInput(uint count, Input[] input, int size);

    private static void RequireTargetFocus()
    {
        if (TargetWindow == IntPtr.Zero || GetForegroundWindow() != TargetWindow)
            throw new InvalidOperationException("The Threadline test window lost foreground focus.");
    }

    private static void Send(Input[] input, bool requireFocus = true)
    {
        if (requireFocus) RequireTargetFocus();
        uint accepted = 0;
        try
        {
            accepted = SendInput((uint)input.Length, input, InputSize);
            if (accepted != input.Length)
                throw new Win32Exception(Marshal.GetLastWin32Error(), "SendInput did not inject every event.");
        }
        finally
        {
            if (accepted > 0 && accepted < input.Length)
                ReleaseInterruptedInput(input, (int)accepted);
        }
    }

    private static List<Input> PendingReleases(Input[] input, int accepted)
    {
        var pending = new List<Input>();
        for (int index = 0; index < accepted; index++)
        {
            var current = input[index];
            if (current.Kind == 1)
            {
                var keyboard = current.Data.Keyboard;
                if ((keyboard.Flags & KeyUp) == 0)
                {
                    pending.Add(Key(keyboard.VirtualKey, keyboard.ScanCode, keyboard.Flags | KeyUp));
                }
                else
                {
                    int match = pending.FindLastIndex(release =>
                        release.Kind == 1 &&
                        release.Data.Keyboard.VirtualKey == keyboard.VirtualKey &&
                        release.Data.Keyboard.ScanCode == keyboard.ScanCode &&
                        release.Data.Keyboard.Flags == keyboard.Flags);
                    if (match >= 0) pending.RemoveAt(match);
                }
            }
            else if (current.Kind == 0 && current.Data.Mouse.Flags == LeftDown) pending.Add(MouseEvent(LeftUp));
            else if (current.Kind == 0 && current.Data.Mouse.Flags == LeftUp)
            {
                int match = pending.FindLastIndex(release => release.Kind == 0 && release.Data.Mouse.Flags == LeftUp);
                if (match >= 0) pending.RemoveAt(match);
            }
        }
        return pending;
    }

    private static void ReleaseInterruptedInput(Input[] input, int accepted)
    {
        var pending = PendingReleases(input, accepted);
        Win32Exception failure = null;
        for (int index = pending.Count - 1; index >= 0; index--)
        {
            try { Release(pending[index]); }
            catch (Win32Exception error) { failure = error; }
        }
        if (failure != null) throw failure;
    }

    private static void Release(Input input)
    {
        var events = new[] { input };
        int error = 0;
        for (int attempt = 0; attempt < 3; attempt++)
        {
            if (SendInput(1, events, InputSize) == 1) return;
            error = Marshal.GetLastWin32Error();
            if (attempt < 2) Thread.Sleep(16);
        }
        throw new Win32Exception(error, "SendInput could not release an injected key or mouse button.");
    }

    private static Input Key(ushort key, ushort scanCode, uint flags)
    {
        return new Input
        {
            Kind = 1,
            Data = new InputData
            {
                Keyboard = new KeyboardInput { VirtualKey = key, ScanCode = scanCode, Flags = flags }
            }
        };
    }

    public static void Chord(params int[] keys)
    {
        var events = new Input[keys.Length * 2];
        for (int index = 0; index < keys.Length; index++)
        {
            var key = (ushort)keys[index];
            var flags = key >= 0x25 && key <= 0x28 ? 1u : 0u;
            events[index] = Key(key, 0, flags);
            events[events.Length - index - 1] = Key(key, 0, flags | KeyUp);
        }
        Send(events);
    }

    public static void TypeText(string text)
    {
        if (String.IsNullOrEmpty(text)) return;
        var events = new Input[text.Length * 2];
        for (int index = 0; index < text.Length; index++)
        {
            var character = (ushort)text[index];
            events[index * 2] = Key(0, character, 4);
            events[index * 2 + 1] = Key(0, character, 4 | KeyUp);
        }
        Send(events);
    }

    private static void RequirePointerTarget(Point point)
    {
        RequireTargetFocus();
        if (WindowFromPoint(point) != TargetWindow)
            throw new InvalidOperationException("A different native window covers the requested Threadline control.");
    }

    private static void Move(Point point)
    {
        RequirePointerTarget(point);
        if (!SetCursorPos(point.X, point.Y)) throw new Win32Exception(Marshal.GetLastWin32Error());
        RequirePointerTarget(point);
    }

    private static Input MouseEvent(uint flags, int data = 0)
    {
        return new Input
        {
            Data = new InputData { Mouse = new MouseInput { Flags = flags, Data = unchecked((uint)data) } }
        };
    }

    private static void ClickPointer(bool requireFocus)
    {
        Send(new[] { MouseEvent(LeftDown), MouseEvent(LeftUp) }, requireFocus);
    }

    public static void Drag(Point from, Point to)
    {
        Move(from);
        Send(new[] { MouseEvent(LeftDown) });
        try
        {
            for (int step = 1; step <= 12; step++)
            {
                Move(new Point { X = from.X + (to.X - from.X) * step / 12, Y = from.Y + (to.Y - from.Y) * step / 12 });
                Thread.Sleep(16);
            }
            RequireTargetFocus();
        }
        finally { Release(MouseEvent(LeftUp)); }
    }

    public static void Click(Point point)
    {
        Move(point);
        ClickPointer(requireFocus: true);
    }

    public static void ClickShellControl(Point point, uint shellProcess)
    {
        RequireShellTarget(point, shellProcess);
        if (!SetCursorPos(point.X, point.Y)) throw new Win32Exception(Marshal.GetLastWin32Error());
        RequireShellTarget(point, shellProcess);
        ClickPointer(requireFocus: false);
    }

    public static bool GetHostedNotificationCorner(IntPtr testWindow, out Rect corner)
    {
        corner = new Rect();
        var monitor = MonitorFromWindow(testWindow, 0);
        var info = new MonitorInfo { Size = Marshal.SizeOf(typeof(MonitorInfo)) };
        if (monitor == IntPtr.Zero || !GetMonitorInfo(monitor, ref info)) return false;
        corner = info.WorkArea;
        corner.Left = Math.Max(corner.Left, corner.Right - 512);
        corner.Top = Math.Max(corner.Top, corner.Bottom - 300);
        return corner.Right > corner.Left && corner.Bottom > corner.Top;
    }

    public sealed class HostedToast
    {
        public IntPtr Window;
        public uint ProcessId;
        public string WindowClass;
    }

    public static HostedToast FindHostedToast(Point point)
    {
        var window = GetAncestor(WindowFromPoint(point), 2);
        if (window == IntPtr.Zero || !IsWindowVisible(window)) return null;
        Rect bounds;
        if (!GetWindowRect(window, out bounds) || bounds.Right <= bounds.Left || bounds.Bottom <= bounds.Top ||
            bounds.Right - bounds.Left > 600 || bounds.Bottom - bounds.Top > 380) return null;

        var name = new StringBuilder(128);
        if (GetClassName(window, name, name.Capacity) == 0) return null;
        string windowClass = name.ToString();
        if (windowClass != "Windows.UI.NotificationToastWindow" && windowClass != "Windows.UI.Core.CoreWindow" &&
            windowClass != "ToastWnd" && windowClass != "tooltips_class32") return null;

        uint processId = WindowProcessId(window);
        if (!IsHostedShellProcess(processId)) return null;
        return new HostedToast { Window = window, ProcessId = processId, WindowClass = windowClass };
    }

    private static bool IsHostedShellProcess(uint processId)
    {
        if (processId == 0) return false;
        var windows = Environment.GetFolderPath(Environment.SpecialFolder.Windows);
        try
        {
            using (var process = Process.GetProcessById((int)processId))
            {
                string image = process.MainModule.FileName;
                string host = Path.GetFileName(image);
                if (String.Equals(image, Path.Combine(windows, "explorer.exe"), StringComparison.OrdinalIgnoreCase))
                    return true;
                return image.StartsWith(Path.Combine(windows, "SystemApps") + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase) &&
                    (String.Equals(host, "ShellExperienceHost.exe", StringComparison.OrdinalIgnoreCase) ||
                        String.Equals(host, "ShellHost.exe", StringComparison.OrdinalIgnoreCase));
            }
        }
        catch (Win32Exception) { return false; }
        catch (InvalidOperationException) { return false; }
        catch (ArgumentException) { return false; }
    }

    public static bool ClickHostedToast(Point point, HostedToast expected, long capturedAt, long deadline)
    {
        if (!IsHostedCaptureFresh(capturedAt, deadline, Stopwatch.GetTimestamp())) return false;
        RequireHostedToast(point, expected);
        if (!IsHostedCaptureFresh(capturedAt, deadline, Stopwatch.GetTimestamp())) return false;
        if (!SetCursorPos(point.X, point.Y)) throw new Win32Exception(Marshal.GetLastWin32Error());
        Point actual = RequireHostedCursor(point);
        RequireHostedToast(actual, expected);
        RequireHostedCursor(actual);
        if (!IsHostedCaptureFresh(capturedAt, deadline, Stopwatch.GetTimestamp())) return false;
        ClickPointer(requireFocus: false);
        return true;
    }

    private static bool IsHostedCaptureFresh(long capturedAt, long deadline, long now)
    {
        return capturedAt > 0 && now >= capturedAt && now < deadline &&
            now - capturedAt < Stopwatch.Frequency * HostedCaptureFreshnessMs / 1000;
    }

    private static Point RequireHostedCursor(Point expected)
    {
        Point actual;
        if (!GetCursorPos(out actual)) throw new Win32Exception(Marshal.GetLastWin32Error());
        if (actual.X != expected.X || actual.Y != expected.Y)
            throw new InvalidOperationException("The actual cursor did not remain at the verified notification point.");
        return actual;
    }

    private static void RequireHostedToast(Point point, HostedToast expected)
    {
        var current = FindHostedToast(point);
        if (current == null || current.Window != expected.Window || current.ProcessId != expected.ProcessId ||
            current.WindowClass != expected.WindowClass)
            throw new InvalidOperationException("The verified native notification window no longer owns the click point.");
    }

    private static void RequireShellTarget(Point point, uint shellProcess)
    {
        if (shellProcess == 0 || WindowProcessId(WindowFromPoint(point)) != shellProcess)
            throw new InvalidOperationException("The selected point is no longer in the shell control.");
    }

    public static uint WindowProcessId(IntPtr window)
    {
        uint process;
        GetWindowThreadProcessId(window, out process);
        return process;
    }

    public static bool FocusByPointer(IntPtr window)
    {
        const uint flags = 0x0001 | 0x0002 | 0x0010;
        if (!SetWindowPos(window, new IntPtr(-1), 0, 0, 0, 0, flags)) return false;
        try
        {
            DwmFlush();
            var point = new Point { X = 36, Y = 16 };
            if (!ClientToScreen(window, ref point) || WindowFromPoint(point) != window) return false;
            if (!SetCursorPos(point.X, point.Y) || WindowFromPoint(point) != window) return false;
            ClickPointer(requireFocus: false);
            return true;
        }
        finally { SetWindowPos(window, new IntPtr(-2), 0, 0, 0, 0, flags); }
    }

    public static void Scroll(Point point, int notches, int intervalMilliseconds)
    {
        Move(point);
        int movement = notches < 0 ? -120 : 120;
        for (int index = 0; index < Math.Abs(notches); index++)
        {
            Send(new[] { MouseEvent(0x0800, movement) });
            Thread.Sleep(intervalMilliseconds);
        }
    }
}
