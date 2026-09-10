/**
 * UI_UX_SPEC §25: "Countdown timing must come from sync engine. Frontend
 * animation must not create its own unsynchronized countdown."
 *
 * Pure model for the backend-driven Ready-Check countdown. The host's
 * `request_play_countdown` schedules the canonical play commit at a
 * backend-owned deadline and exposes it on the snapshot; this module
 * derives what the UI should display AT ANY INSTANT from that deadline.
 * There is no frontend timer chain deciding when playback starts — the
 * backend commit at the deadline is authoritative.
 */

/** §25 countdown lead — must match Rust `COUNTDOWN_LEAD_US`. */
export const COUNTDOWN_LEAD_MS = 3_000;

/** Display rounding granularity (§25 shows whole seconds 3-2-1). */
export const COUNTDOWN_STEP_MS = 900;

export type CountdownDisplay =
  | { phase: "idle" }
  | { phase: "running"; remaining: number; label: number }
  | { phase: "done" };

/**
 * Derive the countdown display from the backend deadline.
 *
 * `nowWallMs` is the current wall clock (UI display only — legal per
 * §16; execution itself is the backend's monotonic commit).
 * `executeAtWallMs` is the backend-provided projection of the same
 * deadline. A negative remaining time is "done" — the backend commit is
 * the authority, the UI just reflects it.
 */
export function countdownDisplayFrom(
  executeAtWallMs: number | null | undefined,
  nowWallMs: number,
): CountdownDisplay {
  if (executeAtWallMs == null || executeAtWallMs <= 0) {
    return { phase: "idle" };
  }
  const remaining = executeAtWallMs - nowWallMs;
  if (remaining <= 0) {
    return { phase: "done" };
  }
  // Whole-second label: the §25 3-2-1 display. remaining 3000→"3",
  // 2999→"3", 2000→"2", 1000→"1", 1→"1" (ceil keeps "1" alive through
  // the final second instead of flashing "0" before the commit).
  const label = Math.min(3, Math.max(1, Math.ceil(remaining / 1_000)));
  return { phase: "running", remaining, label };
}
