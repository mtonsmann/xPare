import Testing
import Foundation
import AppKit
@testable import XPareKit
@testable import XPareCore

/// Image OCR controller behavior, including the explicit command and opt-in
/// continuous mode. Serialized because each test drives one main-actor
/// controller against a mutable fake pasteboard.
@Suite(.serialized) @MainActor
struct StripControllerImageTextTests {

    private func isolatedDefaults() throws -> (UserDefaults, String) {
        let suite = "StripControllerImageTextTests.\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        return (defaults, suite)
    }

    /// Continuous image OCR is separately opt-in; an image-only clipboard is left
    /// untouched unless that setting is enabled.
    @Test func continuousModeDoesNotOCRImagesUnlessEnabled() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let recognizer = RecordingImageTextRecognizer(output: "should not run")
        let pb = FakePasteboard(image: sampleImage(), rawImageBytes: 4)
        let controller = StripController(
            settings: Settings(mode: .continuous),
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults
        )

        let outcome = await controller.stripNow(trigger: .clipboardChanged)
        #expect(outcome == .empty)
        #expect(pb.readBestCalls == 1)
        #expect(pb.readImageCalls == 0)
        #expect(recognizer.callCount == 0)
        #expect(pb.writes.isEmpty)
    }

    /// When the user opts in, continuous mode may OCR image-only clipboards using
    /// the same bounded/off-main path as the explicit command.
    @Test func continuousModeOCRsImageOnlyClipboardWhenEnabled() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let image = sampleImage()
        let recognizer = RecordingImageTextRecognizer(output: "Invoice 42")
        let pb = FakePasteboard(image: image, rawImageBytes: image.data.count)
        let controller = StripController(
            settings: Settings(mode: .continuous, ocrImagesInContinuousMode: true),
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults
        )

        let outcome = await controller.stripNow(trigger: .clipboardChanged)
        #expect(outcome == .stripped(changed: true))
        #expect(pb.readBestCalls == 1)
        #expect(pb.readImageCalls == 1)
        #expect(recognizer.callCount == 1)
        #expect(pb.writes == ["Invoice 42"])
    }

    /// Text-like clipboards keep using the normal core pipeline even if they also
    /// carry an image representation and continuous OCR is enabled.
    @Test func continuousModePrefersTextPipelineOverOCR() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let transformer = RecordingTransformer(output: "clean text")
        let recognizer = RecordingImageTextRecognizer(output: "image text")
        let pb = FakePasteboard(
            snapshot: PasteboardSnapshot(text: "dirty text", kind: .plain),
            image: sampleImage(),
            rawImageBytes: 4
        )
        let controller = StripController(
            settings: Settings(mode: .continuous, ocrImagesInContinuousMode: true),
            pasteboard: pb,
            transformer: transformer,
            imageTextRecognizer: recognizer,
            defaults: defaults
        )

        let outcome = await controller.stripNow(trigger: .clipboardChanged)
        #expect(outcome == .stripped(changed: true))
        #expect(transformer.callCount == 1)
        #expect(pb.readImageCalls == 0)
        #expect(recognizer.callCount == 0)
        #expect(pb.writes == ["clean text"])
    }

    /// Continuous OCR is only a fallback for the exact pasteboard generation that
    /// the text path classified as empty. If another app writes text before OCR
    /// begins, xPare must not read that newer image and overwrite the text.
    @Test func continuousModeOCRSkipsWhenEmptyGenerationChangesBeforeFallback() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let recognizer = RecordingImageTextRecognizer(output: "should not run")
        let pb = FakePasteboard(image: sampleImage(), rawImageBytes: 4)
        pb.afterReadBest = {
            pb.externalSet(
                PasteboardSnapshot(text: "new text", kind: .plain),
                image: sampleImage(),
                rawImageBytes: 4)
        }
        let controller = StripController(
            settings: Settings(mode: .continuous, ocrImagesInContinuousMode: true),
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults
        )

        let outcome = await controller.stripNow(trigger: .clipboardChanged)
        #expect(outcome == .stripped(changed: false))
        #expect(pb.snapshot == PasteboardSnapshot(text: "new text", kind: .plain))
        #expect(pb.readBestCalls == 1)
        #expect(pb.readImageCalls == 0)
        #expect(recognizer.callCount == 0)
        #expect(pb.writes.isEmpty)
    }

    /// The image read itself can race: the generation is captured before the
    /// pasteboard materializes image bytes. Continuous OCR must re-check the
    /// live generation and do-not-process marker before Vision sees those bytes.
    @Test func continuousModeOCRSkipsWhenGenerationChangesDuringImageRead() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let recognizer = RecordingImageTextRecognizer(output: "should not run")
        let pb = FakePasteboard(image: sampleImage(), rawImageBytes: 4)
        pb.afterReadImageGenerationCaptured = {
            pb.externalSetImage(sampleImage(), rawImageBytes: 4)
            pb.hasDoNotProcessMarker = true
        }
        let controller = StripController(
            settings: Settings(mode: .continuous, ocrImagesInContinuousMode: true),
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults
        )

        let outcome = await controller.stripNow(trigger: .clipboardChanged)
        #expect(outcome == .skippedConcealed)
        #expect(pb.readBestCalls == 1)
        #expect(pb.readImageCalls == 1)
        #expect(recognizer.callCount == 0)
        #expect(pb.writes.isEmpty)
    }

    /// Continuous OCR still honors nspasteboard.org do-not-process markers before
    /// reading image bytes.
    @Test func continuousImageOCRSkipsDoNotProcessMarkerBeforeImageRead() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let recognizer = RecordingImageTextRecognizer(output: "should not run")
        let pb = FakePasteboard(image: sampleImage(), rawImageBytes: 4)
        pb.hasDoNotProcessMarker = true
        let controller = StripController(
            settings: Settings(mode: .continuous, ocrImagesInContinuousMode: true),
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults
        )

        let outcome = await controller.stripNow(trigger: .clipboardChanged)
        #expect(outcome == .skippedConcealed)
        #expect(pb.readBestCalls == 0)
        #expect(pb.readImageCalls == 0)
        #expect(recognizer.callCount == 0)
        #expect(pb.writes.isEmpty)
    }

    /// After a continuous OCR self-write, the next observed generation is suppressed
    /// before any read, so xPare does not OCR its own recognized text.
    @Test func continuousImageOCRSelfWriteGenerationIsSuppressed() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let recognizer = RecordingImageTextRecognizer(output: "recognized text")
        let pb = FakePasteboard(image: sampleImage(), rawImageBytes: 4)
        let controller = StripController(
            settings: Settings(mode: .continuous, ocrImagesInContinuousMode: true),
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults
        )

        let first = await controller.stripNow(trigger: .clipboardChanged)
        #expect(first == .stripped(changed: true))
        #expect(recognizer.callCount == 1)

        let second = await controller.stripNow(trigger: .clipboardChanged)
        #expect(second == .stripped(changed: false))
        #expect(pb.readBestCalls == 1)
        #expect(pb.readImageCalls == 1)
        #expect(recognizer.callCount == 1)
        #expect(pb.writes == ["recognized text"])
    }

    /// OCR is a shell command/path: it reads a bounded image representation,
    /// runs the injected recognizer, and writes recognized text back in place.
    @Test func extractImageTextWritesRecognizedText() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let image = sampleImage()
        let recognizer = RecordingImageTextRecognizer(output: "Invoice 42")
        let pb = FakePasteboard(image: image, rawImageBytes: image.data.count)
        let controller = StripController(
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults
        )

        let outcome = await controller.extractImageText()
        #expect(outcome == .stripped(changed: true))
        #expect(pb.writes == ["Invoice 42"])
        #expect(pb.readBestCalls == 0, "image OCR must not run the text pipeline read")
        #expect(recognizer.callCount == 1)
        #expect(recognizer.images == [image])
    }

    /// Without an image representation the OCR command is simply not applicable:
    /// no core transform, no recognizer, and no pasteboard write.
    @Test func extractImageTextIsNotApplicableWithoutImage() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let recognizer = RecordingImageTextRecognizer(output: "should not run")
        let pb = FakePasteboard(snapshot: PasteboardSnapshot(text: "plain", kind: .plain))
        let controller = StripController(
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults
        )

        let outcome = await controller.extractImageText()
        #expect(outcome == .notApplicable)
        #expect(recognizer.callCount == 0)
        #expect(pb.writes.isEmpty)
    }

    /// Whitespace-only OCR output is treated as "no recognized text" and leaves the
    /// clipboard untouched.
    @Test func extractImageTextDoesNotRewriteEmptyRecognition() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let image = sampleImage()
        let recognizer = RecordingImageTextRecognizer(output: " \n\t ")
        let pb = FakePasteboard(image: image, rawImageBytes: image.data.count)
        let controller = StripController(
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults
        )

        let outcome = await controller.extractImageText()
        #expect(outcome == .notApplicable)
        #expect(recognizer.callCount == 1)
        #expect(pb.writes.isEmpty)
    }

    /// Recognized text is also bounded before it is written back to the pasteboard.
    @Test func extractImageTextRefusesOversizedRecognizedText() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let image = sampleImage()
        let text = String(repeating: "x", count: 1_000)
        let recognizer = RecordingImageTextRecognizer(output: text)
        let pb = FakePasteboard(image: image, rawImageBytes: image.data.count)
        let controller = StripController(
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults,
            maxInputBytes: 16
        )

        let outcome = await controller.extractImageText()
        #expect(outcome == .tooLarge(bytes: 1_000, rich: true))
        #expect(recognizer.callCount == 1)
        #expect(pb.writes.isEmpty)
    }

    /// Oversized image bytes are refused before Vision decodes or recognizes the
    /// image.
    @Test func oversizedImageRepresentationIsRefusedBeforeRecognition() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let recognizer = RecordingImageTextRecognizer(output: "should not run")
        let pb = FakePasteboard(image: sampleImage(), rawImageBytes: 1_000)
        let controller = StripController(
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults,
            maxInputBytes: 16
        )

        let outcome = await controller.extractImageText()
        #expect(outcome == .tooLarge(bytes: 1_000, rich: true))
        #expect(
            pb.materializedImageReadCount == 0,
            "oversized image representations must be rejected before recognition")
        #expect(recognizer.callCount == 0)
        #expect(pb.writes.isEmpty)
    }

    /// Oversized decoded dimensions are refused as a size failure and never become
    /// an empty/no-op OCR result.
    @Test func oversizedDecodedImageDimensionsAreReportedAsTooLarge() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let recognizer = ThrowingImageTextRecognizer(
            error: ImageTextRecognitionError.oversizedImageDimensions(
                width: 10_000,
                height: 10_000,
                maxPixelCount: 30_000_000))
        let pb = FakePasteboard(image: sampleImage(), rawImageBytes: 4)
        let controller = StripController(
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults
        )

        let outcome = await controller.extractImageText()
        #expect(outcome == .tooLarge(bytes: 400_000_000, rich: true))
        #expect(recognizer.callCount == 1)
        #expect(pb.writes.isEmpty)
    }

    /// Other unreadable-image recognizer failures remain content-free no-ops.
    @Test func unreadableImageRecognitionIsNotApplicable() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let recognizer = ThrowingImageTextRecognizer(
            error: ImageTextRecognitionError.unreadableImage)
        let pb = FakePasteboard(image: sampleImage(), rawImageBytes: 4)
        let controller = StripController(
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults
        )

        let outcome = await controller.extractImageText()
        #expect(outcome == .notApplicable)
        #expect(pb.writes.isEmpty)
    }

    /// A rejected OCR write is surfaced and not recorded as a self-write generation.
    @Test func extractImageTextSurfacesWriteFailure() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let recognizer = RecordingImageTextRecognizer(output: "text")
        let pb = FakePasteboard(image: sampleImage(), rawImageBytes: 4)
        pb.failNextPlainWrite = true
        let controller = StripController(
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults
        )

        let outcome = await controller.extractImageText()
        #expect(outcome == .writeFailed)
        #expect(pb.writes.isEmpty)
    }

    /// Vision OCR can be slow; the recognizer must run off the main thread just
    /// like the core transform does.
    @Test func imageTextRecognitionRunsOffTheMainThread() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let image = sampleImage()
        let recognizer = RecordingImageTextRecognizer(output: "text", delay: 0.02)
        let pb = FakePasteboard(image: image, rawImageBytes: image.data.count)
        let controller = StripController(
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults
        )

        let outcome = await controller.extractImageText()
        #expect(outcome == .stripped(changed: true))
        #expect(
            recognizer.ranOnMainThread == false,
            "image text recognition must run off the main thread")
    }

    /// CI-safe performance guard for Swift-only OCR orchestration. This does not
    /// benchmark Apple's Vision framework; it catches accidental slow paths in the
    /// shell-owned loop around bounded image reads, detached recognizer calls,
    /// generation checks, and writeback.
    @Test func imageTextCommandOverheadStaysBoundedWithFastRecognizer() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let image = sampleImage()
        let recognizer = RecordingImageTextRecognizer(output: "text")
        let pb = FakePasteboard()
        let controller = StripController(
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults,
            busyThreshold: .seconds(60)
        )

        let iterations = 100
        let start = DispatchTime.now().uptimeNanoseconds
        for _ in 0..<iterations {
            pb.externalSetImage(image, rawImageBytes: image.data.count)
            let outcome = await controller.extractImageText()
            #expect(outcome == .stripped(changed: true))
        }
        let elapsedMs = Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000

        #expect(recognizer.callCount == iterations)
        #expect(pb.writes.count == iterations)
        #expect(
            elapsedMs < 2_000,
            "Swift OCR command orchestration took \(elapsedMs) ms for \(iterations) runs")
    }

    /// CI-safe performance guard for the continuous-mode OCR entry path. This keeps
    /// the text-read miss + bounded image read + fake recognizer + writeback loop
    /// from accidentally growing expensive in xPare-owned Swift code.
    @Test func continuousImageTextCommandOverheadStaysBoundedWithFastRecognizer() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let image = sampleImage()
        let recognizer = RecordingImageTextRecognizer(output: "text")
        let pb = FakePasteboard()
        let controller = StripController(
            settings: Settings(mode: .continuous, ocrImagesInContinuousMode: true),
            pasteboard: pb,
            imageTextRecognizer: recognizer,
            defaults: defaults,
            busyThreshold: .seconds(60)
        )

        let iterations = 100
        let start = DispatchTime.now().uptimeNanoseconds
        for _ in 0..<iterations {
            pb.externalSetImage(image, rawImageBytes: image.data.count)
            let outcome = await controller.stripNow(trigger: .clipboardChanged)
            #expect(outcome == .stripped(changed: true))
        }
        let elapsedMs = Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000

        #expect(recognizer.callCount == iterations)
        #expect(pb.readBestCalls == iterations)
        #expect(pb.readImageCalls == iterations)
        #expect(pb.writes.count == iterations)
        #expect(
            elapsedMs < 2_000,
            "Continuous OCR orchestration took \(elapsedMs) ms for \(iterations) runs")
    }

    /// If the clipboard changes while OCR is running, stale recognized text must not
    /// overwrite the newer clipboard generation.
    @Test func staleImageTextRecognitionDoesNotOverwriteNewerClipboard() async throws {
        let (defaults, suite) = try isolatedDefaults()
        defer { defaults.removePersistentDomain(forName: suite) }

        let blocking = BlockingImageTextRecognizer(output: "old recognized text")
        let pb = FakePasteboard(image: sampleImage(), rawImageBytes: 4)
        let controller = StripController(
            pasteboard: pb,
            imageTextRecognizer: blocking,
            defaults: defaults
        )

        let task = Task { @MainActor in
            await controller.extractImageText()
        }
        let deadline = Date().addingTimeInterval(1.0)
        while !blocking.hasStarted, Date() < deadline {
            await Task.yield()
            try await Task.sleep(for: .milliseconds(1))
        }
        #expect(blocking.hasStarted, "test recognizer should have started")
        if !blocking.hasStarted {
            blocking.release()
        }

        let newer = PasteboardSnapshot(text: "new clipboard", kind: .plain)
        pb.externalSet(newer)
        blocking.release()

        let outcome = await task.value
        #expect(outcome == .stripped(changed: false))
        #expect(pb.snapshot == newer)
        #expect(
            pb.writes.isEmpty,
            "stale OCR output must not overwrite newer clipboard data")
    }
}

private func sampleImage() -> PasteboardImage {
    PasteboardImage(
        data: Data([0x89, 0x50, 0x4e, 0x47]),
        pasteboardType: NSPasteboard.PasteboardType.png.rawValue
    )
}

private final class RecordingImageTextRecognizer: ImageTextRecognizing, @unchecked Sendable {
    private let lock = NSLock()
    private let output: String
    private let delay: TimeInterval
    private var _callCount = 0
    private var _images: [PasteboardImage] = []
    private var _ranOnMainThread: Bool?

    init(output: String, delay: TimeInterval = 0) {
        self.output = output
        self.delay = delay
    }

    var callCount: Int {
        lock.lock()
        defer { lock.unlock() }
        return _callCount
    }

    var images: [PasteboardImage] {
        lock.lock()
        defer { lock.unlock() }
        return _images
    }

    var ranOnMainThread: Bool? {
        lock.lock()
        defer { lock.unlock() }
        return _ranOnMainThread
    }

    func recognizeText(in image: PasteboardImage) throws -> String {
        lock.lock()
        _callCount += 1
        _images.append(image)
        _ranOnMainThread = Thread.isMainThread
        lock.unlock()
        if delay > 0 {
            Thread.sleep(forTimeInterval: delay)
        }
        return output
    }
}

private final class ThrowingImageTextRecognizer: ImageTextRecognizing, @unchecked Sendable {
    private let lock = NSLock()
    private let error: Error
    private var _callCount = 0

    init(error: Error) {
        self.error = error
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
        throw error
    }
}

private final class BlockingImageTextRecognizer: ImageTextRecognizing, @unchecked Sendable {
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

    func recognizeText(in image: PasteboardImage) throws -> String {
        lock.lock()
        _hasStarted = true
        lock.unlock()
        proceed.wait()
        return output
    }
}
