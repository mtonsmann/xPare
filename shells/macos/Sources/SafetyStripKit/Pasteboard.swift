import Foundation
import AppKit
import SafetyStripCore

/// What we read off the pasteboard before stripping: the best available
/// representation plus a hint about which one it was, so the controller can
/// decide whether to run HTML stripping on it.
public struct PasteboardSnapshot: Equatable {
    public enum Kind: Equatable {
        /// `public.html` markup. Should be fed through `strip_html`.
        case html
        /// RTF flattened to its plain-string attributed value.
        case rtf
        /// A plain string with no richer representation available.
        case plain
    }

    public let text: String
    public let kind: Kind

    public init(text: String, kind: Kind) {
        self.text = text
        self.kind = kind
    }
}

/// A pasteboard read that carries the generation it came from. The controller
/// uses this to avoid writing stale transform output over newer clipboard data.
public struct PasteboardRead: Equatable {
    public let snapshot: PasteboardSnapshot
    public let changeCount: Int

    public init(snapshot: PasteboardSnapshot, changeCount: Int) {
        self.snapshot = snapshot
        self.changeCount = changeCount
    }
}

/// The result of a size-aware pasteboard read. Carries no clipboard content
/// unless a bounded snapshot was accepted.
public enum PasteboardReadResult: Equatable {
    case content(PasteboardRead)
    case empty(changeCount: Int)
    case tooLarge(bytes: Int, changeCount: Int)
}

/// Abstraction over the system pasteboard so the controller is testable without
/// touching `NSPasteboard.general`. The real implementation is
/// ``SystemPasteboard``.
public protocol PasteboardProtocol: AnyObject {
    /// Monotonic change counter (mirrors `NSPasteboard.changeCount`).
    var changeCount: Int { get }

    /// Read the best available representation, preferring rich formats so the
    /// core can coerce them to plain text. Reports `.empty` when there is no
    /// text-like content at all. Rich raw representations are size-checked
    /// before materializing/decoding them.
    func readBest(maxRepresentationBytes: Int) -> PasteboardReadResult

    /// Replace the pasteboard contents **in place** with a single plain string:
    /// `clearContents()` then `setString(_:forType: .string)`. No other types
    /// are written, so the rich representation is dropped — which is the point.
    /// Returns the pasteboard generation after the write.
    @discardableResult
    func writePlain(_ text: String) -> Int

    /// Replace the pasteboard contents **in place** with a single file reference
    /// (`clearContents()` then the URL object), used by the opt-in paste-as-file
    /// feature so pasting attaches the file instead of dumping the raw string.
    /// No string type is written alongside — that would defeat the feature.
    /// Returns the pasteboard generation after the write.
    @discardableResult
    func writeFileURL(_ url: URL) -> Int
}

/// `NSPasteboard.general`-backed pasteboard.
public final class SystemPasteboard: PasteboardProtocol {
    private let pasteboard: NSPasteboard

    public init(pasteboard: NSPasteboard = .general) {
        self.pasteboard = pasteboard
    }

    public var changeCount: Int {
        pasteboard.changeCount
    }

    public func readBest(maxRepresentationBytes: Int) -> PasteboardReadResult {
        let generation = pasteboard.changeCount
        let ceiling = max(0, maxRepresentationBytes)

        // 1. Prefer HTML markup — hand it to the core's strip_html. Read the raw
        //    bytes first so an oversized rich representation is refused before
        //    Swift/AppKit materializes it as a String.
        if let htmlData = pasteboard.data(forType: .html),
            !htmlData.isEmpty
        {
            if htmlData.count > ceiling {
                return .tooLarge(bytes: htmlData.count, changeCount: generation)
            }
            if let html = Self.decodeHtml(htmlData),
                !html.isEmpty
            {
                return .content(
                    PasteboardRead(
                        snapshot: PasteboardSnapshot(text: html, kind: .html),
                        changeCount: generation))
            }
        }

        // 2. Otherwise RTF: flatten to its plain attributed-string value. We do
        //    the RTF->plain decode here (an OS/AppKit concern) rather than in
        //    the core, which only deals in plain/HTML/Markdown text.
        if let rtfData = pasteboard.data(forType: .rtf),
            !rtfData.isEmpty
        {
            if rtfData.count > ceiling {
                return .tooLarge(bytes: rtfData.count, changeCount: generation)
            }
            if let attributed = try? NSAttributedString(
                data: rtfData,
                options: [.documentType: NSAttributedString.DocumentType.rtf],
                documentAttributes: nil),
                !attributed.string.isEmpty
            {
                return .content(
                    PasteboardRead(
                        snapshot: PasteboardSnapshot(text: attributed.string, kind: .rtf),
                        changeCount: generation))
            }
        }

        // 3. Fall back to a plain string.
        if let plain = pasteboard.string(forType: .string),
            !plain.isEmpty
        {
            return .content(
                PasteboardRead(
                    snapshot: PasteboardSnapshot(text: plain, kind: .plain),
                    changeCount: generation))
        }

        return .empty(changeCount: generation)
    }

    @discardableResult
    public func writePlain(_ text: String) -> Int {
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
        return pasteboard.changeCount
    }

    @discardableResult
    public func writeFileURL(_ url: URL) -> Int {
        pasteboard.clearContents()
        pasteboard.writeObjects([url as NSURL])
        return pasteboard.changeCount
    }

    private static func decodeHtml(_ data: Data) -> String? {
        // The final rung is a deliberate *lossy* UTF-8 decode (replacement chars) so we
        // always hand the core *some* markup rather than dropping the clipboard entirely
        // when every strict decode fails. That's exactly why the failable initializer that
        // `optional_data_string_conversion` prefers is wrong for that last rung.
        // swiftlint:disable optional_data_string_conversion
        String(data: data, encoding: .utf8)
            ?? String(data: data, encoding: .utf16)
            ?? String(data: data, encoding: .utf16LittleEndian)
            ?? String(data: data, encoding: .utf16BigEndian)
            ?? String(decoding: data, as: UTF8.self)
        // swiftlint:enable optional_data_string_conversion
    }
}
