# Movie Party — Release Notes

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
