import ServiceManagement
import SwiftUI
import XPareCore
import XPareKit

/// The xPare menu-bar app.
///
/// This is a `MenuBarExtra` app with a `Settings` scene. To run it as a true
/// accessory (no Dock icon, no main window) it must be packaged as a bundle with
/// `LSUIElement = true` in Info.plist — see README.md. In this Command-Line-Tools
/// environment it compiles and links, which is what we verify; producing the
/// signed `.app` requires full Xcode.
@main
struct XPareApp: App {
    @StateObject private var model = AppModel()

    var body: some Scene {
        MenuBarExtra("xPare", systemImage: "scissors") {
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
    @Published var settings: XPareKit.Settings
    /// A short, content-free status line for the menu (never clipboard text).
    @Published var lastStatus: String = "Ready"
    /// True while a strip runs long enough to be worth showing — drives the
    /// "Stripping…" indicator. Set from the controller's threshold-gated callback.
    @Published var isStripping: Bool = false
    /// Whether the global hotkey is registered with the OS. `false` after a
    /// failed registration, so the menu and Settings can show the hotkey is
    /// inactive instead of leaving it silently dead.
    @Published var hotkeyActive: Bool = false
    /// Mirror of `SMAppService.mainApp` registration (launch at login).
    @Published var launchAtLogin: Bool = SMAppService.mainApp.status == .enabled
    /// Inline, content-free error from the last launch-at-login toggle, if any.
    @Published var launchAtLoginError: String?

    private let controller: StripController

    init() {
        let controller = StripController()
        self.controller = controller
        self.settings = controller.settings
        controller.onStrippingChange = { [weak self] busy in self?.isStripping = busy }
        controller.onHotkeyStateChange = { [weak self] active in self?.hotkeyActive = active }
        controller.activate()
    }

    /// Toggle continuous vs on-demand.
    func setMode(_ mode: StripMode) {
        settings.mode = mode
        controller.update(settings)
    }

    /// Persist a newly recorded global hotkey. The controller re-registers it
    /// immediately and reports the result through `hotkeyActive`.
    func setHotkey(_ combo: HotkeyCombo) {
        settings.hotkey = combo
        controller.update(settings)
    }

    /// SwiftUI mirror of the configured global hotkey for the "Strip clipboard
    /// now" menu row, so the displayed hint always matches the recorded chord.
    /// `nil` (no hint shown) when the key has no single-character equivalent.
    var stripMenuShortcut: KeyboardShortcut? {
        settings.hotkey.menuShortcut
    }

    /// Toggle launch-at-login via `SMAppService` (the modern sandbox-friendly
    /// API — no helper bundle, no extra entitlement). KNOWN LIMITATION: macOS
    /// resolves the registration against the app's current on-disk path, so it
    /// is reliable only when the app runs from a stable location (e.g.
    /// /Applications); a copy launched from a temporary or build directory may
    /// fail to register or register a path that later disappears. Errors are
    /// surfaced inline in Settings; they carry no clipboard content.
    func setLaunchAtLogin(_ enabled: Bool) {
        do {
            if enabled {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
            launchAtLoginError = nil
        } catch {
            launchAtLoginError = error.localizedDescription
        }
        launchAtLogin = SMAppService.mainApp.status == .enabled
    }

    // MARK: - Persistent pipeline (Clean toggles)

    /// Enable/disable a **zero-parameter** baseline operation, preserving order.
    func setOperation(_ op: XPareCore.Operation, enabled: Bool) {
        var ops = settings.operations
        if enabled {
            if !ops.contains(op) { ops.append(op) }
        } else {
            ops.removeAll { $0 == op }
        }
        commit(ops)
    }

    func isEnabled(_ op: XPareCore.Operation) -> Bool {
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
            ops.removeAll(where: isSort)  // .off → drop the sort op
            commit(ops)
            return
        }
        let newOp = XPareCore.Operation.sortLines(
            descending: flags.descending,
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

    /// Case modes — off plus the four `CaseKind`s — surfaced as a single inline
    /// Picker submenu (the Sort lines idiom: one menu line, active mode in the
    /// title, system ✓ on the child). Mutually exclusive; on iff a non-`off`
    /// mode is picked.
    enum CaseMode: Hashable, CaseIterable, Identifiable {
        case off, upper, lower, title, sentence
        var id: Self { self }

        var label: String {
            switch self {
            case .off: return "Off"
            case .upper: return "UPPERCASE"
            case .lower: return "lowercase"
            case .title: return "Title Case"
            case .sentence: return "Sentence case"
            }
        }

        /// The `changeCase` parameter for this mode, or `nil` when off.
        var kind: CaseKind? {
            switch self {
            case .off: return nil
            case .upper: return .upper
            case .lower: return .lower
            case .title: return .title
            case .sentence: return .sentence
            }
        }

        init(kind: CaseKind) {
            switch kind {
            case .upper: self = .upper
            case .lower: self = .lower
            case .title: self = .title
            case .sentence: self = .sentence
            }
        }
    }

    /// The selected case mode, derived from the pipeline.
    var caseMode: CaseMode {
        for op in settings.operations {
            if case let .changeCase(kind) = op { return CaseMode(kind: kind) }
        }
        return .off
    }

    func setCaseMode(_ mode: CaseMode) {
        var ops = settings.operations
        guard let kind = mode.kind else {
            ops.removeAll(where: isChangeCase)  // .off → drop the op
            commit(ops)
            return
        }
        // Update in place when present (preserves pipeline position in manual
        // order); otherwise append.
        let newOp = XPareCore.Operation.changeCase(case: kind)
        if let idx = ops.firstIndex(where: isChangeCase) {
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

    enum MaskTarget: CaseIterable, Identifiable {
        case emails, ipv4, ipv6
        var id: Self { self }
        var label: String {
            switch self {
            case .emails: return "Emails"
            case .ipv4: return "IPv4 addresses"
            case .ipv6: return "IPv6 addresses"
            }
        }
    }

    /// Compact label for the collapsed masking submenu title.
    var maskSummaryLabel: String {
        let flags = maskFlags
        switch (flags.emails, flags.ipv4, flags.ipv6) {
        case (false, false, false):
            return "Off"
        case (true, true, true):
            return "All"
        default:
            var targets: [String] = []
            if flags.emails { targets.append("Emails") }
            if flags.ipv4 { targets.append("IPv4") }
            if flags.ipv6 { targets.append("IPv6") }
            return targets.joined(separator: ", ")
        }
    }

    func maskEnabled(_ target: MaskTarget) -> Bool {
        let flags = maskFlags
        switch target {
        case .emails: return flags.emails
        case .ipv4: return flags.ipv4
        case .ipv6: return flags.ipv6
        }
    }

    func setMask(_ target: MaskTarget, enabled: Bool) {
        var flags = maskFlags
        switch target {
        case .emails: flags.emails = enabled
        case .ipv4: flags.ipv4 = enabled
        case .ipv6: flags.ipv6 = enabled
        }
        setMaskFlags(flags)
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

    // MARK: - Paste large clipboards as a file (opt-in posture exception)

    /// Menu modes for paste-as-file: off, or "over N KB". Presets cover the
    /// common cases; a threshold typed in Settings appears as an extra
    /// "(custom)" choice so the active mode always carries the system ✓.
    enum PasteAsFileMode: Hashable, Identifiable {
        case off
        case over(kb: Int)
        var id: Self { self }

        /// Full label for the Picker rows.
        var label: String {
            switch self {
            case .off: return "Off"
            case .over(let kb): return "Over \(Self.sizeLabel(kb))"
            }
        }

        /// Compact label for the collapsed submenu title.
        var shortLabel: String {
            switch self {
            case .off: return "Off"
            case .over(let kb): return "> \(Self.sizeLabel(kb))"
            }
        }

        static func sizeLabel(_ kb: Int) -> String {
            kb >= 1024 && kb % 1024 == 0 ? "\(kb / 1024) MB" : "\(kb) KB"
        }
    }

    /// The threshold presets offered in the menu, in KB.
    static let pasteAsFilePresetsKB = [64, 256, 512, 1024]

    /// The selected paste-as-file mode, derived from settings.
    var pasteAsFileMode: PasteAsFileMode {
        settings.pasteLargeAsFile ? .over(kb: settings.pasteAsFileThresholdKB) : .off
    }

    /// The menu's choices: Off, the presets, and — when the stored threshold is
    /// a non-preset value typed in Settings — that custom value, inserted in
    /// size order so the list stays sorted. Offered even while the feature is
    /// off, so toggling Off never strands a custom threshold (re-enabling it
    /// would otherwise require a preset, which overwrites the stored value).
    var pasteAsFileModes: [PasteAsFileMode] {
        var kbs = Self.pasteAsFilePresetsKB
        if !kbs.contains(settings.pasteAsFileThresholdKB) {
            kbs.append(settings.pasteAsFileThresholdKB)
            kbs.sort()
        }
        return [.off] + kbs.map { .over(kb: $0) }
    }

    func setPasteAsFileMode(_ mode: PasteAsFileMode) {
        switch mode {
        case .off:
            // Keep the threshold so re-enabling restores the previous choice.
            settings.pasteLargeAsFile = false
        case .over(let kb):
            settings.pasteLargeAsFile = true
            settings.pasteAsFileThresholdKB = kb
        }
        controller.update(settings)
    }

    /// The typed *custom* threshold (Settings; the menu's "Custom…" continuation).
    func setPasteAsFileThresholdKB(_ kb: Int) {
        // Floor at 1 KB; Settings clamps again at use, but don't persist nonsense.
        settings.pasteAsFileThresholdKB = max(1, kb)
        controller.update(settings)
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
    func runCommand(
        _ op: XPareCore.Operation,
        label: String,
        notApplicableStatus: String? = nil
    ) {
        Task { @MainActor in
            switch await controller.runOnce(operations: [op]) {
            case .stripped(let changed):
                lastStatus = changed ? label : "\(label): no change"
            case .strippedToFile:
                lastStatus = "\(label) — clipboard is now a file"
            case .empty:
                lastStatus = "Clipboard empty"
            case .failed:
                lastStatus = "\(label) failed"
            case .writeFailed:
                lastStatus = "Could not write to clipboard"
            case .skippedConcealed:
                lastStatus = "Skipped: marked confidential by its source"
            case .notApplicable:
                lastStatus = notApplicableStatus ?? "\(label): not applicable"
            case .tooLarge(let bytes, let rich):
                lastStatus = Self.tooLargeStatus(bytes: bytes, rich: rich)
            }
        }
    }

    /// Content-free "too large" status. Names the rich representation when that
    /// is what blew the ceiling, so the refusal is honest about why a seemingly
    /// small selection was refused (its HTML/RTF form can be far larger than
    /// the visible text). The clipboard is left unchanged either way.
    static func tooLargeStatus(bytes: Int, rich: Bool) -> String {
        let mb = bytes / (1024 * 1024)
        return rich
            ? "Rich clipboard content over the size limit (\(mb) MB) — left unchanged"
            : "Clipboard too large (\(mb) MB)"
    }

    /// Run a strip right now from the menu. The transform runs off the main thread;
    /// we await the outcome and update the (content-free) status on the main actor.
    func stripNow() {
        Task { @MainActor in
            switch await controller.stripNow(trigger: .manual) {
            case .stripped(let changed):
                lastStatus = changed ? "Stripped clipboard" : "Already plain"
            case .strippedToFile:
                lastStatus = "Stripped — clipboard is now a file"
            case .empty:
                lastStatus = "Clipboard empty"
            case .failed:
                lastStatus = "Could not strip"
            case .writeFailed:
                lastStatus = "Could not write to clipboard"
            case .skippedConcealed:
                lastStatus = "Skipped: marked confidential by its source"
            case .notApplicable:
                lastStatus = "Nothing to strip"
            case .tooLarge(let bytes, let rich):
                lastStatus = Self.tooLargeStatus(bytes: bytes, rich: rich)
            }
        }
    }

    /// Quit via the controller so its teardown runs first — in particular the
    /// paste-as-file store cleanup, so no paste file outlives the app. Launch-time
    /// cleanup in `activate()` is the backstop for terminations that skip this
    /// path (logout, force quit).
    func quit() {
        controller.deactivate()
        NSApplication.shared.terminate(nil)
    }

    // MARK: - Private helpers

    private func commit(_ ops: [XPareCore.Operation]) {
        settings.operations = ops
        controller.update(settings)
    }

    private func isSort(_ op: XPareCore.Operation) -> Bool {
        if case .sortLines = op { return true }
        return false
    }

    private func isDefang(_ op: XPareCore.Operation) -> Bool {
        if case .defang = op { return true }
        return false
    }

    private func isChangeCase(_ op: XPareCore.Operation) -> Bool {
        if case .changeCase = op { return true }
        return false
    }

    private var maskFlags: (emails: Bool, ipv4: Bool, ipv6: Bool) {
        for op in settings.operations {
            if case let .maskIdentifiers(emails, ipv4, ipv6) = op {
                return (emails, ipv4, ipv6)
            }
        }
        return (false, false, false)
    }

    private func setMaskFlags(_ flags: (emails: Bool, ipv4: Bool, ipv6: Bool)) {
        var ops = settings.operations
        if flags.emails || flags.ipv4 || flags.ipv6 {
            let newOp = XPareCore.Operation.maskIdentifiers(
                emails: flags.emails,
                ipv4: flags.ipv4,
                ipv6: flags.ipv6
            )
            if let idx = ops.firstIndex(where: isMask) {
                ops[idx] = newOp
            } else {
                ops.append(newOp)
            }
        } else {
            ops.removeAll(where: isMask)
        }
        commit(ops)
    }

    private func isMask(_ op: XPareCore.Operation) -> Bool {
        if case .maskIdentifiers = op { return true }
        return false
    }

    private func matches(_ kind: ParamOp, _ op: XPareCore.Operation) -> Bool {
        switch (kind, op) {
        case (.prefix, .prefixLines), (.suffix, .suffixLines),
            (.join, .joinWith), (.split, .splitOn):
            return true
        default:
            return false
        }
    }

    private func makeParamOp(_ kind: ParamOp, _ value: String) -> XPareCore.Operation {
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

    var body: some View {
        Button("Strip clipboard now") {
            model.stripNow()
        }
        // The hint mirrors the *recorded* global hotkey, so the row never
        // advertises a stale chord after the user changes it in Settings.
        .keyboardShortcut(model.stripMenuShortcut)
        .disabled(model.isStripping)

        Divider()

        Text(model.isStripping ? "Stripping…" : model.lastStatus)
        if !model.hotkeyActive {
            // Surfaced hotkey failure: registration was rejected (e.g. the
            // chord is taken by another app), so the shortcut will not fire.
            Text("Hotkey \(model.settings.hotkey.displayString) inactive")
        }

        Divider()

        // Mode toggle: continuous monitoring is opt-in.
        Toggle(
            "Continuous monitoring",
            isOn: Binding(
                get: { model.settings.mode == .continuous },
                set: { model.setMode($0 ? .continuous : .onDemand) }
            ))
        // Output mode: how the stripped result lands on the pasteboard. Bounded
        // options live here per D12 (status-bearing row + radio submenu); the
        // typed threshold is the free parameter, so "Custom…" routes to Settings.
        Menu("Paste as file: \(model.pasteAsFileMode.shortLabel)") {
            Picker(
                "Paste as file",
                selection: Binding(
                    get: { model.pasteAsFileMode },
                    set: { model.setPasteAsFileMode($0) }
                )
            ) {
                ForEach(model.pasteAsFileModes) { mode in
                    Text(mode.label).tag(mode)
                }
            }
            .pickerStyle(.inline)
            Divider()
            Button("Custom…") {
                NSApp.activate(ignoringOtherApps: true)
                openSettings()
            }
        }

        Divider()

        // --- Clean: persistent rewrite toggles (run on every strip) ---
        // Row order mirrors the core's canonical pipeline order (DESIGN.md D13,
        // `Operation::canonical_rank` in core/src/config.rs — ranks in comments),
        // so the menu reads top-to-bottom as the pipeline runs. Keep it in sync
        // when ranks change.
        Text("Clean")
        cleanToggle(.stripHtml, "Strip HTML")  // rank 1
        cleanToggle(.stripMarkdown, "Strip Markdown")  // rank 2
        cleanToggle(.unwrapLines, "Unwrap lines")  // rank 5
        cleanToggle(.collapseWhitespace, "Collapse whitespace")  // rank 6
        cleanToggle(.trimTrailingWhitespace, "Trim trailing whitespace")  // rank 7
        cleanToggle(.cleanUrls, "Clean URL trackers")  // rank 8
        Menu("Mask identifiers: \(model.maskSummaryLabel)") {  // rank 9
            ForEach(AppModel.MaskTarget.allCases) { target in
                Toggle(
                    target.label,
                    isOn: Binding(
                        get: { model.maskEnabled(target) },
                        set: { model.setMask(target, enabled: $0) }
                    ))
            }
        }
        // (Defang's bracket style is a parameter, so it lives in the Settings window.)
        Toggle(
            "Defang IOCs",
            isOn: Binding(  // rank 10
                get: { model.isDefangEnabled },
                set: { model.setDefang(enabled: $0) }
            ))
        cleanToggle(.removeBlankLines, "Remove blank lines")  // rank 14
        cleanToggle(.dedupeLines, "Dedupe lines")  // rank 15
        // Sort is a SINGLE entry: a submenu whose title shows the active mode, with the
        // modes as an inline Picker so the active one gets the system ✓ — the same
        // native checkmark the sibling toggles use, just on the child (Finder "Sort By"
        // idiom). One menu line; state visible in the title; no glyph-alignment hacks.
        Menu("Sort lines: \(model.sortMode.shortLabel)") {  // rank 16
            Picker(
                "Sort lines",
                selection: Binding(
                    get: { model.sortMode },
                    set: { model.setSortMode($0) }
                )
            ) {
                ForEach(AppModel.SortMode.allCases) { mode in
                    Text(mode.label).tag(mode)
                }
            }
            .pickerStyle(.inline)
        }
        Menu("Change case: \(model.caseMode.label)") {  // rank 17
            Picker(
                "Change case",
                selection: Binding(
                    get: { model.caseMode },
                    set: { model.setCaseMode($0) }
                )
            ) {
                ForEach(AppModel.CaseMode.allCases) { mode in
                    Text(mode.label).tag(mode)
                }
            }
            .pickerStyle(.inline)
        }

        Divider()

        // --- Extract: one-shot commands (replace the clipboard; never persisted) ---
        // Same rule: canonical-rank order.
        Text("Extract / convert (replaces clipboard)")
        Button("Convert HTML to Markdown") {  // rank 3
            model.runCommand(
                .htmlToMarkdown,
                label: "Converted to Markdown",
                notApplicableStatus: "No HTML content")
        }
        .disabled(model.isStripping)
        Button("Refang clipboard") {  // rank 11
            model.runCommand(.refang, label: "Refanged")
        }
        .disabled(model.isStripping)
        Button("Extract emails") {  // rank 12
            model.runCommand(.extractEmails, label: "Extracted emails")
        }
        .disabled(model.isStripping)
        Button("Extract URLs") {  // rank 13
            model.runCommand(.extractUrls, label: "Extracted URLs")
        }
        .disabled(model.isStripping)

        Divider()

        // Disabled informational row: the bundled version. "xPare dev" when run
        // unbundled (e.g. `swift run` from a checkout, where no Info.plist
        // carries CFBundleShortVersionString).
        Text(Self.versionLabel)

        // Activate first: an accessory (LSUIElement) app must become active or the
        // Settings window opens behind everything / not at all. `openSettings` is more
        // reliable than `SettingsLink` for a programmatic open from a menu-bar app.
        Button("Settings…") {
            NSApp.activate(ignoringOtherApps: true)
            openSettings()
        }
        .keyboardShortcut(",", modifiers: [.command])

        Button("Quit xPare") {
            model.quit()
        }
        .keyboardShortcut("q")
    }

    /// "xPare v1.2.3" from the bundle, or "xPare dev" outside a bundle.
    private static let versionLabel: String = {
        let version =
            Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
        return version.map { "xPare v\($0)" } ?? "xPare dev"
    }()

    /// One on/off row for a zero-parameter rewrite in the *Clean* section.
    private func cleanToggle(
        _ op: XPareCore.Operation,
        _ label: String
    ) -> some View {
        Toggle(
            label,
            isOn: Binding(
                get: { model.isEnabled(op) },
                set: { model.setOperation(op, enabled: $0) }
            ))
    }
}
