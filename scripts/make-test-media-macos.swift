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
// The output is a 320x240, 15 fps, H.264 (avc1) MP4 of a moving colour field
// with a bright sweeping bar. Two properties matter to the tests:
//
//   1. Every frame is bright and non-uniform, so a "the render buffer contains
//      non-zero pixel data" assertion is meaningful. A black or static fixture
//      would make that assertion either vacuous or wrong.
//   2. Successive frames differ structurally, so a renderer that returns a
//      stale frame cannot pass by accident.
//
// AUDIO (AUD-06, opt-in via `--with-audio`): with the flag the file also gets a
// mono AAC sine track. Without it the output is exactly the video-only fixture
// as before — the committed default MUST NOT change, because the render tests
// depend on its duration and frame content.
//
// The tone is a steady 440 Hz sine. It is deliberately NOT silence: an
// audio-bearing fixture whose samples are all zero could not distinguish "the
// audio track decoded" from "the audio track decoded to nothing", which is the
// entire point of having it.
//
// Usage: swift make-test-media-macos.swift <output.mp4> [seconds] [--with-audio]

import AVFoundation
import AudioToolbox
import CoreMedia
import CoreVideo
import Foundation

let width = 320
let height = 240
let fps: Int32 = 15

let args = CommandLine.arguments
let outPath = args.count > 1 ? args[1] : "movie_party_test_320x240.mp4"
let seconds = args.count > 2 ? (Int(args[2]) ?? 3) : 3
let withAudio = args.contains("--with-audio")
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

// ── Optional mono AAC audio track (AUD-06) ──────────────────────────────────
// Fed as 16-bit LPCM and encoded to AAC by the writer, so no encoder is needed
// beyond what AVFoundation ships with.
let audioSampleRate: Double = 44_100
let audioChannels: UInt32 = 1
let audioBitsPerChannel: UInt32 = 16
let audioFramesPerChunk = 1_024

var audioInput: AVAssetWriterInput?
if withAudio {
    let audioSettings: [String: Any] = [
        AVFormatIDKey: kAudioFormatMPEG4AAC,
        AVSampleRateKey: audioSampleRate,
        AVNumberOfChannelsKey: audioChannels,
        AVEncoderBitRateKey: 64_000,
    ]
    let audio = AVAssetWriterInput(mediaType: .audio, outputSettings: audioSettings)
    audio.expectsMediaDataInRealTime = false
    guard writer.canAdd(audio) else {
        FileHandle.standardError.write("FAILED: writer rejected the audio input\n".data(using: .utf8)!)
        exit(1)
    }
    writer.add(audio)
    audioInput = audio
}

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

// ── Audio: format description + sample-buffer builder ───────────────────────
// Set up BEFORE the feed loop, because the loop interleaves both inputs.
// `totalAudioFrames` is 0 without --with-audio, which is what terminates the
// audio half of the loop condition.
var audioFormatDesc: CMAudioFormatDescription?
let audioBytesPerFrame = Int(audioChannels * audioBitsPerChannel / 8)
let totalAudioFrames = withAudio ? Int(audioSampleRate) * max(seconds, 1) : 0
let toneHz = 440.0

if withAudio {
    var asbd = AudioStreamBasicDescription(
        mSampleRate: audioSampleRate,
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked,
        mBytesPerPacket: audioChannels * audioBitsPerChannel / 8,
        mFramesPerPacket: 1,
        mBytesPerFrame: audioChannels * audioBitsPerChannel / 8,
        mChannelsPerFrame: audioChannels,
        mBitsPerChannel: audioBitsPerChannel,
        mReserved: 0
    )
    let fdStatus = CMAudioFormatDescriptionCreate(
        allocator: kCFAllocatorDefault,
        asbd: &asbd,
        layoutSize: 0,
        layout: nil,
        magicCookieSize: 0,
        magicCookie: nil,
        extensions: nil,
        formatDescriptionOut: &audioFormatDesc
    )
    guard fdStatus == noErr, audioFormatDesc != nil else {
        FileHandle.standardError.write("FAILED: audio format description (\(fdStatus))\n".data(using: .utf8)!)
        exit(1)
    }
}

/// One LPCM chunk of the 440 Hz tone, starting at `startFrame`.
func makeAudioSampleBuffer(startFrame: Int, frames: Int) -> CMSampleBuffer? {
    guard let formatDesc = audioFormatDesc else { return nil }
    let byteCount = frames * audioBytesPerFrame

    // 0.25 amplitude: comfortably non-silent and nowhere near clipping. Every
    // sample is non-zero, so "decoded to silence" cannot pass for success.
    var samples = [Int16](repeating: 0, count: frames)
    for i in 0..<frames {
        let t = Double(startFrame + i) / audioSampleRate
        samples[i] = Int16((sin(2.0 * Double.pi * toneHz * t) * 0.25 * 32767.0).rounded())
    }

    var blockBuffer: CMBlockBuffer?
    guard CMBlockBufferCreateWithMemoryBlock(
        allocator: kCFAllocatorDefault,
        memoryBlock: nil,
        blockLength: byteCount,
        blockAllocator: kCFAllocatorDefault,
        customBlockSource: nil,
        offsetToData: 0,
        dataLength: byteCount,
        flags: 0,
        blockBufferOut: &blockBuffer
    ) == kCMBlockBufferNoErr, let blockBuffer = blockBuffer else { return nil }

    let fillStatus = samples.withUnsafeBytes { raw -> OSStatus in
        CMBlockBufferReplaceDataBytes(
            with: raw.baseAddress!,
            blockBuffer: blockBuffer,
            offsetIntoDestination: 0,
            dataLength: byteCount
        )
    }
    guard fillStatus == kCMBlockBufferNoErr else { return nil }

    var sampleBuffer: CMSampleBuffer?
    guard CMAudioSampleBufferCreateReadyWithPacketDescriptions(
        allocator: kCFAllocatorDefault,
        dataBuffer: blockBuffer,
        formatDescription: formatDesc,
        sampleCount: CMItemCount(frames),
        presentationTimeStamp: CMTime(
            value: CMTimeValue(startFrame),
            timescale: CMTimeScale(audioSampleRate)
        ),
        packetDescriptions: nil,
        sampleBufferOut: &sampleBuffer
    ) == noErr, let sampleBuffer = sampleBuffer else { return nil }

    return sampleBuffer
}

// ── Feed both inputs, kept in step ──────────────────────────────────────────
// Two things are required, and both were learned the hard way:
//
//  1. Each iteration feeds whichever input is BEHIND IN TIME. Feeding video
//     unconditionally (it is checked first) lets it race to the end of the file
//     while audio is still at ~1 s, and AVAssetWriter then reports
//     `isReadyForMoreMediaData == false` on BOTH inputs forever — a silent
//     deadlock, with a partially written file and no error.
//  2. Each input is marked finished as soon as its last sample is appended.
//     Waiting until after the loop means the writer is still expecting more
//     video while audio tries to run past it.
var frame = 0
var audioFrameIndex = 0
while frame < totalFrames || audioFrameIndex < totalAudioFrames {
    let videoTime = Double(frame) / Double(fps)
    let audioTime = Double(audioFrameIndex) / audioSampleRate
    let videoIsBehind = audioFrameIndex >= totalAudioFrames || videoTime <= audioTime

    if frame < totalFrames, videoIsBehind, input.isReadyForMoreMediaData {
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
        if frame == totalFrames { input.markAsFinished() }
    } else if let audioInput = audioInput, audioFrameIndex < totalAudioFrames,
        audioInput.isReadyForMoreMediaData
    {
        let chunkFrames = min(audioFramesPerChunk, totalAudioFrames - audioFrameIndex)
        guard let sampleBuffer = makeAudioSampleBuffer(
            startFrame: audioFrameIndex, frames: chunkFrames)
        else {
            FileHandle.standardError.write(
                "FAILED: audio sample buffer at \(audioFrameIndex)\n".data(using: .utf8)!)
            exit(1)
        }
        if !audioInput.append(sampleBuffer) {
            FileHandle.standardError.write(
                "FAILED: append audio at frame \(audioFrameIndex): \(String(describing: writer.error))\n"
                    .data(using: .utf8)!)
            exit(1)
        }
        audioFrameIndex += chunkFrames
        if audioFrameIndex >= totalAudioFrames { audioInput.markAsFinished() }
    }
    // Yield so this does not busy-spin while the writer drains.
    usleep(200)
}

// Both inputs were marked finished inside the loop, as each one ran out.

// Bounded wait. An unbounded `wait()` here hangs silently if the writer never
// calls back, which is indistinguishable from "still encoding" — the failure
// mode that made the first version of this flag look like a hang.
FileHandle.standardError.write(
    "PROGRESS: fed \(frame)/\(totalFrames) video frames, \(audioFrameIndex)/\(totalAudioFrames) audio frames; finalising\n"
        .data(using: .utf8)!)

let done = DispatchSemaphore(value: 0)
writer.finishWriting { done.signal() }
if done.wait(timeout: .now() + 60) == .timedOut {
    FileHandle.standardError.write("FAILED: finishWriting did not complete within 60s\n".data(using: .utf8)!)
    exit(1)
}

guard writer.status == .completed else {
    FileHandle.standardError.write(
        "FAILED: finishWriting: \(String(describing: writer.error))\n".data(using: .utf8)!)
    exit(1)
}

let attrs = try? FileManager.default.attributesOfItem(atPath: outPath)
let size = (attrs?[.size] as? Int) ?? 0
let audioNote = withAudio ? " + 440 Hz mono AAC audio" : " (no audio track)"
print(
    "OK wrote \(outPath) (\(size) bytes, \(totalFrames) frames, \(width)x\(height) @ \(fps)fps\(audioNote))")
