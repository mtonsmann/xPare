import Testing
import Foundation
@testable import SafetyStripCore

// NOTE ON TEST FRAMEWORK
// ----------------------
// These suites use swift-testing (`import Testing`), not XCTest. This is a hard
// environment constraint: with Command-Line-Tools only (no full Xcode) the
// XCTest framework is not installed, while swift-testing's Testing.framework
// ships with the toolchain and `swift test` runs it. The assertions map 1:1 to
// the XCTest the brief calls for (`#expect` ~ `XCTAssert*`, `#require` ~
// `XCTUnwrap`, `throws:` ~ `XCTAssertThrowsError`).

/// Verifies the Swift `TransformConfig`/`Operation`/`CaseKind` encode to the
/// EXACT JSON wire schema the Rust core's `parse_config` expects
/// (see core/src/config.rs).
@Suite struct TransformConfigTests {

    /// Decode a JSON string into a generic object for order-independent
    /// structural comparison.
    private func jsonObject(_ s: String) throws -> NSDictionary {
        let data = Data(s.utf8)
        let obj = try JSONSerialization.jsonObject(with: data)
        return try #require(obj as? NSDictionary)
    }

    @Test func samplePipelineEncodesToExpectedJSON() throws {
        // strip_html -> change_case(title) -> collapse_whitespace
        let config = TransformConfig(operations: [
            .stripHtml,
            .changeCase(case: .title),
            .collapseWhitespace,
        ])
        let json = try config.encodedJSON()

        let expected = """
        {"version":2,"operations":[{"op":"strip_html"},{"op":"change_case","case":"title"},{"op":"collapse_whitespace"}],"ordering":"canonical"}
        """

        // Compare structurally (key order within objects is not significant to
        // the core, which uses serde), but assert the full shape matches.
        #expect(try jsonObject(json) == jsonObject(expected))
    }

    @Test func noPayloadOpsEncodeAsBareTag() throws {
        let config = TransformConfig(operations: [
            .htmlToMarkdown, .dedupeLines, .extractEmails, .extractUrls
        ])
        let json = try config.encodedJSON()
        let expected = """
        {"version":2,"operations":[{"op":"html_to_markdown"},{"op":"dedupe_lines"},{"op":"extract_emails"},{"op":"extract_urls"}],"ordering":"canonical"}
        """
        #expect(try jsonObject(json) == jsonObject(expected))
    }

    @Test func sortLinesEncodesBothFlags() throws {
        let config = TransformConfig(operations: [
            .sortLines(descending: true, caseInsensitive: false)
        ])
        let json = try config.encodedJSON()
        let dict = try jsonObject(json)
        let ops = try #require(dict["operations"] as? [[String: Any]])
        #expect(ops.count == 1)
        #expect(ops[0]["op"] as? String == "sort_lines")
        #expect(ops[0]["descending"] as? Bool == true)
        #expect(ops[0]["case_insensitive"] as? Bool == false)
    }

    @Test func parameterizedStringOps() throws {
        let config = TransformConfig(operations: [
            .prefixLines(prefix: "> "),
            .suffixLines(suffix: ";"),
            .joinWith(separator: ", "),
            .splitOn(delimiter: "|"),
        ])
        let json = try config.encodedJSON()
        let expected = """
        {"version":2,"operations":[\
        {"op":"prefix_lines","prefix":"> "},\
        {"op":"suffix_lines","suffix":";"},\
        {"op":"join_with","separator":", "},\
        {"op":"split_on","delimiter":"|"}],"ordering":"canonical"}
        """
        #expect(try jsonObject(json) == jsonObject(expected))
    }

    @Test func iocOpsEncodeToWireSchema() throws {
        // defang carries its style; refang and clean_urls are bare tags.
        let config = TransformConfig(operations: [
            .defang(style: .square),
            .refang,
            .cleanUrls,
        ])
        let json = try config.encodedJSON()
        let expected = """
        {"version":2,"operations":[\
        {"op":"defang","style":"square"},\
        {"op":"refang"},\
        {"op":"clean_urls"}],"ordering":"canonical"}
        """
        #expect(try jsonObject(json) == jsonObject(expected))
    }

    @Test func defangRoundStyleEncodes() throws {
        let json = try TransformConfig(operations: [.defang(style: .round)]).encodedJSON()
        let dict = try jsonObject(json)
        let ops = try #require(dict["operations"] as? [[String: Any]])
        #expect(ops[0]["op"] as? String == "defang")
        #expect(ops[0]["style"] as? String == "round")
    }

    @Test func defangDecodesWithDefaultStyleWhenAbsent() throws {
        // serde defaults a missing `style` to square; the Swift mirror must match.
        let json = #"{"version":2,"operations":[{"op":"defang"}]}"#
        let decoded = try JSONDecoder().decode(TransformConfig.self, from: Data(json.utf8))
        #expect(decoded == TransformConfig(operations: [.defang(style: .square)]))
    }

    @Test func iocOpsRoundTripThroughCodable() throws {
        let original = TransformConfig(operations: [
            .defang(style: .round),
            .refang,
            .cleanUrls,
        ])
        let json = try original.encodedJSON()
        let decoded = try JSONDecoder().decode(TransformConfig.self, from: Data(json.utf8))
        #expect(decoded == original)
    }

    @Test func allBracketStylesRawValues() {
        #expect(BracketStyle.square.rawValue == "square")
        #expect(BracketStyle.round.rawValue == "round")
    }

    @Test func allCaseKindsRawValues() {
        #expect(CaseKind.upper.rawValue == "upper")
        #expect(CaseKind.lower.rawValue == "lower")
        #expect(CaseKind.title.rawValue == "title")
        #expect(CaseKind.sentence.rawValue == "sentence")
    }

    @Test func emptyConfigEncodesIdentity() throws {
        // The bare TransformConfig() is the identity transform: version 2, no ops.
        let json = try TransformConfig().encodedJSON()
        let dict = try jsonObject(json)
        #expect(dict["version"] as? Int == 2)
        #expect((dict["operations"] as? [Any])?.count == 0)
    }

    @Test func orderingEncodesAndDefaultsToCanonical() throws {
        // Default ordering is canonical and is always emitted on the wire.
        let dict = try jsonObject(try TransformConfig(operations: []).encodedJSON())
        #expect(dict["ordering"] as? String == "canonical")

        // Non-default round-trips on the wire.
        let asGiven = try jsonObject(
            try TransformConfig(operations: [], ordering: .asGiven).encodedJSON())
        #expect(asGiven["ordering"] as? String == "as_given")

        // A config that omits `ordering` decodes to canonical (mirrors serde default).
        let decoded = try JSONDecoder().decode(
            TransformConfig.self, from: Data(#"{"version":2,"operations":[]}"#.utf8))
        #expect(decoded.ordering == .canonical)
    }

    @Test func allOrderingsRawValues() {
        #expect(Ordering.canonical.rawValue == "canonical")
        #expect(Ordering.asGiven.rawValue == "as_given")
    }

    @Test func roundTripThroughCodable() throws {
        let original = TransformConfig(operations: [
            .stripHtml,
            .changeCase(case: .sentence),
            .sortLines(descending: false, caseInsensitive: true),
            .joinWith(separator: "\n"),
        ])
        let json = try original.encodedJSON()
        let decoded = try JSONDecoder().decode(TransformConfig.self, from: Data(json.utf8))
        #expect(decoded == original)
    }
}
