pub mod cache;
pub mod local_perfect;
pub mod manifest;
pub mod player;
// AUD-08 (CLOSED): `shared_pipeline.rs` was deleted in this batch. It sat here
// undeclared — never compiled by any build, its one `#[ignore]`d test unable to
// run — because declaring it did not compile. It drove three APIs that no longer
// exist in the shape it expected, and the decisive one was
// `QuicClient::send_shared_stream_packet`: **the shared-stream transport API is
// gone from `network/quic.rs` entirely**, so the file was *orphaned* rather than
// merely stale — the capability it existed to prove had been removed from the
// codebase.
//
// That ruled out "declare and repair it". Making it compile would have meant
// either gutting the transport assertion (the only thing the harness proved, so
// the file would have become a shell still claiming coverage) or rebuilding a
// deleted send path — which is Provider Shared implementation work, and Provider
// Shared is deliberately outside the stable scope. Deleting it was the honest
// option; the file remains recoverable from git history.
//
// Nothing here changes what the product can do: **Provider Shared is not
// implemented and stays disabled** (`shared_available: false`; the stable path
// is refused by `providers::sync::provider_mode_gate`). There is no capture,
// encoding, encryption or QUIC media transport in this crate, and none was
// added or removed by the deletion.
pub mod shared_stream;
pub mod stream;
pub mod transfer;
