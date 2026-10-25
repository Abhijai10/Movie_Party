# Batch 5 — Integrated YouTube player

## Revision and scope

Starting commit: `1fcd636858b29ab77601157e4fe3e61778a26824`.
Branch: `codex/integrated-youtube-player`.
Final commit: the commit containing this report (recorded in the delivery message).
No release, tag, installer, push, or main-branch overwrite was performed. v0.9.8 tag object remains `4fca32ce3707d706ddd9e48a8d38cd07d1b62cc3` (peeled `bb225778434e3f07923e03e85d7d7cc1db79146c`). The pre-existing untracked `FINAL_MAIN_RECONCILIATION_AND_RC_BUILD_REPORT.md` was left untouched.

## Implementation and design

- One persistent official YouTube IFrame API player follows the selected content through Lobby, Ready Check and Cinema. Navigation between those views does not create a second player. A new selection, room departure, or explicit driver handoff disposes the previous iframe. Automatic handoff waits until Create Party validation/Chrome attachment has finished, preventing a race with the preserved browser session.
- A Rust server binds only `127.0.0.1` on an ephemeral port and serves three compiled assets behind an unguessable capability path. It exposes no files, proxy, credentials, or Tauri APIs. The wrapper receives a natural HTTP Referer and the application's own `widget_referrer`; it does not impersonate another site. Main-window CSP permits loopback frames, while remote scripts/connections remain restricted to the wrapper's explicit YouTube origins. Remote pages have no Tauri capability grants.
- The supported IFrame API supplies content identity, position, duration, loaded fraction, ready/state/buffering/ended/errors, play/pause/seek and autoplay restrictions. Cued/browser-ready content cannot vote playback-ready. A real user play interaction is required before readiness. Commands require observed state/position across stable samples; expired/duplicate commands cannot silently start playback.
- Existing provider protocol version 2, content revision, host sequence, state epoch, evidence matching, acknowledgements and host-authoritative operation consensus are reused. The local playback driver is not transmitted as a room authority change. No wire-format or protocol-version change. Local Perfect handshake/engine is unchanged.
- Player events require the owned frame's origin, source window, lease, increasing sequence and authoritative selection context. Late results, stale frames, mismatched IDs/durations and old epochs cannot restore readiness. Player loss/handoff cancels pending starts and sends the existing strict-sync pause signal to the peer. Readiness, buffering, EOF, heartbeat loss and failed recovery pauses remain explicit.
- Search uses the existing authorized managed-Chrome flow because no YouTube Data API search credential is configured. URL paste and provider tab selection remain available. No invented search results. Supported oEmbed metadata supplies the real title/channel/thumbnail when available; failed metadata requests preserve the selected title and disclose unavailable channel details.
- Errors 2, 5, 100, 101, 150, 153, autoplay restriction and startup timeout have actionable messages. Managed-Chrome fallback invokes the real existing launcher/profile ownership path with the selected canonical URL. Switching to the iframe waits for the owned browser to stop. Login profiles are preserved; ordinary Chrome is not targeted. Missing/failed Chrome remains an error, not simulated fallback success.
- Cinema keeps the entire official player rectangle free of app overlays, with social/status content beside it and existing controls below. Player visibility checks include ancestor clipping and social/dialog intersections; obscured/offscreen playback pauses. Native YouTube UI, branding, ads and access restrictions remain provider-controlled. Provider Shared stays OFF.

## Files changed

New — Rust (1,037 lines):

- `src-tauri/src/app_runtime/youtube.rs` (780) — `YoutubeState`, `EmbedCommand`,
  `YoutubeSnapshot`, `EmbedAttachment`, `EmbedEvent`; the `youtube_driver` /
  `youtube_attach` / `youtube_detach` / `youtube_event` runtime methods, the
  Chrome↔iframe handoff with generation guarding, lease replacement, heartbeat
  health check, metadata validation and consensus report completion. 14 tests,
  one `#[ignore]`d real-Chrome handoff test.
- `src-tauri/src/providers/youtube/embed.rs` (142) — loopback capability-path
  asset server plus its CSP/`frame-ancestors` response headers. 1 test covering
  capability path, exact `Host`, path traversal and shutdown.

Modified — Rust:

- `src-tauri/src/lib.rs` — registers the four new commands.
- `src-tauri/src/app_runtime.rs` — declares the module, adds
  `AppSnapshot.youtube`, resets `YoutubeState` on room change/departure and
  emits the snapshot.
- `src-tauri/src/app_runtime/provider_playback.rs` — routes PLAY/PAUSE/SEEK to
  the iframe driver when it owns playback.
- `src-tauri/src/app_runtime/provider_room.rs` — drives the heartbeat health
  check and exposes gesture state.
- `src-tauri/src/app_runtime/provider_chooser.rs` — hands off to the in-app
  driver instead of always opening Chrome.
- `src-tauri/src/providers/youtube/mod.rs` — adds `pub mod embed`.
- `src-tauri/tauri.conf.json` — `frame-src http://127.0.0.1:*` for the local
  player origin; all other remote sources remain restricted.

New — frontend (1,147 lines):

- `src/youtube/bridge.js` (225), `bridge.html` (14), `bridge.css` (15) — the
  official IFrame API wrapper served by the loopback origin.
- `src/youtube/YoutubeRuntime.tsx` (226) — one owned iframe, handoff
  serialisation, monotonic event forwarding, visibility polling, command
  dispatch and disposal.
- `src/youtube/YoutubePanel.tsx` (106), `types.ts` (94), `visibility.ts` (52).
- `src/youtube/bridge.test.ts` (220), `runtime.test.ts` (32),
  `visibility.test.ts` (129) — 23 tests.
- `src/test-ui/youtube.fixture.tsx` (167), `youtube.html` (10).
- `src-tauri/examples/youtube_webview_probe.rs` (45) and
  `src-tauri/examples/youtube-probe/tauri.conf.json` (18) — manual WKWebView
  probe, not part of the automated suite.

Modified — frontend: `src/components/AppShell.tsx` (mounts the runtime),
`src/views/CinemaView.tsx`, `src/views/LobbyView.tsx`,
`src/views/ReadyCheckView.tsx` (player slot, panel and unobstructed layout),
`src/views/ProviderContentChooser.tsx` (driver choice),
`src/backend/appRuntime.ts` (`youtube` on the snapshot),
`src/overlays/ProviderStatusOverlay.tsx`, `src/providers/ProviderRoomStatus.tsx`,
`src/styles.css`, `eslint.config.js` (bridge.js is untyped JS by design).

## Checks and results

Executed and passing, on the toolchain CI pins (rust 1.98.0, `stable` 1.96.1 was
not used):

- `cargo fmt --check` → clean.
- `cargo clippy --all-targets --all-features -- -D warnings` → clean, **zero
  warnings**.
- `cargo test -- --test-threads=1` → **679 passed, 0 failed, 5 ignored.**
  Because `src-tauri/mpv_runtime/libmpv.dylib` is staged on this machine, the
  nine real-media tests that CI can never execute (AUD-14) ran for real and
  passed: `bundled_libmpv_plays_pauses_seeks_real_video`,
  `bundled_libmpv_renders_onto_real_calayer_through_production_player`,
  `bundled_libmpv_sw_render_api_produces_decoded_frames`,
  `bundled_runtime_is_loadable`, `production_player_*` (3),
  `audio_bearing_fixture_yields_an_audio_track`, and the `real_audio_test`,
  `real_eof_detection_test`, `real_native_surface_e2e`,
  `real_playback_smoke_test`, `real_sw_render_test` suites.
- `MOVIE_PARTY_ALLOW_VISIBLE_CHROME=1 cargo test -- --test-threads=1 --ignored`
  → **5 of 5 passed.** This includes
  `real_youtube_provider_sync_uses_chrome_cdp`, which had never been run in any
  previous batch: real managed Chrome navigated the live YouTube page, the
  HTML5 player was detected, `currentTime` was a real finite position, pause
  returned true and the seek actually moved the playhead. It also includes this
  batch's `real_managed_chrome_fallback_hands_back_to_single_iframe_driver`,
  which verifies requirement 10's fallback against a real browser process
  rather than a stub launcher.
- `npx tsc --noEmit` → clean.
- `npx eslint . --max-warnings=0` → clean.
- `npx vitest run` → 32 files, **365 tests passed** (358 before this batch; the
  7 added tests cover the player-obstruction guard, which previously had none).
- `pnpm build` → succeeds.
- Version consistency (the gate `release.yml` blocks publishing on) → all four
  declarations agree at 0.9.9.

**Resolved:** an earlier `cargo test --lib` run without `--test-threads=1`
reported four failures (`launch_timeout_kills_and_reaps_the_whole_tree`,
`shutdown_wait_includes_sessions_held_outside_runtime_state`,
`corrupt_ownership_record_is_recoverable_only_by_confirmed_reset`,
`friends_list_remove_and_unknown_add`). Under the project's serial harness all
four pass, so they were contention between tests that share real Chrome process
and profile-lease state — not defects. They remain timing-sensitive and must
not be run in parallel.

**Not verified by this batch.** Requirement 16 is **not** met: only macOS
WKWebView and macOS managed-Chrome paths have evidence. Windows webview
differences, and two-device Cinema certification, remain outstanding.

## Follow-up — control uniformity

The integrated player's surfaces mixed two button languages: the app-wide
`CinemaButton` pill and plain `<button>` elements that `.provider-chooser` CSS
redrew as rectangular, sentence-case, 10px-radius boxes. Both appeared inside the
same panel, so the YouTube screens read as foreign to Create Party and the local
movie controls.

Every control in the provider and YouTube surfaces now goes through
`CinemaButton` — the lookup-mode toggle, tab preview selection, and the recovery
action in `ProviderContentChooser`, joining the driver toggle in
`YoutubePanel`. Selected state uses the existing `primary`/`ghost` variants rather
than a bespoke fill, so it matches the action button beside it exactly. The three
competing rules (the rectangular box, its flat `aria-pressed` fill, and its
brightness hover) were deleted; focus-visible and disabled rules were kept.
`.youtube-panel` typography, thumbnail radius, the disclosure label and the error
notice were aligned to the values the provider panels already use.

Verified by computed style in the rendered fixture, not by eye: all six controls
on the YouTube surfaces now report identical `border-radius: 9999px`,
`text-transform: uppercase`, `letter-spacing: 2.24px`, `min-height: 56px`, and the
selected one carries the same `linear-gradient(124,85,214 …)` as the pre-existing
"Search in Chrome" action.

`src/views/providerUiContracts.test.ts` locks the invariant in — no raw `<button>`
in any provider or YouTube control surface, and no CSS rule may redraw a
provider-chooser control as a competing rectangle. It was confirmed to fail
against the pre-change sources and pass against these.

Gates after the change: `tsc` 0, `eslint --max-warnings=0` 0, `vitest` **371
passed / 33 files** (365 plus the 6 new contract tests), `pnpm build` succeeds.
No Rust file was touched, so the 679-test result above still stands. The Impeccable
detector reports only the pre-existing `transition-property: width` at
`src/styles.css:2073`. `src/styles.css` remains Prettier-unformatted as it was
before this change (3,778 lines of the file's compact house style, present in the
committed version; `pnpm format` is not a CI gate, and reformatting it would bury
this diff in unrelated churn).


## Native platform evidence and limits

The macOS probe uses the production loopback wrapper inside a real Tauri WKWebView, not an ordinary browser. The official public API demo returned its actual ID and Google for Developers metadata, played after clicking YouTube's own control, sought to ten seconds through the official API, and disposed its iframe. This probe establishes the native wrapper/API path, not a complete two-device Cinema certification.

Windows native probe validation is in progress. Browser screenshots and cross-compilation are not evidence of Windows functional playback.

## UI verification

Isolated mocked-IPC fixtures were inspected at 1024×700 and the supported minimum 900×600 for Cinema, Lobby and Ready Check. They establish layout only. The minimum Lobby's fallback button was reached by the real UI's scrolling/click path and the fixture changed to the in-app-player option; this does not claim Chrome launched from that fixture. Real Chrome ownership/handoff is tested separately.

The required fresh Impeccable reviewer initially requested a fix for generic Chrome guidance shown while the iframe driver was active. That single fix was scored resolved; final disposition **ship at that limited scope**. The documenter confirmed preservation of the incumbent purple cinema system, serif headings and controls. No design-system migration occurred. DESIGN.md and its sidecar remain absent. PRODUCT.md's prior preference against permanent Cinema sidebars is a recorded pre-existing constraint; this YouTube-only sidecar is a scoped exception required to keep the official embedded player unobstructed, not a global redesign. The detector ran once and found only the pre-existing width transition in styles.css.

## Compatibility, unresolved issues and next-batch gate

No new dependencies, cloud service, authentication automation, cookie/password/key extraction, DRM capture or provider account sharing. Provider cookies and login tokens are never sent over the room channel. Independent authorized playback remains required.

Two physical devices with independent provider playback were not exercised. YouTube availability, ads, account requirements, embedding restrictions, network timing and seek precision can prevent synchronized playback; the app fails closed and offers the authorized browser path. Universal or frame-accurate YouTube sync is not certified. Native Windows status and the final next-batch gate are recorded below after validation.

## Primary API/policy references

- [Official IFrame Player API](https://developers.google.com/youtube/iframe_api_reference): events, documented methods, autoplay and embed errors including 153.
- [YouTube required minimum functionality](https://developers.google.com/youtube/terms/required-minimum-functionality): player dimensions, client identity, visibility and overlay requirements.
