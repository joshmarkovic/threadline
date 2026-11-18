using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Threading;

public static class ThreadlineWinInput
{
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
            throw new InvalidOperationException("The Threadline test window lost foreground focus.");
    }

    private static void Send(Input[] input, bool requireFocus = true)
    {
        if (requireFocus) RequireTargetFocus();
        if (SendInput((uint)input.Length, input, Marshal.SizeOf(typeof(Input))) != input.Length)
            throw new Win32Exception(Marshal.GetLastWin32Error(), "SendInput did not inject every event.");
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
            events[events.Length - index - 1] = Key(key, 0, flags | 2);
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
            events[index * 2 + 1] = Key(0, character, 6);
        }
        Send(events);
    }

    private static void Move(Point point)
    {
        RequireTargetFocus();
        if (WindowFromPoint(point) != TargetWindow)
            throw new InvalidOperationException("A different native window covers the requested Threadline control.");
        if (!SetCursorPos(point.X, point.Y)) throw new Win32Exception(Marshal.GetLastWin32Error());
    }

    private static void Mouse(uint flags, int data = 0, bool requireFocus = true)
    {
        Send(new[] { new Input
        {
            Data = new InputData { Mouse = new MouseInput { Flags = flags, Data = unchecked((uint)data) } }
        } }, requireFocus);
    }

    public static void Drag(Point from, Point to)
    {
        Move(from);
        Mouse(2);
        try
        {
            for (int step = 1; step <= 12; step++)
            {
                Move(new Point { X = from.X + (to.X - from.X) * step / 12, Y = from.Y + (to.Y - from.Y) * step / 12 });
                Thread.Sleep(16);
            }
        }
        finally { Mouse(4, requireFocus: false); }
    }

    public static void Click(Point point)
    {
        Move(point);
        Mouse(2);
        Mouse(4);
    }

    public static void ClickShellControl(Point point, uint shellProcess)
    {
        uint actualProcess;
        if (GetWindowThreadProcessId(WindowFromPoint(point), out actualProcess) == 0 ||
            actualProcess != shellProcess)
            throw new InvalidOperationException("The selected point is no longer in the shell control.");

        if (!SetCursorPos(point.X, point.Y)) throw new Win32Exception(Marshal.GetLastWin32Error());
        Mouse(2, requireFocus: false);
        Mouse(4, requireFocus: false);
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
            Mouse(2, requireFocus: false);
            Mouse(4, requireFocus: false);
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
            Mouse(0x0800, movement);
            Thread.Sleep(intervalMilliseconds);
        }
    }
}
