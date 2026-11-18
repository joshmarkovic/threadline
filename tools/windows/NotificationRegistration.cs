// Per-user notification integration for Threadline's two fixed installer channels.
using System;
using System.IO;
using System.Security;
using System.Runtime.InteropServices;
using Microsoft.Win32;

internal sealed class NotificationRegistration
{
    private readonly string display, identity, app, link, serverKey, senderKey, classKey, classText, scratch;
    private readonly Guid activator;
    private const string SnapshotName = "threadline-notification.snapshot";
    private const string OldLink = "threadline-notification.previous.lnk";
    private const string NextLink = "threadline-notification.new.lnk";

    private NotificationRegistration(bool local)
    {
        display = local ? "Threadline Installer Test" : "Threadline";
        identity = local ? "Threadline.InstallTest" : "Threadline.Desktop";
        activator = new Guid(local ? "df3bb9a2-0794-44a6-b130-a35197093afe" : "5a20cea8-a5d8-4b5e-9be3-4121ba0aa753");
        classText = activator.ToString("B");
        classKey = @"Software\Classes\CLSID\" + classText;
        serverKey = classKey + @"\LocalServer32";
        senderKey = @"Software\Classes\AppUserModelId\" + identity;
        string data = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        string programs = Environment.GetFolderPath(Environment.SpecialFolder.Programs);
        if (!Path.IsPathRooted(data) || !Directory.Exists(programs))
            throw new IOException("The current user's application or Start menu directory is unavailable.");
        app = Path.Combine(data, "Programs", display, "threadline.exe");
        link = Path.Combine(programs, display + ".lnk");
        scratch = AppDomain.CurrentDomain.BaseDirectory;
    }

    private string Command { get { return "\"" + app + "\" --notification-com-server"; } }

    [STAThread]
    private static int Main(string[] args)
    {
        if (!Environment.Is64BitProcess || args.Length != 2 || (args[1] != "Local" && args[1] != "Release"))
            return 20;
        try
        {
            var registration = new NotificationRegistration(args[1] == "Local");
            switch (args[0])
            {
                case "Prepare": registration.Prepare(); break;
                case "Apply": registration.Apply(); break;
                case "Restore": registration.Restore(); break;
                case "Remove": registration.Remove(); break;
                default: return 20;
            }
            Console.WriteLine("Completed " + args[0] + " for the current user's Threadline notification registration.");
            return 0;
        }
        catch (InvalidDataException)
        { Console.Error.WriteLine("The staged notification snapshot or shortcut did not verify. Rerun the matching installer."); return 21; }
        catch (InvalidOperationException)
        { Console.Error.WriteLine("The existing Start menu shortcut or notification registration belongs to another target."); return 22; }
        catch (UnauthorizedAccessException)
        { Console.Error.WriteLine("Windows denied access to the current user's Start menu, registry or setup staging directory."); return 23; }
        catch (SecurityException)
        { Console.Error.WriteLine("Windows policy denied the notification registration operation."); return 24; }
        catch (COMException error)
        { Console.Error.WriteLine("Windows Shell could not verify or save the Threadline shortcut (HRESULT 0x{0:X8}).", error.ErrorCode); return 25; }
        catch (IOException error)
        { Console.Error.WriteLine("The staged notification file could not be read or written (HRESULT 0x{0:X8}).", error.HResult); return 26; }
        catch (SystemException error)
        { Console.Error.WriteLine("The Windows registration helper failed (HRESULT 0x{0:X8}).", error.HResult); return 27; }
    }
    private string Read(string keyName, string name, string expected)
    {
        using (RegistryKey key = Registry.CurrentUser.OpenSubKey(keyName))
        {
            if (key == null) return null;
            object raw = key.GetValue(name, null, RegistryValueOptions.DoNotExpandEnvironmentNames);
            if (raw == null) return null;
            string value = raw as string;
            if (value == null || key.GetValueKind(name) != RegistryValueKind.String ||
                !String.Equals(value, expected, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException();
            return value;
        }
    }
    private string[] Values()
    {
        return new[] { Read(serverKey, "", Command), Read(senderKey, "CustomActivator", classText), Read(senderKey, "DisplayName", display) };
    }
    private static void Write(string keyName, string name, string value)
    {
        if (value != null)
        {
            using (RegistryKey k = Registry.CurrentUser.CreateSubKey(keyName)) k.SetValue(name, value, RegistryValueKind.String);
        }
        else
        {
            using (RegistryKey k = Registry.CurrentUser.OpenSubKey(keyName, true)) if (k != null) k.DeleteValue(name, false);
        }
    }
    private static void RemoveEmpty(string name)
    {
        using (RegistryKey key = Registry.CurrentUser.OpenSubKey(name))
            if (key == null || key.SubKeyCount != 0 || key.ValueCount != 0) return;
        Registry.CurrentUser.DeleteSubKey(name, false);
    }
    private void SetValues(string[] values)
    {
        Write(serverKey, "", values[0]);
        Write(senderKey, "CustomActivator", values[1]);
        Write(senderKey, "DisplayName", values[2]);
        RemoveEmpty(serverKey); RemoveEmpty(classKey); RemoveEmpty(senderKey);
    }
    private void CheckLink()
    {
        if (File.Exists(link) && ((File.GetAttributes(link) & FileAttributes.ReparsePoint) != 0 ||
            !ThreadlineToastShortcut.TargetsExecutable(link, app))) throw new InvalidOperationException();
    }
    private string At(string name) { return Path.Combine(scratch, name); }
    private void Prepare()
    {
        CheckLink();
        string[] values = Values();
        if (File.Exists(link)) File.Copy(link, At(OldLink), false);
        using (var file = new FileStream(At(SnapshotName), FileMode.CreateNew, FileAccess.Write, FileShare.None))
        using (var writer = new BinaryWriter(file))
        {
            writer.Write("Threadline notification registration 1");
            writer.Write(identity); writer.Write(File.Exists(link));
            foreach (string value in values) { writer.Write(value != null); if (value != null) writer.Write(value); }
            file.Flush(true);
        }
    }
    private string[] Snapshot(out bool hadLink)
    {
        string[] expected = { Command, classText, display };
        var file = new FileInfo(At(SnapshotName));
        if (!file.Exists || file.Length > 131072 || (file.Attributes & FileAttributes.ReparsePoint) != 0) throw new InvalidDataException();
        using (var reader = new BinaryReader(file.OpenRead()))
        {
            if (reader.ReadString() != "Threadline notification registration 1" || reader.ReadString() != identity) throw new InvalidDataException();
            hadLink = reader.ReadBoolean();
            string[] values = new string[3];
            for (int i = 0; i < values.Length; i++)
                if (reader.ReadBoolean())
                {
                    values[i] = reader.ReadString();
                    if (!String.Equals(values[i], expected[i], StringComparison.OrdinalIgnoreCase)) throw new InvalidDataException();
                }
            if (reader.BaseStream.Position != reader.BaseStream.Length) throw new InvalidDataException();
            return values;
        }
    }
    private void Apply()
    {
        bool hadLink;
        Snapshot(out hadLink); CheckLink(); Values();
        if (!File.Exists(app)) throw new IOException("The staged replacement executable is unavailable.");
        string staged = At(NextLink);
        ThreadlineToastShortcut.CreateOwnedLinkWithActivator(staged, app, identity, activator);
        if (!ThreadlineToastShortcut.LinkMatches(staged, identity, app) || !ThreadlineToastShortcut.LinkHasActivator(staged, activator)) throw new InvalidDataException();
        File.Copy(staged, link, true);
        SetValues(new[] { Command, classText, display });
        ThreadlineToastShortcut.NotifyPublishedLink(link, hadLink);
    }
    private void Restore()
    {
        if (!File.Exists(At(SnapshotName))) return;
        bool hadLink;
        string[] values = Snapshot(out hadLink);
        CheckLink(); Values();
        bool existed = File.Exists(link);
        if (hadLink)
        {
            if (!ThreadlineToastShortcut.TargetsExecutable(At(OldLink), app)) throw new InvalidDataException();
            File.Copy(At(OldLink), link, true);
        }
        else if (File.Exists(link)) File.Delete(link);
        SetValues(values);
        if (hadLink) ThreadlineToastShortcut.NotifyPublishedLink(link, existed);
        else if (existed) ThreadlineToastShortcut.NotifyRemovedLink(link);
    }
    private void Remove()
    {
        bool hadLink;
        Snapshot(out hadLink); CheckLink(); Values();
        SetValues(new string[3]);
        if (File.Exists(link))
        {
            File.Delete(link);
            ThreadlineToastShortcut.NotifyRemovedLink(link);
        }
    }
}
