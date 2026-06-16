import Foundation
import ImageIO
import Testing
import UniformTypeIdentifiers
import Vision
@testable import XPareKit

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

        #expect(
            throws: ImageTextRecognitionError.oversizedImageDimensions(
                width: 10_000,
                height: 10_000,
                maxPixelCount: 30_000_000)
        ) {
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

    @Test func unreadableImageDataIsRejected() {
        let image = PasteboardImage(data: Data([0x00, 0x01]), pasteboardType: "public.png")

        #expect(throws: ImageTextRecognitionError.unreadableImage) {
            try VisionTextRecognizer().recognizeText(in: image)
        }
    }

    @Test func blankImageRecognizesAsEmptyText() throws {
        let image = PasteboardImage(data: try blankPngData(), pasteboardType: "public.png")

        let text = try VisionTextRecognizer(maxPixelCount: 1_000).recognizeText(in: image)

        #expect(text.isEmpty)
    }

    @Test func imageOrientationMetadataMapsToVisionOrientation() {
        let properties: [String: Any] = [
            kCGImagePropertyOrientation as String: NSNumber(
                value: CGImagePropertyOrientation.right.rawValue)
        ]

        #expect(VisionTextRecognizer.orientation(from: properties) == .right)
        #expect(VisionTextRecognizer.orientation(from: [:]) == .up)
    }
}

private func blankPngData(width: Int = 8, height: Int = 8) throws -> Data {
    let colorSpace = CGColorSpaceCreateDeviceRGB()
    let context = try #require(
        CGContext(
            data: nil,
            width: width,
            height: height,
            bitsPerComponent: 8,
            bytesPerRow: width * 4,
            space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
    context.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
    context.fill(CGRect(x: 0, y: 0, width: width, height: height))
    let image = try #require(context.makeImage())

    let data = NSMutableData()
    let destination = try #require(
        CGImageDestinationCreateWithData(
            data,
            UTType.png.identifier as CFString,
            1,
            nil))
    CGImageDestinationAddImage(destination, image, nil)
    #expect(CGImageDestinationFinalize(destination))
    return data as Data
}
