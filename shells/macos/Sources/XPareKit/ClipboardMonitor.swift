import Foundation
import AppKit

/// Polls the pasteboard's `changeCount` on a `Timer` and fires a callback when
/// it changes — the basis for continuous mode.
///
/// **Teardown is a hard requirement:** ``stop()`` invalidates *and* nils the
/// timer, so once stopped there is no live timer and no further polling. When
/// the monitor is not running, it owns no scheduled work at all. This is
/// exercised directly by the tests.
///
/// The monitor is `@MainActor`-bound because `Timer` schedules on the current
/// run loop and AppKit's pasteboard is main-thread affine.
@MainActor
public final class ClipboardMonitor {
    /// Default poll cadence, per the shell contract.
    public static let defaultIntervalMs = 500

    private let pasteboard: PasteboardProtocol
    /// Invoked on the main run loop each time `changeCount` advances.
    private let onChange: () -> Void

    private var timer: Timer?
    private var lastChangeCount: Int

    /// Whether a live timer currently exists. Tests assert this is `false`
    /// after ``stop()``.
    public var isRunning: Bool { timer != nil }

    public init(pasteboard: PasteboardProtocol, onChange: @escaping () -> Void) {
        self.pasteboard = pasteboard
        self.onChange = onChange
        self.lastChangeCount = pasteboard.changeCount
    }

    /// Start polling every `intervalMs` milliseconds. Idempotent: an existing
    /// timer is torn down first so we never leak a second one. We baseline the
    /// change count at start so we don't fire for content already on the
    /// pasteboard.
    public func start(intervalMs: Int = ClipboardMonitor.defaultIntervalMs) {
        stop()
        lastChangeCount = pasteboard.changeCount
        let interval = max(0.01, Double(intervalMs) / 1000.0)
        let timer = Timer.scheduledTimer(
            withTimeInterval: interval,
            repeats: true
        ) { [weak self] _ in
            // Hop to the main actor; `self` may be gone if we were torn down.
            MainActor.assumeIsolated {
                self?.poll()
            }
        }
        // Keep firing while menus/modal panels are open.
        RunLoop.current.add(timer, forMode: .common)
        self.timer = timer
    }

    /// Stop polling: invalidate the timer AND drop the reference so no timer or
    /// loop survives. Safe to call when already stopped.
    public func stop() {
        timer?.invalidate()
        timer = nil
    }

    /// Poll once: if the pasteboard changed since we last looked, record the new
    /// count and notify. Exposed for tests to drive deterministically without
    /// waiting on wall-clock time.
    public func poll() {
        let current = pasteboard.changeCount
        guard current != lastChangeCount else { return }
        lastChangeCount = current
        onChange()
    }

    // No `deinit` cleanup: the contract is that callers invoke `stop()`, which
    // invalidates and nils the timer. The scheduled `Timer` is owned by the run
    // loop, not by this object, and its callback captures `self` weakly — so if
    // a monitor is dropped without `stop()`, the weak `self?` is already nil and
    // `poll()` is skipped. We deliberately avoid touching the main-actor-isolated
    // `timer` from a non-isolated `deinit`.
}
