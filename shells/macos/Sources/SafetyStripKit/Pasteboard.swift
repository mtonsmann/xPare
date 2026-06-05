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

/// Abstraction over the system pasteboard so the controller is testable without
/// touching `NSPasteboard.general`. The real implementation is
/// ``SystemPasteboard``.
public protocol PasteboardProtocol: AnyObject {
    /// Monotonic change counter (mirrors `NSPasteboard.changeCount`).
    var changeCount: Int { get }

    /// Read the best available representation, preferring rich formats so the
    /// core can coerce them to plain text. Returns `nil` when there is no
    /// text-like content at all.
    func readBest() -> PasteboardSnapshot?

    /// Replace the pasteboard contents **in place** with a single plain string:
    /// `clearContents()` then `setString(_:forType: .string)`. No other types
    /// are written, so the rich representation is dropped — which is the point.
    func writePlain(_ text: String)
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

    public func readBest() -> PasteboardSnapshot? {
        // 1. Prefer HTML markup — hand it to the core's strip_html.
        if let html = pasteboard.string(forType: .html),
           !html.isEmpty {
            return PasteboardSnapshot(text: html, kind: .html)
        }

        // 2. Otherwise RTF: flatten to its plain attributed-string value. We do
        //    the RTF->plain decode here (an OS/AppKit concern) rather than in
        //    the core, which only deals in plain/HTML/Markdown text.
        if let rtfData = pasteboard.data(forType: .rtf),
           let attributed = try? NSAttributedString(
               data: rtfData,
               options: [.documentType: NSAttributedString.DocumentType.rtf],
               documentAttributes: nil),
           !attributed.string.isEmpty {
            return PasteboardSnapshot(text: attributed.string, kind: .rtf)
        }

        // 3. Fall back to a plain string.
        if let plain = pasteboard.string(forType: .string),
           !plain.isEmpty {
            return PasteboardSnapshot(text: plain, kind: .plain)
        }

        return nil
    }

    public func writePlain(_ text: String) {
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
    }
}
