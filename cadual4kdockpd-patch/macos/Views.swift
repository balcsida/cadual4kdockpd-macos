// The window: a guided flow for the Mac loop fix, one step at a time, drawn from the engine's state.
// Optional and rarely needed actions sit in collapsed sections below the steps. Every write is
// confirmed here by typing a word; the engine checks the word again before it starts.

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
    /// A write from one of the sections, waiting for its typed word.
    @State private var pending: Job?
    @State private var pendingTyped = ""
    @State private var imagePath = ""
    @State private var showHubs = false
    @State private var showMore = false
    @State private var showLog = false

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

                StepRow(step: .connect, current: state.step, title: "Connect the dock", summary: "Dock connected.") { connectStep }
                StepRow(step: .identify, current: state.step, title: "Identify the dock", summary: state.identified?.text ?? "") { identifyStep }
                StepRow(step: .patch, current: state.step, title: "Apply the fix",
                        summary: state.alreadyPatched ? "Not needed: the fix is already in this dock." : "Written and verified.") { patchStep }
                StepRow(step: .restart, current: state.step, title: "Restart the dock", summary: state.alreadyPatched ? "Not needed." : "Restarted.") { restartStep }
                StepRow(step: .test, current: state.step, title: "Test it", summary: "") { testStep }

                Divider()
                Collapsible(title: "Hub firmware 93.92 (optional)", isExpanded: $showHubs) { hubSection }
                Collapsible(title: "More options", isExpanded: $showMore) { moreSection }
                Collapsible(title: "Details", isExpanded: $showLog) { logSection }
            }
            .frame(maxWidth: 700, alignment: .leading)
            .padding(24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .controlSize(.large)
        // closing the window would leave the user without the result of a write
        .onChange(of: engine.writing) { _, writing in
            for window in NSApp.windows { window.standardWindowButton(.closeButton)?.isEnabled = !writing }
        }
        // a build without the firmware inside: the image file is chosen under "More options"
        .onChange(of: state.imageError, initial: true) { _, missing in
            if missing != nil { showMore = true }
        }
        #if DEBUG
        .onAppear {
            if ProcessInfo.processInfo.environment["DOCK_UI_OPEN"] != nil {
                (showHubs, showMore, showLog, pending) = (true, true, true, .hubUpdate)
            }
        }
        #endif
    }

    // MARK: The five steps

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

    // MARK: The sections below the steps

    @ViewBuilder private var hubSection: some View {
        Text("i-tec's update for the dock's two USB hubs. It does not affect the loop on Macs.")
        if let missing = state.hubRomsError {
            Callout(tone: .warning, title: "The hub firmware is missing") { Text(missing) }
        }
        HStack {
            Button("Check the hubs") { engine.send("hub-check") }
            Button("Update the hubs...") { (pending, pendingTyped) = (.hubUpdate, "") }
        }
        .disabled(!idle || state.hubRomsError != nil || pending != nil)
        if pending == .hubUpdate {
            Callout(tone: .warning, title: "This rewrites the firmware of the dock's two USB hubs") {
                Bullets(lines: [
                    "Keep the dock's power supply and the cable connected until the result appears.",
                    "Each hub restarts at the end, so everything plugged into the dock disconnects for a few seconds.",
                    "This tool's hub update has been done on very few docks.",
                ])
            }
            ConfirmRow(word: "UPDATE", action: "Update the hubs", typed: $pendingTyped, enabled: idle, go: { start("hub-update") }, cancel: { pending = nil })
        }
        if state.running == .hubCheck || state.running == .hubUpdate {
            ProgressLine(progress: engine.progress)
        }
        NoteView(note: state.hubNote)
    }

    @ViewBuilder private var moreSection: some View {
        Text("Writes the dock's original firmware back, with Try.SRC on again.")
        Button("Restore the original firmware...") { (pending, pendingTyped) = (.restore, "") }
            .disabled(!idle || state.imageError != nil || pending != nil)
        if pending == .restore {
            Callout(tone: .warning, title: "This erases the dock's power controller and writes the original firmware") {
                Bullets(lines: [
                    "The dock's power supply must stay connected until this says verified.",
                    "Restart the dock afterwards: cable out, power off for 15 seconds, power on, cable in.",
                ])
            }
            ConfirmRow(word: "ERASE", action: "Restore the original", typed: $pendingTyped, enabled: idle, go: { start("restore") }, cancel: { pending = nil })
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
                Text("The tool takes it from Cable Matters' updater, DUD15X0D1_Update_E108.exe. Give that file's path:")
            }
            HStack {
                TextField("Path of the file", text: $imagePath)
                    .textFieldStyle(.roundedBorder)
                Button("Use this file") { engine.send("image \(imagePath.trimmingCharacters(in: .whitespaces))") }
                    .disabled(imagePath.trimmingCharacters(in: .whitespaces).isEmpty || !idle)
            }
        }
    }

    @ViewBuilder private var logSection: some View {
        if engine.log.isEmpty {
            Text("Nothing has been done yet.").foregroundStyle(.secondary)
        } else {
            Text(engine.log.joined(separator: "\n"))
                .font(.system(.callout, design: .monospaced))
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
        }
    }

    /// Starts the write that was waiting for its typed word.
    private func start(_ command: String) {
        engine.send("\(command) \(pendingTyped.trimmingCharacters(in: .whitespaces))")
        (pending, pendingTyped) = (nil, "")
    }
}

// MARK: Building blocks

/// One row of the flow: badge, title, and the step's contents while it is the current one.
private struct StepRow<Content: View>: View {
    let step: Step
    let current: Step
    let title: String
    let summary: String
    @ViewBuilder var content: Content

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            badge.frame(width: 28, height: 28)
            VStack(alignment: .leading, spacing: 10) {
                Text(title)
                    .font(.title3).fontWeight(current == step ? .semibold : .regular)
                    .foregroundStyle(current < step ? .secondary : .primary)
                    .frame(minHeight: 28)
                    .accessibilityAddTraits(.isHeader)
                    .accessibilityLabel("Step \(step.number), \(title), \(current > step ? "done" : current == step ? "current step" : "not started")")
                if current == step {
                    content
                } else if current > step, !summary.isEmpty {
                    Text(summary).foregroundStyle(.secondary)
                }
            }
        }
    }

    /// The step's number in a circle: a tick when done, filled when current.
    @ViewBuilder private var badge: some View {
        if current > step {
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
