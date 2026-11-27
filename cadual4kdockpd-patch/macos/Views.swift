// The window: a guided flow for the Mac loop fix, one step at a time, drawn from the engine's state.
// Optional and rarely needed actions sit in their own windows, opened from the "Dock" menu. Every
// write is confirmed here by typing a word; the engine checks the word again before it starts.

import SwiftUI

// White on this green has a contrast of 5:1; the system green is too light for a white tick.
private let doneGreen = Color(red: 0.18, green: 0.49, blue: 0.20)

private let replugHint = """
    On a Mac an unpatched dock connects on about one plug-in in three. If nothing happens within 5 seconds, \
    unplug the cable, wait 3 seconds and plug it in again. Once connected, it stays connected.
    """

struct FlowView: View {
    let engine: Engine

    /// The word typed for the main step's write.
    @State private var typed = ""
    @State private var showLog = false
    /// The flow's own height: the window fits it.
    @State private var flowHeight: CGFloat = 600
    @Environment(\.openWindow) private var openWindow

    private var state: DockState { engine.state }
    private var idle: Bool { state.running == nil }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Fix the dock's connect and disconnect loop")
                        .font(.title).fontWeight(.semibold)
                        .accessibilityAddTraits(.isHeader)
                    Text("For the i-tec CADUAL4KDOCKPD's loop on Macs. Five steps, about 15 minutes.")
                        .foregroundStyle(.secondary)
                }
                if let stopped = engine.stopped {
                    Callout(tone: .danger, title: "This app cannot talk to the dock") { Text(stopped) }
                }
                if engine.quitRefused && engine.writing {
                    Callout(tone: .warning, title: "The app stays open until the dock has been written and checked") {}
                }

                // the steps down the left, the current one's contents on the right: wide rather than tall
                HStack(alignment: .top, spacing: 24) {
                    VStack(alignment: .leading, spacing: 14) {
                        StepRow(step: .connect, current: state.step, summary: "Dock connected.")
                        StepRow(step: .identify, current: state.step, summary: state.identified?.text ?? "")
                        StepRow(step: .patch, current: state.step,
                                summary: state.alreadyPatched ? "Not needed: the fix is already in this dock." : "Written and verified.")
                        StepRow(step: .restart, current: state.step, summary: state.alreadyPatched ? "Not needed." : "Restarted.")
                        StepRow(step: .test, current: state.step, summary: "", done: testPassed)
                    }
                    .frame(width: 230, alignment: .leading)
                    Divider()
                    VStack(alignment: .leading, spacing: 10) {
                        Text(state.step.title)
                            .font(.title2).fontWeight(.semibold)
                            .accessibilityAddTraits(.isHeader)
                        currentStep
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }

                Divider()
                Collapsible(title: "Details", isExpanded: $showLog) { logSection }
            }
            .frame(maxWidth: 1000, alignment: .leading)
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .leading)
            // measured here, inside the scroll view: a preference would not get out of it on macOS
            .background(GeometryReader { flow in
                Color.clear.onChange(of: flow.size.height, initial: true) { _, height in flowHeight = height }
            })
        }
        // as tall as the flow, so it scrolls only where the screen is too short for all of it
        .frame(height: min(flowHeight, (NSScreen.main?.visibleFrame.height ?? 800) - 40))
        .scrollBounceBehavior(.basedOnSize)
        .controlSize(.large)
        // closing the window would leave the user without the result of a write
        .onChange(of: engine.writing) { _, writing in
            for window in NSApp.windows { window.standardWindowButton(.closeButton)?.isEnabled = !writing }
        }
        // a build without the firmware inside: the image file is chosen in the "More options" window
        .onChange(of: state.imageError, initial: true) { _, missing in
            if missing != nil { openWindow(id: moreWindow) }
        }
        #if DEBUG
        .onAppear {
            if ProcessInfo.processInfo.environment["DOCK_UI_OPEN"] != nil {
                showLog = true
                openWindow(id: hubsWindow)
                openWindow(id: moreWindow)
            }
        }
        #endif
    }

    /// Three plug-ins without a loop: the last step has done what it is there for.
    private var testPassed: Bool { state.step == .test && state.reconnects >= 3 }

    // MARK: The five steps

    @ViewBuilder private var currentStep: some View {
        switch state.step {
        case .connect: connectStep
        case .identify: identifyStep
        case .patch: patchStep
        case .restart: restartStep
        case .test: testStep
        }
    }

    @ViewBuilder private var connectStep: some View {
        Text("Plug in the dock's power supply, then connect the dock to this computer.")
        Busy(text: "Looking for the dock...")
        Text(replugHint).foregroundStyle(.secondary)
    }

    @ViewBuilder private var identifyStep: some View {
        Text("A quick look at the dock. It only reads and takes a few seconds.")
        if state.running == .identify {
            Busy(text: "Reading the dock...")
        } else {
            Button(state.identified == nil ? "Identify the dock" : "Try again") { engine.send("identify") }
                .buttonStyle(.borderedProminent)
                .disabled(!idle)
        }
        NoteView(note: state.identified)
    }

    @ViewBuilder private var patchStep: some View {
        if state.running == .patch {
            Callout(tone: .warning, title: "Keep the dock's power supply and the cable connected") {
                Text("The dock is being checked, erased, written and checked again. This takes about 8 minutes.")
            }
            ProgressLine(progress: engine.progress)
        } else if state.afterFailedWrite {
            Callout(tone: .danger, title: "The write could not be verified. Do not unplug the dock's power supply.") {
                Text("The dock keeps working as long as it stays powered. Write again now.")
            }
            Button("Write again") { engine.send("patch") }
                .buttonStyle(.borderedProminent)
                .disabled(!idle)
        } else if let missing = state.imageError {
            Callout(tone: .danger, title: "The firmware image is missing") { Text(missing) }
        } else {
            Callout(tone: .warning, title: "This switches the setting \"Try.SRC\" off. It erases the dock's power controller and writes new firmware.") {
                Bullets(lines: [
                    "It takes about 8 minutes. Nothing is erased unless the dock holds exactly the expected firmware.",
                    "The dock's power supply must stay connected until this says verified. If the dock loses power in between, it will not start again.",
                    "Leave the cable in. This app cannot be closed while it runs.",
                ])
            }
            ConfirmRow(word: "ERASE", action: "Apply the fix", typed: $typed, enabled: idle) {
                engine.send("patch \(typed.trimmingCharacters(in: .whitespaces))")
                typed = ""
            }
            NoteView(note: state.patched)
        }
    }

    @ViewBuilder private var restartStep: some View {
        Callout(tone: .success, title: "Written and verified") {
            Text("The dock uses the new setting once it has been restarted.")
        }
        VStack(alignment: .leading, spacing: 6) {
            Text("1.  Unplug the USB-C cable from this computer.\(state.unplugged ? "  Done." : "")")
            Text("2.  Unplug the dock's power supply and wait 15 seconds.")
            Text("3.  Plug the power supply back in, then the cable.")
        }
        Busy(text: state.unplugged ? "Waiting for the dock to come back..." : "Waiting for you to unplug the cable...")
        Button("I have restarted it") { engine.send("restarted") }
    }

    @ViewBuilder private var testStep: some View {
        Text("Unplug and replug the cable a few times. Every plug-in should bring the dock up within about 2 seconds.")
        Label {
            Text(state.present ? "Dock connected" : "Dock not connected").fontWeight(.semibold)
        } icon: {
            Image(systemName: state.present ? "checkmark.circle.fill" : "circle.dashed")
                .foregroundStyle(state.present ? AnyShapeStyle(doneGreen) : AnyShapeStyle(.secondary))
        }
        if state.reconnects >= 3 {
            Callout(tone: .success, title: "Connected \(state.reconnects) times since the restart") {
                Text("If every plug-in connected, the loop is gone. You can close this window.")
            }
        } else if state.reconnects > 0 {
            Text("Connected \(state.reconnects) time(s) since the restart. Try it three times.")
        }
    }

    // MARK: The log below the steps

    @ViewBuilder private var logSection: some View {
        if engine.log.isEmpty {
            Text("Nothing has been done yet.").foregroundStyle(.secondary)
        } else {
            // a box of its own: a new line scrolls in here instead of moving the steps above it
            ScrollView {
                Text(engine.log.joined(separator: "\n"))
                    .font(.system(.callout, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
            }
            .defaultScrollAnchor(.bottom)
            .scrollBounceBehavior(.basedOnSize)
            .frame(height: 240)
            .background(Color(nsColor: .textBackgroundColor), in: RoundedRectangle(cornerRadius: 6))
            .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(Color(nsColor: .separatorColor)))
        }
    }
}

// MARK: The windows the "Dock" menu opens

/// i-tec's hub firmware: optional, and nothing to do with the loop the flow fixes.
struct HubView: View {
    let engine: Engine

    @State private var typed = ""
    @State private var confirming = false

    private var state: DockState { engine.state }
    private var idle: Bool { state.running == nil }

    var body: some View {
        Panel {
            Text("i-tec's update for the dock's two USB hubs. It does not affect the loop on Macs.")
            if let missing = state.hubRomsError {
                Callout(tone: .warning, title: "The hub firmware is missing") { Text(missing) }
            }
            HStack {
                Button("Check the hubs") { engine.send("hub-check") }
                Button("Update the hubs...") { (confirming, typed) = (true, "") }
            }
            .disabled(!idle || state.hubRomsError != nil || confirming)
            if confirming {
                Callout(tone: .warning, title: "This rewrites the firmware of the dock's two USB hubs") {
                    Bullets(lines: [
                        "Keep the dock's power supply and the cable connected until the result appears.",
                        "Each hub restarts at the end, so everything plugged into the dock disconnects for a few seconds.",
                        "This tool's hub update has been done on very few docks.",
                    ])
                }
                ConfirmRow(word: "UPDATE", action: "Update the hubs", typed: $typed, enabled: idle,
                           go: { send("hub-update") }, cancel: { confirming = false })
            }
            if state.running == .hubCheck || state.running == .hubUpdate {
                ProgressLine(progress: engine.progress)
            }
            NoteView(note: state.hubNote)
        }
    }

    private func send(_ command: String) {
        engine.send("\(command) \(typed.trimmingCharacters(in: .whitespaces))")
        (confirming, typed) = (false, "")
    }
}

/// What is rarely needed: the way back to the original firmware, and the firmware image itself.
struct MoreView: View {
    let engine: Engine

    @State private var typed = ""
    @State private var confirming = false
    @State private var imagePath = ""

    private var state: DockState { engine.state }
    private var idle: Bool { state.running == nil }

    var body: some View {
        Panel {
            Text("Writes the dock's original firmware back, with Try.SRC on again.")
            Button("Restore the original firmware...") { (confirming, typed) = (true, "") }
                .disabled(!idle || state.imageError != nil || confirming)
            if confirming {
                Callout(tone: .warning, title: "This erases the dock's power controller and writes the original firmware") {
                    Bullets(lines: [
                        "The dock's power supply must stay connected until this says verified.",
                        "Restart the dock afterwards: cable out, power off for 15 seconds, power on, cable in.",
                    ])
                }
                ConfirmRow(word: "ERASE", action: "Restore the original", typed: $typed, enabled: idle,
                           go: { send("restore") }, cancel: { confirming = false })
            }
            if state.running == .restore {
                ProgressLine(progress: engine.progress)
            }
            NoteView(note: state.restoreNote)
            Toggle("An earlier write was not verified: skip the content check next time",
                   isOn: Binding(get: { state.afterFailedWrite }, set: { engine.send("skip-check \($0 ? "on" : "off")") }))
                .disabled(!idle)
            if let missing = state.imageError {
                Callout(tone: .warning, title: "The firmware image is missing") {
                    Text(missing)
                    Text("The tool takes it from Cable Matters' updater, DUD15X0D1_Update_E108.exe. Download it, do not run it, and give its path here.")
                }
                Link("Open Cable Matters' download page", destination: URL(string: "https://kb.cablematters.com/index.php?View=entry&EntryID=129")!)
                HStack {
                    TextField("Path of the file", text: $imagePath)
                        .textFieldStyle(.roundedBorder)
                    Button("Use this file") { engine.send("image \(imagePath.trimmingCharacters(in: .whitespaces))") }
                        .disabled(imagePath.trimmingCharacters(in: .whitespaces).isEmpty || !idle)
                }
            }
        }
    }

    private func send(_ command: String) {
        engine.send("\(command) \(typed.trimmingCharacters(in: .whitespaces))")
        (confirming, typed) = (false, "")
    }
}

/// The frame of a window opened from the "Dock" menu.
private struct Panel<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) { content }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(20)
        }
        .scrollBounceBehavior(.basedOnSize)
        .controlSize(.large)
        .frame(minWidth: 460, minHeight: 240)
    }
}

// MARK: Building blocks


private extension Step {
    var title: String {
        switch self {
        case .connect: "Connect the dock"
        case .identify: "Identify the dock"
        case .patch: "Apply the fix"
        case .restart: "Restart the dock"
        case .test: "Test it"
        }
    }
}

/// One step in the list down the left: badge, title, and once it is done what came of it.
private struct StepRow: View {
    let step: Step
    let current: Step
    let summary: String
    /// The last step has no step after it, so it says itself when it is done.
    var done = false

    private var isDone: Bool { current > step || done }

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            badge.frame(width: 28, height: 28)
            VStack(alignment: .leading, spacing: 2) {
                Text(step.title)
                    .font(.title3).fontWeight(current == step ? .semibold : .regular)
                    .foregroundStyle(current < step ? .secondary : .primary)
                    .frame(minHeight: 28)
                    .accessibilityLabel("Step \(step.number), \(step.title), \(isDone ? "done" : current == step ? "current step" : "not started")")
                if current > step, !summary.isEmpty {
                    Text(summary).font(.callout).foregroundStyle(.secondary)
                }
            }
        }
    }

    /// The step's number in a circle: a tick when done, filled when current.
    @ViewBuilder private var badge: some View {
        if isDone {
            Image(systemName: "checkmark").font(.system(size: 13, weight: .bold)).foregroundStyle(.white)
                .frame(maxWidth: .infinity, maxHeight: .infinity).background(doneGreen, in: Circle())
                .accessibilityHidden(true)
        } else if current == step {
            Text("\(step.number)").font(.system(size: 14, weight: .semibold)).foregroundStyle(.white)
                .frame(maxWidth: .infinity, maxHeight: .infinity).background(Color.accentColor, in: Circle())
                .accessibilityHidden(true)
        } else {
            Text("\(step.number)").font(.system(size: 14)).foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, maxHeight: .infinity).overlay(Circle().strokeBorder(.secondary, lineWidth: 1.5))
                .accessibilityHidden(true)
        }
    }
}

/// How a message is shown. Each kind has its own symbol as well as its own colour.
private enum Tone {
    case warning, danger, success

    var symbol: String {
        switch self {
        case .warning: "exclamationmark.triangle.fill"
        case .danger: "xmark.octagon.fill"
        case .success: "checkmark.circle.fill"
        }
    }

    var colour: Color {
        switch self {
        case .warning: .orange
        case .danger: .red
        case .success: doneGreen
        }
    }
}

/// A tinted box for a message that must not be missed.
private struct Callout<Content: View>: View {
    let tone: Tone
    let title: String
    @ViewBuilder var content: Content

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Image(systemName: tone.symbol).foregroundStyle(tone.colour).accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 6) {
                Text(title).fontWeight(.semibold)
                content
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(tone.colour.opacity(0.15), in: RoundedRectangle(cornerRadius: 8))
        .accessibilityElement(children: .combine)
    }
}

private struct Bullets: View {
    let lines: [String]

    var body: some View {
        ForEach(lines, id: \.self) { line in
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text("\u{2022}")
                Text(line)
            }
        }
    }
}

/// The outcome of an action, next to the action it belongs to.
private struct NoteView: View {
    let note: Note?

    var body: some View {
        if let note, !note.ok {
            Callout(tone: .danger, title: "That did not work") { Text(note.text).textSelection(.enabled) }
        } else if let note, !note.text.isEmpty {
            Callout(tone: .success, title: note.text) {}
        }
    }
}

private struct Busy: View {
    let text: String

    var body: some View {
        HStack(spacing: 8) {
            ProgressView().controlSize(.small)
            Text(text)
        }
    }
}

private struct ProgressLine: View {
    let progress: StepProgress

    var body: some View {
        if progress.label.isEmpty {
            Busy(text: "Working...")
        } else {
            ProgressView(value: progress.fraction) {
                Text("\(progress.label): \(Int((progress.fraction * 100).rounded()))%")
            }
        }
    }
}

/// The typed confirmation for a write: the button works only once the word has been typed.
private struct ConfirmRow: View {
    let word: String
    let action: String
    @Binding var typed: String
    let enabled: Bool
    let go: () -> Void
    var cancel: (() -> Void)?

    var body: some View {
        HStack {
            Text("Type \(word) to confirm:")
            TextField("", text: $typed)
                .textFieldStyle(.roundedBorder)
                .autocorrectionDisabled()
                .frame(width: 110)
                .accessibilityLabel("Type \(word) to confirm")
            Button(action, action: go)
                .buttonStyle(.borderedProminent)
                .disabled(!enabled || typed.trimmingCharacters(in: .whitespaces) != word)
            if let cancel {
                Button("Cancel", action: cancel)
            }
        }
    }
}

/// A section that starts closed. The whole title opens it, not only the small arrow.
private struct Collapsible<Content: View>: View {
    let title: String
    @Binding var isExpanded: Bool
    @ViewBuilder var content: Content

    var body: some View {
        DisclosureGroup(isExpanded: $isExpanded) {
            VStack(alignment: .leading, spacing: 10) { content }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.top, 8)
        } label: {
            Text(title).font(.headline)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
                .onTapGesture { withAnimation { isExpanded.toggle() } }
        }
    }
}
