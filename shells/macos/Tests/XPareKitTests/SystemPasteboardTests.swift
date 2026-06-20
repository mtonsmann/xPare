import AppKit
import Foundation
import Testing

@testable import XPareKit

/// Exercises the *real* `SystemPasteboard` against an app-private
/// `NSPasteboard(name:)` — a uniquely named pasteboard that works headlessly (no
/// GUI session, no touching `NSPasteboard.general`). This covers the AppKit-facing
/// read/decode/size-ceiling logic that `FakePasteboard` deliberately stubs out.
@Suite struct SystemPasteboardTests {

    /// A fresh, isolated pasteboard per test so cases never see each other's data.
    private func makePasteboard() -> (SystemPasteboard, NSPasteboard) {
        let raw = NSPasteboard(name: NSPasteboard.Name("xpare.test.\(UUID().uuidString)"))
        raw.clearContents()
        return (SystemPasteboard(pasteboard: raw), raw)
    }

    /// Build a real RTF byte blob for the given string via AppKit.
    private func rtfData(_ string: String) throws -> Data {
        let attributed = NSAttributedString(string: string)
        return try attributed.data(
            from: NSRange(location: 0, length: attributed.length),
            documentAttributes: [.documentType: NSAttributedString.DocumentType.rtf])
    }

    private func snapshot(_ result: PasteboardReadResult) -> PasteboardSnapshot? {
        guard case .content(let read) = result else { return nil }
        return read.snapshot
    }

    @Test func htmlRepresentationIsPreferredAndTaggedHtml() {
        let (pb, raw) = makePasteboard()
        raw.setData(Data("<p>hi  there</p>".utf8), forType: .html)
        // Also put plain text down; HTML must win.
        raw.setString("ignored plain", forType: .string)

        let snap = snapshot(pb.readBest(maxRepresentationBytes: 1_000))
        #expect(snap?.kind == .html)
        #expect(snap?.text == "<p>hi  there</p>")
    }

    @Test func utf16HtmlIsDecodedViaTheFallbackLadder() {
        let (pb, raw) = makePasteboard()
        // A UTF-16LE BOM (0xFF 0xFE) makes the UTF-8 rung of decodeHtml fail (those bytes
        // are not a valid UTF-8 start), so decoding falls through to the UTF-16 rung, which
        // reads the BOM to pick endianness. Exercises the fallback past plain UTF-8.
        let markup = "<p>café</p>"
        var bytes: [UInt8] = [0xFF, 0xFE]
        bytes.append(contentsOf: markup.utf16.flatMap { [UInt8($0 & 0xff), UInt8($0 >> 8)] })
        raw.setData(Data(bytes), forType: .html)

        let snap = snapshot(pb.readBest(maxRepresentationBytes: 1_000))
        #expect(snap?.kind == .html)
        #expect(snap?.text == markup)
    }

    @Test func rtfFlattensToItsPlainStringValue() throws {
        let (pb, raw) = makePasteboard()
        raw.setData(try rtfData("rich text value"), forType: .rtf)

        let snap = snapshot(pb.readBest(maxRepresentationBytes: 10_000))
        #expect(snap?.kind == .rtf)
        #expect(snap?.text == "rich text value")
    }

    @Test func plainStringIsTheFinalFallback() {
        let (pb, raw) = makePasteboard()
        raw.setString("just plain", forType: .string)

        let snap = snapshot(pb.readBest(maxRepresentationBytes: 1_000))
        #expect(snap?.kind == .plain)
        #expect(snap?.text == "just plain")
    }

    @Test func emptyPasteboardReadsAsEmpty() {
        let (pb, _) = makePasteboard()
        #expect(pb.readBest(maxRepresentationBytes: 1_000) == .empty(changeCount: pb.changeCount))
    }

    @Test func oversizedHtmlIsRefusedBeforeMaterialization() {
        let (pb, raw) = makePasteboard()
        let big = String(repeating: "x", count: 2_000)
        raw.setData(Data("<p>\(big)</p>".utf8), forType: .html)

        let result = pb.readBest(maxRepresentationBytes: 100)
        guard case .tooLarge(let bytes, _) = result else {
            Issue.record("expected .tooLarge, got \(result)")
            return
        }
        #expect(bytes > 100)
    }

    @Test func oversizedRtfIsRefusedBeforeDecoding() throws {
        let (pb, raw) = makePasteboard()
        raw.setData(try rtfData(String(repeating: "y", count: 2_000)), forType: .rtf)

        let result = pb.readBest(maxRepresentationBytes: 100)
        guard case .tooLarge(let bytes, _) = result else {
            Issue.record("expected .tooLarge, got \(result)")
            return
        }
        #expect(bytes > 100)
    }

    @Test func emptyHtmlDataFallsThroughToPlain() {
        let (pb, raw) = makePasteboard()
        // Empty HTML data must not short-circuit the ladder; plain should win.
        raw.setData(Data(), forType: .html)
        raw.setString("fell through", forType: .string)

        let snap = snapshot(pb.readBest(maxRepresentationBytes: 1_000))
        #expect(snap?.kind == .plain)
        #expect(snap?.text == "fell through")
    }

    @Test func negativeCeilingClampsToZeroAndRefusesAnyContent() {
        let (pb, raw) = makePasteboard()
        raw.setData(Data("<p>x</p>".utf8), forType: .html)
        // max(0, negative) == 0, so any non-empty representation is too large.
        guard case .tooLarge = pb.readBest(maxRepresentationBytes: -5) else {
            Issue.record("a negative ceiling must clamp to 0 and refuse content")
            return
        }
    }

    @Test func writePlainReplacesContentsInPlaceAndBumpsChangeCount() throws {
        let (pb, raw) = makePasteboard()
        raw.setData(Data("<p>rich</p>".utf8), forType: .html)
        let before = pb.changeCount

        let after = try #require(pb.writePlain("now plain only"), "system write should succeed")
        #expect(after > before, "writePlain must bump the change count")
        #expect(after == pb.changeCount)
        // The rich representation is gone; only the plain string remains.
        #expect(raw.data(forType: .html) == nil)
        #expect(raw.string(forType: .string) == "now plain only")
    }

    /// The three nspasteboard.org marker types are each recognized, and a
    /// plain unmarked pasteboard is not.
    @Test(arguments: [
        "org.nspasteboard.ConcealedType",
        "org.nspasteboard.TransientType",
        "org.nspasteboard.AutoGeneratedType",
    ])
    func doNotProcessMarkerIsDetected(markerType: String) {
        let (pb, raw) = makePasteboard()
        raw.declareTypes(
            [.string, NSPasteboard.PasteboardType(markerType)], owner: nil)
        raw.setString("x", forType: .string)
        #expect(pb.hasDoNotProcessMarker)
    }

    @Test func unmarkedContentHasNoDoNotProcessMarker() {
        let (pb, raw) = makePasteboard()
        raw.setString("ordinary", forType: .string)
        #expect(!pb.hasDoNotProcessMarker)
    }

    @Test func writeFileURLReplacesContentsWithOnlyAFileReference() throws {
        let (pb, raw) = makePasteboard()
        raw.setString("raw text to be replaced", forType: .string)
        let before = pb.changeCount

        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("SystemPasteboardTests \(UUID().uuidString).txt")
        try Data("file body".utf8).write(to: url)
        defer { try? FileManager.default.removeItem(at: url) }

        let after = try #require(pb.writeFileURL(url), "system write should succeed")
        #expect(after > before, "writeFileURL must bump the change count")
        #expect(after == pb.changeCount)
        // Only the file reference remains — no string type alongside (writing the
        // raw string too would defeat paste-as-file).
        let readBack = NSURL(from: raw)
        #expect(readBack?.isFileURL == true)
        #expect(readBack?.path == url.path)
        #expect(raw.string(forType: .string) != "raw text to be replaced")
    }

    @Test func rawImageRepresentationFailsClosedWithoutMaterializingData() {
        let (pb, raw) = makePasteboard()
        let png = Data([0x89, 0x50, 0x4e, 0x47])
        guard raw.setData(png, forType: .png) else {
            return  // Named pasteboards may be unavailable in headless/sandboxed agents.
        }

        let result = pb.readImage(maxRepresentationBytes: 16)
        #expect(result == .tooLarge(bytes: 17, changeCount: raw.changeCount))
    }

    @Test func rawImageRepresentationWithMaxIntCeilingReportsSaturatedTooLarge() {
        let (pb, raw) = makePasteboard()
        guard raw.setData(Data([0x49, 0x49, 0x2a, 0x00]), forType: .tiff) else {
            return  // Named pasteboards may be unavailable in headless/sandboxed agents.
        }

        let result = pb.readImage(maxRepresentationBytes: Int.max)
        #expect(result == .tooLarge(bytes: Int.max, changeCount: raw.changeCount))
    }

    @Test func imagePasteboardWithoutImageTypeStillReadsEmpty() {
        let (pb, raw) = makePasteboard()
        raw.setString("not an image", forType: .string)

        #expect(pb.readImage(maxRepresentationBytes: 16) == .empty(changeCount: raw.changeCount))
    }

    @Test func changeCountMirrorsTheUnderlyingPasteboard() {
        let (pb, raw) = makePasteboard()
        let start = pb.changeCount
        raw.clearContents()
        raw.setString("bump", forType: .string)
        #expect(pb.changeCount > start)
    }
}
