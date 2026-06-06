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
                Picker("Bracket style", selection: Binding(
                    get: { model.defangStyle },
                    set: { model.setDefangStyle($0) }
                )) {
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

            Section {
                Text("Operations run top-to-bottom in the order shown in the menu. "
                    + "Reordering the pipeline is planned but not yet available here.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .frame(width: 440, height: 380)
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
            Toggle(kind.label, isOn: Binding(
                get: { model.paramEnabled(kind) },
                set: { model.setParam(kind, enabled: $0, value: model.paramValue(kind)) }
            ))
            Spacer(minLength: 8)
            TextField("value", text: Binding(
                get: { model.paramValue(kind) },
                set: { model.setParam(kind, enabled: model.paramEnabled(kind), value: $0) }
            ))
            .textFieldStyle(.roundedBorder)
            .frame(width: 150)
            .disabled(!model.paramEnabled(kind))
        }
    }
}
