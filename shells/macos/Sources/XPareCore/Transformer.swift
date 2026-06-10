import Foundation
import CXPare

/// Errors surfaced by the Swift wrapper around the C ABI. Each ``status`` case
/// maps 1:1 to a non-OK `XpStatus` from the core; ``encodingFailed`` /
/// ``decodingFailed`` cover the Swift-side marshalling.
public enum TransformError: Error, Equatable, CustomStringConvertible {
    /// A required pointer argument was null (`XP_STATUS_ERR_NULL_ARG`).
    case nullArgument
    /// The config JSON was rejected by the core (`XP_STATUS_ERR_INVALID_CONFIG`).
    case invalidConfig
    /// An unexpected internal error / caught panic (`XP_STATUS_ERR_INTERNAL`).
    case internalError
    /// Input exceeded the core's hard size ceiling
    /// (`XP_STATUS_ERR_INPUT_TOO_LARGE`, ABI v2).
    case inputTooLarge
    /// The config declared a schema version this core does not support
    /// (`XP_STATUS_ERR_UNSUPPORTED_CONFIG_VERSION`, ABI v3) — version skew between
    /// shell and core, distinct from a malformed config.
    case unsupportedConfigVersion
    /// The core returned a status not covered by the frozen ABI.
    case unknownStatus(UInt32)
    /// `xp_transform` reported OK but handed back a null output buffer.
    case missingOutputBuffer
    /// Could not encode the config to UTF-8 JSON.
    case encodingFailed
    /// Output bytes were not valid UTF-8 (should not happen — the core emits UTF-8).
    case decodingFailed

    public var description: String {
        switch self {
        case .nullArgument: return "core rejected a null argument"
        case .invalidConfig: return "core rejected the config JSON"
        case .internalError: return "core hit an internal error"
        case .inputTooLarge: return "input exceeds the core's maximum size"
        case .unsupportedConfigVersion:
            return "settings format is newer than this core supports — update the app"
        case .unknownStatus(let raw): return "core returned unknown status \(raw)"
        case .missingOutputBuffer: return "core returned OK but no output buffer"
        case .encodingFailed: return "failed to encode config as UTF-8 JSON"
        case .decodingFailed: return "core output was not valid UTF-8"
        }
    }

    /// Translate a raw `XpStatus` into a thrown error, or `nil` for OK.
    static func from(status: XpStatus) -> TransformError? {
        switch status {
        case XP_STATUS_OK: return nil
        case XP_STATUS_ERR_NULL_ARG: return .nullArgument
        case XP_STATUS_ERR_INVALID_CONFIG: return .invalidConfig
        case XP_STATUS_ERR_INTERNAL: return .internalError
        case XP_STATUS_ERR_INPUT_TOO_LARGE: return .inputTooLarge
        case XP_STATUS_ERR_UNSUPPORTED_CONFIG_VERSION: return .unsupportedConfigVersion
        default: return .unknownStatus(status.rawValue)
        }
    }
}

/// Safe, memory-correct Swift facade over the xPare C ABI.
///
/// Responsibilities:
/// * encode a ``TransformConfig`` to JSON,
/// * call `xp_transform`,
/// * build a Swift `String` from the returned `(ptr, len)` UTF-8 buffer,
/// * `xp_buffer_free` that buffer **exactly once**, even on error paths,
/// * map any non-OK `XpStatus` to a thrown ``TransformError``.
///
/// The wrapper holds no state and performs no I/O, so it is safe to share.
public struct Transformer: Sendable {
    public init() {}

    /// The C ABI version this binary was linked against (`xp_abi_version`).
    public func abiVersion() -> UInt32 {
        xp_abi_version()
    }

    /// The core's hard input ceiling in bytes (`XP_MAX_INPUT_BYTES`). Exposed so the
    /// shell can clamp its own RAM-proportional limit to the core's backstop without
    /// importing the C module itself.
    public static var coreMaxInputBytes: Int { Int(XP_MAX_INPUT_BYTES) }

    /// The core's self-describing capabilities JSON (`xp_capabilities_json`).
    /// The returned pointer is process-static and must not be freed, so we copy
    /// it into a Swift `String`.
    public func capabilities() -> String {
        guard let ptr = xp_capabilities_json() else { return "" }
        return String(cString: ptr)
    }

    /// Transform `input` under `config`, returning the plain-text result.
    ///
    /// - Throws: ``TransformError`` on a non-OK status or a marshalling failure.
    public func transform(_ input: String, config: TransformConfig) throws -> String {
        let configJSON = try config.encodedJSON()
        return try transform(input, configJSON: configJSON)
    }

    /// Lower-level entry point taking pre-encoded config JSON. Useful for tests
    /// that pin the exact wire string.
    public func transform(_ input: String, configJSON: String) throws -> String {
        let inputBytes = Array(input.utf8)

        var outPtr: UnsafeMutablePointer<UInt8>?
        var outLen = 0

        let status: XpStatus = configJSON.withCString { configCStr in
            inputBytes.withUnsafeBufferPointer { inBuf in
                // `inBuf.baseAddress` is nil for an empty array, which the ABI
                // explicitly allows when the length is 0.
                xp_transform(
                    inBuf.baseAddress,
                    inBuf.count,
                    configCStr,
                    &outPtr,
                    &outLen
                )
            }
        }

        if let err = TransformError.from(status: status) {
            // On any error the ABI guarantees `*out` is null, so there is
            // nothing to free.
            throw err
        }

        // OK path: we now own `outPtr`/`outLen` and MUST free it once. Free in a
        // defer so we release the buffer even if String construction throws.
        guard let base = outPtr else {
            throw TransformError.missingOutputBuffer
        }
        defer { xp_buffer_free(base, outLen) }

        let buffer = UnsafeBufferPointer(start: base, count: outLen)
        guard let result = String(bytes: buffer, encoding: .utf8) else {
            throw TransformError.decodingFailed
        }
        return result
    }
}

/// Abstraction over [`Transformer`] so the shell can run the transform from a
/// background task (and tests can inject a stub). `Sendable` because an
/// implementation is invoked off the main actor.
public protocol Transforming: Sendable {
    /// Transform `input` under `config`, returning the plain-text result.
    func transform(_ input: String, config: TransformConfig) throws -> String
}

extension Transformer: Transforming {}
