import Foundation
import ImageIO
import Vision

/// Errors from the local image-text recognizer. They carry no clipboard content.
public enum ImageTextRecognitionError: Error, Equatable {
    case unreadableImage
}

/// Local OCR abstraction so `StripController` can run Vision off the main actor
/// and tests can inject deterministic recognizers without touching Vision.
public protocol ImageTextRecognizing: Sendable {
    func recognizeText(in image: PasteboardImage) throws -> String
}

/// Apple's built-in, on-device Vision OCR over a bounded pasteboard image.
public struct VisionTextRecognizer: ImageTextRecognizing {
    public init() {}

    public func recognizeText(in image: PasteboardImage) throws -> String {
        guard let source = CGImageSourceCreateWithData(image.data as CFData, nil),
              let cgImage = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
            throw ImageTextRecognitionError.unreadableImage
        }

        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.usesLanguageCorrection = true

        let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
        try handler.perform([request])

        let lines = (request.results ?? []).compactMap { observation in
            observation.topCandidates(1)
                .first?
                .string
                .trimmingCharacters(in: .whitespacesAndNewlines)
        }.filter { !$0.isEmpty }

        return lines.joined(separator: "\n")
    }
}
