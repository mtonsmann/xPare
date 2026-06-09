import Foundation

/// Abstraction over the paste-as-file store so the controller is testable with a
/// failing writer. The real implementation is ``PasteFileStore``.
public protocol PasteFileWriting: AnyObject {
    /// Persist `text` as the single current paste file, replacing any previous
    /// one. Returns the file URL to put on the pasteboard, or `nil` on failure
    /// (caller degrades to the normal in-place plain write).
    func write(_ text: String) -> URL?

    /// Best-effort removal of the store's directory and everything in it.
    func removeAll()
}

/// The **only** place in SafetyStrip that may persist clipboard-derived content,
/// and only for the opt-in "paste large clipboards as a file" feature
/// (`Settings.pasteLargeAsFile`, off by default).
///
/// This is a documented exception to the "no persistence of content" posture —
/// see SECURITY.md ("Opt-in paste-as-file exception") and
/// `docs/guardrails/privacy-and-data-handling.md`. The
/// `safetystrip:allow-content-persistence` marker below is recognized by
/// `cargo xtask check-no-content-logging` *only in this file*; anywhere else
/// it is itself a violation.
///
/// Mitigations, in order of importance:
/// - the file lives in a dedicated `PasteAsFile.noindex` directory inside the
///   app sandbox container's `temporaryDirectory` (no entitlement needed;
///   `.noindex` keeps Spotlight from indexing it; excluded from backups);
/// - directory `0700`, file `0600` — owner-only;
/// - **at most one file exists at a time**: every write first removes the
///   previous one;
/// - lifetime is minimized by the controller: the file is removed as soon as
///   the pasteboard stops referencing it, on launch, and on deactivation.
public final class PasteFileStore: PasteFileWriting {
    private let directory: URL
    /// Distinguishes successive paste files within one second (timestamps in the
    /// name have 1 s resolution) so a replacing write never reuses a URL that a
    /// receiving app may have already resolved.
    private var sequence = 0

    /// `directory` is injectable for tests; the default is the sandbox
    /// container's own temp space.
    public init(directory: URL? = nil) {
        self.directory =
            directory
            ?? FileManager.default.temporaryDirectory
            .appendingPathComponent("PasteAsFile.noindex", isDirectory: true)
    }

    public func write(_ text: String) -> URL? {
        // Replace, never accumulate: the previous paste file (if any) goes first.
        removeAll()

        let fm = FileManager.default
        sequence += 1
        var url = directory.appendingPathComponent(Self.fileName(sequence: sequence))
        do {
            try fm.createDirectory(
                at: directory,
                withIntermediateDirectories: true,
                attributes: [.posixPermissions: 0o700])
            // The sanctioned persistence point (see the type doc): the transformed
            // clipboard result becomes the single paste file, owner-only. A write
            // that fails midway is cleaned up by the catch below.
            let transformed = Data(text.utf8)
            try transformed.write(to: url)  // safetystrip:allow-content-persistence
            try fm.setAttributes([.posixPermissions: 0o600], ofItemAtPath: url.path)
            // Transient by design — keep Time Machine and friends away from it.
            var values = URLResourceValues()
            values.isExcludedFromBackup = true
            try? url.setResourceValues(values)
            return url
        } catch {
            // Never leave a partial file behind, and never surface the error with
            // content attached — the caller degrades to a plain pasteboard write.
            removeAll()
            return nil
        }
    }

    public func removeAll() {
        try? FileManager.default.removeItem(at: directory)
    }

    /// `Clipboard <local timestamp> (<n>).txt` — operational metadata only,
    /// **never** derived from the content.
    private static func fileName(sequence: Int) -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd 'at' HH.mm.ss"
        let stamp = formatter.string(from: Date())
        return "Clipboard \(stamp) (\(sequence)).txt"
    }
}
