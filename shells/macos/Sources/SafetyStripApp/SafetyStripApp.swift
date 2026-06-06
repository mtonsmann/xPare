import SwiftUI
import SafetyStripCore
import SafetyStripKit

/// The SafetyStrip menu-bar app.
///
/// This is a `MenuBarExtra` app with a `Settings` scene. To run it as a true
/// accessory (no Dock icon, no main window) it must be packaged as a bundle with
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

        // Route A (DESIGN.md D12): free-text-parameterized ops live in a conventional
        // Settings window, since a `.menu`-style MenuBarExtra cannot host text fields.
        Settings {
            SettingsView(model: model)
        }
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
    /// True while a strip runs long enough to be worth showing — drives the
    /// "Stripping…" indicator. Set from the controller's threshold-gated callback.
    @Published var isStripping: Bool = false

    private let controller: StripController

    init() {
        let controller = StripController()
        self.controller = controller
        self.settings = controller.settings
        controller.onStrippingChange = { [weak self] busy in self?.isStripping = busy }
        controller.activate()
    }

    /// Toggle continuous vs on-demand.
    func setMode(_ mode: StripMode) {
        settings.mode = mode
        controller.update(settings)
    }

    // MARK: - Persistent pipeline (Clean toggles)

    /// Enable/disable a **zero-parameter** baseline operation, preserving order.
    func setOperation(_ op: SafetyStripCore.Operation, enabled: Bool) {
        var ops = settings.operations
        if enabled {
            if !ops.contains(op) { ops.append(op) }
        } else {
            ops.removeAll { $0 == op }
        }
        commit(ops)
    }

    func isEnabled(_ op: SafetyStripCore.Operation) -> Bool {
        settings.operations.contains(op)
    }

    /// Sort modes — off plus the four flag combinations — surfaced as a single inline
    /// `Picker` in the menu so the active mode gets the system ✓ (the native Finder
    /// "Sort By" idiom). Mutually exclusive; sort is on iff a non-`off` mode is picked.
    enum SortMode: Hashable, CaseIterable, Identifiable {
        case off, ascending, descending, ascendingCI, descendingCI
        var id: Self { self }

        /// Full label for the Picker rows.
        var label: String {
            switch self {
            case .off: return "Off"
            case .ascending: return "A → Z"
            case .descending: return "Z → A"
            case .ascendingCI: return "A → Z (ignore case)"
            case .descendingCI: return "Z → A (ignore case)"
            }
        }

        /// Compact label for the collapsed submenu title.
        var shortLabel: String {
            switch self {
            case .off: return "Off"
            case .ascending: return "A → Z"
            case .descending: return "Z → A"
            case .ascendingCI: return "A → Z, aA"
            case .descendingCI: return "Z → A, aA"
            }
        }

        /// The `sort_lines` flags for this mode, or `nil` when sort is off.
        var flags: (descending: Bool, caseInsensitive: Bool)? {
            switch self {
            case .off: return nil
            case .ascending: return (false, false)
            case .descending: return (true, false)
            case .ascendingCI: return (false, true)
            case .descendingCI: return (true, true)
            }
        }
    }

    /// The selected sort mode, derived from the pipeline.
    var sortMode: SortMode {
        for op in settings.operations {
            if case let .sortLines(descending, caseInsensitive) = op {
                switch (descending, caseInsensitive) {
                case (false, false): return .ascending
                case (true, false): return .descending
                case (false, true): return .ascendingCI
                case (true, true): return .descendingCI
                }
            }
        }
        return .off
    }

    func setSortMode(_ mode: SortMode) {
        var ops = settings.operations
        guard let flags = mode.flags else {
            ops.removeAll(where: isSort) // .off → drop the sort op
            commit(ops)
            return
        }
        let newOp = SafetyStripCore.Operation.sortLines(descending: flags.descending,
                                                        caseInsensitive: flags.caseInsensitive)
        // Update in place when present (preserves pipeline position in manual order);
        // otherwise append.
        if let idx = ops.firstIndex(where: isSort) {
            ops[idx] = newOp
        } else {
            ops.append(newOp)
        }
        commit(ops)
    }

    /// Defang toggle + bracket style. Defang carries a `style`, so it needs its own
    /// presence/style accessors rather than the exact-equality `setOperation`.
    var isDefangEnabled: Bool { settings.operations.contains(where: isDefang) }

    var defangStyle: BracketStyle {
        for op in settings.operations {
            if case let .defang(style) = op { return style }
        }
        return .square
    }

    func setDefang(enabled: Bool) {
        let style = defangStyle
        var ops = settings.operations
        ops.removeAll(where: isDefang)
        if enabled { ops.append(.defang(style: style)) }
        commit(ops)
    }

    func setDefangStyle(_ style: BracketStyle) {
        var ops = settings.operations
        var found = false
        ops = ops.map { op in
            if case .defang = op {
                found = true
                return .defang(style: style)
            }
            return op
        }
        if !found { ops.append(.defang(style: style)) }
        commit(ops)
    }

    // MARK: - Free-text parameterized ops (Settings window)

    /// The four ops whose parameter is free text — they cannot live in a `.menu`
    /// MenuBarExtra, so they are configured in the Settings window.
    enum ParamOp: CaseIterable, Identifiable {
        case prefix, suffix, join, split
        var id: Self { self }
        var label: String {
            switch self {
            case .prefix: return "Prefix every line with"
            case .suffix: return "Suffix every line with"
            case .join: return "Join all lines with"
            case .split: return "Split on delimiter"
            }
        }
    }

    func paramEnabled(_ kind: ParamOp) -> Bool {
        settings.operations.contains { matches(kind, $0) }
    }

    func paramValue(_ kind: ParamOp) -> String {
        for op in settings.operations {
            switch (kind, op) {
            case let (.prefix, .prefixLines(p)): return p
            case let (.suffix, .suffixLines(s)): return s
            case let (.join, .joinWith(s)): return s
            case let (.split, .splitOn(d)): return d
            default: continue
            }
        }
        return ""
    }

    func setParam(_ kind: ParamOp, enabled: Bool, value: String) {
        var ops = settings.operations
        if enabled {
            // Update in place when present so editing the text doesn't shuffle the
            // op to the end of the pipeline on every keystroke.
            if let idx = ops.firstIndex(where: { matches(kind, $0) }) {
                ops[idx] = makeParamOp(kind, value)
            } else {
                ops.append(makeParamOp(kind, value))
            }
        } else {
            ops.removeAll { matches(kind, $0) }
        }
        commit(ops)
    }

    // MARK: - Pipeline ordering

    /// True when the user has opted into arranging the pipeline themselves
    /// (`as_given`); false means the core's canonical order is used.
    var isManualOrder: Bool { settings.ordering == .asGiven }

    func setManualOrder(_ manual: Bool) {
        settings.ordering = manual ? .asGiven : .canonical
        controller.update(settings)
    }

    /// Reorder the pipeline (only meaningful in manual order). Indices are into the
    /// current `operations` list, as the Settings reorder list presents them.
    func moveOperations(from source: IndexSet, to destination: Int) {
        var ops = settings.operations
        ops.move(fromOffsets: source, toOffset: destination)
        settings.operations = ops
        controller.update(settings)
    }

    // MARK: - One-shot commands (Extract / Refang)

    /// Run a transient single-op command against the clipboard (never persisted).
    /// Reductions/conversions and refang are surfaced this way per D12.
    func runCommand(_ op: SafetyStripCore.Operation,
                    label: String,
                    notApplicableStatus: String? = nil) {
        Task { @MainActor in
            switch await controller.runOnce(operations: [op]) {
            case .stripped(let changed):
                lastStatus = changed ? label : "\(label): no change"
            case .empty:
                lastStatus = "Clipboard empty"
            case .failed:
                lastStatus = "\(label) failed"
            case .notApplicable:
                lastStatus = notApplicableStatus ?? "\(label): not applicable"
            case .tooLarge(let bytes):
                lastStatus = "Clipboard too large (\(bytes / (1024 * 1024)) MB)"
            }
        }
    }

    /// Run a strip right now from the menu. The transform runs off the main thread;
    /// we await the outcome and update the (content-free) status on the main actor.
    func stripNow() {
        Task { @MainActor in
            switch await controller.stripNow(trigger: .manual) {
            case .stripped(let changed):
                lastStatus = changed ? "Stripped clipboard" : "Already plain"
            case .empty:
                lastStatus = "Clipboard empty"
            case .failed:
                lastStatus = "Could not strip"
            case .notApplicable:
                lastStatus = "Nothing to strip"
            case .tooLarge(let bytes):
                lastStatus = "Clipboard too large (\(bytes / (1024 * 1024)) MB)"
            }
        }
    }

    // MARK: - Private helpers

    private func commit(_ ops: [SafetyStripCore.Operation]) {
        settings.operations = ops
        controller.update(settings)
    }

    private func isSort(_ op: SafetyStripCore.Operation) -> Bool {
        if case .sortLines = op { return true }
        return false
    }

    private func isDefang(_ op: SafetyStripCore.Operation) -> Bool {
        if case .defang = op { return true }
        return false
    }

    private func matches(_ kind: ParamOp, _ op: SafetyStripCore.Operation) -> Bool {
        switch (kind, op) {
        case (.prefix, .prefixLines), (.suffix, .suffixLines),
             (.join, .joinWith), (.split, .splitOn):
            return true
        default:
            return false
        }
    }

    private func makeParamOp(_ kind: ParamOp, _ value: String) -> SafetyStripCore.Operation {
        switch kind {
        case .prefix: return .prefixLines(prefix: value)
        case .suffix: return .suffixLines(suffix: value)
        case .join: return .joinWith(separator: value)
        case .split: return .splitOn(delimiter: value)
        }
    }
}

/// The contents of the menu-bar dropdown.
private struct MenuContent: View {
    @ObservedObject var model: AppModel
    // Programmatic Settings opener — `SettingsLink` does not reliably surface the
    // window for an accessory (`LSUIElement`) menu-bar app; see the Settings button.
    @Environment(\.openSettings) private var openSettings

    /// Zero-parameter rewrite ops exposed as simple on/off toggles in the *Clean*
    /// section. Parameterized rewrites (`sort`, `defang`) and the free-text ops are
    /// handled separately (a sort/defang toggle here, the rest in Settings).
    private static let cleanToggles: [(SafetyStripCore.Operation, String)] = [
        (.stripHtml, "Strip HTML"),
        (.stripMarkdown, "Strip Markdown"),
        (.collapseWhitespace, "Collapse whitespace"),
        (.trimTrailingWhitespace, "Trim trailing whitespace"),
        (.removeBlankLines, "Remove blank lines"),
        (.unwrapLines, "Unwrap lines"),
        (.dedupeLines, "Dedupe lines"),
        (.cleanUrls, "Clean URL trackers"),
    ]

    var body: some View {
        Button("Strip clipboard now") {
            model.stripNow()
        }
        .keyboardShortcut("v", modifiers: [.option, .command])
        .disabled(model.isStripping)

        Divider()

        Text(model.isStripping ? "Stripping…" : model.lastStatus)

        Divider()

        // Mode toggle: continuous monitoring is opt-in.
        Toggle("Continuous monitoring", isOn: Binding(
            get: { model.settings.mode == .continuous },
            set: { model.setMode($0 ? .continuous : .onDemand) }
        ))

        Divider()

        // --- Clean: persistent rewrite toggles (run in order, every strip) ---
        Text("Clean")
        ForEach(Array(Self.cleanToggles.enumerated()), id: \.offset) { _, entry in
            let (op, label) = entry
            Toggle(label, isOn: Binding(
                get: { model.isEnabled(op) },
                set: { model.setOperation(op, enabled: $0) }
            ))
        }
        // Sort is a SINGLE entry: a submenu whose title shows the active mode, with the
        // modes as an inline Picker so the active one gets the system ✓ — the same
        // native checkmark the sibling toggles use, just on the child (Finder "Sort By"
        // idiom). One menu line; state visible in the title; no glyph-alignment hacks.
        Menu("Sort lines: \(model.sortMode.shortLabel)") {
            Picker("Sort lines", selection: Binding(
                get: { model.sortMode },
                set: { model.setSortMode($0) }
            )) {
                ForEach(AppModel.SortMode.allCases) { mode in
                    Text(mode.label).tag(mode)
                }
            }
            .pickerStyle(.inline)
        }
        Toggle("Defang IOCs", isOn: Binding(
            get: { model.isDefangEnabled },
            set: { model.setDefang(enabled: $0) }
        ))
        // (Defang's bracket style is a parameter, so it lives in the Settings window.)

        Divider()

        // --- Extract: one-shot commands (replace the clipboard; never persisted) ---
        Text("Extract / convert (replaces clipboard)")
        Button("Extract emails") {
            model.runCommand(.extractEmails, label: "Extracted emails")
        }
        .disabled(model.isStripping)
        Button("Extract URLs") {
            model.runCommand(.extractUrls, label: "Extracted URLs")
        }
        .disabled(model.isStripping)
        Button("Convert HTML to Markdown") {
            model.runCommand(.htmlToMarkdown,
                             label: "Converted to Markdown",
                             notApplicableStatus: "No HTML content")
        }
        .disabled(model.isStripping)
        Button("Refang clipboard") {
            model.runCommand(.refang, label: "Refanged")
        }
        .disabled(model.isStripping)

        Divider()

        // Activate first: an accessory (LSUIElement) app must become active or the
        // Settings window opens behind everything / not at all. `openSettings` is more
        // reliable than `SettingsLink` for a programmatic open from a menu-bar app.
        Button("Settings…") {
            NSApp.activate(ignoringOtherApps: true)
            openSettings()
        }
        .keyboardShortcut(",", modifiers: [.command])

        Button("Quit SafetyStrip") {
            NSApplication.shared.terminate(nil)
        }
        .keyboardShortcut("q")
    }
}
