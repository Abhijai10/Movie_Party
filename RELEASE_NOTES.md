# Movie Party — Release Notes

## 0.9.5 (Shared TMDB key baked in, compact ready buttons, macOS call permissions)

### Home — posters work out of the box now
- This build ships with the shared TMDB key baked in (injected at build
  time from the repo secret, never committed to the repo). The Home
  hero's trending posters light up on every install — yours and your
  friend's — with nothing to configure.
- Still overridable per device: Settings → General →
  "Trending posters (TMDB)" accepts your own key, which always wins over
  the shared one — so if the shared key ever stops working, paste a
  replacement there, no new build needed.
- The feed now retries once on flaky network failures (some ISPs
  intermittently reset TMDB connections) and still degrades gracefully
  to the built-in wall when both attempts fail.
- Settings shows the real state: "live feed working" vs "key saved but
  TMDB unreachable from this device" vs no key.

### Create Party — compact ready buttons
- The buttons that appear after selecting a movie ("Change" /
  "Create cinema room") had become oversized — a narrow two-column grid
  stretched them into tall wrapped blocks. They now sit on one row at
  their natural size, matching every other button in the app.

### Calls — macOS camera/microphone permission strings
- The macOS bundle now declares NSCameraUsageDescription and
  NSMicrophoneUsageDescription, so macOS can present its camera/mic
  permission prompt properly for the experimental video call. Previously
  the webview could deny getUserMedia outright.

### Bug fix
- A transient vitest flake under a local .env is resolved: the bundled
  token is read lazily and tests stub the environment explicitly.

## 0.9.4 (UX fixes: TMDB hero, self-contained Schedule, Ready Check escape hatch)

### Home — live trending posters (opt-in, offline-first)
- The "Now Showing" hero can now show real movie posters and backdrops
  from TMDB's trending feed — **if** you paste your own free TMDB API
  key in Settings → General → "Trending posters (TMDB)". The key is
  stored only on this device (localStorage); it is never bundled with
  the app and never leaves the app except directly to
  `api.themoviedb.org`.
- No key, no network, no problem: the bundled feature wall stays. The
  same is true whenever TMDB is unreachable — the feed never blocks or
  breaks the Home screen; posters fall back to their gradient cards.
- Results are cached for an hour and re-checked at most every 30
  minutes, so the app stays gentle on rate limits.
- When live TMDB art is shown, the required attribution ("This product
  uses the TMDB API but is not endorsed or certified by TMDB" + link)
  appears with it.

### Create Party — no more cut-off button labels
- Buttons grow to fit their labels: text wraps inside the button
  instead of being clipped. The provider action is now simply
  "Open in <provider>".

### Schedule — fully self-contained
- Schedule no longer depends on picking a movie in Create Party first.
  Pick a date, time, movie (from the active party's media, a file on
  this machine, or your recent picks), and a friend (saved friends,
  the connected participant, or a manual device ID) — all directly on
  the Schedule screen.

### Friends — honest Tailscale onboarding
- A friend who accepted your invite but has not joined your tailnet yet
  now shows a clear two-step explainer: accepting the Movie Party
  invite is step one; joining the private network happens in the
  Tailscale app (Tailscale's own invite) and is a separate, required
  step. A one-click **Open Tailscale** button launches the Tailscale
  app; then Verify runs the real `tailscale ping`.
- The invite link still carries only your name and public key — never
  an auth key or token.

### Preview / Ready Check — the floating tile can't cover the action
- The draggable call/chat tile is now clamped above the bottom control
  dock in the cinema and above the final action buttons in the lobby
  and Ready Check — it can never be dropped on top of Leave / Enter
  Cinema / Ready check.
- **Back to lobby now works.** It was a dead button: clicking it did
  nothing. It now genuinely returns both sides to the lobby —
  readiness votes are retracted, a pending countdown is dropped, and a
  guest on the Ready Check screen follows the host back automatically.
  Back stays disabled only while the start countdown is already
  committing (the one window where retreat would desync the room).

### Behind the scenes
- New command: `back_to_lobby` (host- or guest-initiated retreat;
  readiness + pending PLAY retracted at the coordinator and state
  level, broadcast to the peer).
- `clampCallTilePosition` gained a reserved-bottom parameter so each
  screen can protect its own final action row.
- CSP now allows exactly two extra hosts: `image.tmdb.org` (posters)
  and `api.themoviedb.org` (the trending request) — pinned, no
  wildcards.
- Home "Up next" naming now resolves media picked straight from
  Schedule via the recent-picks list.
- The experimental Shared Movie Experience remains experimental; it is
  not complete and is not claimed to be.

## 0.9.2 (UI overhaul: Friends page, Settings sidebar, movie hero)

### Friends (new page — replaces the Home panel)
- Friends now has its own screen: the Friends chip in the Home header
  opens a dedicated, scrollable Friends page.
- **Invite link + QR**: "Your friend link" shows a `movieparty://friend/`
  link with a QR code. A friend pastes it (or scans it) into their Movie
  Party and you are added by name — no tailnet device tables, no
  addresses. Requires both devices on the same tailnet (Tailscale account
  sharing); the app explains honestly when that isn't true yet.
- **Editable names**: rename any friend to what you actually call them
  (1–40 characters, MP-FRIEND-001); the tailnet plumbing stays under the
  hood.
- Connect / Re-verify still runs a real `tailscale ping` and records the
  path + latency.

### Home
- The page scrolls; the hero is now a rotating "Now Showing" feature
  wall (auto-advances every few seconds, pauses automatically for
  reduced-motion users).
- Buttons vs words: real actions are icon + label chips with borders;
  decorative text stays dim so controls stand out.
- The old inline friends panel is gone (moved to its page).

### Settings
- Two-column layout: a compact sticky sidebar on the left, the selected
  section's content in a card on the right; the whole page scrolls.

### Schedule
- Rebuilt spacing on cards, and the native date/time pickers now render
  dark so the input text is readable.

### Behind the scenes
- New commands: `friend_invite_link`, `rename_friend`; `add_friend`
  accepts a display name. Links are built identically in Rust and the
  frontend (base64url of `{"n":…,"pk":…}`).
- macOS firewall note: the first genuine incoming connection may show
  the standard "accept incoming network connections" prompt — click
  Allow. That is normal macOS behavior for any app.
- Test hardening: the real-Tailscale self-join integration test now
  detects when the local macOS application firewall blocks the
  ephemeral test binary's hairpin UDP and reports a clear skip instead
  of a false failure (rebuilds change the binary's hash-suffixed path,
  so an earlier Allow never carries over to the test binary).

## 0.9.1 (Tailscale gate fix + Friends)

Fixes the blocker where the app showed "Tailscale is installed but it
isn't responding" on every launch-from-Dock/Finder even while Tailscale
was Running — and adds the Friends surface: pick a movie partner from
your tailnet and verify the real connection.

### Fixes
- **Tailscale probe TERM fix (macOS)**: the Tailscale GUI CLI, when spawned
  from a GUI-launched process (no TERM in the environment), prints
  "The Tailscale GUI failed to start" to stdout with **exit code 0**.
  The old probe trusted exit 0 and failed JSON parsing → the whole app
  gated on a false DAEMON_UNAVAILABLE. The probe now sets `TERM=dumb` for
  every CLI spawn and treats the known wrapper banners as a probe failure
  regardless of exit code (ADR-0004).
- Verified end-to-end in a simulated GUI environment
  (`tests/tailscale_probe.rs`): `detect_status` now returns
  `Some("Running")` where the old code path returned the wrapper banner.

### Add Friend (new, Home screen)
- **Friends panel on Home**: lists your Tailscale tailnet peers; "Add
  friend" saves a peer by its stable MagicDNS name.
- **Real connection verification**: adding (or "Connect" on a saved friend)
  runs `tailscale ping` — an actual packet through the tunnel — and
  records path + latency ("Connection verified · direct · 23 ms"). Honest
  states only: "Online — not verified yet" and "Offline" never claim a
  connection that wasn't probed.
- **Schedule integration**: the Schedule guest picker now offers saved
  friends by name, targeting their device across IP changes.
- New stable error codes: MP-NET-TS-007 (no answer through the tunnel),
  MP-NET-TS-008 (device not in your tailnet). Schema v4 adds the
  `friends` table (auto-migrates in place).

## 0.9.0 (V1 code-complete baseline)

The private two-person desktop cinema: synchronized local-media playback,
provider sync mode (YouTube/Netflix/Prime/JioHotstar), voice/video call
alongside the movie, strict synchronization, and scheduled parties.

### Highlights
- Strict-sync synchronized cinema with backend-driven 3-2-1 start countdown
- Local Perfect media transfer with verified chunks + sparse cache
- Provider Sync Mode with empirical per-provider classification
- Cross-device call with adaptive camera ladder (movie-first bandwidth)
- Ephemeral lower-third chat + centered history; reactions
- Scheduled parties with preload math + reminders
- Settings (8 sections), First Run checks, MP-code error screens
- Resilience: crash watchers, §40 disconnect decision, sleep/wake +
  network revalidation, moved-file recovery
- Ghost Mode & Privacy Mode
- Provider Shared Mode: experimental diagnostic tier — capture
  verification + explicit Sync fallback; full transport pending the
  real-DRM spike

### Known limitations
- Provider Shared Mode is diagnostic-only in 0.9.0
- macOS ScreenCaptureKit diagnostic requires ffmpeg (`brew install ffmpeg`)
- Windows capture spike requires a physical Windows machine

The invite flow is complete: the lobby copies the full movieparty://
link (browser-openable — the OS hands it to Movie Party's registered
deep link), a short human code, or a scannable QR (§16); guests get an
explicit schedule accept/decline (§56) and the post-party keep/remove/
save-as retention question (§52).

### Verification status
All automated gates green (Rust fmt/clippy/363+ tests; frontend
lint/tsc/137 vitest/build). The §31.1 manual matrix (two devices,
real providers, real DRM content) is pending physical hardware
verification.
