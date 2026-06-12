import Foundation
import ImageIO
import Vision

/// Errors from the local image-text recognizer. They carry no clipboard content.
public enum ImageTextRecognitionError: Error, Equatable {
    case unreadableImage
    case oversizedImageDimensions(width: Int, height: Int, maxPixelCount: Int)

    var estimatedDecodedBytes: Int? {
        guard case .oversizedImageDimensions(let width, let height, _) = self else {
            return nil
        }
        let pixels = width.multipliedReportingOverflow(by: height)
        guard !pixels.overflow else { return Int.max }
        let bytes = pixels.partialValue.multipliedReportingOverflow(by: 4)
        return bytes.overflow ? Int.max : bytes.partialValue
    }
}

/// Local OCR abstraction so `StripController` can run Vision off the main actor
/// and tests can inject deterministic recognizers without touching Vision.
public protocol ImageTextRecognizing: Sendable {
    func recognizeText(in image: PasteboardImage) throws -> String
}

/// Apple's built-in, on-device Vision OCR over a bounded pasteboard image.
public struct VisionTextRecognizer: ImageTextRecognizing {
    public static let defaultMaxPixelCount = 30_000_000

    private let maxPixelCount: Int

    public init(maxPixelCount: Int = Self.defaultMaxPixelCount) {
        self.maxPixelCount = maxPixelCount
    }

    public func recognizeText(in image: PasteboardImage) throws -> String {
        guard let source = CGImageSourceCreateWithData(image.data as CFData, nil),
              let properties = Self.imageProperties(from: source) else {
            throw ImageTextRecognitionError.unreadableImage
        }
        try Self.validateDimensions(in: properties, maxPixelCount: maxPixelCount)

        guard let cgImage = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
            throw ImageTextRecognitionError.unreadableImage
        }
        let orientation = Self.orientation(from: properties)

        let request = Self.makeRequest()

        let handler = VNImageRequestHandler(cgImage: cgImage, orientation: orientation, options: [:])
        try handler.perform([request])

        let lines = (request.results ?? []).compactMap { observation in
            observation.topCandidates(1)
                .first?
                .string
                .trimmingCharacters(in: .whitespacesAndNewlines)
        }.filter { !$0.isEmpty }

        return lines.joined(separator: "\n")
    }

    static func makeRequest() -> VNRecognizeTextRequest {
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.usesLanguageCorrection = false
        return request
    }

    static func imageProperties(from source: CGImageSource) -> [String: Any]? {
        CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any]
    }

    static func validateDimensions(
        in properties: [String: Any],
        maxPixelCount: Int
    ) throws {
        guard let width = imageDimension(properties[kCGImagePropertyPixelWidth as String]),
              let height = imageDimension(properties[kCGImagePropertyPixelHeight as String]) else {
            throw ImageTextRecognitionError.unreadableImage
        }
        try validateDimensions(width: width, height: height, maxPixelCount: maxPixelCount)
    }

    static func validateDimensions(
        width: Int,
        height: Int,
        maxPixelCount: Int
    ) throws {
        guard width > 0, height > 0 else {
            throw ImageTextRecognitionError.unreadableImage
        }
        guard maxPixelCount > 0,
              width <= maxPixelCount / height else {
            throw ImageTextRecognitionError.oversizedImageDimensions(
                width: width,
                height: height,
                maxPixelCount: maxPixelCount)
        }
    }

    static func orientation(from properties: [String: Any]) -> CGImagePropertyOrientation {
        guard let number = properties[kCGImagePropertyOrientation as String] as? NSNumber,
              let rawValue = UInt32(exactly: number.intValue),
              let orientation = CGImagePropertyOrientation(rawValue: rawValue) else {
            return .up
        }
        return orientation
    }

    private static func imageDimension(_ value: Any?) -> Int? {
        guard let number = value as? NSNumber else { return nil }
        return number.intValue
    }
}
