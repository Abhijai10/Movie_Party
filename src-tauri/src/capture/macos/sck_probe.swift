import CoreMedia
import Dispatch
import Foundation
import ScreenCaptureKit

final class ProbeOutput: NSObject, SCStreamOutput {
    private(set) var frameCount = 0
    private(set) var width = 0
    private(set) var height = 0
    private(set) var firstPts: CMTime?

    func stream(
        _ stream: SCStream,
        didOutputSampleBuffer sampleBuffer: CMSampleBuffer,
        of outputType: SCStreamOutputType
    ) {
        guard outputType == .screen, sampleBuffer.isValid else {
            return
        }

        frameCount += 1
        firstPts = firstPts ?? sampleBuffer.presentationTimeStamp
        if let imageBuffer = sampleBuffer.imageBuffer {
            width = CVPixelBufferGetWidth(imageBuffer)
            height = CVPixelBufferGetHeight(imageBuffer)
        }
    }
}

@main
struct ScreenCaptureKitProbe {
    static func main() async throws {
        let content = try await SCShareableContent.excludingDesktopWindows(
            false,
            onScreenWindowsOnly: true
        )
        guard let display = content.displays.first else {
            fputs("No capturable display found\n", stderr)
            Foundation.exit(2)
        }

        let filter = SCContentFilter(display: display, excludingWindows: [])
        let configuration = SCStreamConfiguration()
        configuration.width = 640
        configuration.height = 360
        configuration.minimumFrameInterval = CMTime(value: 1, timescale: 30)
        configuration.queueDepth = 3

        let output = ProbeOutput()
        let stream = SCStream(filter: filter, configuration: configuration, delegate: nil)
        try stream.addStreamOutput(
            output,
            type: .screen,
            sampleHandlerQueue: DispatchQueue(label: "move-party.sck-probe")
        )
        try await stream.startCapture()
        try await Task.sleep(nanoseconds: 3_000_000_000)
        try await stream.stopCapture()

        guard output.frameCount > 0 else {
            fputs("ScreenCaptureKit started but produced no frames\n", stderr)
            Foundation.exit(3)
        }

        let pts = output.firstPts.map { CMTimeGetSeconds($0) } ?? 0
        print(
            "frames=\(output.frameCount) width=\(output.width) height=\(output.height) pts=\(pts)"
        )
    }
}
