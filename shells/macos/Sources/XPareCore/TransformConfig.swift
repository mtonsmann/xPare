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
    case htmlToMarkdown
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
    case maskIdentifiers(emails: Bool, ipv4: Bool, ipv6: Bool)

    /// The `"op"` tag string for this variant — the snake_case discriminant.
    public var opTag: String {
        switch self {
        case .stripHtml: return "strip_html"
        case .stripMarkdown: return "strip_markdown"
        case .htmlToMarkdown: return "html_to_markdown"
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
        case .maskIdentifiers: return "mask_identifiers"
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
        case emails
        case ipv4
        case ipv6
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(opTag, forKey: .op)
        switch self {
        case .stripHtml, .stripMarkdown, .htmlToMarkdown, .collapseWhitespace,
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
        case .maskIdentifiers(let emails, let ipv4, let ipv6):
            // Serde defaults absent booleans to false but serializes them explicitly.
            try c.encode(emails, forKey: .emails)
            try c.encode(ipv4, forKey: .ipv4)
            try c.encode(ipv6, forKey: .ipv6)
        }
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let op = try c.decode(String.self, forKey: .op)
        switch op {
        case "strip_html": self = .stripHtml
        case "strip_markdown": self = .stripMarkdown
        case "html_to_markdown": self = .htmlToMarkdown
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
        case "mask_identifiers":
            self = .maskIdentifiers(
                emails: try c.decodeIfPresent(Bool.self, forKey: .emails) ?? false,
                ipv4: try c.decodeIfPresent(Bool.self, forKey: .ipv4) ?? false,
                ipv6: try c.decodeIfPresent(Bool.self, forKey: .ipv6) ?? false
            )
        default:
            throw DecodingError.dataCorruptedError(
                forKey: .op, in: c,
                debugDescription: "unknown operation \"\(op)\"")
        }
    }
}

/// How the core orders the pipeline before running it. Mirrors the Rust `Ordering`
/// enum (`#[serde(rename_all = "snake_case")]`); the raw values are the exact JSON
/// strings the core expects.
public enum Ordering: String, Codable, Sendable, CaseIterable {
    /// Stable-sort into the documented canonical order (the default).
    case canonical
    /// Run operations in exactly the given order.
    case asGiven = "as_given"
}

/// A transformation request: a schema version, an ordered pipeline, and how that
/// pipeline is ordered. Mirrors the Rust `Config` struct; `version` must equal
/// ``schemaVersion``.
public struct TransformConfig: Codable, Equatable, Sendable {
    /// The config schema version the core understands. Kept in sync with
    /// `core/src/config.rs::CONFIG_VERSION`. **v3** tightened the free-text
    /// parameter and whole-pipeline growth envelope.
    public static let schemaVersion: UInt32 = 3

    /// Current schema's UTF-8 byte ceiling for free-text operation parameters.
    public static let maxTextParameterBytes = 16

    /// Current schema's maximum operation count. Mirrors
    /// `core/src/config.rs::MAX_CONFIG_OPERATIONS`.
    public static let maxOperations = 32

    /// Current schema's whole-pipeline growth cap. Mirrors
    /// `core/src/config.rs::MAX_PIPELINE_GROWTH_FACTOR`.
    public static let maxPipelineGrowthFactor: UInt64 = 1 << 12

    public var version: UInt32
    public var operations: [Operation]
    /// Defaults to ``Ordering/canonical`` (matches the core's serde default).
    public var ordering: Ordering

    public init(
        version: UInt32 = TransformConfig.schemaVersion,
        operations: [Operation] = [],
        ordering: Ordering = .canonical
    ) {
        self.version = version
        self.operations = operations
        self.ordering = ordering
    }

    private enum CodingKeys: String, CodingKey {
        case version, operations, ordering
    }

    /// Decode tolerantly: a config that omits `ordering` defaults to `canonical`,
    /// exactly as the core's serde `#[serde(default)]` does. (Encoding is synthesized
    /// and always emits all three fields.)
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        version = try c.decode(UInt32.self, forKey: .version)
        operations = try c.decodeIfPresent([Operation].self, forKey: .operations) ?? []
        ordering = try c.decodeIfPresent(Ordering.self, forKey: .ordering) ?? .canonical
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

    /// Normalize shell-owned free-text parameters before emitting the current
    /// wire schema. Persisted settings from an older build may carry values the
    /// current core deliberately rejects.
    public static func normalizedTextParameter(_ value: String) -> String {
        normalizedTextParameter(value, maxBytes: maxTextParameterBytes)
    }

    /// Return operations that fit the current config schema before they cross
    /// the FFI. This is intentionally a shell-boundary helper, not persistence:
    /// old settings blobs may contain values that are no longer valid, while the
    /// live `config_json` must be accepted by the current core.
    public static func normalizedOperationsForCurrentSchema(_ operations: [Operation])
        -> [Operation]
    {
        var result: [Operation] = []
        var product: UInt64 = 1

        for operation in operations {
            guard result.count < maxOperations else { break }

            let normalized = operation.normalizedForCurrentSchema()
            let remainingFactor = maxPipelineGrowthFactor / product
            guard
                let envelopeOp = normalized.clampedForCurrentSchemaGrowth(
                    maxFactor: remainingFactor)
            else { continue }

            let factor = envelopeOp.currentSchemaGrowthFactor
            guard factor <= maxPipelineGrowthFactor / product else { continue }
            result.append(envelopeOp)
            product *= factor
        }

        return result
    }

    static func currentSchemaGrowthProduct(_ operations: [Operation]) -> UInt64 {
        var product: UInt64 = 1
        for operation in operations {
            let multiplied = product.multipliedReportingOverflow(
                by: operation.currentSchemaGrowthFactor)
            if multiplied.overflow { return UInt64.max }
            product = multiplied.partialValue
        }
        return product
    }

    fileprivate static func normalizedTextParameter(_ value: String, maxBytes: Int) -> String {
        let byteLimit = Swift.max(0, maxBytes)
        let withoutLineBreaks =
            value
            .replacingOccurrences(of: "\r", with: "")
            .replacingOccurrences(of: "\n", with: "")
        guard withoutLineBreaks.utf8.count > byteLimit else {
            return withoutLineBreaks
        }

        var result = ""
        var bytes = 0
        for character in withoutLineBreaks {
            let characterBytes = String(character).utf8.count
            if bytes + characterBytes > byteLimit {
                break
            }
            result.append(character)
            bytes += characterBytes
        }
        return result
    }
}

public extension Operation {
    /// Return an operation whose free-text payload fits the current config schema.
    func normalizedForCurrentSchema() -> Operation {
        switch self {
        case .prefixLines(let prefix):
            return .prefixLines(prefix: TransformConfig.normalizedTextParameter(prefix))
        case .suffixLines(let suffix):
            return .suffixLines(suffix: TransformConfig.normalizedTextParameter(suffix))
        case .joinWith(let separator):
            return .joinWith(separator: TransformConfig.normalizedTextParameter(separator))
        case .splitOn(let delimiter):
            return .splitOn(delimiter: TransformConfig.normalizedTextParameter(delimiter))
        case .stripHtml, .stripMarkdown, .htmlToMarkdown, .collapseWhitespace,
            .trimTrailingWhitespace, .removeBlankLines, .unwrapLines, .changeCase,
            .sortLines, .dedupeLines, .extractEmails, .extractUrls, .defang, .refang,
            .cleanUrls, .maskIdentifiers:
            return self
        }
    }
}

extension Operation {
    var currentSchemaGrowthFactor: UInt64 {
        switch self {
        case .prefixLines(let prefix):
            return 1 + UInt64(prefix.utf8.count)
        case .suffixLines(let suffix):
            return 1 + UInt64(suffix.utf8.count)
        case .joinWith(let separator):
            return Swift.max(UInt64(separator.utf8.count), 1)
        case .htmlToMarkdown:
            return 5
        case .changeCase:
            return 3
        case .defang:
            return 3
        case .maskIdentifiers:
            return 2
        case .stripHtml, .stripMarkdown, .collapseWhitespace, .trimTrailingWhitespace,
            .removeBlankLines, .unwrapLines, .sortLines, .dedupeLines, .splitOn,
            .extractEmails, .extractUrls, .refang, .cleanUrls:
            return 1
        }
    }

    func clampedForCurrentSchemaGrowth(maxFactor: UInt64) -> Operation? {
        let maxFactor = Swift.max(UInt64(1), maxFactor)
        switch self {
        case .prefixLines(let prefix):
            let maxBytes = Int(
                min(UInt64(TransformConfig.maxTextParameterBytes), maxFactor - 1))
            return .prefixLines(
                prefix: TransformConfig.normalizedTextParameter(prefix, maxBytes: maxBytes))
        case .suffixLines(let suffix):
            let maxBytes = Int(
                min(UInt64(TransformConfig.maxTextParameterBytes), maxFactor - 1))
            return .suffixLines(
                suffix: TransformConfig.normalizedTextParameter(suffix, maxBytes: maxBytes))
        case .joinWith(let separator):
            let maxBytes = Int(
                min(UInt64(TransformConfig.maxTextParameterBytes), maxFactor))
            return .joinWith(
                separator: TransformConfig.normalizedTextParameter(separator, maxBytes: maxBytes))
        case .stripHtml, .stripMarkdown, .htmlToMarkdown, .collapseWhitespace,
            .trimTrailingWhitespace, .removeBlankLines, .unwrapLines, .changeCase,
            .sortLines, .dedupeLines, .splitOn, .extractEmails, .extractUrls, .defang,
            .refang, .cleanUrls, .maskIdentifiers:
            return currentSchemaGrowthFactor <= maxFactor ? self : nil
        }
    }
}
