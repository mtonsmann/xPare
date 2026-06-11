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
    /// The requested one-shot command does not apply to the current clipboard
    /// representation. Carries no clipboard content.
    case notApplicable
    /// The clipboard exceeded the shell's safe size ceiling and was left untouched
    /// (no transform attempted). Carries only the byte count, never content.
    case tooLarge(bytes: Int)
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
    private let transformer: any Transforming
    private let imageTextRecognizer: any ImageTextRecognizing
    private let defaults: UserDefaults
    /// Largest clipboard (in UTF-8 bytes) this controller will hand to the core.
    /// See ``defaultMaxInputBytes()``.
    private let maxInputBytes: Int
    /// How long a strip must run before the "Stripping…" indicator is shown, so the
    /// instant common case never flickers. Default 400 ms.
    private let busyThreshold: Duration

    private var monitor: ClipboardMonitor?
    private var hotkey: HotkeyManager?
    private var lastSelfWriteChangeCount: Int?
    private var continuousStripInFlight = false
    private var continuousStripPending = false

    /// Called on the main actor when the controller starts (`true`) or stops
    /// (`false`) showing the threshold-gated "Stripping…" indicator. Set by the UI.
    public var onStrippingChange: ((Bool) -> Void)?

    /// Current settings. Mutating via ``update(_:)`` re-applies side effects
    /// (monitor/hotkey) and persists.
    public private(set) var settings: Settings

    public init(
        settings: Settings? = nil,
        pasteboard: PasteboardProtocol = SystemPasteboard(),
        transformer: any Transforming = Transformer(),
        imageTextRecognizer: any ImageTextRecognizing = VisionTextRecognizer(),
        defaults: UserDefaults = .standard,
        maxInputBytes: Int = StripController.defaultMaxInputBytes(),
        busyThreshold: Duration = .milliseconds(400)
    ) {
        self.pasteboard = pasteboard
        self.transformer = transformer
        self.imageTextRecognizer = imageTextRecognizer
        self.defaults = defaults
        self.maxInputBytes = maxInputBytes
        self.busyThreshold = busyThreshold
        self.settings = settings ?? Settings.load(from: defaults)
    }

    /// Default input ceiling: the smaller of the core's hard backstop
    /// (`SS_MAX_INPUT_BYTES`) and a RAM-proportional bound (~1/10 of physical memory).
    /// A transform's peak working set is several times its input, so this keeps a
    /// worst-case strip well under half of physical RAM, refusing larger clipboards
    /// gracefully rather than risking an out-of-memory abort. It scales with the
    /// machine, mirroring how the OS pasteboard is itself memory-bound.
    public static func defaultMaxInputBytes() -> Int {
        let physical = ProcessInfo.processInfo.physicalMemory
        let ramBound = Int(min(physical / 10, UInt64(Int.max)))
        return min(Transformer.coreMaxInputBytes, ramBound)
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
        continuousStripInFlight = false
        continuousStripPending = false
        hotkey?.unregister()
        hotkey = nil
    }

    /// Replace settings, persist them, and re-apply side effects so a mode or
    /// hotkey change takes effect immediately.
    ///
    /// Only the side effects whose inputs actually changed are re-applied: the
    /// common case (toggling/editing operations — e.g. typing in a Settings text
    /// field) persists the new pipeline but does **not** re-register the global
    /// hotkey or re-evaluate the monitor, which would otherwise thrash on every
    /// keystroke.
    public func update(_ newSettings: Settings) {
        let old = settings
        settings = newSettings
        settings.save(to: defaults)
        if newSettings.hotkey != old.hotkey || hotkey == nil {
            installHotkey()
        }
        if newSettings.mode != old.mode || newSettings.pollIntervalMs != old.pollIntervalMs {
            applyMonitorForCurrentMode()
        }
    }

    // MARK: - The core action

    /// Read the pasteboard, transform per the current **settings** off the main
    /// thread, and write the plain result back in place. The everyday "Strip now" /
    /// hotkey / continuous action. Returns a content-free outcome.
    @discardableResult
    public func stripNow(trigger: StripTrigger = .manual) async -> StripOutcome {
        let outcome = await perform(trigger: trigger) { self.effectiveConfig(for: $0) }
        guard trigger == .clipboardChanged,
              outcome == .empty,
              settings.ocrImagesInContinuousMode else {
            return outcome
        }

        let imageOutcome = await extractImageText(trigger: .clipboardChanged)
        return imageOutcome == .notApplicable ? outcome : imageOutcome
    }

    /// Run a **transient** explicit operation list once against the clipboard, without
    /// touching the persisted settings pipeline. This is how reductions (extract
    /// emails/URLs) and one-shot rewrites (refang) are surfaced — per DESIGN.md D12
    /// they are *commands*, not standing toggles, so they must not be saved into
    /// `settings.operations`. An HTML source is still run through `strip_html` first
    /// so reductions see plain text rather than raw markup. `html_to_markdown` is
    /// the exception: it consumes the raw HTML representation directly.
    @discardableResult
    public func runOnce(
        operations: [SafetyStripCore.Operation],
        trigger: StripTrigger = .manual
    ) async -> StripOutcome {
        await perform(trigger: trigger) { snapshot in
            var ops = operations
            let convertsHtmlToMarkdown = ops.contains(.htmlToMarkdown)
            if convertsHtmlToMarkdown {
                guard snapshot.kind == .html else { return nil }
                ops.removeAll { $0 == .stripHtml }
            } else if snapshot.kind == .html {
                ops.removeAll { $0 == .stripHtml }
                ops.insert(.stripHtml, at: 0)
            }
            return TransformConfig(operations: ops)
        }
    }

    /// Explicit one-shot OCR command: read a bounded image representation from the
    /// pasteboard, recognize text with macOS Vision off the main actor, and write
    /// the recognized plain text back in place. This is not part of the core
    /// pipeline; continuous mode only uses the same path when the user explicitly
    /// enables image OCR for image-only clipboards.
    @discardableResult
    public func extractImageText() async -> StripOutcome {
        await extractImageText(trigger: .manual)
    }

    private func extractImageText(trigger: StripTrigger) async -> StripOutcome {
        if trigger == .clipboardChanged,
           pasteboard.changeCount == lastSelfWriteChangeCount {
            return .stripped(changed: false)
        }

        let read: PasteboardImageRead
        switch pasteboard.readImage(maxRepresentationBytes: maxInputBytes) {
        case .content(let content):
            read = content
        case .empty:
            return .notApplicable
        case .tooLarge(let bytes, _):
            return .tooLarge(bytes: bytes)
        }

        if read.image.data.count > maxInputBytes {
            return .tooLarge(bytes: read.image.data.count)
        }

        let image = read.image
        let recognizer = imageTextRecognizer
        let recognized: String? = await runWithBusyIndicator {
            await Task.detached(priority: .userInitiated) { [image, recognizer] in
                try? recognizer.recognizeText(in: image)
            }.value
        }

        guard let output = recognized,
              !output.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return .notApplicable
        }
        let outputByteCount = output.utf8.count
        if outputByteCount > maxInputBytes {
            return .tooLarge(bytes: outputByteCount)
        }

        guard pasteboard.changeCount == read.changeCount else {
            return .stripped(changed: false)
        }

        lastSelfWriteChangeCount = pasteboard.writePlain(output)
        return .stripped(changed: true)
    }

    /// Shared machinery for ``stripNow`` / ``runOnce``: read the best pasteboard
    /// representation, refuse an oversized clipboard, build the config via
    /// `makeConfig`, run the transform OFF the main actor (with the threshold-gated
    /// "Stripping…" signal), and write the result back only when it actually changed.
    private func perform(
        trigger: StripTrigger,
        makeConfig: (PasteboardSnapshot) -> TransformConfig?
    ) async -> StripOutcome {
        if trigger == .clipboardChanged,
           pasteboard.changeCount == lastSelfWriteChangeCount {
            return .stripped(changed: false)
        }

        let read: PasteboardRead
        switch pasteboard.readBest(maxRepresentationBytes: maxInputBytes) {
        case .content(let content):
            read = content
        case .empty:
            return .empty
        case .tooLarge(let bytes, _):
            return .tooLarge(bytes: bytes)
        }

        // Safety ceiling: refuse an oversized clipboard rather than risk an
        // out-of-memory abort transforming it. The clipboard is left untouched.
        let snapshot = read.snapshot
        let byteCount = snapshot.text.utf8.count
        if byteCount > maxInputBytes {
            return .tooLarge(bytes: byteCount)
        }

        guard var config = makeConfig(snapshot) else {
            return .notApplicable
        }
        // D12 guard: continuous mode must NEVER run a reduction — it would silently
        // replace every copied buffer with a derived subset. Drop any that slipped
        // into the pipeline (e.g. from older persisted settings). On-demand triggers
        // (the hotkey, "Strip now", and the one-shot Extract commands) are unaffected.
        if trigger == .clipboardChanged {
            config.operations.removeAll(where: { $0.isReduction })
        }

        // The transform is the only heavy step and touches no main-affine state, so
        // run it OFF the main actor to keep the menu-bar UI responsive on large inputs.
        let input = snapshot.text
        let transformer = self.transformer
        let transformConfig = config
        let output: String? = await runWithBusyIndicator {
            await Task.detached(priority: .userInitiated) { [input, transformConfig, transformer] in
                try? transformer.transform(input, config: transformConfig)
            }.value
        }

        guard let output else {
            // Only the (content-free) failure category is surfaced — never the input.
            return .failed
        }

        guard pasteboard.changeCount == read.changeCount else {
            return .stripped(changed: false)
        }

        // Only rewrite when the result actually differs from what a plain paste would
        // have produced, to avoid bumping the change count needlessly. For HTML/RTF
        // sources there was no plain string to compare to, so we always write.
        let priorPlain = (snapshot.kind == .plain) ? snapshot.text : nil
        if let priorPlain, priorPlain == output {
            return .stripped(changed: false)
        }
        lastSelfWriteChangeCount = pasteboard.writePlain(output)
        return .stripped(changed: true)
    }

    private func runWithBusyIndicator<T: Sendable>(_ operation: () async -> T) async -> T {
        // Threshold-gated "Stripping…" signal: flip to busy only if the work outlasts
        // `busyThreshold`, so the instant common case never flickers. The task reports
        // whether it actually signaled, so we clear the state iff we set it.
        let threshold = busyThreshold
        let busyTask = Task { @MainActor [weak self] () -> Bool in
            do {
                try await Task.sleep(for: threshold)
            } catch {
                return false // cancelled before the threshold elapsed -> never signaled
            }
            self?.onStrippingChange?(true)
            return true
        }

        let result = await operation()

        busyTask.cancel()
        if await busyTask.value {
            onStrippingChange?(false)
        }
        return result
    }

    /// Build the config to run for a given snapshot. The user's ordered
    /// operations are applied as-is, except that an HTML source is always run
    /// through `strip_html` first (the shell contract prefers `public.html` and
    /// hands it to the core's stripper), even if the user listed it later.
    func effectiveConfig(for snapshot: PasteboardSnapshot) -> TransformConfig {
        var ops: [SafetyStripCore.Operation] = settings.operations
        if snapshot.kind == .html {
            ops.removeAll { $0 == .stripHtml }
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
                // Hotkey is the on-demand trigger; honor it regardless of mode so the
                // user can always force a strip. Spawn a task so the (off-main)
                // transform never blocks the run loop the hotkey fired on.
                Task { @MainActor in _ = await self.stripNow(trigger: .manual) }
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
                    self.handleContinuousClipboardChange()
                }
            }
            monitor?.start(intervalMs: settings.pollIntervalMs)
        case .onDemand:
            // Hard requirement: no timer/loop runs when continuous is off.
            monitor?.stop()
            monitor = nil
        }
    }

    private func handleContinuousClipboardChange() {
        if pasteboard.changeCount == lastSelfWriteChangeCount {
            return
        }

        if continuousStripInFlight {
            continuousStripPending = true
            return
        }

        continuousStripInFlight = true
        Task { @MainActor [weak self] in
            await self?.drainContinuousStrips()
        }
    }

    private func drainContinuousStrips() async {
        while true {
            continuousStripPending = false
            _ = await stripNow(trigger: .clipboardChanged)

            if !continuousStripPending {
                continuousStripInFlight = false
                return
            }
        }
    }
}
