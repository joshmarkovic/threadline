using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
using System.Text;

public static class ThreadlineToastShortcut
{
    [ComImport, Guid("00021401-0000-0000-C000-000000000046")]
    private class ShellLink { }

    [ComImport, Guid("000214F9-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IShellLinkW
    {
        void GetPath([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder path, int capacity, IntPtr findData, uint flags);
        void GetIDList(out IntPtr idList);
        void SetIDList(IntPtr idList);
        void GetDescription([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder description, int capacity);
        void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string description);
        void GetWorkingDirectory([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder directory, int capacity);
        void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string directory);
        void GetArguments([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder arguments, int capacity);
        void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string arguments);
        void GetHotkey(out ushort hotkey);
        void SetHotkey(ushort hotkey);
        void GetShowCmd(out int show);
        void SetShowCmd(int show);
        void GetIconLocation([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder iconFile, int capacity, out int iconIndex);
        void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string iconFile, int iconIndex);
        void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string path, uint reserved);
        void Resolve(IntPtr window, uint flags);
        void SetPath([MarshalAs(UnmanagedType.LPWStr)] string path);
    }

    [StructLayout(LayoutKind.Sequential, Pack = 4)]
    private struct PropertyKey
    {
        public Guid Format;
        public uint Id;
    }

    [StructLayout(LayoutKind.Explicit, Size = 24)]
    private struct PropVariant
    {
        [FieldOffset(0)] public ushort Type;
        [FieldOffset(8)] public IntPtr String;
    }

    [ComImport, Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IPropertyStore
    {
        void GetCount(out uint count);
        void GetAt(uint index, out PropertyKey key);
        void GetValue(ref PropertyKey key, out PropVariant value);
        void SetValue(ref PropertyKey key, ref PropVariant value);
        void Commit();
    }

    [DllImport("ole32.dll")]
    private static extern int PropVariantClear(ref PropVariant prop);

    [DllImport("shell32.dll", CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern void SHChangeNotify(uint eventId, uint flags, string item, IntPtr unused);

    public static void CreateOwnedLink(string linkPath, string binary, string appId)
    {
        CreateOwnedLinkWithActivator(linkPath, binary, appId, Guid.Empty);
    }

    public static void CreateOwnedLinkWithActivator(string linkPath, string binary, string appId, Guid activator)
    {
        if (File.Exists(linkPath)) throw new IOException("The unique fixture shortcut path already exists.");
        object link = new ShellLink();
        var value = new PropVariant { Type = 31, String = Marshal.StringToCoTaskMemUni(appId) };
        try
        {
            var shellLink = (IShellLinkW)link;
            shellLink.SetPath(binary);
            shellLink.SetWorkingDirectory(Path.GetDirectoryName(binary));
            shellLink.SetDescription("Open Threadline");
            var store = (IPropertyStore)link;
            var key = new PropertyKey { Format = new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"), Id = 5 };
            store.SetValue(ref key, ref value);
            if (activator != Guid.Empty)
            {
                key.Id = 26;
                var clsid = new PropVariant { Type = 72, String = Marshal.AllocCoTaskMem(16) };
                try
                {
                    Marshal.StructureToPtr(activator, clsid.String, false);
                    store.SetValue(ref key, ref clsid);
                }
                finally { PropVariantClear(ref clsid); }
            }
            store.Commit();
            ((IPersistFile)link).Save(linkPath, true);
        }
        finally
        {
            PropVariantClear(ref value);
            Marshal.FinalReleaseComObject(link);
        }
        SHChangeNotify(0x2, 0x1005, linkPath, IntPtr.Zero);
    }

    public static bool RemoveOwnedLink(string linkPath, string appId)
    {
        if (!File.Exists(linkPath)) return true;
        if (!LinkMatches(linkPath, appId, null)) return false;
        File.Delete(linkPath);
        SHChangeNotify(0x4, 0x1005, linkPath, IntPtr.Zero);
        return !File.Exists(linkPath);
    }

    public static bool LinkMatches(string linkPath, string appId, string sender)
    {
        if (!File.Exists(linkPath)) return false;
        object link = new ShellLink();
        try
        {
            ((IPersistFile)link).Load(linkPath, 0);
            var key = new PropertyKey { Format = new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"), Id = 5 };
            PropVariant value;
            ((IPropertyStore)link).GetValue(ref key, out value);
            try
            {
                if (value.Type != 31 || Marshal.PtrToStringUni(value.String) != appId) return false;
                if (sender == null) return true;
                var target = new StringBuilder(32768);
                ((IShellLinkW)link).GetPath(target, target.Capacity, IntPtr.Zero, 0);
                string resolved = target.ToString();
                return File.Exists(resolved) && String.Equals(Path.GetFullPath(resolved), Path.GetFullPath(sender), StringComparison.OrdinalIgnoreCase);
            }
            finally { PropVariantClear(ref value); }
        }
        finally { Marshal.FinalReleaseComObject(link); }
    }

    public static bool TargetsExecutable(string linkPath, string sender)
    {
        if (!File.Exists(linkPath)) return false;
        object link = new ShellLink();
        try
        {
            ((IPersistFile)link).Load(linkPath, 0);
            var shell = (IShellLinkW)link;
            var target = new StringBuilder(32768);
            var arguments = new StringBuilder(32768);
            shell.GetPath(target, target.Capacity, IntPtr.Zero, 4);
            shell.GetArguments(arguments, arguments.Capacity);
            return arguments.Length == 0 && target.Length > 0
                && String.Equals(Path.GetFullPath(target.ToString()), Path.GetFullPath(sender), StringComparison.OrdinalIgnoreCase);
        }
        finally { Marshal.FinalReleaseComObject(link); }
    }

    public static bool LinkHasActivator(string linkPath, Guid expected)
    {
        if (!File.Exists(linkPath)) return false;
        object link = new ShellLink();
        try
        {
            ((IPersistFile)link).Load(linkPath, 0);
            var key = new PropertyKey { Format = new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"), Id = 26 };
            PropVariant value;
            ((IPropertyStore)link).GetValue(ref key, out value);
            try { return value.Type == 72 && value.String != IntPtr.Zero && (Guid)Marshal.PtrToStructure(value.String, typeof(Guid)) == expected; }
            finally { PropVariantClear(ref value); }
        }
        finally { Marshal.FinalReleaseComObject(link); }
    }
}
