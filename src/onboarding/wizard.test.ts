import { describe, expect, it } from "vitest";
import {
  SHARE_LINK_APPLY_NOTE,
  STAGE_STEP_COPY,
  STAGE_TITLES,
  WIZARD_STAGES,
  checkRowLabel,
  checkRowTone,
  connectednessCopy,
  directionCopy,
  flowRows,
  handshakeEvidenceLine,
  headlineStatus,
  issueRow,
  keyExpiryCopy,
  methodCard,
  roleCopy,
  shareLinkSafetyCopy,
  sideWord,
  stageActions,
  stagePosition,
  topologyCopy,
} from "./wizard";
import type {
  CheckId,
  DirectionEvidence,
  MethodFacts,
  OnboardingMethod,
  OnboardingView,
  PeerConnectionProbe,
  StoredFriend,
  WizardCheck,
  WizardIssue,
  WizardStage,
} from "../backend/appRuntime";

/**
 * Requirement 18: every onboarding state has a test. These are the *view-model*
 * tests — the decisions themselves are tested in Rust
 * (`src-tauri/src/onboarding/mod.rs`), and the runtime boundary in
 * `src-tauri/src/app_runtime/onboarding.rs`. What is checked here is that the UI
 * cannot overstate what the backend proved.
 */

function check(
  id: CheckId,
  state: WizardCheck["state"] = "WAITING",
  code: string | null = null,
): WizardCheck {
  return { id, state, code };
}

function issue(overrides: Partial<WizardIssue> = {}): WizardIssue {
  return {
    id: "SHARE_PENDING",
    code: "MP-ONB-008",
    headline: "Waiting for them to accept in Tailscale",
    guidance: "They need to open the Tailscale link and accept it.",
    blocking: false,
    stage: "TAILSCALE_APPROVAL",
    ...overrides,
  };
}

function view(overrides: Partial<OnboardingView> = {}): OnboardingView {
  return {
    active: true,
    connected: false,
    peerKey: "priya-mac.tailc930b7.ts.net.",
    displayName: "Priya",
    mySide: "GUEST",
    progress: {
      stage: "INVITE_FRIEND",
      method: null,
      checks: [check("TAILSCALE_INSTALLED", "PASSED")],
      issues: [],
      topology: "UNKNOWN",
      direction: "UNKNOWN",
      connectedness: "NOT_ATTEMPTED",
      hasHandshakeProof: false,
      finished: false,
      waitingOnPartner: false,
    },
    shareLink: null,
    ourFingerprint: "MP-AAAA-BBBB-CCCC-DDDD",
    theirFingerprint: null,
    nodeKey: "UNKNOWN",
    tailnet: null,
    probe: null,
    methods: [],
    flows: [],
    directionEvidence: "REASONED_FROM_CODE_AND_DOCS",
    observedAtMs: 0,
    ...overrides,
  };
}

function facts(method: OnboardingMethod): MethodFacts {
  return {
    method,
    summary:
      method === "DEVICE_SHARING"
        ? "Share only the machine Movie Party runs on with one named person."
        : "Bring the other device into the tailnet as a full node.",
    grants: ["They reach this one machine, nothing else on your tailnet."],
    limits: [
      "Creating a share needs tailnet administration rights on YOUR side.",
      "A shared machine can receive incoming connections but cannot start connections, so only the guest-to-host direction works.",
      "They must still log in and accept the share themselves — acceptance is never automatic.",
    ],
    adminNeededBy: "HOST",
    recipientMustConsent: true,
    adminUrl: "https://login.tailscale.com/admin/machines",
  };
}

const probe = (reachable: boolean, message = ""): PeerConnectionProbe => ({
  reachable,
  path: reachable ? "direct" : null,
  latencyMs: reachable ? 14 : null,
  message,
});

describe("wizard stage model", () => {
  it("is the six stages requirement 4 asks for, in order", () => {
    expect(WIZARD_STAGES).toEqual([
      "INVITE_FRIEND",
      "CHOOSE_METHOD",
      "TAILSCALE_APPROVAL",
      "PEER_REACHABILITY",
      "IDENTITY_VERIFICATION",
      "FRIEND_READY",
    ]);
    for (const stage of WIZARD_STAGES) {
      expect(STAGE_TITLES[stage]).toBeTruthy();
      expect(STAGE_STEP_COPY[stage]).toBeTruthy();
      expect(stagePosition(stage).index).toBe(WIZARD_STAGES.indexOf(stage));
    }
    expect(WIZARD_STAGES).toHaveLength(6);
  });

  it("counts steps out of the real total, and never off the end", () => {
    expect(stagePosition("FRIEND_READY")).toEqual({ index: 5, total: 6 });
    expect(stagePosition("NOT_A_STAGE" as WizardStage)).toEqual({ index: 0, total: 6 });
  });

  it("tells the truth about what Movie Party cannot do for the user", () => {
    // Requirement 17: no stage may imply the app created the access.
    expect(STAGE_STEP_COPY.TAILSCALE_APPROVAL).toContain("can't do either of those for you");
    expect(SHARE_LINK_APPLY_NOTE).toContain("cannot claim access for you");
  });

  it("offers only the actions that exist at each stage", () => {
    expect(
      stageActions(view({ progress: { ...view().progress, stage: "INVITE_FRIEND" } })),
    ).toEqual(["CHECK_AGAIN"]);
    expect(
      stageActions(view({ progress: { ...view().progress, stage: "CHOOSE_METHOD" } })),
    ).toEqual(["OPEN_TAILSCALE_ADMIN", "CHECK_AGAIN"]);
    expect(
      stageActions(view({ progress: { ...view().progress, stage: "PEER_REACHABILITY" } })),
    ).toEqual(["CHECK_REACHABILITY", "CHECK_AGAIN"]);
    expect(
      stageActions(view({ progress: { ...view().progress, stage: "IDENTITY_VERIFICATION" } })),
    ).toEqual(["CHECK_AGAIN"]);
    // Only the finished stage may offer to wrap up.
    expect(stageActions(view({ progress: { ...view().progress, stage: "FRIEND_READY" } }))).toEqual(
      ["COPY_PACKAGE", "FINISH"],
    );
  });
});

describe("requirement 15: the word Connected", () => {
  it("is reserved for a real Movie Party handshake", () => {
    const connected = view({
      connected: true,
      progress: { ...view().progress, connectedness: "MOVIE_PARTY_HANDSHAKED" },
    });
    expect(connectednessCopy("MOVIE_PARTY_HANDSHAKED", connected).label).toBe("Connected");
    expect(connectednessCopy("MOVIE_PARTY_HANDSHAKED", connected).tone).toBe("ok");
  });

  it("is never used for a tunnel that only Tailscale can see", () => {
    const copy = connectednessCopy("TUNNEL_REACHABLE_ONLY", view());
    expect(copy.label).not.toBe("Connected");
    expect(copy.label).toBe("Tunnel only");
    expect(copy.detail).toContain("has not authenticated it yet");
    expect(copy.tone).toBe("wait");
  });

  it("is withheld even from a handshaked state the view has not confirmed", () => {
    // Defence in depth: `connected` is the resolved pair of connectedness +
    // proof, so a handshaked connectedness without it must not read as done.
    const copy = connectednessCopy("MOVIE_PARTY_HANDSHAKED", view({ connected: false }));
    expect(copy.label).not.toBe("Connected");
  });

  it("starts honest: nothing attempted, nothing claimed", () => {
    expect(connectednessCopy("NOT_ATTEMPTED", view()).label).toBe("Not connected");
  });

  it("names the evidence behind a connected claim", () => {
    const withProof = view({
      connected: true,
      progress: {
        ...view().progress,
        connectedness: "MOVIE_PARTY_HANDSHAKED",
        hasHandshakeProof: true,
      },
    });
    expect(handshakeEvidenceLine(withProof)).toContain("authenticated room handshake");

    const waiting = view();
    expect(handshakeEvidenceLine(waiting)).toContain("No Movie Party transport handshake");

    // A claim with no proof is reported as a bug, not smoothed over.
    const impossible = view({
      progress: {
        ...view().progress,
        connectedness: "MOVIE_PARTY_HANDSHAKED",
        hasHandshakeProof: false,
      },
    });
    expect(handshakeEvidenceLine(impossible)).toContain("report this as a bug");
  });

  it("keeps the headline banner in step with the reducer", () => {
    expect(headlineStatus(view({ connected: true })).tone).toBe("ok");
    expect(headlineStatus(view({ connected: true })).text).toContain("authenticated");

    const blocked = view({
      progress: {
        ...view().progress,
        issues: [issue({ blocking: true, headline: "Their device is offline" })],
      },
    });
    expect(headlineStatus(blocked).tone).toBe("bad");
    expect(headlineStatus(blocked).text).toContain("Their device is offline");

    const waiting = view({ progress: { ...view().progress, waitingOnPartner: true } });
    expect(headlineStatus(waiting).tone).toBe("wait");
    expect(headlineStatus(waiting).text).toContain("Waiting on the other side");

    expect(headlineStatus(view()).text).toBe("In progress.");
  });
});

describe("checklist rows", () => {
  it("labels all ten checks, uniquely", () => {
    const ids: CheckId[] = [
      "TAILSCALE_INSTALLED",
      "TAILSCALE_SIGNED_IN",
      "NODE_KEY_VALID",
      "ADMIN_ABLE_TO_GRANT_ACCESS",
      "FRIEND_INVITED",
      "METHOD_CHOSEN",
      "PEER_VISIBLE_ON_NETWORK",
      "PEER_REACHABLE_THROUGH_TUNNEL",
      "DIRECTION_WORKS_FOR_MOVIE_PARTY",
      "MOVIE_PARTY_TRANSPORT_HANDSHAKE",
    ];
    const labels = ids.map(checkRowLabel);
    expect(new Set(labels).size).toBe(10);
    expect(labels).toContain("Reachable through the tunnel");
    expect(labels).toContain("Movie Party handshake");
  });

  it("maps every check state to a tone", () => {
    expect(checkRowTone("PASSED")).toBe("ok");
    expect(checkRowTone("WAITING")).toBe("wait");
    expect(checkRowTone("FAILED")).toBe("bad");
    expect(checkRowTone("NOT_APPLICABLE")).toBe("off");
  });
});

describe("requirement 11: how the two devices are attached", () => {
  it("says same-tailnet membership plainly", () => {
    expect(topologyCopy("SAME_TAILNET")).toContain("same tailnet");
  });

  it("reports an unclassifiable link as unknown, not as a guess", () => {
    expect(topologyCopy("UNKNOWN")).toContain("doesn't say");
  });

  it("names which side owns a cross-tailnet share", () => {
    expect(topologyCopy({ SHARED_ACROSS_TAILNETS: { sharer: "HOST" } })).toContain(
      "host owns the share",
    );
    expect(topologyCopy({ SHARED_ACROSS_TAILNETS: { sharer: "GUEST" } })).toContain(
      "guest owns the share",
    );
    expect(sideWord("HOST")).toBe("host");
    expect(sideWord("GUEST")).toBe("guest");
  });
});

describe("requirements 12-14: the sharing direction", () => {
  const reasoned: DirectionEvidence = "REASONED_FROM_CODE_AND_DOCS";

  it("quotes Tailscale's own quarantine rule and marks it unverified", () => {
    const copy = directionCopy("COMPATIBLE", reasoned);
    expect(copy.detail).toContain("cannot start connections");
    expect(copy.detail).toContain("No two-device run has confirmed it");
    expect(copy.headline).toBe("Direction matches");
    expect(copy.remedy).toBeNull();
  });

  it("gives the remedy when the share points the wrong way", () => {
    const copy = directionCopy(
      { INCOMPATIBLE: { remedy: "Share the host's machine with the guest instead." } },
      reasoned,
    );
    expect(copy.headline).toBe("Sharing is pointing the wrong way");
    expect(copy.remedy).toContain("host's machine with the guest");
  });

  it("does not raise a direction problem for members of one tailnet", () => {
    const copy = directionCopy("BOTH_DIRECTIONS_AVAILABLE", reasoned);
    expect(copy.headline).toBe("Either direction works");
    expect(copy.detail).toContain("doesn't apply");
  });

  it("promises nothing when it cannot tell", () => {
    const copy = directionCopy("UNKNOWN", reasoned);
    expect(copy.headline).toBe("Direction unknown");
    expect(copy.remedy).toContain("which device was shared");
  });

  it("flips to confirmed wording only with two-device evidence", () => {
    const copy = directionCopy("COMPATIBLE", "LIVE_VERIFIED_TWO_DEVICES");
    expect(copy.detail).toContain("Confirmed by a real host/guest pair");
    expect(copy.detail).not.toContain("No two-device run");
  });

  it("lists every transport flow with who starts it and what is proved", () => {
    const rows = flowRows([
      {
        name: "Room control (QUIC streams)",
        startsFrom: "GUEST",
        needsReverseInitiation: false,
        evidence: "AUTOMATED_VERIFIED",
      },
      {
        name: "Live-call media (WebRTC UDP)",
        startsFrom: "GUEST",
        needsReverseInitiation: true,
        evidence: "UNVERIFIED_NEEDS_TWO_DEVICES",
      },
    ]);
    expect(rows[0]?.starts).toBe("started by the guest");
    expect(rows[0]?.reverse).toBe("one direction is enough");
    expect(rows[0]?.evidence).toBe("verified in the automated suite");
    // The one flow that may need both directions is flagged as unverified, never
    // presented as working.
    expect(rows[1]?.reverse).toBe("needs the other direction too");
    expect(rows[1]?.evidence).toBe("not verified — needs two devices");
  });
});

describe("requirement 5: an explicit choice between the two methods", () => {
  it("shows grants and limits for both, and who has to act", () => {
    const hostCard = methodCard(facts("DEVICE_SHARING"), true, "HOST");
    const guestCard = methodCard(facts("TAILNET_MEMBERSHIP"), false, "GUEST");

    expect(hostCard.selected).toBe(true);
    expect(guestCard.selected).toBe(false);
    expect(hostCard.title).toBe("Share this one machine");
    expect(guestCard.title).toBe("Join one tailnet");
    expect(hostCard.grants.length).toBeGreaterThan(0);
    expect(hostCard.limits.length).toBeGreaterThan(0);
    expect(guestCard.limits.length).toBeGreaterThan(0);
    expect(hostCard.adminNote).toContain("that's you");
    expect(guestCard.adminNote).toContain("that's them");
    for (const card of [hostCard, guestCard]) {
      expect(card.consent).toContain("never automatic");
    }
  });

  it("carries the quarantine limit into the choice itself", () => {
    const card = methodCard(facts("DEVICE_SHARING"), false, "HOST");
    expect(card.limits.join(" ")).toContain("cannot start connections");
  });
});

describe("requirements 7-8: the pasted share link", () => {
  it("explains the bearer rules before anything is pasted", () => {
    const copy = shareLinkSafetyCopy(null);
    expect(copy).toContain("bearer credential");
    expect(copy).toContain("isn't stored, logged, or sent anywhere");
  });

  it("confirms an invite by fingerprint only", () => {
    const copy = shareLinkSafetyCopy({
      kind: "TAILSCALE_INVITE",
      tokenFingerprint: "1a2b3c4d",
      tailnetHandleMatchesLocal: null,
    });
    expect(copy).toContain("1a2b3c4d");
    expect(copy).toContain("held in memory only");
    expect(copy).toContain("couldn't tell which tailnet");
  });

  it("distinguishes the handle it recognises from one it does not", () => {
    const same = shareLinkSafetyCopy({
      kind: "TAILSCALE_INVITE",
      tokenFingerprint: "ffff0000",
      tailnetHandleMatchesLocal: true,
    });
    const other = shareLinkSafetyCopy({
      kind: "TAILSCALE_INVITE",
      tokenFingerprint: "ffff0000",
      tailnetHandleMatchesLocal: false,
    });
    expect(same).toContain("already on");
    expect(other).toContain("different tailnet");
  });

  it("calls a login link what it is, not a share", () => {
    expect(
      shareLinkSafetyCopy({
        kind: "TAILSCALE_LOGIN",
        tokenFingerprint: "",
        tailnetHandleMatchesLocal: null,
      }),
    ).toContain("sign-in link, not a share invite");
  });
});

describe("requirement 16: the failure states", () => {
  it("renders a blocking issue as a problem and a notice as a wait", () => {
    expect(issueRow(issue({ blocking: true })).tone).toBe("bad");
    expect(issueRow(issue({ blocking: false })).tone).toBe("wait");
    const row = issueRow(
      issue({
        code: "MP-ONB-012",
        headline: "Wrong sharing direction for Movie Party",
        guidance:
          "Movie Party's guest always starts the one connection it makes, so the HOST must be shared.",
      }),
    );
    expect(row.code).toBe("MP-ONB-012");
    expect(row.headline).toContain("sharing direction");
    expect(row.guidance).toContain("HOST must be shared");
  });
});

describe("role and key expiry reporting", () => {
  it("never guesses an ordinary member into an admin", () => {
    expect(roleCopy("OWNER")).toContain("Owner");
    expect(roleCopy("ADMIN")).toContain("admin");
    expect(roleCopy("LIMITED_USER")).toContain("can't create shares");
    expect(roleCopy("UNKNOWN")).toContain("won't guess");
    expect(roleCopy(undefined)).toContain("won't guess");
  });

  it("separates an expired key from one that is merely close", () => {
    expect(keyExpiryCopy("EXPIRED")).toContain("expired");
    expect(keyExpiryCopy("EXPIRING_SOON")).toContain("30 days");
    expect(keyExpiryCopy("VALID")).toContain("valid");
    expect(keyExpiryCopy("UNKNOWN")).toContain("didn't report");
  });

  it("treats a failed probe as information, not an error screen", () => {
    const withProbe = view({ probe: probe(false, "MP-NET-TS-007 no answer from the peer") });
    expect(withProbe.probe?.reachable).toBe(false);
    expect(withProbe.probe?.message).toContain("MP-NET-TS-007");
    // Reachability is a separate claim from connectedness.
    expect(headlineStatus(withProbe).text).toBe("In progress.");
  });

  it("keeps a saved friend list usable for the picker", () => {
    const friends: StoredFriend[] = [
      {
        peerKey: "priya-mac.tailc930b7.ts.net.",
        displayName: "Priya",
        ip: "100.64.0.42",
        addedAtMs: 1,
        lastVerifiedAtMs: null,
        lastPath: null,
        lastLatencyMs: null,
        connectionState: "INVITED",
      },
    ];
    expect(friends[0]?.peerKey).toBe("priya-mac.tailc930b7.ts.net.");
    expect(friends[0]?.displayName).toBe("Priya");
  });
});
