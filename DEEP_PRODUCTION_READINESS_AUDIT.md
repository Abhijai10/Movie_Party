# Deep Production-Readiness Audit — Movie Party v0.9.9-rc1

**Audit type:** read-only. No source, test, CI, or documentation file was modified. No commit, no push, no tag, no release.
**Auditor:** Vera
**Date:** 2026-09-20
**Verdict (short):** **Do not ship as "production-ready", and do not claim two real users can watch a real movie together.**
There is a **P0** defect on the guest playback path that makes sustained synchronized local playback impossible, and it is
structurally invisible to every automated gate in the repository.

---

## 1. Exact baseline

All values read from the working tree at audit time. Nothing assumed.

| Item | Value |
|---|---|
| Branch | `stabilization/v0.9.9-rc1` |
| HEAD | `e19c2eb729de8a2267d2110c4b31c2a1b8592bd6` |
| HEAD subject | `docs(Batch 7A): name every test target in the CI report tables` |
| `origin/main` | `bb225778434e3f07923e03e85d7d7cc1db79146c` |
| Working tree | **clean** (`git status --porcelain` empty) |
| CI-verified SHA (remote candidate tip) | `a44542af0f0f286c9b387c23d72692a56b966a7a` |
| Local commits not pushed | `e19c2eb`, `ef7d6de` (branch `stabilization/v0.9.9-rc1`); `2f1520f` (branch `archive/freebuff-pre-canonical`) |
| `main` relationship | strict ancestor of the candidate branch — promoting v0.9.9 remains a clean fast-forward |

**Tags (dereferenced to commits):**

| Tag | Commit |
|---|---|
| `v0.9.0` | `31c5d4ef2c9e0ebf2b51eb1c4235bbb880b63640` |
| `v0.9.4` | `757cd33b1882177aee714eda7bb99b93e6a72285` |
| `v0.9.5` | `10b321ac21c3545430b82c663c4b6d24b3dae982` |
| `v0.9.6` | `f273092be25b88e5f4f24f946f3bb35f13195af5` |
| `v0.9.8` | `bb225778434e3f07923e03e85d7d7cc1db79146c` |

`v0.9.8` is an **annotated** tag; `git show-ref` reports the tag object, and it dereferences to `bb22577`. All five tags unchanged.

**Application version — four declarations, all consistent:**

| Declaration | Value |
|---|---|
| `package.json` | `0.9.8` |
| `src-tauri/tauri.conf.json` | `0.9.8` |
| `src-tauri/Cargo.toml` | `0.9.8` |
| `Cargo.lock` (`movie-party`) | `0.9.8` |

**Rust toolchain:** pinned via `rust-toolchain.toml`; all local work was run with `cargo +1.98.0`.
**`rustls` resolved version:** `0.23.45` (the Batch 4 bump; `cargo audit` now passes, having failed at v0.9.8).
**Provider Shared state:** `shared_available: false` by design. `media/shared_pipeline.rs` is **not declared** in
`media/mod.rs` — it is not compiled at all (see AUD-08).

**Confirmation the audit targeted the candidate, not v0.9.8:** HEAD is `e19c2eb` on `stabilization/v0.9.9-rc1`, which is
`bb22577` + the Batch 1–6 stabilization commits + 2 docs commits. Every finding below was read from this tree.

---

## 2. Architecture audit

Rust backend (`src-tauri/src`, ~35k lines) owning all state, pushing an `AppSnapshot` to a React 19 frontend that renders
screens and calls back through a single wrapper (`src/backend/appRuntime.ts`).

**Module map (line counts read directly):**

- `app_runtime.rs` — the god-object: state, all Tauri command bodies, all background tasks (~11.7k lines)
- `sync/{local,drift,mod}.rs` — `LocalSyncCoordinator`, room state machine, drift policy
- `network/{quic,tailscale,bandwidth,diagnostics}.rs` — QUIC transport, auth, peer discovery
- `room/invite.rs` — invite encode/parse/validate
- `identity/mod.rs` — device keys, canonical auth message, signatures
- `media/{player,manifest,cache,stream,transfer,local_perfect,shared_pipeline}.rs` — playback, chunked transfer
- `media/player/{mod,mpv_backend}.rs` — `LocalPlayer` trait, real libmpv backend via raw FFI
- `providers/chrome/{mod,process}.rs` — managed Chrome + CDP for provider sync
- `storage/sqlite.rs` — schema + migrations
- `protocol/mod.rs` — envelope versioning

**Trust boundaries identified:** (1) invite URL → credentials; (2) QUIC handshake → pinned cert + CertificateVerify;
(3) auth request → signature + replay guard + identity binding; (4) media file path → player; (5) provider URL → Chrome;
(6) remote peer → `ServerEvent` decode; (7) SQLite file; (8) Chrome subprocess tree.

**Async boundaries:** 61 `.abort()` sites; every named task is `take()`n and aborted on teardown. Tasks are
single-slot fields (`player_event_task`, `heartbeat_task`, `buffer_status_task`, …), and re-spawning replaces + aborts the
predecessor — so duplicate loops are structurally prevented. This is genuinely well built.

**Contradiction found (documentation vs code):** `spawn_player_render_loop` is documented as stopping "when no frame is
available (e.g. paused or headless)" (`app_runtime.rs:5886-5887`), but it breaks only when `state.player` is `None`
(`5897`). A paused player keeps the 30 ms loop spinning. Benign, but the comment misleads a reader into believing the loop
self-throttles.

---

## 3. Real movie playback audit — **the critical finding**

### 3.1 What was traced

`pick_media_file` → validation → path transport → `state.player = Some(...)` (`app_runtime.rs:2994/3008` host,
`6985/6996` guest) → `spawn_player_event_loop` (`3021`, `7172`, gated on `player.is_some()`) → `MpvPlayer`/`LibMpvPlayer`
(`media/player/mpv_backend.rs`) → raw libmpv FFI → SW render (`vo=sw`, `bgr0`) → `spawn_player_render_loop` (30 ms) →
`time-pos` read back in `snapshot()` (`mpv_backend.rs:551-564`) → position/duration/buffering into the snapshot.

### 3.2 The fixture is not a movie

`src-tauri/tests/fixtures/movie_party_test_320x240.mp4` was parsed box-by-box (no `ffprobe` on this machine):

- `ftyp`/`moov`/`trak` present; exactly **one `hdlr` = `vide`**
- **No `soun` handler, no `mp4a`, no `ac-3`, no `ec-3` marker anywhere in the file**
- H.264 (`avc1`), **320×240**, `stsz.sample_count = 90`, `stss` keyframes present, duration ≈ **3 s**

So the only media any automated test has ever decoded is a **3-second, 90-frame, silent, 320×240** clip. The Batch 5
observation that it has no audio is **confirmed by container parse**, not inferred.

### 3.3 Audio: never configured, never exercised

`mpv_set_option` is called for exactly three things (`mpv_backend.rs:240-242`): `msg-level`, `quiet`, `terminal`. There is
**no `ao`, no `audio-device`, no `audio-exclusive`** configuration. `set_volume` exists (`509-527`) and writes mpv's
`volume` property, and `set_playback_rate` writes `speed` (`528-548`).

The only test that drives real libmpv (`tests/real_playback_smoke_test.rs`) explicitly sets **`ao=null`** (line 104) and
**`vo=null`** (line 98). It proves decoding, seek and pause — it proves **nothing about audio output or on-screen
rendering**. Its own header says so.

**Audio synchronization is not merely unproven; no code path in the repo has ever produced a sound.**

### 3.4 End-of-media: not implemented at all

- `PlayerState` (`media/player/mod.rs`) has variants `Stopped | Ready | Playing | Paused | Buffering | Seeking | Error` —
  **no `Completed`/`Ended`**.
- Crate-wide search for `eof-reached`, `end-file`, `keep-open`, `idle-active`, `playback-complete`: **zero hits**.
- No comparison of `position_ms` against `duration_ms` anywhere.
- `RoomState::Ended` is set in exactly one place (`app_runtime.rs:6372`), inside the **user-initiated end-party
  teardown** — never by the media finishing.

**Consequence:** when a movie ends, mpv goes idle, `time-pos` stops advancing, and the room stays in `Playing` forever.
There is no "the movie finished" transition, no ENDED room state from playback, and no host/guest coordination at the end.
For a product whose stated purpose is watching movies, **the end of a movie is an unhandled state.**

### 3.5 Proven vs. apparently-correct

| Capability | Status |
|---|---|
| Decode H.264 / seek / pause (real libmpv) | **Proven locally only** (`real_playback_smoke_test`, macOS, staged dylib, `vo=null`, `ao=null`) |
| SW render to pixel buffer | **Proven locally only** (`real_sw_render_test`, ≥10 pixel-bearing frames required) |
| Native surface presentation | **Proven locally only** (`real_native_surface_e2e`) |
| Audio output / A-V sync | **Never exercised** |
| H.265 / MKV / high bitrate / large file / VFR | **Never exercised** |
| Long movie (2 h) | **Never exercised** |
| End of media | **Not implemented** |
| Two-device playback | **Never exercised** |

### 3.6 `buffered_ahead_ms` is always `None` for mpv

`mpv_backend.rs:577-579` returns `None` unconditionally. The event loop therefore always falls back to the sparse-cache
estimate (`app_runtime.rs:5810`). Not a defect, but it means libmpv's real demuxer cache is discarded in favour of a
whole-file byte-fraction approximation.

---

## 4. Two-device synchronization audit — **P0 found**

### 4.1 The drift mechanism, as built

`app_runtime.rs:5794-5841` (inside `spawn_player_event_loop`, poll every 200 ms):

```rust
if Self::is_host_role(&state) {
    // The host owns the canonical position.
    state.sync.position_ms = snap.position_ms;
} else if !state.sync.strict_sync_paused
    && state.room_state == RoomState::Playing
{
    correction = Some((
        snap.position_ms as i64 - state.sync.position_ms as i64,   // drift
        snap.position_ms,
    ));
}
...
if let Some((drift_ms, position_ms)) = correction {
    Self::apply_drift_correction(&player_arc, drift_ms, position_ms);
}
```

`apply_drift_correction` (`5860-5881`) maps drift through `sync/drift.rs`:
`0..=80` → `Ignore` (rate 1.0) · `81..=250` → rate 0.97/1.03 · `251..=700` → `MicroSeek` back · `>700` → `HardSeek` back.

So the guest's drift is `its own advancing position − state.sync.position_ms`.

### 4.2 The guest's anchor is frozen

Every write to `state.sync.position_ms` on the guest:

| Site | Trigger | Advances with playback? |
|---|---|---|
| `4183` | `PlayCommit` → `target_position_ms` | **no — fixed anchor** |
| `4256` | `PauseCommit` | no |
| `4339` | `SeekCommit` | no |
| `4383` | `BufferLow` | no (and sets `strict_sync_paused = true`) |
| `4477-4483` | `CoordinatorStateUpdate`, **only `if position_ms == 0`** | one-shot |
| `4491` | `RoomStateUpdate` | — **never received** (see 4.3) |

There is **no periodic host→guest position broadcast**. `ServerEvent` (`network/quic.rs:361-475`) is the complete host→guest
vocabulary: `Play/Pause/Seek Prepare+Commit`, `BufferLow/Recovered`, `RoomStateUpdate`, `CoordinatorStateUpdate`, chat,
reactions, control, `SyncError`, call signals, schedule events. Only `RoomStateUpdate` carries a host position — and
`CoordinatorStateUpdate` carries `buffer_ahead_ms` but **no position**.

The coordinator's `host_position_ms` (`sync/local.rs:41`) is likewise only written at commit/prepare/seek/buffer events
(`274`, `287`, `294`, `314`); `update_position` (`293`) is called only from the seek path (`346`, `351`). It never
advances during steady-state playback.

### 4.3 The one sender of `RoomStateUpdate` is dead code

`host_relay_ready_state` (`app_runtime.rs:5242`) is annotated `#[allow(dead_code)]` and — verified by crate-wide
grep — **has no caller anywhere**. It is the only production constructor of `QuicServerEvent::RoomStateUpdate`
(line `5255`); the only other occurrence is a test (`network/quic.rs:3995`). `git log -S` shows it has been uncalled since
the initial snapshot commit `f3e9a8c`.

**The host→guest position channel is unwired.**

### 4.4 Consequence — AUD-01 (P0)

`snap.position_ms` genuinely advances (read from mpv `time-pos`, `mpv_backend.rs:555-556`), while the guest's anchor does
not. Therefore the computed "drift" is **not drift — it is elapsed time since the last commit**, and it grows at
~1000 ms/s. The thresholds then fire on a schedule:

- ~80 ms after commit → rate forced to 0.97 (guest deliberately slowed)
- ~250 ms → `MicroSeek` back to the commit position
- **~0.7 s → `HardSeek` back to the commit position, every poll**

The guest plays roughly a third of a second, is seeked back, and repeats. **A guest cannot watch a movie.** Every
mechanism downstream is affected: repeated seeks churn the decoder, and each correction holds the player lock
(`5866-5868`) while the 200 ms event loop and 30 ms render loop both contend for it.

This is source-level proof, not a runtime observation — but the arithmetic is unconditional and the branch is definitely
taken on a guest (verified: `is_host_role` is `local_participant.role == "Host"`, and the guest sets `"Guest"` at
`3198`/`3310`; `commit_play` fires `RoomState::Playing` at `sync/local.rs:276`, and the guest applies it via
`apply_coordinator_state`, which maps `"PLAYING"` → `RoomState::Playing`).

### 4.5 Why no gate catches it

- **CI**: `libmpv.dylib` is a gitignored build artifact CI cannot build, so all four `real_*` targets report
  **0 tests executed** on both platforms (confirmed: 0 passed / 0 failed / 0 ignored in run `35474284919`). Without real
  libmpv, `handle` is `None`, `snapshot()` returns the stored snapshot, **position never advances**, drift stays `0`, and
  the correction is a harmless `Ignore` → rate 1.0.
- **Integration tests**: `spawn_player_event_loop` and `apply_drift_correction` are **never referenced by any file in
  `tests/`** (verified by grep). `test_b_play_transitions_both_to_playing` asserts `guest.position_ms == host.position_ms`
  *immediately after commit* — the one instant they are equal by construction (`m2_integration.rs:216-220`). No test
  advances time and re-checks.
- **Unit tests**: `drift_correction_restores_normal_rate_after_convergence` and
  `drift_correction_hard_seeks_back_to_host_commit` (`app_runtime.rs:9744`, `9771`) call `apply_drift_correction`
  **directly with hand-picked drift values**. They prove the *mapping* drift→correction; they cannot see a wrong drift
  *argument* at the call site. That is a test that cannot fail for the bug that exists.
- **Real tests**: `real_playback_smoke_test.rs` is a standalone raw-FFI harness — it never constructs the app's
  `MpvPlayer`, never spawns the event loop, and sets `vo=null`/`ao=null`.

So: **no test at any level exercises the drift loop with an advancing position.**

### 4.6 Everything else in the sync state machine

The parts that *are* covered read correctly: the `last_committed_operation_id` guard against replay (`4149`), the
superseded-operation guard (`4154-4158`), `session_generation` invalidation of sleeping commit tasks (`4169`, `4175`),
`pending_operations` bookkeeping, and `strict_sync_paused` on `BufferLow`/disconnect with **no auto-resume**
(`sync/local.rs:337-341`, spec §30). Clock-offset projection (`instant_for_host_mono`) is used rather than raw
wall-clock for execution. Peer-loss recovery is role-aware (`3748-3764`).

**Hours-long correctness cannot be assessed** because the steady-state position channel is missing: there is nothing to
drift *against* over time.

---

## 5. Windows / macOS cross-platform audit

**Every `cfg` site enumerated** (crate-wide): `providers/chrome/process.rs` (unix/windows), `providers/chrome/mod.rs`
(unix/windows), `media/player/mpv_backend.rs` (macos-gated tests).

**Windows process lifecycle** (`providers/chrome/process.rs:363-465`) is well built: a job object with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, created unnamed with no security attributes, `AssignProcessToJobObject` on the
browser (which enrols all *future* children), `TerminateJobObject` scoped to the job, `CloseHandle` exactly once in
`Drop`. The safety comments are accurate and the "cannot reach the user's own Chrome windows" claim is structurally
correct — the job is private. `install_shutdown_hardening()` is a deliberate no-op on Windows because the job object
covers forced kills.

**Batch 1 fix (`test_b`)**: the fix is real and now verified. `test_b_play_transitions_both_to_playing ... ok` on the
**real Windows runner** (run `35474284919` @ `a44542a`, 553 passed / 0 failed). The preceding run `35472905806` @
`2d5c833` failed at Windows **clippy** — two unused test-only imports (`mpv_backend.rs:772`,
`providers/chrome/mod.rs:641`) used only by macOS/unix-gated code — and because clippy fails *before* the test step, the
Tests step was **skipped** and no Windows test ran. **The verified SHA is `a44542a`, not `2d5c833`.**

**Separation of verification levels — this is the honest answer:**

| Level | Windows status |
|---|---|
| Source-level correctness | Reviewed; cfg gating is now consistent (the fix gated each import with its only use) |
| Compile-level | **Verified** — `cargo clippy --all-targets --all-features -D warnings` passes on windows-latest |
| CI verification | **Verified** — 553 passed / 0 failed, 5/5 jobs green at `a44542a` |
| **Actual runtime verification** | **NOT VERIFIED** — no Windows machine has ever launched this app. Job-object kill, `taskkill` fallback, NSIS install, and every Windows code path are compile-verified only. |

I could not reproduce Windows locally: cross-compiling to `x86_64-pc-windows-msvc` dies in `ring`/`aws-lc-sys` build
scripts (no Windows C toolchain here). The CI log plus source analysis are the evidence.

---

## 6. Tailscale / network security audit

**Invite** (`room/invite.rs`): `movieparty://` scheme enforced; fragment required; version + protocol major/minor checked;
room-id shape checked; `join_secret` must be base64url **256-bit**; fingerprint shape checked; `host_ip` must parse and
must be IPv4, and `InviteError::InvalidHostAddress` rejects unspecified/loopback/non-Tailscale (`145-159`); expiry checked,
with invalid expiry treated as expired (`108`, `117`).

**Auth** (`network/quic.rs:2395-2472`), in order — this ordering is deliberate and correct:

1. platform must be `windows` | `macos`
2. room-id / secret-hash / public-key shape checks
3. `room_id` **and** `join_secret_hash` equality against the host's own credentials
4. **host-side invite expiry** (`2435-2441`) — placed *behind* the secret comparison so only a peer already holding the
   secret can learn the invite lapsed. The comment correctly notes that guest-side expiry only constrains an honest client.
5. `verify_auth_request_signature` over `(room_id, join_secret_hash, invite_nonce, device_id)` with the presented key
6. `replay_guard.accept_once(device_id, invite_nonce)` → `REPLAYED_AUTH`
7. `replay_guard.bind_identity(device_id, public_key)` → `IDENTITY_KEY_MISMATCH`, deliberately **after** the signature
   check so the binding cannot be pre-empted with an arbitrary key

**Bind address** (`1428-1459`): client binds `0.0.0.0:0` (documented: macOS refuses to send UDP from a loopback-bound
socket to a Tailscale CGNAT destination — a real two-device join depends on this); server-side `validate_quic_bind_addr`
accepts **only** loopback or Tailscale CGNAT IPv4. Both are correct and the rationale is documented.

**Assessment: no P0/P1 network weakness found.** The auth chain is layered, correctly ordered, and each check is
tested (`rejects_wrong_join_secret`, `rejects_malformed_join_secret_hash`, `REPLAYED_AUTH`, `IDENTITY_KEY_MISMATCH`).
Residual items are P3/INFO: no explicit per-peer connection limit or pre-auth connection cap was found, and stale-peer
cleanup is handled by the disconnect/reconnect machine rather than a dedicated sweeper.

---

## 7. TLS / cryptographic audit

| Property | Finding |
|---|---|
| `rustls` | **0.23.45** (Batch 4 bump; `cargo audit` green) |
| TLS version | **1.3 only** — enforced by `QuicServerConfig::try_from` (`quic.rs:1506-1509`), which rejects any config that cannot do TLS 1.3 |
| Certificate | Self-signed, `rcgen`, CN `localhost`, generated per host session (`1500`) |
| Pinning | Client verifier compares SHA-256 of the peer cert against the invite-carried fingerprint (`1552-1557`) |
| **CertificateVerify** | **Present and correct** (`1567-1574`): `verify_tls13_signature` against the cert's own key. The comment is exactly right — pinning alone is not authentication, because the cert DER is public and travels in the clear |
| Signature algorithms | Read back from the *installed* provider (`1618-1624`) so the verifier and endpoint cannot disagree — a real footgun avoided |
| ALPN | `movieparty-v1`, both sides (`31`, `1491-1494`, `1591-1594`) |
| **0-RTT** | **Explicitly disabled**, and **tested** (`4675-4691`): `max_early_data_size == 0`, `enable_early_data == false` |
| Join-secret auth | SHA-256 of a 256-bit random secret, never the secret itself (`1469-1471`, `108-110`) |

**Did the Batch 4 fixes weaken anything?** No. The `rustls` bump *added* the CertificateVerify verification (F44) and the
0-RTT assertions. No downgrade, confusion, identity-substitution, replay or auth-bypass path was found. `aws-lc-rs` is
the installed provider (`1611`).

---

## 8. Database / migration audit

`storage/sqlite.rs:226-321` is genuinely hardened, and the reasoning is documented rather than asserted:

- **Atomic per step**: `BEGIN IMMEDIATE` → body → `PRAGMA user_version=N` → `COMMIT`, so the schema change and its version
  stamp commit together; an interrupt rolls the whole step back (`274-287`).
- **Resumable**: version stamped per migration, so a crash at step N resumes at N (`262-264`).
- **Idempotent `ADD COLUMN`**: guarded by `column_exists` for both `MIGRATION_002` (`299`) and `MIGRATION_005` (`310`) —
  precisely the "physically altered, version still old" interrupted state.
- **Backfill runs even when the column was already present** (`316`) — that *is* the interrupted state, and this is the
  subtle case most implementations get wrong.
- **`SchemaTooNew` guard** (`255-260`), and `read_schema_version` propagates its error rather than defaulting to 0
  (`253`) — the comment correctly explains that `unwrap_or(0)` would sail past the guard.
- **Unknown version** → explicit `MP-STORE-003` error (`318-320`).
- WAL + `foreign_keys=ON` on open (`182`).

No migration path was found that works in the happy path but fails after interruption. **This area is a genuine strength.**

---

## 9. Frontend / state-machine audit

**Command surface is clean.** Cross-checked all 63 frontend-invoked command names against the 68 registered in
`lib.rs:106+`: **zero invoked-but-unregistered commands** — no control can fail with "command not found". All frontend
traffic goes through one wrapper (`src/backend/appRuntime.ts`), which is the right shape.

**The two known past bugs are fixed *and* properly tested — FALSE POSITIVES:**

- **F7 reconnect latch** (`overlays/ReconnectOverlay.tsx:34-36`): `roomState === "RECONNECTING" ? dismissed : false`.
  Keying on the episode rather than a boolean means a new disconnect re-raises the overlay while one episode cannot stack
  duplicates. Tested in `views/p2Contracts.test.ts:45-64`, including a **two-episode sequence** (dismiss → leave
  RECONNECTING → second disconnect) that would fail against the old latch.
- **F8 countdown once-guard** (`sync/countdownModel.ts:65+`): keyed on the **deadline**, not a boolean — so a re-observation
  of the same deadline cannot re-fire, while a genuinely new countdown can. Tested in `sync/countdownModel.test.ts:46-81`,
  including a **"many replays of one countdown without a second start"** case (`starts === 1`) that is exactly the
  negative control the old bug needed. The countdown is derived from a backend deadline; there is no frontend timer chain.

**AUD-10 (P3):** five registered commands are never invoked from the frontend — `create_schedule`, `delete_schedule`,
`update_and_broadcast_schedule`, `update_schedule_media`, `update_schedule_preload`. The UI uses
`create_and_broadcast_schedule` / `cancel_and_broadcast_schedule` / `guest_accept_schedule` / `list_schedules` instead.
Dead API surface, not a defect — but it is a second, stale schedule API that can drift from the live one.

**Not verified:** screens × navigation transitions were not exhaustively traced (see §19-E). No stale-result-overwrites-fresh-state
defect was found in the paths read, but this is not a clean bill of health.

---

## 10. Chrome / provider sync audit

**Process lifecycle** is careful: managed profile root per session, CDP on a fixed local port, generation-tagged session
(`chrome_generation`) so a stale poll cannot reinsert a replaced session (`app_runtime.rs:5409-5414`), and a session that
loses the race is closed with the lock released.

**Provider sync has a live position source — which sharpens §4.** `spawn_provider_watch` (1 s interval, `5330`) polls the
live page and writes `state.sync.position_ms = position_seconds * 1000` (`5423`), and calls
`buffer_low(PeerRole::Host, …)` on a strict-pause decision (`5434`). So the *provider* path continuously refreshes its own
canonical position; **the local-media path has no equivalent for the guest**, and no host→guest position broadcast exists
at all. The design intent (a periodic live-position source) is visible in the provider path and missing from the local
path — which is consistent with AUD-01/AUD-02 being an unfinished wire rather than a considered design.

**AUD-07 (P2) — the two ignored tests, and what they leave unproven.** Both are `#[ignore]`d and are the "2 ignored" in CI:

- `real_chrome_launches_and_evaluates_cdp` (`providers/chrome/mod.rs:982-1006`) — a genuine functional check (asserts
  `evaluate` returns `2`, then that `document.title` matches after navigation). Would actually fail if broken.
- `real_youtube_provider_sync_uses_chrome_cdp` (`1008-1048`) — **can pass while proving nothing.** At `1026-1033`, if the
  HTML5 player is not detected it prints `EXTERNAL PROVIDER VERIFICATION PENDING` and **`return`s successfully**; the test
  is reported `ok`. It also requires live network + YouTube, and even on success it only asserts that CDP `pause`/`seek`
  return `true` — **it never asserts a position, never compares two sides, and never tests synchronization at all.**

Therefore, even if un-ignored, the second test cannot prove provider *synchronization*. Combined with §3, **provider
synchronization is unproven at every level**: no automated test compares provider positions across two peers, and the
position-broadcast channel the guest would need (AUD-02) is unwired.

---

## 11. Media rendering / libmpv audit

- **Initialization**: raw FFI via `libloading`; only `msg-level`, `quiet`, `terminal` set; `vo=sw`, format `bgr0`.
- **Frame path**: 30 ms render loop (`spawn_player_render_loop`, `5888`) copies the latest RGBA/BGR0 buffer into the native
  layer. It breaks only when `state.player` is `None` — see the §2 doc/code contradiction.
- **Frame lifetime / pixel buffers**: exercised only by `real_sw_render_test`, which requires ≥10 pixel-bearing frames
  (`common/mod.rs:MIN_FRAMES_WITH_PIXELS`) — a sensible guard against a stale-buffer fluke, and the fixture duration floor
  (`MIN_FIXTURE_SECONDS = 2.0`) exists specifically because a shorter clip made the SW renderer emit all-black frames and
  produced a confusing failure. That reasoning is documented and correct.
- **Audio**: no `ao` configured; `ao=null` in the only real test. See AUD-06.
- **EOF**: no handling anywhere. See AUD-03.
- **Cleanup**: `close()` issues mpv `stop` and drops the handle; the render and event loops are aborted on teardown.
- **`CString::new(...).unwrap()` × 19** in `mpv_backend.rs`: every one is either a literal or `format!("{f:.3}")` on a
  float — an interior NUL is impossible, so these cannot panic. **Not a defect.**

**Classification:** no additional product defect found here beyond AUD-03/AUD-06. The absence of a `Completed` state is a
product defect, not a test or environment artefact.

---

## 12. Resource / performance audit

**Task lifecycle is the strong point.** 61 `.abort()` sites; `leave_party` takes and aborts every named task —
`host_session.server_handle`, `peer_event`, `host_event`, `calibration`, `player_event`, `player_render`, `transfer`,
`transfer_stall_watcher`, `chrome_crash_watcher`, `player_failure_watcher`, `reconnect`, `heartbeat`, `buffer_status`,
`provider_watch`, `preload`, `scheduler`. Single-slot fields mean re-spawn replaces + aborts, so duplicate loops are
structurally impossible. Fixed cadences are bounded: heartbeat 2 s, buffer status 500 ms, provider watch 1 s, render 30 ms,
event loop 200 ms, transfer-stall 5 s, tree poll 20 ms.

**AUD-01 is also a performance problem.** The repeated hard seeks on the guest each take the player lock
(`5860-5868`) while the 200 ms event loop and 30 ms render loop both want it — under the current code a guest under
sustained playback would be seeking roughly once per second forever.

**Not verified:** 2-hour/8-hour soak behaviour. With the position channel unwired, a long-session assessment would be
measuring the wrong thing; the honest answer is that long-session behaviour has never been observed. No unbounded
`Vec`/`HashMap` growth was found in the state struct, and `state.call_signals` / `call_signal_ledger` are explicitly reset
on teardown (`6363-6364`).

---

## 13. Error and recovery audit

| Failure | Detected? | State consistent? | User told? | Recoverable? | Can stale state resurrect? |
|---|---|---|---|---|---|
| Peer disappears | Yes — role-aware heartbeat threshold (`3748-3764`) | Yes — `peer_disconnected` → `RECONNECTING`, strict-sync pause | Yes — `ReconnectOverlay` (F7-correct) | Yes — reconnect worker; **no auto-resume** | Guarded by `session_generation` |
| Network drops | Yes — same path | Yes | Yes | Yes | Guarded |
| Chrome fails / CDP times out | Yes — crash watcher + generation check | Yes — stale session closed, not reinserted (`5409-5414`) | Via provider status overlay | Yes — relaunch | Guarded by generation |
| Media file disappears | Partial — load error surfaced (`set_player_diagnostic_error`) | Yes | Yes | Yes | No |
| libmpv missing | Yes — `LibMpvUnavailable` | Yes | Yes | No (environment) | No |
| Player command fails | Yes — `set_player_command_error` | Yes | Yes | Yes | No |
| DB write fails / migration fails | Yes — errors propagated, never defaulted (`253`) | Yes — transactional rollback | Yes — surfaced, not swallowed | Yes — retried next launch | No |
| Malformed peer input | Yes — shape checks + signature + decode errors | Yes | Logged | Yes | Replay guard blocks |
| Peer exceeds limits | **No explicit limit found** (P3) | — | No | — | — |
| App restart mid-party | Yes — no persisted party state | Yes | Yes | Yes — new party | No |
| **Movie reaches its end** | **No — not implemented (AUD-03)** | **No — room stays `Playing` forever** | **No** | **No** | **n/a** |

Recovery architecture is genuinely good. The one hole is the one nobody wrote a state for: **end of media.**

---

## 14. Release / packaging audit

| Item | Finding |
|---|---|
| Version consistency | 4/4 declarations agree (`0.9.8`); **no CI gate enforces this** — AUD-09 |
| Bundle targets | `app`, `dmg`, `nsis` (`tauri.conf.json`) |
| `productName` / identifier | `Movie Party` / `app.movieparty.desktop` |
| libmpv staging | `resources: ["mpv_runtime/"]` — staged into the bundle |
| Windows installer | NSIS, `installMode: currentUser` |
| Release workflow | `.github/workflows/release.yml`, tag-triggered (`v*`), plus a `workflow_dispatch` path with a `tag` input and per-OS `only` selector; `permissions: contents: write`; notes generated into `releaseBody`; `tagName` from the dispatch input or `github.ref_name` |
| **macOS signing / notarization** | **None.** No `APPLE_*`, `CSC_*`, or signing env vars in the workflow → **unsigned, un-notarized builds**; Gatekeeper will block them and users must right-click-open or clear the quarantine attribute |
| Updater | **Absent.** No `updater` plugin configured |
| `.app.tar.gz` semantics | Not produced by this config (targets are `app`/`dmg`/`nsis`); no updater means no signature/`tar.gz` update artifact is required |
| Tag immutability | `v0.9.0/4/5/6/8` all unchanged; workflow comment explicitly forbids retagging |

**Is the missing updater actually required by the locked scope?** For a private two-person app that ships as a DMG/NSIS
installer, no. The *real* packaging gap is not the updater — it is that **unsigned macOS builds require a documented
workaround for every user on every install**, and there is no CI gate ensuring the four version declarations stay in sync
before a tag is pushed.

---

## 15. Test-quality audit

**Counts (run `35474284919` @ `a44542a`, 5/5 jobs green):** Frontend 23 files / 287 tests. macOS 562 passed / 0 failed /
2 ignored / 4 filtered. Windows 553 passed / 0 failed / 2 ignored / 1 filtered. Per-target mapping resolved by name and
the arithmetic closes exactly on both platforms.

**Does each important test really execute?**

| Target | macOS | Windows | Real? |
|---|---|---|---|
| `lib` (unit) | 478 | 469 (9 fewer — legitimately macOS-gated tests) | yes |
| `m2_integration` | 28 | 28 | **simulated** — no real libmpv |
| `m3_*`, `m4_*` | yes | yes | **simulated** |
| `real_native_surface_e2e` | **0** | **0** | macOS-only; CI cannot stage libmpv |
| `real_playback_smoke_test` | **0** | **0** | macOS-only; CI cannot stage libmpv |
| `real_sw_render_test` | **0** | **0** | macOS-only; CI cannot stage libmpv |
| `windows_native_surface_e2e` | 0 | **0** | Windows-only; needs a real surface |
| `tailscale_probe` | 2 | 2 | probe only |

**The single most important test-quality finding:** the four `real_*` targets **execute zero tests in CI on both
platforms**. CI's green is therefore silent about the entire real-media path. This is honestly documented in
`tests/common/mod.rs` and `ci.yml`, and the Batch 5 false-green (`if !exists { return }`) was correctly replaced with
`common::require_*` so a missing prerequisite **fails loudly** instead of passing. That was the right fix.

**Can any important test silently pass?** **Yes — one.** AUD-07's
`real_youtube_provider_sync_uses_chrome_cdp` returns `ok` without asserting when the YouTube player is absent
(`providers/chrome/mod.rs:1026-1033`). It is `#[ignore]`d today, so it does not run in CI — but if anyone un-ignores it to
"prove provider sync", they will get a green that proves nothing.

**Does anything test real media / audio / two devices?**
Real media: locally only, `vo=null`/`ao=null`. **Audio: never.** **Two devices: never.** Negative controls: good where
they exist — the F7 two-episode test and the F8 "many replays" test are genuine negative controls; the drift tests are
not, because they call the correction function directly with hand-picked input (§4.5).

---

## 16. Hidden-bug search

| Pattern | Result |
|---|---|
| `TODO` / `FIXME` / `HACK` / `XXX` | **1 hit, and it is a test fixture string** (`network/tailscale.rs:1442`). Effectively zero. |
| Production `unwrap()` | **Clean.** All non-test `unwrap()` sites are either `CString::new(literal)` in `mpv_backend.rs` (cannot panic) or `unwrap_or_else(\|p\| p.into_inner())` on mutex poisoning. Every `unwrap()` in `tailscale.rs` (≥871 vs `#[cfg(test)]` at 830), `invite.rs` (≥183 vs 164), `app_runtime.rs` (≥7792, test module), `range_server.rs` (626/856 vs 461) and `providers/sync.rs` (704 vs 603) is **inside a test module**. |
| `expect()` | 665 — overwhelmingly test code; production sites are documented invariants. |
| `let _ = ` | 148 — mostly deliberate fire-and-forget channel sends (`let _ = tx.send(..)`) and best-effort teardown (`let _ = conn.execute_batch("ROLLBACK")`, correctly discarding the rollback error in favour of the migration error). Not a defect class here, but the count is high enough that it is not auditable line-by-line in one pass. |
| Empty catch / swallowed `Result` | No empty `catch {}` in the frontend; Rust error paths return `Result` and callers mostly propagate. |
| Fire-and-forget / detached tasks | Every `tokio::spawn` for a named concern is stored in a single-slot field and aborted on teardown. The inline `tokio::spawn` commit tasks are guarded by `session_generation` and `last_committed_operation_id`. |
| Locks around blocking ops | `dispatch_player_play` and the provider poll run under the state mutex — bounded, but the §4 churn amplifies contention. |
| Sleeps in async code | Present but bounded and intentional (poll cadences). |
| Sequence / monotonicity | Envelope `seq` via `coordinator_event_seq` (wrapping_add); `monotonic_us()` for execution deadlines; `record_peer_seq` tracks `last_peer_seq_received` **monotonically** (`sync/local.rs:298-302`) and cannot regress. |
| Unchecked IDs / URLs | Invite parsing is strict; provider URL validation goes through `is_youtube_url`. |
| Path traversal | Media path comes from a native file picker; provider profile root is a per-session temp dir. |
| Dead code / unreachable branches | **AUD-02** (`host_relay_ready_state`), **AUD-08** (`shared_pipeline.rs`), **AUD-10** (5 commands). |
| Error strings vs consumers | `EXTERNAL PROVIDER VERIFICATION PENDING` is printed by a test that still passes — see AUD-07. |
| Code paths that exist but aren't wired | **AUD-01/02**: the guest drift correction is wired, but its input channel is not. |
| UI controls that don't do their claimed action | None found — all 63 invoked commands are registered. |
| **AUD-11 (P3)** | `if state.sync.position_ms == 0` (`app_runtime.rs:4477`) — a one-shot initialisation that only fires when the position is *exactly* zero. Fragile heuristic; a legitimate position of 0 mid-session would re-initialise from the coordinator. |
| **AUD-12 (P3)** | `buffered_ahead_ms()` always `None` for mpv (`mpv_backend.rs:577-579`). |

---

## 17. Production failure simulation

Reasoned from code; no code was modified.

- **Host disappears** — guest heartbeat threshold trips → `peer_disconnected` → `RECONNECTING`, strict-sync pause, overlay shown (F7-correct). No auto-resume. **Handled.**
- **Guest disappears** — host's role-aware path → same recovery machine. **Handled.**
- **Network drops / packets reorder / duplicate / arrive late** — QUIC gives ordered, reliable streams; envelope `seq` is monotonic; `last_committed_operation_id` and `pending_operation_id` guards make duplicate commits idempotent. **Handled** (tested by the duplicate/stale op-id tests).
- **Command fails** — `set_player_command_error`, surfaced. **Handled.**
- **Player fails** — `spawn_player_failure_watcher` promotes to `PlayerFailure` recovery. **Handled.**
- **Chrome fails / provider closes** — crash watcher + generation check; stale session closed, not reinserted. **Handled.**
- **DB write fails / migration interrupted** — transactional, resumable, loudly surfaced. **Handled** (§8).
- **App restarts / killed during teardown** — no persisted party state; job object (Windows) / process-group kill (unix) cover the Chrome tree. **Handled.**
- **Session replaced / reconnect with stale state** — `session_generation` invalidation. **Handled.**
- **Malformed peer input / peer exceeds limits** — shape checks + signature + replay guard; **no explicit peer limit** (P3).
- **Media disappears / becomes invalid** — surfaced as a player error; **recoverable**.
- **Movie reaches its end** — **NOT HANDLED.** No EOF detection, no `Completed` state, room remains `Playing`. (AUD-03)
- **Two devices playing a real movie for more than ~0.7 s** — **BROKEN.** The guest is repeatedly hard-seeked back to the
  commit anchor. (AUD-01)

---

## 18. Finding classification

Every finding has exactly one class. Severity was assigned against *impact on the stated product promise*, not effort to fix.

---

### AUD-01 — Guest playback is repeatedly seeked backwards; sustained synchronized playback is impossible
**Class: P0 — release blocker / catastrophic**
**File / path:** `src-tauri/src/app_runtime.rs:5818-5824` (computation), `5860-5881` (`apply_drift_correction`), `src-tauri/src/sync/drift.rs:12-28` (thresholds); anchor writes at `4183`, `4256`, `4339`, `4383`, `4477-4483`, `4491`
**Code path:** `spawn_player_event_loop` → guest branch → `drift = snap.position_ms − state.sync.position_ms` → `correction_for_drift` → `MicroSeek`/`HardSeek`
**Proof:** The guest's `state.sync.position_ms` is written only by commit/seek/buffer events and by a one-shot `== 0` guard. `snap.position_ms` advances (mpv `time-pos`, `mpv_backend.rs:555-556`). Drift therefore grows at ~1000 ms/s, crossing the 250 ms and 700 ms thresholds within ~0.3 s and ~0.7 s of every commit. Guest role confirmed (`is_host_role` = `role == "Host"`; guest sets `"Guest"` at `3198`/`3310`); `Playing` state confirmed (`sync/local.rs:276` → `apply_coordinator_state`).
**Platform:** both (the defect is platform-independent; it is *observable* only with real decoding)
**Affected journey:** guest watching any movie — the core product promise
**Why P0:** it defeats the single thing the product exists to do. Not a reliability edge case; the ordinary path.
**Covered by an existing batch?** No. Batches 1–6 did not touch the position channel.
**Needs code change?** **Yes.** **Needs manual test?** **Yes.** **Needs regression test?** **Yes** — a test that advances a scripted position across many polls and asserts the guest is not seeked, with the real call site (not the helper) as the unit under test.

---

### AUD-02 — Host→guest position channel is unwired; its only sender is dead code
**Class: P1 — serious production defect**
**File / path:** `src-tauri/src/app_runtime.rs:5241-5273` (`#[allow(dead_code)] fn host_relay_ready_state`), send at `5255`; `src-tauri/src/network/quic.rs:401-404` (`RoomStateUpdate`)
**Proof:** crate-wide grep finds no caller for `host_relay_ready_state`; the only other `RoomStateUpdate` construction is a test (`quic.rs:3995`). `git log -S "host_relay_ready_state"` shows it uncalled since the initial snapshot `f3e9a8c`. No periodic task broadcasts position (`send_host_event` call sites are all commit/readiness/chat/reaction/schedule).
**Platform:** both · **Journey:** any two-device session
**Why P1:** it is the root cause of AUD-01 and the reason drift correction has nothing valid to compare against.
**Covered?** No. **Needs code change?** Yes (wire a periodic position broadcast, or redefine the drift anchor). **Manual test?** Yes. **Regression test?** Yes — assert a `RoomStateUpdate` (or equivalent) reaches the guest at a bounded cadence while Playing.

---

### AUD-03 — End of media is not implemented
**Class: P1 — serious production defect**
**File / path:** `src-tauri/src/media/player/mod.rs` (`PlayerState` — no `Completed`); `app_runtime.rs:6372` (only setter of `RoomState::Ended`, in end-party teardown); `media/player/mpv_backend.rs:551-564` (`snapshot()`)
**Proof:** zero hits crate-wide for `eof-reached`, `end-file`, `keep-open`, `idle-active`, `playback-complete`; no `position_ms >= duration_ms` comparison anywhere; `PlayerState` has no terminal variant.
**Platform:** both · **Journey:** finishing a movie — universal
**Why P1:** the movie ends and the app does not know. Room stays `Playing` indefinitely; no coordinated end, no ENDED transition, no cleanup. Every user hits this on their first film.
**Covered?** No. **Needs code change?** Yes. **Manual test?** Yes. **Regression test?** Yes.

---

### AUD-04 — The drift loop is never exercised by any test at any level
**Class: P2 — meaningful reliability problem** (test-coverage)
**File / path:** `src-tauri/src/app_runtime.rs:5755` (`spawn_player_event_loop`); `tests/` (no reference)
**Proof:** grep for `spawn_player_event_loop` / `apply_drift_correction` in `tests/` → **zero hits**. CI's `real_*` targets execute **0 tests**. `test_b` asserts position equality only at the commit instant (`m2_integration.rs:216-220`).
**Why P2:** it is the reason AUD-01 shipped undetected; the class is "coverage", the consequence was P0.
**Needs code change?** No (test only). **Regression test?** Yes — this is the fix for AUD-01's detection gap.

---

### AUD-05 — Drift unit tests call the helper directly with hand-picked drift, so they cannot fail for the real bug
**Class: P2 — meaningful reliability problem** (test defect)
**File / path:** `app_runtime.rs:9744-9767`, `9771-9787`
**Proof:** both call `AppRuntime::apply_drift_correction(&player_arc, <literal>, <literal>)`. They verify the mapping; the production defect is in the *argument*.
**Why P2:** a test that cannot fail for the bug that exists is worse than no test, because it reads as coverage.
**Needs code change?** No. **Regression test?** Yes — drive the loop, not the helper.

---

### AUD-06 — Audio output is unconfigured and has never been exercised
**Class: P2 — meaningful reliability problem**
**File / path:** `media/player/mpv_backend.rs:240-242` (only `msg-level`/`quiet`/`terminal` set; no `ao`); `tests/real_playback_smoke_test.rs:104` (`ao=null`); fixture has no audio track (§3.2)
**Proof:** container parse shows no audio stream; the only real-libmpv test sets `ao=null`.
**Platform:** both · **Journey:** any movie with sound
**Why P2 not P1:** mpv will select a default `ao`, so audio may well work — but nothing in the repository has ever produced a sound, and A-V sync is a core part of "watch a movie together".
**Covered?** No. **Needs code change?** Possibly (explicit `ao`); at minimum an explicit decision. **Manual test?** Yes. **Regression test?** Yes — an audio-bearing fixture.

---

### AUD-07 — The provider-sync test can pass while proving nothing
**Class: P2 — meaningful reliability problem** (test defect)
**File / path:** `src-tauri/src/providers/chrome/mod.rs:1026-1033`
**Proof:** when the HTML5 player is not detected the test prints a message and **`return`s**; the runner reports `ok`. It also needs live network, and asserts only that CDP `pause`/`seek` return `true` — never a position, never two sides.
**Why P2:** it is the only test that could have evidenced provider synchronization, and it structurally cannot.
**Covered?** No. **Needs code change?** Yes (test only — fail or hard-skip explicitly; assert positions). **Manual test?** Yes.

---

### AUD-08 — `media/shared_pipeline.rs` is not compiled at all
**Class: P2 — meaningful reliability problem** (dead code)
**File / path:** `src-tauri/src/media/shared_pipeline.rs`; absent from `media/mod.rs:1-7`
**Proof:** `media/mod.rs` declares `cache, local_perfect, manifest, player, shared_stream, stream, transfer` — no `shared_pipeline`. The only references to the file are within itself. Its one `#[ignore]`d test can never run.
**Why P2:** consistent with `shared_available: false`, so it is not a live defect — but it is an uncompiled, unverified file that will rot silently and mislead anyone who reads it as a shipped capability.
**Needs code change?** Decide: delete, or declare + feature-gate. **Manual test?** No.

---

### AUD-09 — No CI gate on version consistency
**Class: P2 — meaningful reliability problem** (release)
**File / path:** `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `Cargo.lock`; `.github/workflows/ci.yml` (no version step)
**Proof:** four declarations, all `0.9.8` today, but nothing enforces it. A tag push with three of four bumped ships a mislabelled build.
**Why P2:** cheap to prevent, expensive to discover post-release.
**Needs code change?** CI only. **Regression test?** A one-line consistency check in `ci.yml`.

---

### AUD-10 — Five registered Tauri commands are never invoked
**Class: P3 — minor / non-blocking**
**File / path:** `src-tauri/src/lib.rs:106+`; frontend uses the `*_and_broadcast_schedule` variants
**Commands:** `create_schedule`, `delete_schedule`, `update_and_broadcast_schedule`, `update_schedule_media`, `update_schedule_preload`
**Why P3:** dead surface, not a defect; two parallel schedule APIs can drift.

---

### AUD-11 — `position_ms == 0` one-shot guard is a fragile heuristic
**Class: P3 — minor / non-blocking**
**File / path:** `app_runtime.rs:4477-4484`
**Why P3:** a legitimate position of exactly 0 mid-session re-initialises from the coordinator. Low practical impact; symptomatic of the missing position channel (AUD-02).

---

### AUD-12 — `buffered_ahead_ms()` always returns `None` for mpv
**Class: P3 — minor / non-blocking**
**File / path:** `media/player/mpv_backend.rs:577-579`
**Why P3:** the sparse-cache fallback is deliberate and documented; libmpv's real demuxer cache is simply unused.

---

### AUD-13 — macOS builds are unsigned and un-notarized; no updater
**Class: INFO — observation**
**File / path:** `.github/workflows/release.yml` (no `APPLE_*`/`CSC_*`); `tauri.conf.json` (no `updater` plugin)
**Why INFO:** the updater is genuinely not required by the locked scope (installer-distributed private app). The unsigned-build reality is a *documentation* requirement, not a code defect — but it must be in the release notes, and it is a per-install friction for every macOS user.

---

### AUD-14 — CI's `real_*` targets execute zero tests on both platforms
**Class: INFO — observation** (honestly documented)
**File / path:** `tests/common/mod.rs`; `ci.yml`; CI run `35474284919` (0 passed / 0 failed / 0 ignored)
**Why INFO:** deliberate and documented — `libmpv.dylib` is a gitignored build artifact CI cannot build. Recorded so nobody reads CI-green as real-playback coverage.

---

### FALSE POSITIVES (disproven by reading the code)

| Claim | Verdict |
|---|---|
| "F7 reconnect latch can hide later disconnects" | **Disproven** — keyed on the RECONNECTING episode (`ReconnectOverlay.tsx:34-36`); tested across two episodes (`p2Contracts.test.ts:45-64`) |
| "F8 countdown can start twice" | **Disproven** — keyed on the deadline; tested with many replays asserting `starts === 1` (`countdownModel.test.ts:46-81`) |
| "Migrations can brick a DB after interruption" | **Disproven** — atomic per step, resumable, `ADD COLUMN` guarded, backfill always runs, `SchemaTooNew` guarded, version read never defaulted |
| "TLS pinning alone authenticates nothing" | **Already fixed** — `CertificateVerify` is verified against the cert's own key (`quic.rs:1567-1574`); 0-RTT explicitly disabled and asserted |
| "Replay / identity substitution possible" | **Disproven** — signature over `(room_id, secret_hash, nonce, device_id)`, `accept_once`, `bind_identity` ordered after the signature check |
| "Invite expiry only checked client-side" | **Disproven** — enforced host-side at the auth boundary, deliberately behind the secret comparison (`quic.rs:2435-2441`) |
| "Production code is littered with panicking `unwrap()`" | **Disproven** — every non-test `unwrap()` is an infallible `CString::new(literal)` or a mutex-poison recovery; all other sites are inside `#[cfg(test)]` |
| "Windows `test_b` is unverified" | **Disproven** — `test_b ... ok` on the real Windows runner at `a44542a` |
| "Provider Shared is implemented but broken" | **Disproven** — `shared_available: false` by design; the pipeline file is not even compiled |

---

## 19. Final release decision

### A. Confirmed fixed
- Batch 1 `test_b` on real Windows (`a44542a`, 553 passed / 0 failed) — including the Windows clippy regression that was masking it.
- `cargo audit` now green (rustls 0.23.45); `pnpm audit` green.
- TLS: fingerprint pin **plus** `CertificateVerify`; 0-RTT disabled and asserted; ALPN; provider/endpoint signature-algorithm agreement.
- Auth: host-side expiry, signature over `(room, secret, nonce, device)`, replay guard, identity–key binding ordered correctly.
- Migrations: atomic, resumable, idempotent, `SchemaTooNew`-guarded, errors never defaulted.
- F7 reconnect latch and F8 countdown guard — correct **and** covered by real negative controls.
- Frontend command surface: 63/63 invoked commands registered; single typed wrapper.
- Task lifecycle: every named task single-slot, aborted on teardown; 61 abort sites.
- Windows Chrome teardown: private job object with kill-on-close; cannot reach user Chrome.

### B. Confirmed remaining
- **AUD-01 (P0)** — guest drift anchor frozen → repeated backwards seeks during real playback.
- **AUD-02 (P1)** — host→guest position broadcast unwired; sole sender is dead code.
- **AUD-03 (P1)** — no end-of-media detection; no `Completed` state; room stays `Playing` forever.
- **AUD-04/05 (P2)** — the drift loop is untested; drift tests cannot fail for the real bug.
- **AUD-06 (P2)** — audio unconfigured and never exercised.
- **AUD-07 (P2)** — provider-sync test can pass without proving anything.
- **AUD-08 (P2)** — `shared_pipeline.rs` not compiled.
- **AUD-09 (P2)** — no CI version-consistency gate.
- **AUD-10/11/12 (P3)**, **AUD-13/14 (INFO)**.

### C. Unverified
- Any runtime behaviour on Windows (compile- and CI-verified only).
- Any behaviour with real decoded video on real hardware, in the actual app (all local real-media tests bypass the app's player wrapper and use `vo=null`/`ao=null`).
- Audio output and A-V synchronization.
- H.265, MKV, high bitrate, large files, VFR, 2-hour sessions.
- Two-device behaviour of any kind.
- Exhaustive frontend navigation/state-transition coverage.

### D. Environment limitations
- No Windows machine — cross-compilation to `x86_64-pc-windows-msvc` fails in `ring`/`aws-lc-sys` build scripts (no Windows C toolchain).
- `libmpv.dylib` is a gitignored artifact; CI cannot build it, so the `real_*` targets are structural no-ops in CI.
- `ffprobe` unavailable — the fixture was analysed by direct MP4 box parsing instead.
- One machine — no two-device execution possible.

### E. Manual-only verification required
1. Two real devices over Tailscale, real movie, watch for 10+ minutes — **this is the test that AUD-01 predicts will fail.**
2. Same, with audio, confirming both sides hear sound and lip-sync holds.
3. Seek near start / near end / repeated seeks; pause while buffering; resume after buffering.
4. Watch a movie to its end (AUD-03 predicts no ENDED transition).
5. Windows: launch, play, end-party, confirm no orphaned Chrome processes.
6. Provider sync: two devices on a real YouTube video, compare positions.
7. Exhaustive frontend navigation sweep (back/escape, reconnect overlays, dialogs).

### F. Release blockers
- **AUD-01** — the guest cannot play a movie. Non-negotiable.
- **AUD-03** — the end of a movie is an unhandled state.

### G. Non-blocking risks
- AUD-04/05 (detection gap — becomes a blocker for *confidence*, not for shipping), AUD-06, AUD-07, AUD-08, AUD-09, AUD-10, AUD-11, AUD-12.
- AUD-13/14: documentation and expectation-setting.

### H. Test gaps
- No test exercises the drift loop with an advancing position (AUD-04/05).
- No test compares provider positions across two peers (AUD-07).
- No test covers end-of-media (AUD-03).
- No test uses audio (AUD-06).
- No test covers two devices, ever.
- CI executes zero real-media tests (AUD-14).

### I. Security risks
**No P0/P1 security finding.** The TLS and auth layers are well constructed and correctly ordered. Residual: no explicit per-peer/pre-auth connection cap (P3). The unsigned-macOS-build reality (AUD-13) is a distribution-integrity concern worth documenting, not a vulnerability in the app.

### J. Cross-platform risks
Windows is **compile- and CI-verified but never runtime-verified**. The job-object design is sound and the cfg gating is now consistent. Residual risk is concentrated in untested runtime paths: job-object assignment on a real Chrome launch, `taskkill` fallback, NSIS install, and Windows file paths.

### K. Real two-device playback gaps
Total. No two-device test exists, no position channel exists, and the code that *would* correct drift is being fed a value that grows without bound (AUD-01/02).

### L. Provider / Chrome gaps
Process lifecycle is well engineered (generation-tagged sessions, private job object). But provider **synchronization** is unproven: the one test that could evidence it can return green without asserting (AUD-07), and the position channel the guest would need is the same missing wire as AUD-02.

### M. Recommended remediation order
1. **AUD-02 → AUD-01.** Wire the host→guest position channel (periodic broadcast, or redefine the guest anchor to a projected host position). Fix the anchor *before* touching the thresholds — the thresholds are fine; the input is wrong. Do not raise any timeout.
2. **AUD-04/05.** Add a regression test that drives `spawn_player_event_loop` with a scripted advancing position on both roles, and asserts the guest is **not** seeked while in sync. Break it deliberately first to confirm it goes red.
3. **AUD-03.** Introduce a `Completed`/terminal player state and an EOF transition, coordinated host→guest.
4. **AUD-06.** Decide `ao` explicitly; add an audio-bearing fixture and a test that asserts a stream is opened.
5. **AUD-07.** Make the provider test fail (or hard-skip) rather than return `true`; assert a position.
6. **AUD-09.** One-line version-consistency check in `ci.yml`.
7. **AUD-08/10.** Delete or wire; remove the stale command surface.
8. **AUD-11/12**, then AUD-13/14 documentation.

---

### The eleven explicit questions

1. **Can we honestly call the current beta production-ready?**
   **No.** There is a P0 defect on the guest playback path and an unimplemented end-of-media state. Calling this production-ready would be a false statement about the core feature.

2. **Can we honestly say two real users can watch a real movie together?**
   **No.** On the evidence, a guest would be hard-seeked back to the last commit roughly once per second. Nobody has ever watched a movie synchronised across two devices on this codebase — and the code says they could not.

3. **Is audio synchronization proven?**
   **No.** Audio output is not configured, the only real-libmpv test sets `ao=null`, and the only media fixture has no audio track. No code path in this repository has ever produced a sound.

4. **Is Windows behaviour proven?**
   **Partly — and only at compile/CI level.** Windows clippy passes and 553 tests pass on `windows-latest` at `a44542a`, including `test_b ... ok`. **No Windows machine has ever run this application.** Runtime behaviour — job-object kill, `taskkill` fallback, NSIS install — is unverified.

5. **Is real Chrome/provider synchronization proven?**
   **No.** The two tests are `#[ignore]`d; one can pass without asserting anything; neither compares positions across two peers.

6. **Are there any P0/P1 findings?**
   **Yes.** One P0 (AUD-01) and two P1 (AUD-02, AUD-03).

7. **Are there any P2 findings worth fixing before v0.9.9?**
   **Yes — AUD-04/05 and AUD-09.** AUD-04/05 are what let AUD-01 hide, and without them the P0 fix would ship unverified. AUD-09 is a one-line CI guard. AUD-06 (audio) is arguably P1-adjacent for a movie app and worth a decision before release.

8. **What absolutely must be fixed?**
   **AUD-01** (guest playback), **AUD-02** (the missing wire that causes it), **AUD-03** (end of media), and **AUD-04/05** (a regression test that can actually fail for AUD-01).

9. **What can safely be deferred?**
   AUD-08, AUD-10, AUD-11, AUD-12, and AUD-13's updater question. AUD-07 and AUD-09 are cheap enough to do now but are not blockers.

10. **What manual tests remain?**
    All seven in §19-E — most importantly the two-device real-movie run, which is the only test that can confirm or refute AUD-01, and the end-of-movie run for AUD-03.

11. **Is Provider Shared analysis safe to begin yet?**
    **No.** Provider Shared builds on the same position/synchronization channel that AUD-01/02 show is unwired, and the provider path is unproven (AUD-07). Beginning that analysis now would stack a new subsystem on a broken foundation. **Fix AUD-01/02/03 first.**

---

## Residual limitations of this audit

- **AUD-01 is a source-level proof, not a runtime observation.** I could not run two devices. The arithmetic is unconditional and the branch is provably taken, but I did not watch it happen. That distinction is stated deliberately.
- Windows runtime, audio, real decoded frames in the app, and long sessions were **not** executed.
- The frontend screen/transition matrix was sampled, not exhaustively traced.
- The 148 `let _ =` sites were characterised by pattern and spot-checked, not read line-by-line.
- No code, test, CI, or documentation file was modified; no commit, push, tag, or release was made.
