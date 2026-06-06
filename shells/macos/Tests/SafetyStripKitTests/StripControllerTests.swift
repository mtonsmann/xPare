import Testing
import Foundation
import AppKit
@testable import SafetyStripKit
@testable import SafetyStripCore

/// Controller behavior, end-to-end through the real linked core. `@MainActor`
/// (controller is main-actor) and `.serialized` because one test pumps the run
/// loop.
@Suite(.serialized) @MainActor
struct StripControllerTests {

    private func isolatedDefaults() throws -> (UserDefaults, String) {
        let suite = "StripControllerTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        return (defaults, suite)
    }

    /// End-to-end through the real core: HTML on the pasteboard is stripped and
    /// the plain result is written back in place.
    @Test func stripsHtmlInPlace() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard(snapshot:
            PasteboardSnapshot(text: "<p>hi  there</p>", kind: .html))
        let controller = StripController(
            settings: Settings(mode: .onDemand,
                               operations: [.stripHtml, .collapseWhitespace]),
            pasteboard: pb,
            defaults: defaults
        )

        let outcome = await controller.stripNow(trigger: .manual)
        #expect(outcome == .stripped(changed: true))
        #expect(pb.writes == ["hi there"])
    }

    /// HTML source forces strip_html even if the user didn't list it, because
    /// the shell contract reads public.html and hands it to the core stripper.
    @Test func htmlSourceForcesStripHtml() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard(snapshot:
            PasteboardSnapshot(text: "<b>Bold</b> text", kind: .html))
        // User has NO strip_html configured — only whitespace collapse.
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.collapseWhitespace]),
            pasteboard: pb,
            defaults: defaults
        )

        let config = controller.effectiveConfig(for:
            PasteboardSnapshot(text: "<b>Bold</b> text", kind: .html))
        #expect(config.operations.first == .stripHtml,
                "HTML source must be run through strip_html first")

        let outcome = await controller.stripNow(trigger: .manual)
        #expect(outcome == .stripped(changed: true))
        #expect(pb.writes == ["Bold text"])
    }

    /// If the user already listed strip_html later in the pipeline, HTML input
    /// still runs it first and does not duplicate it.
    @Test func htmlSourceMovesExistingStripHtmlToFrontWithoutDuplicate() throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let controller = StripController(
            settings: Settings(
                mode: .onDemand,
                operations: [.stripMarkdown, .stripHtml, .collapseWhitespace, .stripHtml]
            ),
            pasteboard: FakePasteboard(),
            defaults: defaults
        )

        let config = controller.effectiveConfig(for:
            PasteboardSnapshot(text: "<p>**Bold**</p>", kind: .html))
        #expect(config.operations == [.stripHtml, .stripMarkdown, .collapseWhitespace])
    }

    /// Mechanical guard for the HTML-before-Markdown invariant across likely UI
    /// operation lists: HTML input always starts with exactly one strip_html.
    @Test func htmlSourceAlwaysStartsWithSingleStripHtml() throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let operationLists: [[SafetyStripCore.Operation]] = [
            [],
            [.stripMarkdown],
            [.stripHtml],
            [.stripMarkdown, .stripHtml, .collapseWhitespace, .stripHtml],
            [.extractUrls, .stripMarkdown, .stripHtml],
        ]

        for operations in operationLists {
            let controller = StripController(
                settings: Settings(mode: .onDemand, operations: operations),
                pasteboard: FakePasteboard(),
                defaults: defaults
            )

            let config = controller.effectiveConfig(for:
                PasteboardSnapshot(text: "<p>**Bold**</p>", kind: .html))
            #expect(config.operations.first == .stripHtml)
            #expect(config.operations.filter { $0 == .stripHtml }.count == 1)
        }
    }

    /// A plain string already equal to the stripped result is not rewritten, so
    /// we don't churn the change count.
    @Test func plainUnchangedIsNotRewritten() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard(snapshot:
            PasteboardSnapshot(text: "already plain", kind: .plain))
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.collapseWhitespace]),
            pasteboard: pb,
            defaults: defaults
        )

        let outcome = await controller.stripNow(trigger: .manual)
        #expect(outcome == .stripped(changed: false))
        #expect(pb.writes.isEmpty, "unchanged plain text must not be rewritten")
    }

    @Test func emptyPasteboardYieldsEmptyOutcome() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard(snapshot: nil)
        let controller = StripController(
            settings: Settings(),
            pasteboard: pb,
            defaults: defaults
        )
        #expect(await controller.stripNow() == .empty)
        #expect(pb.writes.isEmpty)
    }

    /// deactivate() must tear the monitor down: after it, no timer from this
    /// controller remains live and nothing writes.
    @Test func deactivateTearsDownMonitor() throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard()
        let controller = StripController(
            settings: Settings(mode: .continuous, operations: [.collapseWhitespace],
                               hotkey: .defaultCombo, pollIntervalMs: 50),
            pasteboard: pb,
            defaults: defaults
        )
        controller.activate()
        controller.deactivate()
        // Drive the run loop briefly; nothing should fire / write.
        let deadline = Date().addingTimeInterval(0.15)
        while Date() < deadline {
            RunLoop.current.run(mode: .common, before: Date().addingTimeInterval(0.02))
        }
        #expect(pb.writes.isEmpty)
    }

    /// A clipboard larger than the controller's ceiling is refused gracefully: no
    /// transform is attempted and the clipboard is left untouched (safety-first —
    /// we never risk an out-of-memory abort on a huge paste).
    @Test func oversizedClipboardIsRefusedAndLeftUntouched() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let text = String(repeating: "x", count: 1000) // 1000 bytes
        let pb = FakePasteboard(snapshot: PasteboardSnapshot(text: text, kind: .plain))
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.collapseWhitespace]),
            pasteboard: pb,
            defaults: defaults,
            maxInputBytes: 16 // far below the 1000-byte clipboard
        )

        let outcome = await controller.stripNow(trigger: .manual)
        #expect(outcome == .tooLarge(bytes: 1000))
        #expect(pb.writes.isEmpty, "oversized clipboard must be left untouched")
    }

    /// Oversized rich data is refused from its raw representation size before
    /// the shell materializes/decodes it.
    @Test func oversizedRichRepresentationIsRefusedBeforeMaterialization() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let transformer = RecordingTransformer(output: "should not run")
        let pb = FakePasteboard(
            snapshot: PasteboardSnapshot(text: "<p>oversized</p>", kind: .html),
            rawRepresentationBytes: 1_000
        )
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.stripHtml]),
            pasteboard: pb,
            transformer: transformer,
            defaults: defaults,
            maxInputBytes: 16
        )

        let outcome = await controller.stripNow(trigger: .manual)
        #expect(outcome == .tooLarge(bytes: 1_000))
        #expect(pb.materializedReadCount == 0,
                "oversized rich representations must be rejected before decode")
        #expect(transformer.callCount == 0,
                "oversized rich representations must not reach the transformer")
        #expect(pb.writes.isEmpty)
    }

    /// Named pasteboard smoke for the real SystemPasteboard path. This does not
    /// touch NSPasteboard.general, but verifies raw rich data is refused before
    /// decode/AppKit materialization or plain fallback.
    @Test func systemPasteboardRejectsOversizedHtmlRepresentationBeforeFallback() throws {
        let name = NSPasteboard.Name("SafetyStripTests.\(UUID().uuidString)")
        let rawPasteboard = NSPasteboard(name: name)
        rawPasteboard.clearContents()
        defer { rawPasteboard.clearContents() }

        let htmlData = Data(repeating: 0x41, count: 128)
        #expect(rawPasteboard.setData(htmlData, forType: .html))
        #expect(rawPasteboard.setString("plain fallback", forType: .string))

        let pasteboard = SystemPasteboard(pasteboard: rawPasteboard)
        let result = pasteboard.readBest(maxRepresentationBytes: 16)
        guard case .tooLarge(let bytes, _) = result else {
            Issue.record("expected oversized HTML to be refused, got \(result)")
            return
        }
        #expect(bytes == 128)
    }

    /// The default ceiling is sane: positive, comfortably above a real clipboard,
    /// and never above the core's hard backstop (`SS_MAX_INPUT_BYTES`).
    @Test func defaultCeilingIsSaneAndClampedToCoreBackstop() {
        let ceiling = StripController.defaultMaxInputBytes()
        #expect(ceiling > 1_000_000, "ceiling should comfortably fit real clipboards")
        #expect(ceiling <= Transformer.coreMaxInputBytes, "must not exceed the core's hard cap")
    }

    /// The transform runs OFF the main thread, so a long run can't freeze the
    /// menu-bar UI. Proven directly: the injected transformer records the thread it
    /// ran on, which must not be the main thread.
    @Test func transformRunsOffTheMainThread() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let slow = SlowTransformer(delay: 0.02)
        let pb = FakePasteboard(snapshot: PasteboardSnapshot(text: "<p>x</p>", kind: .html))
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.stripHtml]),
            pasteboard: pb,
            transformer: slow,
            defaults: defaults
        )

        let outcome = await controller.stripNow(trigger: .manual)
        #expect(outcome == .stripped(changed: true))
        #expect(slow.ranOnMainThread == false, "the transform must run off the main thread")
    }

    /// A run that outlasts the threshold shows the busy indicator, then clears it.
    @Test func busyIndicatorFiresForSlowRuns() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let slow = SlowTransformer(delay: 0.15)
        let pb = FakePasteboard(snapshot: PasteboardSnapshot(text: "<p>x</p>", kind: .html))
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.stripHtml]),
            pasteboard: pb,
            transformer: slow,
            defaults: defaults,
            busyThreshold: .milliseconds(20)
        )
        var events: [Bool] = []
        controller.onStrippingChange = { events.append($0) }

        _ = await controller.stripNow(trigger: .manual)
        #expect(events == [true, false], "a slow run shows then clears the busy indicator")
    }

    /// A fast run finishes well before the threshold, so the indicator never flips —
    /// no flicker for the common case.
    @Test func busyIndicatorStaysSilentForFastRuns() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        // Real (instant) core transformer + a high threshold.
        let pb = FakePasteboard(snapshot: PasteboardSnapshot(text: "<p>x</p>", kind: .html))
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.stripHtml]),
            pasteboard: pb,
            defaults: defaults,
            busyThreshold: .seconds(10)
        )
        var events: [Bool] = []
        controller.onStrippingChange = { events.append($0) }

        let outcome = await controller.stripNow(trigger: .manual)
        #expect(outcome == .stripped(changed: true))
        #expect(events.isEmpty, "a fast run must not flip the busy indicator")
    }

    /// If clipboard contents change while a transform is running, the stale
    /// completion is dropped instead of overwriting the newer clipboard.
    @Test func staleTransformDoesNotOverwriteNewerClipboard() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let blocking = BlockingTransformer(output: "old stripped")
        let pb = FakePasteboard(snapshot:
            PasteboardSnapshot(text: "<p>old</p>", kind: .html))
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.stripHtml]),
            pasteboard: pb,
            transformer: blocking,
            defaults: defaults
        )

        let task = Task { @MainActor in
            await controller.stripNow(trigger: .manual)
        }
        let deadline = Date().addingTimeInterval(1.0)
        while !blocking.hasStarted, Date() < deadline {
            await Task.yield()
            try await Task.sleep(for: .milliseconds(1))
        }
        #expect(blocking.hasStarted, "test transformer should have started")
        if !blocking.hasStarted {
            blocking.release()
        }

        let newer = PasteboardSnapshot(text: "new clipboard", kind: .plain)
        pb.externalSet(newer)
        blocking.release()

        let outcome = await task.value
        #expect(outcome == .stripped(changed: false))
        #expect(pb.snapshot == newer)
        #expect(pb.writes.isEmpty,
                "stale transform output must not overwrite newer clipboard data")
    }

    /// The shared runOnce path must get the same stale-generation protection as
    /// stripNow; transient commands must not overwrite newer clipboard data.
    @Test func staleRunOnceDoesNotOverwriteNewerClipboard() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let blocking = BlockingTransformer(output: "old urls")
        let pb = FakePasteboard(snapshot:
            PasteboardSnapshot(text: "https://old.example", kind: .plain))
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.stripHtml]),
            pasteboard: pb,
            transformer: blocking,
            defaults: defaults
        )

        let task = Task { @MainActor in
            await controller.runOnce(operations: [.extractUrls])
        }
        let deadline = Date().addingTimeInterval(1.0)
        while !blocking.hasStarted, Date() < deadline {
            await Task.yield()
            try await Task.sleep(for: .milliseconds(1))
        }
        #expect(blocking.hasStarted, "test transformer should have started")
        if !blocking.hasStarted {
            blocking.release()
        }

        let newer = PasteboardSnapshot(text: "new clipboard", kind: .plain)
        pb.externalSet(newer)
        blocking.release()

        let outcome = await task.value
        #expect(outcome == .stripped(changed: false))
        #expect(pb.snapshot == newer)
        #expect(pb.writes.isEmpty,
                "stale runOnce output must not overwrite newer clipboard data")
    }

    /// A SafetyStrip self-write in continuous mode is recognized by generation
    /// and not reprocessed when the monitor reports that same change.
    @Test func continuousSelfWriteGenerationIsSuppressed() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let transformer = RecordingTransformer(output: "clean")
        let pb = FakePasteboard(snapshot:
            PasteboardSnapshot(text: "<p>dirty</p>", kind: .html))
        let controller = StripController(
            settings: Settings(mode: .continuous, operations: [.stripHtml]),
            pasteboard: pb,
            transformer: transformer,
            defaults: defaults
        )

        let first = await controller.stripNow(trigger: .manual)
        #expect(first == .stripped(changed: true))
        #expect(transformer.callCount == 1)
        #expect(pb.readBestCalls == 1)

        let second = await controller.stripNow(trigger: .clipboardChanged)
        #expect(second == .stripped(changed: false))
        #expect(transformer.callCount == 1,
                "self-triggered continuous writes must not be transformed again")
        #expect(pb.readBestCalls == 1,
                "self-triggered continuous writes must be suppressed before read")
        #expect(pb.writes == ["clean"])
    }

    /// The monitor callback path suppresses SafetyStrip's own write before it
    /// reads or transforms that same generation again.
    @Test func continuousMonitorSuppressesSelfWriteBeforeRead() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let transformer = RecordingTransformer(output: "clean")
        let pb = FakePasteboard(snapshot:
            PasteboardSnapshot(text: "<p>dirty</p>", kind: .html))
        let controller = StripController(
            settings: Settings(
                mode: .continuous,
                operations: [.stripHtml],
                pollIntervalMs: 10
            ),
            pasteboard: pb,
            transformer: transformer,
            defaults: defaults
        )
        controller.activate()
        defer { controller.deactivate() }

        let first = await controller.stripNow(trigger: .manual)
        #expect(first == .stripped(changed: true))
        #expect(transformer.callCount == 1)
        #expect(pb.readBestCalls == 1)

        try await Task.sleep(for: .milliseconds(150))

        #expect(transformer.callCount == 1,
                "monitor-observed self-writes must not be transformed again")
        #expect(pb.readBestCalls == 1,
                "monitor-observed self-writes must be suppressed before read")
    }

    /// A one-shot command (`runOnce`) transforms the clipboard but must NOT mutate
    /// the persisted pipeline — reductions/refang are commands, not toggles (D12).
    @Test func runOnceDoesNotPersistToSettings() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard(snapshot:
            PasteboardSnapshot(text: "see https://a.com/x and y", kind: .plain))
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.stripHtml]),
            pasteboard: pb,
            defaults: defaults
        )

        let outcome = await controller.runOnce(operations: [.extractUrls])
        #expect(outcome == .stripped(changed: true))
        #expect(pb.writes == ["https://a.com/x"])

        // The transient command must not mutate the persisted pipeline — `runOnce`
        // never writes settings, so the configured pipeline is untouched.
        #expect(controller.settings.operations == [.stripHtml])
    }

    /// Transient command configs also force HTML neutralization first and dedupe
    /// user-provided strip_html before reductions or Markdown stripping run.
    @Test func runOnceHtmlSourceMovesStripHtmlToFrontWithoutDuplicate() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let transformer = RecordingTransformer(output: "plain")
        let pb = FakePasteboard(snapshot:
            PasteboardSnapshot(text: "<p>**https://a.example**</p>", kind: .html))
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.collapseWhitespace]),
            pasteboard: pb,
            transformer: transformer,
            defaults: defaults
        )

        let outcome = await controller.runOnce(
            operations: [.stripMarkdown, .stripHtml, .extractUrls, .stripHtml]
        )
        #expect(outcome == .stripped(changed: true))
        #expect(transformer.configs == [
            TransformConfig(operations: [.stripHtml, .stripMarkdown, .extractUrls])
        ])
    }

    /// HTML-to-Markdown is the one transient command that must consume the raw HTML
    /// representation; injecting strip_html first would destroy the structure it is
    /// meant to preserve.
    @Test func runOnceHtmlToMarkdownUsesRawHtmlWithoutStripHtml() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let transformer = RecordingTransformer(output: "# Title")
        let pb = FakePasteboard(snapshot:
            PasteboardSnapshot(text: "<h1>Title</h1>", kind: .html))
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.stripHtml]),
            pasteboard: pb,
            transformer: transformer,
            defaults: defaults
        )

        let outcome = await controller.runOnce(operations: [.htmlToMarkdown])
        #expect(outcome == .stripped(changed: true))
        #expect(pb.writes == ["# Title"])
        #expect(transformer.configs == [
            TransformConfig(operations: [.htmlToMarkdown])
        ])
        #expect(controller.settings.operations == [.stripHtml])
    }

    /// The conversion command is honest about representation: without HTML on the
    /// clipboard it does not flatten RTF/plain content as a side effect.
    @Test func runOnceHtmlToMarkdownIsNotApplicableWithoutHtml() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let transformer = RecordingTransformer(output: "should not run")
        let pb = FakePasteboard(snapshot:
            PasteboardSnapshot(text: "plain text", kind: .plain))
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: []),
            pasteboard: pb,
            transformer: transformer,
            defaults: defaults
        )

        let outcome = await controller.runOnce(operations: [.htmlToMarkdown])
        #expect(outcome == .notApplicable)
        #expect(transformer.callCount == 0)
        #expect(pb.writes.isEmpty)
    }

    /// Continuous mode must refuse to run a reduction even if one is in the pipeline
    /// (D12): it would silently replace every copied buffer with a derived subset. An
    /// on-demand trigger still runs it.
    @Test func continuousModeSkipsReductions() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard(snapshot:
            PasteboardSnapshot(text: "see https://a.com/x and y", kind: .plain))
        let controller = StripController(
            settings: Settings(mode: .continuous, operations: [.extractUrls]),
            pasteboard: pb,
            defaults: defaults
        )

        // Clipboard-changed trigger → reduction filtered out → buffer left intact.
        let auto = await controller.stripNow(trigger: .clipboardChanged)
        #expect(auto == .stripped(changed: false))
        #expect(pb.writes.isEmpty)

        // Manual trigger → the reduction runs (a deliberate, user-driven action).
        let manual = await controller.stripNow(trigger: .manual)
        #expect(manual == .stripped(changed: true))
        #expect(pb.writes == ["https://a.com/x"])
    }
}

private final class RecordingTransformer: Transforming, @unchecked Sendable {
    private let lock = NSLock()
    private let output: String
    private var _callCount = 0
    private var _configs: [TransformConfig] = []

    init(output: String) {
        self.output = output
    }

    var callCount: Int {
        lock.lock()
        defer { lock.unlock() }
        return _callCount
    }

    var configs: [TransformConfig] {
        lock.lock()
        defer { lock.unlock() }
        return _configs
    }

    func transform(_ input: String, config: TransformConfig) throws -> String {
        lock.lock()
        _callCount += 1
        _configs.append(config)
        lock.unlock()
        return output
    }
}

private final class BlockingTransformer: Transforming, @unchecked Sendable {
    private let lock = NSLock()
    private let proceed = DispatchSemaphore(value: 0)
    private let output: String
    private var _hasStarted = false

    init(output: String) {
        self.output = output
    }

    var hasStarted: Bool {
        lock.lock()
        defer { lock.unlock() }
        return _hasStarted
    }

    func release() {
        proceed.signal()
    }

    func transform(_ input: String, config: TransformConfig) throws -> String {
        lock.lock()
        _hasStarted = true
        lock.unlock()
        proceed.wait()
        return output
    }
}
