# Movie Party — Release Notes

## 0.1.0 (V1 code-complete baseline)

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
- Provider Shared Mode: experimental diagnostic tier (D3-B) — capture
  verification + explicit Sync fallback; full transport pending the
  real-DRM spike (Batch 20 gate)

### Known limitations
- Provider Shared Mode is diagnostic-only in 0.1.0 (D3-B)
- macOS ScreenCaptureKit diagnostic requires ffmpeg (`brew install ffmpeg`)
- Windows capture spike requires a physical Windows machine (Batch 21)

The invite flow is complete: the lobby copies the full movieparty://
link (browser-openable — the OS hands it to Movie Party's registered
deep link), a short human code, or a scannable QR (§16); guests get an
explicit schedule accept/decline (§56) and the post-party keep/remove/
save-as retention question (§52).

### Verification status
All automated gates green (Rust fmt/clippy/363+ tests; frontend
lint/tsc/137 vitest/build). The §31.1 manual matrix (two devices,
real providers, real DRM content) is pending — see
IMPLEMENTATION_TRACKER.md for the external-verification register.
