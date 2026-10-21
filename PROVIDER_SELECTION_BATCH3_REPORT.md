# Batch 3 — Authoritative provider selection over QUIC

## Revision and scope

- Starting commit: `373ec930c81e892edf23de9d917e919def0557f7`.
- Feature branch: `codex/provider-selection-quic`.
- Final revision: the commit containing this report; resolve with `git log -1 --format=%H -- PROVIDER_SELECTION_BATCH3_REPORT.md`. The completion message records its exact SHA. A report cannot embed the hash of its own containing commit.
- No release, installer, tag, push, dependency update, or main overwrite. Existing Batch 1/2 fixes are retained. The preexisting untracked `FINAL_MAIN_RECONCILIATION_AND_RC_BUILD_REPORT.md` is excluded.
- Preserved v0.9.8 tag object `4fca32ce3707d706ddd9e48a8d38cd07d1b62cc3`, peeled commit `bb225778434e3f07923e03e85d7d7cc1db79146c`.

## Implemented behavior and choices

The host owns a versioned canonical provider selection: provider, content ID, allowed canonical URL, public title/player evidence, room ID, UUID epoch, content revision and host sequence. It publishes the initial selection before accepting joins and publishes subsequent verified changes. An authenticated state pull repairs missed broadcasts. Separate content counters avoid depending on ordering between QUIC streams.

Joining applies the descriptor before initializing the guest source. A Provider Sync guest stays in Provider Sync and opens the host's selected URL using its own persistent managed Chrome profile. No provider account data is read or transmitted. Existing managed process ownership, profile isolation, manual login and Create Party browser-retention fixes remain in use.

Guest states distinguish UNSELECTED, OPENING, CONTENT_CONFIRMED, PLAYBACK_READY, ERROR and AUTH_REQUIRED. An open homepage, matching address, or successful CDP transport response cannot establish playback readiness. Actual loaded video evidence must match; YouTube requires the public player content ID. Confirmation precedes readiness. Invalid metadata, closed browser/tab, authentication, launch failure, loading timeout and lost host contact retract readiness and offer manual recovery. Launch failures require explicit retry rather than repeatedly opening Chrome windows.

Acknowledgements carry exact selection context and monotonic acknowledgement sequence. Repeating the same acknowledgement on a fresh authenticated request is safe. Provider RPCs reject retired transport connections, wrong senders and replays. Host selection writes have no guest request counterpart. Stale/conflicting descriptors cannot replace accepted content. New selection epochs cannot silently replace an active room epoch.

The worker checks every two seconds, uses five-second request timeouts, expires host-side acknowledgement freshness after twelve seconds, and reports unverified content after 45 seconds. Actual clock/scheduler delays can extend observation intervals. Browser work happens outside the runtime lock, fenced by room, selection and browser generation. Late results cannot attach an obsolete launch or observation to newer content.

Playback and coordinator state updates carry selection context. A delayed command from the real authenticated host for the previous revision cannot prepare playback of newer content. Selection changes and loss of verified readiness cancel pending playback operations; strict pauses verify the CDP action's semantic result and surface failures. Host-only/default and explicitly shared controls remain unchanged. Existing explicit continue-without-guest policy remains available. Local Perfect rooms require no provider acknowledgement.

The Lobby shows the selected title and each peer's loading state, with guest recovery and manual authentication guidance. Scoped layout changes preserve the incumbent cinema UI and make lower controls reachable at the minimum viewport. Mocked IPC fixtures are isolated from the production entry point.

## Files changed

- `src-tauri/src/providers/selection.rs`: versioned authority, guest state machine, evidence validation and model tests.
- `src-tauri/src/network/quic.rs`: optional negotiation, authenticated selection snapshots/acks, readiness gate, context-bound playback and handshake tests.
- `src-tauri/src/protocol/mod.rs`: reserved registry IDs 224–227 and registry tests.
- `src-tauri/src/app_runtime/provider_room.rs`: room lifecycle, safe browser launch/observation/recovery, consensus gating and runtime regressions.
- `src-tauri/src/app_runtime.rs`: create/join/reconnect/cleanup integration, snapshots and playback context checks.
- `src-tauri/src/app_runtime/provider_chooser.rs`: actual evidence preflight and host selection publication.
- `src-tauri/src/lib.rs`: production managed-browser launcher injection and guest recovery command.
- `src-tauri/src/providers/chrome/content.rs`: public active-video duration/content ID observation.
- `src-tauri/src/providers/chrome/mod.rs`: real Chrome bundled-video and changed-player-ID regression.
- `src-tauri/src/providers/content.rs`, `src-tauri/src/providers/mod.rs`: evidence plumbing/module export.
- `src-tauri/src/sync/local.rs`: selection context for coordinator updates.
- `src-tauri/tests/provider_selection_quic.rs`: three authenticated two-peer QUIC integration tests.
- `src/backend/appRuntime.ts`: typed content-state snapshot and retry IPC.
- `src/providers/ProviderRoomStatus.tsx`, `roomProviderState.ts`, `roomProviderState.test.ts`: state presentation, recovery and frontend regressions.
- `src/views/LobbyView.tsx`, `src/styles.css`: selected title, correct guest identity label and scoped provider layout.
- `src/test-ui/provider-room.fixture.tsx`, `tests/ui/provider-room.html`, `tests/ui/provider-room-small.html`: explicitly mocked UI verification fixtures.
- `docs/core_docs/PROTOCOL_SPEC.md`, this report: compatibility contract and evidence.

## Exact checks and evidence

All shell commands used the repository's RTK wrapper. Final checks:

| Check | Result |
| --- | --- |
| `pnpm test` | 340 passed, 29 files, zero failed |
| `pnpm lint` | Passed, zero warnings |
| `pnpm build` | TypeScript and production Vite build passed; existing large-chunk advisory remains |
| `cargo test --locked --all-features -- --test-threads=1` | 650 passed, zero failed, four explicitly ignored; run with local networking/process inspection available |
| `MOVIE_PARTY_ALLOW_VISIBLE_CHROME=1 cargo test --locked real_chrome_launches_and_evaluates_cdp -- --ignored --test-threads=1` | One passed; disposable real Chrome profile, bundled local MP4, public player ID changes detected |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | Passed |
| `cargo check --all-targets --all-features` | Passed during implementation; final all-target/all-feature Clippy also compiled final targets |
| `cargo fmt --all -- --check` | Passed |
| Prettier check of six new frontend/fixture files | Passed |
| `git diff --check` | Passed |

Regression coverage includes X received/confirmed/ready over actual authenticated loopback QUIC; stale Y, duplicate X, wrong evidence, guest forgery, lost acknowledgement, expiry, late join, reconnect, retired connection, incompatible capabilities, homepage/auth/loading failure, missing launcher, buffer loss, pending-operation cancellation, Local Perfect preservation and explicit guest-abandon policy. Runtime create/join tests use simulated player observations over real QUIC; they are not two physical desktop/provider tests.

The default suite's four ignored tests are visible Chrome active-tab handling, real Chrome CDP launch, profile persistence, and live YouTube synchronization. The CDP launch test was run separately with the user's existing opt-in. The other three were not rerun for this batch. No provider account authentication was automated.

One final-suite attempt inside the restricted sandbox could not access required loopback/process facilities and was stopped. The complete rerun with those facilities passed. Intermediate compile, fixture type and lint diagnostics were corrected before final verification; no unresolved failing test remains.

UI verification used one background browser tab and isolated mocked IPC at 1280×720 and 900×600, including scrolled lower controls and retry success/failure states. Fresh Impeccable reviewer disposition: **ship**, scoped to this UI extension. Read-only documenter confirmed incumbent identity preservation. The one detector finding concerns a preexisting width transition outside the diff. Legacy PRODUCT.md and absent DESIGN.md remain untouched. Captures are stored outside the repository in the task visualization directory under `provider-selection-batch3`.

## Compatibility and remaining risks

- JSON envelope remains 1.0. Optional `provider_selection_version: 1` and `AuthAccept.provider_room` negotiate this extension. Local Perfect legacy shapes remain accepted; provider rooms reject unsupported clients explicitly. IDs 224–227 are new, with no reassignment. Full details are in PROTOCOL_SPEC.md §68.
- Subscription evidence uses public title and duration plus public player ID where available. This is conservative metadata matching, **not cryptographic DRM asset verification**. Provider DOM changes, advertisements, localized titles and different editions can block or weaken matching. Real authorized subscription-provider/account validation remains outstanding.
- Live/infinite-duration content cannot become ready under the finite-duration evidence contract. Matching metadata does not prove frame-perfect provider output; the existing provider synchronization/control limitations remain.
- Host navigation of its pinned selected tab publishes newly verified content. An unrelated new tab is not silently adopted; it requires explicit content confirmation through the existing chooser API. No broad in-room chooser redesign was added.
- Windows native execution was unavailable: Parallels reports `Windows 11 main` as suspended, so no Windows execution was performed. macOS tests, mocked browser captures and CI compilation are not Windows functional evidence. Two real devices with independently authorized provider playback remain a release validation requirement.
- Provider Shared/DRM capture stays OFF. QUIC, Local Perfect transfer/playback, calling, chat, reactions, privacy and strict-sync regressions passed the available serialized suite. No mandatory cloud infrastructure or network-permission expansion was introduced.

The next implementation batch can begin from this committed contract. Release readiness still requires real two-device/provider and Windows desktop execution; this batch does not certify those unavailable checks.
