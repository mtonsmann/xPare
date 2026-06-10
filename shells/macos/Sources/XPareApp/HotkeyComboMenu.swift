import SwiftUI
import XPareKit

/// SwiftUI bridging for ``HotkeyCombo``: turns the persisted Carbon-style combo
/// into the `KeyboardShortcut` shown on the "Strip clipboard now" menu row, so
/// the menu hint always tracks the chord recorded in Settings.
extension HotkeyCombo {
    /// The combo as a SwiftUI menu shortcut, or `nil` when the key has no
    /// single-character equivalent (F-keys, arrows, …) — in that case the row
    /// simply shows no hint; the Carbon registration still fires regardless.
    var menuShortcut: KeyboardShortcut? {
        let name = Self.keyName(forKeyCode: keyCode)
        guard name.count == 1, let char = name.lowercased().first else { return nil }
        return KeyboardShortcut(KeyEquivalent(char), modifiers: eventModifiers)
    }

    /// The combo's Carbon modifier mask as SwiftUI `EventModifiers`.
    private var eventModifiers: EventModifiers {
        var mods: EventModifiers = []
        if modifiers & Self.controlMask != 0 { mods.insert(.control) }
        if modifiers & Self.optionMask != 0 { mods.insert(.option) }
        if modifiers & Self.shiftMask != 0 { mods.insert(.shift) }
        if modifiers & Self.commandMask != 0 { mods.insert(.command) }
        return mods
    }
}
