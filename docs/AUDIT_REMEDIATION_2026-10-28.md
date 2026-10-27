# Audit remediation — 2026-10-28

This records the fixes for the repository-wide audit performed on
`codex/integrated-youtube-player` at `7e456b5`. It exists so the next batch does
not have to rediscover *why* these seams moved.

**Rule applied to every finding:** no finding is closed without a regression
test that was observed to fail before the fix. Where a fix could not be verified
on this machine (no second device, no Windows VM, no real extension page), it is
listed under *Still unverified* rather than implied done.

## Summary

| # | Severity | Defect | Fix | Regression test |
|---|---|---|---|---|
| 1 | **Critical** | Companion pairing threw `InvalidCharacterError` for ~74% of generated keys: the app sends the session key as unpadded **base64url**, and the worker decoded it with `atob()`, which only accepts the standard alphabet | the worker now decodes through the existing `decodeSecret` helper (the one decoder that understands both alphabets) | `extension/background.test.js` — pairs with a key containing `-` and `_`, after asserting `atob` rejects that exact string |
| 2 | **High** | Any peer that completed the QUIC handshake received the whole room event stream without authenticating (only provider events were gated) | the per-connection forwarder holds **all** envelopes until `HelloAuth` has been accepted | `an_unauthenticated_peer_never_receives_room_events` (fails without the gate) |
| 3 | **High** | A `bytes=0-` (or Range-less) request for a fully cached film allocated the whole file in one `Vec`, then a second copy on the socket | responses are bounded by `MAX_RESPONSE_BYTES` (4 MiB) with a truthful `Content-Range`; a large Range-less GET is a 206, never a truncated 200 | `a_request_for_the_whole_file_is_bounded_not_buffered` (fails without the clamp) |
| 4 | **High** | `broadcast::RecvError::Lagged` was treated as "channel closed", silently ending the guest's event stream while the connection stayed up | both delivery loops classify the receive; `Lagged` continues | `a_lagged_reader_is_told_to_keep_going_not_to_stop` |
| 5 | Medium | `reportPlayback` was authorized without a URL and then refused for not having one, so the one live-position capability could never work | a URL-less playback report updates the tab the user already selected; an unknown tab is `TabChanged` | `a_playback_report_updates_the_selected_tab_without_naming_a_url` + `a_playback_report_for_a_tab_nobody_selected_is_refused` |
| 6 | Medium | The MAC covered only `sequence:kind`; the native bridge (documented as untrusted plumbing) could rewrite a URL, title or position inside a signed frame | MAC now covers the canonical payload (UTF-8 byte-length prefixed, same order on both sides). Protocol bumped to **v2** | `rewriting_a_signed_payload_after_signing_is_refused`, `the_signed_payload_counts_utf8_bytes_not_characters`, `a_frame_whose_payload_was_rewritten_after_signing_is_refused` |
| 6b | Medium | The two sides of the signing protocol had no shared vector: each was tested against its own expectations, which is how the `atob` and MAC-coverage bugs shipped | one JSON vector pins both implementations (`extension/test-vectors/companion-signing.json`) | `the_shared_signing_vector_matches_the_extension` (Rust) + `extension/signing.test.js` (also cross-checked against OpenSSL) |
| 7 | Medium | Trust-model docs claimed "the browser never sees the raw secret" and "the host holds no secret" while the handshake reply returns the key and the bridge relays it | claims replaced with the truth (the extension must sign, so it holds the key; the bridge sees frames but cannot authorise) | documentation only — no code claim is now stronger than the code |
| 8 | Medium | After Chrome evicts the MV3 worker, the sequence restarted at 1 and every command was refused as stale until the user re-paired | the last sequence is persisted next to the key and restored on `BEGIN` | `resumes the sequence after the service worker is suspended and revived` |
| 9 | Medium | The software renderer allocated and copied a full RGBA frame on every 30 ms tick, including when paused | render only when `mpv_render_context_update` reports a new frame; buffer sized from live `video-params` every frame; post-render size check | real libmpv: `real_sw_render_test` and `real_native_surface_e2e` (**22 fps** in 1.2 s = the fixture's rate, not the 33 Hz tick) |
| 10 | Medium | A persistent cache-write failure requeued the chunk at Critical with no delay: a full disk meant an endless loop re-fetching a megabyte per iteration | bounded, backed-off retry (250 ms … 1 s); after five consecutive failures the transfer stops through `TransferInterrupted` | `write_retries_back_off_and_then_give_up` |
| 11 | Medium | Loopback range connections had no read timeout: eight silent clients held every permit and a blocking thread, parking the accept loop | per-connection read/write deadline (`DEFAULT_CONNECTION_TIMEOUT_MS`) | `stalled_clients_cannot_exhaust_the_connection_pool` (fails without the timeout) |
| 12 | Medium | Peer-supplied commit deadlines were unbounded; `as i64` also wrapped values above `i64::MAX` into *negative* (i.e. "the past") | saturating conversion + `MAX_HOST_DEADLINE_AHEAD_US` (30 s) ceiling | `a_peer_cannot_park_a_commit_beyond_the_deadline_ceiling` |
| 13 | Low | `chunk_index * CHUNK_SIZE` was unchecked (panic in debug, silent wrap in release) and a too-large index was stamped `u32::MAX` | `checked_mul` + `u32::try_from` refusal | `a_chunk_index_that_cannot_be_multiplied_is_an_error_not_a_panic` |
| 14 | Low | `mpv_set_option` failure returned through `?` without destroying the handle it had just created | every early return in `load_library` destroys the handle | code review (no in-process leak assertion); covered by the same real-libmpv runs |
| 15 | Low | A second, unused envelope-sequence counter (`QuicServer::broadcast`, `next_event_seq`, `broadcast_schedule_*`, `host_broadcast`) would silently corrupt the guest's watermark if ever used | deleted; `send_host_event` is documented as the only producer | compile-time (dead API removed) |
| 16 | Low | `attempts: u8` incremented without saturation; 256 bogus handshakes panicked the only thread serving the companion in a debug build | `saturating_add` | `a_flood_of_bad_handshakes_cannot_kill_the_listener` |
| 17 | Low | The app passed a constant (`MAX_FRAME_BYTES / 2`) as the received frame length, making the size gate dead | the real `bytes.len()` is passed | existing frame-size tests now exercise the real value |
| 18 | Low | `prepare_seek` moved the room to `Seeking` before checking readiness, and fabricated `execute_at_host_mono_us = 750_000` | readiness first; the caller supplies the clock | `a_refused_seek_prepare_does_not_move_the_room`, `a_ready_seek_prepare_uses_the_supplied_clock` |
| 19 | Low | A new coordinator claimed both participants were `ready(0)`; a test comment asserted the opposite | default is `not_ready("initial")` | `play_requires_ready_consensus` now asserts the default's reason |
| 20 | Low | A message handler that threw closed the port with no reply, so the popup could only report a dead connection | the rejection is answered with `MalformedFrame` + guidance | `answers a message whose handler throws instead of closing the port` |
| 21 | Low | `nonminimal_bool` in `onboarding.rs` would fail `clippy -D warnings` | `is_err()` | CI gate |
| 22 | Low | README told a developer to run `cargo test` — the one command this repo records as producing false failures | `cargo test -- --test-threads=1` with the reason | documentation |

## Protocol v2 — a deliberate break

The MAC input changed from `sequence:kind` to `sequence:kind:payload`, so a v1
extension cannot verify against a v2 app. `PROTOCOL_VERSION` was bumped to 2 on
both sides so the disagreement is reported as `VersionMismatch` (with recovery
words) instead of "every command is `NotPaired`". Consequences:

- An already-loaded unpacked extension must be **reloaded**, and the pair
  repeated, after updating.
- `companionProtocol` is stored with the key, so a stored v1 pairing is ignored
  rather than half-used.
- The app drops the session key whenever it starts, so there is no upgrade path
  that silently keeps a v1 key.

## What this does *not* claim

- **No two-device session has been run.** Nothing here changes that; findings 2,
  4, 11 and 12 were found and tested on one machine over real loopback QUIC, which
  is not the same as two machines on a tailnet.
- **Windows is still unexercised**, including the Credential Manager path and the
  NSIS build.
- **No real provider page has been loaded.** The companion is verified against a
  chrome stub (`extension/background.test.js`), not a browser profile.
- `reportPlayback` now works, but the popup still has no control that sends it —
  the capability is reachable from the service worker, not yet from the UI. That
  is a product decision, not a defect left open.
- **Dependency advisories were re-checked, not fixed.** `pnpm audit --prod` → no
  known vulnerabilities. `cargo audit` (2026-10-28, 1 380 advisories loaded) →
  exit 0, twelve warnings: nine unmaintained (all Linux/GTK or proc-macro
  transitive), one `glib` unsoundness in `VariantStrIter`, one tokio unsoundness
  in `AsyncFd` (this repository never constructs an `AsyncFd`), one yanked
  `chacha20`. The ten errors recorded against `quinn-proto` in the earlier audit
  are not present in this lockfile. None of these is a known vulnerability, and
  none is reachable from Movie Party's own code; clearing the unmaintained set
  needs upstream Tauri releases.

## Commands used

```bash
pnpm test                       # 44 files / 477 tests incl. the new extension suites
cargo fmt --check
cargo clippy --all-targets --all-features
cargo test --lib -- --test-threads=1
cargo test --test real_sw_render_test -- --test-threads=1 --nocapture
cargo test --test real_native_surface_e2e -- --test-threads=1 --nocapture
```

Two flips were used to prove the new tests catch the old behaviour: the pre-auth
gate was disabled (`if false && …`) and the range-response clamp removed; the
corresponding tests failed, then passed again once restored.
