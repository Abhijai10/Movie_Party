import { describe, expect, it } from "vitest";
import {
  CHAT_BUBBLE_LIFETIME_MS,
  CHAT_BUBBLE_MAX_VISIBLE,
  DISPLACED_BUBBLE_LIFETIME_MS,
  bubbleMotion,
  bubbleStackOffsetPx,
  emptyBubbleQueue,
  enqueueChatBubble,
  pruneChatBubbles,
} from "./bubbleQueue";

describe("chat bubble queue (ADR-0003 / UI_UX_SPEC §35)", () => {
  it("enqueues with the full 5 s lifetime and a bumping version", () => {
    const queue = enqueueChatBubble(
      emptyBubbleQueue(),
      { id: "m1", sender: "Rahul", body: "bro that was insane" },
      1_000,
    );
    expect(queue.bubbles).toHaveLength(1);
    expect(queue.bubbles[0]?.lifetimeMs).toBe(CHAT_BUBBLE_LIFETIME_MS);
    expect(queue.bubbles[0]?.shownAtMs).toBe(1_000);
    expect(queue.version).toBe(1);
  });

  it("queues up to 3 visible; the 4th displaces the OLDEST (fade-sooner, never >3)", () => {
    let queue = emptyBubbleQueue();
    for (let index = 1; index <= 3; index += 1) {
      queue = enqueueChatBubble(
        queue,
        { id: `m${String(index)}`, sender: `s${String(index)}`, body: `b${String(index)}` },
        index * 1_000,
      );
    }
    expect(queue.bubbles).toHaveLength(CHAT_BUBBLE_MAX_VISIBLE);
    // Enqueue m4 at 4_000 → the visible set is hard-capped at 3: m1 (the
    // oldest, whose full lifetime would run to 6_000) leaves first, and
    // the remaining bubbles keep their lifetimes (m2 shortens only under
    // the displacement rule because it now overflows).
    queue = enqueueChatBubble(
      queue,
      { id: "m4", sender: "s4", body: "b4" },
      4_000,
    );
    expect(queue.bubbles).toHaveLength(CHAT_BUBBLE_MAX_VISIBLE);
    // The displaced bubble is gone from the visible set...
    expect(queue.bubbles.some((bubble) => bubble.id === "m1")).toBe(false);
    // ...the survivor order is preserved...
    expect(queue.bubbles.map((bubble) => bubble.id)).toEqual(["m2", "m3", "m4"]);
    // ...and a 5th arrival retires m2 with a fade-sooner lifetime
    // (visible set stays ≤ 3; the retired bubble is exposed for one
    // exit-animation pass, never an instant vanish).
    const after5 = enqueueChatBubble(queue, { id: "m5", sender: "s5", body: "b5" }, 4_500);
    expect(after5.bubbles.map((bubble) => bubble.id)).toEqual(["m3", "m4", "m5"]);
    const retired = after5.lastRetired ?? [];
    expect(retired.map((bubble) => bubble.id)).toEqual(["m2"]);
    // m2 was shown at 2_000; its full lifetime would end at 7_000. Retired
    // at 4_500, it fades after the displaced 1_800 ms window.
    expect(retired[0]?.lifetimeMs).toBe(2_500 + DISPLACED_BUBBLE_LIFETIME_MS);
    expect(retired[0]?.lifetimeMs).toBeLessThan(CHAT_BUBBLE_LIFETIME_MS);
    // An enqueue under the cap retires nothing.
    expect(emptyBubbleQueue === emptyBubbleQueue).toBe(true);
    // The newest keeps the full lifetime.
    const newest = queue.bubbles[queue.bubbles.length - 1];
    expect(newest?.lifetimeMs).toBe(CHAT_BUBBLE_LIFETIME_MS);
  });

  it("never lets the queue exceed the cap of 3", () => {
    let queue = emptyBubbleQueue();
    for (let index = 0; index < 10; index += 1) {
      queue = enqueueChatBubble(
        queue,
        { id: `m${String(index)}`, sender: "s", body: "b" },
        index * 100,
      );
    }
    expect(queue.bubbles.length).toBeLessThanOrEqual(CHAT_BUBBLE_MAX_VISIBLE);
  });

  it("prunes only bubbles whose lifetime elapsed, keeping the rest", () => {
    let queue = enqueueChatBubble(
      emptyBubbleQueue(),
      { id: "old", sender: "s", body: "b" },
      0,
    );
    queue = enqueueChatBubble(queue, { id: "new", sender: "s", body: "b" }, 4_000);
    // At 5_500: "old" (0 + 5_000) expired; "new" (4_000 + 5_000) lives.
    const pruned = pruneChatBubbles(queue, 5_500);
    expect(pruned.bubbles.map((bubble) => bubble.id)).toEqual(["new"]);
    // No-op prune returns the same queue object (no version churn).
    const again = pruneChatBubbles(pruned, 5_600);
    expect(again).toBe(pruned);
  });

  it("offsets above the bottom-15% subtitle zone only when subtitles are active (§66)", () => {
    expect(bubbleStackOffsetPx(1_000, true)).toBe(150);
    expect(bubbleStackOffsetPx(1_000, false)).toBe(0);
    // Degenerate movie heights never produce a bogus offset.
    expect(bubbleStackOffsetPx(0, true)).toBe(0);
    expect(bubbleStackOffsetPx(Number.NaN, true)).toBe(0);
  });

  it("reduced motion selects a plain fade (§35 / accessibility)", () => {
    expect(bubbleMotion(true)).toBe("fade");
    expect(bubbleMotion(false)).toBe("float");
  });
});
