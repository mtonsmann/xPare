import AppKit
import Foundation
import Testing

@testable import XPareKit

/// The presentation/recording half of `HotkeyCombo`: display formatting and the
/// NSEvent-flags → Carbon-mask conversion behind the Settings shortcut recorder.
@Suite struct HotkeyComboDisplayTests {

    @Test func defaultComboDisplaysAsControlOptionCommandV() {
        #expect(HotkeyCombo.defaultCombo.displayString == "⌃⌥⌘V")
    }

    @Test func modifierSymbolsFollowTheStandardMacOSOrder() {
        // All four held: display order is ⌃⌥⇧⌘ regardless of mask bit order.
        let combo = HotkeyCombo(
            keyCode: 11,  // kVK_ANSI_B
            modifiers: HotkeyCombo.commandMask | HotkeyCombo.shiftMask
                | HotkeyCombo.optionMask | HotkeyCombo.controlMask
        )
        #expect(combo.displayString == "⌃⌥⇧⌘B")
    }

    @Test func unknownKeyCodeFallsBackToANumericName() {
        #expect(HotkeyCombo.keyName(forKeyCode: 200) == "Key 200")
    }

    @Test func namedSpecialKeysResolve() {
        #expect(HotkeyCombo.keyName(forKeyCode: 49) == "Space")
        #expect(HotkeyCombo.keyName(forKeyCode: 122) == "F1")
        #expect(HotkeyCombo.keyName(forKeyCode: 126) == "↑")
    }

    @Test func hasCommandLikeModifierRequiresCommandControlOrOption() {
        #expect(HotkeyCombo(keyCode: 9, modifiers: HotkeyCombo.commandMask).hasCommandLikeModifier)
        #expect(HotkeyCombo(keyCode: 9, modifiers: HotkeyCombo.controlMask).hasCommandLikeModifier)
        #expect(HotkeyCombo(keyCode: 9, modifiers: HotkeyCombo.optionMask).hasCommandLikeModifier)
        #expect(!HotkeyCombo(keyCode: 9, modifiers: HotkeyCombo.shiftMask).hasCommandLikeModifier)
        #expect(!HotkeyCombo(keyCode: 9, modifiers: 0).hasCommandLikeModifier)
    }

    // MARK: - Recorder conversion (NSEvent.ModifierFlags → Carbon masks)

    @Test func recorderConversionMapsEachFlagToItsCarbonMask() throws {
        let combo = try #require(
            HotkeyCombo(
                keyCode: 9,
                modifierFlags: [.command, .option, .control, .shift]))
        #expect(combo.keyCode == 9)
        #expect(
            combo.modifiers
                == HotkeyCombo.commandMask | HotkeyCombo.optionMask
                | HotkeyCombo.controlMask | HotkeyCombo.shiftMask)
    }

    @Test func recorderRefusesChordsWithoutACommandLikeModifier() {
        // Bare keys and shift-only chords would shadow normal typing
        // system-wide, so the recorder must refuse them.
        #expect(HotkeyCombo(keyCode: 9, modifierFlags: []) == nil)
        #expect(HotkeyCombo(keyCode: 9, modifierFlags: [.shift]) == nil)
    }

    @Test func recorderIgnoresNonChordFlagsLikeCapsLockAndFunction() throws {
        // Unrelated NSEvent flags (caps lock, fn, numeric pad) must not leak
        // into the Carbon mask.
        let combo = try #require(
            HotkeyCombo(
                keyCode: 11,
                modifierFlags: [.command, .capsLock, .function, .numericPad]))
        #expect(combo.modifiers == HotkeyCombo.commandMask)
    }
}
