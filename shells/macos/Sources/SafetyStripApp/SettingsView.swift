import SwiftUI
import SafetyStripCore

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
                        + "as soon as the clipboard moves on or SafetyStrip quits."
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
        .frame(width: 440, height: 520)
        .navigationTitle("SafetyStrip Settings")
    }
}

/// One enable-toggle + text-field row for a free-text parameterized op. The field is
/// disabled until the op is enabled, so it's clear the value only applies when on.
private struct ParamRow: View {
    @ObservedObject var model: AppModel
    let kind: AppModel.ParamOp

    var body: some View {
        HStack {
            Toggle(
                kind.label,
                isOn: Binding(
                    get: { model.paramEnabled(kind) },
                    set: { model.setParam(kind, enabled: $0, value: model.paramValue(kind)) }
                ))
            Spacer(minLength: 8)
            TextField(
                "value",
                text: Binding(
                    get: { model.paramValue(kind) },
                    set: { model.setParam(kind, enabled: model.paramEnabled(kind), value: $0) }
                )
            )
            .textFieldStyle(.roundedBorder)
            .frame(width: 150)
            .disabled(!model.paramEnabled(kind))
        }
    }
}

/// A short human label for an operation, used in the manual-order reorder list.
private func opLabel(_ op: SafetyStripCore.Operation) -> String {
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
