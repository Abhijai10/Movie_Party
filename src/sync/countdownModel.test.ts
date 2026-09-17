import { describe, expect, it } from "vitest";
import { COUNTDOWN_LEAD_MS, countdownDisplayFrom, countdownHandoff } from "./countdownModel";

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

describe("F8 — the cinema hand-off fires exactly once per countdown", () => {
  it("fires on the first observation of a deadline", () => {
    const first = countdownHandoff(10_000, null);
    expect(first.shouldStart).toBe(true);
    expect(first.nextStartedFor).toBe(10_000);
  });

  it("does not fire again for the same deadline", () => {
    // Every re-render, effect re-run, duplicate ready event or reconnect
    // replays the same deadline — none of them may start the transition twice.
    const repeated = countdownHandoff(10_000, 10_000);
    expect(repeated.shouldStart).toBe(false);
    expect(repeated.nextStartedFor).toBe(10_000);
  });

  it("fires again for a genuinely new countdown", () => {
    const next = countdownHandoff(20_000, 10_000);
    expect(next.shouldStart).toBe(true);
    expect(next.nextStartedFor).toBe(20_000);
  });

  it("never fires without a deadline, and disarms when one is cleared", () => {
    const idle = countdownHandoff(null, 10_000);
    expect(idle.shouldStart).toBe(false);
    expect(idle.nextStartedFor).toBeNull();
  });

  it("survives many replays of one countdown without a second start", () => {
    let startedFor: number | null = null;
    let starts = 0;
    for (let tick = 0; tick < 50; tick++) {
      const decision = countdownHandoff(10_000, startedFor);
      startedFor = decision.nextStartedFor;
      if (decision.shouldStart) starts += 1;
    }
    expect(starts).toBe(1);
  });
});
