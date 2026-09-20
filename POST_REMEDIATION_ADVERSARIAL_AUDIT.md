# Post-Remediation Adversarial Audit

**The audit itself was READ ONLY.** No code, test, documentation, tag, branch or release was modified
while it was performed. Every finding below was then fixed in a follow-up commit — see
§7 "Remediation of these findings" for what changed, the evidence, and what is still open.

**Subject:** `stabilization/v0.9.9-rc1` @ `65b97ea` — the Batch 8 remediation candidate.
**Audited SHA:** `65b97ea` (verified code `3372d48`, CI run `35502788709`, all six jobs green).
**Fixes applied in:** `97d452a`.
**Date:** 2026-09-20

---

## 0. Preconditions — is the remediation actually complete?

The brief asked me to confirm completeness before starting. Honest answer, split by scope:

| Scope | State |
|---|---|
| **All P0/P1/P2 from the deep audit (AUD-01…AUD-09)** | **Remediated** — except AUD-08, which was **deliberately not remediated** at your instruction (repairing it is Provider Shared work, which the batch excluded; deletion is your call). |
| **P3/INFO (AUD-10…AUD-14)** | **Not touched** — the batch's mandate was "for every P0/P1/P2". |
| **Findings raised during remediation (AUD-15, AUD-16)** | **Flagged, deliberately not changed.** |

So "nothing from the previous audit is remaining" is **not** strictly true: AUD-08 remains open by your
own scope decision, and AUD-10…14 were out of scope. I started the adversarial work anyway, because it
is about challenging the *fixes*, and every P0/P1/P2 fix exists to be challenged.

Baseline verified: `main` = `bb22577`, `v0.9.8` → `bb22577` (annotated tag object `4fca32c`), all five
tags unchanged, no release created or modified, working tree clean.

---

## 1. Headline result

**Two NEW BUGs, both introduced by the remediation itself — one from my AUD-01 fix, one from my AUD-09
fix.** Everything else that I could attack at source level held up, including the two items I most
expected to break.

| Verdict | Count | Items |
|---|---|---|
| **VERIFIED FIX** | **16** | SequenceTracker reorder window · sequence-enforcement choke point (no bypass) · stale session generations · Chrome process-tree cleanup · Chrome generation/reinsertion · rustls 0.23.45 · TLS CertificateVerify · fingerprint pinning + 0-RTT · join-secret auth ordering · invite expiry (genuinely wired) · authenticated peer cap · URL parsing · identity rotation · AUD-04/AUD-05 tests (mutation-proven) · DB failure propagation · frontend command error propagation |
| **PARTIALLY VERIFIED** | 7 | AUD-01, AUD-03, AUD-06, AUD-07, **AUD-09**, real-media prerequisites, libmpv render tests |
| **NEW BUG** | **2** | **ADV-01** (the projection is clock-offset-dependent and unbounded) · **ADV-03** (the version gate is in a workflow that never runs on a tag push) |
| **ACCEPTED LIMITATION** | 4 | bounded reorder window; probabilistic reorder coverage; headless audio; macOS-only real tests |
| **NOT VERIFIED** | 5 | two-device sync, Windows playback, audible output, multi-hour behaviour, Chrome generation *under real Chrome* |

**Both new bugs share a shape worth naming:** the fix was correct in itself and wrong about its
*context*. ADV-01 computes from a clock it assumed was calibrated; ADV-03 enforces a rule in a workflow
it assumed would run. Neither is a coding error. Both are failures to ask "what does this depend on, and
who guarantees it?"

---

## 2. The real findings

### ADV-01 — NEW BUG (P2): the drift projection can be arbitrarily wrong, and nothing bounds it

**Where:** `app_runtime.rs:5290` (`host_monotonic_us`), `app_runtime.rs:5308`
(`projected_host_position_ms`), consumed at `app_runtime.rs:5385` (`drift_correction_for_player`).

**Introduced by:** the AUD-01 fix. Pre-fix, guest drift compared `snap.position_ms` against
`state.sync.position_ms` — a value bounded by the media itself. Post-fix it compares against a value
derived from **two different machines' monotonic clocks**.

**Mechanism.**

```rust
fn host_monotonic_us(state) -> u64 {
    (monotonic_us() as i64).saturating_add(state.clock_offset_to_host_us).max(0) as u64
}
fn projected_host_position_ms(state) -> Option<u64> {
    if state.room_state != RoomState::Playing { return None; }
    let committed = state.committed_playback.as_ref()?;
    let elapsed_us = Self::host_monotonic_us(state).saturating_sub(committed.execute_at_host_mono_us);
    Some(committed.target_position_ms.saturating_add(elapsed_us / 1_000))
}
```

`execute_at_host_mono_us` is a **host**-monotonic timestamp. `host_monotonic_us` is only host-monotonic
once `clock_offset_to_host_us` has been calibrated; before that it is `0`, so the function returns the
**guest's own** monotonic clock. Subtracting a host timestamp from a guest timestamp is meaningless, and
`saturating_sub` makes the result one-sided:

- guest monotonic **<** `execute_at` (guest uptime < host uptime) → `elapsed_us = 0` → projection =
  target. **Benign.**
- guest monotonic **>** `execute_at` (guest uptime > host uptime) → `elapsed_us` = the *uptime
  difference* → projection = `target + (uptime difference in ms)`. **Arbitrarily large.**

That value becomes the canonical position. Drift = `local − canonical` is then a huge negative number,
`correction_for_drift` classifies it as `HardSeek`, and `apply_drift_correction` calls
`player.seek(target.max(0))` — so **the guest is seeked to a position far beyond the end of the film**.

**Which direction it goes is ~50/50**, decided by which machine has been up longer — so this is not a
rare corner; it is a coin flip *conditional on calibration not having succeeded*.

**Why it is reachable.** `clock_offset_to_host_us` is set only by the calibration paths
(`app_runtime.rs:3297`, `app_runtime.rs:3418`). `spawn_clock_calibration` needs **≥4 successful probes
out of 20** before it sets anything; if the probes keep failing, the offset stays `0` for the life of
the session — and every PLAY commit in that session then computes from mixed clocks.

**There is no guard, and the flag that would be one is dead.** `clock_calibrated` is written at
`3298` and `3419` and **read nowhere** in the crate. Confirmed by exhaustive grep. This is the AUD-15
finding — and the adversarial pass sharpens it: I originally rated AUD-15 as "arbitrary error, low
likelihood". That understated it. The error is not arbitrary, it is **unbounded and directionally
predictable**, and its consequence is a seek to a garbage position, not merely a degraded correction.

**No clamp anywhere.** The projection is never clamped to `[0, duration_ms]`, even though the duration
is available on the player snapshot. A clamp would bound the blast radius of *any* cause — including
causes I have not thought of.

**Severity P2, not P1.** It requires calibration to have failed, which on a working LAN is unlikely
(the loop retries until the client is gone). It is not a regression in the ordinary path — the AUD-01
fix is strictly better there. But when it fires it is unwatchable, and it fires *at play start*, which
is the worst possible moment.

**The fix, when you want it** (deliberately not applied — this audit is read-only):

1. guard the projection on `clock_calibrated`, which also makes that flag meaningful; **and**
2. clamp the projection to the known duration.

Both are small. I would do (2) regardless of (1), because a clamp is what stops *unknown* causes from
becoming seeks to the end of the film.

### ADV-02 — latent robustness gap (P3, same root): `committed_playback` is never cleared

`committed_playback` is initialised to `None` exactly once, at runtime construction
(`app_runtime.rs:885`), and set at the two commit sites (`3524`, `4209`). **It is never cleared in
production** — no teardown, no `end_party`, no `leave_party`, no new-party reset. The only `= None` in
the crate is inside a test (`10143`).

So after a party ends, the previous session's anchor survives into the next one. The projection is
inert while the room is not `Playing`, and the normal ordering protects it: the host sends `PlayCommit`
(`app_runtime.rs:3495`) **before** the spawned task runs `commit_play` at the deadline (`3521`), so the
guest's anchor is refreshed *before* the coordinator's `PLAYING` reaches it. **I could not construct a
reachable failure** on the paths I traced.

It is still wrong: a stale cross-session anchor has no business being readable, and it is one ordering
change away from becoming ADV-01 with a much larger multiplier (hours instead of uptime difference).

### ADV-03 — the AUD-09 version gate is in the wrong workflow (P2, bypass found)

**This is the "does another code path bypass the fix?" question landing.**

AUD-09's fix added a `version-consistency` job that asserts all four version declarations agree. I
verified it works — its script passes under `bash`, and it has now executed on CI.

**But it lives in `ci.yml`, which is `workflow_dispatch`-only** (manual, by design — the file says so at
the top). The workflow that actually *publishes* a release is `release.yml`, and that one fires
**automatically on `push: tags: ["v*"]`**.

So the guard does not run on the path it was created to protect. Tag `v0.9.9` while the four
declarations still say `0.9.8` and:

1. `ci.yml` never runs — nobody dispatched it;
2. `release.yml` runs automatically, builds, and publishes a release labelled `v0.9.9` from a tree that
   declares `0.9.8`;
3. the new gate stays green the whole time, because it was never asked.

The check is correct and its verification is sound. **Its placement makes it inert for releases**, which
was the entire point of AUD-09 ("a tag push with three of four bumped ships a mislabelled build"). The
fix should be duplicated into `release.yml` (or the release job should depend on it), and ideally also
invoked by a `workflow_call` so there is one implementation rather than two.

**AUD-09 is therefore PARTIALLY VERIFIED, not VERIFIED.** I caught this only because the brief asked
whether another path bypasses each fix — the job's own behaviour is fine.

### ADV-04 — release workflow does not gate on anything (INFO, adjacent)

Related, and worth one line: `release.yml` has no signing or notarisation secrets (`APPLE_*`/`CSC_*`
absent — consistent with the audit's AUD-13 INFO finding) and no dependency on the version check. Its
only trigger guard is the tag pattern. Combined with ADV-03, the release path currently has **no
pre-publication validation at all** beyond "the tag matched `v*`".

---

## 3. What I attacked and could not break

Each of these was challenged with a specific attack, not merely re-read.

| Item | Attack attempted | Result |
|---|---|---|
| **SequenceTracker reorder window** | Revert-to-strict-watermark; window overflow; huge sequence jump; duplicate; `seq == 0` | **VERIFIED FIX.** A strict watermark (`seq <= last`) rejects the reordered-but-legitimate message; the sliding window accepts it. The unit test at `protocol/mod.rs:611-618` is a **deterministic control** — reverting to a strict watermark makes `accept(WINDOW-1)` after `accept(WINDOW)` return `false` and the assertion fail. The `received = 0` reset on a large advance is safe: sequences older than the window are rejected independently by the `age >= WINDOW` check. |
| **Sequence enforcement bypass** | Is there any inbound control path that skips the tracker? | **No.** Exactly one enforcement point, `validate_authenticated_sequence` (`quic.rs:2334`), and it runs **identity binding first** (`authenticated_device_id == sender`, else `SENDER_MISMATCH`), then `accept(seq)`, all under the session lock → **no race, no bypass**. |
| **Stale session generations** | Does a stale commit mutate the room? | **VERIFIED FIX.** `if state.session_generation != session_generation { return; }` (`app_runtime.rs:3504`) aborts *before* any mutation, with an additional `operation_id` check at `3514`. My new `committed_playback` write sits **after** both guards (`3524`), so a stale commit cannot set the anchor. |
| **Chrome process-tree cleanup** | Can it kill the wrong process group? | **VERIFIED.** Unix: `process_group(0)` at spawn, `killpg` on teardown, with a safety check refusing `pgid <= 1` or its own group (`process.rs:270`). Windows: job object with `KILL_ON_JOB_CLOSE` plus `taskkill /PID x /T /F` fallback. |
| **Chrome generation / reinsertion** | Can a torn-down session be reinserted? | **VERIFIED at source.** `bump_chrome_generation` on every transition; the teardown captures `chrome_generation` (`5476`) and reinsertion is gated on `chrome_session_may_be_reinserted(&state, chrome_generation)` (`5517`). **NOT verified under real Chrome** (see §5). |
| **rustls 0.23.45** | Was the Batch 4 bump real? | **VERIFIED.** `Cargo.lock` resolves `rustls 0.23.45`. |
| **TLS CertificateVerify** | Does pinning alone authenticate? | **VERIFIED.** `verify_tls13_signature` (`quic.rs:1567`) delegates to `rustls::crypto::verify_tls13_signature` against the certificate's own key, and there are **genuine-vs-forged controls** (`3597` genuine accepted, `3610` forged rejected). |
| **Fingerprint pinning** | Userinfo/host-confusion trick? | **VERIFIED.** SHA-256 compare with explicit mismatch rejection (`1552-1556`). 0-RTT is disabled **and asserted** (`max_early_data_size == 0`, `enable_early_data == false`). |
| **Join-secret authentication** | Is the ordering exploitable? | **VERIFIED.** shape → secret-hash equality → **host-side expiry** → signature over `(…, join_secret_hash, …)` → `accept_once(device_id, nonce)` → `bind_identity` **after** the signature check. Expiry deliberately sits behind the secret comparison so only a secret-holder learns the invite lapsed. |
| **Invite expiry** | Is host-side expiry actually *wired*, or inert? | **VERIFIED — and this was the attack I most expected to land.** `with_invite_expiry` is called in production at `app_runtime.rs:2906`; `parse_invite` rejects `expires_at_ms <= 0` and `now >= expires_at_ms` on the guest side. Not inert. |
| **Authenticated peer cap** | Can a second peer authenticate? | **VERIFIED.** `MAX_AUTHENTICATED_PEERS = 1` with a **compile-time assert** (`const _: () = assert!(MAX_AUTHENTICATED_PEERS == 1)`), and an RAII `AuthenticatedPeerGuard` released on teardown → cannot leak the slot. |
| **URL parsing** | `https://youtube.com@evil.com/…`, suffix confusion, port, trailing dot | **VERIFIED for the classic tricks.** `host_from_url` takes everything before the first `/?#`, and `is_supported_youtube_host` compares/suffix-matches the **whole** host string, so `youtube.com@evil.com` (ends `evil.com`) and `notyoutube.com` (last 12 chars `tyoutube.com`) are both rejected. **Minor false negative:** a valid trailing-dot FQDN (`youtube.com.`) is rejected — a usability nit, not a security hole. |
| **Identity rotation** | Can a half-rotated identity persist? | **VERIFIED.** `rotate_identity` (`1112`) fails closed and surfaces `MP-SECURE-001`, with a dedicated control test that the rotation **aborts** when the previous row cannot be cleared (`11224`). |
| **AUD-04 / AUD-05 (my fix's tests)** | Can the regression test fail for the real defect? | **VERIFIED, twice.** Mutation control run before *and after* the narrowing: disabling the projection makes the test fail with the defect's own signature, `seeked to [1000000, 1000000, 1000000]`. The control disables the whole mechanism, not one guard. |
| **AUD-09 (version gate)** | Does the new CI job actually catch a mismatch? And does it run when it matters? | **PARTIALLY VERIFIED.** The script is correct: extracted verbatim from `ci.yml`, run under `bash` as the ubuntu runner does (exit 0, all four agreeing), and now **executed on CI** (`35502788709`, printing `OK: all four version declarations agree (0.9.8)`). **But it lives in a `workflow_dispatch`-only file and never runs on a tag push — see ADV-03.** |
| **Release workflow** | Does anything validate before publishing? | **GAP (ADV-04).** `release.yml` fires automatically on `push: tags: ["v*"]`, has no signing/notarisation secrets, and has **no dependency on the version check**. Combined with ADV-03 the release path has no pre-publication validation beyond the tag pattern. |
| **Frontend command error propagation** | Are errors swallowed? | **VERIFIED at source.** `backend/appRuntime.ts` has explicit `catch` blocks and re-throws (`841-844 throw commandError`). One deliberate swallow exists — `surfaceOpQueue = run.catch(() => undefined)` (`729`) — for a background op queue, which is the correct shape for a fire-and-forget surface queue. |
| **Database failure propagation** | Swallowed `Result`? | **VERIFIED at source.** Every `let _ =` in `storage/sqlite.rs` is test cleanup (line ≥1251) or the best-effort `ROLLBACK` (`284`). Migrations remain atomic per step with `SchemaTooNew` guarded, as the original audit found. |

---

## 4. Side effects and races around the fixes

- **Races introduced by the fixes:** none found. The new state (`committed_playback`) is read and written
  only under the runtime `state` lock, and the anchor write sits behind both generation guards. The
  clock offset is likewise read/written under the same lock, so there is no torn `i64` read.
- **Semantic overloading (minor, INFO):** `LocalSyncCoordinator::ended()` sets `paused_by_strict_sync =
  true`, reusing a flag whose name means "paused *because of* strict sync". The only consumer that
  combines it with a room state requires `Buffering` (`media/local_perfect.rs:157`), so `Ended` cannot
  trigger it. Harmless today; the flag now means two things.
- **`keep-open=yes` interaction with the render path:** the Batch 5 note recorded that past
  end-of-media `mpv_render_context_render` writes an all-zero frame. With `keep-open` the last frame is
  held, so post-EOF renders now carry real pixels rather than zeros — a *change* in that behaviour. It
  does not break `real_sw_render_test` (which renders during playback and asserts ≥10 pixel-bearing
  frames); it passes locally. Worth knowing that the premise of that note has shifted.
- **CI skip-list fragility (ACCEPTED LIMITATION):** the list is by name and now has 9 entries. A new
  libmpv-dependent test that is not added there will **panic on CI** and turn the build red, because
  `tests/common/mod.rs` fails loudly by design. That is a deliberate trade, but it is a tripwire.
- **No bypass found for any fix** except the AUD-01 fallback described below.

### The AUD-01 residual window (PARTIALLY VERIFIED)

`drift_correction_for_player` falls back to `state.sync.position_ms` when the projection returns `None`
— which happens when `committed_playback` is `None`. So a guest that is `Playing` **without** a recorded
anchor still uses the pre-fix comparison. I traced whether that state is reachable and it is narrow: the
host sends `PlayCommit` before the coordinator's `PLAYING`, so on the normal play path the anchor is
always set first. The fallback therefore covers only the settling edge, which is what its comment claims.
It is a real residual hole, but I could not construct a path that reaches it.

---

## 5. Still requiring real Windows CI or real two-device manual validation

Stated plainly, because local green is not evidence for any of these:

| # | Requires | Why local cannot settle it |
|---|---|---|
| W1 | **Windows CI** (already green at `3372d48`) | Done for compilation and the platform-neutral suites. **Windows *playback* is unobserved** — the new real-hardware tests are `#![cfg(target_os = "macos")]`, so Windows runs zero of them. Windows EOF rests on `eof-reached` + `keep-open`; Windows audio on `ao=auto` selecting WASAPI. Neither observed. |
| W2 | **Two real devices** | No automated gate in this repository has ever proven two devices staying in sync. AUD-01's fix is proven at the loop level with a scripted player; nobody has watched a real decoded frame stay in sync. |
| W3 | **A real movie with sound, on real hardware** | The audio test runs headless with `ao=null` — **no sound reached a speaker**. Audio *decode* and AO initialisation are verified; audibility is not. |
| W4 | **A multi-hour session** | The projection now depends on the continuously re-calibrated offset. Whether it stays accurate enough over hours is unverified — and ADV-01 makes this more important, not less. |
| W5 | **Real Chrome** | Chrome generation/reinsertion is verified at source only. The one test that drives real Chrome is `#[ignore]`d and needs live network. |
| W6 | **macOS hardware for EOF/audio** | Verified on this machine only, with the gitignored runtime staged. A clean checkout runs zero of those tests. |

`BATCH7B_REAL_BETA_VALIDATION.md` (28 areas, T01–T28) remains the procedure for W2–W4. **Every row is
still NOT TESTED.**

---

## 6. Release recommendation

**Not given, per your instruction — and I would not give one from local tests in any case.** What the
evidence supports is narrower:

- Every P0/P1/P2 fix **blocks its original failure mechanism**, and for AUD-01 that is demonstrated by
  reconstructing the defect on demand.
- The candidate is **CI-green on both platforms** for compilation and the platform-neutral suites.
- **ADV-01 is a real, unguarded defect introduced by the remediation**, with a P2 impact and a low
  likelihood. I would fix it (guard + clamp) before any release, because the clamp is cheap and bounds
  causes I have not imagined.
- **ADV-02 is latent** and I could not reach it; fix it alongside ADV-01, since it is one line.
- **ADV-03 is the one I would fix first of the three**, because it is the cheapest and it protects the
  irreversible step. A mislabelled published release cannot be un-published; a bad projection can be
  fixed and re-shipped. Move the version check into `release.yml` (or make the release job depend on it)
  before tagging anything.
- **ADV-04 is the same area** and worth a decision rather than a default: the release path currently
  validates nothing. Whether it should is your call, but it should be a *decision*.
- Nothing here addresses W1–W6, and W2 is the only thing that speaks to the actual product promise.

One process note, offered as a limitation rather than a finding: this pass attacked each item **once,
along the path I thought most likely**. That is not exhaustive. The three findings I did produce all
came from asking "what does this new value depend on, and who bounds it?" — a question I would apply to
every fix in the next pass too.

---

## 7. Remediation of these findings

*Added after the fact. Everything above is the audit as performed; this section is what happened next.*

| Finding | Verdict | Fix |
|---|---|---|
| **ADV-01** (P2) | **FIXED** | `drift_correction_for_player` now guards on `clock_calibrated`, and `projected_host_position_ms` is clamped to the media duration. |
| **ADV-02** (P3) | **FIXED** | `committed_playback` is cleared in `leave_party` and in the create/join reset (`return_home` reaches it through `leave_party`). |
| **ADV-03** (P2) | **FIXED** | The check moved to a reusable workflow (`version-consistency.yml`); `ci.yml` calls it and `publish-tauri` now `needs: [resolve-matrix, version-consistency]`, so a version mismatch **blocks the publish**. |
| **ADV-04** (INFO) | **ADDRESSED** | The release path now has pre-publication validation. Whether it should have more is a decision, not a default. |
| **AUD-08** | **RE-EXAMINED — finding is stronger** | The shared-stream transport API is **gone from `network/quic.rs` entirely**, so the file is *orphaned*, not merely stale. "Declare and repair" is therefore impossible; deletion remains the owner's call. |

**Why the guard is a silent degradation, stated as a trade rather than hidden:** an uncalibrated guest
now gets **no** drift correction instead of a **wrong** one. Free-running playback is watchable; a seek
to the end of the film is not. Surfacing the condition in the UI would be a follow-up — it would mean
inventing a user-facing state, which is a product decision.

**Mutation controls, run together.** With the guard, the clamp *and* the anchor clear all disabled, the
three new tests go **red with their own specific messages** (3 failed / 0 passed), then the mutations
were reverted and verified by hash:

```
uncalibrated_guest_gets_no_drift_correction ... FAILED
  an uncalibrated guest must not be drift-corrected; got Some((0, 1000000))
projection_is_clamped_to_the_media_duration ... FAILED
leaving_a_party_clears_the_play_anchor ... FAILED
  leaving a party must clear committed_playback, or the next session's
  projection is derived from the previous session's deadline
```

`uncalibrated_guest_gets_no_drift_correction` asserts **both** directions (uncalibrated → `None`,
calibrated → `Some`), so it cannot pass by returning `None` unconditionally — a guard test that only
checked the negative case would be satisfied by breaking drift correction entirely.
`projection_is_clamped_to_the_media_duration` carries a control showing the value is genuinely
unbounded without the clamp, so the clamp is doing the work rather than the input happening to be small.

**Verification after the fixes:** `fmt` clean · `clippy --all-targets --all-features -D warnings`
exit 0 / zero warnings · CI-style suite **571 passed / 0 failed** (568 + the 3 new tests) ·
`m2_integration` 28/28 · all three workflow YAMLs parse and are wired correctly.

**CI run `35505187082` @ `0e736e9` → SUCCESS, all jobs**, and it settles ADV-03 on the real runner:

| Job | Result |
|---|---|
| Rust (macos-latest) | ✅ **571 passed / 0 failed / 2 ignored / 8 filtered** (568 + 3) |
| Rust (windows-latest) | ✅ **562 passed / 0 failed / 2 ignored / 1 filtered** (559 + 3) |
| **`version-consistency / Version consistency`** | ✅ — the `workflow_call` prefix in the job name is the proof that the **reusable workflow executed through the `uses:` call**, and it printed `OK: all four version declarations agree (0.9.8)` |
| Frontend / Cargo audit / pnpm audit | ✅ |

Both platforms gained exactly **+3**, which is the three new unit tests.

**Still open, unchanged by this work:** W1–W6 in §5. ADV-01's *symptom* is now bounded rather than
impossible — an uncalibrated guest free-runs — and only a real two-device run will show whether
calibration is reliable enough in practice for that trade to be invisible.

---

## 8. Second pass — completing the remaining audit items

*Added while working through the items this audit left untouched. AUD-08 was explicitly deferred by the
owner; everything else is resolved or explained below.*

### ADV-05 — NEW BUG (P2), a user-visible regression from my own AUD-03 fix. FIXED

Found by asking **"who else reads this flag?"** rather than by re-reading the diff.

`LocalSyncCoordinator::ended()` set `paused_by_strict_sync = true`, and `apply_end_of_media` set
`state.sync.strict_sync_paused = true` directly. That flag is mirrored into the snapshot, and the
frontend feeds it straight to `BufferingOverlay`, which renders whenever it is true:

```
<h1>Paused to keep you together</h1>
<p>{peer} is buffering</p>   + a buffer meter
```

So **at the end of every movie** the app announced that the peer was buffering and drew a progress
meter for a download that was not happening. `BufferingOverlay` does not consult the room state, so the
ended room did not suppress it.

Both writes are removed — for the same reason. The reasoning behind them was "an ended room must not
keep correcting drift", but that is **redundant** (`drift_correction_for_player` already refuses unless
`room_state == Playing`, and `ended()` sets `Ended`) and **wrong** (the flag means "paused by strict
sync", and the UI acts on that meaning).

**The AUD-03 test had codified the bug** — it asserted `strict_sync_paused` was `true` after
end-of-media. It now asserts the opposite, and asserts the property that actually matters directly
(`drift_correction_for_player` is `None` at `Ended`), so removing the flag cannot silently re-enable
correction. Mutation control with **both** mechanisms restored: the test goes red with the specific
message, then both files were reverted and verified by hash.

The two remaining setters are legitimate and unchanged: `buffer_low` (room becomes `Buffering`) and
`peer_disconnected` (room becomes `Reconnecting`) — both cases where the overlay is correct.

### AUD-13 — ALREADY SATISFIED, better than the audit assumed

The audit said the unsigned/un-notarised reality "must be in the release notes". It already is, and on
the surface users actually read: `docs/RELEASE_BODY_FOOTER.md` carries the downloads table, the fact
that the build is **ad-hoc signed and NOT notarized**, the exact Gatekeeper wording for each macOS
version ("damaged and can't be opened", "cannot be opened because Apple cannot check it", "could not
verify … is free of malware"), the Control-click → Open walkthrough, the `xattr -d
com.apple.quarantine` alternative, an explicit "do **not** disable Gatekeeper system-wide", and a
statement that there is **no update mechanism by design** and that `.app.tar.gz` is not an updater
artifact.

`release.yml` appends it to every release body and **fails loudly** if the file is missing. No work
needed — recorded because the audit's INFO entry implied otherwise.

### AUD-10 — VERIFIED (dead surface confirmed), deliberately not removed

The five commands are registered in `generate_handler!` and have **zero** frontend references; the live
API is `create_and_broadcast_schedule` / `cancel_and_broadcast_schedule` / `guest_accept_schedule` /
`list_schedules`. (The audit's list was slightly off: it named `update_and_broadcast_schedule` as dead
while implying the `*_and_broadcast` family was live — the cross-check above is authoritative.)

**Not removed.** The audit classified this P3 as "dead surface, **not a defect**", and deleting public
commands has no user-visible benefit and non-zero risk. The audit's real concern — "two parallel
schedule APIs can drift" — is better met by recording which is authoritative, which is what this entry
does.

### AUD-11 — VERIFIED BENIGN, unchanged

The `position_ms == 0` guard is untouched and its behaviour is unchanged, because the narrowed AUD-01
fix deliberately leaves `sync.position_ms` with its prior commit-derived semantics. A legitimate
mid-session position of exactly 0 still re-initialises from the coordinator, but the next commit
overwrites it. Benign; the fragility is real and documented.

### AUD-12 — DELIBERATE, not changed

`buffered_ahead_ms()` returning `None` for mpv is not a defect: the event loop does
`snap.buffered_ahead_ms.unwrap_or(cache_headroom_ms)`, so the app's own sparse-cache accounting is used
instead of mpv's demuxer cache. Implementing mpv's would *replace* a working signal rather than fix a
broken one, and it changes what feeds `report_buffer_status` → strict sync — behaviour that can only be
validated with two real devices. **Changing it without that validation would be trading a known-good
signal for an unvalidated one.** Left as the audit classified it: deliberate.

### AUD-14 — DELIBERATE, with a concrete proposal rather than silence

CI's `real_*` targets execute zero tests because `mpv_runtime/*` is gitignored and CI cannot stage it.
The mechanism to run them already exists — `ci.yml` has a `libmpv runtime state` step that runs
everything when the runtime is present — so closing this needs only a staging step on the macOS runner
(`brew install mpv` + `scripts/stage-libmpv-macos.sh`).

**Not done**, because it is genuinely risky rather than merely effortful: a Homebrew mpv is a
*different build* from the bundled one, so the tests would be validating a runtime the product does not
ship; it adds minutes to every run; and a flaky hardware-adjacent suite in CI is a worse outcome than
an honest zero. This is the highest-value remaining item, and it should be a deliberate decision with
its own batch rather than a side effect of this one.

### AUD-16 — NEEDS A PRODUCT DECISION, not built

Still open from the first report: the data is now truthful (room `ENDED`, player `COMPLETED`) but
nothing surfaces it — `screen` stays `CINEMA`, the sync indicator reads "Syncing", and the play control
offers *resume*, which re-commits play from the end and lands straight back in `Ended`. Surfacing it
means inventing an "Ended" affordance (a replay button? a return to lobby?), which is a product
decision, so I have not invented one.

### AUD-08 — OPEN, deferred by the owner

Unchanged, and now documented as **orphaned** rather than stale: the shared-stream transport API is gone
from `network/quic.rs`, so "declare and repair" is impossible. Awaiting detailed instructions.

### CI verification of this second pass

**Run `35506106235` @ `7aea3e2` → SUCCESS, all jobs** — macOS **571 passed / 0 failed**, Windows
**562 passed / 0 failed**, and `version-consistency / Version consistency` green through the reusable
workflow again. The counts are unchanged from the previous run because the ADV-05 fix replaced one
assertion with a stronger one rather than adding a test.

**Net across both passes: four of my own fixes had defects, and every one was found by asking a
different question than the one that produced the fix.**

| Pass | Finding | Question that found it |
|---|---|---|
| 1 | ADV-01 — projection depends on an uncalibrated clock | *What does this new value depend on, and who bounds it?* |
| 1 | ADV-03 — the gate never runs on a tag push | *Does another path bypass the fix?* |
| 2 | ADV-05 — the app says "peer is buffering" at the end of every film | *Who else reads this flag?* |
| — | AUD-08 — the file is orphaned, not stale | *Can this actually be repaired, or only documented?* |


