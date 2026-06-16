import Testing
import Foundation
@testable import XPareKit
@testable import XPareCore

@Suite struct SettingsTests {

    @Test func defaultsAreOnDemandAndContinuousIsOptIn() {
        let s = Settings()
        #expect(s.mode == .onDemand, "continuous must be opt-in / off by default")
        #expect(s.pollIntervalMs == 500, "default poll interval is 500ms")
        #expect(s.hotkey == .defaultCombo)
    }

    @Test func defaultHotkeyIsControlOptionCommandV() {
        // ⌃⌥⌘V: kVK_ANSI_V (9) + cmdKey | optionKey | controlKey.
        #expect(HotkeyCombo.defaultCombo.keyCode == 9)
        #expect(HotkeyCombo.defaultCombo.modifiers == 0x0100 | 0x0800 | 0x1000)
    }

    /// Changing the *default* combo must not rewrite what users already chose:
    /// a persisted blob carrying the old ⌥⌘V default decodes to exactly that
    /// combo, not to the new default.
    @Test func persistedHotkeyFromAnOlderBuildIsPreservedVerbatim() throws {
        let legacy = Data(#"{"hotkey":{"keyCode":9,"modifiers":2304}}"#.utf8)  // 0x0900 = ⌥⌘
        let decoded = try JSONDecoder().decode(Settings.self, from: legacy)
        #expect(decoded.hotkey == HotkeyCombo(keyCode: 9, modifiers: 0x0100 | 0x0800))
        #expect(decoded.hotkey != .defaultCombo, "stored combos must not be silently upgraded")
    }

    @Test func codableRoundTrip() throws {
        let original = Settings(
            mode: .continuous,
            operations: [
                .stripHtml,
                .changeCase(case: .title),
                .sortLines(descending: true, caseInsensitive: false),
                .maskIdentifiers(emails: true, ipv4: true, ipv6: false),
                .prefixLines(prefix: "- "),
            ],
            hotkey: HotkeyCombo(keyCode: 9, modifiers: 0x0100 | 0x0800),
            pollIntervalMs: 250
        )
        let data = try JSONEncoder().encode(original)
        let decoded = try JSONDecoder().decode(Settings.self, from: data)
        #expect(decoded == original)
    }

    @Test func userDefaultsPersistenceRoundTrip() throws {
        // Use an isolated suite so we don't touch the user's real defaults.
        let suite = "SettingsTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }

        let original = Settings(
            mode: .continuous,
            operations: [.collapseWhitespace, .removeBlankLines],
            hotkey: .defaultCombo,
            pollIntervalMs: 750
        )
        original.save(to: defaults)

        let loaded = Settings.load(from: defaults)
        #expect(loaded == original)
    }

    @Test func loadFallsBackToDefaultsWhenAbsent() throws {
        let suite = "SettingsTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }

        #expect(Settings.load(from: defaults) == Settings())
    }

    @Test func loadFallsBackToDefaultsWhenCorrupt() throws {
        let suite = "SettingsTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }

        defaults.set(Data("garbage not json".utf8), forKey: Settings.defaultsKey)
        #expect(
            Settings.load(from: defaults) == Settings(),
            "a corrupt stored blob must degrade to defaults, not crash")
    }

    @Test func pasteAsFileIsOffByDefaultAndOldBlobsDecodeTolerantly() throws {
        let s = Settings()
        #expect(!s.pasteLargeAsFile, "content persistence must be opt-in / off by default")
        #expect(s.pasteAsFileThresholdKB == Settings.defaultPasteAsFileThresholdKB)

        // A blob saved by an older build (no paste-as-file keys) upgrades to defaults.
        let old = #"{"mode":"continuous","pollIntervalMs":250}"#
        let decoded = try JSONDecoder().decode(Settings.self, from: Data(old.utf8))
        #expect(decoded.pasteLargeAsFile == false)
        #expect(decoded.pasteAsFileThresholdKB == Settings.defaultPasteAsFileThresholdKB)
    }

    @Test func pasteAsFileRoundTripsAndThresholdClampsToAtLeastOneKB() throws {
        let original = Settings(pasteLargeAsFile: true, pasteAsFileThresholdKB: 64)
        let data = try JSONEncoder().encode(original)
        let decoded = try JSONDecoder().decode(Settings.self, from: data)
        #expect(decoded == original)
        #expect(decoded.pasteAsFileThresholdBytes == 64 * 1024)

        var clamped = Settings(pasteAsFileThresholdKB: 0)
        #expect(
            clamped.pasteAsFileThresholdBytes == 1024,
            "a zero threshold must not turn every strip into a file write")
        clamped.pasteAsFileThresholdKB = -5
        #expect(clamped.pasteAsFileThresholdBytes == 1024)

        // The upper clamp: an absurd typed/corrupted KB value must saturate, not
        // overflow-trap the `* 1024` (a crash on every strip while enabled).
        clamped.pasteAsFileThresholdKB = Int.max
        #expect(clamped.pasteAsFileThresholdBytes == (Int.max / 1024) * 1024)
    }

    @Test func transformConfigBuiltFromSettings() {
        let s = Settings(operations: [
            .stripHtml,
            .collapseWhitespace,
            .maskIdentifiers(emails: true, ipv4: false, ipv6: true),
        ])
        let config = s.transformConfig()
        #expect(
            config.operations == [
                .stripHtml,
                .collapseWhitespace,
                .maskIdentifiers(emails: true, ipv4: false, ipv6: true),
            ])
        #expect(config.version == TransformConfig.schemaVersion)
    }

    @Test func transformConfigNormalizesPersistedFreeTextParamsForCurrentSchema() {
        let tooLong = String(repeating: "x", count: TransformConfig.maxTextParameterBytes + 4)
        let expected = String(repeating: "x", count: TransformConfig.maxTextParameterBytes)
        let cases: [(XPareCore.Operation, XPareCore.Operation)] = [
            (.prefixLines(prefix: tooLong), .prefixLines(prefix: expected)),
            (.suffixLines(suffix: tooLong), .suffixLines(suffix: expected)),
            (.joinWith(separator: tooLong), .joinWith(separator: expected)),
            (.splitOn(delimiter: tooLong), .splitOn(delimiter: expected)),
        ]

        for (input, output) in cases {
            #expect(Settings(operations: [input]).transformConfig().operations == [output])
        }
    }

    @Test func transformConfigClampsPersistedPipelineToAggregateGrowthEnvelope() {
        let maxParam = String(repeating: "x", count: TransformConfig.maxTextParameterBytes)
        let config = Settings(operations: [
            .prefixLines(prefix: maxParam),
            .suffixLines(suffix: maxParam),
            .joinWith(separator: maxParam),
            .splitOn(delimiter: maxParam),
        ]).transformConfig()

        #expect(
            config.operations == [
                .prefixLines(prefix: maxParam),
                .suffixLines(suffix: maxParam),
                .joinWith(separator: String(repeating: "x", count: 14)),
                .splitOn(delimiter: maxParam),
            ])
        #expect(
            TransformConfig.currentSchemaGrowthProduct(config.operations)
                <= TransformConfig.maxPipelineGrowthFactor)
    }

    @Test func transformConfigCapsPersistedOperationCountForCurrentSchema() {
        let oversized = Array(
            repeating: XPareCore.Operation.collapseWhitespace,
            count: TransformConfig.maxOperations + 2)

        #expect(
            Settings(operations: oversized).transformConfig().operations.count
                == TransformConfig.maxOperations)
    }

    @Test func textParameterNormalizationIsUtf8SafeAndDropsLineBreaks() {
        #expect(
            TransformConfig.normalizedTextParameter(String(repeating: "é", count: 9))
                == String(repeating: "é", count: 8))
        #expect(TransformConfig.normalizedTextParameter("ab\ncd\rEF") == "abcdEF")
        #expect(TransformConfig.normalizedTextParameter("ab\r\ncd") == "abcd")
    }

    @Test func partialBlobFromAnOlderBuildUpgradesMissingFieldsToDefaults() throws {
        // A settings JSON written by an older build that predates `ordering` (and omits
        // several fields) must decode tolerantly — each absent key falls back to its
        // default rather than throwing. Exercises the `decodeIfPresent ?? default` ladder.
        let legacy = Data(#"{"mode":"continuous"}"#.utf8)
        let decoded = try JSONDecoder().decode(Settings.self, from: legacy)

        #expect(decoded.mode == .continuous, "the present field is honored")
        #expect(decoded.operations == Settings.defaultOperations)
        #expect(decoded.hotkey == .defaultCombo)
        #expect(decoded.pollIntervalMs == 500)
        #expect(decoded.ordering == .canonical)
    }

    @Test func emptyObjectDecodesToAllDefaults() throws {
        // The fully-empty case: `{}` is a valid settings blob that yields a default Settings.
        let decoded = try JSONDecoder().decode(Settings.self, from: Data("{}".utf8))
        #expect(decoded == Settings())
    }
}
