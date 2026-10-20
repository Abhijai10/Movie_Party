# Batch 2: provider content chooser and browser reliability

Date: 2026-10-21. Branch: `codex/provider-content-chooser`.
Starting SHA: `a81b8eefe5e2b424401dea190460cb374755c2f5`.
Final implementation SHA: the commit containing this report, resolvable with `git log -1 --format=%H -- PROVIDER_CHOOSER_BATCH2_REPORT.md`; its full SHA is included in the delivery message. A commit cannot embed its own hash.

## Delivered behavior and decisions

The host can choose a provider, search its official site in managed Chrome or paste a supported title/video URL, choose a specific observed provider tab, preview its public title, confirm it, and create a party only after the selected video is loaded. Search results remain in the provider's authorized browser UI; Movie Party supplies the selection list and preview, not a scraped or simulated catalogue. Subscription authentication remains entirely manual on the provider's page.

A host-local chooser stores canonical provider/content IDs, allowed URL, optional playlist ID, public title, thumbnail URL, target/document identity, focus/visibility, confirmation and local readiness. No cookies, passwords, keys or account storage are read or copied. The existing persistent managed profiles and ownership/recovery mechanisms remain intact. Login persistence was implemented in Batch 1, not reimplemented here.

YouTube watch, short-link, shorts, embed and live routes normalize to a canonical watch identity. Supported playlist URLs open browsing; a concrete video must be selected before party creation. Exact provider domains, HTTPS, constrained IDs and bounded input are required. Home/search pages cannot become selected content; credentials in URLs, unrelated domains, unsupported schemes, ambiguous IDs and custom ports are rejected. Subscription selection accepts conservative Netflix, Prime Video and JioHotstar title/playback routes. Unsupported regional URL variants fail closed.

Only public DOM metadata and media state are observed. YouTube thumbnails use the selected video ID on `https://i.ytimg.com`; that exact host is the only added CSP image permission. A loaded, visible, unambiguous video is required for content readiness. A browser or homepage being READY is insufficient.

Tab selection uses observed focus, then unique visibility; ambiguous windows require explicit selection. Confirmation pins the target and canonical URL, so subsequent sync commands cannot silently switch to another first-listed tab. Changed URL/title/document, missing metadata, login requirement or closed tab invalidate selection; buffering retracts readiness. Browser crashes disable creation with reopen/reselect guidance. More than twelve provider tabs and slow observation report bounded recovery errors.

CDP JavaScript exceptions and boolean false are failures. Play awaits its promise; play/pause/seek/rate commands check resulting media state. Navigation rejects download/error/missing-frame acknowledgements and additionally observes the destination URL. Browser activation checks actual visibility. Shutdown uses the browser endpoint independently of tab ambiguity.

Create Party revalidates the confirmation before room initialization and attachment. The backend also rejects a valid key paired with a different requested URL. Frontend operations serialize observation, confirmation and creation; background polling stops throughout room creation, preserving the existing keep-browser-session crash fix and Tokio runtime fixes. No global runtime lock is held during browser activation or other blocking browser operations.

## Changed files

- `src-tauri/src/providers/content.rs`: URL/identity validation, chooser state, selection keys and transition tests.
- `src-tauri/src/providers/chrome/content.rs`: provider-tab discovery, public metadata/readiness observation, target pinning and focus selection.
- `src-tauri/src/providers/chrome/mod.rs`: CDP semantic checks, browser-endpoint shutdown and real-browser regressions.
- `src-tauri/src/providers/generic.rs`: verified media action results and unambiguous visible media targeting.
- `src-tauri/src/providers/sync.rs`: action-result verification and tests.
- `src-tauri/src/providers/youtube/mod.rs`: reuse the canonical URL parser.
- `src-tauri/src/providers/mod.rs`: expose content module.
- `src-tauri/src/app_runtime/provider_chooser.rs`: host-only chooser lifecycle, generation guards, authoritative confirmation and failure tests.
- `src-tauri/src/app_runtime.rs`: chooser state, browser/content readiness distinction, strict-pause action errors.
- `src-tauri/src/lib.rs`: four local chooser IPC commands and confirmed content attachment guard.
- `src-tauri/tauri.conf.json`: exact YouTube thumbnail host in image CSP.
- `src/providers/contentChooser.ts`, `src/providers/contentChooser.test.ts`: typed local IPC, creation gate and operation queue tests.
- `src/backend/appRuntime.ts`, `src/backend/providerLifecycle.test.ts`: pass selection key and cover attachment arguments.
- `src/providers/providerSelection.ts`: honest browser-ready wording.
- `src/views/ProviderContentChooser.tsx`, `src/views/CreatePartyView.tsx`: chooser and existing wizard integration.
- `src/components/AppShell.tsx`: preflight and confirmed attachment during existing room creation.
- `src/styles.css`: scoped accessible chooser and minimum-desktop layout.
- `src/test-ui/provider-content.fixture.tsx`, `tests/ui/provider-content.html`, `tests/ui/provider-content-small.html`: explicitly mocked, test-only interaction/viewport harness; absent from production entry points.
- This report.

## Checks and evidence

Commands were invoked through RTK. Rust commands ran from `src-tauri`; frontend commands from repository root.

| Check | Final result |
| --- | --- |
| `pnpm lint` | Passed, zero warnings |
| `pnpm test` | 336 passed across 28 files |
| `pnpm build` | TypeScript and Vite passed; existing large-chunk warning remains |
| `cargo check --all-targets --all-features` | Passed on macOS |
| `cargo clippy --all-targets --all-features -- -D warnings` | Passed on final source |
| `cargo fmt --check` | Passed |
| `cargo test --locked -- --test-threads=1` | 629 passed, zero failed, four explicitly ignored on final source |
| Visible Chrome tab/window/closure test | Passed on real macOS Chrome |
| Visible Chrome profile storage close/relaunch test | Passed on real macOS Chrome |
| Visible Chrome launch/evaluate/public metadata test | Passed in its final isolated run; observed real local-page title and rejected unloaded video readiness |
| `git diff --check` | Passed |
| UI fixture walkthrough | Explicit second-tab preview/confirm; creation held with zero overlapping observations; closed-tab and crashed-browser gates/recovery; unsafe direct URL feedback |
| UI layout | Checked at 1280×720 and a 900×600 iframe viewport; both columns scroll and Create remains reachable |
| Impeccable finish review / documentation handoff | Ship for reviewed UI scope; no material findings; incumbent design preserved |
| Windows Batch 2 frontend/Rust/live Chrome/native UI | Unavailable: guest command control did not recover after authorized VM restart; no Batch 2 logs or native execution evidence produced |
| Live authenticated subscription providers / two-device playback | Not run; no credentials or authentication automation. Two-device content propagation belongs to Batch 3 |
| Live public YouTube network integration | Existing opt-in test remains ignored; no network-provider playback claim |

The four default-ignored Rust tests are the three explicitly opted-in real-Chrome tests above plus the existing live-YouTube integration. Windows-only tests are platform-gated on macOS, not counted as Windows passes.

New Rust regression coverage includes malicious/real URLs and canonical identity, playlists, subscription routes, active/ambiguous tabs, explicit confirmation, changed content/document/title, closed tabs, buffering, missing metadata/homepages, requested URL substitution, guest and missing/wrong-session guards, strict-pause recovery from JavaScript false, and navigation semantic rejection. The real-browser tab lifecycle test creates disposable tabs/windows and observes closure. Existing Create Party/process ownership/profile persistence regressions were retained in the authoritative suite. Five frontend tests were added for the creation gate, serialized operations/rejection recovery, chooser IPC, error propagation and selection-key attachment.

Intermediate failures are disclosed: restricted loopback permissions prevented an early QUIC regression run, which passed with socket access; an old test expected homepage/browser READY to allow room creation and was updated to assert rejection; one frontend run hit an unrelated existing TMDB timestamp equality flake (one millisecond), with full reruns passing and no TMDB edits; the new close-target test initially checked destruction immediately after acknowledgement and now waits for the observed result. Chrome startup timed out during resource pressure in two later runs; final isolated metadata/startup retry passed. A test helper argument mismatch during the last addition was caught at compilation and corrected before the final full suite. These failures are not counted as passes.

Local raw logs: `/tmp/movie-party-b2-rust-complete.log`, `/tmp/movie-party-b2-frontend-final.log`, `/tmp/movie-party-b2-build-final.log`, `/tmp/movie-party-b2-chrome-final-verified.log` (tab/storage passes plus failed startup), `/tmp/movie-party-b2-chrome-metadata-final.log` (startup/metadata pass). Screenshots are under `/Users/abhijairaghuvanshi/.codex/visualizations/2026/10/20/01a15905-fe27-7682-a083-b0c3f394b38b/provider-chooser-batch2/`. Screenshots use labelled mocked IPC and synthetic titles, and do not establish real provider metadata or Windows correctness. Thumbnail rendering and assistive-technology interaction were not manually exercised. The detector found only an incumbent width-transition warning at `src/styles.css:2073`; no unrelated styling was changed. Legacy PRODUCT context and missing DESIGN context were reported without repairing them.

## Compatibility, remaining risks and next batch

No QUIC/room wire schema, peer protocol, two-person topology, dependency versions or mandatory infrastructure changed. New IPC is host-local; `launch_provider` now requires a confirmed selection key. Existing stale frontend callers receive an actionable rejection. Provider Shared remains OFF. Local movie/libmpv, WebRTC, chat/reactions and privacy implementations were not rewritten; existing authoritative regressions passed. Recent legitimate fixes remain present.

Selection confirmation is local and intentionally not persisted as reusable authority across browser/app restarts. Managed Chrome itself retains provider login state through its existing per-provider persistent profile. Authentication/consent walls and changing provider markup may prevent title/readiness observation; the UI fails closed and asks the user to finish in Chrome. Multiple visible videos are rejected conservatively. No in-app DRM playback or capture was added.

The Windows VM was restarted after guest commands stalled, then remained unresponsive at sign-in; it was left suspended. Only task-owned command probes, transfer server and chooser test tabs were cleaned up. Ordinary Chrome and the preexisting development server were not stopped. Windows Batch 2 verification remains a required validation item before a release; prior Batch 1 Windows evidence is not substituted.

Batch 3 can begin on this branch's validated host-local identity/confirmation contract, with Windows validation and authorized provider smoke checks still tracked. It must propagate canonical content safely, preserve guest Provider Sync mode, and revalidate each device's independently authorized playback. This batch does not establish two-device provider readiness or release readiness.

`v0.9.8` remains unchanged: tag object `4fca32ce3707d706ddd9e48a8d38cd07d1b62cc3`, peeled commit `bb225778434e3f07923e03e85d7d7cc1db79146c`. No tag, release, installer or push was created. The preexisting untracked `FINAL_MAIN_RECONCILIATION_AND_RC_BUILD_REPORT.md` was left untouched.
