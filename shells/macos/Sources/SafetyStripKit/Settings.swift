import Foundation
import SafetyStripCore

/// How the shell decides *when* to strip the clipboard.
public enum StripMode: String, Codable, Sendable, CaseIterable {
    /// Strip only when the user asks (hotkey or menu action). The default —
    /// continuous monitoring is opt-in.
    case onDemand
    /// Strip automatically whenever the clipboard changes. Opt-in; off by default.
    case continuous
}

/// A modifier+keycode description for the global hotkey. Stored as raw Carbon
/// values so it round-trips through `UserDefaults` without importing Carbon
/// into the settings model.
public struct HotkeyCombo: Codable, Equatable, Sendable {
    /// Carbon virtual key code (e.g. `kVK_ANSI_V` == 9).
    public var keyCode: UInt32
    /// Carbon modifier mask (`cmdKey | optionKey | …`).
    public var modifiers: UInt32

    public init(keyCode: UInt32, modifiers: UInt32) {
        self.keyCode = keyCode
        self.modifiers = modifiers
    }

    /// Default hotkey: ⌥⌘V. The numeric constants match Carbon's
    /// `kVK_ANSI_V` (9), `cmdKey` (0x0100), and `optionKey` (0x0800); we hardcode
    /// them here so this type stays Carbon-free, and `HotkeyManager` re-derives
    /// the same values from the Carbon headers.
    public static let defaultCombo = HotkeyCombo(
        keyCode: 9,
        modifiers: 0x0100 | 0x0800
    )
}

/// User-facing, persisted configuration. **Never** contains clipboard content —
/// only the user's preferences. Persisted via ``load(from:)`` / ``save(to:)``.
public struct Settings: Codable, Equatable, Sendable {
    /// On-demand by default; continuous monitoring is explicitly opt-in.
    public var mode: StripMode
    /// The ordered pipeline of operations to apply, in execution order.
    /// Fully qualified to disambiguate from Foundation's `Operation` (NSOperation).
    public var operations: [SafetyStripCore.Operation]
    /// The global hotkey used in on-demand mode.
    public var hotkey: HotkeyCombo
    /// Poll interval (milliseconds) for continuous mode's change detection.
    public var pollIntervalMs: Int
    /// How `operations` is ordered before running. `canonical` (default) lets the
    /// core arrange the pipeline correctly/efficiently; `asGiven` is the "Manual
    /// order" mode where the user's drag-arranged order is honored exactly.
    public var ordering: Ordering
    /// Whether continuous mode may OCR image-only clipboard contents. Off by
    /// default; the manual OCR command remains available regardless of this setting.
    public var ocrImagesInContinuousMode: Bool

    public init(
        mode: StripMode = .onDemand,
        operations: [SafetyStripCore.Operation] = Settings.defaultOperations,
        hotkey: HotkeyCombo = .defaultCombo,
        pollIntervalMs: Int = 500,
        ordering: Ordering = .canonical,
        ocrImagesInContinuousMode: Bool = false
    ) {
        self.mode = mode
        self.operations = operations
        self.hotkey = hotkey
        self.pollIntervalMs = pollIntervalMs
        self.ordering = ordering
        self.ocrImagesInContinuousMode = ocrImagesInContinuousMode
    }

    private enum CodingKeys: String, CodingKey {
        case mode, operations, hotkey, pollIntervalMs, ordering, ocrImagesInContinuousMode
    }

    /// Decode tolerantly so a settings blob saved by an older build (missing newer
    /// fields like `ordering`) upgrades to defaults rather than failing to load.
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        mode = try c.decodeIfPresent(StripMode.self, forKey: .mode) ?? .onDemand
        operations =
            try c.decodeIfPresent([SafetyStripCore.Operation].self, forKey: .operations)
            ?? Settings.defaultOperations
        hotkey = try c.decodeIfPresent(HotkeyCombo.self, forKey: .hotkey) ?? .defaultCombo
        pollIntervalMs = try c.decodeIfPresent(Int.self, forKey: .pollIntervalMs) ?? 500
        ordering = try c.decodeIfPresent(Ordering.self, forKey: .ordering) ?? .canonical
        ocrImagesInContinuousMode =
            try c.decodeIfPresent(Bool.self, forKey: .ocrImagesInContinuousMode) ?? false
    }

    /// A sensible starting pipeline: coerce rich text to plain (HTML strip is
    /// applied to the HTML representation during pasteboard read) and tidy
    /// whitespace. Order is significant.
    public static let defaultOperations: [SafetyStripCore.Operation] = [
        .stripHtml,
        .collapseWhitespace,
        .trimTrailingWhitespace,
    ]

    /// Build the ``TransformConfig`` to hand the core from the current settings.
    public func transformConfig() -> TransformConfig {
        TransformConfig(operations: operations, ordering: ordering)
    }
}

// MARK: - Persistence

extension Settings {
    /// The single `UserDefaults` key under which settings JSON is stored.
    public static let defaultsKey = "com.safetystrip.settings"

    /// Load settings from `UserDefaults`, falling back to defaults if absent or
    /// corrupt. Never throws — a bad stored blob degrades to defaults.
    public static func load(from defaults: UserDefaults = .standard) -> Settings {
        guard let data = defaults.data(forKey: defaultsKey) else {
            return Settings()
        }
        do {
            return try JSONDecoder().decode(Settings.self, from: data)
        } catch {
            return Settings()
        }
    }

    /// Persist settings to `UserDefaults` as JSON.
    public func save(to defaults: UserDefaults = .standard) {
        guard let data = try? JSONEncoder().encode(self) else { return }
        defaults.set(data, forKey: Settings.defaultsKey)
    }
}
