import Foundation
import AppKit
import SafetyStripCore
import UniformTypeIdentifiers

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

/// Bounded image representation read from the pasteboard for local OCR. The raw
/// bytes are clipboard-derived content, so callers must keep them local and
/// short-lived.
public struct PasteboardImage: Equatable, Sendable {
    public let data: Data
    public let pasteboardType: String

    public init(data: Data, pasteboardType: String) {
        self.data = data
        self.pasteboardType = pasteboardType
    }
}

/// An image pasteboard read that carries the generation it came from.
public struct PasteboardImageRead: Equatable, Sendable {
    public let image: PasteboardImage
    public let changeCount: Int

    public init(image: PasteboardImage, changeCount: Int) {
        self.image = image
        self.changeCount = changeCount
    }
}

/// The result of a size-aware image read. Carries no clipboard content unless a
/// bounded image representation was accepted.
public enum PasteboardImageReadResult: Equatable, Sendable {
    case content(PasteboardImageRead)
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

    /// Read a bounded image representation for explicit local OCR. Reports
    /// `.empty` when no image-like representation is present. Raw image bytes are
    /// size-checked before Vision decodes or recognizes them.
    func readImage(maxRepresentationBytes: Int) -> PasteboardImageReadResult

    /// Replace the pasteboard contents **in place** with a single plain string:
    /// `clearContents()` then `setString(_:forType: .string)`. No other types
    /// are written, so the rich representation is dropped — which is the point.
    /// Returns the pasteboard generation after the write.
    @discardableResult
    func writePlain(_ text: String) -> Int
}

/// `NSPasteboard.general`-backed pasteboard.
public final class SystemPasteboard: PasteboardProtocol {
    private let pasteboard: NSPasteboard
    private static let preferredImageTypes: [NSPasteboard.PasteboardType] = [
        .png,
        NSPasteboard.PasteboardType("public.jpeg"),
        NSPasteboard.PasteboardType("public.heic"),
        NSPasteboard.PasteboardType("public.heif"),
        .tiff,
    ]

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
           !htmlData.isEmpty {
            if htmlData.count > ceiling {
                return .tooLarge(bytes: htmlData.count, changeCount: generation)
            }
            if let html = Self.decodeHtml(htmlData),
               !html.isEmpty {
                return .content(PasteboardRead(
                    snapshot: PasteboardSnapshot(text: html, kind: .html),
                    changeCount: generation))
            }
        }

        // 2. Otherwise RTF: flatten to its plain attributed-string value. We do
        //    the RTF->plain decode here (an OS/AppKit concern) rather than in
        //    the core, which only deals in plain/HTML/Markdown text.
        if let rtfData = pasteboard.data(forType: .rtf),
           !rtfData.isEmpty {
            if rtfData.count > ceiling {
                return .tooLarge(bytes: rtfData.count, changeCount: generation)
            }
            if let attributed = try? NSAttributedString(
                data: rtfData,
                options: [.documentType: NSAttributedString.DocumentType.rtf],
                documentAttributes: nil),
               !attributed.string.isEmpty {
                return .content(PasteboardRead(
                    snapshot: PasteboardSnapshot(text: attributed.string, kind: .rtf),
                    changeCount: generation))
            }
        }

        // 3. Fall back to a plain string.
        if let plain = pasteboard.string(forType: .string),
           !plain.isEmpty {
            return .content(PasteboardRead(
                snapshot: PasteboardSnapshot(text: plain, kind: .plain),
                changeCount: generation))
        }

        return .empty(changeCount: generation)
    }

    public func readImage(maxRepresentationBytes: Int) -> PasteboardImageReadResult {
        let generation = pasteboard.changeCount
        let ceiling = max(0, maxRepresentationBytes)
        var largestOversizedRepresentation: Int?

        for type in imageTypesAvailableOnPasteboard() {
            guard let data = pasteboard.data(forType: type),
                  !data.isEmpty else {
                continue
            }
            if data.count > ceiling {
                largestOversizedRepresentation = max(largestOversizedRepresentation ?? 0, data.count)
                continue
            }
            return .content(PasteboardImageRead(
                image: PasteboardImage(data: data, pasteboardType: type.rawValue),
                changeCount: generation))
        }

        if let bytes = largestOversizedRepresentation {
            return .tooLarge(bytes: bytes, changeCount: generation)
        }
        return .empty(changeCount: generation)
    }

    @discardableResult
    public func writePlain(_ text: String) -> Int {
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
        return pasteboard.changeCount
    }

    private static func decodeHtml(_ data: Data) -> String? {
        String(data: data, encoding: .utf8)
            ?? String(data: data, encoding: .utf16)
            ?? String(data: data, encoding: .utf16LittleEndian)
            ?? String(data: data, encoding: .utf16BigEndian)
            ?? String(decoding: data, as: UTF8.self)
    }

    private func imageTypesAvailableOnPasteboard() -> [NSPasteboard.PasteboardType] {
        guard let availableTypes = pasteboard.types else { return [] }
        var result: [NSPasteboard.PasteboardType] = []

        for type in Self.preferredImageTypes where availableTypes.contains(type) {
            result.append(type)
        }

        for type in availableTypes {
            guard !result.contains(type),
                  Self.isImageType(type) else {
                continue
            }
            result.append(type)
        }

        return result
    }

    private static func isImageType(_ pasteboardType: NSPasteboard.PasteboardType) -> Bool {
        guard let type = UTType(pasteboardType.rawValue) else { return false }
        return type.conforms(to: .image)
    }
}
