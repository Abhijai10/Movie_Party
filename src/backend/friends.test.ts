import { describe, expect, it } from "vitest";
import {
  friendErrorCopy,
  friendStatusCopy,
  friendStatusFor,
  summarizePath,
  type FriendCandidate,
  type PeerConnectionProbe,
  type StoredFriend,
  type TailnetPeersView,
} from "./appRuntime";

/** A saved friend factory with sane defaults for tests. */
function savedFriend(overrides: Partial<StoredFriend> = {}): StoredFriend {
  return {
    peerKey: "rahul-mac.tailc930b7.ts.net.",
    displayName: "rahul-mac",
    ip: "100.64.0.42",
    addedAtMs: 1_000,
    lastVerifiedAtMs: null,
    lastPath: null,
    lastLatencyMs: null,
    ...overrides,
  };
}

/** A candidate factory with sane defaults. */
function candidate(overrides: Partial<FriendCandidate> = {}): FriendCandidate {
  return {
    peerKey: "rahul-mac.tailc930b7.ts.net.",
    displayName: "rahul-mac",
    ip: "100.64.0.42",
    online: true,
    path: "direct",
    ...overrides,
  };
}

describe("friendStatusFor", () => {
  it("marks a verified friend as connected", () => {
    const friend = savedFriend({ lastVerifiedAtMs: Date.now() - 60_000 });
    expect(friendStatusFor(friend, false)).toBe("connected");
  });

  it("falls back to the tailnet online flag when unverified", () => {
    const friend = savedFriend();
    expect(friendStatusFor(friend, true)).toBe("online");
    expect(friendStatusFor(friend, false)).toBe("offline");
  });

  it("treats an unknown online state as offline (honest default)", () => {
    const friend = savedFriend();
    expect(friendStatusFor(friend, undefined)).toBe("offline");
  });

  it("prefers verification over the online flag — verified stays connected", () => {
    // A stale-but-real verification beats "the peer table says online":
    // the tunnel was PROVEN, not inferred.
    const friend = savedFriend({ lastVerifiedAtMs: 1 });
    expect(friendStatusFor(friend, true)).toBe("connected");
  });
});

describe("friendStatusCopy", () => {
  it("describes a verified friend with path and latency", () => {
    const friend = savedFriend({ lastVerifiedAtMs: 9, lastPath: "direct", lastLatencyMs: 23 });
    expect(friendStatusCopy(friend, "connected")).toContain("Connection verified");
    expect(friendStatusCopy(friend, "connected")).toContain("direct");
    expect(friendStatusCopy(friend, "connected")).toContain("23 ms");
  });

  it("never claims a connection for an unverified friend", () => {
    const friend = savedFriend();
    expect(friendStatusCopy(friend, "online")).toBe("Online — not verified yet");
    expect(friendStatusCopy(friend, "offline")).toBe("Offline");
    // The honest distinction: only a REAL verified probe says "Connection
    // verified"; the online flag never upgrades to that claim.
    expect(friendStatusCopy(friend, "online")).not.toContain("Connection verified");
  });

  it("omits the path segment when the probe had none", () => {
    const friend = savedFriend({ lastVerifiedAtMs: 9, lastPath: null, lastLatencyMs: null });
    expect(friendStatusCopy(friend, "connected")).toBe("Connection verified");
  });
});

describe("summarizePath", () => {
  it("extracts the leading path segment", () => {
    expect(summarizePath("direct/ipv4 192.168.1.5:41641")).toBe("direct");
    expect(summarizePath('relay "derp-3"')).toBe("relay");
  });

  it("normalizes relay-late to relay", () => {
    expect(summarizePath("relay-late 100 ms")).toBe("relay");
  });

  it("passes through simple labels", () => {
    expect(summarizePath("direct")).toBe("direct");
  });
});

describe("friendErrorCopy", () => {
  it("maps each stable MP-NET-TS friend code to actionable copy", () => {
    const cases: Array<[string, RegExp]> = [
      ["MP-NET-TS-001 not found", /isn't installed/],
      ["MP-NET-TS-003 status failed", /isn't responding/],
      ["MP-NET-TS-004 no usable address", /no usable Tailscale address/],
      ["MP-NET-TS-007 no answer", /No answer through the tunnel/],
      ["MP-NET-TS-008 not in network", /isn't in your Tailscale network/],
      ["MP-STORE-001 db locked", /couldn't save/],
    ];
    for (const [detail, pattern] of cases) {
      expect(friendErrorCopy(detail, "fallback")).toMatch(pattern);
    }
  });

  it("returns the fallback for unknown details", () => {
    expect(friendErrorCopy("something odd", "Movie Party hiccup.")).toBe("Movie Party hiccup.");
  });

  it("handles Error objects and non-strings", () => {
    expect(friendErrorCopy(new Error("MP-NET-TS-007 x"), "f")).toMatch(/No answer/);
    expect(friendErrorCopy(42, "f")).toBe("f");
  });

  it("never leaks raw backend paths", () => {
    const copy = friendErrorCopy(
      "MP-NET-TS-003 failed: /Users/me/secret/path detail",
      "fallback",
    );
    expect(copy).not.toContain("/Users/me/secret");
  });
});

describe("friend probe shapes", () => {
  it("a reachable probe carries path + latency", () => {
    const probe: PeerConnectionProbe = {
      reachable: true,
      path: "direct",
      latencyMs: 12,
      message: "pong from x via direct in 12ms",
    };
    expect(probe.reachable).toBe(true);
    expect(probe.latencyMs).toBe(12);
  });

  it("the peers view carries candidates and saved friends", () => {
    const view: TailnetPeersView = {
      candidates: [candidate()],
      saved: [savedFriend()],
    };
    expect(view.candidates[0]?.displayName).toBe("rahul-mac");
    expect(view.saved[0]?.peerKey).toBe("rahul-mac.tailc930b7.ts.net.");
  });

  it("candidates may be offline with an unknown path", () => {
    const offline = candidate({ online: false, path: "offline", ip: null });
    expect(offline.online).toBe(false);
    expect(offline.ip).toBeNull();
  });
});
