import Testing
import Foundation
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
}
