import Foundation
@testable import SafetyStripCore

/// A `Transforming` stub for tests: records the thread it ran on and can block for a
/// fixed delay, so tests can prove the transform runs **off the main thread** and can
/// drive the threshold-gated "Stripping…" indicator deterministically.
///
/// `@unchecked Sendable`: the recorded flag is guarded by an `NSLock` (it is written
/// on the background task's thread and read back on the main actor after the call
/// completes).
final class SlowTransformer: Transforming, @unchecked Sendable {
    let delay: TimeInterval
    private let output: String
    private let lock = NSLock()
    private var _ranOnMainThread = false

    init(delay: TimeInterval, output: String = "stripped") {
        self.delay = delay
        self.output = output
    }

    /// Whether the most recent `transform` ran on the main thread (it must not).
    var ranOnMainThread: Bool {
        lock.lock()
        defer { lock.unlock() }
        return _ranOnMainThread
    }

    func transform(_ input: String, config: TransformConfig) throws -> String {
        lock.lock()
        _ranOnMainThread = Thread.isMainThread
        lock.unlock()
        if delay > 0 {
            Thread.sleep(forTimeInterval: delay)
        }
        return output
    }
}
