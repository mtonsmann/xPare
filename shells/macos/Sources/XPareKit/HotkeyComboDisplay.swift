import AppKit
import Foundation

/// Presentation and recording helpers for ``HotkeyCombo``.
///
/// The combo itself stores raw Carbon values so the settings model stays
/// Carbon-free (see ``HotkeyCombo``); this file mirrors the same masks for
/// display and maps AppKit's `NSEvent.ModifierFlags` onto them for the
/// Settings-window shortcut recorder. `HotkeyManager` re-derives the masks
/// from the real Carbon headers, and a test pins the two in sync.
extension HotkeyCombo {
    /// Carbon `cmdKey` mirrored as a raw value.
    public static let commandMask: UInt32 = 0x0100
    /// Carbon `shiftKey` mirrored as a raw value.
    public static let shiftMask: UInt32 = 0x0200
    /// Carbon `optionKey` mirrored as a raw value.
    public static let optionMask: UInt32 = 0x0800
    /// Carbon `controlKey` mirrored as a raw value.
    public static let controlMask: UInt32 = 0x1000

    /// True when the combo holds at least one of ⌘/⌃/⌥ — the floor for a
    /// global hotkey. A bare key or a shift-only chord would shadow normal
    /// typing system-wide, so the recorder refuses to accept one.
    public var hasCommandLikeModifier: Bool {
        modifiers & (Self.commandMask | Self.controlMask | Self.optionMask) != 0
    }

    /// Human-readable chord (e.g. "⌃⌥⌘V"), modifiers in the standard macOS
    /// display order ⌃⌥⇧⌘.
    public var displayString: String {
        var out = ""
        if modifiers & Self.controlMask != 0 { out += "⌃" }
        if modifiers & Self.optionMask != 0 { out += "⌥" }
        if modifiers & Self.shiftMask != 0 { out += "⇧" }
        if modifiers & Self.commandMask != 0 { out += "⌘" }
        return out + Self.keyName(forKeyCode: keyCode)
    }

    /// Build a combo from a recorded `keyDown` event's fields, mapping AppKit's
    /// `NSEvent.ModifierFlags` onto the Carbon masks `RegisterEventHotKey`
    /// expects. Returns `nil` unless at least one of ⌘/⌃/⌥ is held (see
    /// ``hasCommandLikeModifier``), so the recorder cannot produce a chord
    /// that shadows plain typing.
    public init?(keyCode: UInt16, modifierFlags: NSEvent.ModifierFlags) {
        var mask: UInt32 = 0
        if modifierFlags.contains(.command) { mask |= Self.commandMask }
        if modifierFlags.contains(.shift) { mask |= Self.shiftMask }
        if modifierFlags.contains(.option) { mask |= Self.optionMask }
        if modifierFlags.contains(.control) { mask |= Self.controlMask }
        let combo = HotkeyCombo(keyCode: UInt32(keyCode), modifiers: mask)
        guard combo.hasCommandLikeModifier else { return nil }
        self = combo
    }

    /// Display name for a Carbon virtual key code. Virtual key codes are
    /// *positions*, not characters; the names below follow the US/ANSI layout
    /// (the conventional approximation shortcut recorders use). Unknown codes
    /// fall back to a numeric form rather than failing.
    public static func keyName(forKeyCode code: UInt32) -> String {
        keyNames[code] ?? "Key \(code)"
    }

    /// US/ANSI names for the Carbon virtual key codes a hotkey can plausibly
    /// use. A dictionary (not a switch) keeps lookup complexity flat.
    private static let keyNames: [UInt32: String] = [
        0: "A", 1: "S", 2: "D", 3: "F", 4: "H", 5: "G", 6: "Z", 7: "X",
        8: "C", 9: "V", 11: "B", 12: "Q", 13: "W", 14: "E", 15: "R",
        16: "Y", 17: "T", 18: "1", 19: "2", 20: "3", 21: "4", 22: "6",
        23: "5", 24: "=", 25: "9", 26: "7", 27: "-", 28: "8", 29: "0",
        30: "]", 31: "O", 32: "U", 33: "[", 34: "I", 35: "P", 36: "↩",
        37: "L", 38: "J", 39: "'", 40: "K", 41: ";", 42: "\\", 43: ",",
        44: "/", 45: "N", 46: "M", 47: ".", 48: "⇥", 49: "Space",
        50: "`", 51: "⌫", 53: "⎋", 76: "⌅", 96: "F5", 97: "F6",
        98: "F7", 99: "F3", 100: "F8", 101: "F9", 103: "F11", 105: "F13",
        107: "F14", 109: "F10", 111: "F12", 113: "F15", 114: "Help",
        115: "↖", 116: "⇞", 117: "⌦", 118: "F4", 119: "↘", 120: "F2",
        121: "⇟", 122: "F1", 123: "←", 124: "→", 125: "↓", 126: "↑",
    ]
}
