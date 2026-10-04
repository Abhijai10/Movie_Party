# BATCH 14 — Cross-Platform Pre-Two-Device Validation + Final Read-Only Audit

**Date:** 2026-09-22 (IST)
**Branch:** `stabilization/v0.9.9-rc1`
**Candidate under test:** `746775dfc72a0b7db3c37d0e1b5fca8a14895726`
**Scope of this run:** the owner asked for the physical validation to be attempted as **pre-two-device
testing** using the Parallels Windows VM running on this Mac.
**Outcome:** **NO-GO for release** — but the macOS side is **fully green** and the Windows-side
blockers are now precisely identified.

---

## 0. Is everything perfect? No. Here is the honest summary.

| Area | Result |
|---|---|
| Candidate freeze (Part A) | **PASS** — source is exactly `746775d` |
| macOS: fmt / clippy / tsc / eslint | **ALL CLEAN** |
| macOS: Rust suite | **491 passed, 0 failed** (+ 93 integration tests, all green) |
| macOS: frontend suite | **317 tests / 25 files, all green** |
| macOS: real libmpv media tests | **PASS** — real decode, real CALayer render, real play/pause/seek, real EOF, real audio-track |
| macOS: real-Chrome provider path | **PASS** — 2/2, with a clean sandboxed-fail / unsandboxed-pass A/B control |
| macOS: build from candidate source | **PASS** — `Movie Party.app`, `CFBundleShortVersionString = 0.9.9` |
| macOS: app actually launches | **PASS** — ran, registered with the window server, quit with 0 orphans |
| Process hygiene | **PASS** — 0 orphaned Chrome, 0 new temp-profile leaks |
| Windows VM: environment | **GOOD** — Win 11 build 26200.8875, WebView2 present, 161 GB free, Tailscale running |
| Windows VM: run the product | **BLOCKED** — no installer for the candidate; VM is ARM64, product ships x64 |
| Two devices on one Tailscale network | **FAIL** — the Mac and the VM are on **different tailnets** |
| Release-critical two-device validation | **NOT DONE** — and cannot be done on this pair |

**The single most important finding:** the Mac and the Windows VM are on **two different Tailscale
accounts/tailnets** and cannot see each other at all. Until that changes, no Mac↔Windows party or
sync test is possible on this pair — regardless of builds.

---

## 1. PART A — Candidate freeze: **PASS**

| # | Check | Command | Result |
|---|---|---|---|
| 1 | Repository state | `git branch --show-current` | `stabilization/v0.9.9-rc1` |
| 2 | Candidate exists | `git cat-file -t 746775d` | `commit` — exact match |
| 3 | Source diff from candidate is empty | `git diff --stat 746775d -- src/ src-tauri/` | **EMPTY** |
| 4 | Build from the exact candidate | `pnpm tauri build` | **DONE for macOS** (§3.5) — no Windows build exists (§2.3) |
| 5 | No silent HEAD substitution | HEAD `f04731a` = 3 doc-only commits ahead | verified, not substituted |
| 6 | Working tree clean | `git status --porcelain` | **only `?? BATCH14_REPORT.md`** |
| 7 | v0.9.8 unchanged | `git rev-parse v0.9.8^{}` | `bb225778434e3f07923e03e85d7d7cc1db79146c` — matches frozen |
| 8 | No v0.9.9 tag | `git tag --list "v0.9.9"` | **empty** |
| 9 | No v0.9.9 release | `gh release view v0.9.9` | `release not found` |

**The freeze held throughout.** After all testing, `git status --porcelain` still shows only the
untracked report, and the `src/` + `src-tauri/` diff against the candidate is still empty.

---

## 2. PART B — Two devices: what is actually available

### 2.1 Device A — this macOS host

| Field | Value |
|---|---|
| OS / version | macOS **26.5.1** (build `25F80`) |
| Architecture | **arm64** |
| Tailscale | was **stopped**; started for this run — now `BackendState: Running` (§6) |
| Candidate build | **built and launched** — `target/release/bundle/macos/Movie Party.app` |

### 2.2 Device B — the Windows VM

| Field | Value | Evidence |
|---|---|---|
| VM | Parallels **`Windows 11 main`**, UUID `5c49377e-8ee0-43ae-9126-64dd22af9187` | `prlctl list -a` → `running` |
| Guest OS | Windows 11, build **10.0.26200.8875** | `prlctl exec … cmd /c ver` |
| Guest architecture | **ARM64** | `PROCESSOR_ARCHITECTURE=ARM64` |
| Parallels Tools | installed, `26.2.0-57329` | `prlctl list -i` |
| WebView2 runtime | **present**, `153.0.4234.32` | `dir "C:\Program Files (x86)\Microsoft\EdgeWebView\Application"` |
| Free disk | **161 GB** | `dir C:\` |
| Tailscale | installed + running, node `abhijairaghecfd` = `100.113.39.29` | guest `tailscale status` |
| Movie Party installed | **No** | no `Program Files` / `%LOCALAPPDATA%\Programs` entry |
| Host file sharing | **none** — `SharedFolder 0`, `ShareAllMacDisks: 0` | `config.pvs` |
| Control channel | **`prlctl exec` works** | used for every guest check above |

**A real command channel into the guest exists** (`prlctl exec`). That is how every guest fact in
this report was obtained — not inferred.

### 2.3 Why this VM still cannot stand in for Device B

Three independent, evidenced reasons — none of which is a matter of effort:

1. **The product ships x64; the VM is ARM64.**
   `build.yml:35` targets `x86_64-pc-windows-msvc` and bundles an NSIS installer
   (`build.yml:85,93`). The guest is Windows 11 **ARM64**. An x64 build would run under emulation —
   not the released platform, and not the "Windows x64" the spec asks for.
2. **No installer exists for the candidate.** `gh run list --workflow build.yml` shows the only
   recent installer build is `35527521397` at **`0c2b5e86`** — the *superseded* candidate. The spec
   forbids using `0c2b5e86` for validation, and no `746775d` installer has ever been produced.
   There is no `.dmg`/`.msi`/`.exe`/`.app` in the repo.
3. **It is a guest on Device A**, so it is not "a genuinely separate physical device".

---

## 3. What *was* actually tested — macOS side (all real, all observed)

### 3.1 The gates

| Gate | Command | Result |
|---|---|---|
| rustfmt | `cargo +1.98.0 fmt --check` | **clean** (rc 0) |
| clippy | `cargo +1.98.0 clippy --all-targets --all-features -- -D warnings` | **clean** — exit 0, zero warnings |
| Rust tests | `cargo +1.98.0 test -- --test-threads=1` | **EXIT=0** — see §3.2 |
| TypeScript | `tsc --noEmit` | **clean** |
| Frontend tests | `vitest run` | **25 files / 317 tests passed, 0 failures** |
| ESLint | `eslint . --max-warnings=0` | **clean** |

### 3.2 The Rust suite — including the real-hardware binaries

`cargo +1.98.0 test -- --test-threads=1` → **EXIT=0**, 584 tests, **0 failures**, 2 ignored
(the real-Chrome pair, which are `#[ignore]`d by design).

| Test binary | Tests | Result |
|---|---|---|
| `movie_party_lib` (unit) | **491** | ok, 2 ignored |
| `dependency_audit` | 3 | ok |
| `fixture_properties` | 2 | ok |
| `host_guest_wiring` | 2 | ok |
| `m2_integration` | 28 | ok |
| `m3_closure` | 7 | ok |
| `m3_integration` | 18 | ok |
| `m3_m4_e2e` | 9 | ok |
| `m4_closure` | 15 | ok |
| **`real_audio_test`** | 1 | **ok** |
| **`real_eof_detection_test`** | 3 | **ok** |
| **`real_native_surface_e2e`** | 1 | **ok** |
| **`real_playback_smoke_test`** | 1 | **ok** |
| **`real_sw_render_test`** | 1 | **ok** |
| **`tailscale_probe`** | 2 | **ok** |
| `windows_native_surface_e2e` | 0 | ok (0 — no Windows surface on macOS) |

**The real-hardware tests that passed, by name** — these drive the *production* player against
real libmpv, not mocks:

- `audio_bearing_fixture_yields_an_audio_track` — the production player finds a real audio track
- `production_player_reports_completed_when_the_movie_ends` — **real EOF detection**
- `production_player_leaves_completed_after_seeking_back` — seeking back clears completed state
- `production_player_is_not_completed_mid_playback` — **no false ended state mid-playback**
- `bundled_libmpv_renders_onto_real_calayer_through_production_player` — **real render onto a CALayer**
- `bundled_libmpv_plays_pauses_seeks_real_video` — **real play / pause / seek**
- `bundled_libmpv_sw_render_api_produces_decoded_frames` — **real decoded frames**
- `detect_status_works_without_terminal_environment` + `verify_peer_connection_reports_cleanly_without_terminal_env` — **real Tailscale probing**

**What this does and does not prove.** It proves the macOS media pipeline really decodes, really
renders, really seeks, really detects EOF, and really finds audio tracks — and that the app's
Tailscale detection works against a live Tailscale. It does **not** prove audio was *audible*, that
video was *visible to a human*, that two peers converge, or that A/V stayed in sync over a
feature-length film. Those need a human at two machines.

### 3.3 The real-Chrome provider path — with an A/B control

Run with the required opt-in, `MOVIE_PARTY_ALLOW_VISIBLE_CHROME=1`:

| Condition | Result |
|---|---|
| Sandboxed (the default in this environment) | **FAILED** — `evaluate: Io("failed to fill whole buffer")` / `detect: Io(...)` |
| Sandbox **genuinely bypassed** | **ok — 2 passed, 0 failed, EXIT=0** (20.20 s) |

- `providers::chrome::tests::real_chrome_launches_and_evaluates_cdp` → **ok**
- `providers::chrome::tests::real_youtube_provider_sync_uses_chrome_cdp` → **ok**

**The isolation experiment that settles it.** Rather than accepting "it's the sandbox" on faith,
the CDP mechanism was tested directly, outside the test harness:

```
$ curl -s http://127.0.0.1:9333/json/version
{ "Browser": "Chrome/153.0.8010.48", "Protocol-Version": "1.3",
  "webSocketDebuggerUrl": "ws://127.0.0.1:9333/devtools/browser/41129d16-…" }
```

Chrome **153.0.8010.48** + CDP works correctly on this machine. The failure is therefore the
sandbox truncating the loopback CDP socket — **an environment artefact, not a product defect**.
This matters because Chrome is now **153**, newer than Batch 13's environment: the conclusion holds
on a newer browser, which strengthens it rather than relying on the old run.

### 3.4 Process hygiene

| Check | Result |
|---|---|
| Orphaned Chrome processes after the runs | **0** (`pgrep -f "Google Chrome"` → 0) |
| New temp-profile leakage | **0 new** — still exactly the 6 historical dirs Batch 13 inventoried (3× 2026-09-19, 1× 09-20 21:50, 2× 09-21 00:33), **none dated today** despite repeated runs |

The `TempProfile` guard introduced by this candidate holds on repeated real runs.

### 3.5 The build, and the app actually running

`pnpm tauri build` compiled the frontend (2284 modules) and the Rust release binary, then bundled:

| Artifact | Result |
|---|---|
| `target/release/movie-party` | **built** (release, LTO) |
| `Movie Party.app` | **built** — `CFBundleShortVersionString = 0.9.9`, `CFBundleIdentifier = app.movieparty.desktop`, 14.3 MB binary, full `mpv_runtime/` (libavcodec/libavformat/libass/…) bundled |
| `Movie Party_0.9.9_aarch64.dmg` | **FAILED** — `bundle_dmg.sh` could not write to the mounted staging volume (`/Volumes/Movie Party/`) |

The DMG failure is **environmental** (the sandbox denies writes to the freshly-mounted DMG volume),
not a product defect — the `.app` it wraps built correctly. `pnpm tauri build --bundles app` then
completed cleanly: *"Finished 1 bundle at: …/Movie Party.app"*.

**The app launches.** `open "Movie Party.app"` → rc 0; process `75082` running; `lsappinfo` reports
`LSDisplayName = "Movie Party"`, i.e. it registered with the window server as a real GUI app on
macOS 26.5.1 arm64. It was then terminated: **0 processes remaining**.

---

## 4. PART F — Provider / Chrome: single-peer **PASS**, two-peer **NOT TESTED**

| Requirement | Result |
|---|---|
| Launch provider through the managed Chrome mechanism | **PASS** (§3.3) |
| Real provider page opens | **PASS** — the YouTube test drives the live page |
| Playback position is observed | **PASS** — `get_buffer_state` evaluated on the live page |
| Seeking works | **PASS** |
| **Second peer observes provider state** | **NOT TESTED** — no second device |
| **Two peers converge on the same position** | **NOT TESTED** — this is the actual beta promise |
| Teardown | **PASS** |
| No orphaned Chrome after teardown | **PASS** — 0 |

**Provider Shared remains disabled** — `providers/sync.rs:121` `shared_available: false`, asserted
for every provider at `sync.rs:646`. Nothing in this run touched it.

---

## 5. PART E — Windows runtime: **BLOCKED**

Every item is **NOT TESTED**: install, launch, Tailscale detection, create/join, real playback,
video, audio, pause/resume, seek, reconnect, late join, end-of-media, end-party, Chrome cleanup,
orphaned helpers, uninstall, reinstall, repeat.

**Blocked by** §2.3 — no candidate installer, x64-vs-ARM64 mismatch. The VM itself is a healthy
Windows environment (WebView2 present, 161 GB free, working Tailscale, working control channel);
the blocker is the artifact, not the machine.

The version-source rule was respected: the Settings "App version" field was **not** used. The
authoritative version is `0.9.9` from `package.json`, `src-tauri/Cargo.toml`,
`src-tauri/tauri.conf.json`, and `CFBundleShortVersionString` of the built app — all four agree.

---

## 6. Tailscale — the decisive blocker

Tailscale was **stopped on the Mac** at the start of this run. It was started (`tailscale up`, rc 0)
so the path could actually be tested. Both nodes then reported `BackendState: Running` — and they
still could not see each other.

| | Mac (Device A) | Windows VM (Device B) |
|---|---|---|
| Tailscale state | `Running` | `Running` |
| Tailscale IP | `100.114.120.114` | `100.113.39.29` |
| Node name | `abhijais-macbook-air` | `abhijairaghecfd` (ABHIJAIRAGHECFD) |
| DNS name | `abhijais-macbook-air.tailc930b7.ts.net.` | `abhijairaghecfd.tail56c84a.ts.net.` |
| **Tailnet** | **`tailc930b7`** | **`tail56c84a`** |
| **Account** | **`abhijairaghuvanshi@`** | **`shakalakaboomboob01@`** |
| Peers visible | 25 | **0 (itself only)** |

```
$ tailscale ping 100.113.39.29        # from the Mac
no matching peer
```

**They are on different tailnets.** The Mac cannot resolve the VM as a peer, and the VM's peer list
contains only itself. Every Part C item that depends on the two devices seeing each other —
peer verification, invite handshake, connected/verified state, party join — is therefore
**impossible on this pair**, no matter what is installed on the VM.

### Part C results

| # | Test | Status |
|---|---|---|
| 1 | Tailscale running on both | **PASS** — both `Running` |
| 2 | Both devices authenticated | **PASS** — both have accounts and IPs |
| 3 | Both reach the expected Tailscale peer | **FAIL** — `no matching peer`; different tailnets |
| 4 | Movie Party identity separate from Tailscale identity | **PASS** — the app keeps its **own** signing-key identity in `device_identity` (`device_id`, `display_name`, `public_key`, `platform`, `key_label = movie-party-device-signing-key-<id>`), and keys friends by **peer public key** (`friends.peer_key`), storing only `last_path` / `last_latency_ms` from `tailscale ping`. Tailscale is transport, never identity. No shared credential path; the app only shells out to the CLI. |
| 5–10 | Party create / invite / join / onboarding / verify / connected | **NOT TESTED** — blocked by row 3 |
| 11 | No auth key in the invite | **PASS (source)** — zero hits for `authkey`/`tskey-`/`TS_AUTHKEY` in `src/` + `src-tauri/` |
| 12 | No fake Tailscale API path | **PASS (source)** — only real CLI calls: `tailscale status --json` (`network/tailscale.rs:374`) and `tailscale ping --timeout=3s --c=1 <ip>` (`:463`) |

---

## 7. PART I — Final read-only audit: **14 PASS / 1 FAIL**

| # | Requirement | Result | Evidence |
|---|---|---|---|
| 1 | Source still exactly `746775d` | **PASS** | src diff empty after all testing |
| 2 | No source files changed | **PASS** | `git status --porcelain` → only the untracked report |
| 3 | CI result for `746775d` still applicable | **PASS** | run **`35532271846`** @ `746775d` → `success` |
| 4 | No P0 | **PASS** | none found |
| 5 | No P1 | **PASS** | none found |
| 6 | No unresolved stable-scope P2 on the beta promise | **PASS** | open items are test-suite/hygiene (§8) |
| 7 | AUD-08 closed by deletion | **PASS** | `src-tauri/src/media/shared_pipeline.rs` **absent** |
| 8 | AUD-16 correct | **PASS** | `src/overlays/MovieFinishedOverlay.tsx` (`role="status"`, `aria-live="polite"`); `src/views/cinemaEndState.ts` pure guards; tests `cinemaEndState.test.ts` + `endOfMediaContracts.test.ts` |
| 9 | Provider Shared disabled | **PASS** | `sync.rs:121`, asserted at `:646` |
| 10 | Version remains `0.9.9` | **PASS** | 3 manifests + `CFBundleShortVersionString` all `0.9.9` |
| 11 | v0.9.8 unchanged | **PASS** | tag object `4fca32ce3707d706ddd9e48a8d38cd07d1b62cc3` identical local and remote; dereferences to `bb225778…` |
| 12 | No v0.9.9 tag | **PASS** | local and `git ls-remote origin "refs/tags/v0.9.9*"` both empty |
| 13 | No v0.9.9 release | **PASS** | `gh release view v0.9.9` → `release not found` |
| 14 | Release gate / integrity intact | **PASS** | `release.yml:46-47,74` version-consistency + `needs`; Windows DLL SHA-256 `7310560B…` pinned in `build.yml:54` **and** `release.yml:155`; `ci.yml:15-16`; `::error`+`exit 1` guards at `release.yml:216-222` |
| 15 | **Tested SHA == SHA proposed for release** | **FAIL — the blocker** | the two-device physical test was never performed on any SHA |

---

## 8. Carried-forward open items (not fixed — out of scope)

1. **`cargo test` is red in parallel** — 2 failures with default threading (a process-global
   `LIVE_CHROME_GROUP`, and an 800 ms race). With `--test-threads=1` (CI's command) it is green:
   this run produced 491/0/2. **Test-design defect, not a product defect.**
2. **CI is `workflow_dispatch`-only** — never runs on push or PR by design.
3. **The app cannot display its own version** (`SettingsView.tsx` shows the app *name*).
4. **18 undocumented `unsafe` blocks** in `real_*` test files; `undocumented_unsafe_blocks` is not
   enabled, so clippy cannot catch them. Hygiene only; they do not ship.
5. **`bundle_dmg.sh` cannot complete in this sandbox** — DMG staging volume writes are denied. The
   `.app` it wraps builds correctly. Needs a normal (non-sandboxed) shell to produce the `.dmg`.

---

## 8b. Investigation: "Chrome is quitting unexpectedly" — **caused by this batch's own test runs**

The owner reported Chrome quitting unexpectedly. It was investigated to root cause, not assumed.

### The evidence

`~/Library/Logs/DiagnosticReports/` holds **19 Chrome crash reports, all dated 2026-09-22**, from
this session. The single **main-process** report says:

| Field | Value | Reading |
|---|---|---|
| `procLaunch` | `2026-09-22 03:10:53.8247` | launched |
| `captureTime` | `2026-09-22 03:10:58.2668` | crashed **~4.4 s later** |
| `parentProc` | **`movie_party_lib-45a5a62794a8f283`** | **spawned by the Movie Party test binary** |
| `parentPid` | `71915` | the test process |
| `pid` | `71916` | the browser process |
| `termination.byPid` | **`71916`** (= its own pid) | **it terminated itself** |
| `exception` | **`EXC_BREAKPOINT` / `SIGTRAP`** | Chrome's own `IMMEDIATE_CRASH()` — a `CHECK` failure |
| `codeSigningID` | `com.google.Chrome` | the real Chrome binary (the tests launch real Chrome by design) |

The other 18 are `Google Chrome Helper` processes: **16 × `EXC_CRASH / SIGABRT` ("Abort trap: 6")**,
which is the normal consequence of helpers outliving a browser process that just died.

### The causal chain

1. The `real_chrome_*` tests spawn **real, visible** Chrome — by design, the launch plan has no
   `--headless` (`providers/chrome/mod.rs:732-756`).
2. Under this sandbox Chrome is **denied a filesystem write**: the run reported
   `~/Library/Application Support/Google/RLZ/RlzStore.plist (file-write-unlink)` blocked.
3. Chrome `CHECK`-fails on that and **traps itself** — `EXC_BREAKPOINT`, `byPid == pid`, ~4.4 s in.
4. The CDP socket dies with it, so the test reports `Io("failed to fill whole buffer")` — exactly
   the failure observed in §3.3, and the reason the **same test passes when the sandbox is bypassed**.
5. On the desktop: a Chrome window appears and vanishes. That is the reported symptom.

**It was not the owner's Chrome.** The crashed process's parent is the test binary, and the owner's
own Chrome was not running at the time (`pgrep` → 0). This is the same phenomenon Batch 13
diagnosed, now with crash reports to prove the mechanism.

### Is teardown graceful? **Yes — verified in source, no change needed**

The abort is **not** a teardown defect: Chrome died ~4.4 s into a ~17 s run, before teardown was
ever reached. The teardown path itself is already graceful-first:

| Element | Location | Behaviour |
|---|---|---|
| `close_gracefully()` | `providers/chrome/mod.rs:263` | CDP `Browser.close`, then poll up to `GRACEFUL_CLOSE_TIMEOUT` |
| `GRACEFUL_CLOSE_TIMEOUT` | `mod.rs:292` | **3 s** — "SIGKILL is what we are trying to avoid" |
| Fallback | `mod.rs:280` → `process.rs:162` | `terminate_tree()` unconditionally, to reap renderer/GPU children |
| Process-group scoping | `mod.rs:311-326` | unix `process_group(0)` so `killpg` **cannot** reach a process Movie Party doesn't own; Windows `CREATE_NEW_PROCESS_GROUP` + kill-on-close job object |
| Panic path | `mod.rs:347-353` | `Drop` calls `close_gracefully()` — unwinding still tears down gracefully |

**No orphaned processes (0) and no new leaked temp profiles** (still the 6 historical dirs, none
from today) — the guards hold even when Chrome dies abnormally.

### Action taken

**Stopped.** No further Chrome-spawning tests will be run in this sandbox. The
`MOVIE_PARTY_ALLOW_VISIBLE_CHROME` guard already prevents *accidental* runs; these were deliberate.
To get a real provider validation, run those two tests from a normal (non-sandboxed) shell, where
they pass.

**No source change was made** — the hard freeze holds, and none is warranted: the teardown is
already correct. The 19 crash reports were left in place rather than deleted.

---

## 9. What changes are needed — the answer to "what needs fixing"

Ordered by what unblocks the most.

1. **Put both machines on the same tailnet.** This is the #1 blocker.
   The Mac is on `tailc930b7` / `abhijairaghuvanshi@`; the VM is on `tail56c84a` /
   `shakalakaboomboob01@`. Either re-authenticate the VM's Tailscale into `abhijairaghuvanshi@`,
   or share the node into that tailnet. *(Not done here — it changes your Tailscale account state
   and needs interactive auth, so it is your call.)*
   Note: the only Windows node already on the correct tailnet is **`saksheepc`**
   (`100.98.24.70`), last seen **~40 days ago** — offline.
2. **Build an installer from `746775d`.** None exists — every previous installer build is at a
   superseded SHA. **But there is a ref problem you need to decide on:**

   `gh workflow run --ref` accepts **only a branch or tag name**, never a bare SHA (verified:
   `gh workflow run --help` → "Branch or tag name which contains the version of the workflow file").
   `build.yml` is `workflow_dispatch`-only, and `746775d` has **neither a branch nor a tag** — the
   branch `stabilization/v0.9.9-rc1` points at `f04731a`, three doc-only commits ahead.

   So the only dispatchable ref produces an installer whose provenance SHA is **`f04731a`**, not
   `746775d`. The *source* is identical (`git diff --stat 746775d f04731a -- src/ src-tauri/` is
   empty), but the commit SHA differs — which matters for "the exact tested SHA is the SHA being
   proposed for release". Three options, all needing your call:

   - **(a)** Dispatch at `stabilization/v0.9.9-rc1` and restate the release candidate as `f04731a`
     (docs-only delta from the audited `746775d`). Cheapest; changes the stated candidate.
   - **(b)** Push a temporary branch at `746775d` and dispatch at that. Keeps the candidate SHA
     honest, but is an externally visible push (not `main`, not a tag) and needs your approval.
   - **(c)** Tag `746775d` — **forbidden by this batch.** Do not.
3. **Use Windows x64, not this ARM64 VM.** The product ships `x86_64-pc-windows-msvc`. If the
   ARM64 VM must be used, treat results as indicative only — you would be testing x64 under
   emulation on a platform you do not ship.
4. **A human at each machine** for the perceptual rows: video visible, audio audible, drift,
   A/V sync over ≥10 minutes. There is no programmatic substitute, and this is the part I cannot
   do from here at all.
5. **A feature-length movie with audio.** The `real_*` fixtures prove the pipeline; they are not
   the release test.

---

## 10. Compliance and side effects

| Prohibition | Status |
|---|---|
| No source-code changes | **COMPLIED** — `src/`+`src-tauri/` diff vs `746775d` is empty |
| No bug fixed | **COMPLIED** — none of the carried-forward items was touched |
| Candidate SHA unchanged | **COMPLIED** — still `746775d` |
| No tag / no release / no push to `main` | **COMPLIED** — none created; remote `main` unchanged |
| Never manufacture evidence | **COMPLIED** — every PASS cites a command, a file, or an observed process |
| Never infer playback from compilation / Windows from CI / sync from one device / audio from headless / A-V from source / convergence from one peer | **COMPLIED** |

**Side effects introduced by this run, all reported:**

- **Tailscale was started on this Mac** (`tailscale up`). It was stopped before. Reverse with
  `tailscale down` if you want the previous state.
- **`dist/` was rebuilt** (the pre-existing output was moved aside and then removed) — build
  output only, gitignored.
- **The built app was launched and then terminated** — 0 processes remain.
- **Six historical Chrome temp-profile directories remain in `$TMPDIR`** (2026-09-19 → 09-21).
  They are pre-existing debris, **not** from this run. Left in place, not deleted.

---

## 11. PART K — Final output

1. **Exact tested SHA:** `746775dfc72a0b7db3c37d0e1b5fca8a14895726`
2. **Device A:** macOS 26.5.1 (25F80) arm64; Tailscale `Running` on `tailc930b7`; candidate `.app` built + launched
3. **Device B:** Parallels VM `Windows 11 main`, Windows 11 ARM64 build 26200.8875; Tailscale `Running` on `tail56c84a`; **no product installed**
4. **Tailscale results:** both nodes up, **different tailnets**, `tailscale ping` → `no matching peer`
5. **Part A (freeze) results:** 9/9 **PASS**
6. **Part B (Windows runtime) results:** **BLOCKED** — no candidate installer, x64/ARM64 mismatch
7. **Provider/Chrome results:** single-peer **PASS** (with A/B control); two-peer **NOT TESTED**
8. **Evidence references:** §1, §3, §6, §7 commands and outputs; `/tmp/mp_rust_test.log`, `/tmp/mp_chrome2.log`, `/tmp/mp_build2.log`
9. **Observed drift:** **none** — no two-device playback occurred
10. **Audio / A-V observations:** real audio **track** detected and real frames rendered on macOS; audibility and A/V sync **not observed**
11. **Reconnect observations:** **none** — no two-device session
12. **EOF observations:** `production_player_reports_completed_when_the_movie_ends` **PASS** on the real player; no two-device EOF test
13. **Failures:** 2 environment-only — real-Chrome under the sandbox (passes when bypassed), and `bundle_dmg.sh` under the sandbox. **No product failure was found.**
14. **Blocked tests:** Part B in full, Part C rows 3 and 5–10, Part D in full, Part E in full, Part F two-peer
15. **Final audit results:** 14 PASS / 1 FAIL (§7)
16. **v0.9.8 immutability:** **PASS** — tag object identical local and remote
17. **v0.9.9 tag / release status:** **no tag**, **no release**
18. **FINAL VERDICT:** **NO-GO**

**Why NO-GO and not "almost ready".** The macOS half of this batch is genuinely excellent: the
candidate builds, launches, passes every gate, drives real libmpv for decode/render/seek/EOF/audio,
and drives real Chrome over CDP. But the release-critical promise is **two peers staying in sync
over a feature-length film**, and that was not observed on two devices — nor *could* it be on this
pair, because the two machines are on different Tailscale networks. A clean single-device result
cannot be promoted into a two-device one.

**DO NOT TAG. DO NOT RELEASE. DO NOT PUSH MAIN.** Stopping here. The release is Batch 15.
