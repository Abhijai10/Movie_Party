# Batch 4 — Network/security hardening and rustls update

**Status:** complete, verified. `v0.9.8` (`bb22577`) untouched — no tag created or moved, no release
edited, no force-push. Provider Shared untouched. No broad dependency upgrade. No unrelated cleanup.

**Exact rustls version after this batch: `0.23.45`** (was `0.23.43`).

---

## 1. Findings verified

Every finding was confirmed against the real code before anything changed. Where a fix could be
reverted, it was, so the finding is *reproduced* rather than argued.

### RUSTSEC-2026-0285 — CONFIRMED

`Cargo.lock` resolved `rustls 0.23.43`. It is both a direct dependency of `movie-party` and transitive
via `quinn 0.11.11` / `quinn-proto 0.11.16`:

```
rustls v0.23.43
├── movie-party v0.9.8 (src-tauri)
├── quinn v0.11.11
└── quinn-proto v0.11.16
```

The advisory (GHSA-2mjx-qc3c-rqvc, CVSS 5.3 `AV:N/AC:L/PR:N/UI:N/S:U/C:L/I:N/A:N`) is that rustls
accepted TLS 1.3 handshake messages sent at the wrong encryption level when they followed a
key-changing message in the same record — a plaintext `EncryptedExtensions` packed into the same record
as the `ServerHello` was accepted, where RFC 8446 §5.1 requires termination. Patched `>=0.23.45`;
`<0.23.13` unaffected. **0.23.43 is affected.**

### MP-14 — CONFIRMED: received messages were not held to the local rules

`chat/mod.rs` already had the rules (`MAX_CHAT_BODY_BYTES = 2_000`, `validate_chat_message`,
`validate_reaction`, `ReactionRateLimiter`), and the *local* send paths applied them
(`host_send_chat`, `send_chat_message`, `host_send_reaction`, `send_reaction`). The receive paths did
not:

- `network/quic.rs`, `handle_request`, `ClientRequest::ChatMessage` broadcast the guest's `body`
  **verbatim**. The only bound was the framing limit — `MAX_CONTROL_FRAME_BYTES = 256 KiB` — i.e.
  **128× the local cap**. `message_id` was an unvalidated `String`.
- `ClientRequest::Reaction` was **rate-limited but never value-checked**, so any string reached the
  room as a "reaction".
- `app_runtime.rs`, `apply_peer_event` pushed both straight into `state.chat` / `state.reactions`
  with no check at all, and both are plain `Vec`s — **unbounded**, and cloned into every emitted
  snapshot.

So a peer could inject a 256 KiB chat body and an unlimited history, while the local UI could not.

### MP-16 — CONFIRMED: expiry existed only on the guest side

`room/invite.rs` enforces `expires_at_ms` in `parse_invite` — but that runs on the **joiner's** machine,
against the **joiner's** clock. `network/quic.rs::validate_handshake` had no expiry check, and
`QuicServer` never received the host's own expiry: `bind`/`bind_with_local_media` took only
`credentials`, display name and device id. The host had no idea its invite had lapsed.

### MP-18 — CONFIRMED: no authenticated-peer cap

`QuicServer::run` is `while let Some(incoming) = self.endpoint.accept().await { tokio::spawn(...) }` —
one task per accepted connection, with nothing counting or capping them.

### MP-19 — CONFIRMED and REPRODUCED: userinfo bypassed the destination check

`providers/sync.rs::host_from_url` split the authority by hand:

```rust
let end = without_scheme.find(['/', '?', '#']).unwrap_or(without_scheme.len());
let host = &without_scheme[..end];
```

`bare_host` then split on `:`. For `https://x@127.0.0.1:8080/` that yields the host `x@127.0.0.1`,
which is not an `IpAddr`, contains a dot, and is not all digits — so `is_public_destination` returned
`true` and a **loopback URL was accepted into the managed Chrome instance**.

**This was not a provider-domain bypass, and the report should not claim it was.** The provider
allowlist compared the raw authority text, so `https://netflix.com@evil.example/` failed closed.
Confirmed empirically: the negative control (below) restores the old parser and the provider-allowlist
test **still passes**.

### MP-15 — CONFIRMED, and sharper than "weaker than ideal"

`hello.device_id` and `hello.public_key` were both peer-asserted with nothing binding them, and the
signature's canonical message includes `device_id` — so a peer could present *any* device id with a key
it controls.

The practical impact is worse than impersonation-in-general. In `app_runtime.rs`,
`local_participant.id` **is** the host's `device_id` (`id: identity.device_id.clone()`), and
`apply_peer_event` **skips the room-binding and sequence checks** for the local participant's id:

```rust
if sender != &state.local_participant.id {
    if envelope.room_id != credentials.room_id { return; }   // skipped for the host's id
}
```

A guest claiming the host's device id would therefore speak with the host's authority *and* bypass
those two guards.

---

## 2. Fixes implemented

### rustls 0.23.43 → 0.23.45 — the smallest compatible change

```bash
cargo update -p rustls --precise 0.23.45
```

**Lockfile only.** `Cargo.toml` still reads `rustls = "0.23"`, which 0.23.45 satisfies — so no manifest
edit and no unrelated bump. The whole graph was verified by diffing name+version sets before and after:

> **521 package names before, 521 after. Zero added, zero removed. `rustls` is the only version that
> changed.**

One extra diff line (`tempfile`'s `getrandom` edge, `0.4.3` → `0.3.4`) is **not** caused by this bump.
Proved by a control: pinning rustls to its *current* `0.23.43` — a no-op version change — flips that
same line, because `tempfile 3.27.0` declares `getrandom >=0.3.0, <0.5` and cargo re-resolves the edge
whenever it touches the lockfile. Both `getrandom` versions were already in the graph; no crate version
changed and no new code entered the build.

`url = "2"` was added as a **direct** dependency for MP-19. It is already in the graph transitively via
`tauri`, so this likewise adds no new code — the same reasoning already documented in `Cargo.toml` for
`libc` / `windows-sys`.

### MP-14 — validate received messages, and bound the history

- `chat/mod.rs`: `MAX_CHAT_HISTORY` / `MAX_REACTION_HISTORY` (200 each); the body and reaction rules
  extracted into `validate_chat_body` / `validate_reaction_token` so send and receive share one
  definition; new `validate_received_chat_message` / `validate_received_reaction` adding the id-shape
  checks (new codes `MP-CHAT-005` invalid message id, `MP-CHAT-006` invalid reaction id); new
  `push_bounded`.
- `network/quic.rs`: the host validates at the **transport boundary** and answers with new
  `ServerResponse::ChatRejected` / `ReactionRejected` carrying the `MP-CHAT-xxx` code, instead of
  relaying. A rejected message is never broadcast.
- `app_runtime.rs`: `apply_peer_event` validates **again** on receipt — the boundary is the
  authoritative rejection point, but this is what protects the *applied* state, which must hold even if
  an event arrives by another route. All four push sites (2 receive, 2 local) now go through
  `push_bounded`.

### MP-16 — enforce expiry at the host authorization boundary

`QuicServer::with_invite_expiry(expires_at_ms)` carries the host's own expiry into
`validate_handshake`, which rejects with `INVITE_EXPIRED`. The value is the one the host generated for
its own invite, so **a joiner cannot extend it by lying** — which is what makes host-side enforcement
meaningful rather than a second opinion from the same untrusted party.

Deliberately placed **behind** the join-secret comparison, so an unauthenticated peer learns nothing
about the invite's state. `app_runtime.rs` now computes `invite_expires_at_ms` once and uses it for
*both* the invite it hands out and its own check, so the two can never disagree.

### MP-18 — an explicit, race-safe cap of one authenticated peer

`AuthenticatedPeers` holds a single `Option<(device_id, generation)>` behind a mutex, with an RAII
guard. `handle_request` claims it under the session lock, so a connection claims at most once, and a
failed claim converts an `AuthAccept` into `AuthReject { SERVER_FULL }`.

**Keyed on device id, deliberately.** A re-authentication from the *same* peer is a **transport
replacement**, not a second peer — that is exactly the documented reconnect path
(`heartbeat_room_state` is "used after an authenticated transport replacement"), so a cap that refused
it would have broken reconnects. The guard carries a generation so a replaced connection's stale guard
cannot release its replacement's slot. Released explicitly at connection teardown, with `Drop` as the
backstop.

The cap is tied to its representation at compile time, so raising it cannot silently do nothing:

```rust
const _: () = assert!(
    MAX_AUTHENTICATED_PEERS == 1,
    "AuthenticatedPeers stores exactly one slot; change the representation before raising the cap"
);
```

### MP-19 — parse with a real URL parser

`parse_http_url` uses `url::Url` and requires an `http`/`https` scheme and a host; the **parsed** host
is what gets validated. Userinfo, host and port are separated structurally, so they cannot be mistaken
for one another. `is_public_destination` now matches on `url::Host` (Domain / Ipv4 / Ipv6) instead of a
string, and the special case for numeric shorthands is gone — the WHATWG parser normalises `127.1`,
`2130706433`, `0x7f.1` and `0177.0.0.1` to an `Ipv4` host, so they are judged as the address they
denote. The all-digits-and-dots check is kept as belt and braces, now explicitly documented as
unreachable.

Provider allowlist semantics are **unchanged**: it still matches on the true host, and a literal
address can never match a provider domain.

### MP-15 — minimal identity binding, no auth redesign

Two additive checks in `validate_handshake`:

1. A guest may not present the host's own device id (`DEVICE_ID_IS_HOST`).
2. `AuthReplayGuard::bind_identity` binds a `device_id` to the first public key seen for it
   (`IDENTITY_KEY_MISMATCH`), so a holder of the join secret cannot re-use an established device id
   under a fresh key.

The binding runs **after** signature verification — that is what proves the peer controls the private
key for that device id, so it cannot be pre-empted with an arbitrary key. **The join secret and the
replay protections are untouched**: no protocol change, no change to the canonical signed message.

### TLS configs split out for direct assertion

`configure_server` / `make_client_endpoint` were split into `server_crypto_config` /
`client_crypto_config` so the TLS *policy* (0-RTT, ALPN) can be asserted directly rather than only
inferred from behaviour.

---

## 3. Tests added (29)

| File | Added | Covers |
|---|---|---|
| `src/network/quic.rs` | 12 | MP-16 (unit + 2 end-to-end), MP-18 (slot semantics + 2 end-to-end), MP-15 (2 unit + 1 end-to-end), MP-14 boundary, TLS 0-RTT policy, ALPN |
| `src/chat/mod.rs` | 6 | received-message id/body rules, received-reaction rules, `push_bounded` ordering + zero capacity |
| `src/app_runtime.rs` | 4 | received chat/reaction rejection, bounded chat history, bounded reaction history |
| `src/providers/sync.rs` | 4 | userinfo cannot masquerade as a public destination; parsed-host provider matching; numeric shorthands; no unintended narrowing |
| `tests/dependency_audit.rs` (new) | 3 | rustls resolved version `>= 0.23.45`; one resolved copy per rustls-family crate; the lockfile parser is not vacuous |

The rustls test reads **`Cargo.lock`**, not the manifest — a requirement of `rustls = "0.23"` can still
resolve to an affected crate if the lockfile is stale, and the resolved version is the one that ships.

### Negative controls (a test that cannot fail proves nothing)

Each mechanism was disabled, the matching test observed **failing**, then restored with hashes verified
by `shasum`.

- **MP-19** — restoring the naive splitter fails `mp19_userinfo_cannot_masquerade_as_a_public_destination`
  on the exact reported URL, and fails the numeric-shorthand test on `0x7f.1`. **This is what
  reproduces the finding.** The control also **disproved a claim I had written**: the old parser
  accepted `https://x@example.com/` too, so the comment asserting otherwise was corrected.
- **MP-14** — removing the receive-path validation *and* the bound fails 3 of 4 runtime tests with the
  intended messages. (The 4th calls `push_bounded` directly, as its own doc comment states.)
- **MP-15 / MP-16 / MP-18** — all four mechanisms disabled fails **exactly the 7 enforcement tests**;
  the 2 "must still work" tests (`mp16_host_accepts_a_join_inside_its_invite_window`,
  `mp18_the_same_guest_may_replace_its_own_transport`) correctly keep passing, which is what shows the
  new checks are not blanket refusals.
- **rustls** — raising the threshold to `0.23.46` fails and reports the resolved version as `0.23.45`,
  proving the test reads the lockfile rather than trusting a constant.

### A real bug the tests caught — in this batch's own code

`push_bounded` originally returned early when `max == 0`, leaving the history **unbounded**. A
mis-set bound would have failed silently **open**. It now clears, so a zero bound fails *visibly
empty*. Found by `push_bounded_with_zero_capacity_stores_nothing`, which I wrote expecting the correct
behaviour and which was red on first run.

### One existing fixture changed, deliberately

`tests/m3_closure.rs::reconnect_reuses_single_session_worker` called
`DeviceIdentity::new_ephemeral()` **inside** its helper, so its "replacement" connection was a second,
*different* peer — which MP-18 now correctly refuses. Production uses the **persisted** identity for
both join and reconnect (`RuntimeInner::identity()`), so the fixture was changed to match: one
identity, passed in. The test's actual subject (guest-side worker replacement) is unchanged, and its
assertions were not weakened.

---

## 4. Validation

Rust toolchain **pinned to `1.98.0`** to match CI.

| Gate | Result |
|---|---|
| `cargo +1.98.0 fmt --check` | **clean** |
| `cargo +1.98.0 clippy --all-targets --all-features -- -D warnings` | **clean** |
| `cargo +1.98.0 test --no-fail-fast -- --test-threads=1` | **565 passed / 1 failed** |

Per-target: lib `479 passed / 0 failed / 2 ignored` · dependency_audit 3 · host_guest_wiring 2 ·
m2_integration 28 · m3_closure 7 · m3_integration 18 · m3_m4_e2e 9 · m4_closure 15 ·
real_native_surface_e2e 1 · real_playback_smoke_test 1 · tailscale_probe 2 ·
real_sw_render_test **0 passed / 1 failed** · windows_native_surface_e2e 0 · doc-tests 0.

**The single failure is pre-existing and A/B-verified this batch.** All Batch 4 changes were stashed
(`git stash push`, files backed up to `/tmp` first) and `real_sw_render_test` re-run on the pristine
tree: **identical failure, identical line** (`real_sw_render_test.rs:292`, *"rendered frame buffer must
contain non-zero pixel data"*). The stash was popped and all eight file hashes re-verified as matching.
`real_native_surface_e2e` — the known flaky one — passed on this run.

### Security properties explicitly re-checked

| Property | Evidence |
|---|---|
| TLS 1.3 still enforced | quinn's TLS-1.3-only `QuicServerConfig`/`QuicClientConfig` conversions succeed; ALPN `movieparty-v1` asserted from live `handshake_data` |
| `CertificateVerify` still enforced | `f44_a`, `f44_c`, `f44_d` pass |
| Fingerprint pinning still enforced | `f44_b`, `rejects_server_with_wrong_certificate_fingerprint` pass |
| App-layer join-secret auth still enforced | `rejects_wrong_join_secret`, `rejects_replayed_auth_nonce`, `rejects_malformed_join_secret_hash` pass |
| No 0-RTT / resumption accidentally enabled | **new** `tls_policy_disables_zero_rtt_on_both_sides`: `max_early_data_size == 0`, `!enable_early_data` |

### Toolchain trap (cost one re-run)

Bare `cargo` on this machine is **1.96.1**, not the CI-pinned **1.98.0**. The first gate run therefore
used the wrong toolchain. Everything was re-run with `cargo +1.98.0 …` — identical results (565/1, fmt
and clippy clean), so nothing changed, but **the numbers above are the `+1.98.0` ones.**

---

## 5. Remaining risks

1. **The frontend gates are RED in this environment — and it is not Batch 4.** `tsc --noEmit` 3 errors,
   `eslint . --max-warnings=0` 61 errors, `vitest run` 6 failed suites (218 tests passed, 69 never
   loaded), `vite build` fails after 2m06s. **One root cause**: the environment's fs shim refuses
   exactly one file, `@tauri-apps/api/core.js`/`core.d.ts` — `vite build` transforms **2280 other
   modules fine**. The shim brokers reads to the host with a **125 s** timeout
   (`BROKER_FILE_READ_DEFAULT_TIMEOUT_MS = 125_000`); the first access timed out (my first `tsc` run
   was SIGTERM'd with no output), and the session then **caches the denial**, so retries fail fast with
   *"Do not retry the same operation during this request"* — confirmed futile (`vitest` ×2,
   `vite build` ×2, byte-identical). `dangerouslyDisableSandbox` does not help (the shim is injected
   via `NODE_OPTIONS`, so it survives an OS-sandbox bypass). **Batch 4 modifies zero frontend files** —
   `git status` shows only `src-tauri/**` plus the root `Cargo.lock` — so it cannot be the cause, and
   `dist/` was built successfully at 17:33 the same day. The fix is a user action (approve the prompt
   or restart the session); the owner was offered a shim bypass and **chose to accept the gates as
   unverified instead**, so nothing was bypassed.
2. **Pre-auth concurrency is still uncapped.** MP-18 caps *authenticated* peers, as specified. A
   hostile peer can still open many pre-authentication connections; each costs a handshake and a task
   until the transport idle timeout.
3. **A lingering connection from a *different* device id holds the slot** until the transport idle
   timeout. Intended ("one guest at a time"), but worth knowing when a guest switches machines.
4. **The guest's optimistic echo.** `send_chat_message` appends locally *before* sending, so a host
   rejection is not surfaced to the sender. The host is authoritative and does not apply the message.
5. **`created_host_time_us` from a peer is accepted as-is.** Only the id and body/reaction are
   validated; the timestamp is not sanity-checked against the host's own clock.
6. **MP-15 residual.** The device_id→key map clears at 4 096 entries, matching the existing nonce
   policy. Reaching that requires the join secret plus 4 096 successful authentications against a
   one-peer cap.
7. **Windows runtime behaviour is unverified** (as with every batch): the crate cannot be
   cross-compiled here because `aws-lc-sys`' C build needs the Windows SDK. No Windows-only code
   changed in Batch 4.

---

## 6. Confirmation of the release freeze

- `v0.9.8` → **`bb22577`** — unchanged. `git rev-list -n1 v0.9.8` returns `bb22577`.
- All five tags intact: `v0.9.0`, `v0.9.4`, `v0.9.5`, `v0.9.6`, `v0.9.8`.
- `origin/main` → **`bb22577`**. **Nothing was pushed.** `git log origin/main..HEAD` lists two local
  commits only.
- No force-push, no reset: `git reflog show main` shows ordinary commits only.
- **Provider Shared untouched.** `provider_mode_gate` and the experimental `PROVIDER_SHARED` path are
  unmodified, as are `media/shared_pipeline.rs`, `media/local_perfect.rs` and `media/shared_stream.rs`.
  Nothing was implemented or enabled.

### Files changed by Batch 4

```
 Cargo.lock                        |   7 +-
 src-tauri/Cargo.toml              |   5 +
 src-tauri/src/app_runtime.rs      | 249 ++++++++--
 src-tauri/src/chat/mod.rs         | 171 ++++++-
 src-tauri/src/network/quic.rs     | 984 +++++++++++++++++++++++++++++++---
 src-tauri/src/providers/sync.rs   | 205 +++++++--
 src-tauri/tests/m3_closure.rs     |   +-   (fixture: one identity per reconnect)
 src-tauri/tests/dependency_audit.rs | new
 BATCH4_REPORT.md                  | new
```

Committed locally as one commit — `app_runtime.rs` carries MP-14/15/16/18 together, so per-finding
splitting would produce commits that do not individually compile and `git add -p` is not drivable
non-interactively here. Same structure and same reasoning as Batches 1–3.

### Side effects

- `/tmp/Cargo.lock.batch4.bak`, `/tmp/{sync.rs,app_runtime.rs,quic.rs,dependency_audit.rs}.batch4`,
  `/tmp/batch4_backup/**` — scratch backups used for the A/B and the negative controls. All verified
  byte-identical to the working tree via `shasum` after each restore. Nothing in the repo depends on
  them; they can be deleted.
- `/tmp/rustls_idx.json`, `/tmp/tf.json` — crates.io sparse-index responses used to check the advisory
  range and `tempfile`'s declared requirement.
- A `git stash` was created and popped during the A/B (`stash@{0}`, "batch4-ab"); it was dropped by
  `stash pop`, leaving no stash behind.
- `node_modules` was inspected after a failed `pnpm build` and is **intact** (253 `.pnpm` entries,
  package files still dated 2026-09-10, nothing rewritten). `dist/` is **untouched** (same files, same
  17:33 timestamps) — the failed `vite build` died during module resolution, before vite's write phase.
- `~/.workbuddy-ai/skills/movie-party-verify/SKILL.md` was corrected: its §4b claimed the
  `@tauri-apps/api/core` block was "gone". It is intermittent, so the skill now records the 125 s
  approval-timeout mechanism, that retries are futile, and that the fix is a user action.
