import { describe, expect, it } from "vitest";
import {
  friendErrorCopy,
  friendStatusCopy,
  friendStatusFor,
  summarizePath,
  type FriendCandidate,
  type FriendConnectionState,
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
    connectionState: "TAILSCALE_JOINED",
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

describe("friendStatusFor (friend-flow states)", () => {
  it("marks a ping-verified friend as MOVIE_PARTY_VERIFIED", () => {
    const friend = savedFriend({ lastVerifiedAtMs: Date.now() - 60_000 });
    expect(friendStatusFor(friend, false)).toBe("MOVIE_PARTY_VERIFIED");
  });

  it("shows an INVITED friend as TAILSCALE_PENDING regardless of the online flag", () => {
    // INVITED means the friend accepted the link but their device has
    // not been observed in this tailnet yet — even a stray "online"
    // reading must not upgrade that to a join claim.
    const friend = savedFriend({ connectionState: "INVITED" });
    expect(friendStatusFor(friend, true)).toBe("TAILSCALE_PENDING");
    expect(friendStatusFor(friend, false)).toBe("TAILSCALE_PENDING");
    expect(friendStatusFor(friend, undefined)).toBe("TAILSCALE_PENDING");
  });

  it("derives ONLINE/OFFLINE for a TAILSCALE_JOINED friend from the live flag", () => {
    const friend = savedFriend({ connectionState: "TAILSCALE_JOINED" });
    expect(friendStatusFor(friend, true)).toBe("ONLINE");
    expect(friendStatusFor(friend, false)).toBe("OFFLINE");
  });

  it("treats an unknown online state as offline (honest default)", () => {
    const friend = savedFriend();
    expect(friendStatusFor(friend, undefined)).toBe("OFFLINE");
  });

  it("prefers verification over the online flag — verified stays verified", () => {
    // A stale-but-real verification beats "the peer table says online":
    // the tunnel was PROVEN, not inferred.
    const friend = savedFriend({ lastVerifiedAtMs: 1 });
    expect(friendStatusFor(friend, true)).toBe("MOVIE_PARTY_VERIFIED");
  });
});

describe("friendStatusCopy", () => {
  it("describes a verified friend with path and latency", () => {
    const friend = savedFriend({ lastVerifiedAtMs: 9, lastPath: "direct", lastLatencyMs: 23 });
    expect(friendStatusCopy(friend, "MOVIE_PARTY_VERIFIED")).toContain("Connection verified");
    expect(friendStatusCopy(friend, "MOVIE_PARTY_VERIFIED")).toContain("direct");
    expect(friendStatusCopy(friend, "MOVIE_PARTY_VERIFIED")).toContain("23 ms");
  });

  it("explains the pending state for an INVITED friend honestly", () => {
    const friend = savedFriend({ connectionState: "INVITED" });
    const copy = friendStatusCopy(friend, "TAILSCALE_PENDING");
    expect(copy).toContain("waiting for their device to join");
    // The pending copy never claims a connection exists.
    expect(copy).not.toContain("Connection verified");
    expect(copy).not.toContain("Online");
  });

  it("never claims a connection for an unverified friend", () => {
    const friend = savedFriend();
    expect(friendStatusCopy(friend, "ONLINE")).toBe("Online — not verified yet");
    expect(friendStatusCopy(friend, "OFFLINE")).toBe("Offline");
    // The honest distinction: only a REAL verified probe says "Connection
    // verified"; the online flag never upgrades to that claim.
    expect(friendStatusCopy(friend, "ONLINE")).not.toContain("Connection verified");
  });

  it("omits the path segment when the probe had none", () => {
    const friend = savedFriend({ lastVerifiedAtMs: 9, lastPath: null, lastLatencyMs: null });
    expect(friendStatusCopy(friend, "MOVIE_PARTY_VERIFIED")).toBe("Connection verified");
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
      ["MP-FRIEND-001 bad invite", /link|invite/i],
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

  it("persisted friend states enumerate the invitation flow", () => {
    const states: FriendConnectionState[] = [
      "INVITED",
      "TAILSCALE_JOINED",
      "MOVIE_PARTY_VERIFIED",
    ];
    // A friend saved straight from a link starts at INVITED; the tailnet
    // observation promotes to TAILSCALE_JOINED; only a real ping marks
    // MOVIE_PARTY_VERIFIED. ONLINE/OFFLINE is live, never persisted.
    for (const state of states) {
      const friend = savedFriend({ connectionState: state });
      expect(friend.connectionState).toBe(state);
    }
  });
});
