import Foundation

/// Case transformation kinds. Mirrors the Rust `CaseKind` enum
/// (`#[serde(rename_all = "snake_case")]`), so the raw values are the exact
/// JSON strings the core expects.
public enum CaseKind: String, Codable, Sendable, CaseIterable {
    case upper
    case lower
    case title
    case sentence
}

/// Bracket convention used by `Operation.defang`. Mirrors the Rust `BracketStyle`
/// enum (`#[serde(rename_all = "snake_case")]`), so the raw values are the exact
/// JSON strings the core expects.
public enum BracketStyle: String, Codable, Sendable, CaseIterable {
    case square
    case round
}

/// One transformation step.
///
/// Encodes to the **exact** JSON the Rust core's `Operation` enum expects:
/// internally tagged on `"op"`, snake_case variant names, e.g.
/// `{"op":"strip_html"}` or `{"op":"change_case","case":"title"}`.
/// See `core/src/config.rs` — this is the frozen wire schema.
///
/// We implement `Codable` by hand (rather than deriving) because Swift's
/// synthesized enum coding uses a different envelope shape than Serde's
/// internally-tagged representation.
public enum Operation: Equatable, Sendable {
    // --- Must (common baseline) ---
    case stripHtml
    case stripMarkdown
    case collapseWhitespace
    case trimTrailingWhitespace
    case removeBlankLines
    case unwrapLines
    case changeCase(case: CaseKind)

    // --- Stretch ---
    case sortLines(descending: Bool, caseInsensitive: Bool)
    case dedupeLines
    case prefixLines(prefix: String)
    case suffixLines(suffix: String)
    case joinWith(separator: String)
    case splitOn(delimiter: String)
    case extractEmails
    case extractUrls

    // --- IOC handling (rewrites) ---
    case defang(style: BracketStyle)
    case refang
    case cleanUrls

    /// The `"op"` tag string for this variant — the snake_case discriminant.
    public var opTag: String {
        switch self {
        case .stripHtml: return "strip_html"
        case .stripMarkdown: return "strip_markdown"
        case .collapseWhitespace: return "collapse_whitespace"
        case .trimTrailingWhitespace: return "trim_trailing_whitespace"
        case .removeBlankLines: return "remove_blank_lines"
        case .unwrapLines: return "unwrap_lines"
        case .changeCase: return "change_case"
        case .sortLines: return "sort_lines"
        case .dedupeLines: return "dedupe_lines"
        case .prefixLines: return "prefix_lines"
        case .suffixLines: return "suffix_lines"
        case .joinWith: return "join_with"
        case .splitOn: return "split_on"
        case .extractEmails: return "extract_emails"
        case .extractUrls: return "extract_urls"
        case .defang: return "defang"
        case .refang: return "refang"
        case .cleanUrls: return "clean_urls"
        }
    }

    /// A *reduction* (DESIGN.md D12) replaces the buffer with a derived subset rather
    /// than rewriting it in place. Reductions don't compose and must never run in
    /// continuous mode — the shell surfaces them as one-shot commands. Everything
    /// else is a *rewrite* (safe as a persistent, always-on toggle).
    public var isReduction: Bool {
        switch self {
        case .extractEmails, .extractUrls: return true
        default: return false
        }
    }
}

extension Operation: Codable {
    private enum CodingKeys: String, CodingKey {
        case op
        case `case`
        case descending
        case caseInsensitive = "case_insensitive"
        case prefix
        case suffix
        case separator
        case delimiter
        case style
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(opTag, forKey: .op)
        switch self {
        case .stripHtml, .stripMarkdown, .collapseWhitespace,
             .trimTrailingWhitespace, .removeBlankLines, .unwrapLines,
             .dedupeLines, .extractEmails, .extractUrls, .refang, .cleanUrls:
            // No payload — the `op` tag is the whole object.
            break
        case .defang(let style):
            // Serde derives `#[serde(default)]` on `style` but always emits it on
            // serialize; we match that for a stable, explicit wire form.
            try c.encode(style, forKey: .style)
        case .changeCase(let caseKind):
            try c.encode(caseKind, forKey: .case)
        case .sortLines(let descending, let caseInsensitive):
            // Serde derives `#[serde(default)]` on both, but always emits them
            // on serialize; we match that for a stable, explicit wire form.
            try c.encode(descending, forKey: .descending)
            try c.encode(caseInsensitive, forKey: .caseInsensitive)
        case .prefixLines(let prefix):
            try c.encode(prefix, forKey: .prefix)
        case .suffixLines(let suffix):
            try c.encode(suffix, forKey: .suffix)
        case .joinWith(let separator):
            try c.encode(separator, forKey: .separator)
        case .splitOn(let delimiter):
            try c.encode(delimiter, forKey: .delimiter)
        }
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let op = try c.decode(String.self, forKey: .op)
        switch op {
        case "strip_html": self = .stripHtml
        case "strip_markdown": self = .stripMarkdown
        case "collapse_whitespace": self = .collapseWhitespace
        case "trim_trailing_whitespace": self = .trimTrailingWhitespace
        case "remove_blank_lines": self = .removeBlankLines
        case "unwrap_lines": self = .unwrapLines
        case "change_case":
            self = .changeCase(case: try c.decode(CaseKind.self, forKey: .case))
        case "sort_lines":
            let desc = try c.decodeIfPresent(Bool.self, forKey: .descending) ?? false
            let ci = try c.decodeIfPresent(Bool.self, forKey: .caseInsensitive) ?? false
            self = .sortLines(descending: desc, caseInsensitive: ci)
        case "dedupe_lines": self = .dedupeLines
        case "prefix_lines":
            self = .prefixLines(prefix: try c.decode(String.self, forKey: .prefix))
        case "suffix_lines":
            self = .suffixLines(suffix: try c.decode(String.self, forKey: .suffix))
        case "join_with":
            self = .joinWith(separator: try c.decode(String.self, forKey: .separator))
        case "split_on":
            self = .splitOn(delimiter: try c.decode(String.self, forKey: .delimiter))
        case "extract_emails": self = .extractEmails
        case "extract_urls": self = .extractUrls
        case "defang":
            // `style` is optional on the wire (serde default = square).
            let style = try c.decodeIfPresent(BracketStyle.self, forKey: .style) ?? .square
            self = .defang(style: style)
        case "refang": self = .refang
        case "clean_urls": self = .cleanUrls
        default:
            throw DecodingError.dataCorruptedError(
                forKey: .op, in: c,
                debugDescription: "unknown operation \"\(op)\"")
        }
    }
}

/// A transformation request: a schema version plus an ordered pipeline.
/// Mirrors the Rust `Config` struct; `version` must equal ``schemaVersion``.
public struct TransformConfig: Codable, Equatable, Sendable {
    /// The config schema version the core understands. Kept in sync with
    /// `core/src/config.rs::CONFIG_VERSION`.
    public static let schemaVersion: UInt32 = 1

    public var version: UInt32
    public var operations: [Operation]

    public init(version: UInt32 = TransformConfig.schemaVersion,
                operations: [Operation] = []) {
        self.version = version
        self.operations = operations
    }

    /// Encode to a JSON string the core's `parse_config` accepts.
    ///
    /// Key *order* is not significant: the core decodes with serde, which is
    /// order-independent (it only rejects unknown fields). `JSONEncoder` does
    /// not guarantee a particular key order, so tests compare the decoded
    /// structure rather than the raw string. Slashes are left unescaped so URL
    /// and email separators round-trip cleanly.
    public func encodedJSON() throws -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.withoutEscapingSlashes]
        let data = try encoder.encode(self)
        guard let s = String(data: data, encoding: .utf8) else {
            throw TransformError.encodingFailed
        }
        return s
    }
}
