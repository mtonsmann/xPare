import Testing
import Foundation
@testable import SafetyStripKit

/// `PasteFileStore` is the single sanctioned content-persistence point (opt-in
/// paste-as-file). These tests pin its mitigations: one file at a time,
/// owner-only permissions, full cleanup. All I/O happens in an isolated temp
/// directory, never the real store location.
@Suite struct PasteFileStoreTests {

    private func isolatedStore() -> (PasteFileStore, URL) {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("PasteFileStoreTests.\(UUID().uuidString)",
                                    isDirectory: true)
        return (PasteFileStore(directory: dir), dir)
    }

    private func contents(of dir: URL) -> [String] {
        (try? FileManager.default.contentsOfDirectory(atPath: dir.path)) ?? []
    }

    @Test func writeCreatesFileWithExactContentAndOwnerOnlyPermissions() throws {
        let (store, dir) = isolatedStore()
        defer { store.removeAll() }

        let text = String(repeating: "secret ", count: 100)
        let url = try #require(store.write(text))

        #expect(try String(contentsOf: url, encoding: .utf8) == text)
        #expect(url.pathExtension == "txt")
        #expect(url.deletingLastPathComponent().path == dir.path)

        let fileAttrs = try FileManager.default.attributesOfItem(atPath: url.path)
        #expect((fileAttrs[.posixPermissions] as? Int) == 0o600,
                "the paste file must be owner-only")
        let dirAttrs = try FileManager.default.attributesOfItem(atPath: dir.path)
        #expect((dirAttrs[.posixPermissions] as? Int) == 0o700,
                "the store directory must be owner-only")
    }

    @Test func eachWriteReplacesThePreviousFile() throws {
        let (store, dir) = isolatedStore()
        defer { store.removeAll() }

        let first = try #require(store.write("first"))
        let second = try #require(store.write("second"))

        #expect(contents(of: dir).count == 1, "at most one paste file may exist")
        #expect(!FileManager.default.fileExists(atPath: first.path),
                "the previous paste file must be gone")
        #expect(try String(contentsOf: second, encoding: .utf8) == "second")
        #expect(first.lastPathComponent != second.lastPathComponent,
                "a replacing write must not reuse the previous URL")
    }

    @Test func removeAllDeletesTheWholeDirectory() throws {
        let (store, dir) = isolatedStore()
        _ = try #require(store.write("transient"))

        store.removeAll()
        #expect(!FileManager.default.fileExists(atPath: dir.path),
                "removeAll must leave nothing behind, not even the directory")
    }

    @Test func removeAllIsSafeWhenNothingWasWritten() {
        let (store, _) = isolatedStore()
        store.removeAll() // must not crash or throw
    }

    @Test func fileNameCarriesNoContent() throws {
        let (store, _) = isolatedStore()
        defer { store.removeAll() }

        let url = try #require(store.write("hunter2 password"))
        #expect(!url.lastPathComponent.contains("hunter2"),
                "the file name must never be derived from clipboard content")
        #expect(url.lastPathComponent.hasPrefix("Clipboard "))
    }
}
