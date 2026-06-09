import Testing
import Foundation
@testable import SafetyStripKit
@testable import SafetyStripCore

/// A `PasteFileWriting` whose writes always fail, to pin the controller's
/// degrade-to-plain-write fallback.
private final class FailingPasteFileStore: PasteFileWriting {
    private(set) var writeAttempts = 0
    func write(_ text: String) -> URL? {
        writeAttempts += 1
        return nil
    }
    func removeAll() {}
}

/// Controller behavior for the opt-in paste-as-file feature: threshold gating,
/// off-by-default, fallback on write failure, and stale-file cleanup.
@Suite @MainActor
struct PasteAsFileControllerTests {

    private func isolatedDefaults() throws -> (UserDefaults, String) {
        let suite = "PasteAsFileControllerTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        return (defaults, suite)
    }

    private func isolatedStore() -> (PasteFileStore, URL) {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent(
                "PasteAsFileControllerTests.\(UUID().uuidString)",
                isDirectory: true)
        return (PasteFileStore(directory: dir), dir)
    }

    /// ~2 KB of already-plain text (no trailing whitespace, so the pipeline's
    /// trim leaves it untouched) — above a 1 KB threshold, below any input ceiling.
    private let largeText = String(repeating: "lorem ipsum,", count: 170)

    private func enabledSettings(thresholdKB: Int = 1) -> Settings {
        Settings(
            mode: .onDemand,
            operations: [.trimTrailingWhitespace],
            pasteLargeAsFile: true,
            pasteAsFileThresholdKB: thresholdKB)
    }

    @Test func largeOutputBecomesAFileWhenEnabled() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }
        let (store, _) = isolatedStore()
        defer { store.removeAll() }

        let pb = FakePasteboard(snapshot: PasteboardSnapshot(text: largeText, kind: .plain))
        let controller = StripController(
            settings: enabledSettings(),
            pasteboard: pb,
            defaults: defaults,
            pasteFileStore: store
        )

        let outcome = await controller.stripNow(trigger: .manual)
        #expect(outcome == .strippedToFile)
        #expect(pb.writes.isEmpty, "no raw string may be written alongside the file")
        let url = try #require(pb.fileURLWrites.first)
        #expect(
            try String(contentsOf: url, encoding: .utf8) == largeText,
            "the file must hold exactly what a plain write would have pasted")
    }

    @Test func outputAtOrBelowThresholdStaysPlain() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }
        let (store, dir) = isolatedStore()
        defer { store.removeAll() }

        let pb = FakePasteboard(
            snapshot:
                PasteboardSnapshot(text: "small  text ", kind: .plain))
        let controller = StripController(
            settings: Settings(
                mode: .onDemand,
                operations: [.collapseWhitespace, .trimTrailingWhitespace],
                pasteLargeAsFile: true,
                pasteAsFileThresholdKB: 1),
            pasteboard: pb,
            defaults: defaults,
            pasteFileStore: store
        )

        let outcome = await controller.stripNow(trigger: .manual)
        #expect(outcome == .stripped(changed: true))
        #expect(pb.writes == ["small text"])
        #expect(pb.fileURLWrites.isEmpty)
        #expect(!FileManager.default.fileExists(atPath: dir.path))
    }

    @Test func featureIsOffByDefault() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }
        let (store, dir) = isolatedStore()
        defer { store.removeAll() }

        let pb = FakePasteboard(
            snapshot:
                PasteboardSnapshot(text: largeText + "  ", kind: .plain))
        // Default settings: pasteLargeAsFile must be false.
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.trimTrailingWhitespace]),
            pasteboard: pb,
            defaults: defaults,
            pasteFileStore: store
        )

        let outcome = await controller.stripNow(trigger: .manual)
        #expect(outcome == .stripped(changed: true))
        #expect(pb.fileURLWrites.isEmpty, "persistence must be strictly opt-in")
        #expect(!FileManager.default.fileExists(atPath: dir.path))
    }

    @Test func failedFileWriteFallsBackToPlainWrite() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard(
            snapshot:
                PasteboardSnapshot(text: largeText + "  ", kind: .plain))
        let failing = FailingPasteFileStore()
        let controller = StripController(
            settings: enabledSettings(),
            pasteboard: pb,
            defaults: defaults,
            pasteFileStore: failing
        )

        let outcome = await controller.stripNow(trigger: .manual)
        #expect(failing.writeAttempts == 1)
        #expect(
            outcome == .stripped(changed: true),
            "a failed file write must degrade to the normal in-place write")
        #expect(pb.writes == [largeText])
        #expect(pb.fileURLWrites.isEmpty)
    }

    @Test func staleFileIsDeletedOnceThePasteboardMovesOn() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }
        let (store, dir) = isolatedStore()
        defer { store.removeAll() }

        let pb = FakePasteboard(snapshot: PasteboardSnapshot(text: largeText, kind: .plain))
        let controller = StripController(
            settings: enabledSettings(),
            pasteboard: pb,
            defaults: defaults,
            pasteFileStore: store
        )

        #expect(await controller.stripNow(trigger: .manual) == .strippedToFile)
        let url = try #require(pb.fileURLWrites.first)
        #expect(FileManager.default.fileExists(atPath: url.path))

        // Another app replaces the clipboard — nothing references our file anymore.
        pb.externalSet(PasteboardSnapshot(text: "fresh small copy", kind: .plain))
        _ = await controller.stripNow(trigger: .manual)
        #expect(
            !FileManager.default.fileExists(atPath: url.path),
            "the stale paste file must be removed on the next strip")
        #expect(!FileManager.default.fileExists(atPath: dir.path))
    }

    @Test func deactivateRemovesThePasteFile() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }
        let (store, dir) = isolatedStore()

        let pb = FakePasteboard(snapshot: PasteboardSnapshot(text: largeText, kind: .plain))
        let controller = StripController(
            settings: enabledSettings(),
            pasteboard: pb,
            defaults: defaults,
            pasteFileStore: store
        )

        #expect(await controller.stripNow(trigger: .manual) == .strippedToFile)
        controller.deactivate()
        #expect(
            !FileManager.default.fileExists(atPath: dir.path),
            "no paste file may outlive the controller")
    }
}
