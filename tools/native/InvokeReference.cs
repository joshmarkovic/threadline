using System;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using System.Windows.Automation;
using System.Windows.Automation.Provider;
using System.Windows.Forms;

[ComVisible(true), ClassInterface(ClassInterfaceType.None)]
public sealed class ReferenceProvider : IRawElementProviderSimple, IInvokeProvider
{
    private readonly Control owner;
    private readonly IntPtr hwnd;
    private readonly string directory;
    private int sequence;

    public ReferenceProvider(Control owner, IntPtr hwnd, string directory)
    {
        this.owner = owner;
        this.hwnd = hwnd;
        this.directory = directory;
    }

    public ProviderOptions ProviderOptions { get { return ProviderOptions.ServerSideProvider; } }
    public IRawElementProviderSimple HostRawElementProvider { get { return AutomationInteropProvider.HostProviderFromHandle(hwnd); } }
    public object GetPatternProvider(int id) { return id == InvokePatternIdentifiers.Pattern.Id ? this : null; }
    public object GetPropertyValue(int id)
    {
        if (id == AutomationElementIdentifiers.AutomationIdProperty.Id) return "threadline-reference-invoke";
        if (id == AutomationElementIdentifiers.NameProperty.Id) return "Threadline Invoke reference";
        if (id == AutomationElementIdentifiers.ControlTypeProperty.Id) return ControlType.Button.Id;
        if (id == AutomationElementIdentifiers.IsEnabledProperty.Id) return true;
        if (id == AutomationElementIdentifiers.IsControlElementProperty.Id) return true;
        if (id == AutomationElementIdentifiers.IsContentElementProperty.Id) return true;
        return null;
    }

    public void Invoke()
    {
        long entered = Stopwatch.GetTimestamp();
        int n = Interlocked.Increment(ref sequence);
        owner.BeginInvoke((Action)delegate {
            long handled = Stopwatch.GetTimestamp();
            Program.Publish(directory, "sample-" + n, entered + "," + handled);
        });
    }
}

internal sealed class ReferenceButton : Control
{
    private readonly string directory;
    private ReferenceProvider provider;
    internal ReferenceButton(string directory) { this.directory = directory; }
    protected override void OnHandleCreated(EventArgs e)
    {
        base.OnHandleCreated(e);
        provider = new ReferenceProvider(this, Handle, directory);
    }
    protected override void WndProc(ref Message msg)
    {
        if (msg.Msg == 0x003D && msg.LParam.ToInt32() == AutomationInteropProvider.RootObjectId && provider != null)
        {
            msg.Result = AutomationInteropProvider.ReturnRawElementProvider(Handle, msg.WParam, msg.LParam, provider);
            return;
        }
        base.WndProc(ref msg);
    }
}

internal sealed class ReferenceForm : Form
{
    protected override bool ShowWithoutActivation { get { return true; } }
    protected override CreateParams CreateParams
    {
        get { var cp = base.CreateParams; cp.ExStyle |= 0x08000000; return cp; }
    }
}

internal static class Program
{
    internal static void Publish(string dir, string name, string value)
    {
        string target = Path.Combine(dir, name);
        File.WriteAllText(target + ".partial", value);
        File.Move(target + ".partial", target);
    }

    [STAThread]
    private static int Main(string[] args)
    {
        if (args.Length != 1 || !Directory.Exists(args[0])) return 2;
        string dir = args[0];
        using (var form = new ReferenceForm())
        using (var timer = new System.Windows.Forms.Timer())
        {
            var lifetime = Stopwatch.StartNew();
            var button = new ReferenceButton(dir) { Dock = DockStyle.Fill };
            form.Text = "Threadline UI Automation timing fixture";
            form.ShowInTaskbar = false;
            form.Width = 240;
            form.Height = 90;
            form.Controls.Add(button);
            form.Shown += delegate {
                Publish(dir, "ready", button.Handle.ToInt64().ToString(CultureInfo.InvariantCulture)
                    + "," + Stopwatch.Frequency.ToString(CultureInfo.InvariantCulture));
            };
            timer.Interval = 100;
            timer.Tick += delegate {
                if (File.Exists(Path.Combine(dir, "stop")) || lifetime.Elapsed.TotalSeconds > 120) form.Close();
            };
            timer.Start();
            Application.Run(form);
        }
        return 0;
    }
}
