# Provider baseline audit — Batch 0

Date: 2026-10-20 (IST). Scope: reconcile current source and add regression protection only.

## Repository identity and preserved work

- Starting branch: `main`, tracking `origin/main`; both locally recorded refs were at
  **`50140336096fb841213bf4a35b221797e890bb3d`**. No remote fetch was performed.
- Working branch: `codex/provider-baseline-audit`, created from that HEAD.
- Starting tracked worktree: clean. Pre-existing untracked
  `FINAL_MAIN_RECONCILIATION_AND_RC_BUILD_REPORT.md` was read as historical evidence and
  left untouched/untracked. Its `fe2754f` claim is historical, not the current baseline.
- Previous candidate: `fe2754ffd3cc56389d9ef4edd3699c57c6b18178`.
- `v0.9.8` tag object: `4fca32ce3707d706ddd9e48a8d38cd07d1b62cc3`;
  peeled commit: `bb225778434e3f07923e03e85d7d7cc1db79146c`.
  No tags, release metadata, installers, or remote branches were modified/published.
- This batch's final commit is the commit containing this document and the two test
  changes listed below; its exact SHA is reported in the completion message (a commit
  cannot contain its own SHA).

Six commits landed after the old candidate, in chronological order:

| Commit    | Change preserved                                                                 |
| --------- | -------------------------------------------------------------------------------- |
| `763fa50` | Actually ad-hoc sign the macOS app bundle                                        |
| `e4f676d` | Explain macOS damaged-app and Windows SmartScreen causes                         |
| `162dee0` | Repair the pinned Windows libmpv download asset                                  |
| `245858f` | Preserve provider browser across Create Party, URL fallback, visible errors      |
| `d4878fd` | Spawn provider workers safely from synchronous command threads                   |
| `5014033` | Read current browser URL during status detection instead of retaining launch URL |

`git diff fe2754f..HEAD --stat` at the start showed 18 files, 509 insertions,
86 deletions: three workflow files; README and two release documents; Tauri config;
`app_runtime.rs`, `lib.rs`, `providers/sync.rs`; four Rust integration suites;
frontend backend wrapper, AppShell and CreatePartyView. These recent fixes are all
retained; this batch changes no production behavior or dependencies.

## Actual flow and its failure boundaries

Locations below refer to production lines at the starting SHA; test additions are
appended near the end of `app_runtime.rs`, so production references remain stable.

1. **Choose provider.** `src/views/CreatePartyView.tsx:179` derives readiness only
   when the selected ID matches the snapshot. Provider controls start at line 452.
   `src/providers/providerSelection.ts` supplies provider homes/search URLs and the
   readiness actions. Sync is the supported path; Shared remains disabled.
2. **Open Chrome.** AppShell's `openProvider` (`src/components/AppShell.tsx:683`)
   calls `open_provider_browser` (`src-tauri/src/lib.rs:744`). A matching live session
   is reused; otherwise `launch_provider_chrome` at line 721 closes the previous
   session, allocates a localhost CDP port, and starts Chrome with a provider-specific
   user-data directory. `src-tauri/src/providers/chrome/mod.rs:59,130,202` owns the
   launch arguments, profile path validation and launch failure cleanup. CDP binds
   to `127.0.0.1`, not a tailnet/public listener.
3. **Sign in.** The user signs in manually on the provider's own Chrome page.
   `AppRuntime::open_provider_browser` (`app_runtime.rs:1882`) records LoginRequired
   without changing the screen or creating a room. Status checks call
   `detect_provider_session_status` (`:1954`), using login/media adapter detection
   plus `location.href`. No cookie/password extraction is involved.
4. **Choose content.** Ready offers a title input, which navigates Chrome to the
   provider's search page (`lib.rs:789`, `app_runtime.rs:2059`). The user selects the
   result in Chrome. Navigating offers Check playback. Detection maps media presence
   to PlaybackReady (`providers/sync.rs:389`); it does not establish a loaded,
   buffered, authorized playable video. Media presence takes precedence even if
   login detection also reports true. Status reads adopt an allowed current URL.
5. **Create Party.** CreatePartyView sends the snapshot URL (`:240`). AppShell
   (`:594–638`) first creates the QUIC room with `createLocalParty(null, true)`, then
   calls `launchProvider`. `create_local_party` (`app_runtime.rs:2811`) aborts previous
   workers and transfers blocking teardown outside the state lock while preserving
   Chrome when requested (`DeferredTeardown::take`, `:7664`). `launch_provider`
   (`lib.rs:661`) reuses a live same-provider session, validates readiness and attaches
   it; otherwise it resolves a launch URL and launches a new session. Attachment
   (`app_runtime.rs:2039`) restores provider/crash workers and marks local media ready.
   Room creation precedes attachment validation, so an attach failure can leave a
   room created without a usable provider attachment; this is not atomic.
6. **Join.** `join_party` (`app_runtime.rs:3120`) validates the invite before teardown,
   clears previous media/provider/session state, sets LOCAL_PERFECT at line 3202,
   authenticates QUIC and enters Lobby. It starts calibration, heartbeat, buffer
   reporting and a local-media fetch attempt. It does not learn provider identity,
   mode or selection from the host and does not launch the guest's provider browser.
7. **Ready/play.** `set_ready` and the host play gate require PlaybackReady for a
   provider source (existing regression tests at `:9704` and `:11366`). Canonical
   play/pause/seek commits route to CDP in provider mode (`:5713`); the watch worker
   (`:5535`) polls position/buffer only while Playing and not strict-paused. It does
   not poll content identity. This is a host-side implementation, not evidence of
   working two-device provider playback given the join gap.
8. **Cleanup.** `leave_party` aborts workers, resets provider state and defers
   Chrome/player close outside the lock. `close_provider_session` (`:1768`) bumps
   the Chrome generation to reject stale reinsertion. Chrome close/drop
   (`providers/chrome/mod.rs:263,348`) attempts Browser.close then terminates/reaps
   the owned process tree; `chrome/process.rs` implements Unix groups and Windows
   job ownership. Existing generation and process-tree tests must remain intact.

## Exact crash fix and already-fixed defects

**The Create Party SIGABRT fix is `d4878fdcb3b4a367eda265b0d14d4da30b6f771a`.**
The synchronous Tauri `launch_provider` command reaches `attach_launched_provider`,
which starts the provider poller and Chrome crash watcher. Previously both called
bare `tokio::spawn` with no entered Tokio runtime on that command thread. That
panics with “there is no reactor running”; the reported app crash was SIGABRT at
the command boundary. The commit changes both to `spawn_on_runtime`, which tries
the current handle and falls back to Tauri's global runtime (`app_runtime.rs:7719`).
Both corrected call sites are present at starting HEAD. The existing plain Rust
test `attaching_a_provider_from_a_synchronous_context_does_not_panic` directly
exercises the no-entered-runtime condition. This batch extends it with replacement
and teardown coverage on both runtime paths. Unit evidence does not reproduce a
signed desktop application's original OS crash log or real provider login flow.

Two separate defects must not be confused with that crash:

- `245858f` fixed the earlier no-op/URL mismatch by preserving the already-open
  Chrome session during room creation, adding URL fallback, and putting errors
  below the source cards (`CreatePartyView.tsx:323`, `AppShell.tsx:620`,
  `appRuntime.ts:524`, `DeferredTeardown::take`). Existing URL resolver tests already
  cover stale/empty input, mismatched providers and YouTube home rejection.
- `5014033` fixes **local current-URL adoption** in `detect_provider_session_status`.
  It reads `location.href` and updates `state.provider.url` only if the provider
  allowlist accepts it. It does not propagate selection to the guest, identify a
  title/duration, or add content-change controls after room creation.

## Requested gap reconciliation

| Suspected gap                                      | Current verdict and exact evidence                                                                                                                                                                                                                                                                                                                                                                                          |
| -------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Only a URL, no meaningful browsing UI              | **Partially improved, still incomplete.** CreatePartyView has a provider wizard/title search and real external Chrome browsing (`:452–620`). Lobby (`src/views/LobbyView.tsx:59–69`) and Cinema (`src/views/CinemaView.tsx:203`) still use the URL as the title fallback. No in-app catalog or content-selection view exists.                                                                                               |
| No host-to-guest content selection                 | **Present.** `network/quic.rs` has no provider selection handlers; `join_party` restores only room/peer/transport data. Protocol enum IDs ProviderState/Error/ModeRequest/ModeResult exist (`protocol/mod.rs:163`) but are not an implemented selection flow.                                                                                                                                                               |
| Guest resetting to LOCAL_PERFECT                   | **Present.** Explicit reset in `join_party:3202`, with no provider restoration after authentication (`:3259–3315`).                                                                                                                                                                                                                                                                                                         |
| Homepage READY accepted before video ready         | **Present in backend.** `validate_provider_ready_for_room:1928` accepts Ready as well as PlaybackReady; existing test `provider_validate_readiness_rejects_login_required:8592` explicitly expects Ready success. The frontend offers Create only for PlaybackReady. Separately, media-element presence is weaker than actual playback readiness.                                                                           |
| Temporary Chrome profiles                          | **Present, with nuance.** `lib.rs:732,828` uses `temp_dir()/MoviePartyProfiles/<provider>`. The same directory can survive launches, and graceful close flushes it; it is not a fresh random profile each time. It is still OS-temp storage with no durable per-user profile lifecycle guarantee.                                                                                                                           |
| First tab instead of active tab                    | **Present.** `ManagedChromeSession::page_target`, `chrome/mod.rs:240`, returns the first eligible page target. No selected-target identity or active-tab verification exists. URL reads and commands may hit the wrong tab.                                                                                                                                                                                                 |
| CDP response success without semantic verification | **Present.** `CdpPageSession::execute:404` checks the top-level CDP error and returns the result. Runtime exceptions, false adapter values and rejected `void media.play()` are not checked by `dispatch_provider_commit:5802`, which accepts any Ok value. Navigation also lacks load/playability verification.                                                                                                            |
| No usable in-app provider playback                 | **Present for in-app rendering.** Provider playback runs in an independent Chrome window. Cinema attaches a native surface only for LOCAL_PERFECT (`CinemaView.tsx:249`). ProviderStatusOverlay requires `snapshot.media != null` (`:10`), so it does not give a provider-only room a useful browser playback surface. External Chrome capability is not proof of a usable integrated player. DRM capture remains disabled. |
| Unused media_identity_command                      | **Present in production.** Defined at `providers/sync.rs:563`, used by its command-construction test only; neither status detection nor the watch worker calls it. Current URL adoption is a separate location.href read.                                                                                                                                                                                                   |

Additional limits of the recent URL fix: Netflix/Prime/Hotstar URL checks are
domain allowlists (`providers/sync.rs:129–166`), not content-path validators, so
home/search/login pages there can still be adopted. YouTube rejects normal home
and search URLs. Failure to read the URL is ignored and keeps the old selection;
leaving content can also leave an old URL. Status detection/navigation still run
blocking CDP while holding the global state lock (`app_runtime.rs:1968,2065`),
unlike the dispatch/watch take-execute-return path. The Create view's status controls
are not exposed in Lobby: the history's “change without recreating” claim describes
a backend capability, not a complete post-create user flow.

## Regression protection and design choices

Changed files in this batch only:

1. `PROVIDER_BASELINE_AUDIT.md` — this reconciliation and dependency plan.
2. `src/backend/providerLifecycle.test.ts` — three command-boundary tests: provider
   creation passes the preservation flag, local creation defaults to teardown, and
   failed provider attach rejects with the actionable MP-PROVIDER-004 code.
3. `src-tauri/src/app_runtime.rs` — test-only additions: worker replacement and leave
   cleanup with and without an entered Tokio runtime; readiness degradation from
   PlaybackReady to Launching, Navigating, Unavailable and Error rejects room creation.
   Checks include task identity, actual termination, state reset and stable errors.

These extend the existing crash regression without changing accepted behavior or
freezing known defects into new success expectations. The frontend tests mock only
the IPC boundary; they are not browser sign-in or UI click tests. The worker tests
do not launch Chrome. No new production abstraction, fixture playback or auth
automation was introduced.

Existing tests to extend in subsequent batches:

- `src/providers/providerSelection.test.ts`: full frontend readiness actions, creation
  gate, search URLs, Shared gating and stale selection labels.
- `providers/sync.rs` tests: readiness detection, launch URL resolution, provider URL
  allowlist, shared mode gate, action/error mapping and identity command generation.
- `app_runtime.rs` provider dispatch/readiness tests: strict pause on provider failure,
  host play gates, session generations and stale Chrome reinsertion guards.
- `providers/chrome/mod.rs` and `chrome/process.rs`: profile validation, loopback CDP,
  launch timeout/exit cleanup and real process-tree ownership.
- `src-tauri/tests/host_guest_wiring.rs`, `m2_integration.rs`, `m3_closure.rs`,
  `m3_integration.rs`, `m3_m4_e2e.rs`: real serialized QUIC lifecycle/sync/transfer.
- Two explicitly ignored real Chrome/YouTube tests in `chrome/mod.rs`: retain them
  as external verification gates; add dedicated target/semantic-result coverage
  before claiming live provider reliability. The recent location.href round-trip
  remains unverified against a live browser in this batch.

## Validation

Host: macOS arm64. Bundled `src-tauri/mpv_runtime/libmpv.dylib` is present.
Authoritative suite setup is `.github/workflows/ci.yml:60–132`: Rust tests serialized
with `--test-threads=1`, and explicit libmpv skips only when the runtime is missing.
No libmpv skip flags were used here.

Commands run (all shell commands prefixed with `rtk`):

- `rtk pnpm test`: **324 passed, 0 failed**, 27 files (starting baseline 321/26).
- `rtk pnpm lint`: passed; repeated after frontend test additions.
- `rtk pnpm build`: TypeScript and Vite passed; repeated after additions. Existing
  warning: main JavaScript chunk exceeds 500 kB after minification.
- `rtk cargo fmt --check`: passed; `rtk cargo fmt` formatted test additions only.
- `rtk cargo clippy --all-targets --all-features -- -D warnings`: passed.
- Initial `rtk cargo test -- --test-threads=1` inside the sandbox: **457 passed,
  42 failed, 2 ignored** in the library binary; socket bind failures reported
  Operation not permitted. Cargo stopped before integration suites. These are
  recorded failures, not reclassified as passes.
- The same serialized command was rerun with local networking allowed: **595
  passed, 0 failed, 2 ignored** across library and integration binaries (library:
  502 passed; integrations: 93 passed). Full output is saved to
  `/tmp/movie-party-b0-rust-test.log`. Real macOS libmpv audio, EOF, native CALayer,
  play/pause/seek and software-render checks all executed and passed. The two
  ignored tests are `real_chrome_launches_and_evaluates_cdp` and
  `real_youtube_provider_sync_uses_chrome_cdp`; Windows native-surface tests are
  excluded by platform configuration and executed zero tests on this host.
- `rtk proxy pnpm exec prettier --check src/backend/providerLifecycle.test.ts
PROVIDER_BASELINE_AUDIT.md`: passed after formatting the new audit. The first
  formatting check flagged the audit; it was corrected with Prettier and rechecked.
- `rtk git diff --check`: passed.

Not executed: real Windows runtime/desktop checks, signed/bundled desktop sign-in
and Create Party reproduction, actual two-device Tailscale/provider verification,
and the explicitly ignored live Chrome/YouTube tests. No Windows functionality is
claimed from macOS or historical CI compilation.

## Dependency ordering and next-batch decision

1. Preserve browser ownership and runtime spawn fixes as prerequisites for all work.
2. Establish reliable selected-tab/content identity and semantic CDP result parsing;
   distinguish browser/login/media presence from actual playback readiness. Resolve
   frontend/backend readiness disagreement and partial room creation failure.
3. Establish durable managed profiles and explicit lifecycle/isolation policy before
   relying on repeated authorized sign-ins. Keep authentication manual.
4. Define provider selection/mode propagation with protocol compatibility rules,
   authenticated host authority, late-join/reconnect behavior and independent guest
   readiness. Then remove the guest's local-mode mismatch for provider rooms.
5. Build browsing/content controls and usable external-browser handoff/presentation
   on the verified identity/readiness model; only then validate canonical play,
   pause, seek, buffer stalls and cleanup on two real devices.
6. Perform real macOS and Windows desktop validation before any release task.

Steps 2 and 3 can be designed independently, but content propagation depends on a
trustworthy selected-content model; integrated playback claims depend on both.
No wire format, protocol version, compatibility policy, release version, network
permissions or cloud infrastructure changes occur in Batch 0. The next scoped
implementation batch **can safely begin**: the baseline is identified and the
available checks pass after resolving sandbox socket restrictions. The current
provider product is not ready for a two-device reliability/release claim. All
confirmed feature gaps above remain unresolved deliberately in this audit batch.
