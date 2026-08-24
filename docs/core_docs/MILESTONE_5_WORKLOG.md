# Milestone 5 Worklog — Real Two-Peer Call

## Status: 🟨 IN PROGRESS

## Architecture

### Call Signal Routing (`src-tauri/src/network/quic.rs`)
- CallSignal variant added to both ClientRequest and ServerEvent
- Host: broadcasts CallSignal via host_event_tx
- Guest: sends CallSignal via QUIC client
- Bidirectional offer/answer/ICE exchange over authenticated QUIC

### Privacy Mode (`src-tauri/src/app_runtime.rs`)
- set_privacy_mode(true): disables camera AND mic
- set_privacy_mode(false): does NOT auto-re-enable either
- User must manually re-enable camera/mic after leaving Privacy
- Ghost mode set automatically when Privacy enabled

### Call Modes
- VideoVoice: camera enabled, mic enabled (after user enables)
- VoiceOnly: camera disabled, mic enabled
- Off: both disabled

### Adaptive Quality (Policy)
- CameraState with tier_a/tier_b/tier_c degradation levels
- Policy exists but actual RTCRtpSender constraint wiring is external

## Tests (3 in m2_integration.rs)
1. m5_call_signal_offer_arrives_at_guest
2. m5_call_signal_answer_arrives_at_host
3. m5_call_signal_ice_arrives_bidirectional

## Files
- `src-tauri/src/network/quic.rs` — CallSignal ClientRequest + ServerEvent
- `src-tauri/src/app_runtime.rs` — submit_call_signal, set_privacy_mode
- `src/call/webrtc.ts` — getUserMedia, adaptive quality

## External Verification Needed
1. macOS camera/mic permission dialog
2. Real WebRTC peer connection with two devices
3. Privacy mode on/off with actual media tracks
