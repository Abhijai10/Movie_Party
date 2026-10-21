# Batch 4 — Provider playback consensus and recovery

## Revision and scope

- Starting SHA: `8f42903ecb32df5c004e5a9980921eab8a48ab0a`.
- Branch: `codex/provider-playback-consensus`.
- Final SHA: the commit containing this report, resolved with `git log -1 --format=%H -- PROVIDER_PLAYBACK_BATCH4_REPORT.md`; its exact value is recorded in the completion message. A report cannot contain its own commit hash.
- No tag, release, installer, push, dependency update or main overwrite. v0.9.8 is preserved. The unrelated preexisting untracked reconciliation report is excluded.

## Result and design

Browser running, authentication, selected identity, buffered player readiness and successfully applied playback are separate facts. Provider rooms now reserve a host-owned operation when the existing coordinator broadcasts a canonical commit. The room stays unconfirmed until both players supply matching observed results for that operation and content revision. Local Perfect retains its existing clock, handshake and playback path.

The existing host-clock deadlines drive provider commands. Play, pause and seek target the pinned managed page's unique visible video. Identity is checked before and after execution; the player must actually reach the requested position/state. Play also requires position advancement. Seek waits boundedly for completion and leaves the video paused; an explicitly requested resume enters a fresh ordinary play protocol after both seek results pass. Neither a successful CDP transport response nor JavaScript acceptance alone is playback success.

Position acceptance uses a 1.5-second bound. A play dispatch more than 500 ms late fails instead of claiming a synchronized start. These are safety thresholds, not promises of frame-accurate or universal synchronization. Observed provider position, duration and buffer update the player snapshot. Unexpected pauses or divergence beyond the position bound trigger strict pause against the existing projected host clock; unsupported provider behavior is surfaced rather than silently corrected with speculative precise seeks.

The versioned selection acknowledgement now optionally includes the guest's operation result. The host descriptor exposes the current operation and decision. This reuses authenticated QUIC, its sender/replay gate and the existing two-second repair polling rather than introducing another transport or cloud service. Exact context/operation/action matching, monotonic operation sequences, immutable terminal decisions, duplicate suppression and browser/session generation checks prevent old results from completing newer work.

Preparation without commit and operation verification without completion time out after fifteen seconds plus polling/scheduling delay. A browser temporarily borrowed by another observer is awaited boundedly, rather than reported as closed. Commands execute outside the runtime lock and late results cannot mutate a replaced room or selection.

Buffering, tab/browser loss, authentication expiry, missing identity and lost host contact retract readiness and cancel pending starts. Recovery pause attempts run even when host state requests fail; they verify the pause result and explicitly request a manual pause if Chrome cannot confirm it. Recovery requires fresh readiness and a new host play cycle. A late join/reconnect cannot inherit PLAYING from a previous successful operation without its own current player/result evidence. Existing explicit solo/guest-abandon policy is preserved.

Host-observed EOF ends the room without presenting ordinary buffering. Guest-only EOF retracts readiness for host confirmation. The owned provider browser is paused on the ended-room path as well. New verified content clears old EOF/readiness and pending operations so both players prepare again. Local Perfect EOF semantics are unchanged.

Autoplay rejection produces GESTURE_REQUIRED. The in-app button focuses the managed Chrome tab and explains that the user must click its actual video play button once. Focus success does not fake permission or readiness. Observed manual playback clears the restriction; the strict pause and fresh Ready cycle still apply. No CDP userGesture flag, autoplay bypass, authentication automation, DRM capture or profile credential copying was added.

Provider capabilities are now labelled PARTIAL with external verification pending. The Lobby explains precision limits. Provider Shared remains OFF. Local diagnostic events accept only fixed action/outcome enums and numeric revision; URLs, account data, titles and raw browser errors cannot enter those events.

## Changed files

- `src-tauri/src/providers/playback.rs`: observed-player model, bounded commands, operation consensus/ordering, closed diagnostic schema and model tests.
- `src-tauri/src/app_runtime/provider_playback.rs`: verified command execution, deadlines, host-clock safety, consensus decisions, gesture/recovery/EOF handling and runtime simulations.
- `src-tauri/src/providers/selection.rs`: version 2 negotiation model, optional operations/reports, validation and acknowledgement integration.
- `src-tauri/src/app_runtime.rs`: coordinator hooks, pending-operation guards, cleanup, genuine-readiness gating and observed provider dispatch.
- `src-tauri/src/app_runtime/provider_room.rs`: health observation, content-change invalidation, repair worker and content-state snapshots.
- `src-tauri/src/network/quic.rs`: explicit Batch 3/version 1 provider incompatibility regression.
- `src-tauri/src/providers/chrome/content.rs`: actual public position/duration/buffer/paused/ended/seeking observation and stricter readiness.
- `src-tauri/src/providers/chrome/mod.rs`: real Chrome local-video play/pause/seek/rejection regression; observer visibility for shared implementation.
- `src-tauri/src/providers/content.rs`, `src-tauri/src/providers/mod.rs`: observation plumbing/module export.
- `src-tauri/src/providers/sync.rs`: honest PARTIAL capability labels and matching regression.
- `src-tauri/src/lib.rs`: managed-tab focus IPC registration and blocking-pool execution.
- `src-tauri/tests/provider_selection_quic.rs`: authenticated operation-result integration covering play/pause/seek, wrong/duplicate results and rejection.
- `src/backend/appRuntime.ts`: typed gesture/verification phase and focus IPC.
- `src/providers/ProviderGestureControl.tsx`: manual gesture guidance with busy/error feedback.
- `src/providers/ProviderRoomStatus.tsx`, `src/overlays/ProviderStatusOverlay.tsx`: Lobby/Cinema recovery and honest verification states.
- `src/providers/roomProviderState.test.ts`: focus action and rejection regressions.
- `src/test-ui/provider-room.fixture.tsx`: explicitly mocked gesture/focus failure and isolated overlay states.
- `docs/core_docs/PROTOCOL_SPEC.md`, this report: compatibility and verification contract.

## Checks performed

Commands use the repository RTK wrapper. No Windows execution is inferred from macOS checks.

| Check | Result |
| --- | --- |
| `pnpm test` | 342 passed across 29 files; zero failed |
| `pnpm lint` | Passed, zero warnings |
| `pnpm build` | TypeScript and production frontend build passed; existing large-chunk advisory remains |
| `cargo test --locked --all-features -- --test-threads=1` | 664 passed, zero failed, four intentionally ignored; isolated serialized final run |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | Passed |
| `cargo fmt --all -- --check` | Passed |
| Prettier check of five changed/new UI and fixture files | Passed |
| `git diff --check` | Passed |
| `MOVIE_PARTY_ALLOW_VISIBLE_CHROME=1 cargo test --locked real_chrome_launches_and_evaluates_cdp -- --ignored --test-threads=1` | One passed; real macOS Chrome, disposable profile and bundled local MP4 |

The real Chrome test verifies post-command player state for play/pause/seek, rejects an out-of-range seek despite command acceptance, and uses a test-only NotAllowedError to exercise the gesture-rejection result. Muting the local test fixture is test setup only. This does not establish real-provider autoplay policy or authorized subscription playback behavior.

Coverage includes false browser READY, wrong identity, matching operation/revision, both-peer success, rejected seek, duplicate play/acks, stale operations and terminal decisions, repeated selections, late join/reconnect, preparation/command timeout, late dispatch, buffering, EOF, gesture recovery and observed host-clock divergence. Real authenticated loopback QUIC carries operation results; runtime player observations in the integration simulations are explicitly synthetic test fixtures, not simulated production behavior.

The default Rust run intentionally ignores four visible/live Chrome tests. The CDP launch test above was run separately with the user's existing opt-in. Active-tab, profile-persistence and live YouTube tests were not rerun for this batch. No provider authentication was automated.

Intermediate verification exposed one existing Local Perfect reconnect timeout and two existing 800-ms fake-Chrome startup/PID-publication failures during overlapping verification work. The reconnect test passed in isolation. The final authoritative run is isolated from other builds/test processes; its result above is the completion evidence. Existing tests and their timing bounds were not weakened. These timing sensitivities remain worth monitoring. Intermediate Rust fixture ownership/type diagnostics and frontend lint diagnostics were corrected before final checks.

UI validation used one background test tab at 1280×720 and 900×600, with scrolled Lobby controls and isolated actual overlay components. Focus success did not change the fixture to Ready; failure remained visible. The detector returned `[]`. Fresh Impeccable finish reviewer disposition: **ship** for this narrow UI extension; read-only documenter confirmed the incumbent cinema system is preserved. Legacy PRODUCT.md and absent DESIGN.md remain untouched. Screenshots are stored in the task visualization directory under `provider-playback-batch4`. They prove mocked UI rendering, not native Cinema or provider operation.

## Compatibility, limits and next batch

- Provider selection capability/model version is now **2**. Batch 3/version 1 provider peers are explicitly rejected. Local Perfect JSON envelope remains 1.0 and legacy Local Perfect peers remain supported. Existing registry IDs/message meanings are retained; no new mandatory infrastructure or permissions.
- Report outcomes contain no cookies, passwords, tokens, browser storage or raw URLs. Diagnostics exclude arbitrary text instead of attempting to sanitize provider page content after logging it.
- This is observed, best-effort HTML video control. Provider DOM changes, DRM behavior, ads, localization, player APIs, browser policy and scheduling can make operations fail. Subscription identity still uses the conservative public fingerprint from Batch 3, not cryptographic asset attestation.
- Pausing an unreachable/crashed browser cannot be guaranteed remotely; the UI explicitly reports inability to verify and requests manual action. Polling and acknowledgement freshness introduce bounded detection delay, rather than instantaneous network-loss detection.
- Windows desktop checks were not run: Parallels reports `Windows 11 main` suspended. Two real devices with independent authorized provider accounts were not exercised. Frame-accurate cross-device synchronization and universal precise seeks are not certified.
- Local movie playback/transfer, QUIC, calls, chat, reactions, privacy and existing strict-sync tests remain in the authoritative regression suite. No unrelated engine rewrite or dependency update was made.

The next implementation batch can begin from the committed version 2 contract once the authoritative checks above pass. Real Windows and authorized two-device/provider validation remain release gates; this implementation batch does not certify them.
