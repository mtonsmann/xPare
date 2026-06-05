import Testing
import Foundation
@testable import SafetyStripCore

/// Exercises the real linked Rust core through the C ABI. Passing these proves
/// the staticlib link, the `(ptr,len)` buffer protocol, and `ss_buffer_free`
/// all work from Swift end to end. (swift-testing; see TransformConfigTests for
/// why not XCTest.)
@Suite struct TransformerIntegrationTests {
    private let transformer = Transformer()

    @Test func abiVersionMatchesHeader() {
        // The frozen header pins SS_ABI_VERSION == 1.
        #expect(transformer.abiVersion() == 1)
    }

    @Test func capabilitiesIsJSONWithOperations() throws {
        let caps = transformer.capabilities()
        #expect(!caps.isEmpty)
        let obj = try JSONSerialization.jsonObject(with: Data(caps.utf8))
        let dict = try #require(obj as? [String: Any])
        #expect(dict["config_version"] as? Int == 1)
        #expect(dict["operations"] as? [Any] != nil)
    }

    @Test func stripHtmlAndCollapseWhitespace() throws {
        // The headline integration case from the brief: strip_html on
        // "<p>hi  there</p>" then collapse_whitespace.
        let config = TransformConfig(operations: [.stripHtml, .collapseWhitespace])
        let out = try transformer.transform("<p>hi  there</p>", config: config)
        #expect(out == "hi there")
    }

    @Test func changeCaseUpper() throws {
        let config = TransformConfig(operations: [.changeCase(case: .upper)])
        let out = try transformer.transform("hello world", config: config)
        #expect(out == "HELLO WORLD")
    }

    @Test func emptyInputIsHandled() throws {
        // input_len == 0 with no operations is the identity transform; the ABI
        // explicitly permits a null/empty input here.
        let out = try transformer.transform("", config: TransformConfig())
        #expect(out == "")
    }

    @Test func identityRoundTripsUnicode() throws {
        let s = "café — emoji 😀 and tabs\tend"
        let out = try transformer.transform(s, config: TransformConfig())
        #expect(out == s)
    }

    @Test func invalidConfigVersionThrows() {
        // Hand the core a config with an unsupported version; expect the mapped
        // invalidConfig error, not a crash.
        let badJSON = #"{"version":999,"operations":[]}"#
        #expect(throws: TransformError.invalidConfig) {
            try transformer.transform("x", configJSON: badJSON)
        }
    }

    @Test func malformedConfigThrows() {
        #expect(throws: TransformError.invalidConfig) {
            try transformer.transform("x", configJSON: "not json")
        }
    }

    @Test func manyTransformsDoNotLeakOrCrash() throws {
        // Repeatedly allocate + free output buffers to shake out double-free or
        // use-after-free in the wrapper's buffer handling.
        let config = TransformConfig(operations: [.collapseWhitespace])
        for i in 0..<500 {
            let out = try transformer.transform("a    b    \(i)", config: config)
            #expect(out == "a b \(i)")
        }
    }
}
