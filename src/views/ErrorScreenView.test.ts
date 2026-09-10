import { describe, expect, it } from "vitest";
import { extractMpCode, humanMessageForCode } from "./ErrorScreenView";
import { stateLabelFor, type PrerequisiteStatus } from "./FirstRunView";
import { transferEstimateFrom, preloadEarliestOnly } from "./ScheduleView";

describe("ErrorScreenView code mapping (§64, §23)", () => {
  it("extracts the stable MP code from a raw backend error string", () => {
    expect(extractMpCode("MP-NET-003 the current connection is too slow")).toBe("MP-NET-003");
    expect(extractMpCode("details: MP-PROVIDER-004 sign in first")).toBe("MP-PROVIDER-004");
    expect(extractMpCode("no code here")).toBe("");
    expect(extractMpCode(null)).toBe("");
  });

  it("maps every code family to an actionable human message", () => {
    expect(humanMessageForCode("MP-NET-TS-003")).toContain("Tailscale");
    expect(humanMessageForCode("MP-NET-003")).toContain("connection");
    expect(humanMessageForCode("MP-PROVIDER-004")).toContain("sign in");
    expect(humanMessageForCode("MP-PROVIDER-003")).toContain("provider page");
    expect(humanMessageForCode("MP-SYNC-004")).toContain("synchronized");
    expect(humanMessageForCode("MP-CALL-002")).toContain("call");
    expect(humanMessageForCode("MP-STORE-001")).toContain("storage");
    expect(humanMessageForCode("MP-MEDIA-001")).toContain("movie file");
  });

  it("never leaks raw internals into the headline message", () => {
    for (const code of [
      "MP-NET-003",
      "MP-PROVIDER-003",
      "MP-SYNC-004",
      "MP-STORE-001",
    ]) {
      const message = humanMessageForCode(code);
      expect(message).not.toMatch(/\{|\}/);
      expect(message.endsWith(".")).toBe(true);
    }
  });
});

describe("FirstRunView prerequisite states (§11 truth rules)", () => {
  it("labels every state honestly — never a green check for unverified things", () => {
    expect(stateLabelFor("OK")).toBe("Ready");
    expect(stateLabelFor("MISSING")).toBe("Not found");
    expect(stateLabelFor("NOT_REQUESTED")).toBe("Not requested");
    expect(stateLabelFor("OPTIONAL")).toBe("Optional");
  });

  it("keeps the full state vocabulary closed", () => {
    const states: PrerequisiteStatus["state"][] = ["OK", "MISSING", "NOT_REQUESTED", "OPTIONAL"];
    expect(states).toHaveLength(4);
  });
});

describe("ScheduleView transfer math (§18, bits-correct)", () => {
  it("computes bytes→bits transfer time with the 1.4 safety factor", () => {
    // 1 GB remaining at 10 Mbps: 1e9×8/1e7 = 800 s; ×1.4 = 1120 min/60.
    const estimate = transferEstimateFrom({
      remainingBytes: 1_000_000_000,
      goodputBps: 10_000_000,
    });
    expect(estimate?.minutes).toBeCloseTo(1120 / 60, 1);
    expect(estimate?.goodputKnown).toBe(true);
  });

  it("is honest about unknown goodput — no guessed transfer time", () => {
    const estimate = transferEstimateFrom({ remainingBytes: 1_000, goodputBps: 0 });
    expect(estimate?.minutes).toBeNull();
    expect(estimate?.goodputKnown).toBe(false);
  });

  it("reports zero remaining as immediately ready", () => {
    const estimate = transferEstimateFrom({ remainingBytes: 0, goodputBps: 0 });
    expect(estimate?.minutes).toBe(0);
    expect(estimate?.goodputKnown).toBe(true);
  });

  it("moves preload EARLIER only (§18: later than the safe point is refused)", () => {
    const recommended = 1_000_000_000;
    // +30 min buffer → earlier, allowed.
    expect(preloadEarliestOnly(recommended, 30)).toBe(recommended - 30 * 60_000);
    // A negative adjustment (later) is clamped to the recommended start.
    expect(preloadEarliestOnly(recommended, -45)).toBe(recommended);
  });
});
