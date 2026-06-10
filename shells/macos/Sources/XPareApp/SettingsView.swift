import AppKit
import SwiftUI
import XPareCore
import XPareKit

/// The Settings window (DESIGN.md D12, "Route A").
///
/// Home for the **free-text-parameterized** operations — `prefix`/`suffix`/`join`/
/// `split` — which a `.menu`-style `MenuBarExtra` cannot host (it has no room for a
/// text field), plus the two `sort` flags. The everyday on/off toggles and the
/// one-shot Extract commands stay in the menu; this window is the typed-input home
/// macOS users expect.
struct SettingsView: View {
    @ObservedObject var model: AppModel

    var body: some View {
        Form {
            Section("General") {
                // Launch at login via SMAppService.mainApp — see
                // `AppModel.setLaunchAtLogin` for the stable-location caveat.
                Toggle(
                    "Launch at login",
                    isOn: Binding(
                        get: { model.launchAtLogin },
                        set: { model.setLaunchAtLogin($0) }
                    ))
                if let error = model.launchAtLoginError {
                    Text(error)
                        .font(.caption)
                        .foregroundStyle(.red)
                }
            }

            Section("Global hotkey") {
                HotkeyRecorderRow(model: model)
            }

            Section("Line operations with text") {
                ForEach(AppModel.ParamOp.allCases) { kind in
                    ParamRow(model: model, kind: kind)
                }
            }

            Section("Defang") {
                Picker(
                    "Bracket style",
                    selection: Binding(
                        get: { model.defangStyle },
                        set: { model.setDefangStyle($0) }
                    )
                ) {
                    Text("Square  [.]").tag(BracketStyle.square)
                    Text("Round  (.)").tag(BracketStyle.round)
                }
                .disabled(!model.isDefangEnabled)
                if !model.isDefangEnabled {
                    Text("Turn on “Defang IOCs” in the menu to choose a style.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            // Sort's flags now live in the menu's "Sort options" submenu (one home per
            // control), so they're intentionally not duplicated here.

            // Paste-as-file's on/off and preset thresholds live in the menu (one home
            // per control); this section is only the typed *custom* threshold the
            // menu's "Custom…" item routes to.
            Section("Paste as file") {
                HStack {
                    Text("Custom threshold")
                    Spacer(minLength: 8)
                    TextField(
                        "KB",
                        value: Binding(
                            get: { model.settings.pasteAsFileThresholdKB },
                            set: { model.setPasteAsFileThresholdKB($0) }
                        ),
                        format: .number
                    )
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 80)
                    Text("KB")
                }
                .disabled(!model.settings.pasteLargeAsFile)
                if !model.settings.pasteLargeAsFile {
                    Text("Turn on “Paste as file” in the menu to set a custom threshold.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Text(
                    "Replaces the clipboard with a temporary text file when the "
                        + "stripped result exceeds the threshold, so pasting attaches a file. "
                        + "The file is the one exception to “content is never persisted”: "
                        + "it is owner-only, kept out of Spotlight and backups, and deleted "
                        + "on the next strip once the clipboard moves on, at every launch, "
                        + "and on quit from the menu."
                )
                .font(.caption)
                .foregroundStyle(.secondary)
            }

            Section("Pipeline order") {
                Toggle(
                    "Manual order (drag to arrange)",
                    isOn: Binding(
                        get: { model.isManualOrder },
                        set: { model.setManualOrder($0) }
                    ))
                if model.isManualOrder {
                    if model.settings.operations.isEmpty {
                        Text("No operations enabled yet — turn some on in the menu.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    } else {
                        List {
                            ForEach(model.settings.operations.indices, id: \.self) { i in
                                Text(opLabel(model.settings.operations[i]))
                            }
                            .onMove { from, to in model.moveOperations(from: from, to: to) }
                        }
                        .frame(minHeight: 140)
                    }
                } else {
                    Text(
                        "Operations run in the recommended canonical order (correct and "
                            + "efficient). Turn on manual order to arrange them yourself."
                    )
                    .font(.caption)
                    .foregroundStyle(.secondary)
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: 440, height: 560)
        .navigationTitle("xPare Settings")
    }
}

/// Records a replacement global hotkey. While recording, a LOCAL key-event
/// monitor (scoped to this app's own windows, so it needs no Accessibility or
/// Input Monitoring grant — the global monitors the posture forbids) captures
/// the next keyDown. Escape cancels; chords without at least one of ⌘/⌃/⌥ are
/// refused (`HotkeyCombo.init(keyCode:modifierFlags:)`), since they would
/// shadow normal typing. The monitor is removed the moment recording ends and
/// when the view disappears, so no monitor outlives the interaction.
private struct HotkeyRecorderRow: View {
    @ObservedObject var model: AppModel
    @State private var isRecording = false
    @State private var keyMonitor: Any?

    var body: some View {
        HStack {
            Text("Strip clipboard now")
            Spacer(minLength: 8)
            Button(isRecording ? "Type shortcut…" : model.settings.hotkey.displayString) {
                isRecording ? stopRecording() : startRecording()
            }
        }
        .onDisappear { stopRecording() }
        if isRecording {
            Text("Press a key with ⌘, ⌃, or ⌥. Esc cancels.")
                .font(.caption)
                .foregroundStyle(.secondary)
        } else if !model.hotkeyActive {
            Text("This shortcut could not be registered — it may be in use by another app.")
                .font(.caption)
                .foregroundStyle(.red)
        }
    }

    private func startRecording() {
        isRecording = true
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            handleKeyDown(event)
        }
    }

    /// Removing the monitor is unconditional teardown: safe to call twice.
    private func stopRecording() {
        if let keyMonitor { NSEvent.removeMonitor(keyMonitor) }
        keyMonitor = nil
        isRecording = false
    }

    /// Always returns `nil` (the event is swallowed) so keystrokes made while
    /// recording never reach the form underneath.
    private func handleKeyDown(_ event: NSEvent) -> NSEvent? {
        if event.keyCode == 53 {  // kVK_Escape — cancel, keep the current hotkey
            stopRecording()
            return nil
        }
        guard
            let combo = HotkeyCombo(keyCode: event.keyCode, modifierFlags: event.modifierFlags)
        else {
            return nil  // no ⌘/⌃/⌥ held — keep recording until a valid chord or Esc
        }
        model.setHotkey(combo)
        stopRecording()
        return nil
    }
}

/// One enable-toggle + text-field row for a free-text parameterized op.
///
/// The field is always editable so a value can be typed *before* enabling; the
/// typed draft lives here and is committed to settings only while the op is on.
/// Inline validation closes the empty-parameter gap: the toggle stays disabled
/// while the value is empty (enabling, say, split-on-delimiter with no
/// delimiter could only fail or do nothing at strip time), and clearing the
/// field while enabled shows a warning instead of failing later.
private struct ParamRow: View {
    @ObservedObject var model: AppModel
    let kind: AppModel.ParamOp
    @State private var draft: String = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Toggle(
                    kind.label,
                    isOn: Binding(
                        get: { model.paramEnabled(kind) },
                        set: { model.setParam(kind, enabled: $0, value: draft) }
                    )
                )
                .disabled(!model.paramEnabled(kind) && draft.isEmpty)
                Spacer(minLength: 8)
                TextField("value", text: $draft)
                    .textFieldStyle(.roundedBorder)
                    .frame(width: 150)
                    .onChange(of: draft) {
                        if model.paramEnabled(kind) {
                            model.setParam(kind, enabled: true, value: draft)
                        }
                    }
            }
            if draft.isEmpty {
                Text(
                    model.paramEnabled(kind)
                        ? "This operation needs a value."
                        : "Enter a value to enable."
                )
                .font(.caption)
                .foregroundStyle(model.paramEnabled(kind) ? Color.orange : Color.secondary)
            }
        }
        .onAppear { draft = model.paramValue(kind) }
    }
}

/// A short human label for an operation, used in the manual-order reorder list.
private func opLabel(_ op: XPareCore.Operation) -> String {
    switch op {
    case .stripHtml: return "Strip HTML"
    case .stripMarkdown: return "Strip Markdown"
    case .htmlToMarkdown: return "Convert HTML to Markdown"
    case .collapseWhitespace: return "Collapse whitespace"
    case .trimTrailingWhitespace: return "Trim trailing whitespace"
    case .removeBlankLines: return "Remove blank lines"
    case .unwrapLines: return "Unwrap lines"
    case .changeCase(let c): return "Change case (\(c.rawValue))"
    case .sortLines: return "Sort lines"
    case .dedupeLines: return "Dedupe lines"
    case .prefixLines(let p): return "Prefix lines (\(p))"
    case .suffixLines(let s): return "Suffix lines (\(s))"
    case .joinWith(let s): return "Join with (\(s))"
    case .splitOn(let d): return "Split on (\(d))"
    case .extractEmails: return "Extract emails"
    case .extractUrls: return "Extract URLs"
    case .defang(let style): return "Defang IOCs (\(style.rawValue))"
    case .refang: return "Refang"
    case .cleanUrls: return "Clean URL trackers"
    case .maskIdentifiers(let emails, let ipv4, let ipv6):
        var targets: [String] = []
        if emails { targets.append("emails") }
        if ipv4 { targets.append("IPv4") }
        if ipv6 { targets.append("IPv6") }
        return targets.isEmpty
            ? "Mask identifiers"
            : "Mask identifiers (\(targets.joined(separator: ", ")))"
    }
}
