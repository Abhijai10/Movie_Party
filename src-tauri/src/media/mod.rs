pub mod cache;
pub mod local_perfect;
pub mod manifest;
pub mod player;
// AUD-08 (NOT remediated): `shared_pipeline.rs` sits in this directory but is
// deliberately NOT declared. Declaring it does not compile — it references
// three APIs that have since changed:
//   * `QuicServer::run()` now takes `Option<Arc<Mutex<LocalSyncCoordinator>>>`
//   * `QuicClient::connect(..)`'s signature changed
//   * `QuicClient::send_shared_stream_packet` no longer exists
// It is a macOS-only proof harness (`screencapture` + `ffmpeg
// h264_videotoolbox`) for Provider Shared, which remains
// `shared_available: false`. Repairing it would be Shared implementation work
// and is out of scope here; deleting it is the owner's call. Left undeclared
// so it cannot silently rot further, with its staleness recorded in
// `shared_pipeline.rs` and in the remediation report.
pub mod shared_stream;
pub mod stream;
pub mod transfer;
