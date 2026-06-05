import Foundation
@testable import SafetyStripKit

/// An in-memory `PasteboardProtocol` for deterministic tests — no NSPasteboard,
/// no global state. Tracks reads/writes and bumps `changeCount` on every write.
final class FakePasteboard: PasteboardProtocol {
    private(set) var changeCount: Int = 0
    private(set) var writes: [String] = []
    var snapshot: PasteboardSnapshot?

    init(snapshot: PasteboardSnapshot? = nil) {
        self.snapshot = snapshot
    }

    func readBest() -> PasteboardSnapshot? {
        snapshot
    }

    func writePlain(_ text: String) {
        writes.append(text)
        snapshot = PasteboardSnapshot(text: text, kind: .plain)
        changeCount += 1
    }

    /// Simulate an external app putting new content on the clipboard (bumps the
    /// change count the way a real external write would).
    func externalSet(_ snapshot: PasteboardSnapshot) {
        self.snapshot = snapshot
        changeCount += 1
    }
}
