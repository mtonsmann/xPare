import SwiftUI
import SafetyStripCore
import SafetyStripKit

/// The SafetyStrip menu-bar app.
///
/// This is a `MenuBarExtra`-only app. To run it as a true accessory (no Dock
/// icon, no main window) it must be packaged as a bundle with
/// `LSUIElement = true` in Info.plist — see README.md. In this Command-Line-Tools
/// environment it compiles and links, which is what we verify; producing the
/// signed `.app` requires full Xcode.
@main
struct SafetyStripApp: App {
    @StateObject private var model = AppModel()

    var body: some Scene {
        MenuBarExtra("SafetyStrip", systemImage: "scissors") {
            MenuContent(model: model)
        }
        .menuBarExtraStyle(.menu)
    }
}

/// Observable wiring around a ``StripController``. Holds the UI-facing copy of
/// settings and forwards edits to the controller (which persists + re-applies
/// side effects).
@MainActor
final class AppModel: ObservableObject {
    // Qualified to disambiguate from SwiftUI's `Settings` scene type.
    @Published var settings: SafetyStripKit.Settings
    /// A short, content-free status line for the menu (never clipboard text).
    @Published var lastStatus: String = "Ready"

    private let controller: StripController

    init() {
        let controller = StripController()
        self.controller = controller
        self.settings = controller.settings
        controller.activate()
    }

    var transformer: Transformer { Transformer() }

    /// Toggle continuous vs on-demand.
    func setMode(_ mode: StripMode) {
        settings.mode = mode
        controller.update(settings)
    }

    /// Enable/disable a baseline operation, preserving order.
    func setOperation(_ op: SafetyStripCore.Operation, enabled: Bool) {
        var ops = settings.operations
        if enabled {
            if !ops.contains(op) { ops.append(op) }
        } else {
            ops.removeAll { $0 == op }
        }
        settings.operations = ops
        controller.update(settings)
    }

    func isEnabled(_ op: SafetyStripCore.Operation) -> Bool {
        settings.operations.contains(op)
    }

    /// Run a strip right now from the menu.
    func stripNow() {
        switch controller.stripNow(trigger: .manual) {
        case .stripped(let changed):
            lastStatus = changed ? "Stripped clipboard" : "Already plain"
        case .empty:
            lastStatus = "Clipboard empty"
        case .failed:
            lastStatus = "Could not strip"
        case .tooLarge(let bytes):
            lastStatus = "Clipboard too large (\(bytes / (1024 * 1024)) MB)"
        }
    }
}

/// The contents of the menu-bar dropdown.
private struct MenuContent: View {
    @ObservedObject var model: AppModel

    /// The baseline operations exposed as simple on/off toggles in the menu.
    /// (Parameterized ops like prefix/suffix/join live in a fuller settings
    /// window, out of scope for this headless cut.)
    private static let toggleableOps: [(SafetyStripCore.Operation, String)] = [
        (.stripHtml, "Strip HTML"),
        (.stripMarkdown, "Strip Markdown"),
        (.collapseWhitespace, "Collapse whitespace"),
        (.trimTrailingWhitespace, "Trim trailing whitespace"),
        (.removeBlankLines, "Remove blank lines"),
        (.unwrapLines, "Unwrap lines"),
        (.dedupeLines, "Dedupe lines"),
    ]

    var body: some View {
        Button("Strip clipboard now") {
            model.stripNow()
        }
        .keyboardShortcut("v", modifiers: [.option, .command])

        Divider()

        Text(model.lastStatus)

        Divider()

        // Mode toggle: continuous monitoring is opt-in.
        Toggle("Continuous monitoring", isOn: Binding(
            get: { model.settings.mode == .continuous },
            set: { model.setMode($0 ? .continuous : .onDemand) }
        ))

        Divider()

        // Operation toggles.
        ForEach(Array(Self.toggleableOps.enumerated()), id: \.offset) { _, entry in
            let (op, label) = entry
            Toggle(label, isOn: Binding(
                get: { model.isEnabled(op) },
                set: { model.setOperation(op, enabled: $0) }
            ))
        }

        Divider()

        Button("Quit SafetyStrip") {
            NSApplication.shared.terminate(nil)
        }
        .keyboardShortcut("q")
    }
}
