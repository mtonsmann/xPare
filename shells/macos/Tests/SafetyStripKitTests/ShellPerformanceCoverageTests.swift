import Testing
import Foundation
import AppKit
@testable import SafetyStripKit
@testable import SafetyStripCore

/// CI-safe performance guards for Swift-owned shell features. These tests use
/// synthetic data and injected fakes only: no `NSPasteboard.general`, no Vision
/// OCR, and no clipboard content from outside the process.
@Suite(.serialized) @MainActor
struct ShellPerformanceCoverageTests {
    private func isolatedDefaults() throws -> (UserDefaults, String) {
        let suite = "ShellPerformanceCoverageTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        return (defaults, suite)
    }

    @Test func settingsPersistenceAndConfigAssemblyStayBounded() throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let settings = Settings(
            mode: .continuous,
            operations: representativeOperations,
            hotkey: HotkeyCombo(keyCode: 9, modifiers: 0x0100 | 0x0800),
            pollIntervalMs: 250,
            ordering: .asGiven
        )

        let iterations = 500
        let elapsedMs = try measureMilliseconds {
            for _ in 0..<iterations {
                settings.save(to: defaults)
                let loaded = Settings.load(from: defaults)
                _ = loaded.transformConfig()
                _ = try loaded.transformConfig().encodedJSON()
            }
        }

        #expect(elapsedMs < 2_000,
                "settings persistence/config assembly took \(elapsedMs) ms for \(iterations) runs")
    }

    @Test func stripNowShellOrchestrationStaysBoundedWithFastTransformer() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let transformer = CountingTransformer()
        let pb = FakePasteboard()
        let controller = StripController(
            settings: Settings(
                mode: .onDemand,
                operations: [.stripHtml, .collapseWhitespace, .trimTrailingWhitespace]
            ),
            pasteboard: pb,
            transformer: transformer,
            defaults: defaults,
            busyThreshold: .seconds(60)
        )

        let iterations = 150
        let elapsedMs = await measureMilliseconds {
            for i in 0..<iterations {
                pb.externalSet(PasteboardSnapshot(text: "<p>dirty \(i)</p>", kind: .html))
                let outcome = await controller.stripNow(trigger: .manual)
                #expect(outcome == .stripped(changed: true))
            }
        }

        #expect(transformer.callCount == iterations)
        #expect(pb.writes.count == iterations)
        #expect(elapsedMs < 2_000,
                "stripNow shell orchestration took \(elapsedMs) ms for \(iterations) runs")
    }

    @Test func runOnceCommandOrchestrationStaysBoundedWithFastTransformer() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let transformer = CountingTransformer()
        let pb = FakePasteboard()
        let controller = StripController(
            settings: Settings(mode: .onDemand, operations: [.collapseWhitespace]),
            pasteboard: pb,
            transformer: transformer,
            defaults: defaults,
            busyThreshold: .seconds(60)
        )

        let iterations = 150
        let elapsedMs = await measureMilliseconds {
            for i in 0..<iterations {
                pb.externalSet(PasteboardSnapshot(
                    text: "<p>https://example.com/\(i)</p>",
                    kind: .html
                ))
                let outcome = await controller.runOnce(
                    operations: [.stripMarkdown, .extractUrls]
                )
                #expect(outcome == .stripped(changed: true))
            }
        }

        #expect(transformer.callCount == iterations)
        #expect(transformer.configs.allSatisfy {
            $0.operations == [.stripHtml, .stripMarkdown, .extractUrls]
        })
        #expect(elapsedMs < 2_000,
                "runOnce shell orchestration took \(elapsedMs) ms for \(iterations) runs")
    }

    @Test func monitorPollingAndHotkeyDispatchStayBounded() {
        let pb = FakePasteboard()
        var monitorFireCount = 0
        let monitor = ClipboardMonitor(pasteboard: pb) { monitorFireCount += 1 }

        let pollIterations = 20_000
        let pollElapsedMs = measureMilliseconds {
            for i in 0..<pollIterations {
                if i % 4 == 0 {
                    pb.externalSet(PasteboardSnapshot(text: "value \(i)", kind: .plain))
                }
                monitor.poll()
            }
        }
        #expect(monitorFireCount == pollIterations / 4)
        #expect(pollElapsedMs < 1_000,
                "monitor polling took \(pollElapsedMs) ms for \(pollIterations) polls")

        var hotkeyFireCount = 0
        var ids: [UInt32] = []
        for _ in 0..<1_000 {
            let id = HotkeyDispatch.nextID()
            ids.append(id)
            HotkeyDispatch.shared.register(id: id) { hotkeyFireCount += 1 }
        }
        defer {
            for id in ids {
                HotkeyDispatch.shared.unregister(id: id)
            }
        }

        let dispatchElapsedMs = measureMilliseconds {
            for id in ids {
                HotkeyDispatch.shared.fire(id: id)
            }
        }
        #expect(hotkeyFireCount == ids.count)
        #expect(dispatchElapsedMs < 1_000,
                "hotkey dispatch took \(dispatchElapsedMs) ms for \(ids.count) fires")
    }

    @Test func namedPasteboardRawRepresentationPreflightStaysBounded() throws {
        let name = NSPasteboard.Name("SafetyStripPerformanceTests.\(UUID().uuidString)")
        let rawPasteboard = NSPasteboard(name: name)
        rawPasteboard.clearContents()
        defer { rawPasteboard.clearContents() }

        let htmlData = Data(repeating: 0x41, count: 128 * 1024)
        rawPasteboard.declareTypes([.html, .string], owner: nil)
        guard rawPasteboard.setData(htmlData, forType: .html),
              rawPasteboard.setString("plain fallback", forType: .string) else {
            return // Named pasteboards may be unavailable in headless/sandboxed agents.
        }

        let pasteboard = SystemPasteboard(pasteboard: rawPasteboard)
        let iterations = 250
        let elapsedMs = measureMilliseconds {
            for _ in 0..<iterations {
                let result = pasteboard.readBest(maxRepresentationBytes: 16)
                guard case .tooLarge(let bytes, _) = result else {
                    Issue.record("expected oversized HTML to be refused, got \(result)")
                    return
                }
                #expect(bytes == htmlData.count)
            }
        }

        #expect(elapsedMs < 2_000,
                "named pasteboard preflight took \(elapsedMs) ms for \(iterations) reads")
    }

    @Test func imageTextCommandOverheadStaysBoundedWithFastRecognizer() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let image = performanceSampleImage()
        let recognizer = CountingImageTextRecognizer(output: "text")
        let pb = FakePasteboard()
        let controller = StripController(
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults,
            busyThreshold: .seconds(60)
        )

        let iterations = 100
        let elapsedMs = await measureMilliseconds {
            for _ in 0..<iterations {
                pb.externalSetImage(image, rawImageBytes: image.data.count)
                let outcome = await controller.extractImageText()
                #expect(outcome == .stripped(changed: true))
            }
        }

        #expect(recognizer.callCount == iterations)
        #expect(pb.writes.count == iterations)
        #expect(elapsedMs < 2_000,
                "Swift OCR command orchestration took \(elapsedMs) ms for \(iterations) runs")
    }

    private var representativeOperations: [SafetyStripCore.Operation] {
        [
            .stripHtml,
            .stripMarkdown,
            .htmlToMarkdown,
            .collapseWhitespace,
            .trimTrailingWhitespace,
            .removeBlankLines,
            .unwrapLines,
            .changeCase(case: .sentence),
            .sortLines(descending: true, caseInsensitive: true),
            .dedupeLines,
            .prefixLines(prefix: "> "),
            .suffixLines(suffix: ";"),
            .joinWith(separator: ", "),
            .splitOn(delimiter: "|"),
            .extractEmails,
            .extractUrls,
            .defang(style: .round),
            .refang,
            .cleanUrls,
            .maskIdentifiers(emails: true, ipv4: true, ipv6: true),
        ]
    }
}

@MainActor
private func measureMilliseconds(_ work: () throws -> Void) rethrows -> Double {
    let start = DispatchTime.now().uptimeNanoseconds
    try work()
    return Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000
}

@MainActor
private func measureMilliseconds(_ work: () async throws -> Void) async rethrows -> Double {
    let start = DispatchTime.now().uptimeNanoseconds
    try await work()
    return Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000
}

private final class CountingTransformer: Transforming, @unchecked Sendable {
    private let lock = NSLock()
    private var _callCount = 0
    private var _configs: [TransformConfig] = []

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
        let current = _callCount
        lock.unlock()
        return "clean \(current): \(input.count)"
    }
}

private func performanceSampleImage() -> PasteboardImage {
    PasteboardImage(
        data: Data([0x89, 0x50, 0x4e, 0x47]),
        pasteboardType: NSPasteboard.PasteboardType.png.rawValue
    )
}

private final class CountingImageTextRecognizer: ImageTextRecognizing, @unchecked Sendable {
    private let lock = NSLock()
    private let output: String
    private var _callCount = 0

    init(output: String) {
        self.output = output
    }

    var callCount: Int {
        lock.lock()
        defer { lock.unlock() }
        return _callCount
    }

    func recognizeText(in image: PasteboardImage) throws -> String {
        lock.lock()
        _callCount += 1
        lock.unlock()
        return output
    }
}
