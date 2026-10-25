import { describe, expect, it } from "vitest";
import { Phase, canSend, disabledReasons, initialState, nextSequence, reduce } from "./state.js";

const NETFLIX = "https://www.netflix.com/title/80181170";
const PAIRED_CAPABILITIES = ["identify_tab", "report_content", "report_playback"];

function paired() {
  let state = initialState();
  state = reduce(state, { type: "ENABLE" });
  return reduce(state, { type: "PAIRED", capabilities: PAIRED_CAPABILITIES });
}

function active() {
  return reduce(paired(), { type: "SELECT_TAB", tabId: 4, url: NETFLIX });
}

describe("companion session", () => {
  it("does nothing until the user turns it on", () => {
    const state = initialState();
    expect(state.phase).toBe(Phase.Off);
    expect(canSend(state, "reportContent")).toBe(false);
    // Enabling never skips pairing.
    expect(reduce(state, { type: "ENABLE" }).phase).toBe(Phase.NeedsPairing);
    expect(reduce(state, { type: "PAIRED", capabilities: [] }).phase).toBe(Phase.Off);
  });

  it("grants only what Movie Party said this provider can do", () => {
    const state = active();
    expect([...state.granted].sort()).toEqual([...PAIRED_CAPABILITIES].sort());
    expect(canSend(state, "controlPlayback")).toBe(false);
    expect(canSend(state, "requestNavigation")).toBe(false);
    expect(canSend(state, "identifyTab")).toBe(true);
    // A later, narrower grant cannot reopen what the app withheld.
    const narrowed = reduce(state, {
      type: "SELECT_TAB",
      tabId: 5,
      url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
      capabilities: ["identify_tab"],
    });
    expect([...narrowed.granted]).toEqual(["identify_tab"]);
  });

  it("never reads or remembers a tab outside the provider allowlist", () => {
    const state = reduce(paired(), {
      type: "SELECT_TAB",
      tabId: 9,
      url: "https://mail.google.com/inbox/secrets",
    });
    expect(state.tabId).toBeNull();
    expect(state.url).toBeNull();
    expect(state.error).toBe("OriginNotAllowed");
    expect(state.phase).toBe(Phase.Ready);
    expect(JSON.stringify({ ...state, granted: [...state.granted] })).not.toContain("google");
  });

  it("loses the session on every real-world disruption", () => {
    for (const event of [
      { type: "TAB_CHANGED" },
      { type: "BROWSER_CLOSED" },
      { type: "PERMISSION_DENIED" },
      { type: "VERSION_MISMATCH" },
      { type: "HOST_UNREACHABLE" },
    ]) {
      const state = reduce(active(), event);
      expect(state.phase).toBe(Phase.Error);
      expect(state.tabId).toBeNull();
      expect(canSend(state, "identifyTab")).toBe(false);
      expect(state.error).toBeTypeOf("string");
      // Recovery is always available; a dead end would strand the user.
      expect(reduce(state, { type: "BACK_TO_READY" }).phase).toBe(Phase.Ready);
    }
  });

  it("switches one capability off without losing the rest", () => {
    const state = reduce(active(), {
      type: "CAPABILITY_REFUSED",
      capability: "report_playback",
      reason: "No visible media element on this page yet.",
    });
    expect(state.granted.has("report_playback")).toBe(false);
    expect(state.granted.has("identify_tab")).toBe(true);
    expect(state.phase).toBe(Phase.Active);
    expect(canSend(state, "reportPlayback")).toBe(false);
    expect(canSend(state, "identifyTab")).toBe(true);
    expect(disabledReasons(state).report_playback).toMatch(/visible media/);
  });

  it("ends the session when Movie Party closes, but keeps the pairing", () => {
    const state = reduce(active(), { type: "LEAVE_APP" });
    expect(state.phase).toBe(Phase.Ready);
    expect(state.tabId).toBeNull();
    expect(state.granted.size).toBe(0);
    expect(state.available.size).toBe(PAIRED_CAPABILITIES.length);
  });

  it("unlinks completely, secret and capabilities included", () => {
    const state = reduce(active(), { type: "DISCONNECT" });
    expect(state.phase).toBe(Phase.NeedsPairing);
    expect(state.granted.size).toBe(0);
    expect(state.available.size).toBe(0);
    expect(state.sequence).toBe(0);
  });

  it("advances the sequence so a captured message cannot be replayed", () => {
    const state = active();
    expect(nextSequence(state)).toBe(state.sequence + 1);
    const again = reduce(state, { type: "SELECT_TAB", tabId: 4, url: NETFLIX });
    expect(again.sequence).toBeGreaterThan(state.sequence);
  });

  it("ignores an event it does not know instead of treating it as permission", () => {
    const state = active();
    expect(reduce(state, { type: "GRANT_EVERYTHING" })).toBe(state);
    expect(reduce(state, {})).toBe(state);
    expect(reduce(state, null)).toBe(state);
  });

  it("refuses unknown message kinds outright", () => {
    const state = active();
    for (const kind of ["readCookies", "readHistory", "attachDebugger", ""]) {
      expect(canSend(state, kind)).toBe(false);
    }
  });
});
