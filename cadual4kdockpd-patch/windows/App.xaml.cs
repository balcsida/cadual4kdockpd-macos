using System;
using Microsoft.UI.Xaml;

namespace DockPatch;

public partial class App : Application
{
    private Window? window;
    private bool started;

    public App()
    {
        InitializeComponent();
        UnhandledException += (_, e) =>
        {
            if (!started)
            {
                Program.Report("CADUAL4KDOCKPD Patch could not start.", e.Exception);
                Environment.Exit(1);
            }
            // Later on the window stays open: the engine carries on by itself, and its result must stay visible.
            e.Handled = true;
            Program.Report("CADUAL4KDOCKPD Patch ran into a problem. If the dock is being written, leave everything connected.", e.Exception);
        };
    }

    protected override void OnLaunched(Microsoft.UI.Xaml.LaunchActivatedEventArgs args)
    {
        window = new MainWindow();
        window.Activate();
        started = true;
    }
}
