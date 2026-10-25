/**
 * Batch 9 — the pure UI model for the friend + Tailscale wizard.
 *
 * The Rust reducer (`src-tauri/src/onboarding`) owns the decisions; this module
 * owns only what those decisions are *called* and what the user can do about
 * them. It exists as a separate, tested module because the one place a wizard
 * like this goes wrong is copy that overstates a state: "Connected" for a tunnel
 * that Movie Party has never authenticated, or "ready" for a share the other
 * person has not accepted yet.
 */
import type {
  CheckId,
  CheckState,
  Connectedness,
  DirectionEvidence,
  DirectionVerdict,
  NodeKeyState,
  MethodFacts,
  OnboardingMethod,
  OnboardingSide,
  OnboardingView,
  TailnetRole,
  PeerTopology,
  ShareLinkSummary,
  TransportFlow,
  WizardIssue,
  WizardStage,
} from "../backend/appRuntime";

/** Requirement 4: the six stages, in the order the wizard shows them. */
export const WIZARD_STAGES: WizardStage[] = [
  "INVITE_FRIEND",
  "CHOOSE_METHOD",
  "TAILSCALE_APPROVAL",
  "PEER_REACHABILITY",
  "IDENTITY_VERIFICATION",
  "FRIEND_READY",
];

export const STAGE_TITLES: Record<WizardStage, string> = {
  INVITE_FRIEND: "Invite your friend",
  CHOOSE_METHOD: "Choose how you'll connect",
  TAILSCALE_APPROVAL: "Tailscale approval",
  PEER_REACHABILITY: "Reach their device",
  IDENTITY_VERIFICATION: "Verify the identity",
  FRIEND_READY: "Ready for the party",
};

export const STAGE_STEP_COPY: Record<WizardStage, string> = {
  INVITE_FRIEND:
    "Send your signed invitation. Nothing becomes a friendship until they accept it — and accepting is their decision, not yours.",
  CHOOSE_METHOD:
    "Two ways to put your devices on the same private network. They grant different things, so pick deliberately.",
  TAILSCALE_APPROVAL:
    "Tailscale creates and delivers the access. You open its own page, they accept with their own account. Movie Party can't do either of those for you.",
  PEER_REACHABILITY:
    "Movie Party needs to reach their device through the tunnel. This is where a wrong share direction shows up.",
  IDENTITY_VERIFICATION:
    "The last step is Movie Party itself: one authenticated transport handshake between the two devices.",
  FRIEND_READY:
    "Both identities are trusted, the tunnel carried a real handshake, and the two of you are ready.",
};

/**
 * Requirement 15, stated as copy. The label may only ever be "Connected" with a
 * handshake proof behind it; `TUNNEL_REACHABLE_ONLY` is deliberately *not*
 * rendered as connected, and the wording says exactly what it does prove.
 */
export function connectednessCopy(
  connectedness: Connectedness,
  view: Pick<OnboardingView, "connected" | "theirFingerprint">,
): { label: string; detail: string; tone: "ok" | "wait" | "off" } {
  if (connectedness === "MOVIE_PARTY_HANDSHAKED" && view.connected) {
    return {
      label: "Connected",
      detail: "Movie Party authenticated this device over the room transport.",
      tone: "ok",
    };
  }
  if (connectedness === "TUNNEL_REACHABLE_ONLY") {
    return {
      label: "Tunnel only",
      detail:
        "Tailscale reaches this device, but Movie Party has not authenticated it yet. Host or join one party to complete the handshake.",
      tone: "wait",
    };
  }
  return { label: "Not connected", detail: "Nothing has been proved yet.", tone: "off" };
}

/** Stage 6 needs the evidence line, not just a green tick. */
export function handshakeEvidenceLine(view: OnboardingView): string {
  const proof = view.progress.connectedness === "MOVIE_PARTY_HANDSHAKED";
  if (!proof) return "No Movie Party transport handshake recorded by this app run.";
  return view.progress.hasHandshakeProof
    ? "Proof: this app run completed an authenticated room handshake with that device id."
    : "Claimed without proof — report this as a bug; Movie Party never marks a device connected on a status entry.";
}

export function checkRowLabel(id: CheckId): string {
  switch (id) {
    case "TAILSCALE_INSTALLED":
      return "Tailscale on this device";
    case "TAILSCALE_SIGNED_IN":
      return "Signed in and connected";
    case "NODE_KEY_VALID":
      return "This device's Tailscale key";
    case "ADMIN_ABLE_TO_GRANT_ACCESS":
      return "You can grant the access";
    case "FRIEND_INVITED":
      return "Friendship accepted";
    case "METHOD_CHOSEN":
      return "Connection method";
    case "PEER_VISIBLE_ON_NETWORK":
      return "Their device is on the network";
    case "PEER_REACHABLE_THROUGH_TUNNEL":
      return "Reachable through the tunnel";
    case "DIRECTION_WORKS_FOR_MOVIE_PARTY":
      return "Connection direction works";
    case "MOVIE_PARTY_TRANSPORT_HANDSHAKE":
      return "Movie Party handshake";
  }
}

export function checkRowTone(state: CheckState): "ok" | "wait" | "bad" | "off" {
  switch (state) {
    case "PASSED":
      return "ok";
    case "FAILED":
      return "bad";
    case "NOT_APPLICABLE":
      return "off";
    case "WAITING":
      return "wait";
  }
}

/** Requirement 11: the topology sentence, without over-claiming. */
export function topologyCopy(topology: PeerTopology): string {
  if (typeof topology === "string") {
    return topology === "SAME_TAILNET"
      ? "Both devices are nodes of the same tailnet."
      : "This device's Tailscale status doesn't say how the other machine is attached.";
  }
  const { sharer } = topology.SHARED_ACROSS_TAILNETS;
  return `One machine was shared across tailnets — the ${sideWord(sharer)} owns the share.`;
}

export function sideWord(side: OnboardingSide): string {
  return side === "HOST" ? "host" : "guest";
}

/**
 * Requirements 12-14. The quarantine rule is quoted from Tailscale's own
 * wording, and the answer is given for *this* direction, because Movie Party's
 * guest is the only side that ever starts a connection.
 */
export function directionCopy(
  verdict: DirectionVerdict,
  evidence: DirectionEvidence,
): { headline: string; detail: string; remedy: string | null } {
  const detail =
    evidence === "REASONED_FROM_CODE_AND_DOCS"
      ? "Reasoned from this build's dial topology (the guest starts the one connection it makes) and Tailscale's published rule: “A shared machine can receive incoming connections but cannot start connections.” No two-device run has confirmed it yet."
      : "Confirmed by a real host/guest pair across a share.";
  if (typeof verdict === "object") {
    return {
      headline: "Sharing is pointing the wrong way",
      detail,
      remedy: verdict.INCOMPATIBLE.remedy,
    };
  }
  switch (verdict as DirectionVerdict) {
    case "COMPATIBLE":
      return {
        headline: "Direction matches",
        detail:
          "The host's machine is the shared one, so the guest dialling the host is the direction Tailscale allows. " +
          detail,
        remedy: null,
      };
    case "BOTH_DIRECTIONS_AVAILABLE":
      return {
        headline: "Either direction works",
        detail:
          "Both devices are tailnet members, so the quarantine limitation doesn't apply. " + detail,
        remedy: null,
      };
    default:
      return {
        headline: "Direction unknown",
        detail:
          "This device can't tell how the other machine is attached, so nothing is promised. " +
          detail,
        remedy:
          "Ask whoever created the access which device was shared, or make both devices members of one tailnet.",
      };
  }
}

/** Requirement 13: per-flow, what starts where — and what is not yet verified. */
export function flowRows(flows: TransportFlow[]): Array<{
  name: string;
  starts: string;
  reverse: string;
  evidence: string;
}> {
  return flows.map((flow) => ({
    name: flow.name,
    starts: `started by the ${sideWord(flow.startsFrom)}`,
    reverse: flow.needsReverseInitiation
      ? "needs the other direction too"
      : "one direction is enough",
    evidence:
      flow.evidence === "AUTOMATED_VERIFIED"
        ? "verified in the automated suite"
        : flow.evidence === "UNVERIFIED_NEEDS_TWO_DEVICES"
          ? "not verified — needs two devices"
          : flow.evidence,
  }));
}

/**
 * Requirement 5 + 17. The choice card never offers an action Movie Party cannot
 * perform: it opens Tailscale's own admin page and otherwise waits.
 */
export function methodCard(
  facts: MethodFacts,
  selected: boolean,
  mySide: OnboardingSide,
): {
  id: OnboardingMethod;
  title: string;
  summary: string;
  grants: string[];
  limits: string[];
  consent: string;
  adminNote: string;
  selected: boolean;
} {
  const who = sideWord(facts.adminNeededBy);
  return {
    id: facts.method,
    title: facts.method === "DEVICE_SHARING" ? "Share this one machine" : "Join one tailnet",
    summary: facts.summary,
    grants: facts.grants,
    limits: facts.limits,
    consent:
      "They must sign in and accept in Tailscale themselves — acceptance is never automatic.",
    adminNote:
      `Someone with ${who} tailnet rights has to create this access` +
      (mySide === facts.adminNeededBy ? " — that's you." : " — that's them."),
    selected,
  };
}

/** Requirement 7/8 copy shown beside the paste field. */
export function shareLinkSafetyCopy(link: ShareLinkSummary | null | undefined): string {
  if (!link) {
    return "Paste a Tailscale share or invite link here and Movie Party will only confirm what it is. The link is a bearer credential: it isn't stored, logged, or sent anywhere, and only its fingerprint is shown.";
  }
  if (link.kind === "TAILSCALE_LOGIN") {
    return "That's a Tailscale sign-in link, not a share invite. Sign in in Tailscale, then come back.";
  }
  const handle =
    link.tailnetHandleMatchesLocal === true
      ? "It belongs to the tailnet this device is already on."
      : link.tailnetHandleMatchesLocal === false
        ? "It belongs to a different tailnet than this device is on."
        : "Movie Party couldn't tell which tailnet it belongs to.";
  return `Recognised as a one-time Tailscale invite (fingerprint ${link.tokenFingerprint}). ${handle} It is held in memory only — nothing was stored or logged.`;
}

/** The paste box must not be able to imply the app joined the network itself. */
export const SHARE_LINK_APPLY_NOTE =
  "Movie Party can open this link in your browser, but you still have to accept it in Tailscale. It cannot claim access for you.";

/** Requirement 16: the issue row, straight from the reducer's own text. */
export function issueRow(issue: WizardIssue): {
  code: string;
  headline: string;
  guidance: string;
  tone: "bad" | "wait";
} {
  return {
    code: issue.code,
    headline: issue.headline,
    guidance: issue.guidance,
    tone: issue.blocking ? "bad" : "wait",
  };
}

/**
 * What the footer offers at each stage. Nothing more — no phantom actions.
 * Sending the invitation itself stays on the Friends screen, so stage 1 only
 * re-reads the position here.
 */
export function stageActions(
  view: OnboardingView,
): Array<
  "OPEN_TAILSCALE_ADMIN" | "CHECK_AGAIN" | "CHECK_REACHABILITY" | "COPY_PACKAGE" | "FINISH"
> {
  const stage = view.progress.stage;
  switch (stage) {
    case "INVITE_FRIEND":
      return ["CHECK_AGAIN"];
    case "CHOOSE_METHOD":
      return ["OPEN_TAILSCALE_ADMIN", "CHECK_AGAIN"];
    case "TAILSCALE_APPROVAL":
      return ["OPEN_TAILSCALE_ADMIN", "CHECK_AGAIN"];
    case "PEER_REACHABILITY":
      return ["CHECK_REACHABILITY", "CHECK_AGAIN"];
    case "IDENTITY_VERIFICATION":
      return ["CHECK_AGAIN"];
    case "FRIEND_READY":
      return ["COPY_PACKAGE", "FINISH"];
  }
}

/** The waiting/blocked banner, derived from the reducer rather than guessed. */
export function headlineStatus(view: OnboardingView): {
  text: string;
  tone: "ok" | "wait" | "bad";
} {
  const blocking = view.progress.issues.filter((issue) => issue.blocking);
  if (view.connected)
    return { text: "Connected — Movie Party authenticated this device.", tone: "ok" };
  const first = blocking[0];
  if (first) {
    return { text: `${first.headline}: ${first.guidance}`, tone: "bad" };
  }
  if (view.progress.waitingOnPartner) {
    return { text: "Waiting on the other side — they have to accept in Tailscale.", tone: "wait" };
  }
  return { text: "In progress.", tone: "wait" };
}

/** Step counter for the header. `FRIEND_READY` is step 6 of 6. */
export function stagePosition(stage: WizardStage): { index: number; total: number } {
  const index = WIZARD_STAGES.indexOf(stage);
  return { index: index < 0 ? 0 : index, total: WIZARD_STAGES.length };
}

export function roleCopy(role: TailnetRole | undefined): string {
  switch (role) {
    case "OWNER":
      return "Owner of this tailnet — you can create shares and invitations.";
    case "ADMIN":
      return "Tailnet admin — you can create shares and invitations.";
    case "LIMITED_USER":
      return "Ordinary member — you can't create shares or invitations here, so the wizard tells you who can.";
    default:
      return "Tailscale didn't report your role on this tailnet. Movie Party won't guess either way.";
  }
}

export function keyExpiryCopy(state: NodeKeyState): string {
  switch (state) {
    case "EXPIRED":
      return "This device's Tailscale key has expired. Open Tailscale and reconnect to renew it.";
    case "EXPIRING_SOON":
      return "This device's Tailscale key expires within 30 days. Worth renewing before a party.";
    case "VALID":
      return "This device's Tailscale key is valid.";
    case "UNKNOWN":
      return "Tailscale didn't report a key expiry for this device.";
  }
}
