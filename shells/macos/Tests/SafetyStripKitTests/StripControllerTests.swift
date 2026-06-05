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
    @Test func stripsHtmlInPlace() throws {
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

        let outcome = controller.stripNow(trigger: .manual)
        #expect(outcome == .stripped(changed: true))
        #expect(pb.writes == ["hi there"])
    }

    /// HTML source forces strip_html even if the user didn't list it, because
    /// the shell contract reads public.html and hands it to the core stripper.
    @Test func htmlSourceForcesStripHtml() throws {
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

        let outcome = controller.stripNow(trigger: .manual)
        #expect(outcome == .stripped(changed: true))
        #expect(pb.writes == ["Bold text"])
    }

    /// A plain string already equal to the stripped result is not rewritten, so
    /// we don't churn the change count.
    @Test func plainUnchangedIsNotRewritten() throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard(snapshot:
            PasteboardSnapshot(text: "already plain", kind: .plain))
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.collapseWhitespace]),
            pasteboard: pb,
            defaults: defaults
        )

        let outcome = controller.stripNow(trigger: .manual)
        #expect(outcome == .stripped(changed: false))
        #expect(pb.writes.isEmpty, "unchanged plain text must not be rewritten")
    }

    @Test func emptyPasteboardYieldsEmptyOutcome() throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let pb = FakePasteboard(snapshot: nil)
        let controller = StripController(
            settings: Settings(),
            pasteboard: pb,
            defaults: defaults
        )
        #expect(controller.stripNow() == .empty)
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
}
