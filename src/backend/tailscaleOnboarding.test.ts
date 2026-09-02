import { describe, expect, it } from "vitest";
import {
  isTailscaleReady,
  pollIntervalMs,
  refreshLabelFor,
  TAILSCALE_POLL_INTERVAL_READY_MS,
  TAILSCALE_POLL_INTERVAL_SETUP_MS,
} from "./tailscaleOnboarding";

describe("isTailscaleReady", () => {
  it("returns true only for READY", () => {
    expect(isTailscaleReady("READY")).toBe(true);
  });

  it("returns false for every non-ready state", () => {
    for (const state of [
      "NOT_INSTALLED",
      "DAEMON_UNAVAILABLE",
      "NEEDS_LOGIN",
      "STOPPED",
      "NO_USABLE_ADDRESS",
    ] as const) {
      expect(isTailscaleReady(state)).toBe(false);
    }
  });
});

describe("pollIntervalMs", () => {
  it("returns a slower cadence when ready", () => {
    expect(pollIntervalMs(true)).toBe(TAILSCALE_POLL_INTERVAL_READY_MS);
    expect(pollIntervalMs(false)).toBe(TAILSCALE_POLL_INTERVAL_SETUP_MS);
    expect(TAILSCALE_POLL_INTERVAL_READY_MS).toBeGreaterThan(
      TAILSCALE_POLL_INTERVAL_SETUP_MS,
    );
  });
});

describe("refreshLabelFor", () => {
  it('returns "I\'ve signed in" for NEEDS_LOGIN', () => {
    expect(refreshLabelFor("NEEDS_LOGIN")).toBe("I've signed in");
  });

  it('returns "Check again" for every other non-ready state', () => {
    for (const state of [
      "NOT_INSTALLED",
      "DAEMON_UNAVAILABLE",
      "STOPPED",
      "NO_USABLE_ADDRESS",
    ] as const) {
      expect(refreshLabelFor(state)).toBe("Check again");
    }
  });

  it("returns a non-empty string for every state", () => {
    for (const state of [
      "NOT_INSTALLED",
      "DAEMON_UNAVAILABLE",
      "NEEDS_LOGIN",
      "STOPPED",
      "NO_USABLE_ADDRESS",
      "READY",
    ] as const) {
      expect(refreshLabelFor(state).length).toBeGreaterThan(0);
    }
  });
});