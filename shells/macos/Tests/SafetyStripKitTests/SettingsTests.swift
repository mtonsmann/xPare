import Testing
import Foundation
@testable import SafetyStripKit
@testable import SafetyStripCore

@Suite struct SettingsTests {

    @Test func defaultsAreOnDemandAndContinuousIsOptIn() {
        let s = Settings()
        #expect(s.mode == .onDemand, "continuous must be opt-in / off by default")
        #expect(s.pollIntervalMs == 500, "default poll interval is 500ms")
        #expect(s.hotkey == .defaultCombo)
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
        #expect(Settings.load(from: defaults) == Settings(),
                "a corrupt stored blob must degrade to defaults, not crash")
    }

    @Test func transformConfigBuiltFromSettings() {
        let s = Settings(operations: [
            .stripHtml,
            .collapseWhitespace,
            .maskIdentifiers(emails: true, ipv4: false, ipv6: true),
        ])
        let config = s.transformConfig()
        #expect(config.operations == [
            .stripHtml,
            .collapseWhitespace,
            .maskIdentifiers(emails: true, ipv4: false, ipv6: true),
        ])
        #expect(config.version == TransformConfig.schemaVersion)
    }
}
