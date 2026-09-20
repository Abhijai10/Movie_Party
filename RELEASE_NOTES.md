# Movie Party — Release Notes

## 0.9.9 (stabilization)

The stabilization pass between v0.9.8 and the first beta. Everything here is a
correctness, integrity or honesty fix — there are no new features, and no
experimental work. The Shared Movie Experience remains gated off exactly as
before: it is not implemented and cannot be enabled.

### Watching together actually stays synchronized
- **Guest playback is no longer repeatedly seeked backwards.** The drift
  projection could be arbitrarily wrong because it ran against a clock that had
  never been calibrated, and nothing bounded the result. Correction is now gated
  on calibration and clamped to the media duration, so a projected position can
  no longer land outside the movie.
- **A simultaneous readiness hand-off no longer drops a message.** The two
  participants could report ready at the same instant; each report travels on its
  own connection and they can arrive out of order, so the later one was rejected
  as a sequence error, ignored, and never retried — leaving the room waiting
  forever. A bounded reorder window now accepts a legitimate reorder while still
  rejecting duplicates and anything stale.
- **Stale playback state no longer survives leaving a party.** A previously
  committed playback was never cleared, so it could be reused after returning to
  the lobby.

### The end of a movie
- **The end of a movie is now reported, and shown.** A finished film previously
  left the room in a state indistinguishable from a pause: the app still claimed
  to be *Syncing*, the control dock still offered a play button, and pressing it
  tried to start the movie again from the end. The film's final frame now stays
  on screen with a short *Movie Finished* card and a single **Back to Lobby**
  action, and playback controls are disabled while the movie is over.
- **No more "Paused to keep you together" as the credits roll.** That notice —
  meant for a peer whose buffer had run dry mid-film — was being shown at the end
  of *every* movie.

### Provider playback
- **Managed Chrome is now owned, and torn down whole.** A replaced browser
  process could be terminated together with the one that replaced it, and a
  crash of Movie Party itself could leave a browser tree running. Teardown is now
  scoped to the process group on macOS and Linux, and to a kill-on-close job
  object on Windows.
- **Failures are reported instead of swallowed** in the local database writes and
  in provider command handling.

### Security
- **`rustls` 0.23.45.** This clears the dependency advisory that was red at
  v0.9.8; `cargo audit` passes.
- Network and provider-URL hardening, including a real URL parser for the
  provider destination checks.

### Release integrity
- **A version mismatch now blocks a release.** The consistency check existed but
  lived in a workflow that only runs when dispatched by hand, so it never ran on
  the one path that publishes. It is now a shared workflow that the release job
  *depends on*, so four declarations that disagree stop the publish rather than
  being discovered after it.
- **The shipped Windows runtime is integrity-checked.** The DLL bundled into the
  Windows installer was previously pinned only by filename — an asset replaced at
  the same URL would have been staged silently. Its SHA-256 is now verified
  before extraction.
- **All six macOS runtime source downloads are verified**, not just named.
- **A masked failure can no longer ship a degraded macOS runtime.** A shell
  pattern in the staging step could swallow a failed lookup and produce a build
  without its OpenGL loader; it now fails loudly instead.
- **The release body is derived from the tag's own notes.** The body was a
  hardcoded string that had been reused unchanged across three releases, so each
  one advertised the previous release's changes. A missing notes section now
  fails the release rather than publishing stale text.

### Testing
- **The test suite no longer reports passes it did not earn.** Tests that needed
  a real media runtime used to return early and report success without running a
  single assertion; they now fail loudly when the runtime is absent, and CI skips
  them explicitly and says so.
- Audio output is now exercised against a real audio-bearing fixture, and
  end-of-media is verified on a real player rather than only in unit tests.
- An orphaned source file that no build compiled — and whose only test could
  therefore never run — has been removed.

## 0.9.8 (P2 remediation)

The deferred P2 findings from the v0.9.6 audit, re-audited against the v0.9.7
code and fixed where they were still real. No new features, and no experimental
work — the Shared Movie Experience remains gated off exactly as before.

### Cinema and party UX
- **A hidden control dock is no longer clickable.** The dock faded out with
  opacity while its interactive container stayed clickable, so invisible
  Leave / play / chat buttons still swallowed clicks along the bottom of the
  screen. Interactivity now follows visibility.
- **Guests see the right leave wording.** A guest pressing Leave was shown the
  host-only "End Movie Party for everyone?" confirmation. The role was already
  tracked but never passed to the confirmation. (The backend was already
  correct — a guest has no host server to stop.)
- **Reconnects recover every time.** Dismissing "Keep Waiting" once used to
  disable the reconnect overlay permanently, so later disconnects passed
  silently. The dismissal is now scoped to one disconnect episode.
- **The countdown starts once.** The hand-off into Cinema could fire repeatedly
  because the effect's guard reset on every re-render. It is now keyed on the
  countdown's deadline, so duplicate ready events and re-renders cannot start
  the transition twice while a genuinely new countdown still can.
- **The call tile can no longer cover the controls.** Resizing the floating
  call tile did not re-clamp its position, so growing it could bury the control
  dock. It is now re-clamped against the new size.
- **The reaction tray sits above the call tile** (it was z-index 4 against the
  tile's 45), raised by the minimum step rather than escalating the whole
  stacking order.
- **"Back to lobby" actually goes back to the lobby.** On the cinema error
  screen it opened the Leave / End-for-everyone confirmation instead — a
  destructive action behind a non-destructive label.

### Settings
- **Typing your display name no longer gets wiped.** The draft was re-synced on
  every snapshot because the effect depended on the participants *array*, which
  the backend re-creates on every tick. It now depends on the name value.
- **The provider control no longer claims to log you out.** "Reset Provider
  Profile" asked you to confirm a logout and then only re-checked status. There
  is no command that clears a signed-in provider profile, so the control is now
  labelled for what it does.
- **"Clear cache" is no longer a dead button.** Cached copies are removed per
  party by the post-party prompt; there is no bulk clear, and the row now says
  so instead of offering a button that did nothing.

### Friends and calls
- **Invites are identity-only on the Rust side too.** The parser documented
  that smuggled extra fields invalidate a link but only read the two it wanted,
  so an invite carrying an auth key or token was accepted there while the
  frontend rejected it.
- **Re-accepting an invite no longer demotes a verified friend.** Re-opening an
  old link wiped the verification record and its cached path/latency.
- **Joined friends can show Online.** Both call sites passed a hard-coded
  "unknown" online hint, so the Online state was unreachable and every joined
  friend read "Offline" forever. The live tailnet status is now consulted, and
  an absent peer still reads Offline rather than a fabricated Online.
- **Turning the camera off releases the camera.** The toggle only muted the
  track, so the capture device stayed open and the macOS indicator stayed lit
  while the UI said the camera was off. Off now releases the device; on
  re-acquires it through the existing sender. The degradation ladder keeps its
  documented freeze behaviour.
- **A failed call can retry.** The session key was committed before the start
  resolved and never cleared on failure, so one transient error disabled the
  call for that configuration with no way back. Failures now clear the key and
  schedule one bounded retry.

### Playback, schedules and Home
- **libmpv recovery is possible again.** A single transient library failure set
  a sticky "unavailable" flag that made every later command fail for the life
  of the process; a successful load now clears it.
- **"Change movie" changes the movie.** The active party's media took
  precedence over an explicit pick, so the new choice was silently discarded.
- **The guest schedule prompt no longer dismisses on failure** — it stays up
  and says so, instead of marking the schedule answered when nothing reached
  the backend.
- **Schedules can be cancelled.** The backend command existed and was
  registered, but no UI could reach it, so "Cancelled" was a state nothing
  could produce.
- **Duplicate test id removed** in the guest schedule prompt.
- **The Home feed refreshes.** The 30-minute refresh aborted the request it had
  just issued, so the wall never refreshed and the aborted fetch was recorded
  as a network failure, corrupting the Settings diagnostic. Refreshes are now
  sequenced, and a malformed response is reported as a parse failure rather
  than a network one.
- **Display-name limits are character-counted**, not byte-counted, so valid
  non-Latin names are no longer silently rejected.

### Robustness
- **Externally launched processes are reaped** instead of being left as
  zombies for the life of the app.
- **A panic on file-descriptor exhaustion was removed** from the media range
  server; the connection now closes cleanly.

### Provider Shared — still not implemented
Unchanged and doubly gated: the experimental mode must clear a per-device
diagnostic *and* the stable path still refuses anything that is not Provider
Sync. The boundary is now covered by tests.

## 0.9.7 (stabilization: release blockers and security fixes)

A focused stabilization patch. No new features, and no experimental work —
the Shared Movie Experience remains gated off exactly as before.

### Reliability
- **A database upgrade can no longer be bricked by an interruption.** The
  migration runner now applies each step *and* its schema version in one
  transaction, so a crash mid-upgrade rolls back and retries on the next
  launch instead of failing forever. `ADD COLUMN` steps are now idempotent,
  which means a database that was interrupted after the column was added but
  before the version was recorded recovers cleanly. A database written by a
  *newer* build is now refused rather than having its version quietly
  rewritten backwards.
- **Playback failures are detected again.** The player event loop reported a
  failed player as `ERROR` while the failure watcher, both play gates, the
  Cinema view and the provider overlay all looked for `PLAYER_ERROR`. The
  mismatch meant a real playback failure was silently ignored — playback
  could be started on a broken player and the "Playback unavailable" message
  never appeared. One canonical failure state is now produced in one place,
  and the diagnostic message survives the event loop so recovery can fire.

### Security
- **The QUIC peer certificate is now genuinely verified.** Movie Party pins
  the host's certificate fingerprint, but the handshake-signature callbacks
  accepted any signature. Because a certificate is public, that pin did not
  actually prove the peer held the matching private key. Signatures are now
  verified against the certificate's own public key; fingerprint pinning and
  the app-layer join-secret authentication are unchanged.
- **Retention paths are contained.** The check guarding Movie Party's media
  cache used a prefix test that accepted `..`, so a crafted media id could
  resolve outside the cache directory. Media ids are now validated as single
  path components and the resolved path must be a direct child of the cache
  root.
- **Generic links can no longer target your own machine or network.**
  Pasting a link that resolves to loopback, a private range, a link-local or
  CGNAT address, `localhost`, or a `.local` name is now refused, so the
  managed browser can't be pointed at local services. Public links, including
  plaintext `http` to a public host, behave exactly as before.

### Fixed
- **Drag-and-drop in Create Party now works.** A dropped movie file used to
  show as selected while the backend received no path at all, producing a
  cinema room with nothing to play. Drops now resolve to a real file path
  through the native drag-drop event, and a drop Movie Party cannot use says
  so instead of pretending it worked.
- **You can leave the "Connect to your movie partner" screen.** When a join
  failed because the host was unreachable, that screen replaced the whole app
  and offered only Try Again and Setup help. It now has Back and Cancel,
  which return you to a clean Home state without bypassing Tailscale setup.
- The React hook dependency warning in the Cinema view is resolved rather
  than suppressed, and the Rust tree is fully `rustfmt`-clean, so both CI
  quality gates pass again.

## 0.9.6 (Cinema chat rebuilt, call tile diet, rename yourself)

### Preview — chat lives on the right side now
- Incoming chat bubbles render on the **right side** of the screen
  instead of the middle, so they never sit on top of the movie's
  subtitles or the action.
- Clicking the chat icon now shows your **chat history alongside the
  composer**, anchored to the right side as a slim panel. The panel
  **steps aside after 5 seconds** — but keep the cursor on it and it
  stays for as long as you do; moving away restarts the countdown.
  Clicking the chat icon again brings it straight back.
- The composer bar is narrower (it was unnecessarily wide) and stays
  available for typing after the history panel has auto-hidden.

### Preview — the call tile got out of the way
- The floating partner-video tile dropped its clutter: the "Away"
  status line and the close (x) button are gone. It now carries only
  your partner's name, a video toggle, and a minimize button.
- The **video toggle** shows or hides the partner's video — audio keeps
  playing while the video is hidden. Hiding the tile itself moved to
  the "Show call" button in the control dock (it toggles now).

### Settings — rename yourself
- Settings → General → Display name is now editable: type your name,
  hit Save (or just click away). It persists across restarts and is
  what your movie partner sees. Your device identity and trust chain
  are untouched by a rename — only the label changes.

### Bug fixes
- A stale error from one failed action no longer lingers on the
  snapshot after a later action succeeds.
- The compose bar is actually centered now (the centering transform
  was being clobbered by the entry animation).

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
