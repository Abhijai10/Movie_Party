import { describe, expect, it } from "vitest";
import {
  clearedJoinAttempt,
  createSingleFlight,
  isTailscaleReady,
  pollIntervalMs,
  refreshLabelFor,
  shouldShowPartnerConnectView,
  shouldShowTailscaleSetup,
  TAILSCALE_POLL_INTERVAL_READY_MS,
  TAILSCALE_POLL_INTERVAL_SETUP_MS,
} from "./tailscaleOnboarding";

describe("Partner Connect escape path (F4)", () => {
  it("gates the screen on MP-NET-TS-005 only", () => {
    expect(shouldShowPartnerConnectView("MP-NET-TS-005")).toBe(true);
    expect(shouldShowPartnerConnectView(null)).toBe(false);
    for (const other of [
      "MP-NET-TS-001",
      "MP-NET-TS-003",
      "MP-ROOM-001",
      "MP-NET-003",
    ]) {
      expect(shouldShowPartnerConnectView(other)).toBe(false);
    }
  });

  it("escaping clears the gate, so the screen cannot re-render", () => {
    // The screen replaces the whole app while this code is set; the escape
    // path clears it, which is what actually returns the user to Home.
    expect(shouldShowPartnerConnectView(clearedJoinAttempt.joinFailureCode)).toBe(
      false,
    );
  });

  it("clears every stale join value, not just the failure code", () => {
    // A surviving error message or pending invite would leak into the next
    // join attempt, so the reset must cover all three.
    expect(clearedJoinAttempt.joinError).toBeNull();
    expect(clearedJoinAttempt.joinFailureCode).toBeNull();
    expect(clearedJoinAttempt.pendingInvite).toBe("");
  });

  it("never bypasses Tailscale onboarding", () => {
    // Cancelling only resets the join attempt. Readiness still comes from the
    // live Tailscale state, so an unusable device keeps seeing the setup gate
    // instead of being waved through to Home.
    for (const state of [
      "NOT_INSTALLED",
      "DAEMON_UNAVAILABLE",
      "NEEDS_LOGIN",
      "STOPPED",
      "NO_USABLE_ADDRESS",
    ] as const) {
      expect(isTailscaleReady(state)).toBe(false);
      expect(shouldShowTailscaleSetup(state)).toBe(true);
    }
  });
});

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

describe("shouldShowTailscaleSetup", () => {
  it("shows the setup gate for every non-ready state", () => {
    for (const state of [
      "NOT_INSTALLED",
      "DAEMON_UNAVAILABLE",
      "NEEDS_LOGIN",
      "STOPPED",
      "NO_USABLE_ADDRESS",
    ] as const) {
      expect(shouldShowTailscaleSetup(state)).toBe(true);
    }
  });

  it("does not show the setup gate once READY (Home is allowed)", () => {
    expect(shouldShowTailscaleSetup("READY")).toBe(false);
  });
});

describe("shouldShowPartnerConnectView", () => {
  it("shows the partner surface only for an unreachable-host join failure", () => {
    expect(shouldShowPartnerConnectView("MP-NET-TS-005")).toBe(true);
    for (const code of [
      null,
      "MP-NET-TS-001",
      "MP-NET-TS-002",
      "MP-NET-TS-003",
      "MP-NET-TS-004",
      "MP-NET-TS-006",
      "MP-ROOM-001",
    ]) {
      expect(shouldShowPartnerConnectView(code)).toBe(false);
    }
  });

  it("never treats partner unreachability as a Home blocker", () => {
    // The onboarding gate keys off Tailscale local readiness alone; a join
    // failure code is handled at the join stage, not as a Home prerequisite.
    for (const code of ["MP-NET-TS-005", "MP-NET-TS-004", "MP-ROOM-001", null]) {
      expect(shouldShowTailscaleSetup("READY")).toBe(false);
      expect(shouldShowPartnerConnectView(code)).toBe(code === "MP-NET-TS-005");
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

describe("createSingleFlight", () => {
  it("never runs overlapping refresh requests", async () => {
    const run = createSingleFlight<string>();
    let executions = 0;
    const resolve: Array<() => void> = [];

    const first = run(async () => {
      executions += 1;
      await new Promise<void>((done) => resolve.push(done));
      return "first";
    });
    const second = run(() => {
      executions += 1;
      return Promise.resolve("second");
    });

    expect(await second).toBeUndefined();
    expect(executions).toBe(1);

    resolve[0]?.();
    expect(await first).toBe("first");
    expect(executions).toBe(1);
  });

  it("allows a fresh request after the previous one finishes", async () => {
    const run = createSingleFlight<number>();
    let executions = 0;

    await run(() => {
      executions += 1;
      return Promise.resolve(1);
    });
    const next = await run(() => {
      executions += 1;
      return Promise.resolve(42);
    });

    expect(next).toBe(42);
    expect(executions).toBe(2);
  });
});
