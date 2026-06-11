import Foundation
import ImageIO
import Testing
import Vision
@testable import SafetyStripKit

@Suite("Image text recognizer")
struct ImageTextRecognizerTests {
    @Test func visionRequestPreservesLiteralCandidates() {
        let request = VisionTextRecognizer.makeRequest()

        #expect(request.recognitionLevel == .accurate)
        #expect(request.usesLanguageCorrection == false)
    }

    @Test func oversizedDecodedDimensionsAreRejectedBeforeDecode() {
        let properties: [String: Any] = [
            kCGImagePropertyPixelWidth as String: NSNumber(value: 10_000),
            kCGImagePropertyPixelHeight as String: NSNumber(value: 10_000),
        ]

        #expect(throws: ImageTextRecognitionError.oversizedImageDimensions(
            width: 10_000,
            height: 10_000,
            maxPixelCount: 30_000_000
        )) {
            try VisionTextRecognizer.validateDimensions(
                in: properties,
                maxPixelCount: 30_000_000)
        }
    }

    @Test func unreadableDimensionsAreRejectedBeforeDecode() {
        #expect(throws: ImageTextRecognitionError.unreadableImage) {
            try VisionTextRecognizer.validateDimensions(
                in: [:],
                maxPixelCount: 30_000_000)
        }
    }

    @Test func imageOrientationMetadataMapsToVisionOrientation() {
        let properties: [String: Any] = [
            kCGImagePropertyOrientation as String: NSNumber(value: CGImagePropertyOrientation.right.rawValue),
        ]

        #expect(VisionTextRecognizer.orientation(from: properties) == .right)
        #expect(VisionTextRecognizer.orientation(from: [:]) == .up)
    }
}
