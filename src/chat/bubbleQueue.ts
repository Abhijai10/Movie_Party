/**
 * ADR-0003 / UI_UX_SPEC §35 — ephemeral chat bubble queue.
 *
 * Pure state model for lower-third chat message bubbles:
 * - default lifetime 5 s (CHAT_BUBBLE_LIFETIME_MS);
 * - messages queue;
 * - maximum simultaneously visible: 3 (CHAT_BUBBLE_MAX_VISIBLE);
 * - when the cap is exceeded, the OLDEST message fades sooner — its
 *   remaining lifetime is shortened so the cap is never visually
 *   exceeded (§35: "Older messages fade sooner if necessary");
 * - subtitle-zone avoidance: when subtitles are active the bubble stack
 *   is offset above the bottom 15 % of movie height (§66);
 * - reduced motion: no float/translate — the model carries the flag so
 *   the view renders a plain fade.
 */

export const CHAT_BUBBLE_LIFETIME_MS = 5_000;
export const CHAT_BUBBLE_MAX_VISIBLE = 3;
/** §66 subtitle-sensitive zone: bottom 15 % of movie height. */
export const SUBTITLE_ZONE_FRACTION = 0.15;
/** Shortened lifetime for a bubble displaced by the cap (fades sooner). */
export const DISPLACED_BUBBLE_LIFETIME_MS = 1_800;

export type ChatBubble = {
  id: string;
  sender: string;
  body: string;
  /** Monotonic ms timestamp the bubble was enqueued at. */
  shownAtMs: number;
  /** Lifetime this bubble was granted (full or displaced-shortened). */
  lifetimeMs: number;
};

export type ChatBubbleQueue = {
  bubbles: ChatBubble[];
  /** Bumped whenever the queue changes so views can key animations. */
  version: number;
  /**
   * Bubbles retired by the cap on the most recent enqueue, with their
   * fade-sooner lifetimes. The view renders their exit animation once;
   * the visible `bubbles` set never exceeds the cap.
   */
  lastRetired?: ChatBubble[];
};

export function emptyBubbleQueue(): ChatBubbleQueue {
  return { bubbles: [], version: 0 };
}

/**
 * Enqueue an incoming message. The newest message always gets the full
 * lifetime. If the queue is at the cap, the OLDEST bubble's lifetime is
 * shortened to the displaced value so it fades sooner — never dropped
 * instantly (the content stays readable for a beat), never left past the
 * cap.
 */
export function enqueueChatBubble(
  queue: ChatBubbleQueue,
  message: { id: string; sender: string; body: string },
  nowMs: number,
): ChatBubbleQueue {
  const bubble: ChatBubble = {
    id: message.id,
    sender: message.sender,
    body: message.body,
    shownAtMs: nowMs,
    lifetimeMs: CHAT_BUBBLE_LIFETIME_MS,
  };
  const bubbles = [...queue.bubbles, bubble];
  // §35: "Maximum simultaneously visible: 3. Older messages fade sooner if
  // necessary." When the queue is at the cap, the OLDEST bubble is retired
  // from the visible set — but with a fade-sooner lifetime so it never
  // vanishes instantly (the displaced lifetime, measured from now).
  const overflow = bubbles.length - CHAT_BUBBLE_MAX_VISIBLE;
  const displaced = overflow > 0 ? bubbles.slice(0, overflow) : [];
  const kept = overflow > 0 ? bubbles.slice(overflow) : bubbles;
  const retired: ChatBubble[] = [];
  for (const old of displaced) {
    const remaining = old.shownAtMs + old.lifetimeMs - nowMs;
    retired.push(
      remaining <= DISPLACED_BUBBLE_LIFETIME_MS
        ? old
        : {
            ...old,
            lifetimeMs: nowMs - old.shownAtMs + DISPLACED_BUBBLE_LIFETIME_MS,
          },
    );
  }
  const finalBubbles = kept.filter(
    (candidate) => nowMs < candidate.shownAtMs + candidate.lifetimeMs,
  );
  // The retired bubbles are exposed for one render pass of fade-out via
  // `retiring` — the visible set itself is hard-capped.
  return {
    bubbles: finalBubbles,
    version: queue.version + 1,
    ...(retired.length > 0 ? { lastRetired: retired } : {}),
  };
}

/**
 * Drop bubbles whose lifetime has elapsed at `nowMs`.
 */
export function pruneChatBubbles(queue: ChatBubbleQueue, nowMs: number): ChatBubbleQueue {
  const bubbles = queue.bubbles.filter(
    (bubble) => nowMs < bubble.shownAtMs + bubble.lifetimeMs,
  );
  if (bubbles.length === queue.bubbles.length) {
    return queue;
  }
  return { bubbles, version: queue.version + 1 };
}

/**
 * §35/§66: the vertical offset (px) for the bubble stack, given the movie
 * height and whether subtitles are active. Without subtitles the stack
 * sits in the lower third; with subtitles it clears the bottom 15 %
 * subtitle-sensitive zone.
 */
export function bubbleStackOffsetPx(movieHeightPx: number, subtitlesActive: boolean): number {
  if (!Number.isFinite(movieHeightPx) || movieHeightPx <= 0) {
    return 0;
  }
  return subtitlesActive ? Math.round(movieHeightPx * SUBTITLE_ZONE_FRACTION) : 0;
}

/** §35 reduced-motion: plain fade, no float/translate. */
export function bubbleMotion(reducedMotion: boolean): "fade" | "float" {
  return reducedMotion ? "fade" : "float";
}
