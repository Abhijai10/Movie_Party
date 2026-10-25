import { describe, expect, it } from "vitest";
import {
  ACTION_CARDS,
  VERDICT_LABEL,
  actionCard,
  capabilityLine,
  matrixSummary,
  revocationLine,
  revocationTone,
  sharePlanBlocker,
  sharePlanLines,
} from "./access";
import type {
  AccessMatrixView,
  CapabilityRow,
  RevocationView,
  SharePlanView,
} from "../backend/appRuntime";

/**
 * Batch 10's contract in tests: the surface may describe Tailscale's console,
 * never impersonate it; and the three removals stay three different things.
 */

function row(overrides: Partial<CapabilityRow> = {}): CapabilityRow {
  return {
    operation: "CREATE_DEVICE_SHARE",
    verdict: "CONSOLE_ONLY",
    requiredRole: "Owner, Admin or IT admin",
    scope: null,
    evidenceQuote: "A shared machine can receive incoming connections but cannot start connections.",
    evidenceUrl: "https://tailscale.com/docs/features/sharing",
    productResponse: "The wizard opens the Tailscale admin page.",
    ...overrides,
  };
}

function matrix(overrides: Partial<AccessMatrixView> = {}): AccessMatrixView {
  return {
    rows: [row()],
    anyAutomatable: false,
    sharedWithExternalUserNote: "The recipient must sign in and accept.",
    ...overrides,
  };
}

function plan(overrides: Partial<SharePlanView> = {}): SharePlanView {
  return {
    plan: {
      deviceName: "abhijai-mac.tailc930b7.ts.net",
      tailnetSuffix: "tailc930b7.ts.net",
      recipient: "priya@example.com",
      lifetimeDays: 7,
      role: "ALLOWED",
      consoleUrl: "https://login.tailscale.com/admin/machines",
    },
    roleAllowed: true,
    roleUnknown: false,
    confirmation:
      "Share only this machine (abhijai-mac.tailc930b7.ts.net) with priya@example.com for 7 days.",
    notPerformed:
      "Movie Party cannot create or revoke a Tailscale share: the documented API has no sharing endpoint or scope.",
    ...overrides,
  };
}

function revocation(overrides: Partial<RevocationView> = {}): RevocationView {
  return {
    peerKey: "priya-mac.tailc930b7.ts.net.",
    state: "UNKNOWN",
    copy: "Movie Party never saw this machine as a share from here, so it cannot confirm a revocation either way.",
    consoleUrl: "https://login.tailscale.com/admin/machines",
    ...overrides,
  };
}

describe("capability matrix copy", () => {
  it("says plainly that nothing is automated today", () => {
    expect(matrixSummary(matrix())).toContain("None of these actions can be automated safely today");
    expect(matrixSummary(matrix())).toContain("console");
  });

  it("changes its wording only when something actually qualifies", () => {
    expect(matrixSummary(matrix({ anyAutomatable: true }))).toContain("Some actions can be automated");
  });

  it("labels every verdict distinctly", () => {
    const labels = Object.values(VERDICT_LABEL);
    expect(new Set(labels).size).toBe(4);
    expect(VERDICT_LABEL.FORBIDDEN_BY_POLICY).toContain("refused");
  });

  it("states the scope when one exists and says so when none is documented", () => {
    expect(capabilityLine(row({ scope: "auth_keys" }))).toContain("Scope: auth_keys");
    expect(capabilityLine(row({ scope: null }))).toContain("No scope is documented");
  });
});

describe("requirement 13: three removals, never one", () => {
  it("keeps three distinct actions with distinct effects", () => {
    expect(ACTION_CARDS).toHaveLength(3);
    const ids = new Set(ACTION_CARDS.map((card) => card.id));
    const titles = new Set(ACTION_CARDS.map((card) => card.title));
    const effects = new Set(ACTION_CARDS.map((card) => card.effect));
    expect(ids.size).toBe(3);
    expect(titles.size).toBe(3);
    expect(effects.size).toBe(3);
  });

  it("only the local removal is performed by Movie Party", () => {
    expect(actionCard("REMOVE_MOVIE_PARTY_FRIEND").needsConsole).toBe(false);
    expect(actionCard("REVOKE_TAILSCALE_DEVICE_SHARE").needsConsole).toBe(true);
    expect(actionCard("REMOVE_TAILNET_MEMBER").needsConsole).toBe(true);
    expect(actionCard("REVOKE_TAILSCALE_DEVICE_SHARE").performedBy).toBe(
      "Tailscale admin console",
    );
  });

  it("says what each one does not do", () => {
    expect(actionCard("REMOVE_MOVIE_PARTY_FRIEND").notThis).toContain(
      "does not revoke a Tailscale share",
    );
    expect(actionCard("REMOVE_MOVIE_PARTY_FRIEND").effect).toContain("Tailscale access keeps working");
    expect(actionCard("REVOKE_TAILSCALE_DEVICE_SHARE").notThis).toContain(
      "does not remove them from the tailnet",
    );
    expect(actionCard("REMOVE_TAILNET_MEMBER").notThis).toContain("more than revoking a share");
    for (const card of ACTION_CARDS) {
      expect(card.confirmTitle.endsWith("?")).toBe(true);
      expect(card.confirmBody.length).toBeGreaterThan(40);
    }
  });

  it("refuses an id that is not one of the three", () => {
    expect(() => actionCard("DELETE_TAILNET" as never)).toThrow(/Unknown access action/);
  });
});

describe("requirements 8-10: the share plan", () => {
  it("states this machine, one recipient and a bounded lifetime", () => {
    const lines = sharePlanLines(plan());
    expect(lines[0]).toContain("this device only");
    expect(lines[0]).toContain("abhijai-mac.tailc930b7.ts.net");
    expect(lines[1]).toBe("Recipient: priya@example.com");
    expect(lines[2]).toBe("Lifetime: 7 days");
    expect(lines.join(" ")).toContain("cannot create or revoke");
  });

  it("blocks on a role that cannot share, and on a role it cannot read", () => {
    expect(sharePlanBlocker(plan({ plan: { ...plan().plan, role: "NOT_ALLOWED" }, roleAllowed: false }))).toContain(
      "not an Owner, Admin or IT admin",
    );
    expect(
      sharePlanBlocker(plan({ plan: { ...plan().plan, role: "UNKNOWN" }, roleAllowed: false, roleUnknown: true })),
    ).toContain("did not report your role");
    expect(sharePlanBlocker(plan())).toBeNull();
  });
});

describe("requirement 14: revocation state", () => {
  it("confirms only what was observed, and points at the console otherwise", () => {
    expect(
      revocationTone(
        revocation({
          state: "NO_LONGER_VISIBLE",
          copy: "The shared machine is no longer visible. Confirm it on Tailscale's Machines page.",
        }),
      ),
    ).toBe("ok");
    expect(revocationTone(revocation({ state: "STILL_SHARED" }))).toBe("wait");
    expect(revocationTone(revocation())).toBe("off");
    expect(revocationLine(revocation())).toContain("cannot confirm");
  });
});
