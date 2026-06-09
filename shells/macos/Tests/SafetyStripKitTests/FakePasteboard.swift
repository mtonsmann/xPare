import Foundation
@testable import SafetyStripKit

/// An in-memory `PasteboardProtocol` for deterministic tests — no NSPasteboard,
/// no global state. Tracks reads/writes and bumps `changeCount` on every write.
final class FakePasteboard: PasteboardProtocol {
    private(set) var changeCount: Int = 0
    private(set) var writes: [String] = []
    private(set) var readBestCalls: Int = 0
    private(set) var materializedReadCount: Int = 0
    var snapshot: PasteboardSnapshot?
    var rawRepresentationBytes: Int?

    init(snapshot: PasteboardSnapshot? = nil, rawRepresentationBytes: Int? = nil) {
        self.snapshot = snapshot
        self.rawRepresentationBytes = rawRepresentationBytes
    }

    func readBest(maxRepresentationBytes: Int) -> PasteboardReadResult {
        readBestCalls += 1
        let generation = changeCount
        if let rawRepresentationBytes,
            rawRepresentationBytes > maxRepresentationBytes
        {
            return .tooLarge(bytes: rawRepresentationBytes, changeCount: generation)
        }

        materializedReadCount += 1
        guard let snapshot else {
            return .empty(changeCount: generation)
        }
        return .content(PasteboardRead(snapshot: snapshot, changeCount: generation))
    }

    @discardableResult
    func writePlain(_ text: String) -> Int {
        writes.append(text)
        snapshot = PasteboardSnapshot(text: text, kind: .plain)
        rawRepresentationBytes = nil
        changeCount += 1
        return changeCount
    }

    /// Simulate an external app putting new content on the clipboard (bumps the
    /// change count the way a real external write would).
    func externalSet(_ snapshot: PasteboardSnapshot, rawRepresentationBytes: Int? = nil) {
        self.snapshot = snapshot
        self.rawRepresentationBytes = rawRepresentationBytes
        changeCount += 1
    }
}
