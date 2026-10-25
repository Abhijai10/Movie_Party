import { describe, expect, it } from "vitest";
import type { CompanionStatus } from "../backend/appRuntime";
import {
  CAPABILITY_LABELS,
  companionCapabilityLines,
  companionHeadline,
  companionStep,
  companionSubline,
  usablePairingCode,
} from "./companionState";

const status = (over: Partial<CompanionStatus> = {}): CompanionStatus => ({
  listening: false,
  port: 0,
  pairingCode: null,
  paired: false,
  browser: null,
  protocol: 1,
  granted: [],
  refused: [
    { capability: "control_playback", reason: "Never observed on a protected player." },
    { capability: "request_navigation", reason: "Needs your explicit click." },
  ],
  error: null,
  guidance: null,
  ...over,
});

describe("companion settings copy", () => {
  it("walks the user through the steps in the real order", () => {
    expect(companionStep(null)).toBe("load");
    expect(companionStep(status())).toBe("start");
    expect(companionStep(status({ listening: true }))).toBe("pair");
    expect(companionStep(status({ listening: true, paired: true }))).toBe("chooseTab");
  });

  it("never offers a pairing code while the listener is off", () => {
    const withCode = status({ pairingCode: "ABCD2345" });
    expect(usablePairingCode(withCode)).toBeNull();
    expect(usablePairingCode(status({ listening: true, pairingCode: "ABCD2345" }))).toBe(
      "ABCD2345",
    );
    // A stale code from a previous session must not be shown as usable.
    expect(usablePairingCode(status({ paired: true, pairingCode: "ABCD2345" }))).toBeNull();
  });

  it("says plainly that nothing runs until the user opts in", () => {
    expect(companionSubline(status())).toMatch(/Nothing runs until/i);
    expect(companionSubline(status({ listening: true }))).toMatch(/five attempts/);
    expect(companionSubline(status({ listening: true, paired: true }))).toMatch(
      /only the tab you choose/,
    );
  });

  it("lists refused capabilities with their reason rather than hiding them", () => {
    const lines = companionCapabilityLines(status({ listening: true, granted: ["identify_tab"] }));
    expect(lines).toContain("Recognise the provider tab you chose: allowed");
    expect(lines.some((line) => line.startsWith("Play, pause or seek from Movie Party: off"))).toBe(
      true,
    );
    expect(lines.join("\n")).toContain("Never observed on a protected player.");
  });

  it("names every capability the protocol can grant or refuse", () => {
    for (const name of [
      "identify_tab",
      "report_content",
      "report_playback",
      "request_navigation",
      "control_playback",
    ]) {
      expect(CAPABILITY_LABELS[name]).toBeTruthy();
    }
  });

  it("heads each step with an action, not a status lamp", () => {
    expect(companionHeadline(status())).toMatch(/Turn the companion on/);
    expect(companionHeadline(status({ listening: true }))).toMatch(/Pair your browser/);
    expect(companionHeadline(status({ listening: true, paired: true }))).toMatch(
      /Open the extension/,
    );
  });
});
