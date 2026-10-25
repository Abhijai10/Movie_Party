/**
 * Batch 10 — the access surface's copy, kept out of the component so it can be
 * tested and so no screen can invent a capability.
 *
 * Two rules are encoded here:
 *  - a capability the documented matrix does not allow is rendered as a
 *    *console step with an explanation*, never as a button that pretends;
 *  - the three removals are separate cards with separate confirmations, because
 *    removing a friend, revoking a share and removing a member differ in blast
 *    radius by orders of magnitude.
 */
import type {
  AccessMatrixView,
  AutomationVerdict,
  CapabilityRow,
  DestructiveActionId,
  RevocationView,
  SharePlanView,
} from "../backend/appRuntime";

export const VERDICT_LABEL: Record<AutomationVerdict, string> = {
  AUTOMATABLE: "Movie Party can do this",
  DOCUMENTED_BUT_UNSCOPED: "Console only — no narrow API scope is documented",
  CONSOLE_ONLY: "Console only — Tailscale documents no API for this",
  FORBIDDEN_BY_POLICY: "Not offered — refused by Movie Party's rules",
};

export function capabilityLine(row: CapabilityRow): string {
  const scope = row.scope ? `Scope: ${row.scope}.` : "No scope is documented.";
  return `${VERDICT_LABEL[row.verdict]} ${scope} ${row.productResponse}`;
}

/** Requirement 2 in one line: the matrix never claims more than it can cite. */
export function matrixSummary(view: AccessMatrixView): string {
  if (view.anyAutomatable) {
    return "Some actions can be automated with a documented, narrow scope. Each one still asks for confirmation.";
  }
  return "None of these actions can be automated safely today: Tailscale documents no sharing endpoint, and the invite endpoints have no published narrow scope. Every action below happens in Tailscale's own console.";
}

export type ActionCard = {
  id: DestructiveActionId;
  title: string;
  effect: string;
  performedBy: string;
  needsConsole: boolean;
  confirmTitle: string;
  confirmBody: string;
  /** The action this one is most often mistaken for. */
  notThis: string;
};

export const ACTION_CARDS: ActionCard[] = [
  {
    id: "REMOVE_MOVIE_PARTY_FRIEND",
    title: "Remove Movie Party friend",
    effect:
      "Movie Party stops trusting this identity. Tailscale access keeps working, and the two devices can still reach each other.",
    performedBy: "Movie Party, on this device",
    needsConsole: false,
    confirmTitle: "Remove this friend from Movie Party?",
    confirmBody:
      "Their identity stays blocked from re-adding until you unblock it. This does not touch Tailscale.",
    notThis: "It does not revoke a Tailscale share and does not remove anyone from the tailnet.",
  },
  {
    id: "REVOKE_TAILSCALE_DEVICE_SHARE",
    title: "Revoke Tailscale device share",
    effect:
      "That one machine stops accepting connections from them. Other devices on your tailnet are unaffected.",
    performedBy: "Tailscale admin console",
    needsConsole: true,
    confirmTitle: "Revoke the share in Tailscale?",
    confirmBody:
      "Only an Owner, Admin or IT admin of this tailnet can revoke it. Movie Party opens the Machines page and then reports whether the shared machine still appears.",
    notThis: "It does not remove them from the tailnet and does not change Movie Party friendship.",
  },
  {
    id: "REMOVE_TAILNET_MEMBER",
    title: "Remove tailnet member",
    effect:
      "Their account leaves the tailnet and loses access to every device in it — not only this app.",
    performedBy: "Tailscale admin console",
    needsConsole: true,
    confirmTitle: "Remove this person from the tailnet?",
    confirmBody:
      "This is the broadest of the three actions. It affects every shared and owned device in the tailnet, and it cannot be undone from Movie Party.",
    notThis: "It does more than revoking a share, and more than removing a Movie Party friend.",
  },
];

export function actionCard(id: DestructiveActionId): ActionCard {
  const found = ACTION_CARDS.find((card) => card.id === id);
  if (!found) {
    // The id set is closed; this is unreachable in the product and exists so the
    // type system cannot be bypassed silently.
    throw new Error(`Unknown access action: ${id}`);
  }
  return found;
}

/** Requirement 10: the sentence that must be read before a console write. */
export function sharePlanLines(view: SharePlanView): string[] {
  return [
    `Machine: ${view.plan.deviceName} (this device only)`,
    `Recipient: ${view.plan.recipient}`,
    `Lifetime: ${String(view.plan.lifetimeDays)} days`,
    view.confirmation,
    view.notPerformed,
  ];
}

export function sharePlanBlocker(view: SharePlanView): string | null {
  if (view.plan.role === "NOT_ALLOWED") {
    return "Your Tailscale login is not an Owner, Admin or IT admin of this tailnet, so it cannot share a machine. Ask whoever administers the tailnet.";
  }
  if (view.roleUnknown) {
    return "Tailscale did not report your role on this tailnet. Check the admin console rather than assuming either way.";
  }
  return null;
}

/** Requirement 14: say what can be confirmed, and where the rest is confirmed. */
export function revocationLine(view: RevocationView): string {
  return view.copy;
}

export function revocationTone(view: RevocationView): "ok" | "wait" | "off" {
  if (view.state === "NO_LONGER_VISIBLE") return "ok";
  if (view.state === "STILL_SHARED") return "wait";
  return "off";
}
