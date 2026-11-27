using System;
using System.Runtime.InteropServices;
using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Windows.Graphics;
using Windows.UI;

namespace DockPatch;

/// <summary>Shows the engine's state. The layout and all fixed text are in MainWindow.xaml.</summary>
public sealed partial class MainWindow : Window
{
    private static readonly string[] StepTitles = ["Connect the dock", "Identify the dock", "Apply the fix", "Restart the dock", "Test it"];

    // Fixed colours that read on the light and on the dark theme; white on the first two has a contrast of at least 4.5:1.
    private static readonly Color Accent = ColorHelper.FromArgb(0xff, 0x1f, 0x6f, 0xeb);
    private static readonly Color Done = ColorHelper.FromArgb(0xff, 0x2e, 0x7d, 0x32);
    private static readonly Color Upcoming = ColorHelper.FromArgb(0xff, 0x80, 0x80, 0x80);

    private readonly Engine engine = new();
    private (int Step, bool Passed) drawn;
    private bool closeRefused;
    private bool restoreAsked;
    private bool moreOpened;
    /// <summary>WinUI shows one dialog at a time; a second ShowAsync would throw.</summary>
    private bool dialogOpen;

    public MainWindow()
    {
        InitializeComponent();
        var scale = GetDpiForWindow(WinRT.Interop.WindowNative.GetWindowHandle(this)) / 96.0;
        AppWindow.Resize(new SizeInt32((int)(780 * scale), (int)(860 * scale)));
        // closing the window would leave the user without the result of a write
        AppWindow.Closing += (_, e) =>
        {
            if (engine.Writing)
            {
                e.Cancel = true;
                closeRefused = true;
                Render();
            }
        };
        // the dialogs need the loaded window, which the engine's first report may come before
        Root.Loaded += (_, _) => Render();
        // the dialog holds the progress of a restore: it stays until the restore ends
        MoreDialog.Closing += (_, e) => e.Cancel = engine.State.Running == "restore";
        // a new line, or the box being opened, shows the newest lines; after layout, which sets the height
        LogText.SizeChanged += (_, _) => DispatcherQueue.TryEnqueue(() => LogScroll.ChangeView(null, LogScroll.ScrollableHeight, null, true));
        engine.Changed += Render;
        Render();
        engine.Start(DispatcherQueue);
    }

    [DllImport("user32.dll")]
    private static extern uint GetDpiForWindow(IntPtr window);

    private void Render()
    {
        var s = engine.State;
        var idle = s.Running is null;
        var step = s.StepNumber;

        Open(StoppedBar, engine.Stopped is not null);
        StoppedBar.Message = engine.Stopped ?? "";
        Open(CloseRefusedBar, closeRefused && engine.Writing);

        Summary2.Text = s.Identified?.Text ?? "";
        Summary3.Text = s.AlreadyPatched ? "Not needed: the fix is already in this dock." : "Written and verified.";
        Summary4.Text = s.AlreadyPatched ? "Not needed." : "Restarted.";
        Grid[] badges = [Badge1, Badge2, Badge3, Badge4, Badge5];
        TextBlock[] titles = [Title1, Title2, Title3, Title4, Title5];
        TextBlock?[] summaries = [Summary1, Summary2, Summary3, Summary4, null];
        StackPanel[] bodies = [Body1, Body2, Body3, Body4, Body5];
        // the last step has no step after it, so three plug-ins without a loop mark it done
        var passed = step == 5 && s.Reconnects >= 3;
        for (var i = 0; i < 5; i++)
        {
            var (done, current) = (step > i + 1 || (i == 4 && passed), step == i + 1);
            if (drawn != (step, passed))
            {
                DrawBadge(badges[i], i + 1, done, current);
            }
            titles[i].FontWeight = current ? FontWeights.SemiBold : FontWeights.Normal;
            titles[i].Opacity = done || current ? 1 : 0.6;
            AutomationProperties.SetName(titles[i], $"Step {i + 1}, {StepTitles[i]}, {(done ? "done" : current ? "current step" : "not started")}");
            Show(bodies[i], current);
            if (summaries[i] is { } summary)
            {
                Show(summary, done && summary.Text.Length > 0);
            }
        }
        drawn = (step, passed);

        // 2: identify
        Show(IdentifyBusy, s.Running == "identify");
        Show(IdentifyButton, s.Running != "identify");
        IdentifyButton.Content = s.Identified is null ? "Identify the dock" : "Try again";
        IdentifyButton.IsEnabled = idle;
        ShowNote(IdentifyNote, s.Identified);

        // 3: apply the fix
        var writing = s.Running == "patch";
        var failed = !writing && s.AfterFailedWrite;
        var missing = !writing && !failed && s.ImageError is not null;
        Show(PatchWriting, writing);
        Show(PatchFailed, failed);
        Open(PatchMissing, missing);
        PatchMissing.Message = s.ImageError ?? "";
        Show(PatchConfirm, !writing && !failed && !missing);
        WriteAgainButton.IsEnabled = idle;
        PatchButton.IsEnabled = idle && PatchWord.Text.Trim() == "ERASE";
        ShowNote(PatchNote, s.Patched);
        ShowProgress(PatchProgressText, PatchProgressBar);

        // 4: restart
        UnplugLine.Text = "1.  Unplug the USB-C cable from this computer." + (s.Unplugged ? "  Done." : "");
        RestartWaiting.Text = s.Unplugged ? "Waiting for the dock to come back..." : "Waiting for you to unplug the cable...";

        // 5: test
        Show(PresentIcon, s.Present);
        PresentText.Text = s.Present ? "Dock connected" : "Dock not connected";
        Open(TestDone, s.Reconnects >= 3);
        TestDone.Title = $"Connected {s.Reconnects} times since the restart";
        Show(TestCount, s.Reconnects is > 0 and < 3);
        TestCount.Text = $"Connected {s.Reconnects} time(s) since the restart. Try it three times.";

        // more options
        RestoreButton.IsEnabled = idle && s.ImageError is null && !restoreAsked;
        Show(RestoreConfirm, restoreAsked);
        RestoreGo.IsEnabled = idle && RestoreWord.Text.Trim() == "ERASE";
        Show(RestoreProgress, s.Running == "restore");
        ShowProgress(RestoreProgressText, RestoreProgressBar);
        ShowNote(RestoreNote, s.RestoreNote);
        SkipCheck.IsChecked = s.AfterFailedWrite;
        SkipCheck.IsEnabled = idle;
        Show(ImagePanel, s.ImageError is not null);
        ImageMissing.Message = s.ImageError ?? "";
        UseImageButton.IsEnabled = idle && ImagePath.Text.Trim().Length > 0;
        if (s.ImageError is not null && !moreOpened && Content.XamlRoot is not null)
        {
            // a build without the firmware inside: this is where its image file is chosen
            moreOpened = true;
            ShowDialog(MoreDialog);
        }

        LogText.Text = engine.Log.Count == 0 ? "Nothing has been done yet." : string.Join("\n", engine.Log);
    }

    /// <summary>The step's number in a circle: a tick when done, filled when current.</summary>
    private static void DrawBadge(Grid host, int number, bool done, bool current)
    {
        host.Children.Clear();
        var circle = new Ellipse { Width = 28, Height = 28 };
        if (done || current)
        {
            circle.Fill = new SolidColorBrush(done ? Done : Accent);
        }
        else
        {
            circle.Stroke = new SolidColorBrush(Upcoming);
            circle.StrokeThickness = 1.5;
        }
        host.Children.Add(circle);
        FrameworkElement mark = done
            ? new FontIcon { Glyph = "", FontSize = 14, Foreground = new SolidColorBrush(Colors.White) }
            : new TextBlock
            {
                Text = number.ToString(),
                FontSize = 14,
                FontWeight = current ? FontWeights.SemiBold : FontWeights.Normal,
                Foreground = new SolidColorBrush(current ? Colors.White : Upcoming),
            };
        mark.HorizontalAlignment = HorizontalAlignment.Center;
        mark.VerticalAlignment = VerticalAlignment.Center;
        host.Children.Add(mark);
    }

    private static void Show(UIElement element, bool visible)
    {
        element.Visibility = visible ? Visibility.Visible : Visibility.Collapsed;
    }

    /// <summary>A closed message bar would still take up a row's spacing, so it is hidden as well.</summary>
    private static void Open(InfoBar bar, bool open)
    {
        bar.IsOpen = open;
        Show(bar, open);
    }

    /// <summary>The outcome of an action, next to the action it belongs to.</summary>
    private static void ShowNote(InfoBar bar, Note? note)
    {
        Open(bar, note is not null && (!note.Ok || note.Text.Length > 0));
        if (note is null)
        {
            return;
        }
        bar.Severity = note.Ok ? InfoBarSeverity.Success : InfoBarSeverity.Error;
        bar.Title = note.Ok ? note.Text : "That did not work";
        bar.Message = note.Ok ? "" : note.Text;
    }

    private void ShowProgress(TextBlock text, ProgressBar bar)
    {
        var progress = engine.Progress;
        bar.IsIndeterminate = progress.Label.Length == 0;
        bar.Value = progress.Fraction * 100;
        text.Text = progress.Label.Length == 0 ? "Working..." : $"{progress.Label}: {Math.Round(progress.Fraction * 100)}%";
    }

    private async void ShowDialog(ContentDialog dialog)
    {
        if (dialogOpen)
        {
            return;
        }
        dialogOpen = true;
        dialog.XamlRoot = Content.XamlRoot;
        await dialog.ShowAsync();
        dialogOpen = false;
    }

    private void Hubs_Click(object sender, RoutedEventArgs e) => ShowDialog(HubsDialog);

    private void More_Click(object sender, RoutedEventArgs e) => ShowDialog(MoreDialog);

    private void Identify_Click(object sender, RoutedEventArgs e) => engine.Send("identify");

    private void WriteAgain_Click(object sender, RoutedEventArgs e) => engine.Send("patch");

    private void Patch_Click(object sender, RoutedEventArgs e)
    {
        // the engine checks the typed word again before it starts
        engine.Send($"patch {PatchWord.Text.Trim()}");
        PatchWord.Text = "";
    }

    private void Restarted_Click(object sender, RoutedEventArgs e) => engine.Send("restarted");

    private void RestoreAsk_Click(object sender, RoutedEventArgs e)
    {
        restoreAsked = true;
        RestoreWord.Text = "";
        Render();
    }

    private void Restore_Click(object sender, RoutedEventArgs e)
    {
        engine.Send($"restore {RestoreWord.Text.Trim()}");
        restoreAsked = false;
        RestoreWord.Text = "";
        Render();
    }

    private void RestoreCancel_Click(object sender, RoutedEventArgs e)
    {
        restoreAsked = false;
        Render();
    }

    private void SkipCheck_Click(object sender, RoutedEventArgs e) => engine.Send("skip-check " + (SkipCheck.IsChecked == true ? "on" : "off"));

    private void UseImage_Click(object sender, RoutedEventArgs e) => engine.Send($"image {ImagePath.Text.Trim()}");

    private void Text_Changed(object sender, TextChangedEventArgs e) => Render();
}
