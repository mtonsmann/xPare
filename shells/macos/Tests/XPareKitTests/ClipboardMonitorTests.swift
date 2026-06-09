import Testing
import Foundation
@testable import XPareKit

/// Monitor lifecycle tests. `@MainActor` because ClipboardMonitor is main-actor
/// bound and these drive the run loop; `.serialized` so the run-loop pumping in
/// one test doesn't interleave with another.
@Suite(.serialized) @MainActor
struct ClipboardMonitorTests {

    /// The hard requirement: start() creates a timer; stop() invalidates AND
    /// nils it, so no timer survives and no further polls happen.
    @Test func startThenStopLeavesNoTimerAndStopsPolling() {
        let pb = FakePasteboard()
        var fireCount = 0
        let monitor = ClipboardMonitor(pasteboard: pb) { fireCount += 1 }

        #expect(!monitor.isRunning, "fresh monitor must not be running")

        monitor.start(intervalMs: 10)
        #expect(monitor.isRunning, "start() must create a live timer")

        monitor.stop()
        #expect(!monitor.isRunning, "stop() must nil the timer")

        // After stop, even an external change must not resurrect scheduled work.
        // Pump the run loop: a still-scheduled 10ms timer would fire here.
        pb.externalSet(PasteboardSnapshot(text: "new", kind: .plain))
        let firesAfterStop = fireCount
        let deadline = Date().addingTimeInterval(0.15)
        while Date() < deadline {
            RunLoop.current.run(mode: .common, before: Date().addingTimeInterval(0.02))
        }
        #expect(fireCount == firesAfterStop, "no timer should fire after stop()")
    }

    /// poll() fires the callback exactly once per distinct change-count advance.
    @Test func pollFiresOnChangeOnly() {
        let pb = FakePasteboard()
        var fireCount = 0
        let monitor = ClipboardMonitor(pasteboard: pb) { fireCount += 1 }
        monitor.start(intervalMs: 1000)  // long interval; we drive poll() by hand
        defer { monitor.stop() }

        // No change yet.
        monitor.poll()
        #expect(fireCount == 0)

        // One external change -> one fire.
        pb.externalSet(PasteboardSnapshot(text: "a", kind: .plain))
        monitor.poll()
        #expect(fireCount == 1)

        // Polling again with no further change -> no new fire.
        monitor.poll()
        #expect(fireCount == 1)

        // Another change -> another fire.
        pb.externalSet(PasteboardSnapshot(text: "b", kind: .plain))
        monitor.poll()
        #expect(fireCount == 2)
    }

    /// start() is idempotent: calling it twice doesn't leave two live timers
    /// (one stop() fully tears down).
    @Test func restartDoesNotLeakTimers() {
        let pb = FakePasteboard()
        let monitor = ClipboardMonitor(pasteboard: pb) {}
        monitor.start(intervalMs: 50)
        monitor.start(intervalMs: 50)  // restarts; previous timer invalidated
        #expect(monitor.isRunning)
        monitor.stop()
        #expect(!monitor.isRunning)
    }
}
