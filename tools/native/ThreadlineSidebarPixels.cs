using System;
using System.Drawing;
using System.Runtime.InteropServices;

public static class ThreadlineSidebarPixels
{
    [DllImport("user32.dll")]
    private static extern bool PrintWindow(IntPtr window, IntPtr destination, uint flags);

    // PrintWindow asks this one HWND to draw its own client. No desktop DC is opened.
    public static Bitmap Capture(IntPtr window, uint expectedPid, int width, int height)
    {
        uint pid;
        if (width <= 0 || height <= 0 || (long)width * height > 40000000 ||
            GetWindowThreadProcessId(window, out pid) == 0 || pid != expectedPid)
            throw new ArgumentException("Unexpected fixture window.");
        var image = new Bitmap(width, height);
        try {
            using (var graphics = Graphics.FromImage(image)) {
                var dc = graphics.GetHdc();
                try {
                    if (!PrintWindow(window, dc, 3))
                        throw new InvalidOperationException("The owned client did not provide pixels.");
                } finally { graphics.ReleaseHdc(dc); }
            }
            if (GetWindowThreadProcessId(window, out pid) == 0 || pid != expectedPid)
                throw new InvalidOperationException("The fixture window changed during capture.");
            return image;
        } catch { image.Dispose(); throw; }
    }

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    public sealed class Sample
    {
        public int Left, Top, Right, Bottom, BluePixels, WhitePixels;
        public double DigitCenterX, DigitCenterY;
    }

    public static Sample Measure(Bitmap image, Rectangle row)
    {
        if (row.IsEmpty || !new Rectangle(0, 0, image.Width, image.Height).Contains(row))
            throw new ArgumentException("The sidebar control lies outside the fixture window.");
        var sample = new Sample { Left = row.Right, Top = row.Bottom };
        // Measure the rightmost uninterrupted run of blue-bearing columns. Selected
        // label glyphs can reach the right half of a narrow rail, separated by a gap.
        for (int x = row.Right - 1; x >= row.Left + row.Width / 2; x--) {
            int before = sample.BluePixels;
            for (int y = row.Top; y < row.Bottom; y++) {
                var c = image.GetPixel(x, y);
                if (Math.Abs(c.R - 23) > 8 || Math.Abs(c.G - 62) > 8 || Math.Abs(c.B - 112) > 8) continue;
                sample.BluePixels++;
                sample.Left = Math.Min(sample.Left, x);
                sample.Top = Math.Min(sample.Top, y);
                sample.Right = Math.Max(sample.Right, x + 1);
                sample.Bottom = Math.Max(sample.Bottom, y + 1);
            }
            if (before != 0 && before == sample.BluePixels) break;
        }
        if (sample.BluePixels == 0) return sample;
        double cx = (sample.Left + sample.Right) / 2.0, cy = (sample.Top + sample.Bottom) / 2.0;
        double radius = Math.Min(sample.Right - sample.Left, sample.Bottom - sample.Top) / 2.0;
        int minX = sample.Right, maxX = sample.Left, minY = sample.Bottom, maxY = sample.Top;
        for (int y = sample.Top; y < sample.Bottom; y++) {
            for (int x = sample.Left; x < sample.Right; x++) {
                double dx = x + 0.5 - cx, dy = y + 0.5 - cy;
                if (dx * dx + dy * dy >= 0.80 * 0.80 * radius * radius) continue;
                var c = image.GetPixel(x, y);
                if (c.R < 225 || c.G < 225 || c.B < 225) continue;
                sample.WhitePixels++;
                minX = Math.Min(minX, x); maxX = Math.Max(maxX, x + 1);
                minY = Math.Min(minY, y); maxY = Math.Max(maxY, y + 1);
            }
        }
        if (sample.WhitePixels != 0) {
            sample.DigitCenterX = (minX + maxX) / 2.0 - cx;
            sample.DigitCenterY = (minY + maxY) / 2.0 - cy;
        }
        return sample;
    }
}
