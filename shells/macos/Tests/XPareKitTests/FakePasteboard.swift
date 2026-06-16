import Foundation
@testable import XPareCore
@testable import XPareKit

/// An in-memory `PasteboardProtocol` for deterministic tests — no NSPasteboard,
/// no global state. Tracks reads/writes and bumps `changeCount` on every write.
final class FakePasteboard: PasteboardProtocol {
    private(set) var changeCount: Int = 0
    private(set) var writes: [String] = []
    private(set) var fileURLWrites: [URL] = []
    private(set) var readBestCalls: Int = 0
    private(set) var readImageCalls: Int = 0
    private(set) var materializedReadCount: Int = 0
    private(set) var materializedImageReadCount: Int = 0
    var snapshot: PasteboardSnapshot?
    var rawRepresentationBytes: Int?
    var image: PasteboardImage?
    var rawImageBytes: Int?
    var afterReadBest: (() -> Void)?
    var afterReadImageGenerationCaptured: (() -> Void)?
    /// Simulates the nspasteboard.org "do not process" marker types
    /// (concealed/transient/auto-generated) being declared by the writer.
    var hasDoNotProcessMarker: Bool = false
    /// When true, the next `writePlain` simulates the system rejecting the
    /// string write AFTER `clearContents` already ran (mirroring
    /// `SystemPasteboard`): the old contents are gone, the new string never
    /// landed, and the generation still advanced (the clear bumps it).
    var failNextPlainWrite = false
    /// Same simulation for `writeFileURL`.
    var failNextFileURLWrite = false

    init(
        snapshot: PasteboardSnapshot? = nil,
        rawRepresentationBytes: Int? = nil,
        image: PasteboardImage? = nil,
        rawImageBytes: Int? = nil
    ) {
        self.snapshot = snapshot
        self.rawRepresentationBytes = rawRepresentationBytes
        self.image = image
        self.rawImageBytes = rawImageBytes
    }

    func readBest(maxRepresentationBytes: Int) -> PasteboardReadResult {
        readBestCalls += 1
        let generation = changeCount
        defer {
            let callback = afterReadBest
            afterReadBest = nil
            callback?()
        }
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

    func readImage(maxRepresentationBytes: Int) -> PasteboardImageReadResult {
        readImageCalls += 1
        let generation = changeCount
        if let callback = afterReadImageGenerationCaptured {
            afterReadImageGenerationCaptured = nil
            callback()
        }
        if let rawImageBytes,
            rawImageBytes > maxRepresentationBytes
        {
            return .tooLarge(bytes: rawImageBytes, changeCount: generation)
        }

        materializedImageReadCount += 1
        guard let image else {
            return .empty(changeCount: generation)
        }
        return .content(PasteboardImageRead(image: image, changeCount: generation))
    }

    func writePlain(_ text: String) -> Int? {
        // The clear half of the in-place rewrite always runs: it empties the
        // pasteboard and bumps the generation even when the set half fails.
        changeCount += 1
        snapshot = nil
        rawRepresentationBytes = nil
        image = nil
        rawImageBytes = nil
        if failNextPlainWrite {
            failNextPlainWrite = false
            return nil
        }
        writes.append(text)
        snapshot = PasteboardSnapshot(text: text, kind: .plain)
        return changeCount
    }

    func writeFileURL(_ url: URL) -> Int? {
        // A file-URL pasteboard has no text-like representation to read back,
        // mirroring SystemPasteboard.writeFileURL (which writes only the URL type).
        changeCount += 1
        snapshot = nil
        rawRepresentationBytes = nil
        image = nil
        rawImageBytes = nil
        if failNextFileURLWrite {
            failNextFileURLWrite = false
            return nil
        }
        fileURLWrites.append(url)
        return changeCount
    }

    /// Simulate an external app putting new content on the clipboard (bumps the
    /// change count the way a real external write would).
    func externalSet(
        _ snapshot: PasteboardSnapshot,
        rawRepresentationBytes: Int? = nil,
        image: PasteboardImage? = nil,
        rawImageBytes: Int? = nil
    ) {
        self.snapshot = snapshot
        self.rawRepresentationBytes = rawRepresentationBytes
        self.image = image
        self.rawImageBytes = rawImageBytes
        changeCount += 1
    }

    /// Simulate an external app putting an image on the clipboard.
    func externalSetImage(_ image: PasteboardImage, rawImageBytes: Int? = nil) {
        snapshot = nil
        rawRepresentationBytes = nil
        self.image = image
        self.rawImageBytes = rawImageBytes
        changeCount += 1
    }
}

/// Records every transform call; shared by the StripController suites.
final class RecordingTransformer: Transforming, @unchecked Sendable {
    private let lock = NSLock()
    private let output: String
    private var _callCount = 0
    private var _configs: [TransformConfig] = []

    init(output: String) {
        self.output = output
    }

    var callCount: Int {
        lock.lock()
        defer { lock.unlock() }
        return _callCount
    }

    var configs: [TransformConfig] {
        lock.lock()
        defer { lock.unlock() }
        return _configs
    }

    func transform(_ input: String, config: TransformConfig) throws -> String {
        lock.lock()
        _callCount += 1
        _configs.append(config)
        lock.unlock()
        return output
    }
}
