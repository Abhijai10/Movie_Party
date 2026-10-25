# Batch 6 — Subscription provider sync and shared browsing UX

Baseline commit: `2e7ab2c` (Batch 5 integrated YouTube player, plus the control
uniformity follow-up). No release, tag, push, installer or main-branch overwrite
was performed.

## 1. What inspection found (requirement 1)

The Netflix, Prime Video and JioHotstar adapters are each 88 lines and are **wired
into production**, not dead code: `login_required_command`, `detect_media_command`
and `media_identity_command` dispatch per provider
([providers/sync.rs](src-tauri/src/providers/sync.rs)), and
`detect_provider_session_status` executes them against a real CDP page
([app_runtime.rs:2028](src-tauri/src/app_runtime.rs#L2028)). Play/pause/seek/position/
buffer all delegate to `GenericProviderAdapter`, which targets a **single visible
`<video>`** and refuses to guess when there is more than one.

What is genuinely strong already: content identity and URL canonicalisation
([providers/content.rs](src-tauri/src/providers/content.rs)) rejects
non-HTTPS, credentials, ports, lookalike hosts, path traversal, ambiguous IDs,
homepages and search pages; tab selection reconciles on `selection_key` (a SHA-256
over target + identity + title + document) so a changed URL, reloaded document,
renamed title or closed tab invalidates confirmation; selection propagates over
authenticated QUIC as a versioned descriptor that guests cannot author; consensus
requires both peers to report the same operation and content revision.

What was actually broken or missing is below.

## 2. Episode identity — a real defect fixed

`parse_content` dropped every query parameter. Netflix and Prime Video encode the
instalment in the query (`?ep=`, `?v=`, `?eid=`), so **two different episodes of
the same series produced one identical identity and one identical canonical URL**.
A host who confirmed season 1 episode 2 would guide their guest to episode 1, and
neither side could tell.

Fix: `ContentIdentity` gains `episode_id`, extracted through one validated helper
(≤120 chars, `[A-Za-z0-9:_-]`, exactly one occurrence — empty, duplicated, encoded
slashes and markup are all rejected), carried in the canonical URL and therefore in
`selection_key`. `content_id` deliberately stays the site's own show ID because
`ContentEvidence::valid_for` requires `player_content_id == content_id`
([selection.rs:49](src-tauri/src/providers/selection.rs#L49)); collapsing the two
would have made every episode tab unconfirmable.

Also normalised: Netflix `/title/{id}`, `/watch/{id}` and `/title/{id}/watch` now
collapse to one canonical `/watch/{id}`, so a host on the title page and a guest on
the watch page agree instead of blocking each other.

## 3. Capability declarations now carry evidence (requirements 13, 14)

Previously all four providers declared the same `support_level: "PARTIAL"`,
`sync_available: true` and `verification: "EXTERNAL_VERIFICATION_PENDING"` — the
same words whether or not anyone had ever loaded the site.

Added `ActionEvidence` (`LIVE_VERIFIED` / `AUTOMATED_ONLY` / `UNVERIFIED`) and
`ProviderAction { operation, evidence, note }`, with `provider_actions()` as the
single source of truth and `verification` derived from it rather than written by
hand. The capability payload now carries the per-operation list, and the chooser
renders it under "What is actually verified", with the tier named in words as well
as colour (PRODUCT.md forbids colour-only status).

Three tests keep it honest: operations that have never been observed **must** stay
`UNVERIFIED`; live evidence is pinned to the exact five operations a real macOS run
observed and is asserted empty for every subscription provider; and every operation
must be listed once with a non-templated reason.

Provider Shared remains off everywhere: `shared_available: false` with the reason
"unavailable until capture is verified on this device". Nothing in this batch
changes that, and no capture or DRM path was attempted.

## 4. Host-led browsing companion (requirements 7, 8)

Found by inspection: `ProviderContentChooser` with `inRoom` was rendered **only by
the YouTube panel**. Once a Netflix/Prime/JioHotstar party existed, the host had no
in-room way to recover or change the title at all — and `retry_room_provider`
refuses a host by design (it re-opens the *host's* content in the *guest's*
browser), so the one available button was correctly hidden.

The companion now appears for managed-browser providers in the Lobby: the current
selection is named, local and partner states are shown, the host can reopen their
own managed Chrome when content needs attention, and "Change title" opens the
existing chooser. Guest retry is unchanged and still role-gated to match the
backend.

## 5. Supported, partially supported, unverified

Each provider exposes 14 operations. Nothing here is inferred from a selector
having been written.

**YouTube — Provider Sync (managed Chrome)**

| Tier | Operations |
|---|---|
| Supported (live-verified, macOS) | `managed_chrome_launch`, `provider_page_navigation`, `playback_observation`, `pause_command`, `seek_command` |
| Partially supported (automated only) | `content_identity_extraction`, `episode_identity`, `consensus_playback`, `play_command` |
| Unverified | `login_gate_detection`, `two_device_sync`, `entitlement_or_region_failure`, `session_expiry_recovery`, `site_change_resilience` |

`play_command` sits at automated-only on purpose: the live run asserted pause and
that a seek actually moved the playhead, never a programmatic play.

**Netflix, Prime Video, JioHotstar — identical shape, no exceptions claimed**

| Tier | Operations |
|---|---|
| Supported (live-verified) | **none** |
| Partially supported (automated only) | `managed_chrome_launch`, `content_identity_extraction`, `episode_identity`, `consensus_playback` |
| Unverified | `provider_page_navigation`, `playback_observation`, `play_command`, `pause_command`, `seek_command`, `login_gate_detection`, `two_device_sync`, `entitlement_or_region_failure`, `session_expiry_recovery`, `site_change_resilience` |

For these three providers **no page has ever been loaded**. Their selectors, their
`login_required` probes, and the generic HTML5 control path are all still
hypotheses about someone else's site.

**Provider Shared / screen capture (all providers)**

| Tier | Status |
|---|---|
| Disabled by design | `shared_available: false`; capture code exists but is never offered |
| Unverified | Any claim about protected-stream capture. A Widevine/PlayReady page commonly yields black frames on legitimate OS capture surfaces |

## 6. Honest limits of this batch

- **Requirement 3's full journey and requirement 5's guest guidance are not
  end-to-end verified for any subscription provider.** The guest path does set its
  own state and the launch machinery exists, but no run has observed a real
  provider page load on a second machine.
- **Requirement 6 (never assume a shared account, catalogue, entitlement or
  region) is honoured structurally** — each viewer plays in their own managed
  profile and Provider Sync is independent-per-device by design — but the
  *failure* half is unmet: there is still no signal that distinguishes "not in your
  catalogue" from a generic error. It is declared `UNVERIFIED` rather than
  implemented optimistically, and row S11 of the checklist exists to settle it.
- **Requirement 10** — subscription playback always uses the real Chrome window;
  the in-app IFrame player is gated to `provider == "youtube"`, so nothing can
  render a black webview in place of a DRM stream.
- **Requirement 12** — lifecycle is host/guest-symmetric in code (same launcher,
  profile lease, process-group reaping) and tested on macOS; Windows is unverified
  (Batch 5 requirement 16 remains open).
- **Requirement 16** — no credential, cookie or password is read, stored or
  automated: `safe_url` rejects userinfo URLs, the loopback server serves no
  cookies or proxy, thumbnails come from oEmbed with `credentials: "omit"`, and the
  UI keeps signing-in on the provider's own page.
- The subscription companion controls are verified by types, unit tests and the
  shared components already measured in Batch 5's uniformity pass. The rendered
  fixtures are YouTube-based, so no screenshot proves the Netflix layout — that is
  a real gap, not a pass.

## 7. Checks and results

Rust, on the toolchain CI pins (1.98.0):

- `cargo fmt --check` → clean.
- `cargo clippy --all-targets --all-features -- -D warnings` → clean, zero warnings.
- `cargo test -- --test-threads=1` → **686 passed, 0 failed, 5 ignored** across 19
  suites. Includes the nine real libmpv/media suites, which run here because
  `mpv_runtime` is staged and cannot run in CI.
- `MOVIE_PARTY_ALLOW_VISIBLE_CHROME=1 cargo test -- --test-threads=1 --ignored` →
  **5 of 5 passed**, including the live YouTube CDP run and the managed-Chrome
  handoff.

Frontend:

- `tsc --noEmit` → clean. `eslint --max-warnings=0` → clean.
- `vitest run` → **376 passed / 33 files**.
- `pnpm build` → succeeds.
- Evidence disclosure verified in the rendered fixture: three tiers each named in
  words with distinct colour, summary in the system's control voice
  (`text-transform: uppercase`, `letter-spacing: 1.92px`).

### One live flake, recorded rather than papered over

The first `--ignored` run failed `real_youtube_provider_sync_uses_chrome_cdp` at
the post-seek position read: `get_position must return a numeric currentTime`. Two
immediate re-runs and a final full `--ignored` run all passed, so it is
intermittent, not a regression — and it is not reachable from anything this batch
changed (the YouTube path never touches `parse_content` beyond `is_youtube_url`,
which passed earlier in the same run).

The cause is the honest design of `GenericProviderAdapter`: it refuses to guess and
returns null unless **exactly one** visible `<video>` exists. On a live YouTube page
that condition can briefly fail — a preview or advertisement element appears
alongside the player, or the player is swapped mid-seek. The test then fails loudly
instead of passing vacuously, which is the behaviour AUD-07 demanded.

Deliberately **not** changed: adding a "pick the biggest/longest video" heuristic
would be a guess about YouTube's DOM without evidence, and would quietly weaken
requirement 14. Row S8 of the checklist exists to establish what the site actually
does. The strictness stays until that is observed.

## 8. Requirement disposition

| # | Requirement | State |
|---|---|---|
| 1 | Inspect adapters | Done — §1 |
| 2 | Robust identity + normalisation | Done — episode defect fixed and tested |
| 3 | Host journey end to end | Partially: implemented; no real subscription run |
| 4 | Host selection reflected in-app | Done (provider, title, readiness, peer state, controls) |
| 5 | Guest auto-guided to same title | Implemented; **unverified** on a real second device |
| 6 | Never assume shared account/region | Structural only; failure half unmet (§6) |
| 7 | Meaningful error + change/retry | Guest retry unchanged; host recovery added; region error **not** distinguished |
| 8 | Host-led browsing companion | Added for managed-browser providers (§4) |
| 9 | No pixel-mirroring of personalised pages | Held — canonical content + supported events only |
| 10 | Real Chrome windows, no black webview | Held — in-app player gated to YouTube |
| 11 | Playback verification + recovery | Evidence tiers declared per operation; expiry/tab-closure/site-change all `UNVERIFIED` |
| 12 | Chrome lifecycle host and guest | Symmetric in code; macOS only |
| 13 | DRM/capture detection, Shared disabled | `shared_available: false` everywhere, unchanged |
| 14 | Declarations reflect what was verified | Done — evidence model (§3) |
| 15 | Adapter + integration tests, checklist | Done — Rust and TS tests, `docs/PROVIDER_VERIFICATION_CHECKLIST.md` |
| 16 | No passwords/cookies/login automation | Held (§6) |

No provider operation was promoted to a stronger tier than its evidence. Nothing in
this batch claims a working subscription party.

