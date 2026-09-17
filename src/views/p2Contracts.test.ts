import { describe, expect, it } from "vitest";
import { cinemaDockInteractionClass } from "../components/mp/CinemaControls";
import { answerRecorded } from "../components/mp/GuestScheduleAccept";
import { reconnectLatchAfter } from "../overlays/ReconnectOverlay";
import { endPartyConfirmCopy } from "./EndPartyConfirmView";
import { canCancelSchedule } from "./HomeView";
import { PROVIDER_RECHECK_LABEL } from "./SettingsView";

/**
 * Contracts behind the v0.9.8 P2 remediation. Each of these encodes a rule the
 * code previously broke, so a regression fails here rather than in the UI.
 */

describe("F5 — cinema dock interactivity follows visibility", () => {
  it("is interactive only while visible", () => {
    expect(cinemaDockInteractionClass(true)).toBe("pointer-events-auto");
    expect(cinemaDockInteractionClass(false)).toBe("pointer-events-none");
  });

  it("never leaves a hidden dock clickable", () => {
    // The dock hides with opacity alone, which does not stop hit-testing.
    expect(cinemaDockInteractionClass(false)).not.toContain("auto");
  });
});

describe("F6 — leave/end confirmation wording matches the role", () => {
  it("tells a host the movie stops for both", () => {
    const host = endPartyConfirmCopy(true);
    expect(host.title).toMatch(/everyone/i);
    expect(host.body).toMatch(/both participants/i);
    expect(host.confirmLabel).toBe("End Party");
  });

  it("never shows a guest the host-only wording", () => {
    const guest = endPartyConfirmCopy(false);
    expect(guest.title).not.toMatch(/everyone/i);
    expect(guest.body).not.toMatch(/both participants/i);
    expect(guest.body).toMatch(/leave/i);
    expect(guest.confirmLabel).toBe("Leave Party");
  });
});

describe("F7 — the reconnect dismissal latch is per episode", () => {
  it("holds while the room is still reconnecting", () => {
    expect(reconnectLatchAfter("RECONNECTING", true)).toBe(true);
    expect(reconnectLatchAfter("RECONNECTING", false)).toBe(false);
  });

  it("clears once the room leaves RECONNECTING, so the next episode shows", () => {
    for (const state of ["PLAYING", "PAUSED", "LOBBY", "ENDED"]) {
      expect(reconnectLatchAfter(state, true)).toBe(false);
    }
  });

  it("models the full cycle: dismiss → recover → disconnect again", () => {
    let latch = false;
    latch = reconnectLatchAfter("RECONNECTING", latch); // episode 1 begins
    expect(latch).toBe(false); // overlay visible
    latch = true; // user pressed Keep Waiting
    latch = reconnectLatchAfter("RECONNECTING", latch);
    expect(latch).toBe(true); // still dismissed within the same episode
    latch = reconnectLatchAfter("PLAYING", latch); // connection restored
    expect(latch).toBe(false);
    latch = reconnectLatchAfter("RECONNECTING", latch); // episode 2 begins
    expect(latch).toBe(false); // overlay visible again — the old bug
  });
});

describe("F38 — the guest schedule prompt only dismisses on a recorded answer", () => {
  it("dismisses when the backend answered", () => {
    expect(answerRecorded({ screen: "LOBBY" })).toBe(true);
  });

  it("stays open when the call failed", () => {
    // invokeSnapshot resolves to null on failure.
    expect(answerRecorded(null)).toBe(false);
    expect(answerRecorded(undefined)).toBe(false);
  });
});

describe("F39 — a schedule is cancellable only while something remains to cancel", () => {
  it("allows cancelling a pending schedule", () => {
    for (const status of ["Planned", "WaitingForPeer", "WaitingForGuest", "Transferring"]) {
      expect(canCancelSchedule(status)).toBe(true);
    }
  });

  it("refuses once it is already cancelled or finished", () => {
    expect(canCancelSchedule("Cancelled")).toBe(false);
    expect(canCancelSchedule("Completed")).toBe(false);
  });
});

describe("F10 — the provider control does not promise a logout", () => {
  it("describes the operation it actually performs", () => {
    const label = PROVIDER_RECHECK_LABEL.toLowerCase();
    // The action is a status re-check; no command clears a signed-in provider
    // profile, so the copy must not claim one.
    expect(label).not.toContain("log out");
    expect(label).not.toContain("logout");
    expect(label).not.toContain("reset");
    expect(label).toContain("re-check");
  });
});
