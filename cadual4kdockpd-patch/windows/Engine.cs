// The window's side of the engine: starts `cadual4kdockpd-patch.exe serve` (it is unpacked next to
// this program's own files) and mirrors what it reports. The engine owns the dock and the whole flow (see
// ../src/serve.rs); nothing here talks to USB.

using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Text.Json;
using System.Threading.Tasks;
using Microsoft.UI.Dispatching;

namespace DockPatch;

/// <summary>The outcome of an action, shown next to it.</summary>
public sealed record Note(bool Ok, string Text);

/// <summary>Where the user is in the flow, as the engine reports it after every change.</summary>
public sealed record DockState(
    string Step = "connect",
    bool Present = false,
    string? Running = null,
    Note? Identified = null,
    Note? Patched = null,
    Note? HubNote = null,
    Note? RestoreNote = null,
    bool AlreadyPatched = false,
    bool Unplugged = false,
    int Reconnects = 0,
    bool AfterFailedWrite = false,
    string? ImageError = null,
    string? HubRomsError = null)
{
    /// <summary>1 to 5: connect, identify, patch, restart, test.</summary>
    public int StepNumber => Array.IndexOf(["connect", "identify", "patch", "restart", "test"], Step) + 1;
}

/// <summary>How far a long step is. An empty label means no step is being measured.</summary>
public sealed record StepProgress(string Label = "", double Fraction = 0);

/// <summary>One line from the engine: exactly one of the three is set.</summary>
public sealed record EngineEvent(DockState? State = null, StepProgress? Progress = null, string? Log = null);

public sealed class Engine
{
    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web);
    private Process? process;

    public DockState State { get; private set; } = new();
    public StepProgress Progress { get; private set; } = new();
    public List<string> Log { get; } = [];

    /// <summary>Why the engine is not running, when it is not.</summary>
    public string? Stopped { get; private set; }

    /// <summary>A job that changes the dock is running: the window must stay open until it ends.</summary>
    public bool Writing => State.Running is "patch" or "restore" or "hubUpdate";

    /// <summary>Raised on the window's thread after every line from the engine.</summary>
    public event Action? Changed;

    public void Start(DispatcherQueue window)
    {
        var program = Path.Combine(AppContext.BaseDirectory, "cadual4kdockpd-patch.exe");
        var utf8 = new UTF8Encoding(encoderShouldEmitUTF8Identifier: false);
        var start = new ProcessStartInfo(program, "serve")
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            // the default is the console's code page, which would garble a file path with accents
            StandardInputEncoding = utf8,
            StandardOutputEncoding = utf8,
        };
        // a build for internal use carries the firmware image at the end of this program's own file
        start.Environment["DOCK_IMAGE_IN"] = Environment.ProcessPath ?? "";
        try
        {
            process = Process.Start(start);
        }
        catch (Exception e) when (e is System.ComponentModel.Win32Exception or InvalidOperationException)
        {
            Stopped = $"The dock engine ({program}) could not be started: {e.Message}";
            Changed?.Invoke();
            return;
        }
        if (process is null)
        {
            return;
        }
        var output = process.StandardOutput;
        Task.Run(async () =>
        {
            // read on a thread of its own, so a busy window never keeps the engine waiting
            while (await output.ReadLineAsync() is { } line)
            {
                window.TryEnqueue(() => Take(line));
            }
            window.TryEnqueue(() =>
            {
                Stopped = "The part of this app that talks to the dock has stopped. Close the app and open it again.";
                Changed?.Invoke();
            });
        });
    }

    /// <summary>Sends one command to the engine. The engine ends by itself when this app does.</summary>
    public void Send(string command)
    {
        try
        {
            process?.StandardInput.WriteLine(command);
            process?.StandardInput.Flush();
        }
        catch (Exception e) when (e is IOException or InvalidOperationException)
        {
            // the engine has gone; the read loop reports it
        }
    }

    private void Take(string line)
    {
        EngineEvent? engineEvent = null;
        try
        {
            engineEvent = JsonSerializer.Deserialize<EngineEvent>(line, Json);
        }
        catch (JsonException)
        {
        }
        if (engineEvent is null)
        {
            Log.Add(line);
        }
        else
        {
            State = engineEvent.State ?? State;
            Progress = engineEvent.Progress ?? Progress;
            if (engineEvent.Log is { } text)
            {
                Log.Add(text);
            }
        }
        Changed?.Invoke();
    }
}
