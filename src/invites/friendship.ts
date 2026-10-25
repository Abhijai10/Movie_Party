import type { FriendshipState, IncomingInvite } from "../backend/appRuntime";

/**
 * The words and available actions for each friendship position.
 *
 * Two things are deliberately kept apart here. Trust is what Movie Party believes
 * about a device identity; connection is whether that device is reachable right
 * now. A friend who is offline is still a friend, and a peer that answers the
 * tunnel is not automatically one.
 */

export const FRIENDSHIP_LABELS: Record<FriendshipState, string> = {
  NOT_FRIENDS: "Not friends",
  INVITED: "Invite sent · waiting for them to accept",
  PENDING_ACCEPTANCE: "Invited you · needs your decision",
  ACCEPTED: "Friends",
  BLOCKED: "Blocked",
  REMOVED: "Removed",
  CONNECTION_UNAVAILABLE: "Friends · not reachable right now",
};

export type FriendAction = "connect" | "reinvite" | "remove" | "block" | "unblock";

export function friendshipActions(state: FriendshipState | undefined): FriendAction[] {
  switch (state ?? "NOT_FRIENDS") {
    case "BLOCKED":
      return ["unblock"];
    case "REMOVED":
      // A removed device may be invited again, but is not silently re-trusted.
      return ["reinvite"];
    case "ACCEPTED":
    case "CONNECTION_UNAVAILABLE":
      return ["connect", "remove", "block"];
    default:
      return ["connect", "remove", "block"];
  }
}

/** Destructive actions get a confirmation step; nothing else should steal one. */
export function isDestructive(action: FriendAction): boolean {
  return action === "remove" || action === "block";
}

export function actionLabel(action: FriendAction): string {
  switch (action) {
    case "connect":
      return "Connect";
    case "reinvite":
      return "Invite again";
    case "remove":
      return "Remove";
    case "block":
      return "Block";
    case "unblock":
      return "Unblock";
  }
}

export function removalWarning(name: string): string {
  return `Removing ${name} revokes their Movie Party trust on this device. It does not remove them from Tailscale — do that in the Tailscale app.`;
}

export function blockWarning(name: string): string {
  return `Blocking ${name} also stops their existing invite links from being accepted again.`;
}

/**
 * A room invitation and a friendship are different offers. Copy must not let the
 * user read one as the other: a one-off guest seat is not a standing friendship,
 * and a friend is not automatically allowed into a room that never invited them.
 */
export function friendVersusGuestNote(hasRoomInvite: boolean): string {
  return hasRoomInvite
    ? "This join link opens the current party for one session. It does not make anyone a friend."
    : "Friends are remembered. Starting a party still needs a separate join link for that session.";
}

export function fingerprintHelp(fingerprint: string): string {
  return `Read "${fingerprint}" to each other over a channel you already trust. If both screens show it, this is the same device.`;
}

/** Only a decision taken inside the window counts as still open. */
export function inviteExpiryCopy(expiresAtMs: number, nowMs: number): string {
  const remainingMs = expiresAtMs - nowMs;
  if (remainingMs <= 0) return "This invitation has expired. Ask for a new one.";
  const minutes = Math.floor(remainingMs / 60_000);
  if (minutes < 60) return `Expires in about ${String(Math.max(1, minutes))} min.`;
  const days = Math.floor(minutes / (60 * 24));
  return days >= 1
    ? `Expires in about ${String(days)} day(s).`
    : `Expires in about ${String(Math.floor(minutes / 60))} h.`;
}

/** What the confirmation card says before the user decides. */
export function invitePrompt(invite: IncomingInvite): string {
  const who = invite.displayName || "A device";
  const base = `${who} wants to add this device as a friend.`;
  switch (invite.knownState) {
    case "ACCEPTED":
      return `You already added ${who}. Accepting again changes nothing.`;
    case "BLOCKED":
      return `${base} You blocked it earlier.`;
    case "REMOVED":
      return `${base} You removed this device before, and its identity key matches.`;
    default:
      return base;
  }
}
