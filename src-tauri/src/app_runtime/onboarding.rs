//! Batch 9 runtime: the live wizard session.
//!
//! Every decision comes from `crate::onboarding::advance`, which is pure. This
//! file only observes (the Tailscale CLI, storage, transport handshakes), holds
//! the session, and renders it. Two invariants are enforced here rather than in
//! the reducer because they are about *where data goes*:
//!
//! * A pasted Tailscale share link lives in this session's memory only. It is
//!   never written to storage and never serialized into a view; the view carries
//!   its fingerprint instead. Any text that came from the Tailscale CLI is
//!   scrubbed before it leaves this module.
//! * `Connected` requires a handshake this process actually observed. Proofs are
//!   keyed by the authenticated device id and are deliberately not persisted —
//!   after a restart nothing is connected until the transport re-authenticates.

use super::*;
use crate::app_runtime::friendship::normalised_peer_key;
use crate::friends::FriendshipState;
use crate::identity::identity_fingerprint;
use crate::network::tailscale::{
    self, NodeKeyState, PeerConnectionProbe, TailnetIdentity, TailnetRole,
};
use crate::onboarding::{
    self, advance, build_invitation_package, direction_evidence, method_facts, ConnectionMethod,
    DirectionEvidence, HandshakeProof, InviteStage, MethodFacts, OnboardingInputs,
    OnboardingProgress, PastedShareLink, ShareLinkError, ShareLinkSummary, Side, TransportFlow,
    GUEST_HANDSHAKE_MECHANISM, HANDSHAKE_MECHANISM, TRANSPORT_FLOWS,
};
use crate::storage::sqlite::StoredFriend;

use std::collections::HashMap;

/// What the frontend renders for one peer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingView {
    /// A wizard session is open. `onboarding_view` answers this when none is,
    /// so the shell can route into the app's normal screens instead of blocking
    /// on a failed call.
    pub active: bool,
    /// Requirement 15, resolved: the pair Movie Party can show as "Connected".
    /// It is only ever true when the reducer's connectedness carries a handshake
    /// proof observed by this process.
    pub connected: bool,
    pub peer_key: String,
    pub display_name: String,
    pub my_side: Side,
    pub progress: OnboardingProgress,
    /// Metadata only — a pasted link's token is never part of this payload.
    pub share_link: Option<ShareLinkSummary>,
    pub our_fingerprint: String,
    /// Their fingerprint, when Batch 8's acceptance bound an identity key.
    pub their_fingerprint: Option<String>,
    pub tailnet: Option<TailnetIdentity>,
    /// This device's node key, classified. The UI must not re-derive it from a
    /// timestamp string it cannot parse.
    pub node_key: NodeKeyState,
    pub probe: Option<PeerConnectionProbe>,
    pub methods: Vec<MethodFacts>,
    pub flows: Vec<TransportFlow>,
    pub direction_evidence: DirectionEvidence,
    pub observed_at_ms: i64,
}

#[derive(Debug, Clone)]
struct Session {
    peer_key: String,
    display_name: String,
    my_side: Side,
    method: Option<ConnectionMethod>,
    /// In memory only; see the module comment.
    pasted: Option<PastedShareLink>,
    probe: Option<PeerConnectionProbe>,
    partner_reported_expired: bool,
    tailnet: Option<TailnetIdentity>,
    inputs: Option<OnboardingInputs>,
    observed_at_ms: i64,
}

/// The wizard's whole runtime footprint.
#[derive(Debug, Default)]
pub struct OnboardingState {
    session: Option<Session>,
    /// Device ids whose Movie Party transport handshake completed in this
    /// process, with the proof. Never persisted, never inferred from a Tailscale
    /// status entry or a ping.
    handshakes: HashMap<String, HandshakeProof>,
}

const NO_SESSION: &str = "MP-ONB-032 no onboarding is running";

impl AppRuntime {
    /// Start (or replace) the wizard for one peer.
    pub fn onboarding_begin(
        &self,
        peer_key: &str,
        my_side: &str,
    ) -> Result<OnboardingView, String> {
        if peer_key.trim().is_empty() {
            return Err("MP-ONB-030 pick a device to set up first.".to_string());
        }
        let peer_key = normalised_peer_key(peer_key.trim());
        let side =
            Side::from_id(my_side).ok_or("MP-ONB-031 say whether you are hosting or joining")?;
        let friend = self.find_friend(&peer_key);
        let display_name = friend
            .map(|friend| friend.display_name)
            .unwrap_or_else(|| tailscale::peer_display_name(&peer_key));
        self.lock().onboarding.session = Some(Session {
            peer_key,
            display_name,
            my_side: side,
            method: None,
            pasted: None,
            probe: None,
            partner_reported_expired: false,
            tailnet: None,
            inputs: None,
            observed_at_ms: 0,
        });
        self.onboarding_view()
    }

    /// End the wizard and forget everything it held, including any pasted link.
    pub fn onboarding_cancel(&self) {
        self.lock().onboarding.session = None;
    }

    pub fn onboarding_set_side(&self, my_side: &str) -> Result<OnboardingView, String> {
        let side =
            Side::from_id(my_side).ok_or("MP-ONB-031 say whether you are hosting or joining")?;
        let mut state = self.lock();
        let session = state.onboarding.session.as_mut().ok_or(NO_SESSION)?;
        session.my_side = side;
        let session = session.clone();
        drop(state);
        self.render(&session)
    }

    pub fn onboarding_choose_method(&self, method: &str) -> Result<OnboardingView, String> {
        let method = ConnectionMethod::from_id(method)
            .ok_or("MP-ONB-033 that is not one of the two connection methods")?;
        let mut state = self.lock();
        let session = state.onboarding.session.as_mut().ok_or(NO_SESSION)?;
        session.method = Some(method);
        let session = session.clone();
        drop(state);
        self.render(&session)
    }

    /// The official Tailscale page for the chosen method. Handed out only after
    /// the user picked a method; opening it stays their explicit action, and
    /// Movie Party never creates the share on their behalf.
    pub fn onboarding_admin_url(&self) -> Result<String, String> {
        let state = self.lock();
        let session = state.onboarding.session.as_ref().ok_or(NO_SESSION)?;
        let method = session
            .method
            .ok_or("MP-ONB-034 choose how the two devices will connect first")?;
        Ok(method_facts(method).admin_url.to_string())
    }

    /// Keeps a pasted share link in memory and returns only its metadata.
    pub fn onboarding_paste_share_link(&self, link: &str) -> Result<ShareLinkSummary, String> {
        let local_suffix = self.lock().onboarding.session.as_ref().and_then(|session| {
            session
                .tailnet
                .as_ref()
                .and_then(|identity| identity.magic_dns_suffix.clone())
        });
        let captured =
            PastedShareLink::capture(link, local_suffix.as_deref()).map_err(|error| {
                // The input may be a live bearer link, so a rejection must name the
                // problem without repeating it.
                match error {
                    ShareLinkError::UnrecognisedLink => {
                        "MP-ONB-020 that is not a Tailscale link Movie Party can recognise"
                            .to_string()
                    }
                    ShareLinkError::InsecureScheme => {
                        "MP-ONB-021 a Tailscale link must use https".to_string()
                    }
                    ShareLinkError::MissingToken => {
                        "MP-ONB-022 that link is missing its one-time token".to_string()
                    }
                }
            })?;
        let summary = captured.summary.clone();
        self.lock()
            .onboarding
            .session
            .as_mut()
            .ok_or(NO_SESSION)?
            .pasted = Some(captured);
        Ok(summary)
    }

    /// The pasted link, for its one permitted use: opening it in the user's own
    /// browser because they asked for that.
    pub fn onboarding_share_link_url(&self) -> Result<String, String> {
        let state = self.lock();
        let session = state.onboarding.session.as_ref().ok_or(NO_SESSION)?;
        session
            .pasted
            .as_ref()
            .map(|link| link.url().to_string())
            .ok_or("MP-ONB-035 no Tailscale link has been pasted in this wizard".to_string())
    }

    /// The user says their partner's link never arrived or had expired. This is
    /// the only honest source for that signal: a share on another tailnet is not
    /// visible from here.
    pub fn onboarding_report_link_expired(&self) -> Result<OnboardingView, String> {
        let mut state = self.lock();
        let session = state.onboarding.session.as_mut().ok_or(NO_SESSION)?;
        session.partner_reported_expired = true;
        let session = session.clone();
        drop(state);
        self.render(&session)
    }

    /// Re-render from the last observation. No I/O, so stage actions are cheap.
    pub fn onboarding_view(&self) -> Result<OnboardingView, String> {
        let session = self.lock().onboarding.session.clone().ok_or(NO_SESSION)?;
        self.render(&session)
    }

    /// Observe the network again and recompute. This is the only wizard call that
    /// runs the Tailscale CLI.
    pub async fn onboarding_refresh(&self) -> Result<OnboardingView, String> {
        let session = self.lock().onboarding.session.clone().ok_or(NO_SESSION)?;
        let status = tailscale::detect_status().await.ok();
        let mut inputs = OnboardingInputs::start(session.my_side);
        let mut tailnet: Option<TailnetIdentity> = None;

        match status.as_ref() {
            Some(status) => {
                tailnet = status.tailnet.clone();
                inputs.tailscale_installed = true;
                inputs.daemon_answered = status.backend_state.is_some();
                inputs.signed_in = matches!(
                    status.backend_state.as_deref(),
                    Some("Running") | Some("Starting")
                );
                inputs.role = tailnet
                    .as_ref()
                    .map(|identity| identity.role)
                    .unwrap_or(TailnetRole::Unknown);
                inputs.node_key = NodeKeyState::from_expiry(
                    tailnet
                        .as_ref()
                        .and_then(|identity| identity.key_expiry.as_deref()),
                    wall_now_ms(),
                );
                if let Some(peer) = status
                    .peers
                    .iter()
                    .find(|peer| normalised_peer_key(&peer.dns_name) == session.peer_key)
                {
                    inputs.peer_visible = true;
                    inputs.peer_online = peer.online;
                    inputs.peer_same_tailnet = tailscale::peer_is_same_tailnet(status, peer);
                    inputs.peer_shared_in = Some(peer.shared_in);
                }
            }
            // No CLI at all. Say so rather than wait forever on a status that
            // will never arrive.
            None => {
                inputs.tailscale_installed = false;
                inputs.daemon_answered = false;
                inputs.signed_in = false;
            }
        }

        // The friendship position is local truth, not something to poll.
        if let Some(state) = self.friendship_state(&session.peer_key) {
            inputs.friendship = Some(state);
            inputs.invite = match state {
                FriendshipState::Accepted => InviteStage::Accepted,
                FriendshipState::Invited => InviteStage::Sent,
                FriendshipState::PendingAcceptance => InviteStage::ReceivedAwaitingDecision,
                FriendshipState::Blocked => InviteStage::Blocked,
                _ => InviteStage::NotStarted,
            };
            // "Already asked" is a notice, not a wall: an outstanding INVITED
            // position is exactly the duplicate case requirement 16 lists.
            inputs.duplicate_invite_pending = inputs.invite == InviteStage::Sent;
        }
        // A stored outstanding invitation says the same thing more precisely, and
        // covers the case where the trust row has since moved on.
        if self.has_outstanding_invite(&session.peer_key) {
            inputs.duplicate_invite_pending = true;
        }

        let observed = Session {
            tailnet: tailnet.clone(),
            ..session.clone()
        };
        self.store_observation(&observed, inputs);
        self.render(&observed)
    }

    /// Runs a real `tailscale ping` toward this friend. It can prove reachability
    /// and nothing more — requirement 15 keeps `Connected` behind the handshake.
    pub async fn onboarding_check_reachability(&self) -> Result<OnboardingView, String> {
        let peer_key = self
            .lock()
            .onboarding
            .session
            .as_ref()
            .ok_or(NO_SESSION)?
            .peer_key
            .clone();
        let verification = self.verify_friend(&peer_key).await?;
        {
            let mut state = self.lock();
            if let Some(session) = state.onboarding.session.as_mut() {
                // A CLI failure line can carry an accept URL with a bearer
                // token in it; the probe is shown to the user, so scrub it here.
                session.probe = Some(scrub_probe(&verification.probe));
            }
        }
        self.onboarding_refresh().await
    }

    /// The copyable invitation: the signed Movie Party invite plus the Tailscale
    /// steps. The pasted share link is never embedded — see
    /// `build_invitation_package`.
    pub async fn onboarding_invitation_package(
        &self,
    ) -> Result<onboarding::InvitationPackage, String> {
        let session = self.lock().onboarding.session.clone().ok_or(NO_SESSION)?;
        let method = session
            .method
            .ok_or("MP-ONB-034 choose how the two devices will connect first")?;
        let sent = self.friend_invite_signed().await?;
        // Attribute the issued invite to this peer, so "you already asked them"
        // becomes a fact read from storage instead of an inference.
        self.record_package_recipient(&sent.link, &session.peer_key)
            .await?;
        let pasted = session.pasted.as_ref().map(|link| link.url().to_string());
        Ok(build_invitation_package(
            &sent.link,
            method,
            &session.display_name,
            sent.expires_at_ms,
            pasted.as_deref(),
        ))
    }

    /// Records that a peer's device completed a real Movie Party transport
    /// handshake: the host's view, after the guest's auth signature and join
    /// secret verified.
    pub(crate) fn record_host_side_handshake(&self, device_id: &str) {
        self.record_handshake(
            device_id,
            self.friend_key_for_device(device_id),
            HANDSHAKE_MECHANISM,
        );
    }

    /// The guest's view of the same handshake: the host answered as the device id
    /// the invite named, over a connection pinned to the invite's fingerprint.
    pub(crate) fn record_guest_side_handshake(&self, device_id: &str) {
        self.record_handshake(
            device_id,
            self.friend_key_for_device(device_id),
            GUEST_HANDSHAKE_MECHANISM,
        );
    }

    fn friend_key_for_device(&self, device_id: &str) -> Option<String> {
        let db = self.lock().db.clone()?;
        db.friend_key_for_device(device_id).ok().flatten()
    }

    fn record_handshake(
        &self,
        device_id: &str,
        peer_key: Option<String>,
        verified_by: &'static str,
    ) {
        if device_id.trim().is_empty() {
            return;
        }
        let proof = HandshakeProof {
            device_id: device_id.to_string(),
            peer_key,
            verified_by,
            at_ms: wall_now_ms(),
        };
        self.lock()
            .onboarding
            .handshakes
            .insert(proof.device_id.clone(), proof);
    }

    /// The idle view: no session, nothing claimed. Used by `onboarding_view` so
    /// the shell's poll has one shape to render either way.
    pub(crate) fn onboarding_idle_view(&self) -> OnboardingView {
        let our_public_key = self.inner.identity().public_key_base64();
        OnboardingView {
            active: false,
            connected: false,
            peer_key: String::new(),
            display_name: String::new(),
            my_side: Side::Host,
            progress: advance(&OnboardingInputs::start(Side::Host)),
            share_link: None,
            our_fingerprint: identity_fingerprint(&our_public_key).unwrap_or_default(),
            their_fingerprint: None,
            node_key: NodeKeyState::Unknown,
            tailnet: None,
            probe: None,
            methods: vec![
                method_facts(ConnectionMethod::DeviceSharing),
                method_facts(ConnectionMethod::TailnetMembership),
            ],
            flows: TRANSPORT_FLOWS.to_vec(),
            direction_evidence: direction_evidence(),
            observed_at_ms: 0,
        }
    }

    /// Stores which friend an issued invitation was handed to. The invite is
    /// signed by us and only names us, so without this the ledger could never
    /// tell an outstanding request for one peer from one for another.
    pub(crate) async fn record_package_recipient(
        &self,
        link: &str,
        recipient_peer_key: &str,
    ) -> Result<(), String> {
        let invite = crate::friends::parse_invite_link(link).map_err(|e| e.to_string())?;
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        db.set_invite_recipient(&invite.nonce, recipient_peer_key)
            .map_err(|e| e.to_string())
    }

    /// True when an invitation we issued to this peer is still outstanding.
    pub(crate) fn has_outstanding_invite(&self, recipient_peer_key: &str) -> bool {
        let Some(db) = self.lock().db.clone() else {
            return false;
        };
        db.pending_sent_invite_for_recipient(recipient_peer_key, wall_now_ms())
            .ok()
            .flatten()
            .is_some()
    }

    fn find_friend(&self, peer_key: &str) -> Option<StoredFriend> {
        self.list_friends()
            .into_iter()
            .find(|friend| normalised_peer_key(&friend.peer_key) == peer_key)
    }

    fn friend_identity(&self, peer_key: &str) -> Option<(String, String)> {
        let db = self.lock().db.clone()?;
        db.friend_identity_for_peer(peer_key).ok().flatten()
    }

    /// Stores a fresh network observation on the session. Everything the session
    /// itself owns (chosen method, pasted link, probe, handshake proof, and even
    /// which side the user said they are) is applied at render time, so a stage
    /// action can never be silently dropped.
    fn store_observation(&self, session: &Session, inputs: OnboardingInputs) {
        let mut state = self.lock();
        if let Some(existing) = state.onboarding.session.as_mut() {
            existing.inputs = Some(inputs);
            existing.tailnet = session.tailnet.clone();
            existing.observed_at_ms = wall_now_ms();
        }
    }

    /// Renders a view for a session. `Connectedness` comes from the pure reducer
    /// only — this is the one place that could be tempted to guess, so the view's
    /// own `connected` flag resolves from the reducer's proof flag as well.
    fn render(&self, session: &Session) -> Result<OnboardingView, String> {
        let inputs = self.merged_inputs(session);
        let progress = advance(&inputs);
        let stored = self.lock().onboarding.session.clone();
        let observed_at_ms = stored
            .as_ref()
            .map(|session| session.observed_at_ms)
            .unwrap_or_default();
        let tailnet = stored.as_ref().and_then(|session| session.tailnet.clone());
        let our_public_key = self.inner.identity().public_key_base64();
        Ok(OnboardingView {
            active: true,
            connected: progress.connectedness.is_connected() && progress.has_handshake_proof,
            peer_key: session.peer_key.clone(),
            display_name: session.display_name.clone(),
            my_side: session.my_side,
            their_fingerprint: self
                .friend_identity(&session.peer_key)
                .and_then(|(_, public_key)| identity_fingerprint(&public_key).ok()),
            our_fingerprint: identity_fingerprint(&our_public_key).unwrap_or_default(),
            node_key: inputs.node_key,
            progress,
            share_link: session.pasted.as_ref().map(|link| link.summary.clone()),
            tailnet,
            probe: session.probe.clone(),
            methods: vec![
                method_facts(ConnectionMethod::DeviceSharing),
                method_facts(ConnectionMethod::TailnetMembership),
            ],
            flows: TRANSPORT_FLOWS.to_vec(),
            direction_evidence: direction_evidence(),
            observed_at_ms,
        })
    }

    /// The session's own facts applied over the last network observation, so a
    /// stage action (method, side, pasted link, probe) can never be dropped
    /// because the UI did not refresh first.
    fn merged_inputs(&self, session: &Session) -> OnboardingInputs {
        let mut inputs = session
            .inputs
            .clone()
            .unwrap_or_else(|| OnboardingInputs::start(session.my_side));
        inputs.my_side = session.my_side;
        inputs.method = session.method;
        inputs.share_link_seen = session.pasted.is_some();
        inputs.share_invite_reported_expired = session.partner_reported_expired;
        if let Some(probe) = session.probe.as_ref() {
            inputs.probe_reachable = Some(probe.reachable);
            inputs.probe_message = Some(if probe.reachable {
                probe.path.clone().unwrap_or_else(|| "direct".to_string())
            } else {
                onboarding::scrub_tailscale_secrets(&probe.message)
            });
        }
        inputs.handshake = self
            .friend_identity(&session.peer_key)
            .and_then(|(device_id, _)| self.lock().onboarding.handshakes.get(&device_id).cloned());
        inputs
    }
}

/// Requirement 8: a probe message is CLI output, and a CLI failure line can
/// carry an accept URL with a bearer token in it. Scrub before the probe is kept
/// or displayed.
fn scrub_probe(probe: &PeerConnectionProbe) -> PeerConnectionProbe {
    let mut probe = probe.clone();
    probe.message = onboarding::scrub_tailscale_secrets(&probe.message);
    probe
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::friends::FRIEND_INVITE_TTL_MS;
    use crate::onboarding::{CheckId, CheckState, Connectedness, WizardStage};

    const PEER: &str = "priya-mac.tailc930b7.ts.net";
    /// This device's own tailnet name — an invitation names its sender.
    const OUR_PEER: &str = "abhijai-mac.tailc930b7.ts.net";
    const SHARE_LINK: &str =
        "https://acme-abc123.ts.net.login.tailscale.com/a/TSKruntimelevelltsecret";

    fn runtime_with_db() -> (AppRuntime, std::path::PathBuf) {
        let runtime = AppRuntime::new();
        let dir = std::env::temp_dir().join(format!("mp-onboarding-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        runtime.init_db_at_path(&dir.join("mp.db"));
        assert!(
            runtime.lock().db.is_some(),
            "the test database did not open"
        );
        (runtime, dir)
    }

    fn clean(dir: std::path::PathBuf) {
        std::fs::remove_dir_all(dir).ok();
    }

    /// A network reading that is good everywhere except for whatever the test
    /// is about to add.
    fn healthy(side: Side) -> OnboardingInputs {
        let mut inputs = OnboardingInputs::start(side);
        inputs.tailscale_installed = true;
        inputs.daemon_answered = true;
        inputs.signed_in = true;
        inputs.role = TailnetRole::Admin;
        inputs.node_key = NodeKeyState::Valid;
        inputs.invite = InviteStage::Accepted;
        inputs.friendship = Some(FriendshipState::Accepted);
        inputs.method = Some(ConnectionMethod::DeviceSharing);
        inputs
    }

    fn passed_check(view: &OnboardingView, id: CheckId) -> bool {
        view.progress
            .checks
            .iter()
            .any(|check| check.id == id && check.state == CheckState::Passed)
    }

    fn observed(runtime: &AppRuntime, inputs: OnboardingInputs) {
        let session = runtime
            .lock()
            .onboarding
            .session
            .clone()
            .expect("session opened by the test");
        runtime.store_observation(&session, inputs);
    }

    #[test]
    fn the_wizard_needs_a_peer_and_a_role() {
        let (runtime, dir) = runtime_with_db();
        assert!(runtime
            .onboarding_begin("", "HOST")
            .unwrap_err()
            .starts_with("MP-ONB-030"));
        assert!(runtime
            .onboarding_begin(PEER, "SPECTATOR")
            .unwrap_err()
            .starts_with("MP-ONB-031"));
        // A valid start normalises the peer key the same way the friend store does.
        let view = runtime.onboarding_begin(PEER, "host").expect("begun");
        assert_eq!(view.peer_key, format!("{PEER}."));
        assert_eq!(view.my_side, Side::Host);
        assert!(view.active);
        clean(dir);
    }

    #[test]
    fn an_idle_view_claims_nothing() {
        let (runtime, dir) = runtime_with_db();
        let view = runtime.onboarding_idle_view();
        assert!(!view.active);
        assert!(!view.connected);
        assert_eq!(view.progress.stage, WizardStage::InviteFriend);
        assert!(!view.progress.connectedness.is_connected());
        assert_eq!(view.peer_key, "");
        // Nothing is asserted about a node key the CLI never reported.
        assert_eq!(
            view.node_key,
            crate::network::tailscale::NodeKeyState::Unknown
        );
        // The idle shape still carries the whole wizard contract for the UI.
        assert_eq!(view.methods.len(), 2);
        assert_eq!(view.flows.len(), TRANSPORT_FLOWS.len());
        assert_eq!(
            view.direction_evidence,
            DirectionEvidence::ReasonedFromCodeAndDocs
        );
        clean(dir);
    }

    /// Requirement 4's order is enforced by the runtime, not only the reducer:
    /// no connection method can be configured for a device that has not agreed
    /// to be friends.
    #[test]
    fn a_method_cannot_be_chosen_before_the_friend_accepted() {
        let (runtime, dir) = runtime_with_db();
        runtime.onboarding_begin(PEER, "HOST").expect("begun");
        let view = runtime
            .onboarding_choose_method("DEVICE_SHARING")
            .expect("method stored");
        assert_eq!(view.progress.method, Some(ConnectionMethod::DeviceSharing));
        assert_eq!(view.progress.stage, WizardStage::InviteFriend);
        assert!(!view.progress.finished);

        // With the friendship accepted, the session leaves stage 1 — and lands on
        // stage 3, because no peer is visible on the tailnet yet.
        observed(&runtime, healthy(Side::Host));
        let view = runtime.onboarding_view().expect("rendered");
        assert_eq!(view.progress.stage, WizardStage::TailscaleApproval);
        assert!(passed_check(&view, CheckId::FriendInvited));
        assert!(passed_check(&view, CheckId::MethodChosen));
        clean(dir);
    }

    #[test]
    fn choosing_an_unknown_method_is_refused() {
        let (runtime, dir) = runtime_with_db();
        runtime.onboarding_begin(PEER, "HOST").expect("begun");
        assert!(runtime
            .onboarding_choose_method("TELEPORT")
            .unwrap_err()
            .starts_with("MP-ONB-033"));
        // The admin page only exists once a method is chosen.
        assert!(runtime
            .onboarding_admin_url()
            .unwrap_err()
            .starts_with("MP-ONB-034"));
        runtime
            .onboarding_choose_method("TAILNET_MEMBERSHIP")
            .expect("chosen");
        assert_eq!(
            runtime.onboarding_admin_url().expect("url"),
            "https://login.tailscale.com/admin/users"
        );
        clean(dir);
    }

    /// Requirement 8 at the boundary that matters: the payload the frontend (and
    /// any logging around it) can see must not contain the bearer token.
    #[test]
    fn a_pasted_share_link_never_reaches_the_view_payload() {
        let (runtime, dir) = runtime_with_db();
        runtime.onboarding_begin(PEER, "GUEST").expect("begun");
        let summary = runtime
            .onboarding_paste_share_link(SHARE_LINK)
            .expect("recognised");
        assert_eq!(summary.kind, onboarding::ShareLinkKind::TailscaleInvite);
        assert!(!summary.token_fingerprint.is_empty());

        let view = runtime.onboarding_view().expect("rendered");
        let json = serde_json::to_string(&view).expect("serializes");
        assert!(
            !json.contains("TSKruntimelevelltsecret"),
            "the wizard payload leaked the bearer token"
        );
        assert!(json.contains("\"shareLink\""));
        // The link is remembered as a fact, and the token is reachable only for
        // the one purpose it exists for: opening it because the user asked.
        let share_link = view.share_link.expect("link remembered");
        assert_eq!(share_link, summary);
        assert_eq!(
            runtime.onboarding_share_link_url().expect("for opening"),
            SHARE_LINK
        );
        clean(dir);
    }

    #[test]
    fn rejecting_a_link_does_not_repeat_it() {
        let (runtime, dir) = runtime_with_db();
        runtime.onboarding_begin(PEER, "GUEST").expect("begun");
        for (input, code) in [
            ("https://evil.example.com/a/TSKtokenishere", "MP-ONB-020"),
            (
                "http://acme.ts.net.login.tailscale.com/a/TSKplaintexttoken",
                "MP-ONB-021",
            ),
            ("https://acme.login.tailscale.com/a/", "MP-ONB-022"),
        ] {
            let error = runtime.onboarding_paste_share_link(input).unwrap_err();
            assert!(error.starts_with(code), "{error}");
            assert!(
                !error.contains("TSK") && !error.contains("tailscale.com/a"),
                "the rejection echoed the link: {error}"
            );
        }
        clean(dir);
    }

    #[test]
    fn cancelling_forgets_the_pasted_link() {
        let (runtime, dir) = runtime_with_db();
        runtime.onboarding_begin(PEER, "GUEST").expect("begun");
        runtime
            .onboarding_paste_share_link(SHARE_LINK)
            .expect("pasted");
        runtime.onboarding_cancel();
        assert!(runtime
            .onboarding_share_link_url()
            .unwrap_err()
            .starts_with(NO_SESSION));
        assert!(!runtime.onboarding_view().is_ok());
        clean(dir);
    }

    #[test]
    fn a_reported_expired_link_shows_the_revoked_state() {
        let (runtime, dir) = runtime_with_db();
        runtime.onboarding_begin(PEER, "HOST").expect("begun");
        observed(&runtime, healthy(Side::Host));
        let view = runtime.onboarding_report_link_expired().expect("recorded");
        // The observation reached the view, so the UI never parses a timestamp.
        assert_eq!(
            view.node_key,
            crate::network::tailscale::NodeKeyState::Valid
        );
        assert!(view
            .progress
            .issues
            .iter()
            .any(|issue| issue.code == "MP-ONB-009"));
        // And it is reported as blocking, because a new share really is needed.
        assert!(
            view.progress
                .issues
                .iter()
                .find(|issue| issue.code == "MP-ONB-009")
                .expect("issue")
                .blocking
        );
        clean(dir);
    }

    #[test]
    fn the_side_the_user_claims_decides_the_direction_advice() {
        let (runtime, dir) = runtime_with_db();
        // As the guest looking at a shared host machine, the direction is fine.
        runtime.onboarding_begin(PEER, "GUEST").expect("begun");
        observed(&runtime, {
            let mut inputs = healthy(Side::Guest);
            inputs.peer_visible = true;
            inputs.peer_online = true;
            inputs.peer_same_tailnet = Some(false);
            inputs.peer_shared_in = Some(true);
            inputs
        });
        let view = runtime.onboarding_view().expect("rendered");
        assert_eq!(
            view.progress.direction,
            onboarding::DirectionVerdict::Compatible
        );

        // The same facts read from the host's chair are not usable.
        let view = runtime.onboarding_set_side("HOST").expect("switched");
        assert_eq!(view.my_side, Side::Host);
        assert!(matches!(
            view.progress.direction,
            onboarding::DirectionVerdict::Incompatible { .. }
        ));
        assert!(view
            .progress
            .issues
            .iter()
            .any(|issue| issue.code == "MP-ONB-012"));
        clean(dir);
    }

    /// Requirement 15 at the runtime boundary: everything can look perfect — a
    /// visible peer, a reachable tunnel, an accepted friendship — and the wizard
    /// still must not say Connected until this process has seen a real Movie
    /// Party handshake for that device.
    #[tokio::test]
    async fn connected_waits_for_the_transport_handshake() {
        let (inviter, inviter_dir) = runtime_with_db();
        let (recipient, recipient_dir) = runtime_with_db();
        let link = onboarding_invite_from(&inviter);
        let preview = recipient.preview_friend_invite(&link).expect("preview");
        let friend = recipient
            .accept_friend_invitation(preview.nonce.clone())
            .await
            .expect("accepted");
        // Acceptance is a trust decision, not a verification: the connection
        // state stays where Batch 8 left it.
        assert_eq!(
            friend.connection_state,
            crate::storage::sqlite::FriendConnectionState::Invited
        );

        recipient
            .onboarding_begin(&friend.peer_key, "GUEST")
            .expect("begun");
        recipient
            .onboarding_choose_method("DEVICE_SHARING")
            .expect("method chosen");
        let mut inputs = healthy(Side::Guest);
        inputs.peer_visible = true;
        inputs.peer_online = true;
        inputs.peer_same_tailnet = Some(false);
        inputs.peer_shared_in = Some(true);
        inputs.probe_reachable = Some(true);
        inputs.probe_message = Some("direct".to_string());
        observed(&recipient, inputs.clone());

        let before = recipient.onboarding_view().expect("view");
        assert_eq!(
            before.progress.connectedness,
            Connectedness::TunnelReachableOnly,
            "a ping alone must never read as connected"
        );
        assert!(!before.connected);
        assert!(!before.progress.has_handshake_proof);
        assert_eq!(before.progress.stage, WizardStage::IdentityVerification);
        assert!(!before.progress.finished);

        // The transport authenticates the device.
        let inviter_device = inviter.device_identity_for_test().device_id;
        recipient.record_host_side_handshake(&inviter_device);
        observed(&recipient, inputs);

        let after = recipient.onboarding_view().expect("view");
        assert!(after.connected);
        assert!(after.progress.has_handshake_proof);
        assert_eq!(
            after.progress.connectedness,
            Connectedness::MoviePartyHandshaked
        );
        assert_eq!(after.progress.stage, WizardStage::FriendReady);
        assert!(after.progress.finished);
        // The proof names its mechanism so the UI can be checked against it.
        let proof = recipient
            .lock()
            .onboarding
            .handshakes
            .get(&inviter_device)
            .expect("proof kept")
            .clone();
        assert_eq!(proof.verified_by, HANDSHAKE_MECHANISM);
        assert_eq!(proof.peer_key.as_deref(), Some(friend.peer_key.as_str()));
        assert_eq!(after.their_fingerprint, Some(preview.fingerprint));

        clean(inviter_dir);
        clean(recipient_dir);
    }

    /// A handshake for a stranger's device must not light up this friend's
    /// wizard — the proof is matched by identity, not by "someone connected".
    #[test]
    fn a_handshake_from_another_device_does_not_count() {
        let (runtime, dir) = runtime_with_db();
        runtime.onboarding_begin(PEER, "GUEST").expect("begun");
        let mut inputs = healthy(Side::Guest);
        inputs.peer_visible = true;
        inputs.peer_online = true;
        inputs.peer_same_tailnet = Some(true);
        inputs.probe_reachable = Some(true);
        observed(&runtime, inputs.clone());

        runtime.record_host_side_handshake("some-other-device");
        observed(&runtime, inputs);
        let view = runtime.onboarding_view().expect("view");
        assert!(!view.connected);
        assert_eq!(
            view.progress.connectedness,
            Connectedness::TunnelReachableOnly
        );
        clean(dir);
    }

    /// The raw probe is the one wizard payload that carries CLI text, and a CLI
    /// failure line can contain an accept URL with a live bearer token.
    #[test]
    fn probe_failure_text_is_scrubbed_before_it_is_shown() {
        let (runtime, dir) = runtime_with_db();
        runtime.onboarding_begin(PEER, "GUEST").expect("begun");
        let polluted = PeerConnectionProbe {
            reachable: false,
            path: None,
            latency_ms: None,
            message: format!("could not reach the peer, accept access at {SHARE_LINK} first"),
        };
        // Exactly what onboarding_check_reachability stores.
        let stored = scrub_probe(&polluted);
        assert!(
            !stored.message.contains("TSKruntimelevelltsecret"),
            "{stored:?}"
        );
        assert!(stored.message.contains("[REDACTED]"), "{}", stored.message);
        {
            let mut state = runtime.lock();
            state.onboarding.session.as_mut().expect("session").probe = Some(stored);
        }
        let view = runtime.onboarding_view().expect("rendered");
        let json = serde_json::to_string(&view).expect("serializes");
        assert!(
            !json.contains("TSKruntimelevelltsecret"),
            "a CLI message leaked a bearer token into the view"
        );
        assert!(json.contains("[REDACTED]"));
        // The reducer read the same scrubbed text and still classified the
        // failure rather than losing it.
        assert_eq!(view.progress.connectedness, Connectedness::NotAttempted);
        assert!(view
            .progress
            .checks
            .iter()
            .any(|check| check.id == CheckId::PeerReachableThroughTunnel
                && check.state == onboarding::CheckState::Failed));
        clean(dir);
    }

    /// Requirement 8, proven where it can be: the wizard's own database file and
    /// the payload the frontend receives must both be free of the bearer token.
    /// A pasted link is memory-only by construction, not by promise.
    #[test]
    fn a_pasted_link_is_never_written_to_the_database_or_the_payload() {
        let (runtime, dir) = runtime_with_db();
        runtime.onboarding_begin(PEER, "GUEST").expect("begun");
        runtime
            .onboarding_paste_share_link(SHARE_LINK)
            .expect("recognised");
        // Force the session to render, then close it, so any accidental write
        // would already have happened.
        let view = runtime.onboarding_view().expect("rendered");
        let payload = serde_json::to_string(&view).expect("serializes");
        runtime.onboarding_cancel();
        assert!(
            !payload.contains("TSKruntimelevelltsecret"),
            "the wizard payload leaked the bearer token"
        );

        // WAL mode means recent writes live in the sidecar files, so scan every
        // file the database owns rather than only `mp.db`.
        let entries = std::fs::read_dir(&dir)
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        assert!(
            entries
                .iter()
                .any(|path| path.extension().map(|ext| ext == "db") == Some(true)),
            "no database file was created"
        );
        let mut stored = String::new();
        for path in &entries {
            if let Ok(bytes) = std::fs::read(path) {
                stored.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
        assert!(
            stored.contains("friend_invites"),
            "none of {:?} held the live schema",
            entries
        );
        assert!(
            !stored.contains("TSKruntimelevelltsecret"),
            "the share token reached on-disk storage"
        );

        clean(dir);
    }

    /// The wizard's duplicate-request notice is a stored fact: building a package
    /// for a peer attributes the invite to them, and an accepted one stops being
    /// outstanding. No inference from a trust column is involved.
    #[tokio::test]
    async fn a_duplicate_request_is_read_from_the_invite_ledger() {
        let (runtime, dir) = runtime_with_db();
        let recipient = format!("{PEER}.");
        runtime.onboarding_begin(PEER, "HOST").expect("begun");
        assert!(!runtime.has_outstanding_invite(&recipient));

        let now_ms = wall_now_ms();
        let invite = crate::friends::create_invite(
            &runtime.device_identity_for_test(),
            "Abhijai",
            OUR_PEER,
            now_ms,
            crate::friends::FRIEND_INVITE_TTL_MS,
        )
        .expect("invite");
        let link = crate::friends::encode_invite_link(&invite).expect("link");
        runtime
            .lock()
            .db
            .as_ref()
            .expect("db")
            .record_friend_invite(&invite, "SENT", now_ms)
            .expect("record");

        runtime
            .record_package_recipient(&link, &recipient)
            .await
            .expect("attributed");
        assert!(
            runtime.has_outstanding_invite(&recipient),
            "an attributed, unexpired sent invite is outstanding"
        );

        // Settling it — the friend's own acceptance — retires the notice.
        runtime
            .lock()
            .db
            .as_ref()
            .expect("db")
            .settle_friend_invite(&invite.nonce, "ACCEPTED", wall_now_ms())
            .expect("settle");
        assert!(!runtime.has_outstanding_invite(&recipient));
        clean(dir);
    }

    /// Requirement 9: the invitation is expiring and single-use, and never
    /// carries the Tailscale link.
    #[test]
    fn the_invitation_package_shape_is_enforced_without_network() {
        let (runtime, dir) = runtime_with_db();
        runtime.onboarding_begin(PEER, "HOST").expect("begun");
        // No method chosen yet: refused before anything is signed or read.
        let rt_error = block_on(runtime.onboarding_invitation_package()).unwrap_err();
        assert!(rt_error.starts_with("MP-ONB-034"), "{rt_error}");
        clean(dir);
    }

    /// Builds a signed friend invitation from `runtime` without touching the
    /// network, mirroring `app_runtime::friendship`'s test helper.
    fn onboarding_invite_from(runtime: &AppRuntime) -> String {
        let now_ms = wall_now_ms();
        let invite = crate::friends::create_invite(
            &runtime.device_identity_for_test(),
            "Priya",
            PEER,
            now_ms,
            FRIEND_INVITE_TTL_MS,
        )
        .expect("invite");
        crate::friends::encode_invite_link(&invite).expect("link")
    }

    /// Runs a small future to completion for the one async assertion that needs
    /// it, without pulling a whole runtime into a sync test.
    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(future)
    }
}
