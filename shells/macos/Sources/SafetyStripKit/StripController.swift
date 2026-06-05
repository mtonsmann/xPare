import Foundation
import SafetyStripCore

/// The reason a strip was triggered, used only for control flow — never logged
/// with any clipboard content attached.
public enum StripTrigger: Equatable, Sendable {
    /// User invoked it (hotkey or "Strip now" menu item).
    case manual
    /// The clipboard changed while in continuous mode.
    case clipboardChanged
}

/// Outcome of a single strip attempt. Carries no clipboard text so it is safe
/// to surface in menus or (non-content) diagnostics.
public enum StripOutcome: Equatable, Sendable {
    /// Pasteboard was read, transformed, and (if changed) rewritten in place.
    case stripped(changed: Bool)
    /// Nothing text-like was on the pasteboard.
    case empty
    /// The core rejected the input or config.
    case failed
}

/// Ties the pieces together: read the pasteboard → build a ``TransformConfig``
/// from ``Settings`` → run the core ``Transformer`` → write the plain result
/// back **in place**.
///
/// Privacy invariant: this type NEVER logs, prints, or otherwise emits clipboard
/// content (no `print` / `NSLog` / `os_log` of the text). Clipboard bytes live
/// only in locals for the duration of a transform.
///
/// `@MainActor` because it owns the monitor/hotkey (both main-actor) and the
/// pasteboard is main-thread affine.
@MainActor
public final class StripController {
    private let pasteboard: PasteboardProtocol
    private let transformer: Transformer
    private let defaults: UserDefaults

    private var monitor: ClipboardMonitor?
    private var hotkey: HotkeyManager?

    /// Current settings. Mutating via ``update(_:)`` re-applies side effects
    /// (monitor/hotkey) and persists.
    public private(set) var settings: Settings

    public init(
        settings: Settings? = nil,
        pasteboard: PasteboardProtocol = SystemPasteboard(),
        transformer: Transformer = Transformer(),
        defaults: UserDefaults = .standard
    ) {
        self.pasteboard = pasteboard
        self.transformer = transformer
        self.defaults = defaults
        self.settings = settings ?? Settings.load(from: defaults)
    }

    // MARK: - Lifecycle

    /// Wire up OS integrations to match the current settings: register the
    /// hotkey and, if in continuous mode, start the monitor. Tear down anything
    /// not needed by the current mode.
    public func activate() {
        installHotkey()
        applyMonitorForCurrentMode()
    }

    /// Tear down all OS integrations (monitor timer + hotkey). After this no
    /// timer or event handler from this controller remains live.
    public func deactivate() {
        monitor?.stop()
        monitor = nil
        hotkey?.unregister()
        hotkey = nil
    }

    /// Replace settings, persist them, and re-apply side effects so a mode or
    /// hotkey change takes effect immediately.
    public func update(_ newSettings: Settings) {
        settings = newSettings
        settings.save(to: defaults)
        installHotkey()
        applyMonitorForCurrentMode()
    }

    // MARK: - The core action

    /// Read the pasteboard, transform per settings, and write the plain text
    /// back in place. Returns an outcome describing what happened (no content).
    @discardableResult
    public func stripNow(trigger: StripTrigger = .manual) -> StripOutcome {
        guard let snapshot = pasteboard.readBest() else {
            return .empty
        }

        let config = effectiveConfig(for: snapshot)
        let output: String
        do {
            output = try transformer.transform(snapshot.text, config: config)
        } catch {
            // Deliberately do NOT include the clipboard text (or the input) in
            // any surfaced error — only the (content-free) error category.
            return .failed
        }

        // Only rewrite when the result actually differs from what a plain paste
        // would have produced, to avoid bumping the change count needlessly.
        // For HTML/RTF sources there was no plain string to compare to, so we
        // always write the stripped plain text.
        let priorPlain = (snapshot.kind == .plain) ? snapshot.text : nil
        if let priorPlain, priorPlain == output {
            return .stripped(changed: false)
        }
        pasteboard.writePlain(output)
        return .stripped(changed: true)
    }

    /// Build the config to run for a given snapshot. The user's ordered
    /// operations are applied as-is, except that an HTML source is always run
    /// through `strip_html` first (the shell contract prefers `public.html` and
    /// hands it to the core's stripper), even if the user did not list it.
    func effectiveConfig(for snapshot: PasteboardSnapshot) -> TransformConfig {
        var ops: [SafetyStripCore.Operation] = settings.operations
        if snapshot.kind == .html, !ops.contains(.stripHtml) {
            ops.insert(.stripHtml, at: 0)
        }
        return TransformConfig(operations: ops)
    }

    // MARK: - Side effects

    private func installHotkey() {
        let combo = settings.hotkey
        if hotkey == nil {
            hotkey = HotkeyManager { [weak self] in
                guard let self else { return }
                // Hotkey is the on-demand trigger; honor it regardless of mode
                // so the user can always force a strip.
                _ = self.stripNow(trigger: .manual)
            }
        }
        hotkey?.register(keyCode: combo.keyCode, modifiers: combo.modifiers)
    }

    private func applyMonitorForCurrentMode() {
        switch settings.mode {
        case .continuous:
            if monitor == nil {
                monitor = ClipboardMonitor(pasteboard: pasteboard) { [weak self] in
                    guard let self else { return }
                    _ = self.stripNow(trigger: .clipboardChanged)
                }
            }
            monitor?.start(intervalMs: settings.pollIntervalMs)
        case .onDemand:
            // Hard requirement: no timer/loop runs when continuous is off.
            monitor?.stop()
            monitor = nil
        }
    }
}
