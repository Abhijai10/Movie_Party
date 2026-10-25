# Movie Party — final integration report (RC validation)

Date: 2026-10-26. Branch: `codex/integrated-youtube-player`.
Candidate version: 0.9.9 (unpublished; no tag, no release created).

## Verdict

**NO-GO for release.** Automated validation is green on macOS, but **no physical
two-device session has been run** — not on macOS, not on Windows — and Phase E
cannot be performed from this machine. Per this batch's own rule, the product is
not release-ready until that evidence exists. Nothing was published; no tag was
created; no release asset was uploaded.

## Phase A — reconciliation

| Check | Finding |
| --- | --- |
| Remote / lineage | `origin/main` = `5014033`; the candidate branch is **33 commits ahead, 0 behind** — origin/main is the merge-base, so integration needs no conflict resolution |
| Batch commits | All 33 reviewed in `git log origin/main..HEAD`; they map one-to-one onto Batches 5–10 plus the dependency patch |
| Duplicate protocol | One definition: `ClientRequest` / `ServerResponse` / `ServerEvent` exist only in `network/quic.rs`; host and guest run the same crate, so there is no second authoritative room or selection protocol |
| Protocol versions | `PROTOCOL_MAJOR/MINOR = 1.0` in `lib.rs`, `ENVELOPE_V_MAJOR/MINOR = 1.0` in `protocol/mod.rs`, and `SELECTION_VERSION` on the selection path — host and guest compare the same constants |
| Chrome crash/lifecycle fixes | Present: `unexpected_browser_exit_releases_profile_and_requires_readiness_again`, `launch_timeout_kills_and_reaps_the_whole_tree`, `shutdown_wait_includes_sessions_held_outside_runtime_state`, `profile_storage_survives_managed_close_and_relaunch` |
| Provider Shared | Disabled and pinned by `capabilities_expose_sync_without_claiming_shared_readiness` (`shared_available == false` for all four providers); no capture pipeline exists |
| Abandoned flags | None. Every `MOVIE_PARTY_*` token is a const, a test-only override (`DB_PATH`, `COMPANION_ENDPOINT`), or a dev override (`LIBMPV_PATH`); `shared_controls` defaults to `false` |
| Secrets | Clean: no `.env`/key/credential files added; no private-key blocks, `tskey-`, `ghp_`, `AKIA`, or `client_secret` in tracked source or in the 33-commit diff. The only token-shaped literal is inside `#[cfg(test)]` and exists to prove scrubbing |
| Merge | No merge required (fast-forward available). Remote tags untouched; the newly fetched `v0.9.9-rc1` tag was not modified |

## Phase B — automated validation

| Gate | Result |
| --- | --- |
| `tsc --noEmit` | PASS (0 errors) |
| `eslint . --max-warnings=0` | PASS (0 problems) |
| `vitest run` | PASS — **468 tests / 42 files** |
| `pnpm build` | PASS |
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets --all-features -D warnings` | PASS |
| `cargo test --all-targets -- --test-threads=1` | PASS — **845 tests in 20 binaries (0 failed, 5 ignored by design)** |
| Provider protocol integration | PASS — `provider_selection_quic`, provider selection/consensus suites |
| Browser lifecycle | PASS — incl. real Chrome launch/reap/profile tests |
| YouTube player | PASS — IFrame driver, obstruction guard, runtime error mapping |
| Other-provider adapters | PASS — content identity, episode collapse, evidence tiers (fixtures only; no live sessions) |
| Friend invitation/security | PASS — signing, replay, block/remove, consent gating, invite ledger |
| Tailscale onboarding state | PASS — reducer + runtime + live CLI identity test |
| `cargo audit` / `pnpm audit --prod` | PASS — 0 vulnerabilities (7 dev-only transitive findings documented) |
| GitHub CI (macOS + Windows) | **NOT RUN** — dispatched on the candidate SHA (see Phase D); no CI run had completed at the time of the gate run |

### Executions vs skips vs simulations

- **Actually executed on this machine:** all of the above, plus the real libmpv
  binaries (real decode, seek, pause, CALayer presentation, audio, EOF) and the
  opt-in visible-Chrome suite.
- **Skipped by design:** 5 ignored Rust tests that need a second device or
  environment; the Windows-only native-surface test.
- **Simulated/mocked:** every subscription-provider row (S1–S16) runs against
  fixtures or localhost pages only. No Netflix/Prime/JioHotstar account was used.
- **Unavailable physical devices:** no second Mac, no Windows machine, no second
  tailnet.

### One gate failure, characterised

The first full serial run reported `test_r_reconnect_requires_fresh_consensus`
failing with "host predicate not satisfied within 30s". It **passes in isolation
in 2.3 s**. The failing run was executed concurrently with the frontend gates
(`tsc`/`vitest`/`vite build`), which starves the 30-second wait. The authoritative
re-run with nothing else running is the one recorded above. Recorded as **P2**:
the test's timeout is load-sensitive; the product behaviour it covers is fine.

## Phase C — functional matrix

Automated evidence marks what the suites actually prove. Physical rows are
`NOT_TESTED` and are the reason for the NO-GO.

### Local movie

| Check | Evidence | Status |
| --- | --- | --- |
| Host and guest, loopback | `m2_integration`, `media::local_perfect` | PASS (automated) |
| Media transfer + cached same-file detection | manifest full-hash/media-id tests, transfer suite | PASS (automated) |
| Audio and video | real libmpv binaries (decode, audio track) | PASS (automated) |
| 3-2-1 synchronization | coordinator/clock tests | PASS (automated) |
| Pause/resume/seek | real playback smoke (play/pause/seek moves the playhead) | PASS (automated) |
| Buffering/disconnect/recovery | reconnect + fresh-consensus tests | PASS (automated) |
| Late join | guest reconnect/ready-state tests | PASS (automated) |
| EOF/completed | real EOF binaries + production-player lifecycle | PASS (automated) |
| Two real machines | — | **NOT_TESTED** |

### Provider mode

| Check | Evidence | Status |
| --- | --- | --- |
| YouTube browse/search, canonical identity | content identity + YouTube adapter tests | PASS (automated) |
| Host choice → guest, same video | selection-over-QUIC tests | PASS (automated) |
| READY consensus, play/pause/seek, content change | consensus + coordinator tests | PASS (automated) |
| Browser/profile persistence, crash recovery | real Chrome profile/relaunch tests | PASS (automated) |
| Unsupported embed fallback | IFrame driver tests | PASS (automated) |
| Netflix/Prime/JioHotstar authorized sessions | checklist S1–S16 | **NOT_TESTED (blocked: no accounts)** |
| No provider credentials transferred | invite/provider payload tests; no credential field exists | PASS (automated) |

### Social

| Check | Evidence | Status |
| --- | --- | --- |
| Create / accept / reject invitation | friendship + runtime suites | PASS (automated) |
| Verify identity, fingerprint | identity + invite signature tests | PASS (automated) |
| Invite a friend to a party | room invite + trust separation tests | PASS (automated) |
| Block / remove (separate from Tailscale) | lifecycle + three-action separation tests | PASS (automated) |
| Revoked/expired invitations, replay | ledger + expiry + replay tests | PASS (automated) |
| Trust vs Tailscale permissions kept separate | Batch 8/10 tests | PASS (automated) |
| Two real people | — | **NOT_TESTED** |

### Tailscale

| Check | Evidence | Status |
| --- | --- | --- |
| Same-tailnet connectivity | loopback + tailnet tests | PASS (automated) |
| Device-share cross-tailnet connectivity | direction logic only | **NOT_TESTED (needs 2 tailnets)** |
| One-way quarantine behaviour | `classify_direction` + `MP-ONB-012` tests | PASS (reasoned), **NOT_TESTED (media/calls)** |
| Wrong tailnet / expired key / offline | `MP-ONB-004/005/007` + live identity test | PASS (automated) |
| Invitation rejection | decline/expiry tests | PASS (automated) |
| SSH/remote diagnostics | — | **NOT_TESTED** (out of product scope) |

### Call / UX

| Check | Evidence | Status |
| --- | --- | --- |
| No call / voice / video modes | call state machine + WebRTC tests | PASS (automated) |
| Mic muted initially | call state tests | PASS (automated) |
| Call recovery | renegotiation tests | PASS (automated) |
| Chat, reactions | chat + reaction suites | PASS (automated) |
| Ghost / Privacy | privacy + ghost UI tests | PASS (automated) |
| Provider browser window transitions | real Chrome window tests | PASS (automated) |
| Windows/macOS installer lifecycle | — | **NOT_TESTED** (no Windows machine) |

## Phase D — installer build

| Item | Value |
| --- | --- |
| EXACT_FINAL_RC_SHA | ``d2ce3e03ee10d4c9a4eb54aa7b35eba186351589`` |
| Local macOS build | not built locally; the approved `rc-build.yml` workflow is the only build path used |
| CI dispatch | `rc-build.yml` on `d2ce3e0` — [run 40236368933](https://github.com/Abhijai10/Movie_Party/actions/runs/40236368933), dispatched 2026-10-26, in progress at report time |
| Artifacts (names) | pending the dispatched run; names will follow the workflow's `Movie Party_0.9.9_*` pattern (Windows x64 NSIS EXE, macOS ARM64 DMG) |
| SHA-256 checksums | pending the dispatched run; the workflow computes and uploads SHA-256 checksums itself |
| Tag / release created | **None** (by design and by instruction) |

## Phase E — physical validation

**NOT PERFORMED.** No Windows machine, no second Mac, no second tailnet account.
A CI runner is not a physical test and this report does not treat one as such.

To run it, on the exact candidate above:

1. Install the DMG (macOS ARM64) and the NSIS EXE (Windows x64) from the RC run.
2. Sign both devices into Tailscale (same tailnet **and** the device-share
   variant, in separate sessions).
3. Complete the friend flow: invite → accept on the other device → verify the
   fingerprint matches on both screens.
4. Run the local-movie matrix (transfer, cache hit, sync, pause/seek, buffering,
   disconnect/recovery, late join, EOF) and record observed values.
5. Run the provider matrix for YouTube, then for each subscription provider where
   an authorized session exists.
6. Run the Tailscale matrix, including the sharing-direction test (host shares →
   guest dials) and the call matrix.
7. Record PASS/FAIL/NOT_TESTED per row in `docs/ONBOARDING_MANUAL_CHECKLIST.md`,
   `docs/PROVIDER_VERIFICATION_CHECKLIST.md` and `docs/COMPANION_SETUP.md`.

If any row fails: record the reproduction, fix narrowly, produce a **new SHA**,
rerun these gates, and rebuild both artifacts.

## Provider capability matrix (unchanged by this batch)

| Provider | Sync | Shared | Verification |
| --- | --- | --- | --- |
| YouTube | yes | **no** | LIVE_VERIFIED_MACOS_PARTIAL |
| Netflix | yes | **no** | NOT_LIVE_VERIFIED |
| Prime Video | yes | **no** | NOT_LIVE_VERIFIED |
| JioHotstar | yes | **no** | NOT_LIVE_VERIFIED |

## Friend / Tailscale onboarding status

- Friendship: signed, expiring, single-use invitations; acceptance required;
  block/remove recorded as identity positions; the unsigned legacy path and the
  no-consent commands are gone.
- Ten of the fourteen `MP-ONB` states are covered by automated tests; the
  remaining ones need the CLI conditions injected on a real machine.
- Tailscale control-plane automation: **none**, by design (see
  `TAILSCALE_ONBOARDING_SECURITY_REVIEW.md`). Sharing remains a guided console
  step; the app observes revocation but does not perform it.

## Security limitations

1. **No physical two-device validation** — the release blocker.
2. Subscription providers are unverified against real pages; capability matrix
   says so on the surface.
3. Windows is unbuilt and untested here; the RC workflow is the first real
   Windows build of this candidate.
4. Shared browser / Provider Shared: not implemented; DRM makes it impossible for
   subscription content.
5. Dev-only dependency advisories (7) remain with no in-range fix; shipped
   dependencies are clean.
6. `test_r_reconnect_requires_fresh_consensus` is load-sensitive (P2).

## Issues

| Severity | Issue | Status |
| --- | --- | --- |
| **P0** | No physical two-device validation; Windows never executed | Open — blocks GO |
| **P1** | Subscription-provider live rows S1–S16 unrun (no authorized accounts) | Open — blocks provider claims |
| **P2** | `test_r_reconnect_requires_fresh_consensus` times out under concurrent load (passes isolated and in the serial suite) | Recorded |
| **P2** | Windows installer/NSIS lifecycle never run | Open |
| **P3** | 7 dev-only dependency advisories without in-range fixes | Accepted, documented |

## Recommendation

**NO-GO.** Ship no release from this SHA. The automated surface is in good shape:
468 frontend tests, 845 tests in 20 binaries (0 failed, 5 ignored by design), clean lint/format/build, zero shipped
dependency vulnerabilities, and every batch report's claims mapped to a test or an
explicit `NOT_TESTED`. What is missing is the only evidence that can justify a
release: two real people, two real machines, one real session.
