pub mod cache;
pub mod local_perfect;
pub mod manifest;
pub mod player;
// AUD-08 (NOT remediated): `shared_pipeline.rs` sits in this directory but is
// deliberately NOT declared. Declaring it does not compile — it references
// three APIs that have since changed:
//   * `QuicServer::run()` now takes `Option<Arc<Mutex<LocalSyncCoordinator>>>`
//   * `QuicClient::connect(..)` takes `display_name: String`, not `&str`
//   * `QuicClient::send_shared_stream_packet` no longer exists
// That third one matters: the post-remediation adversarial audit found that the
// **shared-stream transport API is gone from `network/quic.rs` entirely**, so
// this file is *orphaned* rather than merely stale — the capability it exists to
// prove has been removed from the codebase.
//
// It is a macOS-only proof harness (`screencapture` + `ffmpeg
// h264_videotoolbox`) for Provider Shared, which remains
// `shared_available: false`. "Declare and repair it" is therefore not an
// option: repairing it would mean either gutting the transport assertion (the
// only thing it proves) or rebuilding a deleted send path, which is Shared
// implementation work and out of scope. **Deleting it is the owner's call**;
// left undeclared so it cannot silently rot further, with the finding recorded
// in `shared_pipeline.rs` and in both audit reports.
pub mod shared_stream;
pub mod stream;
pub mod transfer;
