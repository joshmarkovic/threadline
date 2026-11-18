using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;

public static class ThreadlineWinInput
{
    private const uint KeyUp = 2;
    private const uint LeftDown = 2;
    private const uint LeftUp = 4;
    private const uint AbsoluteVirtualDesktopMove = 0x0001 | 0x4000 | 0x8000;
    private static readonly int InputSize = Marshal.SizeOf(typeof(Input));

    public static IntPtr TargetWindow { get; set; }

    [StructLayout(LayoutKind.Sequential)]
    public struct Point { public int X, Y; }

    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left, Top, Right, Bottom; }

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

    [DllImport("user32.dll", SetLastError = true)]
    private static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);

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

    [DllImport("user32.dll")]
    private static extern bool GetPhysicalCursorPos(out Point point);

    [DllImport("user32.dll")]
    private static extern bool GetClipCursor(out Rect bounds);

    [DllImport("user32.dll")]
    private static extern int GetSystemMetrics(int index);

    private delegate bool MonitorEnumerator(IntPtr monitor, IntPtr dc, ref Rect bounds, IntPtr data);

    [DllImport("user32.dll")]
    private static extern bool EnumDisplayMonitors(IntPtr dc, IntPtr clip, MonitorEnumerator callback, IntPtr data);

    [DllImport("user32.dll")]
    private static extern IntPtr WindowFromPoint(Point point);

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
            throw new TargetUnavailableException("The Threadline test window lost foreground focus.");
    }

    private static void Send(Input[] input, bool requireFocus = true)
    {
        if (requireFocus) RequireTargetFocus();
        uint accepted = 0;
        try
        {
            accepted = SendInput((uint)input.Length, input, InputSize);
            RequireAllEvents(accepted, input.Length);
        }
        finally
        {
            if (accepted > 0 && accepted < input.Length)
                ReleaseInterruptedInput(input, (int)accepted);
        }
    }

    internal static void RequireAllEvents(uint accepted, int expected)
    {
        if (accepted != expected)
            throw new Win32Exception(Marshal.GetLastWin32Error(), "SendInput accepted " + accepted + " of " + expected + " events.");
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

    public sealed class TargetUnavailableException : InvalidOperationException
    {
        internal TargetUnavailableException(string message) : base(message) { }
    }

    public sealed class WheelEvidence
    {
        public bool Calibrated { get; internal set; }
        public int AcceptedDown { get; internal set; }
        public int AcceptedUp { get; internal set; }
        public int PhysicalReadbacks { get; internal set; }
        public int Repositioned { get; internal set; }
        public int? FirstMissedReadback { get; internal set; }
        public int? StrokeInProgress { get; internal set; }
        public double LongestGapMs { get; internal set; }
        public double MeanGapMs { get; internal set; }

        public bool Complete { get { return Calibrated && AcceptedDown == 25 && AcceptedUp == 25 && PhysicalReadbacks == 50; } }
    }

    private static bool Contains(Rect bounds, Point point)
    {
        return point.X >= bounds.Left && point.X < bounds.Right && point.Y >= bounds.Top && point.Y < bounds.Bottom;
    }

    private static bool SamePoint(Point first, Point second)
    {
        return first.X == second.X && first.Y == second.Y;
    }

    private static bool SameRect(Rect first, Rect second)
    {
        return first.Left == second.Left && first.Top == second.Top && first.Right == second.Right && first.Bottom == second.Bottom;
    }

    private static Rect VirtualDesktop()
    {
        const int left = 76, top = 77, width = 78, height = 79;
        int x = GetSystemMetrics(left), y = GetSystemMetrics(top);
        return new Rect { Left = x, Top = y, Right = x + GetSystemMetrics(width), Bottom = y + GetSystemMetrics(height) };
    }

    private static Rect VerifiedVirtualDesktop()
    {
        Rect desktop = new Rect { Left = int.MaxValue, Top = int.MaxValue, Right = int.MinValue, Bottom = int.MinValue };
        bool enumerated = EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero,
            (IntPtr monitor, IntPtr dc, ref Rect bounds, IntPtr data) => {
                desktop.Left = Math.Min(desktop.Left, bounds.Left);
                desktop.Top = Math.Min(desktop.Top, bounds.Top);
                desktop.Right = Math.Max(desktop.Right, bounds.Right);
                desktop.Bottom = Math.Max(desktop.Bottom, bounds.Bottom);
                return true;
            }, IntPtr.Zero);
        if (!enumerated || !SameRect(desktop, VirtualDesktop()))
            throw new TargetUnavailableException("Physical monitor bounds and the virtual desktop disagree.");
        return desktop;
    }

    internal static int NormalizePixel(int coordinate, int origin, int extent)
    {
        const int units = 65536;
        long pixel = (long)coordinate - origin;
        if (extent <= 0 || extent > units || pixel < 0 || pixel >= extent)
            throw new InvalidOperationException("The target pixel is outside the representable virtual desktop.");
        // Choose an integer inside the bin that maps back to this physical pixel.
        long first = (pixel * units + extent - 1) / extent;
        long last = ((pixel + 1) * units + extent - 1) / extent - 1;
        return (int)((first + last) / 2);
    }

    internal static void RequireWheelOwner(IntPtr window, IntPtr foreground, IntPtr hit, uint owner, uint child, bool visible)
    {
        if (window == IntPtr.Zero || owner != child || child == 0 || !visible)
            throw new TargetUnavailableException("The wheel target is no longer the visible Threadline child window.");
        if (foreground != window)
            throw new TargetUnavailableException("Threadline lost foreground focus during wheel input.");
        if (hit != window)
            throw new TargetUnavailableException("The planned wheel point is covered by a different native window.");
    }

    private static void RequireWheelTarget(IntPtr window, uint child, Point target)
    {
        RequireWheelOwner(window, GetForegroundWindow(), WindowFromPoint(target), WindowProcessId(window), child, IsWindowVisible(window));
        Rect clip;
        if (!GetClipCursor(out clip) || !Contains(clip, target))
            throw new TargetUnavailableException("Cursor clipping does not allow the planned wheel point.");
    }

    public static void Scroll(IntPtr window, uint child, uint dpi, Point client, WheelEvidence evidence)
    {
        IntPtr previous = SetThreadDpiAwarenessContext(new IntPtr(-4));
        if (previous == IntPtr.Zero)
            throw new TargetUnavailableException("Could not select per-monitor physical coordinates for wheel input.");
        try
        {
            RequireTargetFocus();
            Rect originalClient;
            var target = client;
            if (window != TargetWindow || child == 0 || dpi == 0)
                throw new ArgumentException("Wheel input needs the configured child window, process and DPI.");
            if (GetDpiForWindow(window) != dpi ||
                !GetClientRect(window, out originalClient) || !Contains(originalClient, client) || !ClientToScreen(window, ref target))
                throw new TargetUnavailableException("Threadline did not expose the expected native wheel viewport at its current DPI.");
            RequireWheelTarget(window, child, target);
            Rect desktop = VerifiedVirtualDesktop();

            var move = MouseEvent(AbsoluteVirtualDesktopMove);
            move.Data.Mouse.X = NormalizePixel(target.X, desktop.Left, desktop.Right - desktop.Left);
            move.Data.Mouse.Y = NormalizePixel(target.Y, desktop.Top, desktop.Bottom - desktop.Top);
            RequireWheelTarget(window, child, target);
            Send(new[] { move });
            Point cursor;
            var calibration = Stopwatch.StartNew();
            do
            {
                RequireWheelTarget(window, child, target);
                evidence.Calibrated = GetPhysicalCursorPos(out cursor) && SamePoint(cursor, target);
                if (evidence.Calibrated) break;
                Thread.Sleep(2);
            } while (calibration.ElapsedMilliseconds < 40);
            if (!evidence.Calibrated)
                throw new TargetUnavailableException("Windows did not confirm the calibrated physical wheel point.");

            long first = 0, last = 0;
            for (int stroke = 1; stroke <= 50; stroke++)
            {
                evidence.StrokeInProgress = stroke;
                Rect currentClient;
                var currentTarget = client;
                if (GetDpiForWindow(window) != dpi || !GetClientRect(window, out currentClient) ||
                    !SameRect(originalClient, currentClient) || !ClientToScreen(window, ref currentTarget) ||
                    !SamePoint(target, currentTarget) || !SameRect(desktop, VirtualDesktop()))
                    throw new TargetUnavailableException("Display or window geometry changed during wheel input.");
                if (!GetPhysicalCursorPos(out cursor))
                    throw new TargetUnavailableException("Windows did not report the physical pointer before wheel input.");
                bool repositioned = !SamePoint(cursor, target);
                RequireWheelTarget(window, child, target);
                long now = Stopwatch.GetTimestamp();
                // SendInput keeps the move and wheel consecutive in the native input stream.
                Send(new[] { move, MouseEvent(0x0800, stroke <= 25 ? -120 : 120) });
                if (stroke <= 25) evidence.AcceptedDown++; else evidence.AcceptedUp++;
                if (repositioned) evidence.Repositioned++;
                if (GetPhysicalCursorPos(out cursor) && SamePoint(cursor, target)) evidence.PhysicalReadbacks++;
                else if (!evidence.FirstMissedReadback.HasValue) evidence.FirstMissedReadback = stroke;
                if (stroke == 1) first = now;
                else
                {
                    evidence.LongestGapMs = Math.Max(evidence.LongestGapMs, (now - last) * 1000.0 / Stopwatch.Frequency);
                    evidence.MeanGapMs = (now - first) * 1000.0 / Stopwatch.Frequency / (stroke - 1);
                }
                last = now;
                evidence.StrokeInProgress = null;
                Thread.Sleep(24);
            }
        }
        finally
        {
            SetThreadDpiAwarenessContext(previous);
        }
    }
}
