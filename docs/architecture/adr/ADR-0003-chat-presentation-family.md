# ADR-0003: Chat Presentation Family (Decision D2)

**Status:** Accepted  
**Date:** 2026-09-05 (Batch 17 chat work)  
**Author:** Batch 17 implementation agent  
**Supersedes:** the chat-size family codified in `0_Remaining_Things.md` §16.1 (the "360–420 px shared overlay" family)  
**Superseded by:** N/A

---

# 1. CONTEXT

Two project documents specify different chat presentation families, and
they contradict each other:

- **`UI_UX_SPEC.md` §34–§36** (locked core document) specifies:
  - chat compose: `min(560px, 70vw)` bottom-center, above the control dock;
  - chat history (press C): `min(620px, 75vw) × min(560px, 70vh)` centered
    translucent card, movie remains full size behind, no auto-pause;
  - message presentation (§35): lower-third ephemeral bubbles, 5 s default
    lifetime, message queue, max 3 simultaneously visible, older messages
    fade sooner if necessary, subtitle-zone avoidance (move chat higher when
    subtitles are present).

- **`0_Remaining_Things.md` §16.1** codified the implemented family:
  360–420 px width, 420–520 px height, right-anchored overlay
  (`.cinema-chat-overlay`), 5 s transient preview, manual mode with a
  persistent composer.

The 2026-09-05 audit (finding P5 / decision D2) flagged the contradiction
and required this ADR before the Batch 17 chat work, with the losing
document amended.

Current phase: V1 feature completion (Batch 17 — Chat & Cinema Spec
Alignment, findings P4/P5/P7).

Affected subsystem: frontend chat presentation in Cinema Mode and Lobby
(`src/overlays/`, `src/components/mp/ChatOverlay.tsx`, `src/views/CinemaView.tsx`).

Expected behavior from the locked docs: the UI_UX_SPEC §34–36 family is the
spec'd contract; AGENTS.md §0 subordinates everything below the core docs.

Observed behavior: the implemented overlay matches the §16.1 family
(right-anchored 360–420 px panel), not §34–36.

# 2. REQUIREMENT THAT TRIGGERED THIS ADR

The audit demanded a single, documented decision (D2) resolving which chat
family is canonical, plus an amendment to the losing document, before any
Batch 17 chat code was written.

# 3. OPTIONS

**A. Keep the §16.1 family** (right-anchored panel, amend UI_UX_SPEC §34–36).

- Pros: zero frontend rework; the panel is already tested.
- Cons: amends a locked core document to match code — the exact inversion
  of authority AGENTS §0 forbids; the spec family (bottom-center compose,
  centered history, lower-third ephemeral bubbles) is the designed cinema
  experience and matches PRD's "movie first" principle (§37: overlays only,
  never sidebars) more faithfully for ephemeral messages; a right-anchored
  panel occupies the movie's edge persistently.

**B. Adopt the UI_UX_SPEC §34–36 family** (chosen).

- Pros: honors the locked core document unchanged; lower-third ephemeral
  bubbles are less intrusive than a panel for the common case (a quick
  message during the movie); the compose/history split matches the two
  distinct intents (send now vs. read backlog); centered history keeps the
  movie full-size behind (§36); amend only `0_Remaining_Things.md` — a
  status document, not a spec.
- Cons: frontend rework of the chat overlay surface; the existing overlay's
  behavioral tests (transient preview, unread badge, Ghost/Privacy
  suppression) must be preserved through the migration.

**C. Hybrid** (panel for history, spec bubbles for ephemeral).

- Rejected as a false middle: it keeps both layout families alive and
  re-creates the ambiguity this ADR exists to remove.

# 4. DECISION

**Adopt the UI_UX_SPEC §34–36 family.** Specifically:

1. **Ephemeral message presentation (§35)**: incoming chat messages render
   as lower-third bubbles — 5 s default lifetime, queued, max 3
   simultaneously visible, older messages fade sooner when the cap is
   exceeded, moved above the subtitle-sensitive zone (bottom 15 % of movie
   height, §66) when subtitles are active. Reduced-motion preference is
   respected (no float/translate animations; a plain fade).
2. **Compose (§34)**: the composer is a bottom-center input above the
   control dock, `min(560px, 70vw)` — invoked on Enter, not a big panel.
3. **History (§36)**: press C opens the backlog as a centered translucent
   card, `min(620px, 75vw) × min(560px, 70vh)`; the movie stays full size
   behind; opening history never auto-pauses playback.
4. **Behavioral rules preserved from §16.1** (they never conflicted with
   §34–36): Ghost/Privacy suppression, unread badge while hidden, typing
   never destroyed by the transient timer, manual open/close.

`0_Remaining_Things.md` §16.1 is amended to record this decision (loser
doc amended; UI_UX_SPEC stands unchanged).

# 5. ROLLBACK

Revert the chat overlay commits and restore `.cinema-chat-overlay`; the
§16.1 family remains in git history. No wire-format, storage, or protocol
surface depends on the presentation family, so rollback is purely
presentational.

# 6. TEST PLAN

- Pure state model (vitest): bubble-queue semantics — enqueue, 5 s
  lifetime, max-3 cap with oldest fading sooner (shortened lifetime),
  subtitle-zone offset, reduced-motion mode.
- Existing behavioral tests stay green through the migration (Ghost/
  Privacy suppression, unread badge, typing preservation).
- Manual (external, per §31 matrix): visual confirmation on macOS and
  Windows that the lower-third zone avoids subtitles on real content.

# 7. CONSEQUENCES

- The right-anchored `.cinema-chat-overlay` family is retired from Cinema
  Mode; the Lobby may keep the shared overlay family where it does not
  conflict (the spec's chat sections target Cinema Mode).
- Camera-card spec values (P7, §28–29) are unaffected by this ADR and land
  in the same batch as separate work.
