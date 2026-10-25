//! Batch 9 — the guided "become friends and get connected" wizard.
//!
//! Everything here is deliberately pure: the wizard reads what the Tailscale
//! CLI and the Movie Party transport already reported, and decides what stage
//! the user is in, what is standing in the way, and what they should do next.
//! No I/O, no process spawning, no state — that lives in
//! `app_runtime::onboarding`. Keeping the decision function pure is what makes
//! the eight failure states of requirement 16 testable without two machines.
//!
//! Three rules shape the whole module:
//!
//! 1. **"Connected" means a Movie Party handshake.** A tailnet status entry or
//!    a successful `tailscale ping` proves the tunnel exists; it does not prove
//!    the other device is the friend Movie Party agreed to trust.
//! 2. **Tailscale share links are bearer credentials.** Whoever holds the link
//!    holds the access. The wizard recognises them, describes them, and can
//!    open them on request, but never stores, logs, or republishes them.
//! 3. **Not everyone administers their tailnet.** Creating a share or inviting
//!    a user is an administrative act. The wizard detects the local role and
//!    routes a limited user to the person who can actually do it.

use serde::{Deserialize, Serialize};

use crate::friends::FriendshipState;
use crate::network::tailscale::{NodeKeyState, PeerConnectionProbe, TailnetRole};

/// The two roles a person plays in a Movie Party room. Directionality questions
/// ("who may dial whom") are only meaningful relative to these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Side {
    Host,
    Guest,
}

impl Side {
    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "HOST" => Some(Side::Host),
            "GUEST" => Some(Side::Guest),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Side::Host => "HOST",
            Side::Guest => "GUEST",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Side::Host => Side::Guest,
            Side::Guest => Side::Host,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Side::Host => "host",
            Side::Guest => "guest",
        }
    }
}

/// The two ways Tailscale can put the two machines on the same private network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ConnectionMethod {
    /// One machine is shared with one named external user (Tailshare).
    DeviceSharing,
    /// The other device joins the tailnet as a full node.
    TailnetMembership,
}

impl ConnectionMethod {
    pub fn id(self) -> &'static str {
        match self {
            ConnectionMethod::DeviceSharing => "DEVICE_SHARING",
            ConnectionMethod::TailnetMembership => "TAILNET_MEMBERSHIP",
        }
    }

    pub fn from_id(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "DEVICE_SHARING" => Some(ConnectionMethod::DeviceSharing),
            "TAILNET_MEMBERSHIP" => Some(ConnectionMethod::TailnetMembership),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ConnectionMethod::DeviceSharing => "Share this one machine",
            ConnectionMethod::TailnetMembership => "Join the same tailnet",
        }
    }
}

/// What a method actually grants and what it costs, stated in the terms
/// Tailscale itself uses. The wizard shows both lists — an explicit choice
/// needs the limits, not only the benefits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MethodFacts {
    pub method: ConnectionMethod,
    pub summary: String,
    pub grants: Vec<String>,
    pub limits: Vec<String>,
    /// Which person must hold tailnet administration rights to create the
    /// access: the sharer for device sharing, the inviting tailnet's admin for
    /// membership. Neither is Movie Party itself.
    pub admin_needed_by: Side,
    /// The recipient always has to complete Tailscale's own consent/login; the
    /// app can start them on the path but cannot finish it for them.
    pub recipient_must_consent: bool,
    /// Where the admin goes to create it. Opened on request, never automated.
    pub admin_url: &'static str,
}

const TAILSCALE_ADMIN_SHARE_URL: &str = "https://login.tailscale.com/admin/machines";
const TAILSCALE_ADMIN_USERS_URL: &str = "https://login.tailscale.com/admin/users";

pub fn method_facts(method: ConnectionMethod) -> MethodFacts {
    match method {
        ConnectionMethod::DeviceSharing => MethodFacts {
            method,
            summary: "Share only the machine Movie Party runs on with one named person.".to_string(),
            grants: vec![
                "They reach this one machine, nothing else on your tailnet.".to_string(),
                "The share is issued to one named account and can be revoked from the admin console at any time.".to_string(),
                "They keep their own Tailscale account and identity; you never hand over a key or password.".to_string(),
                "No new software: if they already use Tailscale they just accept the share.".to_string(),
            ],
            limits: vec![
                "Creating a share needs tailnet administration rights on YOUR side.".to_string(),
                "A shared machine can receive incoming connections but cannot start connections, so only the guest-to-host direction works.".to_string(),
                "They must still log in and accept the share themselves — acceptance is never automatic.".to_string(),
            ],
            admin_needed_by: Side::Host,
            recipient_must_consent: true,
            admin_url: TAILSCALE_ADMIN_SHARE_URL,
        },
        ConnectionMethod::TailnetMembership => MethodFacts {
            method,
            summary: "Bring the other device into the tailnet as a full node.".to_string(),
            grants: vec![
                "Both devices can start connections to each other, subject to the tailnet's ACLs.".to_string(),
                "Movie Party's own direction rules stop mattering, so live calls are less likely to hit a one-way block.".to_string(),
                "One setup covers every future party between the two devices.".to_string(),
            ],
            limits: vec![
                "A full node can see more than one machine unless the tailnet's ACLs restrict it.".to_string(),
                "Someone must hold tailnet administration rights to send the invite — often not the person using Movie Party.".to_string(),
                "On a work tailnet the policy is somebody else's, and Movie Party cannot change it.".to_string(),
                "They must accept the invitation and sign in with their own Tailscale account.".to_string(),
            ],
            admin_needed_by: Side::Host,
            recipient_must_consent: true,
            admin_url: TAILSCALE_ADMIN_USERS_URL,
        },
    }
}

/// Requirement 4: one wizard, six stages, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WizardStage {
    InviteFriend,
    ChooseMethod,
    TailscaleApproval,
    PeerReachability,
    IdentityVerification,
    FriendReady,
}

pub const WIZARD_STAGES: [WizardStage; 6] = [
    WizardStage::InviteFriend,
    WizardStage::ChooseMethod,
    WizardStage::TailscaleApproval,
    WizardStage::PeerReachability,
    WizardStage::IdentityVerification,
    WizardStage::FriendReady,
];

impl WizardStage {
    pub fn ordinal(self) -> usize {
        WIZARD_STAGES.iter().position(|s| *s == self).unwrap_or(0)
    }

    pub fn next(self) -> Option<WizardStage> {
        WIZARD_STAGES.get(self.ordinal() + 1).copied()
    }

    pub fn is_terminal(self) -> bool {
        self == WizardStage::FriendReady
    }
}

/// Where the Movie Party friendship itself stands, as far as the wizard cares.
/// Derived from Batch 8's trust states, never stored twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InviteStage {
    NotStarted,
    /// We created a signed invite and handed it out.
    Sent,
    /// We were shown an invite and have not decided yet.
    ReceivedAwaitingDecision,
    /// Both sides agreed; trust exists.
    Accepted,
    /// The invite expired or was declined before anyone accepted.
    ExpiredOrDeclined,
    /// This peer is blocked on purpose.
    Blocked,
}

/// One line of the wizard's progress list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckId {
    TailscaleInstalled,
    TailscaleSignedIn,
    NodeKeyValid,
    AdminAbleToGrantAccess,
    FriendInvited,
    MethodChosen,
    PeerVisibleOnNetwork,
    PeerReachableThroughTunnel,
    DirectionWorksForMovieParty,
    MoviePartyTransportHandshake,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckState {
    Passed,
    /// Not true yet, but nothing is wrong — waiting on the other side or on a
    /// refresh.
    Waiting,
    Failed,
    /// Genuinely does not apply (e.g. admin rights when the other person is the
    /// one who has to share).
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub id: CheckId,
    pub state: CheckState,
    /// Stable MP code when the check is not passed, so copy and telemetry agree.
    pub code: Option<String>,
}

impl Check {
    fn new(id: CheckId, state: CheckState, code: Option<&str>) -> Self {
        Self {
            id,
            state,
            code: code.map(|c| c.to_string()),
        }
    }

    fn passed(id: CheckId) -> Self {
        Self::new(id, CheckState::Passed, None)
    }
}

/// Requirement 16: every way this wizard can legitimately be stuck.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OnboardingIssue {
    TailscaleMissing,
    TailscaleDaemonUnreachable,
    LoginRequired,
    NodeKeyExpired,
    PeerOffline,
    PolicyDenied,
    WrongTailnet,
    SharePending,
    InviteRevoked,
    DuplicateFriendRequest,
    NotTailnetAdmin,
    DirectionBlocked,
    HandshakeMissing,
    BlockedByYou,
}

impl OnboardingIssue {
    pub fn code(self) -> &'static str {
        match self {
            OnboardingIssue::TailscaleMissing => "MP-ONB-001",
            OnboardingIssue::TailscaleDaemonUnreachable => "MP-ONB-002",
            OnboardingIssue::LoginRequired => "MP-ONB-003",
            OnboardingIssue::NodeKeyExpired => "MP-ONB-004",
            OnboardingIssue::PeerOffline => "MP-ONB-005",
            OnboardingIssue::PolicyDenied => "MP-ONB-006",
            OnboardingIssue::WrongTailnet => "MP-ONB-007",
            OnboardingIssue::SharePending => "MP-ONB-008",
            OnboardingIssue::InviteRevoked => "MP-ONB-009",
            OnboardingIssue::DuplicateFriendRequest => "MP-ONB-010",
            OnboardingIssue::NotTailnetAdmin => "MP-ONB-011",
            OnboardingIssue::DirectionBlocked => "MP-ONB-012",
            OnboardingIssue::HandshakeMissing => "MP-ONB-013",
            OnboardingIssue::BlockedByYou => "MP-ONB-014",
        }
    }

    pub fn headline(self) -> &'static str {
        match self {
            OnboardingIssue::TailscaleMissing => "Tailscale isn't installed here",
            OnboardingIssue::TailscaleDaemonUnreachable => "Tailscale isn't answering",
            OnboardingIssue::LoginRequired => "Tailscale needs you to sign in",
            OnboardingIssue::NodeKeyExpired => "This device's Tailscale key expired",
            OnboardingIssue::PeerOffline => "Their device is offline",
            OnboardingIssue::PolicyDenied => "Tailscale policy blocks this pair",
            OnboardingIssue::WrongTailnet => "You are on different tailnets",
            OnboardingIssue::SharePending => "Waiting for them to accept in Tailscale",
            OnboardingIssue::InviteRevoked => "The Tailscale invite expired or was revoked",
            OnboardingIssue::DuplicateFriendRequest => "You already asked this friend",
            OnboardingIssue::NotTailnetAdmin => "Your account can't grant this access",
            OnboardingIssue::DirectionBlocked => "Wrong sharing direction for Movie Party",
            OnboardingIssue::HandshakeMissing => "Movie Party hasn't authenticated them yet",
            OnboardingIssue::BlockedByYou => "You blocked this friend",
        }
    }

    /// What the user can actually do next. Never promises an action Movie Party
    /// cannot perform: it never installs, signs in, or changes a tailnet
    /// identity for them.
    pub fn guidance(self) -> &'static str {
        match self {
            OnboardingIssue::TailscaleMissing => {
                "Install Tailscale on this device from tailscale.com, then check again. Movie Party doesn't download it for you."
            }
            OnboardingIssue::TailscaleDaemonUnreachable => {
                "Open the Tailscale app so its service is running, then check again."
            }
            OnboardingIssue::LoginRequired => {
                "Open Tailscale and sign in. Your partner has to sign in and accept on their device too — Movie Party can't do either for you."
            }
            OnboardingIssue::NodeKeyExpired => {
                "Open Tailscale and reconnect this device; that renews its key. Keys expire on the tailnet's schedule, not Movie Party's."
            }
            OnboardingIssue::PeerOffline => {
                "Ask them to open Tailscale on that device and keep it running while you party."
            }
            OnboardingIssue::PolicyDenied => {
                "The tailnet's ACLs must allow traffic between the two devices. If you don't administer the tailnet, ask whoever does."
            }
            OnboardingIssue::WrongTailnet => {
                "Either share this machine with their account, or have them join the tailnet this invite belongs to."
            }
            OnboardingIssue::SharePending => {
                "They need to open the Tailscale link and accept it while signed in with their own account."
            }
            OnboardingIssue::InviteRevoked => {
                "Create a fresh share or invitation. Unused Tailscale invite links expire, and revoking is instant."
            }
            OnboardingIssue::DuplicateFriendRequest => {
                "This invite is still waiting for their decision. Send a new one only if they say the old one expired."
            }
            OnboardingIssue::NotTailnetAdmin => {
                "Your Tailscale login can't create shares or invitations here. Ask the tailnet's admin to share the machine with them, or use a tailnet you own."
            }
            OnboardingIssue::DirectionBlocked => {
                "Movie Party's guest always starts the one connection it makes, so the HOST has to be the shared machine. Share the other way, share mutually, or put both devices in one tailnet."
            }
            OnboardingIssue::HandshakeMissing => {
                "The tunnel looks fine, but Movie Party hasn't authenticated this device yet. Join or host a party once; the handshake happens there."
            }
            OnboardingIssue::BlockedByYou => {
                "Unblock them from the Friends list first. Blocking stops Movie Party trusting that identity."
            }
        }
    }

    pub fn is_blocking(self) -> bool {
        !matches!(
            self,
            OnboardingIssue::DuplicateFriendRequest | OnboardingIssue::SharePending
        )
    }

    /// The stage this issue belongs to, for the "why am I stuck here" line.
    pub fn stage(self) -> WizardStage {
        match self {
            OnboardingIssue::DuplicateFriendRequest | OnboardingIssue::BlockedByYou => {
                WizardStage::InviteFriend
            }
            OnboardingIssue::NotTailnetAdmin => WizardStage::ChooseMethod,
            OnboardingIssue::TailscaleMissing
            | OnboardingIssue::TailscaleDaemonUnreachable
            | OnboardingIssue::LoginRequired
            | OnboardingIssue::NodeKeyExpired
            | OnboardingIssue::InviteRevoked
            | OnboardingIssue::SharePending
            | OnboardingIssue::WrongTailnet => WizardStage::TailscaleApproval,
            OnboardingIssue::PeerOffline | OnboardingIssue::PolicyDenied => {
                WizardStage::PeerReachability
            }
            OnboardingIssue::DirectionBlocked | OnboardingIssue::HandshakeMissing => {
                WizardStage::IdentityVerification
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub id: OnboardingIssue,
    pub code: String,
    pub headline: String,
    pub guidance: String,
    pub blocking: bool,
    pub stage: WizardStage,
}

impl Issue {
    pub fn new(id: OnboardingIssue) -> Self {
        Self {
            id,
            code: id.code().to_string(),
            headline: id.headline().to_string(),
            guidance: id.guidance().to_string(),
            blocking: id.is_blocking(),
            stage: id.stage(),
        }
    }
}

/// Requirement 11: are the two machines in one tailnet, or is one host machine
/// shared across tailnets?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PeerTopology {
    /// Both nodes are ordinary members of the same tailnet.
    SameTailnet,
    /// The other node reached this tailnet through sharing rather than
    /// membership. `sharer` is the side whose tailnet owns the share.
    SharedAcrossTailnets { sharer: Side },
    /// The local CLI did not report enough to compare.
    Unknown,
}

/// Requirement 12/14: whether the sharing that exists can carry the one
/// connection Movie Party makes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DirectionVerdict {
    /// Membership (or any setup where both nodes can start connections): the
    /// direction question doesn't arise.
    BothDirectionsAvailable,
    /// The host's machine is the shared one, so the guest dialling the host is
    /// the allowed direction.
    Compatible,
    /// The share runs the wrong way: only host→guest would be allowed, and
    /// Movie Party never dials that way.
    Incompatible {
        /// What to do about it, in one sentence, so the UI can print it.
        remedy: String,
    },
    /// Not enough local evidence to decide.
    Unknown,
}

impl DirectionVerdict {
    pub fn blocks_progress(&self) -> bool {
        matches!(self, DirectionVerdict::Incompatible { .. })
    }
}

/// The transport flows a party depends on, and which side starts each one.
/// Measured from this codebase, not assumed: `start_host` binds the QUIC
/// server and `join_party` is the only client connect in production, so every
/// flow below is carried by the single connection the guest opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportFlow {
    pub name: &'static str,
    pub starts_from: Side,
    /// Whether the flow needs the opposite direction as well.
    pub needs_reverse_initiation: bool,
    pub evidence: &'static str,
}

pub const TRANSPORT_FLOWS: [TransportFlow; 5] = [
    TransportFlow {
        name: "Room control (QUIC streams)",
        starts_from: Side::Guest,
        needs_reverse_initiation: false,
        evidence: "AUTOMATED_VERIFIED",
    },
    TransportFlow {
        name: "Movie file streaming",
        starts_from: Side::Guest,
        needs_reverse_initiation: false,
        evidence: "AUTOMATED_VERIFIED",
    },
    TransportFlow {
        name: "Chat",
        starts_from: Side::Guest,
        needs_reverse_initiation: false,
        evidence: "AUTOMATED_VERIFIED",
    },
    TransportFlow {
        name: "Live-call signalling (SDP + ICE)",
        starts_from: Side::Guest,
        needs_reverse_initiation: false,
        evidence: "AUTOMATED_VERIFIED",
    },
    TransportFlow {
        name: "Live-call media (WebRTC UDP)",
        starts_from: Side::Guest,
        needs_reverse_initiation: true,
        evidence: "UNVERIFIED_NEEDS_TWO_DEVICES",
    },
];

/// Requirement 13, stated honestly: what has actually been demonstrated about
/// this directionality, as opposed to reasoned from the code and the docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DirectionEvidence {
    /// Derived by reading this repo's dial topology plus Tailscale's published
    /// sharing rules. No two-machine run has confirmed it.
    ReasonedFromCodeAndDocs,
    /// Confirmed by a real host/guest pair across a share.
    LiveVerifiedTwoDevices,
}

pub const fn direction_evidence() -> DirectionEvidence {
    DirectionEvidence::ReasonedFromCodeAndDocs
}

/// Requirement 15: what "Connected" is allowed to mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Connectedness {
    /// Nothing has proved anything yet.
    NotAttempted,
    /// The tunnel is fine — a status entry or a ping — but Movie Party has not
    /// authenticated the device. Never rendered as "Connected".
    TunnelReachableOnly,
    /// A real Movie Party transport handshake completed and verified the peer's
    /// device identity. This is the only connected state.
    MoviePartyHandshaked,
}

impl Connectedness {
    pub fn is_connected(self) -> bool {
        self == Connectedness::MoviePartyHandshaked
    }

    pub fn label(self) -> &'static str {
        match self {
            Connectedness::NotAttempted => "Not connected",
            Connectedness::TunnelReachableOnly => "Tunnel only — not Movie Party",
            Connectedness::MoviePartyHandshaked => "Connected",
        }
    }
}

/// Proof that Movie Party itself authenticated a peer: the room handshake where
/// the guest's join secret, device id and ed25519 signature were all verified
/// (or, on the guest side, where the host's certificate fingerprint and device
/// identity were). A `tailscale ping` reply cannot produce this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandshakeProof {
    pub device_id: String,
    /// The tailnet DNS name the peer was reached at, when known.
    pub peer_key: Option<String>,
    /// Which mechanism produced the proof; printed in the UI so the claim is
    /// checkable rather than decorative.
    pub verified_by: &'static str,
    pub at_ms: i64,
}

pub const HANDSHAKE_MECHANISM: &str =
    "QUIC room handshake: join secret + device id + ed25519 signature verified";

/// The guest's view of the same handshake. It is genuinely authenticated — the
/// connection is pinned to the certificate fingerprint the invite carried and
/// the host answered with the device id the invite named — but the guest is not
/// proving its own signature here, so the two sides must not share one label.
pub const GUEST_HANDSHAKE_MECHANISM: &str =
    "QUIC AuthAccept from the device id the invite named, over a connection pinned to the invite's certificate fingerprint";

/// The gate itself: a handshake proof is necessary and nothing else suffices.
pub fn connectedness(proof: Option<&HandshakeProof>, tunnel_reachable: bool) -> Connectedness {
    if proof.is_some() {
        return Connectedness::MoviePartyHandshaked;
    }
    if tunnel_reachable {
        return Connectedness::TunnelReachableOnly;
    }
    Connectedness::NotAttempted
}

/// Everything the reducer needs, already observed by the runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardingInputs {
    /// Which role the local user is playing in the party they are setting up.
    pub my_side: Side,
    pub method: Option<ConnectionMethod>,
    pub invite: InviteStage,
    /// `tailscale status --json` reading of the local device.
    pub tailscale_installed: bool,
    pub daemon_answered: bool,
    pub signed_in: bool,
    pub role: TailnetRole,
    pub node_key: NodeKeyState,
    /// What the tailnet says about the friend's device.
    pub peer_visible: bool,
    pub peer_online: bool,
    pub peer_same_tailnet: Option<bool>,
    pub peer_shared_in: Option<bool>,
    /// The last real `tailscale ping` result, if one has run.
    pub probe_reachable: Option<bool>,
    pub probe_message: Option<String>,
    /// Requirement 16 additions the runtime can observe.
    pub share_link_seen: bool,
    pub share_invite_reported_expired: bool,
    pub duplicate_invite_pending: bool,
    pub friendship: Option<FriendshipState>,
    pub handshake: Option<HandshakeProof>,
}

impl OnboardingInputs {
    /// A clean starting point: nothing observed, nothing attempted.
    pub fn start(my_side: Side) -> Self {
        Self {
            my_side,
            method: None,
            invite: InviteStage::NotStarted,
            tailscale_installed: false,
            daemon_answered: false,
            signed_in: false,
            role: TailnetRole::Unknown,
            node_key: NodeKeyState::Unknown,
            peer_visible: false,
            peer_online: false,
            peer_same_tailnet: None,
            peer_shared_in: None,
            probe_reachable: None,
            probe_message: None,
            share_link_seen: false,
            share_invite_reported_expired: false,
            duplicate_invite_pending: false,
            friendship: None,
            handshake: None,
        }
    }
}

/// The wizard's whole visible state, produced from `OnboardingInputs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingProgress {
    pub stage: WizardStage,
    pub method: Option<ConnectionMethod>,
    pub checks: Vec<Check>,
    pub issues: Vec<Issue>,
    pub topology: PeerTopology,
    pub direction: DirectionVerdict,
    pub connectedness: Connectedness,
    /// Whether a handshake proof from this process backs the connectedness
    /// claim. The view may only call the pair "Connected" when both hold, which
    /// is why the flag travels with the verdict instead of being re-derived.
    pub has_handshake_proof: bool,
    pub finished: bool,
    /// True when the wizard is stuck on something the user must go do outside
    /// Movie Party; drives "waiting" vs "broken" styling.
    pub waiting_on_partner: bool,
}

/// Requirement 12 stated as a constant, because it is the fact the rest of the
/// direction logic is built on: Movie Party's guest is the only side that ever
/// starts a transport connection. Tailscale's rule ("a shared machine can
/// receive incoming connections but cannot start connections") therefore works
/// for Movie Party only when the *host's* machine is the shared one.
pub const MOVIE_PARTY_CONNECTION_INITIATOR: Side = Side::Guest;

/// Reduces observations into stage + checks + issues. Pure and total: it never
/// invents a pass, and anything it cannot observe is `Waiting` or `Unknown`.
pub fn advance(inputs: &OnboardingInputs) -> OnboardingProgress {
    let mut checks = Vec::new();
    let mut issues: Vec<Issue> = Vec::new();

    // ── Local Tailscale standing ──────────────────────────────────────
    if inputs.tailscale_installed {
        checks.push(Check::passed(CheckId::TailscaleInstalled));
    } else {
        checks.push(Check::new(
            CheckId::TailscaleInstalled,
            CheckState::Failed,
            Some(OnboardingIssue::TailscaleMissing.code()),
        ));
        issues.push(Issue::new(OnboardingIssue::TailscaleMissing));
    }

    // ── Local Tailscale standing: installed, answering, signed in ────
    let daemon_down = inputs.tailscale_installed && !inputs.daemon_answered;
    if daemon_down || !inputs.tailscale_installed {
        if inputs.tailscale_installed {
            checks.push(Check::new(
                CheckId::TailscaleSignedIn,
                CheckState::Waiting,
                Some(OnboardingIssue::TailscaleDaemonUnreachable.code()),
            ));
            issues.push(Issue::new(OnboardingIssue::TailscaleDaemonUnreachable));
        } else {
            checks.push(Check::new(
                CheckId::TailscaleSignedIn,
                CheckState::Waiting,
                Some(OnboardingIssue::TailscaleMissing.code()),
            ));
        }
    } else if !inputs.signed_in {
        checks.push(Check::new(
            CheckId::TailscaleSignedIn,
            CheckState::Failed,
            Some(OnboardingIssue::LoginRequired.code()),
        ));
        issues.push(Issue::new(OnboardingIssue::LoginRequired));
    } else {
        checks.push(Check::passed(CheckId::TailscaleSignedIn));
    }

    match inputs.node_key {
        NodeKeyState::Valid | NodeKeyState::Unknown => {
            checks.push(if inputs.node_key == NodeKeyState::Unknown {
                Check::new(CheckId::NodeKeyValid, CheckState::Waiting, None)
            } else {
                Check::passed(CheckId::NodeKeyValid)
            });
        }
        NodeKeyState::ExpiringSoon => {
            checks.push(Check::new(
                CheckId::NodeKeyValid,
                CheckState::Passed,
                Some(OnboardingIssue::NodeKeyExpired.code()),
            ));
        }
        NodeKeyState::Expired => {
            checks.push(Check::new(
                CheckId::NodeKeyValid,
                CheckState::Failed,
                Some(OnboardingIssue::NodeKeyExpired.code()),
            ));
            issues.push(Issue::new(OnboardingIssue::NodeKeyExpired));
        }
    }

    // ── Who can grant the access ──────────────────────────────────────
    let admin_check = match inputs.method {
        // Only the sharer's side needs administration, so the check is only
        // meaningful for the method the user actually chose.
        None => Check::new(CheckId::AdminAbleToGrantAccess, CheckState::Waiting, None),
        Some(method) => match inputs.role {
            TailnetRole::Owner | TailnetRole::Admin => {
                Check::passed(CheckId::AdminAbleToGrantAccess)
            }
            // Unknown is reported as waiting with no code: Movie Party must not
            // tell an administrator they are a limited user.
            TailnetRole::Unknown => {
                Check::new(CheckId::AdminAbleToGrantAccess, CheckState::Waiting, None)
            }
            TailnetRole::LimitedUser => {
                if method == ConnectionMethod::DeviceSharing && inputs.my_side == Side::Host {
                    issues.push(Issue::new(OnboardingIssue::NotTailnetAdmin));
                    Check::new(
                        CheckId::AdminAbleToGrantAccess,
                        CheckState::Failed,
                        Some(OnboardingIssue::NotTailnetAdmin.code()),
                    )
                } else {
                    Check::new(CheckId::AdminAbleToGrantAccess, CheckState::Waiting, None)
                }
            }
        },
    };
    checks.push(admin_check);

    // ── The friendship itself (Batch 8) ───────────────────────────────
    let invited = inputs.invite == InviteStage::Accepted;
    let friend_blocked = inputs.friendship == Some(FriendshipState::Blocked)
        || inputs.invite == InviteStage::Blocked;
    if friend_blocked {
        issues.push(Issue::new(OnboardingIssue::BlockedByYou));
        checks.push(Check::new(
            CheckId::FriendInvited,
            CheckState::Failed,
            Some(OnboardingIssue::BlockedByYou.code()),
        ));
    } else if invited {
        checks.push(Check::passed(CheckId::FriendInvited));
    } else {
        checks.push(Check::new(
            CheckId::FriendInvited,
            CheckState::Waiting,
            Some(OnboardingIssue::DuplicateFriendRequest.code()),
        ));
    }
    if inputs.duplicate_invite_pending && !invited {
        issues.push(Issue::new(OnboardingIssue::DuplicateFriendRequest));
    }
    if inputs.invite == InviteStage::ExpiredOrDeclined {
        issues.push(Issue::new(OnboardingIssue::InviteRevoked));
    }

    // ── Method choice ─────────────────────────────────────────────────
    checks.push(if inputs.method.is_some() {
        Check::passed(CheckId::MethodChosen)
    } else {
        Check::new(CheckId::MethodChosen, CheckState::Waiting, None)
    });

    // ── Topology + reachability ───────────────────────────────────────
    let topology = classify_topology(inputs);
    let direction = classify_direction(inputs, &topology);
    if direction.blocks_progress() {
        issues.push(Issue::new(OnboardingIssue::DirectionBlocked));
    }

    if inputs.peer_visible {
        checks.push(Check::passed(CheckId::PeerVisibleOnNetwork));
    } else {
        checks.push(Check::new(
            CheckId::PeerVisibleOnNetwork,
            CheckState::Waiting,
            Some(OnboardingIssue::SharePending.code()),
        ));
        // A peer we cannot see at all is the normal state right after the share
        // is created: they still have to accept it in Tailscale.
        if inputs.share_link_seen || inputs.method.is_some() {
            issues.push(Issue::new(OnboardingIssue::SharePending));
        }
    }

    if inputs.peer_visible && !inputs.peer_online {
        issues.push(Issue::new(OnboardingIssue::PeerOffline));
    }

    match inputs.probe_reachable {
        Some(true) => checks.push(Check::passed(CheckId::PeerReachableThroughTunnel)),
        Some(false) => {
            let issue = policy_denied_from_probe(inputs.probe_message.as_deref());
            if issue != OnboardingIssue::PeerOffline && !issues_contains(&issues, issue) {
                issues.push(Issue::new(issue));
            }
            checks.push(Check::new(
                CheckId::PeerReachableThroughTunnel,
                CheckState::Failed,
                Some(issue.code()),
            ));
        }
        None => checks.push(Check::new(
            CheckId::PeerReachableThroughTunnel,
            CheckState::Waiting,
            None,
        )),
    }

    checks.push(if direction.blocks_progress() {
        Check::new(
            CheckId::DirectionWorksForMovieParty,
            CheckState::Failed,
            Some(OnboardingIssue::DirectionBlocked.code()),
        )
    } else if matches!(direction, DirectionVerdict::Unknown) {
        Check::new(
            CheckId::DirectionWorksForMovieParty,
            CheckState::Waiting,
            None,
        )
    } else {
        Check::passed(CheckId::DirectionWorksForMovieParty)
    });

    // ── The only thing that may be called Connected ──────────────────
    let tunnel_reachable = inputs.probe_reachable.unwrap_or(false);
    let connectedness = connectedness(inputs.handshake.as_ref(), tunnel_reachable);
    match &connectedness {
        Connectedness::MoviePartyHandshaked => {
            checks.push(Check::passed(CheckId::MoviePartyTransportHandshake))
        }
        Connectedness::TunnelReachableOnly => {
            issues.push(Issue::new(OnboardingIssue::HandshakeMissing));
            checks.push(Check::new(
                CheckId::MoviePartyTransportHandshake,
                CheckState::Failed,
                Some(OnboardingIssue::HandshakeMissing.code()),
            ));
        }
        Connectedness::NotAttempted => checks.push(Check::new(
            CheckId::MoviePartyTransportHandshake,
            CheckState::Waiting,
            None,
        )),
    }

    if inputs.share_invite_reported_expired {
        issues.push(Issue::new(OnboardingIssue::InviteRevoked));
    }
    dedupe_issues(&mut issues);

    let stage = resolved_stage(inputs, &issues, invited, &connectedness, &direction);
    let finished = stage == WizardStage::FriendReady;
    let waiting_on_partner = issues.iter().any(|issue| {
        matches!(
            issue.id,
            OnboardingIssue::SharePending
                | OnboardingIssue::PeerOffline
                | OnboardingIssue::DuplicateFriendRequest
                | OnboardingIssue::LoginRequired
        )
    });

    OnboardingProgress {
        stage,
        method: inputs.method,
        checks,
        issues,
        topology,
        direction,
        connectedness,
        has_handshake_proof: inputs.handshake.is_some(),
        finished,
        waiting_on_partner,
    }
}

fn issues_contains(issues: &[Issue], issue: OnboardingIssue) -> bool {
    issues.iter().any(|existing| existing.id == issue)
}

fn dedupe_issues(issues: &mut Vec<Issue>) {
    issues.dedup_by(|a, b| a.id == b.id);
}

/// Maps the CLI's ping failure text onto the taxonomy. Only claims
/// `PolicyDenied` on text Tailscale actually produces for a denied route; a
/// silent peer stays `PeerOffline`.
fn policy_denied_from_probe(message: Option<&str>) -> OnboardingIssue {
    let text = message.unwrap_or_default().to_ascii_lowercase();
    if text.contains("policy") || text.contains("acl") || text.contains("denied") {
        OnboardingIssue::PolicyDenied
    } else if text.contains("not in your tailscale network") || text.contains("no matching peer") {
        OnboardingIssue::WrongTailnet
    } else {
        OnboardingIssue::PeerOffline
    }
}

/// Requirement 11: same tailnet, or one machine shared across tailnets.
fn classify_topology(inputs: &OnboardingInputs) -> PeerTopology {
    match (inputs.peer_same_tailnet, inputs.peer_shared_in) {
        (Some(true), _) => PeerTopology::SameTailnet,
        (Some(false), _) => PeerTopology::SharedAcrossTailnets {
            // The node we are looking at came from another tailnet, so its own
            // tailnet is the one that granted the share.
            sharer: inputs.my_side.other(),
        },
        (None, Some(true)) => PeerTopology::SharedAcrossTailnets {
            sharer: inputs.my_side.other(),
        },
        _ => PeerTopology::Unknown,
    }
}

/// Requirement 12, applied to Movie Party's actual dial topology.
///
/// Tailscale: "A shared machine can receive incoming connections but cannot
/// start connections." Movie Party: the guest always starts the connection.
/// So a share works only when the host's machine is the shared one.
fn classify_direction(inputs: &OnboardingInputs, topology: &PeerTopology) -> DirectionVerdict {
    match topology {
        PeerTopology::SameTailnet => DirectionVerdict::BothDirectionsAvailable,
        PeerTopology::Unknown => DirectionVerdict::Unknown,
        PeerTopology::SharedAcrossTailnets { sharer } => {
            if *sharer == inputs.my_side.other() {
                // We are the recipient of a share: the other machine is the
                // shared one. That is only useful if the shared machine is the
                // host, because the guest is the side that dials.
                let shared_machine_is_host = inputs.my_side == Side::Guest;
                if shared_machine_is_host {
                    DirectionVerdict::Compatible
                } else {
                    DirectionVerdict::Incompatible {
                        remedy: INCOMPATIBLE_SHARE_REMEDY.to_string(),
                    }
                }
            } else {
                DirectionVerdict::Unknown
            }
        }
    }
}

/// Requirement 14: the documented alternatives when one-way sharing is not
/// enough, offered instead of asking the user to fight the quarantine.
pub const INCOMPATIBLE_SHARE_REMEDY: &str =
    "You shared their machine into your tailnet, but Movie Party's guest is the side that dials. Use one of: (1) the HOST shares their machine with the GUEST's account; (2) both sides share with each other (mutual sharing); (3) invite them as a node of one tailnet so both directions work.";

/// Requirement 12/13 as one answer, for the UI's explainer and for tests.
pub fn direction_summary(my_side: Side, topology: PeerTopology) -> (DirectionVerdict, String) {
    let inputs = OnboardingInputs {
        my_side,
        peer_same_tailnet: match topology {
            PeerTopology::SameTailnet => Some(true),
            PeerTopology::SharedAcrossTailnets { .. } => Some(false),
            PeerTopology::Unknown => None,
        },
        peer_shared_in: match topology {
            PeerTopology::SharedAcrossTailnets { .. } => Some(true),
            _ => None,
        },
        ..OnboardingInputs::start(my_side)
    };
    let verdict = classify_direction(&inputs, &topology);
    let text = match &verdict {
        DirectionVerdict::BothDirectionsAvailable => {
            "Both devices are members of the same tailnet, so either side can start a connection. Movie Party's guest-initiated design works unchanged.".to_string()
        }
        DirectionVerdict::Compatible => {
            "The host's machine is the shared one. Tailscale lets a shared machine receive connections, and Movie Party's guest is the only side that starts one — so the direction matches.".to_string()
        }
        DirectionVerdict::Incompatible { remedy } => remedy.clone(),
        DirectionVerdict::Unknown => {
            "This device's Tailscale status does not say how the other machine is attached, so Movie Party cannot promise the direction works.".to_string()
        }
    };
    (verdict, text)
}

/// Decides the stage to show. Advances only past gates that are actually
/// satisfied, and never past a blocking issue for that stage. The order is
/// requirement 4's, so an unanswered invitation cannot be skipped over by
/// having Tailscale already in good shape.
fn resolved_stage(
    inputs: &OnboardingInputs,
    issues: &[Issue],
    invited: bool,
    connectedness: &Connectedness,
    direction: &DirectionVerdict,
) -> WizardStage {
    let blocked_at = |stage: WizardStage| {
        issues
            .iter()
            .any(|issue| issue.blocking && issue.stage == stage)
    };
    // Stage 1: the invitation has to be accepted before the wizard moves on. A
    // blocked or outstanding identity is not accepted, so nobody can configure a
    // connection to a device that has not agreed to be friends — and a blocked
    // one holds here, where the unblock path is offered.
    if !invited {
        return WizardStage::InviteFriend;
    }
    if inputs.method.is_none() {
        return WizardStage::ChooseMethod;
    }

    // Stages 3 and 4 belong to Tailscale's own world.
    if blocked_at(WizardStage::TailscaleApproval) || !inputs.peer_visible {
        return WizardStage::TailscaleApproval;
    }
    if blocked_at(WizardStage::PeerReachability)
        || inputs.probe_reachable != Some(true)
        || direction.blocks_progress()
    {
        return WizardStage::PeerReachability;
    }
    // The final gate is the Movie Party handshake — a reachable tunnel alone
    // stops the wizard here.
    if !connectedness.is_connected() || blocked_at(WizardStage::IdentityVerification) {
        return WizardStage::IdentityVerification;
    }
    WizardStage::FriendReady
}

/// The wizard's own report of what a probe means, so the UI can print the same
/// words the reducer acted on.
pub fn describe_probe(probe: &PeerConnectionProbe) -> String {
    if probe.reachable {
        format!(
            "Tailscale reached the device through the tunnel ({}{}). This proves the tunnel, not Movie Party trust.",
            probe.path.as_deref().unwrap_or("unknown path"),
            probe
                .latency_ms
                .map_or_else(String::new, |ms| format!(", {ms} ms"))
        )
    } else {
        probe.message.clone()
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Requirement 7/8: the share link, handled as the bearer credential it is.
// ──────────────────────────────────────────────────────────────────────────

/// Hosts that legitimately carry a Tailscale invitation, share, or login link.
/// A share link is issued from the tailnet's own `…​.login.tailscale.com` host,
/// so any subdomain of it counts.
const TAILSCALE_ACCOUNT_HOSTS: [&str; 2] = ["login.tailscale.com", "web.tailscale.com"];

fn is_tailscale_host(host: &str) -> bool {
    TAILSCALE_ACCOUNT_HOSTS.contains(&host) || host.ends_with(".login.tailscale.com")
}

/// The path prefix of a one-time invitation/share link. Tailscale's own
/// "invite any user" flow issues `…/a/<token>` URLs; the token is the secret.
const TAILSCALE_SECRET_PATH: &str = "/a/";

/// What the wizard may say about a pasted link without keeping it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareLinkSummary {
    /// `TAILSCALE_INVITE` for a recognised `…/a/<token>` link.
    pub kind: ShareLinkKind,
    /// Non-reversible, short id derived from the token so two pastes can be
    /// compared, and the UI can confirm *which* link it saw, without ever being
    /// able to reproduce it.
    pub token_fingerprint: String,
    /// True when the link's tailnet handle matches this device's tailnet
    /// suffix — i.e. the user may already be on that tailnet.
    pub tailnet_handle_matches_local: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ShareLinkKind {
    /// A recognised Tailscale one-time invite/share link.
    TailscaleInvite,
    /// A Tailscale login (not a bearer share) URL.
    TailscaleLogin,
    /// Anything else: the wizard refuses to treat it as a share link.
    NotATailscaleLink,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ShareLinkError {
    #[error("MP-ONB-020 that is not a Tailscale link Movie Party can recognise")]
    UnrecognisedLink,
    #[error("MP-ONB-021 a Tailscale link must use https")]
    InsecureScheme,
    #[error("MP-ONB-022 the link is missing its one-time token")]
    MissingToken,
}

/// Classifies a pasted link and reports only non-secret facts about it. The
/// input is never stored by this function and its token never leaves it.
pub fn inspect_share_link(
    raw: &str,
    local_dns_suffix: Option<&str>,
) -> Result<ShareLinkSummary, ShareLinkError> {
    let parsed = url::Url::parse(raw.trim()).map_err(|_| ShareLinkError::UnrecognisedLink)?;
    if parsed.scheme() != "https" {
        return Err(ShareLinkError::InsecureScheme);
    }
    let host = parsed.host_str().ok_or(ShareLinkError::UnrecognisedLink)?;
    if !is_tailscale_host(host) {
        return Err(ShareLinkError::UnrecognisedLink);
    }

    let path = parsed.path();
    if path.contains(TAILSCALE_SECRET_PATH) {
        // The token is the segment immediately after `/a/`; `/a/` on its own is
        // not a link anyone can use.
        let token = path
            .split_once(TAILSCALE_SECRET_PATH)
            .and_then(|(_, rest)| rest.split('/').next())
            .filter(|value| !value.trim().is_empty())
            .ok_or(ShareLinkError::MissingToken)?;
        let handle = host
            .strip_suffix(".login.tailscale.com")
            .filter(|h| !h.is_empty() && *h != "login")
            .map(|h| h.split('.').next().unwrap_or(h).to_string());
        let tailnet_handle_matches_local = match (handle, local_dns_suffix) {
            (Some(handle), Some(suffix)) => {
                let suffix_head = suffix.split('.').next().unwrap_or_default();
                Some(handle.eq_ignore_ascii_case(suffix_head))
            }
            _ => None,
        };
        return Ok(ShareLinkSummary {
            kind: ShareLinkKind::TailscaleInvite,
            token_fingerprint: fingerprint_share_token(token),
            tailnet_handle_matches_local,
        });
    }

    Ok(ShareLinkSummary {
        kind: ShareLinkKind::TailscaleLogin,
        token_fingerprint: String::new(),
        tailnet_handle_matches_local: None,
    })
}

/// A short, purpose-tagged digest of a share token. One-way, so the wizard can
/// say "the link ending in `1a2b3c4d`" without holding the link.
pub fn fingerprint_share_token(token: &str) -> String {
    // A keyed hash with a fixed, app-known key: one-way, so the wizard can
    // name the link it saw without being able to reproduce it.
    let key = blake3::derive_key("movieparty-share-link-fingerprint-v1", b"v1");
    let hex = blake3::keyed_hash(&key, token.as_bytes()).to_hex();
    hex[..8].to_string()
}

/// A pasted Tailscale link, held only in memory for as long as the wizard needs
/// it. Its `Debug` prints the fingerprint instead of the token, so a stray
/// `{:?}` in a trace or an error chain cannot leak the bearer credential. It is
/// never written to the database, the diagnostics bundle, or the app snapshot.
#[derive(Clone)]
pub struct PastedShareLink {
    raw: String,
    pub summary: ShareLinkSummary,
}

impl PastedShareLink {
    pub fn capture(raw: &str, local_dns_suffix: Option<&str>) -> Result<Self, ShareLinkError> {
        let summary = inspect_share_link(raw, local_dns_suffix)?;
        Ok(Self {
            raw: raw.trim().to_string(),
            summary,
        })
    }

    /// The only reader that returns the token, and only so the app can open the
    /// link in the user's browser when they explicitly ask for it.
    pub fn url(&self) -> &str {
        &self.raw
    }

    pub fn fingerprint(&self) -> &str {
        &self.summary.token_fingerprint
    }
}

impl core::fmt::Debug for PastedShareLink {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("PastedShareLink")
            .field("kind", &self.summary.kind)
            .field("fingerprint", &self.summary.token_fingerprint)
            .field("raw", &"[REDACTED]")
            .finish()
    }
}

/// True when a URL text looks like a bearer Tailscale invite, so anything about
/// to be written to a log or a message can be scrubbed first.
pub fn looks_like_tailscale_secret_url(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("login.tailscale.com/a/") || lower.contains("tailscale.com/login/a/")
}

/// Redacts any Tailscale one-time token inside `text`. Used on every message
/// that came out of the Tailscale CLI or a pasted value, because requirement 8
/// says these links must never be logged or stored indefinitely — and a CLI
/// error line is exactly where one can appear.
pub fn scrub_tailscale_secrets(text: &str) -> String {
    let mut scrubbed = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let Some(rest) = text[index..].find("login.tailscale.com/") else {
            scrubbed.push_str(&text[index..]);
            break;
        };
        let absolute = index + rest;
        scrubbed.push_str(&text[index..absolute]);
        // Keep the host, drop everything from the path onwards up to whitespace
        // so the token cannot survive into a log line.
        let tail = &text[absolute..];
        let end = tail
            .find(char::is_whitespace)
            .or_else(|| tail.find('"'))
            .unwrap_or(tail.len());
        let secret = &tail[..end];
        if secret.contains("/a/") || secret.contains("/start") {
            scrubbed.push_str("login.tailscale.com/[REDACTED]");
        } else {
            scrubbed.push_str(secret);
        }
        index = absolute + end;
    }
    scrubbed
}

/// The invitation package (requirement 9): the signed Movie Party invite plus
/// the Tailscale steps for the chosen method. It is the thing a user pastes into
/// a message to their friend — so it must never carry a bearer share link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitationPackage {
    pub text: String,
    pub method: ConnectionMethod,
    pub expires_at_ms: i64,
    /// One-time by construction: the Movie Party invite's nonce is consumed on
    /// acceptance and it stops working at `expires_at_ms`.
    pub one_time: bool,
    /// Self-audit flag asserted by tests: the package never embeds a share link.
    pub carries_no_tailscale_secret: bool,
}

/// Builds the package. `tailscale_share_link`, if the caller passes one, is
/// deliberately dropped — the friend gets Tailscale's own delivery to their own
/// account instead of a copy that outlives the conversation.
pub fn build_invitation_package(
    movie_party_invite: &str,
    method: ConnectionMethod,
    display_name: &str,
    expires_at_ms: i64,
    tailscale_share_link: Option<&str>,
) -> InvitationPackage {
    let steps: Vec<String> = match method {
        ConnectionMethod::DeviceSharing => vec![
            "1. Your host sends a Tailscale share of that one machine to your own Tailscale account.".to_string(),
            "2. Open the link Tailscale sends you and accept it while signed in — the share is not applied by itself.".to_string(),
            "3. Keep Tailscale running on this device during the party.".to_string(),
        ],
        ConnectionMethod::TailnetMembership => vec![
            "1. Your host (or their tailnet admin) invites your Tailscale account to the network.".to_string(),
            "2. Open the invitation Tailscale sends you and accept it while signed in.".to_string(),
            "3. Install or open Tailscale on this device and connect.".to_string(),
        ],
    };
    let mut text = format!(
        "Movie Party invite from {}\n\n{}\n\nHow to connect with Tailscale ({}):\n{}\n\nThis Movie Party invite expires and works once only. Tailscale sends its own link for step 1-2 directly to you; Movie Party never puts share links in this text.",
        display_name.trim(),
        movie_party_invite.trim(),
        method.label(),
        steps.join("\n")
    );
    if let Some(link) = tailscale_share_link {
        // Mention that a link exists without reproducing it, so the friend knows
        // what to expect and the secret does not spread.
        text.push_str(&format!(
            "\n(A Tailscale link was prepared for you — it is delivered by Tailscale, fingerprint {}.)",
            fingerprint_share_token(
                link.rsplit('/').find(|s| !s.is_empty()).unwrap_or_default()
            )
        ));
    }
    InvitationPackage {
        carries_no_tailscale_secret: !looks_like_tailscale_secret_url(&text),
        text,
        method,
        expires_at_ms,
        one_time: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::friends::FRIEND_INVITE_TTL_MS;

    const NOW: i64 = 1_767_225_600_000; // 2026-01-01T00:00:00Z

    fn inputs(side: Side) -> OnboardingInputs {
        let mut inputs = OnboardingInputs::start(side);
        inputs.tailscale_installed = true;
        inputs.daemon_answered = true;
        inputs.signed_in = true;
        inputs.role = TailnetRole::Admin;
        inputs.node_key = NodeKeyState::Valid;
        inputs
    }

    fn proof() -> HandshakeProof {
        HandshakeProof {
            device_id: "device-peer".to_string(),
            peer_key: Some("rahul-mac.tailc930b7.ts.net.".to_string()),
            verified_by: HANDSHAKE_MECHANISM,
            at_ms: NOW,
        }
    }

    fn passed(progress: &OnboardingProgress, id: CheckId) -> bool {
        progress
            .checks
            .iter()
            .any(|check| check.id == id && check.state == CheckState::Passed)
    }

    fn has_issue(progress: &OnboardingProgress, id: OnboardingIssue) -> bool {
        progress.issues.iter().any(|issue| issue.id == id)
    }

    /// The full happy path, ending in the only state allowed to say Connected.
    #[test]
    fn walks_all_six_stages_to_ready() {
        // The guest's view: the host's machine is the one shared into this
        // tailnet, which is the direction Movie Party can actually use.
        let mut step = inputs(Side::Guest);
        assert_eq!(advance(&step).stage, WizardStage::InviteFriend);

        step.invite = InviteStage::Accepted;
        assert_eq!(advance(&step).stage, WizardStage::ChooseMethod);

        step.method = Some(ConnectionMethod::DeviceSharing);
        assert_eq!(advance(&step).stage, WizardStage::TailscaleApproval);

        step.peer_visible = true;
        step.peer_online = true;
        step.peer_same_tailnet = Some(false);
        step.peer_shared_in = Some(true);
        assert_eq!(advance(&step).stage, WizardStage::PeerReachability);

        step.probe_reachable = Some(true);
        assert_eq!(advance(&step).stage, WizardStage::IdentityVerification);

        step.handshake = Some(proof());
        let progress = advance(&step);
        assert_eq!(progress.stage, WizardStage::FriendReady);
        assert!(progress.finished);
        assert_eq!(progress.connectedness, Connectedness::MoviePartyHandshaked);
        assert!(progress.connectedness.is_connected());
        assert!(progress.has_handshake_proof);
        assert!(
            progress.issues.is_empty(),
            "unexpected issues: {:?}",
            progress.issues
        );
        assert_eq!(
            WIZARD_STAGES.iter().map(|s| s.ordinal()).sum::<usize>(),
            (0..6).sum::<usize>()
        );
    }

    #[test]
    fn stage_order_is_the_wizards_contract() {
        assert_eq!(WIZARD_STAGES[0], WizardStage::InviteFriend);
        assert_eq!(WIZARD_STAGES[1], WizardStage::ChooseMethod);
        assert_eq!(WIZARD_STAGES[2], WizardStage::TailscaleApproval);
        assert_eq!(WIZARD_STAGES[3], WizardStage::PeerReachability);
        assert_eq!(WIZARD_STAGES[4], WizardStage::IdentityVerification);
        assert_eq!(WIZARD_STAGES[5], WizardStage::FriendReady);
        assert_eq!(
            WizardStage::IdentityVerification.next(),
            Some(WizardStage::FriendReady)
        );
        assert_eq!(WizardStage::FriendReady.next(), None);
        assert!(WizardStage::FriendReady.is_terminal());
        assert!(!WizardStage::PeerReachability.is_terminal());
    }

    /// Requirement 15, the central claim of this module: nothing that is not a
    /// Movie Party handshake may be called Connected.
    #[test]
    fn a_reachable_tunnel_is_never_enough_for_connected() {
        let mut step = inputs(Side::Guest);
        step.invite = InviteStage::Accepted;
        step.method = Some(ConnectionMethod::DeviceSharing);
        step.peer_visible = true;
        step.peer_online = true;
        step.peer_same_tailnet = Some(true);
        step.probe_reachable = Some(true);
        let progress = advance(&step);
        assert_eq!(progress.connectedness, Connectedness::TunnelReachableOnly);
        assert!(!progress.connectedness.is_connected());
        assert_eq!(progress.stage, WizardStage::IdentityVerification);
        assert!(!progress.finished);
        assert!(has_issue(&progress, OnboardingIssue::HandshakeMissing));
        assert!(!passed(&progress, CheckId::MoviePartyTransportHandshake));
        // The flag a view needs in order to refuse the word "Connected": the
        // claim and its evidence must never drift apart.
        assert!(!progress.has_handshake_proof);
        assert_eq!(
            progress.connectedness.is_connected(),
            progress.has_handshake_proof
        );
    }

    #[test]
    fn connectedness_only_follows_the_proof() {
        assert_eq!(connectedness(None, false), Connectedness::NotAttempted);
        assert_eq!(
            connectedness(None, true),
            Connectedness::TunnelReachableOnly
        );
        assert_eq!(
            connectedness(Some(&proof()), false),
            Connectedness::MoviePartyHandshaked
        );
        assert_eq!(
            connectedness(Some(&proof()), true),
            Connectedness::MoviePartyHandshaked
        );
        assert_eq!(
            Connectedness::TunnelReachableOnly.label(),
            "Tunnel only — not Movie Party"
        );
        assert_eq!(Connectedness::MoviePartyHandshaked.label(), "Connected");
    }

    /// Requirement 12: the quarantine direction, resolved against Movie Party's
    /// single dial.
    #[test]
    fn sharing_direction_works_only_for_the_host_being_shared() {
        // Guest looking at a shared host machine: the allowed direction.
        let mut guest = inputs(Side::Guest);
        guest.peer_same_tailnet = Some(false);
        guest.peer_shared_in = Some(true);
        let topology = classify_topology(&guest);
        assert_eq!(
            topology,
            PeerTopology::SharedAcrossTailnets { sharer: Side::Host }
        );
        assert_eq!(
            classify_direction(&guest, &topology),
            DirectionVerdict::Compatible
        );

        // Host looking at a guest machine they shared in: wrong direction.
        let mut host = inputs(Side::Host);
        host.peer_same_tailnet = Some(false);
        host.peer_shared_in = Some(true);
        let topology = classify_topology(&host);
        let verdict = classify_direction(&host, &topology);
        assert!(matches!(verdict, DirectionVerdict::Incompatible { .. }));
        assert!(verdict.blocks_progress());
        // The remedy must name all three documented fixes.
        let DirectionVerdict::Incompatible { remedy } = verdict else {
            unreachable!(" asserted above")
        };
        assert!(remedy.contains("HOST shares their machine with the GUEST"));
        assert!(remedy.contains("mutual sharing"));
        assert!(remedy.contains("one tailnet"));
    }

    #[test]
    fn incompatible_direction_holds_the_wizard_at_reachability() {
        let mut step = inputs(Side::Host);
        step.invite = InviteStage::Accepted;
        step.method = Some(ConnectionMethod::TailnetMembership);
        step.peer_visible = true;
        step.peer_online = true;
        step.peer_same_tailnet = Some(false);
        step.peer_shared_in = Some(true);
        step.probe_reachable = Some(true);
        step.handshake = Some(proof());
        let progress = advance(&step);
        assert_eq!(progress.stage, WizardStage::PeerReachability);
        assert!(has_issue(&progress, OnboardingIssue::DirectionBlocked));
        assert!(!progress.finished);
    }

    #[test]
    fn same_tailnet_membership_never_reports_a_direction_problem() {
        let mut step = inputs(Side::Host);
        step.peer_same_tailnet = Some(true);
        let progress = advance(&step);
        assert_eq!(progress.topology, PeerTopology::SameTailnet);
        assert_eq!(
            progress.direction,
            DirectionVerdict::BothDirectionsAvailable
        );
        assert!(!has_issue(&progress, OnboardingIssue::DirectionBlocked));
    }

    #[test]
    fn unknown_topology_is_reported_as_unknown_not_as_same_tailnet() {
        let step = inputs(Side::Guest);
        let progress = advance(&step);
        assert_eq!(progress.topology, PeerTopology::Unknown);
        assert_eq!(progress.direction, DirectionVerdict::Unknown);
        assert!(!passed(&progress, CheckId::DirectionWorksForMovieParty));
    }

    #[test]
    fn every_transport_flow_is_guest_initiated_except_call_media() {
        for flow in TRANSPORT_FLOWS.iter() {
            assert_eq!(
                flow.starts_from, MOVIE_PARTY_CONNECTION_INITIATOR,
                "{} must start from the guest",
                flow.name
            );
        }
        let media = TRANSPORT_FLOWS
            .iter()
            .find(|flow| flow.name.contains("WebRTC"))
            .expect("media flow listed");
        assert!(media.needs_reverse_initiation);
        assert_eq!(media.evidence, "UNVERIFIED_NEEDS_TWO_DEVICES");
        // Nothing in this module may claim the directionality is live-proven.
        assert_eq!(
            direction_evidence(),
            DirectionEvidence::ReasonedFromCodeAndDocs
        );
    }

    /// Requirement 16, one assertion per state.
    #[test]
    fn reports_missing_tailscale() {
        let mut step = inputs(Side::Host);
        step.tailscale_installed = false;
        step.daemon_answered = false;
        let progress = advance(&step);
        assert!(has_issue(&progress, OnboardingIssue::TailscaleMissing));
        assert_eq!(progress.stage, WizardStage::InviteFriend);
    }

    #[test]
    fn reports_expired_node_key() {
        let mut step = inputs(Side::Host);
        step.node_key = NodeKeyState::Expired;
        let progress = advance(&step);
        assert!(has_issue(&progress, OnboardingIssue::NodeKeyExpired));
        assert!(!passed(&progress, CheckId::NodeKeyValid));
    }

    #[test]
    fn expiring_soon_key_is_not_reported_as_expired() {
        let mut step = inputs(Side::Host);
        step.node_key = NodeKeyState::ExpiringSoon;
        let progress = advance(&step);
        assert!(!has_issue(&progress, OnboardingIssue::NodeKeyExpired));
        assert!(passed(&progress, CheckId::NodeKeyValid));
    }

    #[test]
    fn reports_offline_peer() {
        let mut step = inputs(Side::Host);
        step.method = Some(ConnectionMethod::DeviceSharing);
        step.peer_visible = true;
        step.peer_online = false;
        let progress = advance(&step);
        assert!(has_issue(&progress, OnboardingIssue::PeerOffline));
    }

    #[test]
    fn reports_policy_denial_from_probe_text() {
        let mut step = inputs(Side::Host);
        step.method = Some(ConnectionMethod::TailnetMembership);
        step.peer_visible = true;
        step.peer_online = true;
        step.peer_same_tailnet = Some(true);
        step.probe_reachable = Some(false);
        step.probe_message = Some("connection denied by tailnet ACL policy".to_string());
        let progress = advance(&step);
        assert!(has_issue(&progress, OnboardingIssue::PolicyDenied));
        assert!(!has_issue(&progress, OnboardingIssue::PeerOffline));
    }

    #[test]
    fn reports_wrong_tailnet_when_the_peer_is_not_in_the_network() {
        let mut step = inputs(Side::Guest);
        step.method = Some(ConnectionMethod::DeviceSharing);
        step.peer_visible = true;
        step.peer_online = true;
        step.probe_reachable = Some(false);
        step.probe_message =
            Some("MP-NET-TS-008 that device is not in your Tailscale network".to_string());
        let progress = advance(&step);
        assert!(has_issue(&progress, OnboardingIssue::WrongTailnet));
    }

    #[test]
    fn reports_share_pending_before_the_peer_appears() {
        let mut step = inputs(Side::Host);
        step.invite = InviteStage::Accepted;
        step.method = Some(ConnectionMethod::DeviceSharing);
        step.share_link_seen = true;
        let progress = advance(&step);
        assert!(has_issue(&progress, OnboardingIssue::SharePending));
        assert!(progress.waiting_on_partner);
        assert_eq!(progress.stage, WizardStage::TailscaleApproval);
        // Waiting, not failed: nothing is broken yet.
        assert_eq!(
            progress
                .checks
                .iter()
                .find(|c| c.id == CheckId::PeerVisibleOnNetwork)
                .map(|c| c.state),
            Some(CheckState::Waiting)
        );
    }

    #[test]
    fn reports_revoked_or_expired_share() {
        let mut step = inputs(Side::Guest);
        step.method = Some(ConnectionMethod::DeviceSharing);
        step.share_invite_reported_expired = true;
        let progress = advance(&step);
        assert!(has_issue(&progress, OnboardingIssue::InviteRevoked));
        assert!(progress
            .issues
            .iter()
            .any(|i| i.code == "MP-ONB-009" && i.blocking));
    }

    #[test]
    fn reports_duplicate_request_without_blocking_it() {
        let mut step = inputs(Side::Host);
        step.invite = InviteStage::Sent;
        step.duplicate_invite_pending = true;
        let progress = advance(&step);
        assert!(has_issue(
            &progress,
            OnboardingIssue::DuplicateFriendRequest
        ));
        let issue = progress
            .issues
            .iter()
            .find(|i| i.id == OnboardingIssue::DuplicateFriendRequest)
            .expect("issue");
        assert!(!issue.blocking, "a duplicate is a notice, not a wall");
        assert_eq!(progress.stage, WizardStage::InviteFriend);
    }

    #[test]
    fn limited_user_choosing_device_sharing_is_told_who_can_act() {
        let mut step = inputs(Side::Host);
        step.role = TailnetRole::LimitedUser;
        step.method = Some(ConnectionMethod::DeviceSharing);
        let progress = advance(&step);
        assert!(has_issue(&progress, OnboardingIssue::NotTailnetAdmin));
        assert!(!passed(&progress, CheckId::AdminAbleToGrantAccess));
    }

    #[test]
    fn unknown_role_is_never_reported_as_limited_user() {
        // Requirement: do not assume the user administers their tailnet — but
        // also do not assume the opposite when the CLI said nothing.
        let mut step = inputs(Side::Host);
        step.role = TailnetRole::Unknown;
        step.method = Some(ConnectionMethod::DeviceSharing);
        let progress = advance(&step);
        assert!(!has_issue(&progress, OnboardingIssue::NotTailnetAdmin));
        assert_eq!(
            progress
                .checks
                .iter()
                .find(|c| c.id == CheckId::AdminAbleToGrantAccess)
                .map(|c| c.state),
            Some(CheckState::Waiting)
        );
    }

    #[test]
    fn blocked_friend_stops_the_wizard() {
        let mut step = inputs(Side::Host);
        step.friendship = Some(FriendshipState::Blocked);
        step.invite = InviteStage::Blocked;
        let progress = advance(&step);
        assert!(has_issue(&progress, OnboardingIssue::BlockedByYou));
        assert_eq!(progress.stage, WizardStage::InviteFriend);
        assert!(!progress.finished);
    }

    #[test]
    fn daemon_silence_and_missing_login_are_different_problems() {
        // Installed but not answering is a service problem, not a login one;
        // conflating them tells the user to sign in when there is nothing to
        // sign in to.
        let mut quiet = inputs(Side::Host);
        quiet.daemon_answered = false;
        quiet.signed_in = false;
        let progress = advance(&quiet);
        assert!(has_issue(
            &progress,
            OnboardingIssue::TailscaleDaemonUnreachable
        ));
        assert!(!has_issue(&progress, OnboardingIssue::LoginRequired));
        assert_eq!(
            progress
                .checks
                .iter()
                .find(|c| c.id == CheckId::TailscaleSignedIn)
                .map(|c| c.state),
            Some(CheckState::Waiting)
        );

        let mut logged_out = inputs(Side::Host);
        logged_out.signed_in = false;
        let progress = advance(&logged_out);
        assert!(has_issue(&progress, OnboardingIssue::LoginRequired));
        assert!(!has_issue(
            &progress,
            OnboardingIssue::TailscaleDaemonUnreachable
        ));
        assert_eq!(
            progress
                .checks
                .iter()
                .find(|c| c.id == CheckId::TailscaleSignedIn)
                .map(|c| c.state),
            Some(CheckState::Failed)
        );
        // Signing in is the other person's job too, so this is a wait, not a fault.
        assert!(progress.waiting_on_partner);
    }

    #[test]
    fn issue_codes_are_unique_and_prefixed() {
        let all = [
            OnboardingIssue::TailscaleMissing,
            OnboardingIssue::TailscaleDaemonUnreachable,
            OnboardingIssue::LoginRequired,
            OnboardingIssue::NodeKeyExpired,
            OnboardingIssue::PeerOffline,
            OnboardingIssue::PolicyDenied,
            OnboardingIssue::WrongTailnet,
            OnboardingIssue::SharePending,
            OnboardingIssue::InviteRevoked,
            OnboardingIssue::DuplicateFriendRequest,
            OnboardingIssue::NotTailnetAdmin,
            OnboardingIssue::DirectionBlocked,
            OnboardingIssue::HandshakeMissing,
            OnboardingIssue::BlockedByYou,
        ];
        assert_eq!(all.len(), 14, "requirement 16 states + extras");
        let mut codes: Vec<&str> = all.iter().map(|i| i.code()).collect();
        codes.sort_unstable();
        let before = codes.len();
        codes.dedup();
        assert_eq!(before, codes.len(), "issue codes must be unique");
        for issue in all {
            assert!(issue.code().starts_with("MP-ONB-"), "{}", issue.code());
            assert!(!issue.headline().is_empty());
            assert!(
                issue.guidance().chars().count() > 40,
                "guidance for {} is too thin",
                issue.code()
            );
        }
    }

    #[test]
    fn methods_state_both_grants_and_limits() {
        for method in [
            ConnectionMethod::DeviceSharing,
            ConnectionMethod::TailnetMembership,
        ] {
            let facts = method_facts(method);
            assert!(!facts.grants.is_empty());
            assert!(
                facts.limits.len() >= 3,
                "{method:?} must tell the user what it costs"
            );
            assert!(facts.recipient_must_consent);
            assert!(facts
                .admin_url
                .starts_with("https://login.tailscale.com/admin/"));
        }
        let sharing = method_facts(ConnectionMethod::DeviceSharing);
        assert!(sharing
            .limits
            .iter()
            .any(|limit| limit.contains("cannot start connections")));
        let membership = method_facts(ConnectionMethod::TailnetMembership);
        assert!(membership
            .grants
            .iter()
            .any(|grant| grant.contains("Both devices can start connections")));
    }

    #[test]
    fn method_ids_round_trip() {
        for method in [
            ConnectionMethod::DeviceSharing,
            ConnectionMethod::TailnetMembership,
        ] {
            assert_eq!(ConnectionMethod::from_id(method.id()), Some(method));
        }
        assert_eq!(ConnectionMethod::from_id("TELEPATHY"), None);
        assert_eq!(ConnectionMethod::from_id(""), None);
    }

    // ── Bearer-link hygiene ───────────────────────────────────────────

    const REAL_INVITE: &str =
        "https://acme-abc123.ts.net.login.tailscale.com/a/TSKsecrettokenvalue";

    #[test]
    fn pasted_link_is_described_without_reproducing_the_token() {
        let summary = inspect_share_link(REAL_INVITE, Some("other.ts.net")).expect("recognised");
        assert_eq!(summary.kind, ShareLinkKind::TailscaleInvite);
        assert_eq!(summary.token_fingerprint.len(), 8);
        assert!(summary
            .token_fingerprint
            .chars()
            .all(|c| c.is_ascii_hexdigit()));
        let json = serde_json::to_string(&summary).expect("serializes");
        assert!(
            !json.contains("TSKsecrettokenvalue"),
            "summary leaked the bearer token: {json}"
        );
    }

    #[test]
    fn same_link_pastes_to_the_same_fingerprint_and_a_different_one_does_not() {
        let a = inspect_share_link(REAL_INVITE, None).expect("a");
        let b = inspect_share_link(&format!("  {REAL_INVITE}  "), None).expect("b");
        assert_eq!(a.token_fingerprint, b.token_fingerprint);
        let other = inspect_share_link(
            "https://acme-abc123.ts.net.login.tailscale.com/a/TSKdifferenttoken",
            None,
        )
        .expect("other");
        assert_ne!(a.token_fingerprint, other.token_fingerprint);
    }

    #[test]
    fn tailnet_handle_comparison_is_only_ever_best_effort() {
        let matching = inspect_share_link(REAL_INVITE, Some("acme-abc123.ts.net")).expect("match");
        assert_eq!(matching.tailnet_handle_matches_local, Some(true));
        let mismatch =
            inspect_share_link(REAL_INVITE, Some("tail99aa00.ts.net")).expect("mismatch");
        assert_eq!(mismatch.tailnet_handle_matches_local, Some(false));
        // No local suffix reported => genuinely unknown, not "wrong tailnet".
        let unknown = inspect_share_link(REAL_INVITE, None).expect("unknown");
        assert_eq!(unknown.tailnet_handle_matches_local, None);
    }

    #[test]
    fn non_tailscale_and_insecure_links_are_refused() {
        assert_eq!(
            inspect_share_link("https://evil.example.com/a/TSKx", None),
            Err(ShareLinkError::UnrecognisedLink)
        );
        assert_eq!(
            inspect_share_link("http://acme.ts.net.login.tailscale.com/a/TSKx", None),
            Err(ShareLinkError::InsecureScheme)
        );
        assert_eq!(
            inspect_share_link("not a url", None),
            Err(ShareLinkError::UnrecognisedLink)
        );
        assert_eq!(
            inspect_share_link("https://acme.login.tailscale.com/a/", None),
            Err(ShareLinkError::MissingToken)
        );
        // A plain login URL is a Tailscale link, just not a bearer invite.
        let login =
            inspect_share_link("https://login.tailscale.com/start?key=abc", None).expect("login");
        assert_eq!(login.kind, ShareLinkKind::TailscaleLogin);
    }

    #[test]
    fn scrubbing_removes_bearer_tokens_from_messages() {
        let line =
            "error: open https://acme.ts.net.login.tailscale.com/a/TSKsupersecret and accept";
        let scrubbed = scrub_tailscale_secrets(line);
        assert!(!scrubbed.contains("TSKsupersecret"), "{scrubbed}");
        assert!(
            scrubbed.contains("login.tailscale.com/[REDACTED]"),
            "{scrubbed}"
        );
        assert!(
            scrubbed.contains("and accept"),
            "scrubbing ate the rest of the line"
        );
        // Ordinary text and ordinary URLs must survive untouched.
        let plain = "Tailscale is running on tailc930b7.ts.net";
        assert_eq!(scrub_tailscale_secrets(plain), plain);
        let admin = "visit https://login.tailscale.com/admin/machines to share";
        assert_eq!(scrub_tailscale_secrets(admin), admin);
    }

    #[test]
    fn pasted_link_debug_output_never_contains_the_token() {
        let link = PastedShareLink::capture(REAL_INVITE, None).expect("captured");
        let text = format!("{link:?}");
        assert!(
            !text.contains("TSKsecrettokenvalue"),
            "Debug leaked the bearer token: {text}"
        );
        assert!(text.contains("[REDACTED]"), "{text}");
        // The token is still reachable by the one caller allowed to use it.
        assert_eq!(link.url(), REAL_INVITE);
        assert_eq!(link.fingerprint(), link.summary.token_fingerprint.as_str());
        let again = PastedShareLink::capture(&format!(" {REAL_INVITE} "), None).expect("recapture");
        assert_eq!(again.fingerprint(), link.fingerprint());
    }

    #[test]
    fn only_tailscale_account_hosts_are_trusted_as_link_sources() {
        assert!(is_tailscale_host("login.tailscale.com"));
        assert!(is_tailscale_host("acme-abc123.ts.net.login.tailscale.com"));
        assert!(is_tailscale_host("web.tailscale.com"));
        // A host that merely ends with the right letters is not Tailscale.
        assert!(!is_tailscale_host(
            "evil-login.tailscale.com.attacker.example"
        ));
        assert!(!is_tailscale_host("tailscale.com.evil"));
        assert!(!is_tailscale_host("login.tailscale.co"));
    }

    #[test]
    fn secret_url_detector_covers_the_real_shape() {
        assert!(looks_like_tailscale_secret_url(REAL_INVITE));
        assert!(!looks_like_tailscale_secret_url(&scrub_tailscale_secrets(
            REAL_INVITE
        )));
        assert!(!looks_like_tailscale_secret_url(
            "https://login.tailscale.com/admin/machines"
        ));
    }

    // ── Invitation package ────────────────────────────────────────────

    #[test]
    fn package_carries_the_invite_and_steps_but_never_a_share_link() {
        let package = build_invitation_package(
            "movieparty://friend/v2/abc",
            ConnectionMethod::DeviceSharing,
            "Riya",
            NOW + FRIEND_INVITE_TTL_MS,
            Some(REAL_INVITE),
        );
        assert!(package.text.contains("movieparty://friend/v2/abc"));
        assert!(package.text.contains("Riya"));
        assert!(package.text.contains("Share this one machine"));
        assert!(package.one_time);
        assert!(package.carries_no_tailscale_secret);
        assert!(
            !package.text.contains("TSKsecrettokenvalue"),
            "the package must not carry the bearer link: {}",
            package.text
        );
        assert!(package.text.contains("delivered by Tailscale"));
        assert_eq!(package.expires_at_ms, NOW + FRIEND_INVITE_TTL_MS);
    }

    #[test]
    fn package_without_a_share_link_makes_no_promise_about_one() {
        let package = build_invitation_package(
            "movieparty://friend/v2/abc",
            ConnectionMethod::TailnetMembership,
            "Riya",
            NOW,
            None,
        );
        assert!(!package.text.contains("fingerprint"));
        assert!(package.text.contains("tailnet admin"));
        assert!(package.carries_no_tailscale_secret);
    }

    #[test]
    fn package_text_survives_the_serialization_boundary() {
        let package = build_invitation_package(
            "movieparty://friend/v2/abc",
            ConnectionMethod::TailnetMembership,
            "Riya",
            NOW,
            None,
        );
        let json = serde_json::to_string(&package).expect("serializes");
        let back: InvitationPackage = serde_json::from_str(&json).expect("round-trips");
        assert_eq!(back, package);
        assert!(json.contains("\"oneTime\":true"));
        assert!(json.contains("\"carriesNoTailscaleSecret\":true"));
    }

    // ── Wiring sanity ─────────────────────────────────────────────────

    #[test]
    fn progress_serializes_with_camel_case_field_names() {
        let progress = advance(&inputs(Side::Guest));
        let json = serde_json::to_value(&progress).expect("serializes");
        for key in [
            "stage",
            "method",
            "checks",
            "issues",
            "topology",
            "direction",
            "connectedness",
            "finished",
            "waitingOnPartner",
        ] {
            assert!(json.get(key).is_some(), "missing {key} in {json}");
        }
        assert_eq!(json["stage"], serde_json::json!("INVITE_FRIEND"));
    }

    #[test]
    fn probe_description_tells_the_truth_about_what_it_proved() {
        let probe = PeerConnectionProbe {
            reachable: true,
            path: Some("direct".to_string()),
            latency_ms: Some(12),
            message: String::new(),
        };
        let text = describe_probe(&probe);
        assert!(text.contains("direct"));
        assert!(text.contains("12 ms"));
        assert!(
            text.contains("not Movie Party"),
            "a ping must never be described as trust: {text}"
        );
        let failed = PeerConnectionProbe {
            reachable: false,
            path: None,
            latency_ms: None,
            message: "MP-NET-TS-007 no answer".to_string(),
        };
        assert_eq!(describe_probe(&failed), "MP-NET-TS-007 no answer");
    }

    #[test]
    fn direction_summary_explains_each_topology() {
        let (verdict, text) = direction_summary(Side::Guest, PeerTopology::SameTailnet);
        assert_eq!(verdict, DirectionVerdict::BothDirectionsAvailable);
        assert!(text.contains("same tailnet"));
        let (verdict, text) = direction_summary(
            Side::Guest,
            PeerTopology::SharedAcrossTailnets { sharer: Side::Host },
        );
        assert_eq!(verdict, DirectionVerdict::Compatible);
        assert!(text.contains("guest"));
        let (verdict, _) = direction_summary(
            Side::Host,
            PeerTopology::SharedAcrossTailnets {
                sharer: Side::Guest,
            },
        );
        assert!(verdict.blocks_progress());
    }

    #[test]
    fn issues_are_never_listed_twice() {
        let mut step = inputs(Side::Host);
        step.tailscale_installed = false;
        step.node_key = NodeKeyState::Expired;
        step.peer_visible = false;
        step.method = Some(ConnectionMethod::DeviceSharing);
        step.share_link_seen = true;
        step.probe_reachable = Some(false);
        step.probe_message = Some("no reply".to_string());
        // PeerOffline can be raised twice (visibility + probe); the list must
        // still hold one entry per code.
        let progress = advance(&step);
        let mut codes: Vec<&str> = progress.issues.iter().map(|i| i.code.as_str()).collect();
        codes.sort_unstable();
        let len = codes.len();
        codes.dedup();
        assert_eq!(len, codes.len(), "duplicate issues: {:?}", progress.issues);
    }
}
