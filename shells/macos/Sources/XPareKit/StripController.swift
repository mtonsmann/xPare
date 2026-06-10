import Foundation
import XPareCore

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
    /// Paste-as-file (opt-in) kicked in: the transformed result exceeded the
    /// user's threshold and the pasteboard now holds a file reference instead
    /// of the raw string. Carries no clipboard content.
    case strippedToFile
    /// Nothing text-like was on the pasteboard.
    case empty
    /// The core rejected the input or config.
    case failed
    /// The requested one-shot command does not apply to the current clipboard
    /// representation. Carries no clipboard content.
    case notApplicable
    /// The transformed result could not be written back: the system rejected
    /// the pasteboard write *after* the in-place rewrite had already cleared
    /// the old contents. Surfaced (instead of being recorded as a successful
    /// self-write) so the user learns the clipboard may now be empty.
    case writeFailed
    /// Continuous mode saw content carrying an nspasteboard.org "do not
    /// process" marker (concealed/transient/auto-generated — password managers
    /// and the like) and left it untouched without even reading it. Manual
    /// triggers never produce this: an explicit user action still runs.
    case skippedConcealed
    /// The clipboard exceeded the shell's safe size ceiling and was left untouched
    /// (no transform attempted). `rich` records WHY for an honest status line:
    /// `true` when a rich representation (raw HTML/RTF bytes, or their extracted
    /// text) blew the ceiling, `false` for an oversized plain string. Carries
    /// only the byte count, never content.
    case tooLarge(bytes: Int, rich: Bool)
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
    private let defaults: UserDefaults
    /// Owns the single transient file behind the opt-in paste-as-file feature —
    /// the sanctioned content-persistence exception (see ``PasteFileStore``).
    private let pasteFileStore: any PasteFileWriting
    /// Largest clipboard (in UTF-8 bytes) this controller will hand to the core.
    /// See ``defaultMaxInputBytes()``.
    private let maxInputBytes: Int
    /// How long a strip must run before the "Stripping…" indicator is shown, so the
    /// instant common case never flickers. Default 400 ms.
    private let busyThreshold: Duration

    private var monitor: ClipboardMonitor?
    private var hotkey: HotkeyManager?
    private var lastSelfWriteChangeCount: Int?
    /// The pasteboard generation of our last file-URL write, while the paste
    /// file still exists. The file is referenced by the pasteboard iff
    /// `pasteboard.changeCount` still equals this; once it differs, the file is
    /// stale and is deleted on the next strip (best-effort lifetime minimization).
    private var pasteFileChangeCount: Int?
    private var continuousStripInFlight = false
    private var continuousStripPending = false

    /// Called on the main actor when the controller starts (`true`) or stops
    /// (`false`) showing the threshold-gated "Stripping…" indicator. Set by the UI.
    public var onStrippingChange: ((Bool) -> Void)?

    /// Called on the main actor each time a hotkey (re)registration attempt
    /// resolves, with the resulting active state. Set by the UI so a failed
    /// Carbon registration is surfaced (e.g. a menu "hotkey inactive" line)
    /// instead of leaving a silently dead hotkey.
    public var onHotkeyStateChange: ((Bool) -> Void)?

    /// Whether the global hotkey is currently registered with the OS. `false`
    /// before ``activate()``, after ``deactivate()``, and after a failed
    /// registration attempt.
    public private(set) var isHotkeyActive = false

    /// Current settings. Mutating via ``update(_:)`` re-applies side effects
    /// (monitor/hotkey) and persists.
    public private(set) var settings: Settings

    public init(
        settings: Settings? = nil,
        pasteboard: PasteboardProtocol = SystemPasteboard(),
        transformer: any Transforming = Transformer(),
        defaults: UserDefaults = .standard,
        maxInputBytes: Int = StripController.defaultMaxInputBytes(),
        busyThreshold: Duration = .milliseconds(400),
        pasteFileStore: any PasteFileWriting = PasteFileStore()
    ) {
        self.pasteboard = pasteboard
        self.transformer = transformer
        self.defaults = defaults
        self.maxInputBytes = maxInputBytes
        self.busyThreshold = busyThreshold
        self.pasteFileStore = pasteFileStore
        self.settings = settings ?? Settings.load(from: defaults)
    }

    /// Default input ceiling: the smaller of the core's hard backstop
    /// (`XP_MAX_INPUT_BYTES`) and a RAM-proportional bound (~1/10 of physical memory).
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
        // Leftover paste files from a previous run are deleted up front, even if
        // the pasteboard might still reference one — when in doubt, the privacy
        // posture wins over a stale paste working.
        pasteFileStore.removeAll()
        pasteFileChangeCount = nil
        installHotkey()
        applyMonitorForCurrentMode()
    }

    /// Tear down all OS integrations (monitor timer + hotkey). After this no
    /// timer or event handler from this controller remains live, and no paste
    /// file remains on disk.
    public func deactivate() {
        monitor?.stop()
        monitor = nil
        continuousStripInFlight = false
        continuousStripPending = false
        hotkey?.unregister()
        hotkey = nil
        setHotkeyActive(false)
        pasteFileStore.removeAll()
        pasteFileChangeCount = nil
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
        await perform(trigger: trigger) { self.effectiveConfig(for: $0) }
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
        operations: [XPareCore.Operation],
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

    /// Shared machinery for ``stripNow`` / ``runOnce``: read the best pasteboard
    /// representation, refuse an oversized clipboard, build the config via
    /// `makeConfig`, run the transform OFF the main actor (with the threshold-gated
    /// "Stripping…" signal), and write the result back only when it actually changed.
    private func perform(
        trigger: StripTrigger,
        makeConfig: (PasteboardSnapshot) -> TransformConfig?
    ) async -> StripOutcome {
        removeStalePasteFile()

        if trigger == .clipboardChanged,
            pasteboard.changeCount == lastSelfWriteChangeCount
        {
            return .stripped(changed: false)
        }

        // nspasteboard.org convention (privacy): password managers and similar
        // tools mark secrets/ephemeral buffers with a "do not process" type.
        // An automatic continuous-mode pass must honor the marker and leave the
        // content untouched — before reading it at all. A manual trigger
        // (hotkey / menu) is a deliberate user action and still runs.
        if trigger == .clipboardChanged, pasteboard.hasDoNotProcessMarker {
            return .skippedConcealed
        }

        let read: PasteboardRead
        switch readWithinCeiling() {
        case .ok(let content):
            read = content
        case .bail(let earlyOutcome):
            return earlyOutcome
        }
        let snapshot = read.snapshot

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

        // Threshold-gated "Stripping…" signal: flip to busy only if the work outlasts
        // `busyThreshold`, so the instant common case never flickers. The task reports
        // whether it actually signaled, so we clear the state iff we set it.
        let threshold = busyThreshold
        let busyTask = Task { @MainActor [weak self] () -> Bool in
            do {
                try await Task.sleep(for: threshold)
            } catch {
                return false  // cancelled before the threshold elapsed → never signaled
            }
            self?.onStrippingChange?(true)
            return true
        }

        // The transform is the only heavy step and touches no main-affine state, so
        // run it OFF the main actor to keep the menu-bar UI responsive on large inputs.
        let input = snapshot.text
        let transformer = self.transformer
        let output: String? = await Task.detached(priority: .userInitiated) {
            try? transformer.transform(input, config: config)
        }.value

        busyTask.cancel()
        if await busyTask.value {
            onStrippingChange?(false)
        }

        guard let output else {
            // Only the (content-free) failure category is surfaced — never the input.
            return .failed
        }

        guard pasteboard.changeCount == read.changeCount else {
            return .stripped(changed: false)
        }

        // Runs before the unchanged-skip below: an already-plain large buffer
        // should still become a file.
        if let fileOutcome = writeAsFileIfLarge(output) {
            return fileOutcome
        }

        // Only rewrite when the result actually differs from what a plain paste would
        // have produced, to avoid bumping the change count needlessly. For HTML/RTF
        // sources there was no plain string to compare to, so we always write.
        let priorPlain = (snapshot.kind == .plain) ? snapshot.text : nil
        if let priorPlain, priorPlain == output {
            return .stripped(changed: false)
        }
        guard let generation = pasteboard.writePlain(output) else {
            // The system rejected the write after the old contents were already
            // cleared. Do NOT record a self-write generation — the clipboard
            // holds the cleared generation, not our output, and suppressing it
            // would hide the very change the user needs to notice. Surface the
            // (content-free) failure instead.
            return .writeFailed
        }
        lastSelfWriteChangeCount = generation
        return .stripped(changed: true)
    }

    /// A ceiling-checked pasteboard read: the content, or the content-free
    /// outcome `perform` should return early.
    private enum CeilingRead {
        case ok(PasteboardRead)
        case bail(StripOutcome)
    }

    /// Read the best pasteboard representation, refusing an oversized clipboard
    /// at both levels — raw representation bytes (inside `readBest`) and the
    /// extracted text (the safety ceiling here: refuse rather than risk an
    /// out-of-memory abort transforming it; the clipboard is left untouched).
    private func readWithinCeiling() -> CeilingRead {
        switch pasteboard.readBest(maxRepresentationBytes: maxInputBytes) {
        case .empty:
            return .bail(.empty)
        case .tooLarge(let bytes, _):
            // `readBest` size-checks only raw *rich* representations (HTML/RTF
            // bytes), so this refusal is always about rich content.
            return .bail(.tooLarge(bytes: bytes, rich: true))
        case .content(let content):
            let byteCount = content.snapshot.text.utf8.count
            if byteCount > maxInputBytes {
                return .bail(
                    .tooLarge(bytes: byteCount, rich: content.snapshot.kind != .plain))
            }
            return .ok(content)
        }
    }

    // MARK: - Paste-as-file (the opt-in persistence exception; see PasteFileStore)

    /// Best-effort paste-file lifetime minimization, run at the top of every
    /// strip: once the pasteboard has moved past our file-URL write, nothing
    /// references the file — delete it.
    private func removeStalePasteFile() {
        guard let fileGeneration = pasteFileChangeCount,
            pasteboard.changeCount != fileGeneration
        else { return }
        pasteFileStore.removeAll()
        pasteFileChangeCount = nil
    }

    /// Opt-in paste-as-file: when enabled and the transformed result exceeds the
    /// user's threshold, persist it via the (sanctioned) ``PasteFileStore`` and
    /// put a file reference on the pasteboard instead of the raw string. Returns
    /// `nil` — including on a failed file write or a failed pasteboard write —
    /// to let the caller degrade to the normal in-place plain write so the strip
    /// result is never lost.
    private func writeAsFileIfLarge(_ output: String) -> StripOutcome? {
        guard settings.pasteLargeAsFile,
            output.utf8.count > settings.pasteAsFileThresholdBytes,
            let fileURL = pasteFileStore.write(output)
        else { return nil }
        guard let generation = pasteboard.writeFileURL(fileURL) else {
            // The system rejected the URL write: nothing references the file
            // just persisted, so delete it (lifetime minimization) and degrade
            // to the plain write path.
            pasteFileStore.removeAll()
            return nil
        }
        lastSelfWriteChangeCount = generation
        pasteFileChangeCount = generation
        return .strippedToFile
    }

    /// Build the config to run for a given snapshot. The user's ordered
    /// operations are applied as-is, except that an HTML source is always run
    /// through `strip_html` first (the shell contract prefers `public.html` and
    /// hands it to the core's stripper), even if the user listed it later.
    func effectiveConfig(for snapshot: PasteboardSnapshot) -> TransformConfig {
        var ops: [XPareCore.Operation] = settings.operations
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
        // Registration can fail (Carbon error, chord taken by another app). The
        // result must reach the UI — a silently dead hotkey looks identical to
        // a working one until the user needs it.
        let registered =
            hotkey?.register(keyCode: combo.keyCode, modifiers: combo.modifiers) ?? false
        setHotkeyActive(registered)
    }

    /// Record the hotkey's registration state and notify the UI. Always fires
    /// the callback (even when unchanged) so a re-registration attempt after a
    /// failure refreshes the surfaced state.
    private func setHotkeyActive(_ active: Bool) {
        isHotkeyActive = active
        onHotkeyStateChange?(active)
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
