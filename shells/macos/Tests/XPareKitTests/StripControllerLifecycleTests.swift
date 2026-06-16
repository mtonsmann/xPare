import Testing
import Foundation
import AppKit
@testable import XPareKit
@testable import XPareCore

/// Controller lifecycle and side effects: activation, settings updates, hotkey
/// registration state, write failures, and the concealed-content skip. Split from
/// `StripControllerTests` to keep each suite under the lint body-length ceiling.
/// `@MainActor` (controller is main-actor) and `.serialized` to match its sibling.
@Suite(.serialized) @MainActor
struct StripControllerLifecycleTests {

    private func isolatedDefaults() throws -> (UserDefaults, String) {
        let suite = "StripControllerLifecycleTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        return (defaults, suite)
    }

    /// `activate()` in the default on-demand mode installs the hotkey but starts no
    /// monitor (the hard "no timer when continuous is off" rule). The registered
    /// hotkey is torn down by `deactivate()`.
    @Test func activateInOnDemandModeRegistersHotkeyButRunsNoMonitor() throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard()
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.collapseWhitespace]),
            pasteboard: pb,
            defaults: defaults
        )
        controller.activate()
        // Pump the run loop: an on-demand controller must never poll/write on its own.
        let deadline = Date().addingTimeInterval(0.1)
        while Date() < deadline {
            RunLoop.current.run(mode: .common, before: Date().addingTimeInterval(0.02))
        }
        #expect(pb.writes.isEmpty, "on-demand mode must not write without a trigger")
        controller.deactivate()
    }

    /// `update()` persists the new settings and re-applies only the side effects whose
    /// inputs changed: editing the operations list must NOT thrash the monitor/hotkey,
    /// but the new pipeline is saved and used.
    @Test func updateOperationsPersistsWithoutDisturbingMode() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard(snapshot: PasteboardSnapshot(text: "  hi  ", kind: .plain))
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.collapseWhitespace]),
            pasteboard: pb,
            defaults: defaults
        )
        controller.activate()
        defer { controller.deactivate() }

        var next = controller.settings
        next.operations = [.trimTrailingWhitespace, .collapseWhitespace]
        controller.update(next)

        #expect(controller.settings.operations == [.trimTrailingWhitespace, .collapseWhitespace])
        // Persisted: a fresh load sees the updated pipeline.
        #expect(Settings.load(from: defaults).operations == next.operations)
    }

    /// Switching the mode via `update()` re-applies the monitor: on→continuous starts
    /// it (it strips on a clipboard change) and continuous→on-demand tears it down.
    @Test func updateModeStartsThenStopsTheMonitor() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard(snapshot: PasteboardSnapshot(text: "<p>x</p>", kind: .plain))
        let controller = StripController(
            settings: Settings(
                mode: .onDemand, operations: [.collapseWhitespace], pollIntervalMs: 30),
            pasteboard: pb,
            defaults: defaults
        )
        controller.activate()
        defer { controller.deactivate() }

        // Turn continuous ON: an external clipboard change should now be picked up and
        // stripped. The monitor's timer fires on the main run loop, which advances while we
        // await (mirrors `continuousMonitorSuppressesSelfWriteBeforeRead`).
        var on = controller.settings
        on.mode = .continuous
        controller.update(on)
        pb.externalSet(PasteboardSnapshot(text: "  spaced  out  ", kind: .plain))

        var sawWrite = false
        for _ in 0..<50 where !sawWrite {
            try await Task.sleep(for: .milliseconds(20))
            sawWrite = !pb.writes.isEmpty
        }
        #expect(sawWrite, "continuous monitor should strip an external clipboard change")

        // Turn continuous OFF: the monitor stops; no further writes after a new change.
        var off = controller.settings
        off.mode = .onDemand
        controller.update(off)
        let writesAfterStop = pb.writes.count
        pb.externalSet(PasteboardSnapshot(text: "  more  spaces  ", kind: .plain))
        try await Task.sleep(for: .milliseconds(150))
        #expect(pb.writes.count == writesAfterStop, "no writes should land after on-demand")
    }

    /// Changing only the hotkey re-installs it (the combo-changed branch of `update()`)
    /// without flipping modes; the controller stays usable on demand.
    @Test func updateHotkeyReinstallsAndStillStripsOnDemand() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard(snapshot: PasteboardSnapshot(text: "  trim me  ", kind: .plain))
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.trimTrailingWhitespace]),
            pasteboard: pb,
            defaults: defaults
        )
        controller.activate()
        defer { controller.deactivate() }

        var next = controller.settings
        next.hotkey = HotkeyCombo(keyCode: 11, modifiers: 0x0100)  // ⌘B
        controller.update(next)
        #expect(controller.settings.hotkey == next.hotkey)

        let outcome = await controller.stripNow(trigger: .manual)
        #expect(outcome == .stripped(changed: true))
    }

    /// A transform that throws surfaces the content-free `.failed` outcome and leaves the
    /// clipboard untouched — the failure category is reported, never the input.
    @Test func transformFailureSurfacesFailedAndDoesNotWrite() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard(snapshot: PasteboardSnapshot(text: "anything", kind: .plain))
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.collapseWhitespace]),
            pasteboard: pb,
            transformer: ThrowingTransformer(),
            defaults: defaults
        )

        let outcome = await controller.stripNow(trigger: .manual)
        #expect(outcome == .failed)
        #expect(pb.writes.isEmpty)
    }

    /// A pasteboard write the system rejects must surface `.writeFailed` and
    /// must NOT be recorded as a self-write: the cleared generation is someone
    /// else's to process, so a later continuous-mode change report for it is
    /// still read instead of being suppressed as our own.
    @Test func failedPlainWriteSurfacesWriteFailedAndRecordsNoSelfWrite() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard(
            snapshot:
                PasteboardSnapshot(text: "<p>hi</p>", kind: .html))
        pb.failNextPlainWrite = true
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.stripHtml]),
            pasteboard: pb,
            defaults: defaults
        )

        let outcome = await controller.stripNow(trigger: .manual)
        #expect(outcome == .writeFailed)
        #expect(pb.writes.isEmpty, "the failed write must not appear as a success")

        // The post-clear generation must not be suppressed as a self-write: a
        // continuous-mode report for it still attempts a read (and finds the
        // pasteboard empty, because the clear destroyed the old contents).
        let readsBefore = pb.readBestCalls
        let followUp = await controller.stripNow(trigger: .clipboardChanged)
        #expect(followUp == .empty)
        #expect(
            pb.readBestCalls == readsBefore + 1,
            "a failed write must not register a self-write generation")
    }

    /// nspasteboard.org convention: continuous mode must leave content marked
    /// concealed/transient/auto-generated untouched — without even reading it.
    @Test func continuousTriggerSkipsDoNotProcessMarkedContent() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let transformer = RecordingTransformer(output: "should not run")
        let pb = FakePasteboard(
            snapshot:
                PasteboardSnapshot(text: "hunter2  ", kind: .plain))
        pb.hasDoNotProcessMarker = true
        let controller = StripController(
            settings: Settings(mode: .continuous, operations: [.collapseWhitespace]),
            pasteboard: pb,
            transformer: transformer,
            defaults: defaults
        )

        let outcome = await controller.stripNow(trigger: .clipboardChanged)
        #expect(outcome == .skippedConcealed)
        #expect(pb.readBestCalls == 0, "marked content must be skipped before any read")
        #expect(transformer.callCount == 0)
        #expect(pb.writes.isEmpty)
    }

    /// The marker can appear while a read is in progress; continuous mode must
    /// re-check before handing materialized text to the core.
    @Test func continuousTriggerSkipsDoNotProcessMarkerThatAppearsDuringRead() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let transformer = RecordingTransformer(output: "should not run")
        let pb = FakePasteboard(
            snapshot: PasteboardSnapshot(text: "old text", kind: .plain))
        pb.afterReadBest = {
            pb.externalSet(PasteboardSnapshot(text: "new secret", kind: .plain))
            pb.hasDoNotProcessMarker = true
        }
        let controller = StripController(
            settings: Settings(mode: .continuous, operations: [.collapseWhitespace]),
            pasteboard: pb,
            transformer: transformer,
            defaults: defaults
        )

        let outcome = await controller.stripNow(trigger: .clipboardChanged)
        #expect(outcome == .skippedConcealed)
        #expect(pb.readBestCalls == 1)
        #expect(transformer.callCount == 0)
        #expect(pb.writes.isEmpty)
    }

    /// The marker only gates *automatic* processing: a manual trigger is a
    /// deliberate user action and still strips marked content.
    @Test func manualTriggerStillProcessesMarkedContent() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard(
            snapshot:
                PasteboardSnapshot(text: "spaced  out", kind: .plain))
        pb.hasDoNotProcessMarker = true
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.collapseWhitespace]),
            pasteboard: pb,
            defaults: defaults
        )

        let outcome = await controller.stripNow(trigger: .manual)
        #expect(outcome == .stripped(changed: true))
        #expect(pb.writes == ["spaced out"])
    }

    /// The hotkey registration result is surfaced as observable state plus a
    /// callback (the success path is what's drivable headlessly; the failure
    /// path shares the same single `setHotkeyActive` funnel).
    @Test func activateSurfacesHotkeyRegistrationState() throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.stripHtml]),
            pasteboard: FakePasteboard(),
            defaults: defaults
        )
        var reported: [Bool] = []
        controller.onHotkeyStateChange = { reported.append($0) }

        #expect(controller.isHotkeyActive == false, "inactive before activate()")
        controller.activate()
        #expect(controller.isHotkeyActive == true)
        #expect(reported == [true])

        controller.deactivate()
        #expect(controller.isHotkeyActive == false, "deactivate() reports the hotkey gone")
        #expect(reported == [true, false])
    }
}

/// Always throws, to drive the controller's content-free `.failed` path.
private struct ThrowingTransformer: Transforming {
    func transform(_ input: String, config: TransformConfig) throws -> String {
        throw TransformError.internalError
    }
}
