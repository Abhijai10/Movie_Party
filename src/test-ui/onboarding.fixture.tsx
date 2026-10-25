// Test-only IPC fixture. Never imported by the production entry point.
//
// Batch 9's wizard has six stages and fourteen distinct failure states, and most
// of them cannot be reached on a machine that has Tailscale installed and signed
// in. This harness renders the real component against built views so each state
// can be looked at, and so the "Connected only after a handshake" rule can be
// checked where it is actually displayed.
import { createRoot } from "react-dom/client";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { useState, type JSX } from "react";
import { OnboardingWizardView } from "../views/OnboardingWizardView";
import type {
  CheckId,
  OnboardingView,
  StoredFriend,
  WizardCheck,
  WizardIssue,
  WizardStage,
} from "../backend/appRuntime";
import "../styles.css";

mockWindows("main");

const friends: StoredFriend[] = [
  {
    peerKey: "priya-mac.tailc930b7.ts.net.",
    displayName: "Priya",
    ip: "100.64.0.42",
    addedAtMs: 1_767_225_600_000,
    lastVerifiedAtMs: null,
    lastPath: null,
    lastLatencyMs: null,
    connectionState: "INVITED",
  },
];

const ALL_CHECKS: Array<{ id: CheckId; label: string }> = [
  { id: "TAILSCALE_INSTALLED", label: "Tailscale on this device" },
  { id: "TAILSCALE_SIGNED_IN", label: "Signed in and connected" },
  { id: "NODE_KEY_VALID", label: "This device's Tailscale key" },
  { id: "ADMIN_ABLE_TO_GRANT_ACCESS", label: "You can grant the access" },
  { id: "FRIEND_INVITED", label: "Friendship accepted" },
  { id: "METHOD_CHOSEN", label: "Connection method" },
  { id: "PEER_VISIBLE_ON_NETWORK", label: "Their device is on the network" },
  { id: "PEER_REACHABLE_THROUGH_TUNNEL", label: "Reachable through the tunnel" },
  { id: "DIRECTION_WORKS_FOR_MOVIE_PARTY", label: "Connection direction works" },
  { id: "MOVIE_PARTY_TRANSPORT_HANDSHAKE", label: "Movie Party handshake" },
];

function checks(passed: CheckId[], failed: Partial<Record<CheckId, string>> = {}): WizardCheck[] {
  // One row per check id. The earlier shape appended overrides on top of the full
  // list, which produced two rows for the same id — and React renders duplicate
  // keys as duplicate DOM, so a stale "MP-ONB-001" sat next to a healthy row.
  return ALL_CHECKS.map(({ id }) => {
    const code = failed[id];
    if (code !== undefined) return { id, state: "FAILED", code };
    return { id, state: passed.includes(id) ? "PASSED" : "WAITING", code: null };
  });
}

function issue(
  id: string,
  code: string,
  headline: string,
  guidance: string,
  stage: WizardStage,
  blocking = true,
): WizardIssue {
  return { id, code, headline, guidance, blocking, stage };
}

const methods = [
  {
    method: "DEVICE_SHARING" as const,
    summary: "Share only the machine Movie Party runs on with one named person.",
    grants: [
      "They reach this one machine, nothing else on your tailnet.",
      "The share is issued to one named account and can be revoked from the admin console at any time.",
      "They keep their own Tailscale account and identity; you never hand over a key or password.",
      "No new software: if they already use Tailscale they just accept the share.",
    ],
    limits: [
      "Creating a share needs tailnet administration rights on YOUR side.",
      "A shared machine can receive incoming connections but cannot start connections, so only the guest-to-host direction works.",
      "They must still log in and accept the share themselves — acceptance is never automatic.",
    ],
    adminNeededBy: "HOST" as const,
    recipientMustConsent: true,
    adminUrl: "https://login.tailscale.com/admin/machines",
  },
  {
    method: "TAILNET_MEMBERSHIP" as const,
    summary: "Bring the other device into the tailnet as a full node.",
    grants: [
      "Both devices can start connections to each other, subject to the tailnet's ACLs.",
      "Movie Party's own direction rules stop mattering, so live calls are less likely to hit a one-way block.",
      "One setup covers every future party between the two devices.",
    ],
    limits: [
      "A full node can see more than one machine unless the tailnet's ACLs restrict it.",
      "Someone must hold tailnet administration rights to send the invite — often not the person using Movie Party.",
      "On a work tailnet the policy is somebody else's, and Movie Party cannot change it.",
      "They must accept the invitation and sign in with their own Tailscale account.",
    ],
    adminNeededBy: "HOST" as const,
    recipientMustConsent: true,
    adminUrl: "https://login.tailscale.com/admin/users",
  },
];

const flows = [
  {
    name: "Room control (QUIC streams)",
    startsFrom: "GUEST" as const,
    needsReverseInitiation: false,
    evidence: "AUTOMATED_VERIFIED",
  },
  {
    name: "Movie file streaming",
    startsFrom: "GUEST" as const,
    needsReverseInitiation: false,
    evidence: "AUTOMATED_VERIFIED",
  },
  {
    name: "Chat",
    startsFrom: "GUEST" as const,
    needsReverseInitiation: false,
    evidence: "AUTOMATED_VERIFIED",
  },
  {
    name: "Live-call signalling (SDP + ICE)",
    startsFrom: "GUEST" as const,
    needsReverseInitiation: false,
    evidence: "AUTOMATED_VERIFIED",
  },
  {
    name: "Live-call media (WebRTC UDP)",
    startsFrom: "GUEST" as const,
    needsReverseInitiation: true,
    evidence: "UNVERIFIED_NEEDS_TWO_DEVICES",
  },
];

const tailnet = {
  stableId: "TnSLAyoyHo11CNTRL",
  magicDnsSuffix: "tailc930b7.ts.net",
  role: "OWNER" as const,
  keyExpiry: "2027-02-20T20:43:23Z",
  haveNodeKey: true,
  loginUrlPending: false,
};

function base(overrides: Partial<OnboardingView>): OnboardingView {
  return {
    active: true,
    connected: false,
    peerKey: "priya-mac.tailc930b7.ts.net.",
    displayName: "Priya",
    mySide: "GUEST",
    progress: {
      stage: "INVITE_FRIEND",
      method: null,
      checks: checks([]),
      issues: [],
      topology: "UNKNOWN",
      direction: "UNKNOWN",
      connectedness: "NOT_ATTEMPTED",
      hasHandshakeProof: false,
      finished: false,
      waitingOnPartner: false,
    },
    shareLink: null,
    ourFingerprint: "MP-1A2B-3C4D-5E6F-7A8B",
    theirFingerprint: null,
    nodeKey: "VALID",
    tailnet,
    probe: null,
    methods,
    flows,
    directionEvidence: "REASONED_FROM_CODE_AND_DOCS",
    observedAtMs: 1_767_225_600_000,
    ...overrides,
  };
}

/** One entry per state the wizard can be in. */
export const CASES: Record<string, () => OnboardingView> = {
  "00 not started (idle)": () => base({ active: false }),
  "01 invite a friend": () =>
    base({
      progress: {
        ...base({}).progress,
        checks: checks(["TAILSCALE_INSTALLED", "TAILSCALE_SIGNED_IN", "NODE_KEY_VALID"]),
        issues: [
          issue(
            "DUPLICATE_FRIEND_REQUEST",
            "MP-ONB-010",
            "You already asked this friend",
            "This invite is still waiting for their decision. Send a new one only if they say the old one expired.",
            "INVITE_FRIEND",
            false,
          ),
        ],
        waitingOnPartner: true,
      },
    }),
  "02 choose a method": () =>
    base({
      progress: {
        ...base({}).progress,
        stage: "CHOOSE_METHOD",
        checks: checks([
          "TAILSCALE_INSTALLED",
          "TAILSCALE_SIGNED_IN",
          "NODE_KEY_VALID",
          "FRIEND_INVITED",
        ]),
      },
    }),
  "03 limited user picks sharing": () =>
    base({
      tailnet: { ...tailnet, role: "LIMITED_USER" },
      progress: {
        ...base({}).progress,
        stage: "CHOOSE_METHOD",
        method: "DEVICE_SHARING",
        checks: checks([
          "TAILSCALE_INSTALLED",
          "TAILSCALE_SIGNED_IN",
          "NODE_KEY_VALID",
          "FRIEND_INVITED",
          "METHOD_CHOSEN",
        ]),
        issues: [
          issue(
            "NOT_TAILNET_ADMIN",
            "MP-ONB-011",
            "Your account can't grant this access",
            "Your Tailscale login can't create shares or invitations here. Ask the tailnet's admin to share the machine with them, or use a tailnet you own.",
            "CHOOSE_METHOD",
          ),
        ],
      },
    }),
  "04 waiting on their acceptance": () =>
    base({
      progress: {
        ...base({}).progress,
        stage: "TAILSCALE_APPROVAL",
        method: "DEVICE_SHARING",
        checks: checks([
          "TAILSCALE_INSTALLED",
          "TAILSCALE_SIGNED_IN",
          "NODE_KEY_VALID",
          "ADMIN_ABLE_TO_GRANT_ACCESS",
          "FRIEND_INVITED",
          "METHOD_CHOSEN",
        ]),
        issues: [
          issue(
            "SHARE_PENDING",
            "MP-ONB-008",
            "Waiting for them to accept in Tailscale",
            "They need to open the Tailscale link and accept it while signed in with their own account.",
            "TAILSCALE_APPROVAL",
            false,
          ),
        ],
        waitingOnPartner: true,
      },
      shareLink: {
        kind: "TAILSCALE_INVITE",
        tokenFingerprint: "1a2b3c4d",
        tailnetHandleMatchesLocal: false,
      },
    }),
  "05 tailscale missing": () =>
    base({
      tailnet: null,
      nodeKey: "UNKNOWN",
      progress: {
        ...base({}).progress,
        stage: "TAILSCALE_APPROVAL",
        method: "DEVICE_SHARING",
        checks: checks(["FRIEND_INVITED", "METHOD_CHOSEN", "ADMIN_ABLE_TO_GRANT_ACCESS"], {
          TAILSCALE_INSTALLED: "MP-ONB-001",
          TAILSCALE_SIGNED_IN: "MP-ONB-002",
        }),
        issues: [
          issue(
            "TAILSCALE_MISSING",
            "MP-ONB-001",
            "Tailscale isn't installed here",
            "Install Tailscale on this device from tailscale.com, then check again. Movie Party doesn't download it for you.",
            "TAILSCALE_APPROVAL",
          ),
        ],
      },
    }),
  "06 expired node key": () =>
    base({
      nodeKey: "EXPIRED",
      progress: {
        ...base({}).progress,
        stage: "TAILSCALE_APPROVAL",
        method: "DEVICE_SHARING",
        checks: checks(
          ["TAILSCALE_INSTALLED", "TAILSCALE_SIGNED_IN", "FRIEND_INVITED", "METHOD_CHOSEN"],
          { NODE_KEY_VALID: "MP-ONB-004" },
        ),
        issues: [
          issue(
            "NODE_KEY_EXPIRED",
            "MP-ONB-004",
            "This device's Tailscale key expired",
            "Open Tailscale and reconnect this device; that renews its key. Keys expire on the tailnet's schedule, not Movie Party's.",
            "TAILSCALE_APPROVAL",
          ),
        ],
      },
    }),
  "07 reachability + wrong direction": () =>
    base({
      progress: {
        ...base({}).progress,
        stage: "PEER_REACHABILITY",
        method: "DEVICE_SHARING",
        topology: { SHARED_ACROSS_TAILNETS: { sharer: "GUEST" } },
        direction: {
          INCOMPATIBLE: {
            remedy:
              "You shared their machine into your tailnet, but Movie Party's guest is the side that dials. Use one of: (1) the HOST shares their machine with the GUEST's account; (2) both sides share with each other (mutual sharing); (3) invite them as a node of one tailnet so both directions work.",
          },
        },
        checks: checks([
          "TAILSCALE_INSTALLED",
          "TAILSCALE_SIGNED_IN",
          "NODE_KEY_VALID",
          "ADMIN_ABLE_TO_GRANT_ACCESS",
          "FRIEND_INVITED",
          "METHOD_CHOSEN",
          "PEER_VISIBLE_ON_NETWORK",
          "PEER_REACHABLE_THROUGH_TUNNEL",
        ]),
        issues: [
          issue(
            "DIRECTION_BLOCKED",
            "MP-ONB-012",
            "Wrong sharing direction for Movie Party",
            "Movie Party's guest always starts the one connection it makes, so the HOST has to be the shared machine. Share the other way, share mutually, or put both devices in one tailnet.",
            "IDENTITY_VERIFICATION",
          ),
        ],
      },
      probe: { reachable: true, path: "direct", latencyMs: 18, message: "" },
    }),
  "08 same tailnet, reachable": () =>
    base({
      progress: {
        ...base({}).progress,
        stage: "PEER_REACHABILITY",
        method: "TAILNET_MEMBERSHIP",
        topology: "SAME_TAILNET",
        direction: "BOTH_DIRECTIONS_AVAILABLE",
        checks: checks([
          "TAILSCALE_INSTALLED",
          "TAILSCALE_SIGNED_IN",
          "NODE_KEY_VALID",
          "ADMIN_ABLE_TO_GRANT_ACCESS",
          "FRIEND_INVITED",
          "METHOD_CHOSEN",
          "PEER_VISIBLE_ON_NETWORK",
          "PEER_REACHABLE_THROUGH_TUNNEL",
        ]),
      },
      probe: { reachable: true, path: "peerRelay", latencyMs: 34, message: "" },
    }),
  "09 tunnel only, no handshake": () =>
    base({
      progress: {
        ...base({}).progress,
        stage: "IDENTITY_VERIFICATION",
        method: "DEVICE_SHARING",
        topology: { SHARED_ACROSS_TAILNETS: { sharer: "HOST" } },
        direction: "COMPATIBLE",
        connectedness: "TUNNEL_REACHABLE_ONLY",
        checks: checks([
          "TAILSCALE_INSTALLED",
          "TAILSCALE_SIGNED_IN",
          "NODE_KEY_VALID",
          "ADMIN_ABLE_TO_GRANT_ACCESS",
          "FRIEND_INVITED",
          "METHOD_CHOSEN",
          "PEER_VISIBLE_ON_NETWORK",
          "PEER_REACHABLE_THROUGH_TUNNEL",
        ]),
        issues: [
          issue(
            "HANDSHAKE_MISSING",
            "MP-ONB-013",
            "Movie Party hasn't authenticated them yet",
            "The tunnel looks fine, but Movie Party hasn't authenticated this device yet. Join or host a party once; the handshake happens there.",
            "IDENTITY_VERIFICATION",
          ),
        ],
      },
      probe: { reachable: true, path: "direct", latencyMs: 11, message: "" },
    }),
  "10 connected and ready": () =>
    base({
      connected: true,
      theirFingerprint: "MP-9F8E-7D6C-5B4A-3210",
      progress: {
        ...base({}).progress,
        stage: "FRIEND_READY",
        method: "DEVICE_SHARING",
        topology: { SHARED_ACROSS_TAILNETS: { sharer: "HOST" } },
        direction: "COMPATIBLE",
        connectedness: "MOVIE_PARTY_HANDSHAKED",
        hasHandshakeProof: true,
        finished: true,
        checks: checks(ALL_CHECKS.map((entry) => entry.id)),
      },
      probe: { reachable: true, path: "direct", latencyMs: 9, message: "" },
    }),
  "11 blocked friend": () =>
    base({
      progress: {
        ...base({}).progress,
        stage: "INVITE_FRIEND",
        checks: checks(["TAILSCALE_INSTALLED", "TAILSCALE_SIGNED_IN", "NODE_KEY_VALID"]),
        issues: [
          issue(
            "BLOCKED_BY_YOU",
            "MP-ONB-014",
            "You blocked this friend",
            "Unblock them from the Friends list first. Blocking stops Movie Party trusting that identity.",
            "INVITE_FRIEND",
          ),
        ],
      },
    }),
};

function Fixture(): JSX.Element {
  const keys = Object.keys(CASES);
  // `?case=4` (index or exact label) lets a scripted browser check one state per
  // page load instead of clicking through the toolbar.
  const requested =
    typeof window === "undefined"
      ? undefined
      : new URLSearchParams(window.location.search).get("case");
  const initial =
    (requested !== undefined && requested !== null
      ? (keys[Number(requested)] ?? keys.find((key) => key === requested))
      : undefined) ??
    keys[0] ??
    "";
  const [current, setCurrent] = useState(initial);
  const [events, setEvents] = useState<string[]>([]);
  const selected = CASES[current];
  const view = selected ? selected() : base({});

  const log = (entry: string) => {
    setEvents((previous) => [entry, ...previous].slice(0, 8));
  };

  return (
    <div className="min-h-screen bg-[#05050B] text-white">
      <nav className="flex flex-wrap gap-2 p-4 bg-black/60 sticky top-0 z-50">
        {keys.map((key) => (
          <button
            key={key}
            type="button"
            onClick={() => {
              setCurrent(key);
            }}
            className="px-3 py-1.5 rounded-full border border-white/15 text-xs bg-white/5 hover:bg-white/10"
          >
            {key}
          </button>
        ))}
      </nav>
      <OnboardingWizardView
        view={view}
        friends={friends}
        busy={null}
        error={events[0] ?? null}
        onBack={() => {
          log("back");
        }}
        onStart={(peerKey, side) => {
          log(`start ${peerKey} ${side}`);
        }}
        onSetSide={(side) => {
          log(`side ${side}`);
        }}
        onChooseMethod={(method) => {
          log(`method ${method}`);
        }}
        onOpenAdmin={() => {
          log("open admin");
        }}
        onPasteShareLink={(link) => {
          log(`paste ${link.slice(0, 12)}…`);
        }}
        onOpenShareLink={() => {
          log("open link");
        }}
        onRefresh={() => {
          log("refresh");
        }}
        onCheckReachability={() => {
          log("reachability");
        }}
        onReportLinkExpired={() => {
          log("expired");
        }}
        onCopyPackage={() => {
          log("package");
        }}
        onFinish={() => {
          log("finish");
        }}
      />
    </div>
  );
}

mockIPC((command) => {
  // The harness renders the component directly; this only exists so a stray IPC
  // call inside the view resolves instead of hanging.
  if (command === "onboarding_view") return base({});
  return null;
});

const rootElement = document.getElementById("root");
if (rootElement) createRoot(rootElement).render(<Fixture />);
