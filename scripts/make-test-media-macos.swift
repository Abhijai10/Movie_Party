// Deterministic H.264 media fixture generator for the Movie Party real_* tests.
//
// Why a generator exists at all: the real_* playback tests need a real
// decodable video, and they used to read one from a hardcoded /tmp path that
// nothing in the repo created — so on any clean checkout they silently passed
// without executing. The fixture is now committed (see .gitignore), and this
// script is how it is reproduced.
//
// Why Swift/AVFoundation rather than ffmpeg: ffmpeg is not a dependency of this
// repository and is not guaranteed to exist on a machine that builds it, while
// AVFoundation ships with macOS — the only platform these tests run on. This
// keeps regeneration a single command with no external tooling.
//
// The output is a 320x240, 15 fps, H.264 (avc1), audio-free MP4 of a moving
// colour field with a bright sweeping bar. Two properties matter to the tests:
//
//   1. Every frame is bright and non-uniform, so a "the render buffer contains
//      non-zero pixel data" assertion is meaningful. A black or static fixture
//      would make that assertion either vacuous or wrong.
//   2. Successive frames differ structurally, so a renderer that returns a
//      stale frame cannot pass by accident.
//
// Usage: swift make-test-media-macos.swift <output.mp4> [seconds]

import AVFoundation
import CoreVideo
import Foundation

let width = 320
let height = 240
let fps: Int32 = 15

let args = CommandLine.arguments
let outPath = args.count > 1 ? args[1] : "movie_party_test_320x240.mp4"
let seconds = args.count > 2 ? (Int(args[2]) ?? 3) : 3
let totalFrames = Int(fps) * max(seconds, 1)

let url = URL(fileURLWithPath: outPath)
try? FileManager.default.removeItem(at: url)

guard let writer = try? AVAssetWriter(outputURL: url, fileType: .mp4) else {
    FileHandle.standardError.write("FAILED: could not create AVAssetWriter\n".data(using: .utf8)!)
    exit(1)
}

let settings: [String: Any] = [
    AVVideoCodecKey: AVVideoCodecType.h264,
    AVVideoWidthKey: width,
    AVVideoHeightKey: height,
]

let input = AVAssetWriterInput(mediaType: .video, outputSettings: settings)
input.expectsMediaDataInRealTime = false

let adaptor = AVAssetWriterInputPixelBufferAdaptor(
    assetWriterInput: input,
    sourcePixelBufferAttributes: [
        kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
        kCVPixelBufferWidthKey as String: width,
        kCVPixelBufferHeightKey as String: height,
    ]
)

writer.add(input)
guard writer.startWriting() else {
    FileHandle.standardError.write("FAILED: startWriting: \(String(describing: writer.error))\n".data(using: .utf8)!)
    exit(1)
}
writer.startSession(atSourceTime: .zero)

var pool: CVPixelBufferPool?
CVPixelBufferPoolCreate(
    nil, nil,
    [
        kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
        kCVPixelBufferWidthKey as String: width,
        kCVPixelBufferHeightKey as String: height,
    ] as CFDictionary,
    &pool
)

/// One frame of a deterministic pattern. No frame is black: every channel has a
/// floor well above zero.
func makeFrame(_ index: Int) -> CVPixelBuffer? {
    guard let pool = pool else { return nil }
    var maybeBuffer: CVPixelBuffer?
    guard CVPixelBufferPoolCreatePixelBuffer(nil, pool, &maybeBuffer) == kCVReturnSuccess,
          let buffer = maybeBuffer else { return nil }

    CVPixelBufferLockBaseAddress(buffer, [])
    defer { CVPixelBufferUnlockBaseAddress(buffer, []) }

    guard let base = CVPixelBufferGetBaseAddress(buffer) else { return nil }
    let bytesPerRow = CVPixelBufferGetBytesPerRow(buffer)
    let pixels = base.assumingMemoryBound(to: UInt8.self)

    let phase = Double(index) / Double(max(totalFrames, 1))
    let barX = (index * 7) % max(width - 24, 1)

    for y in 0..<height {
        let row = pixels + y * bytesPerRow
        let gy = Double(y) / Double(height)
        for x in 0..<width {
            let px = row + x * 4
            let gx = Double(x) / Double(width)
            // Deliberately bright floors (>= 60) so no pixel is ever zero.
            let blue = UInt8(60.0 + 150.0 * (0.5 + 0.5 * sin((gx + phase) * 6.283185307))
                .rounded())
            let green = UInt8(70.0 + 120.0 * gy)
            let red = UInt8(80.0 + 130.0 * (0.5 + 0.5 * cos((gx + gy + phase * 2.0) * 6.283185307))
                .rounded())
            px[0] = blue
            px[1] = green
            px[2] = red
            px[3] = 255
        }
        // A bright bar that sweeps across the frame, so consecutive frames
        // differ in structure and not merely in colour.
        if barX < width {
            for x in barX..<min(barX + 24, width) {
                let px = row + x * 4
                px[0] = 255
                px[1] = 255
                px[2] = 255
                px[3] = 255
            }
        }
    }

    return buffer
}

var frame = 0
while frame < totalFrames {
    if input.isReadyForMoreMediaData {
        guard let buffer = makeFrame(frame) else {
            FileHandle.standardError.write("FAILED: frame \(frame) buffer\n".data(using: .utf8)!)
            exit(1)
        }
        let time = CMTime(value: CMTimeValue(frame), timescale: fps)
        if !adaptor.append(buffer, withPresentationTime: time) {
            FileHandle.standardError.write(
                "FAILED: append frame \(frame): \(String(describing: writer.error))\n".data(using: .utf8)!)
            exit(1)
        }
        frame += 1
    } else {
        usleep(2000)
    }
}

input.markAsFinished()
let done = DispatchSemaphore(value: 0)
writer.finishWriting { done.signal() }
done.wait()

guard writer.status == .completed else {
    FileHandle.standardError.write(
        "FAILED: finishWriting: \(String(describing: writer.error))\n".data(using: .utf8)!)
    exit(1)
}

let attrs = try? FileManager.default.attributesOfItem(atPath: outPath)
let size = (attrs?[.size] as? Int) ?? 0
print("OK wrote \(outPath) (\(size) bytes, \(totalFrames) frames, \(width)x\(height) @ \(fps)fps)")
