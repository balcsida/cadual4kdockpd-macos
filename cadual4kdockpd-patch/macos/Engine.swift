// The window's side of the engine: starts `cadual4kdockpd-patch serve` and mirrors what it reports.
// The engine owns the dock and the whole flow (see src/serve.rs); nothing here talks to USB.

import Foundation
import Observation

enum Step: String, Decodable, Comparable {
    case connect, identify, patch, restart, test

    var number: Int {
        switch self {
        case .connect: 1
        case .identify: 2
        case .patch: 3
        case .restart: 4
        case .test: 5
        }
    }

    static func < (a: Step, b: Step) -> Bool { a.number < b.number }
}

enum Job: String, Decodable {
    case identify, patch, restore, hubCheck, hubUpdate

    /// Jobs that change the dock: the app must stay open until they end.
    var writes: Bool { self == .patch || self == .restore || self == .hubUpdate }
}

/// The outcome of an action, shown next to it.
struct Note: Decodable, Equatable {
    var ok: Bool
    var text: String
}

/// Where the user is in the flow, as the engine reports it after every change.
struct DockState: Decodable, Equatable {
    var step = Step.connect
    var present = false
    var running: Job?
    var identified: Note?
    var patched: Note?
    var hubNote: Note?
    var restoreNote: Note?
    var alreadyPatched = false
    var unplugged = false
    var reconnects = 0
    var afterFailedWrite = false
    var imageError: String?
    var hubRomsError: String?
}

/// How far a long step is. An empty label means no step is being measured.
struct StepProgress: Decodable, Equatable {
    var label = ""
    var fraction = 0.0
}

/// One line from the engine: exactly one of the three is set.
private struct Event: Decodable {
    var state: DockState?
    var progress: StepProgress?
    var log: String?
}

@Observable final class Engine {
    private(set) var state = DockState()
    private(set) var progress = StepProgress()
    private(set) var log: [String] = []
    /// Why the engine is not running, when it is not.
    private(set) var stopped: String?
    /// The user tried to quit during a write.
    var quitRefused = false

    private var process: Process?
    private var input: FileHandle?

    var writing: Bool { state.running?.writes ?? false }

    func start() {
        #if DEBUG
        // layout checks without a dock: a file of engine output instead of the engine
        if let demo = ProcessInfo.processInfo.environment["DOCK_UI_DEMO"] {
            (try? String(contentsOfFile: demo, encoding: .utf8))?.split(separator: "\n").forEach { take(String($0)) }
            return
        }
        #endif
        guard let program = Bundle.main.url(forAuxiliaryExecutable: "cadual4kdockpd-patch") else {
            stopped = "The app is incomplete: its dock engine is missing."
            return
        }
        let process = Process()
        let toEngine = Pipe()
        let fromEngine = Pipe()
        process.executableURL = program
        process.arguments = ["serve"]
        process.standardInput = toEngine
        process.standardOutput = fromEngine
        do {
            try process.run()
        } catch {
            stopped = "The dock engine could not be started: \(error.localizedDescription)"
            return
        }
        self.process = process
        input = toEngine.fileHandleForWriting
        Task {
            do {
                for try await line in fromEngine.fileHandleForReading.bytes.lines { take(line) }
            } catch {}
            stopped = "The part of this app that talks to the dock has stopped. Quit the app and open it again."
        }
    }

    /// Sends one command to the engine. The engine ends by itself when this app does.
    func send(_ command: String) {
        try? input?.write(contentsOf: Data((command + "\n").utf8))
    }

    private func take(_ line: String) {
        guard let event = try? JSONDecoder().decode(Event.self, from: Data(line.utf8)) else {
            log.append(line)
            return
        }
        if let state = event.state { self.state = state }
        if let progress = event.progress { self.progress = progress }
        if let text = event.log { log.append(text) }
    }
}
