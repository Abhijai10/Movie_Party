import { describe, expect, it } from "vitest";
import { COUNTDOWN_LEAD_MS, countdownDisplayFrom } from "./countdownModel";

describe("§25 backend-driven countdown model", () => {
  it("is idle without a backend-provided deadline", () => {
    expect(countdownDisplayFrom(null, 1_000)).toEqual({ phase: "idle" });
    expect(countdownDisplayFrom(0, 1_000)).toEqual({ phase: "idle" });
    expect(countdownDisplayFrom(undefined, 1_000)).toEqual({ phase: "idle" });
  });

  it("derives the label from the BACKEND deadline, not a local timer", () => {
    const deadline = 10_000;
    // 3 s out → "3"
    expect(countdownDisplayFrom(deadline, 7_000)).toEqual({
      phase: "running",
      remaining: 3_000,
      label: 3,
    });
    // 2 s out → "2"
    expect(countdownDisplayFrom(deadline, 8_000)).toEqual({
      phase: "running",
      remaining: 2_000,
      label: 2,
    });
    // 1 s out → "1"
    expect(countdownDisplayFrom(deadline, 9_000)).toEqual({
      phase: "running",
      remaining: 1_000,
      label: 1,
    });
  });

  it("is done at or past the deadline (the backend commit is authoritative)", () => {
    expect(countdownDisplayFrom(10_000, 10_000)).toEqual({ phase: "done" });
    expect(countdownDisplayFrom(10_000, 11_000)).toEqual({ phase: "done" });
  });

  it("never shows more than 3 even if the clock jumps backwards", () => {
    // A wall-clock jump after the countdown started must not display a
    // bogus "4" — the label is capped at the §25 3-2-1 sequence.
    const jumped = countdownDisplayFrom(10_000, 10_000 - COUNTDOWN_LEAD_MS - 5_000);
    expect(jumped.phase === "running" ? jumped.label : 0).toBe(3);
  });
});
