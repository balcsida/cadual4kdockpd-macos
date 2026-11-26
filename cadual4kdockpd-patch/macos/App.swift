import SwiftUI

let engine = Engine()

@main struct PatchApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        Window("CADUAL4KDOCKPD Patch", id: "main") {
            FlowView(engine: engine)
                .frame(minWidth: 560, minHeight: 520)
        }
        .defaultSize(width: 720, height: 780)
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
    /// Development only: `DOCK_UI_SHOT=<file.png>` saves a picture of the window and quits;
    /// `DOCK_UI_DARK=1` uses the dark appearance. See also `DOCK_UI_DEMO` in Engine.swift.
    private func saveScreenshotIfAsked() {
        let environment = ProcessInfo.processInfo.environment
        guard let file = environment["DOCK_UI_SHOT"] else { return }
        NSApp.appearance = NSAppearance(named: environment["DOCK_UI_DARK"] == nil ? .aqua : .darkAqua)
        Task {
            try? await Task.sleep(for: .seconds(1))
            NSApp.windows.first?.setContentSize(NSSize(width: 720, height: 1040))
            try? await Task.sleep(for: .seconds(1))
            if let view = NSApp.windows.first?.contentView, let picture = view.bitmapImageRepForCachingDisplay(in: view.bounds) {
                view.cacheDisplay(in: view.bounds, to: picture)
                try? picture.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: file))
            }
            exit(0)
        }
    }
    #endif
}
