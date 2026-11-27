import SwiftUI

let engine = Engine()

/// The windows the "Dock" menu opens. They are out of the flow: optional, or rarely needed.
let hubsWindow = "hubs", moreWindow = "more"

@main struct PatchApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        Window("CADUAL4KDOCKPD Patch", id: "main") {
            FlowView(engine: engine)
                .frame(minWidth: 720)
        }
        .defaultSize(width: 900, height: 560)
        // the height follows the flow (see FlowView); only the width is the user's
        .windowResizability(.contentSize)
        .commands { DockCommands() }

        // .commandsRemoved: the Dock menu opens these, so they add no items of their own
        Window("Hub firmware 93.92", id: hubsWindow) { HubView(engine: engine) }
            .defaultSize(width: 520, height: 360)
            .commandsRemoved()
        Window("More options", id: moreWindow) { MoreView(engine: engine) }
            .defaultSize(width: 560, height: 420)
            .commandsRemoved()
    }
}

private struct DockCommands: Commands {
    @Environment(\.openWindow) private var openWindow

    var body: some Commands {
        CommandMenu("Dock") {
            Button("Hub firmware 93.92...") { openWindow(id: hubsWindow) }
            Button("More options...") { openWindow(id: moreWindow) }
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        // writing to an engine that has gone must be an error, not the end of the app
        signal(SIGPIPE, SIG_IGN)
        engine.start()
        #if DEBUG
        saveScreenshotIfAsked()
        #endif
    }

    /// Quitting would leave the user without the result of a write, so it waits until the write ends.
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard engine.writing else { return .terminateNow }
        engine.quitRefused = true
        return .terminateCancel
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    #if DEBUG
    /// Development only: `DOCK_UI_SHOT=<file.png>` saves a picture of every open window and quits,
    /// the main one as the given file and the ones from the Dock menu as `<file>-2.png` and so on;
    /// `DOCK_UI_DARK=1` uses the dark appearance. See also `DOCK_UI_DEMO` in Engine.swift.
    private func saveScreenshotIfAsked() {
        let environment = ProcessInfo.processInfo.environment
        guard let file = environment["DOCK_UI_SHOT"] else { return }
        NSApp.appearance = NSAppearance(named: environment["DOCK_UI_DARK"] == nil ? .aqua : .darkAqua)
        Task {
            try? await Task.sleep(for: .seconds(1))
            let windows = NSApp.windows.filter(\.isVisible)
            windows.first?.setContentSize(NSSize(width: 900, height: 1040))
            try? await Task.sleep(for: .seconds(1))
            for (index, window) in windows.enumerated() {
                guard let view = window.contentView, let picture = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { continue }
                view.cacheDisplay(in: view.bounds, to: picture)
                let name = index == 0 ? file : file.replacingOccurrences(of: ".png", with: "-\(index + 1).png")
                try? picture.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: name))
            }
            exit(0)
        }
    }
    #endif
}
