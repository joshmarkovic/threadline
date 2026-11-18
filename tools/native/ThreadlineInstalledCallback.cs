using System;
using System.Runtime.InteropServices;
using System.Text.RegularExpressions;

public static class ThreadlineInstalledCallback
{
    [ComImport, Guid("53E31837-6600-4A81-9395-75CFFE746F94")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface INotificationActivationCallback
    {
        [PreserveSig]
        int Activate([MarshalAs(UnmanagedType.LPWStr)] string appUserModelId,
            [MarshalAs(UnmanagedType.LPWStr)] string invokedArgs, IntPtr inputs, uint count);
    }

    [DllImport("ole32.dll", ExactSpelling = true)]
    private static extern int CoCreateInstance(ref Guid clsid, IntPtr outer, uint context,
        ref Guid iid, [MarshalAs(UnmanagedType.Interface)] out INotificationActivationCallback callback);

    public static void Invoke(string groupId, string historyArgument)
    {
        if (groupId == null || !Regex.IsMatch(groupId, @"\A[0-9a-f]{32}\z") ||
            (historyArgument != "activity1." + groupId + ".open" &&
             historyArgument != "activity1." + groupId + ".read" &&
             historyArgument != "activity1." + groupId + ".snooze"))
            throw new ArgumentException("Expected an exact action from the fixture-owned notification.");
        Guid clsid = new Guid("df3bb9a2-0794-44a6-b130-a35197093afe");
        Guid iid = typeof(INotificationActivationCallback).GUID;
        INotificationActivationCallback callback;
        const uint LocalServer = 4;
        Marshal.ThrowExceptionForHR(CoCreateInstance(ref clsid, IntPtr.Zero, LocalServer, ref iid, out callback));
        try
        {
            Marshal.ThrowExceptionForHR(callback.Activate("Threadline.InstallTest", historyArgument, IntPtr.Zero, 0));
        }
        finally
        {
            Marshal.FinalReleaseComObject(callback);
        }
    }
}
