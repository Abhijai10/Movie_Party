import { describe, expect, it } from "vitest";
import { buildFriendInviteLink, parseFriendInvite } from "./deepLinks";

const PEER_KEY = "rahul-mac.tailc930b7.ts.net.";

/** btoa but base64url (matches the wire format the app produces). */
function b64url(value: string): string {
  return btoa(value).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

describe("friend invite round-trip", () => {
  it("builds and parses a friend invite", () => {
    const link = buildFriendInviteLink(PEER_KEY, "Rahul");
    expect(link.startsWith("movieparty://friend/")).toBe(true);

    const parsed = parseFriendInvite(link);
    expect(parsed.ok).toBe(true);
    if (parsed.ok) {
      expect(parsed.peerKey).toBe(PEER_KEY);
      expect(parsed.displayName).toBe("Rahul");
      expect(parsed.invite).toBe(link);
    }
  });

  it("survives non-ASCII names (UTF-8 payloads)", () => {
    const link = buildFriendInviteLink(PEER_KEY, "Joaquín");
    const parsed = parseFriendInvite(link);
    expect(parsed.ok).toBe(true);
    if (parsed.ok) {
      expect(parsed.displayName).toBe("Joaquín");
    }
  });

  it("survives surrounding whitespace", () => {
    const link = buildFriendInviteLink(PEER_KEY, "Rahul");
    const parsed = parseFriendInvite(`  ${link}  `);
    expect(parsed.ok).toBe(true);
  });

  it("rejects join-party links", () => {
    const parsed = parseFriendInvite("movieparty://join/abc#def");
    expect(parsed.ok).toBe(false);
    if (!parsed.ok) {
      expect(parsed.message).toContain("friend");
    }
  });

  it("rejects empty and wrong-scheme inputs", () => {
    expect(parseFriendInvite("").ok).toBe(false);
    expect(parseFriendInvite("https://example.com").ok).toBe(false);
    expect(parseFriendInvite("movieparty://friend/").ok).toBe(false);
  });

  it("rejects garbage codes", () => {
    expect(parseFriendInvite("movieparty://friend/###").ok).toBe(false);
    // Valid base64url, but the payload is not our JSON shape.
    expect(parseFriendInvite("movieparty://friend/aGVsbG8").ok).toBe(false);
  });

  it("rejects payloads missing fields", () => {
    const payload = b64url(JSON.stringify({ pk: PEER_KEY }));
    const parsed = parseFriendInvite(`movieparty://friend/${payload}`);
    expect(parsed.ok).toBe(false);
  });

  it("rejects invalid device identities", () => {
    const payload = b64url(JSON.stringify({ pk: "not-a-domain", n: "R" }));
    expect(parseFriendInvite(`movieparty://friend/${payload}`).ok).toBe(false);
  });

  it("rejects oversized invites", () => {
    const parsed = parseFriendInvite(`movieparty://friend/${"A".repeat(2000)}`);
    expect(parsed.ok).toBe(false);
  });
});
