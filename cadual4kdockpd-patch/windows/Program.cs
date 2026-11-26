// The program's entry point: what the XAML compiler would generate, plus a visible report when the
// app cannot start. Without it a WinUI app that fails at start simply never appears.

using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;

namespace DockPatch;

public static class Program
{
    /// <summary>Where the last error is saved, so it can be sent on.</summary>
    public static readonly string ErrorFile = Path.Combine(Path.GetTempPath(), "CADUAL4KDOCKPD-Patch-error.txt");

    [STAThread]
    private static void Main()
    {
        try
        {
            WinRT.ComWrappersSupport.InitializeComWrappers();
            Application.Start(p =>
            {
                SynchronizationContext.SetSynchronizationContext(new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread()));
                _ = new App();
            });
        }
        catch (Exception e)
        {
            Report("CADUAL4KDOCKPD Patch could not start.", e);
            Environment.ExitCode = 2;
        }
    }

    /// <summary>Saves the error and shows it: a failure must not be silent.</summary>
    public static void Report(string what, Exception e)
    {
        var text = $"{what}\n\n{e}";
        try
        {
            Console.Error.WriteLine(text);
            File.WriteAllText(ErrorFile, text);
        }
        catch (Exception)
        {
            // showing it still works
        }
        // MB_ICONERROR
        MessageBoxW(IntPtr.Zero, $"{text}\n\nThis text was also saved to {ErrorFile}", "CADUAL4KDOCKPD Patch", 0x10);
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int MessageBoxW(IntPtr owner, string text, string caption, uint type);
}
