# Milestone 7 Worklog — Provider Shared Pipeline

## Status: 🟥 BLOCKED

## Architecture

### Existing Components
- `src-tauri/src/capture/macos/sck_probe.swift` — ScreenCaptureKit probe
- `src-tauri/src/media/shared_pipeline.rs` — Pipeline proof (test-only)
- `src-tauri/src/encode/` — Encoding module

### Pipeline Design
managed Chrome → ScreenCaptureKit → frame callback → VideoToolbox H.264 → QUIC media stream → presentation buffer → Host + Guest same encoded timeline

### What Exists
- Capture module with ScreenCaptureKit constants
- Encode module structure
- shared_pipeline proof-of-concept (not wired into AppRuntime)

### What's Missing
- ScreenCaptureKit not wired into AppRuntime (requires macOS permission dialog)
- QUIC media stream for encoded frames not implemented
- Presentation buffer not implemented
- Host parity (Host watching encoded, not direct Chrome) not implemented

## Blocker
ScreenCaptureKit requires macOS system permission dialog requiring human interaction.
Cannot proceed without granting Screen Recording permission.

## Files
- `src-tauri/src/capture/macos/sck_probe.swift`
- `src-tauri/src/media/shared_pipeline.rs`
- `src-tauri/src/encode/`

## External Verification Needed
1. Grant Screen Recording permission in macOS System Settings
2. Run capture probe → verify frames produced
3. Wire capture → encode → QUIC → presentation pipeline
