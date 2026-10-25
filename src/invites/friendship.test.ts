import { describe, expect, it } from "vitest";
import type { FriendshipState, IncomingInvite } from "../backend/appRuntime";
import {
  FRIENDSHIP_LABELS,
  actionLabel,
  blockWarning,
  fingerprintHelp,
  friendVersusGuestNote,
  inviteExpiryCopy,
  invitePrompt,
  isDestructive,
  removalWarning,
} from "./friendship";

const ALL: FriendshipState[] = [
  "NOT_FRIENDS",
  "INVITED",
  "PENDING_ACCEPTANCE",
  "ACCEPTED",
  "BLOCKED",
  "REMOVED",
  "CONNECTION_UNAVAILABLE",
];

const invite = (over: Partial<IncomingInvite> = {}): IncomingInvite => ({
  nonce: "abc",
  displayName: "Priya",
  peerKey: "priya-mac.tailc930b7.ts.net.",
  fingerprint: "MP-1A2B-3C4D-5E6F-7A8B",
  expiresAtMs: 10_000,
  knownState: null,
  warning: null,
  ...over,
});

describe("friendship lifecycle copy", () => {
  it("names every state in its own words", () => {
    const labels = ALL.map((state) => FRIENDSHIP_LABELS[state]);
    expect(new Set(labels).size).toBe(ALL.length);
    expect(labels.every((label) => label.length > 3)).toBe(true);
  });

  it("keeps trust separate from reachability", () => {
    expect(FRIENDSHIP_LABELS.CONNECTION_UNAVAILABLE).toContain("Friends");
    expect(FRIENDSHIP_LABELS.CONNECTION_UNAVAILABLE).toContain("not reachable");
    expect(FRIENDSHIP_LABELS.REMOVED).not.toContain("offline");
  });

  it("offers only reversible actions to a blocked or removed device", async () => {
    const { friendshipActions } = await import("./friendship");
    expect(friendshipActions("BLOCKED")).toEqual(["unblock"]);
    expect(friendshipActions("REMOVED")).toEqual(["reinvite"]);
    expect(friendshipActions("ACCEPTED")).toContain("connect");
    // An absent position is treated as not friends, never as trusted.
    expect(friendshipActions(undefined)).toEqual(friendshipActions("NOT_FRIENDS"));
    expect(friendshipActions("PENDING_ACCEPTANCE")).not.toContain("reinvite");
  });

  it("asks for confirmation exactly where the action is hard to undo", async () => {
    const { friendshipActions } = await import("./friendship");
    const destructive = new Set(["remove", "block"]);
    for (const state of ALL) {
      for (const action of friendshipActions(state)) {
        expect(isDestructive(action)).toBe(destructive.has(action));
        expect(actionLabel(action).length).toBeGreaterThan(1);
      }
    }
  });

  it("states plainly what removal does and does not do", () => {
    const text = removalWarning("Priya");
    expect(text).toContain("revokes their Movie Party trust");
    expect(text).toContain("does not remove them from Tailscale");
    expect(blockWarning("Priya")).toContain("invite links");
  });

  it("keeps a room guest and a friend from being read as the same thing", () => {
    const guest = friendVersusGuestNote(true);
    const friend = friendVersusGuestNote(false);
    expect(guest).toContain("one session");
    expect(guest).toContain("does not make anyone a friend");
    expect(friend).toContain("separate join link");
    expect(guest).not.toBe(friend);
  });

  it("explains the fingerprint as something read aloud, not clicked through", () => {
    expect(fingerprintHelp("MP-1A2B-3C4D-5E6F-7A8B")).toContain("MP-1A2B-3C4D-5E6F-7A8B");
    expect(fingerprintHelp("MP-1A2B-3C4D-5E6F-7A8B")).toContain("same device");
  });

  it("counts an invitation down without ever offering a dead decision", () => {
    const DAY = 86_400_000;
    expect(inviteExpiryCopy(10_000, 10_000)).toContain("expired");
    expect(inviteExpiryCopy(10_000, 10_001)).toContain("expired");
    expect(inviteExpiryCopy(10_000, 9_000)).toContain("min");
    expect(inviteExpiryCopy(30 * 60_000, 0)).toContain("min");
    expect(inviteExpiryCopy(5 * 3_600_000, 0)).toContain("h");
    expect(inviteExpiryCopy(7 * DAY, 0)).toContain("day");
  });

  it("warns on the card when we already hold a position on that identity", () => {
    expect(invitePrompt(invite())).toBe("Priya wants to add this device as a friend.");
    expect(invitePrompt(invite({ knownState: "ACCEPTED" }))).toContain("already added");
    expect(invitePrompt(invite({ knownState: "BLOCKED" }))).toContain("blocked");
    expect(invitePrompt(invite({ knownState: "REMOVED" })).toLowerCase()).toContain("identity key");
    // A blank name must not produce an empty or misleading sentence.
    expect(invitePrompt(invite({ displayName: "" }))).toContain("A device");
  });
});
