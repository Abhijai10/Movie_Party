# Real-provider verification checklist

Provider Sync for Netflix, Prime Video, JioHotstar and YouTube is implemented and
automated-tested, but automated tests drive localhost pages and synthetic tab
fixtures. **Nothing in this file has been proven until the rows below are filled
on real provider pages with real, authorized accounts.** Movie Party never stores
or automates provider credentials: you sign in yourself inside managed Chrome.

Run one provider at a time. Record the observation, not the intention.

## Before you start

1. Google Chrome installed and not already running with the Movie Party profile.
2. Tailscale signed in on both devices, same tailnet.
3. `cargo test -- --test-threads=1 --ignored` with
   `MOVIE_PARTY_ALLOW_VISIBLE_CHROME=1` set — these tests open real, visible
   Chrome windows by design ([providers/chrome/mod.rs](../src-tauri/src/providers/chrome/mod.rs)
   `require_visible_chrome_consent`). Never run them beside an unattended
   Movie Party session.
4. A second device signed in with a **different** account than the host's, where
   possible. Requirement: the two people must not be assumed to share an account,
   catalogue, entitlement or region.

## Evidence tiers

Use exactly these three values. `PARTIAL` never means "verified".

| Tier | Meaning |
|---|---|
| `LIVE-VERIFIED` | Observed on the real provider page on a named OS and date. |
| `AUTOMATED-ONLY` | A test exercises the code path against a fixture or localhost page. |
| `UNVERIFIED` | Implemented, but nothing has ever observed it on the real site. |

## Per-provider routes

These are the only URL shapes `parse_content` accepts. Anything else must be
rejected with a message naming the recovery.

| Provider | Accepted routes | Episode discriminator | Canonical form |
|---|---|---|---|
| Netflix | `/watch/{digits}`, `/title/{digits}`, `/title/{digits}/watch` | `?ep=` or `?v=` | `https://www.netflix.com/watch/{id}[?ep={e}]` |
| Prime Video | `/detail/{id}`, `/detail/{a}/{id}`, `/gp/video/detail/{id}` (amazon.com requires the `gp` route) | `?eid=` | `https://{host}{path}[?eid={e}]` |
| JioHotstar | any path containing `movies`/`shows`/`tv`/`watch` and an 8–20 digit segment | the digit segment is already episode-level | `https://{host}/{path}` |
| YouTube | `/watch?v=`, `youtu.be/{id}`, `/shorts/`, `/embed/`, `/live/` | 11-character video ID | `https://www.youtube.com/watch?v={id}[&list={p}]` |

## Checklist rows

Fill `Result` with `PASS`, `FAIL`, `BLOCKED` or `NOT AVAILABLE`, plus one line of
what you actually saw. A blank row is an honest answer; a guessed one is not.

### 1. Every subscription provider (Netflix, Prime Video, JioHotstar)

| # | Check | Expected | Result |
|---|---|---|---|
| S1 | `provider_search_url` opens the provider's own search page | Real results page; Movie Party shows no invented catalogue | |
| S2 | Login gate detected on a signed-out profile | `LoginRequired`, prompt to sign in in Chrome, never a blank player | |
| S3 | After you sign in yourself, the tab becomes selectable | Title and content identity extracted from the provider page | |
| S4 | `media_identity` returns the real title, not the site name | No `Netflix`/`Prime Video`/`Hotstar` placeholder titles | |
| S5 | Does the page expose `data-video-id` / `data-content-id`? | Record the literal value — this decides whether `player_content_id` can ever match `content_id` ([selection.rs:49](../src-tauri/src/providers/selection.rs#L49)) | |
| S6 | Show a **movie** end to end | Play, pause, seek apply and are observed back | |
| S7 | Show an **episode** of a series | Host and guest land on the same instalment, not the show page | |
| S8 | DRM: is a real `<video>` element present while protected playback runs? | If no visible `<video>` exists, generic HTML5 control **cannot** work — record it as a structural limit, not a bug to hide | |
| S9 | Programmatic `play()` / `pause()` / `currentTime` on the protected element | The provider's own player obeys; if it refuses, say so | |
| S10 | Multi-device: both peers report matching evidence before consensus | Ready never reached on one peer alone | |
| S11 | Title unavailable on the guest's account or region | Guest sees a meaningful error naming the recovery (choose another title / retry), host is not silently blocked | |
| S12 | Guest closes the tab mid-party | Selection invalidated with a prompt, never a false `PlaybackReady` | |
| S13 | Guest's session expires mid-party | `AuthRequired`, sign-in guidance, no black-frame claim of playback | |
| S14 | Site redesign breaks a selector | Detected as an issue on the tab, not a silent "no tabs" | |
| S15 | Managed Chrome relaunched with the persistent profile | Still signed in; ownership lease recovers | |
| S16 | Window closed from the taskbar, not in-app | Process tree reaped, app reports recovery instead of a stuck state | |

### 2. YouTube Provider Sync (managed Chrome path)

Same rows as above, plus:

| # | Check | Expected | Result |
|---|---|---|---|
| Y1 | `real_youtube_provider_sync_uses_chrome_cdp` | Already `LIVE-VERIFIED` on macOS: player detected, real `currentTime`, pause true, seek moved the playhead. Re-record on Windows | |
| Y2 | Embedding-restricted video | Falls back to managed Chrome, never a blank in-app frame | |

### 3. YouTube in-app IFrame player

| # | Check | Expected | Result |
|---|---|---|---|
| P1 | Cinema view shows the official player, unobstructed | No Movie Party overlay inside the player rectangle | |
| P2 | Autoplay blocked until you press play once | `GESTURE_REQUIRED`, actionable copy | |
| P3 | Errors 2, 5, 100, 101, 150, 153 | Each maps to its own recovery message | |
| P4 | Handoff in-app ↔ managed Chrome | Only one audible player at a time | |
| P5 | Two devices, both in-app driver | Positions agree; disagreement pauses instead of drifting | |
| P6 | WebView2 (Windows) differences vs WKWebView | Requirement 16 — currently unmet | |

## What must never be recorded as a pass

- Provider Shared / screen capture: **disabled by design**. `shared_available` is
  `false` for every provider with the reason
  "Provider Shared is experimental and unavailable until capture is verified on
  this device." If capture is tested, that is a separate experiment with its own
  document.
- Any DRM-key extraction, ad skipping, or login automation — out of scope by
  product rule and by provider terms.
- A `PlaybackReady` derived from Chrome launching, a tab existing, or a page
  finishing loading. Ready requires a real media observation.
- Treating "the fixture rendered" as "the provider worked".

## Report

Write the outcome to `PROVIDER_BATCH6_REPORT.md` in the three-tier vocabulary,
splitting supported / partially supported / unverified operations. Do not promote
a capability declaration until the row that justifies it is filled.
