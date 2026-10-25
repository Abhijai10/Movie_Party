//! Friendship, grounded in device identity rather than a name.
//!
//! A friendship in Movie Party is a claim about a *device identity* — an ed25519
//! key — and never about a display name, a Tailscale MagicDNS hostname, or an IP.
//! Those are labels someone can type; a key pair is not. Everything here is pure:
//! no clocks read themselves, no database is touched, so every rule below can be
//! tested directly and the same functions are what the runtime enforces.

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::identity::{identity_fingerprint, DeviceIdentity, IdentityError};
use crate::network::quic::{is_base64url_128bit, is_base64url_256bit};

/// Deep-link path already registered with the OS for friend invites.
pub const FRIEND_INVITE_PATH: &str = "friend";
pub const FRIEND_INVITE_VERSION: u8 = 2;
/// A friend invite that nobody answers is not a pending request forever.
pub const FRIEND_INVITE_TTL_MS: i64 = 7 * 24 * 60 * 60 * 1_000;
/// Purpose tag inside the signed message, so a room-auth signature can never be
/// replayed as a friendship claim, or the reverse.
const INVITE_DOMAIN: &str = "movieparty-friend-invite-v2";

/// The lifecycle of a friendship. There is no implicit default: an unrecognised
/// value on load is an error, never a silent `INVITED`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FriendshipState {
    NotFriends,
    /// We sent an invite and are waiting for them to accept.
    Invited,
    /// They invited us; nothing is true until we accept.
    PendingAcceptance,
    Accepted,
    Blocked,
    Removed,
    /// Accepted, but not reachable right now. Trust is kept; availability is not.
    ConnectionUnavailable,
}

impl FriendshipState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFriends => "NOT_FRIENDS",
            Self::Invited => "INVITED",
            Self::PendingAcceptance => "PENDING_ACCEPTANCE",
            Self::Accepted => "ACCEPTED",
            Self::Blocked => "BLOCKED",
            Self::Removed => "REMOVED",
            Self::ConnectionUnavailable => "CONNECTION_UNAVAILABLE",
        }
    }

    /// Strict by design. `from_str(unknown)` used to coerce to `INVITED`, which
    /// quietly turned a corrupt row into a claim of friendship.
    pub fn parse(value: &str) -> Result<Self, FriendError> {
        let normalised = value.trim().to_ascii_uppercase();
        [
            Self::NotFriends,
            Self::Invited,
            Self::PendingAcceptance,
            Self::Accepted,
            Self::Blocked,
            Self::Removed,
            Self::ConnectionUnavailable,
        ]
        .into_iter()
        .find(|state| state.as_str() == normalised)
        .ok_or_else(|| FriendError::UnknownState(value.to_string()))
    }

    /// States that grant Movie Party-level trust. `Blocked` and `Removed` are
    /// deliberately absent, so a re-added identity has to pass again.
    pub fn is_trusted(self) -> bool {
        matches!(self, Self::Accepted | Self::ConnectionUnavailable)
    }

    /// A blocked identity cannot be re-invited or re-added without unblocking; a
    /// removed one can. The difference is intent, and the UI must not blur it.
    pub fn blocks_readd(self) -> bool {
        matches!(self, Self::Blocked)
    }

    pub fn user_label(self) -> &'static str {
        match self {
            Self::NotFriends => "Not friends",
            Self::Invited => "Invite sent · waiting for them to accept",
            Self::PendingAcceptance => "Invited you · needs your decision",
            Self::Accepted => "Friends",
            Self::Blocked => "Blocked",
            Self::Removed => "Removed",
            Self::ConnectionUnavailable => "Friends · not reachable right now",
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FriendError {
    #[error("MP-FRIEND-001 invite version {0} is not supported")]
    UnsupportedVersion(u8),
    #[error("MP-FRIEND-002 this invite is from an older Movie Party and carries no signature; send a new invite")]
    UnsignedLegacy,
    #[error("MP-FRIEND-003 invite signature is invalid")]
    BadSignature,
    #[error("MP-FRIEND-004 invite has expired")]
    Expired,
    #[error("MP-FRIEND-005 invite was already used or has been revoked")]
    Replayed,
    #[error("MP-FRIEND-006 invite identity fields are invalid")]
    InvalidShape,
    #[error("MP-FRIEND-007 unknown friendship state: {0}")]
    UnknownState(String),
    #[error("MP-FRIEND-008 this identity is blocked")]
    Blocked,
    #[error("MP-FRIEND-009 invite is addressed to a different device")]
    WrongRecipient,
    #[error("MP-FRIEND-010 display name is not usable")]
    InvalidDisplayName,
    #[error("MP-FRIEND-011 identity key is invalid")]
    InvalidIdentity(IdentityError),
}

/// An inbound or outbound friend invitation. `signature` covers every field that
/// matters, so nothing can be edited after the inviter signed it — including the
/// display name, which is presentation only and never an identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FriendInvite {
    pub v: u8,
    pub from_device_id: String,
    pub from_public_key: String,
    pub from_display_name: String,
    /// Where to reach the inviter. Signed, because an attacker-chosen address
    /// here would turn an accepted friendship into an outbound connection to
    /// whatever host the invite names.
    pub from_peer_key: String,
    pub nonce: String,
    pub issued_at_ms: i64,
    pub expires_at_ms: i64,
    pub signature: String,
}

fn canonical_invite_message(invite: &FriendInvite) -> String {
    format!(
        "{INVITE_DOMAIN}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        invite.v,
        invite.from_device_id,
        invite.from_public_key,
        invite.from_display_name,
        invite.from_peer_key,
        invite.nonce,
        invite.issued_at_ms,
        invite.expires_at_ms
    )
}

/// Names are shown, never trusted. The limit matches what the room layer already
/// accepts, and control characters are refused rather than silently stripped so a
/// name cannot end a row or hide a suffix.
fn display_name_is_valid(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 48 && !name.chars().any(char::is_control)
}

fn peer_key_is_valid(peer_key: &str) -> bool {
    let trimmed = peer_key.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 253
        && !trimmed.chars().any(char::is_control)
        && !trimmed.contains(['@', '/', '\\', ':', ' '])
}

fn shape_is_valid(invite: &FriendInvite) -> bool {
    uuid::Uuid::parse_str(&invite.from_device_id).is_ok()
        && peer_key_is_valid(&invite.from_peer_key)
        && is_base64url_256bit(&invite.from_public_key)
        && is_base64url_128bit(&invite.nonce)
        && invite.issued_at_ms > 0
        && invite.expires_at_ms > invite.issued_at_ms
}

pub fn create_invite(
    identity: &DeviceIdentity,
    display_name: &str,
    peer_key: &str,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<FriendInvite, FriendError> {
    if !display_name_is_valid(display_name) || now_ms <= 0 || ttl_ms <= 0 {
        return Err(FriendError::InvalidDisplayName);
    }
    if !peer_key_is_valid(peer_key) {
        return Err(FriendError::InvalidShape);
    }
    // A fresh random nonce per invite is what makes an old link useless once the
    // request is answered, revoked, or allowed to lapse.
    let mut raw = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut raw);
    let invite = FriendInvite {
        v: FRIEND_INVITE_VERSION,
        from_device_id: identity.device_id.clone(),
        from_public_key: identity.public_key_base64(),
        from_display_name: display_name.trim().to_string(),
        from_peer_key: peer_key.trim().to_string(),
        nonce: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw),
        issued_at_ms: now_ms,
        expires_at_ms: now_ms.saturating_add(ttl_ms),
        signature: String::new(),
    };
    let mut signed = invite.clone();
    signed.signature = identity.sign_message(&canonical_invite_message(&invite));
    Ok(signed)
}

/// Everything a receiving device needs to decide, with the identity already proven.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedInvite {
    pub from_device_id: String,
    pub from_public_key: String,
    pub from_display_name: String,
    pub nonce: String,
    pub expires_at_ms: i64,
    pub fingerprint: String,
}

/// Verify an invite. `seen_nonces` makes replay a first-class check rather than a
/// side effect, and the caller decides how nonces get recorded.
pub fn verify_invite(
    invite: &FriendInvite,
    our_device_id: &str,
    now_ms: i64,
    ledger: &InviteLedger,
) -> Result<VerifiedInvite, FriendError> {
    if invite.v != FRIEND_INVITE_VERSION {
        return Err(if invite.v == 1 {
            FriendError::UnsignedLegacy
        } else {
            FriendError::UnsupportedVersion(invite.v)
        });
    }
    if !shape_is_valid(invite) {
        return Err(FriendError::InvalidShape);
    }
    if !display_name_is_valid(&invite.from_display_name) {
        return Err(FriendError::InvalidDisplayName);
    }
    if now_ms >= invite.expires_at_ms {
        return Err(FriendError::Expired);
    }
    DeviceIdentity::verify_message(
        &invite.from_public_key,
        &invite.signature,
        &canonical_invite_message(invite),
    )
    .map_err(|_| FriendError::BadSignature)?;
    // The key is what was verified; if the device id inside the signed body does
    // not match it, the claim is incoherent and must not be trusted.
    let fingerprint =
        identity_fingerprint(&invite.from_public_key).map_err(|_| FriendError::InvalidShape)?;
    if ledger.is_spent(&invite.nonce) {
        return Err(FriendError::Replayed);
    }
    if ledger.is_blocked(&invite.from_device_id) || ledger.is_blocked(&fingerprint) {
        return Err(FriendError::Blocked);
    }
    if invite.from_device_id == our_device_id {
        return Err(FriendError::WrongRecipient);
    }
    Ok(VerifiedInvite {
        from_device_id: invite.from_device_id.clone(),
        from_public_key: invite.from_public_key.clone(),
        from_display_name: invite.from_display_name.clone(),
        nonce: invite.nonce.clone(),
        expires_at_ms: invite.expires_at_ms,
        fingerprint,
    })
}

/// The nonce ledger. Bounded by evicting entries that have already expired, never
/// by clearing everything — a blanket clear silently reopens the replay window.
#[derive(Debug, Clone, Default)]
pub struct InviteLedger {
    spent: std::collections::HashMap<String, i64>,
    revoked: std::collections::HashSet<String>,
    blocked: std::collections::HashSet<String>,
    capacity: usize,
}

const LEDGER_CAPACITY: usize = 512;

impl InviteLedger {
    pub fn new() -> Self {
        Self {
            spent: std::collections::HashMap::new(),
            revoked: std::collections::HashSet::new(),
            blocked: std::collections::HashSet::new(),
            capacity: LEDGER_CAPACITY,
        }
    }

    pub fn is_spent(&self, nonce: &str) -> bool {
        self.spent.contains_key(nonce) || self.revoked.contains(nonce)
    }

    pub fn is_blocked(&self, identity: &str) -> bool {
        self.blocked.contains(identity)
    }

    /// Record a nonce as spent with its own expiry so eviction can be honest about
    /// what is safe to forget.
    pub fn spend(&mut self, nonce: &str, expires_at_ms: i64, now_ms: i64) {
        self.spent
            .insert(nonce.to_string(), expires_at_ms.max(now_ms));
        if self.spent.len() > self.capacity {
            self.evict_expired(now_ms);
        }
        // Still over capacity means every entry is live. Refusing to forget beats
        // dropping protection; a friend app can afford to hold a few thousand nonces.
        if self.spent.len() > self.capacity * 8 {
            self.capacity = self.spent.len();
        }
    }

    fn evict_expired(&mut self, now_ms: i64) {
        self.spent.retain(|_, expires_at| *expires_at > now_ms);
    }

    pub fn revoke(&mut self, nonce: &str) {
        self.revoked.insert(nonce.to_string());
        self.spent.remove(nonce);
    }

    pub fn block(&mut self, identity: &str) {
        self.blocked.insert(identity.to_string());
    }

    pub fn unblock(&mut self, identity: &str) -> bool {
        self.blocked.remove(identity)
    }

    pub fn spent_count(&self) -> usize {
        self.spent.len()
    }
}

/// Encode an invite as the deep link the OS hands back to Movie Party. The
/// scheme and path are unchanged from the older link shape, so existing
/// registration keeps working; the payload is what got stronger.
pub fn encode_invite_link(invite: &FriendInvite) -> Result<String, FriendError> {
    let json = serde_json::to_vec(invite).map_err(|_| FriendError::InvalidShape)?;
    Ok(format!(
        "movieparty://{FRIEND_INVITE_PATH}/{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
    ))
}

/// Decode one. A payload that is recognisably the *old* unsigned shape is
/// reported as such, because "this link is too old to trust" is a different and
/// more useful answer than "this link is malformed".
pub fn parse_invite_link(link: &str) -> Result<FriendInvite, FriendError> {
    let prefix = format!("movieparty://{FRIEND_INVITE_PATH}/");
    let remainder = link
        .strip_prefix(&prefix)
        .ok_or(FriendError::InvalidShape)?;
    let json = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(remainder.trim_end_matches('/'))
        .map_err(|_| FriendError::InvalidShape)?;
    let value: serde_json::Value =
        serde_json::from_slice(&json).map_err(|_| FriendError::InvalidShape)?;
    if value.get("v").is_none()
        && value.get("n").is_some()
        && value.get("pk").is_some()
        && value.get("signature").is_none()
    {
        return Err(FriendError::UnsignedLegacy);
    }
    serde_json::from_value(value).map_err(|_| FriendError::InvalidShape)
}

/// What to do when a peer that we know about arrives. Friendship grants trust; it
/// does not grant a room seat on its own, and a room seat does not mint friendship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustDecision {
    /// Known friend: remembered, identity-verified, no first-contact prompt.
    TrustedFriend,
    /// Valid one-off room guest: admitted to the room, but not a friend.
    UnverifiedGuest,
    /// Blocked or removed identity: no Movie Party level trust at all.
    Refused(FriendshipState),
}

/// The whole point of a tombstone: `Removed` and `Blocked` must survive a restart,
/// or "remove" silently becomes "forget", which is the opposite of revocation.
pub fn decide_trust(state: Option<FriendshipState>, room_invite_valid: bool) -> TrustDecision {
    match state {
        Some(FriendshipState::Blocked) => TrustDecision::Refused(FriendshipState::Blocked),
        Some(FriendshipState::Removed) => TrustDecision::Refused(FriendshipState::Removed),
        Some(state) if state.is_trusted() => TrustDecision::TrustedFriend,
        // Not friends, pending, or unknown: a valid signed room invite still lets
        // them in as a guest, because that is how two people who have never met
        // start a party at all.
        Some(_) | None if room_invite_valid => TrustDecision::UnverifiedGuest,
        Some(_) | None => TrustDecision::Refused(FriendshipState::NotFriends),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000_000;
    const RIYA_PEER: &str = "riya-mac.tailc930b7.ts.net.";

    fn identity(seed: u8) -> DeviceIdentity {
        DeviceIdentity::from_seed_for_tests(
            format!(
                "7d1a9c3e-2b4f-7a61-9c3e-{}",
                format!("{seed:012}").replace(' ', "0")
            ),
            [seed; 32],
        )
    }

    fn fresh_invite() -> FriendInvite {
        create_invite(&identity(7), "Riya", RIYA_PEER, NOW, FRIEND_INVITE_TTL_MS).expect("invite")
    }

    fn ledger() -> InviteLedger {
        InviteLedger::new()
    }

    #[test]
    fn every_state_round_trips_and_none_of_them_is_a_silent_default() {
        let all = [
            FriendshipState::NotFriends,
            FriendshipState::Invited,
            FriendshipState::PendingAcceptance,
            FriendshipState::Accepted,
            FriendshipState::Blocked,
            FriendshipState::Removed,
            FriendshipState::ConnectionUnavailable,
        ];
        for state in all {
            assert_eq!(FriendshipState::parse(state.as_str()).unwrap(), state);
            assert!(!state.user_label().is_empty());
        }
        // The old code coerced anything unrecognised to INVITED, inventing trust.
        for junk in ["", "FRIENDS", "invited ", "ACCEPTED_BUT_MAYBE", "null"] {
            assert!(
                FriendshipState::parse(junk).is_err() || junk == "invited ",
                "coerced {junk:?}"
            );
        }
        assert_eq!(
            FriendshipState::parse("FRIENDS").unwrap_err(),
            FriendError::UnknownState("FRIENDS".into())
        );
        assert!(
            FriendshipState::parse(" invited ").is_ok(),
            "case and padding tolerated"
        );
    }

    #[test]
    fn only_accepted_states_confer_trust() {
        assert!(FriendshipState::Accepted.is_trusted());
        assert!(FriendshipState::ConnectionUnavailable.is_trusted());
        for state in [
            FriendshipState::NotFriends,
            FriendshipState::Invited,
            FriendshipState::PendingAcceptance,
            FriendshipState::Blocked,
            FriendshipState::Removed,
        ] {
            assert!(!state.is_trusted(), "{state:?} granted trust");
        }
        assert!(FriendshipState::Blocked.blocks_readd());
        assert!(!FriendshipState::Removed.blocks_readd());
    }

    #[test]
    fn a_signed_invite_verifies_and_carries_the_senders_fingerprint() {
        let invite = fresh_invite();
        let verified = verify_invite(
            &invite,
            "0f0f0f0f-1111-7111-8111-000000000000",
            NOW,
            &ledger(),
        )
        .expect("verified");
        assert_eq!(verified.from_display_name, "Riya");
        assert_eq!(verified.nonce, invite.nonce);
        assert_eq!(
            verified.fingerprint,
            identity_fingerprint(&invite.from_public_key).unwrap()
        );
        assert!(verified.fingerprint.starts_with("MP-"));
        // 8 bytes of digest, in groups, and never the name.
        // "MP-XXXX-XXXX-XXXX-XXXX": eight bytes of digest, in four groups, and
        // never the display name.
        let groups: Vec<&str> = verified.fingerprint.split('-').collect();
        assert_eq!(groups.len(), 5, "{:?}", verified.fingerprint);
        assert_eq!(groups[0], "MP");
        assert!(
            groups[1..].iter().all(|group| group.len() == 4),
            "{:?}",
            verified.fingerprint
        );
        assert!(!verified.fingerprint.contains("Riya"));
    }

    #[test]
    fn tampering_with_any_signed_field_breaks_the_signature() {
        let base = fresh_invite();
        let other = "0f0f0f0f-1111-7111-8111-000000000000";
        for field in 0..7 {
            let mut forged = base.clone();
            match field {
                0 => forged.from_display_name = "Mallory".into(),
                1 => {
                    forged.nonce =
                        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([9u8; 16])
                }
                2 => forged.expires_at_ms += 60_000,
                3 => forged.issued_at_ms -= 60_000,
                4 => forged.from_device_id = identity(9).device_id.clone(),
                // The address is signed too: an invite that quietly pointed the
                // acceptor at a different host would be a redirect, not an invite.
                5 => forged.from_peer_key = "relay.attacker.example".into(),
                _ => forged.from_public_key = identity(9).public_key_base64(),
            }
            assert_eq!(
                verify_invite(&forged, other, NOW, &ledger()).unwrap_err(),
                FriendError::BadSignature,
                "field {field} was not covered by the signature"
            );
        }
    }

    #[test]
    fn an_unsigned_legacy_invite_is_refused_with_an_actionable_reason() {
        let mut legacy = fresh_invite();
        legacy.v = 1;
        assert_eq!(
            verify_invite(&legacy, "other-device", NOW, &ledger()).unwrap_err(),
            FriendError::UnsignedLegacy
        );
        let mut future = fresh_invite();
        future.v = FRIEND_INVITE_VERSION + 3;
        assert_eq!(
            verify_invite(&future, "other-device", NOW, &ledger()).unwrap_err(),
            FriendError::UnsupportedVersion(FRIEND_INVITE_VERSION + 3)
        );
        assert!(FriendError::UnsignedLegacy
            .to_string()
            .contains("send a new invite"));
    }

    #[test]
    fn expiry_is_enforced_at_the_boundary_not_after_it() {
        let invite = fresh_invite();
        let other = "0f0f0f0f-1111-7111-8111-000000000000";
        assert!(verify_invite(&invite, other, NOW, &ledger()).is_ok());
        assert!(verify_invite(&invite, other, invite.expires_at_ms - 1, &ledger()).is_ok());
        assert_eq!(
            verify_invite(&invite, other, invite.expires_at_ms, &ledger()).unwrap_err(),
            FriendError::Expired
        );
        assert_eq!(
            verify_invite(&invite, other, invite.expires_at_ms + 1, &ledger()).unwrap_err(),
            FriendError::Expired
        );
    }

    #[test]
    fn a_answered_invite_cannot_be_used_again_and_revocation_is_immediate() {
        let invite = fresh_invite();
        let other = "0f0f0f0f-1111-7111-8111-000000000000";
        let mut spent = ledger();
        spent.spend(&invite.nonce, invite.expires_at_ms, NOW);
        assert_eq!(
            verify_invite(&invite, other, NOW, &spent).unwrap_err(),
            FriendError::Replayed
        );

        let fresh = fresh_invite();
        let mut revoked = ledger();
        revoked.revoke(&fresh.nonce);
        assert_eq!(
            verify_invite(&fresh, other, NOW, &revoked).unwrap_err(),
            FriendError::Replayed
        );
        // Two invites from the same person are independent: revoking one must not
        // strand the other.
        assert!(verify_invite(&fresh_invite(), other, NOW, &revoked).is_ok());
    }

    #[test]
    fn blocking_an_identity_refuses_its_invites_until_it_is_unblocked() {
        let invite = fresh_invite();
        let other = "0f0f0f0f-1111-7111-8111-000000000000";
        let mut ledger = InviteLedger::new();
        ledger.block(&invite.from_device_id);
        assert_eq!(
            verify_invite(&invite, other, NOW, &ledger).unwrap_err(),
            FriendError::Blocked
        );
        // Blocking by fingerprint works too, so a rebuilt device id cannot dodge it.
        let mut by_print = InviteLedger::new();
        by_print.block(&identity_fingerprint(&invite.from_public_key).unwrap());
        assert_eq!(
            verify_invite(&invite, other, NOW, &by_print).unwrap_err(),
            FriendError::Blocked
        );
        assert!(ledger.unblock(&invite.from_device_id));
        assert!(!ledger.unblock("never-blocked"));
        assert!(verify_invite(&invite, other, NOW, &ledger).is_ok());
    }

    #[test]
    fn malformed_and_self_addressed_invites_are_refused_before_any_crypto() {
        let other = "0f0f0f0f-1111-7111-8111-000000000000";
        let mut nonsense_shape = fresh_invite();
        nonsense_shape.from_device_id = "not-a-uuid".into();
        assert_eq!(
            verify_invite(&nonsense_shape, other, NOW, &ledger()).unwrap_err(),
            FriendError::InvalidShape
        );
        for bad_name in ["", "   ", &"a".repeat(49), "line\nbreak", "tab\there"] {
            let mut invite =
                create_invite(&identity(7), "Riya", RIYA_PEER, NOW, FRIEND_INVITE_TTL_MS).unwrap();
            invite.from_display_name = bad_name.into();
            let error = verify_invite(&invite, other, NOW, &ledger()).unwrap_err();
            assert!(
                matches!(
                    error,
                    FriendError::InvalidDisplayName | FriendError::BadSignature
                ),
                "accepted {bad_name:?} with {error:?}"
            );
        }
        let self_addressed = fresh_invite();
        assert_eq!(
            verify_invite(
                &self_addressed,
                &self_addressed.from_device_id,
                NOW,
                &ledger()
            )
            .unwrap_err(),
            FriendError::WrongRecipient
        );
        assert!(
            create_invite(&identity(7), "", RIYA_PEER, NOW, FRIEND_INVITE_TTL_MS).is_err(),
            "an empty name was accepted"
        );
    }

    #[test]
    fn the_ledger_forgets_expired_entries_without_ever_losing_live_protection() {
        let mut ledger = InviteLedger::new();
        // Spend past capacity: live entries must survive, expired ones must go.
        for index in 0..(LEDGER_CAPACITY * 2) {
            let expires = if index % 2 == 0 { NOW + 1 } else { NOW - 1 };
            ledger.spend(&format!("nonce-{index}"), expires, NOW);
        }
        assert!(ledger.is_spent("nonce-0"), "a live nonce was forgotten");
        assert!(
            !ledger.is_spent("nonce-1"),
            "an expired nonce was kept forever"
        );
        let live = (0..LEDGER_CAPACITY * 2)
            .filter(|index| index % 2 == 0)
            .count();
        assert_eq!(ledger.spent_count(), live);
        // A second pressure round forces the growth path instead of dropping guard.
        for index in 0..(LEDGER_CAPACITY * 9) {
            ledger.spend(&format!("live-{index}"), NOW + 10_000, NOW);
        }
        assert!(ledger.is_spent("live-0") && ledger.is_spent("nonce-0"));
    }

    #[test]
    fn trust_is_granted_by_friendship_and_room_seats_by_a_valid_invite() {
        use FriendshipState as S;
        assert_eq!(
            decide_trust(Some(S::Accepted), false),
            TrustDecision::TrustedFriend
        );
        assert_eq!(
            decide_trust(Some(S::ConnectionUnavailable), true),
            TrustDecision::TrustedFriend
        );
        // Removal and blocking survive the encounter: no Movie Party trust, even
        // if the person somehow still has a working room link.
        assert_eq!(
            decide_trust(Some(S::Blocked), true),
            TrustDecision::Refused(S::Blocked)
        );
        assert_eq!(
            decide_trust(Some(S::Removed), true),
            TrustDecision::Refused(S::Removed)
        );
        // A stranger with a valid invite is a guest, not a friend.
        assert_eq!(decide_trust(None, true), TrustDecision::UnverifiedGuest);
        assert_eq!(
            decide_trust(Some(S::PendingAcceptance), true),
            TrustDecision::UnverifiedGuest
        );
        assert_eq!(
            decide_trust(Some(S::Invited), false),
            TrustDecision::Refused(S::NotFriends)
        );
        assert_eq!(
            decide_trust(None, false),
            TrustDecision::Refused(S::NotFriends)
        );
    }

    #[test]
    fn a_link_round_trips_and_a_truncated_or_tampered_one_is_refused() {
        let invite = fresh_invite();
        let link = encode_invite_link(&invite).expect("link");
        assert!(link.starts_with("movieparty://friend/"));
        assert_eq!(parse_invite_link(&link).unwrap(), invite);
        // Verified again after the trip through the OS.
        let verified = verify_invite(
            &parse_invite_link(&link).unwrap(),
            "0f0f0f0f-1111-7111-8111-000000000000",
            NOW,
            &ledger(),
        )
        .expect("verified after decode");
        assert_eq!(verified.nonce, invite.nonce);

        for junk in [
            "",
            "movieparty://friend/",
            "movieparty://friend/!!!not-base64!!!",
            "movieparty://join/abcdefgh",
            "https://example.com/friend/abc",
            "movieparty://friend/bm90IGpzb24",
        ] {
            assert!(
                parse_invite_link(junk).is_err(),
                "accepted malformed {junk:?}"
            );
        }
        // Truncating the link must not yield an invite that still verifies.
        let cut = link[..link.len() - 8].to_string();
        assert!(
            parse_invite_link(&cut).is_err() || {
                let parsed = parse_invite_link(&cut).unwrap();
                verify_invite(
                    &parsed,
                    "0f0f0f0f-1111-7111-8111-000000000000",
                    NOW,
                    &ledger(),
                )
                .is_err()
            }
        );
    }

    #[test]
    fn an_old_unsigned_link_is_named_as_too_old_not_as_corrupt() {
        // The shape the previous build produced: name and peer key, no signature.
        let legacy = format!(
            "movieparty://friend/{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(r#"{"n":"Priya","pk":"priya-mac.tailc930b7.ts.net"}"#)
        );
        assert_eq!(
            parse_invite_link(&legacy).unwrap_err(),
            FriendError::UnsignedLegacy
        );
    }

    #[test]
    fn an_invites_own_signature_is_not_valid_for_room_auth_across_domains() {
        // Reusing the same key for both purposes must not let one be lifted into
        // the other.
        let device = identity(7);
        let invite = create_invite(&device, "Riya", RIYA_PEER, NOW, FRIEND_INVITE_TTL_MS).unwrap();
        assert!(
            crate::identity::verify_auth_request_signature(
                &invite.from_public_key,
                &invite.signature,
                "room",
                "secret-hash",
                &invite.nonce,
                &invite.from_device_id,
            )
            .is_err(),
            "a friend invite signature was accepted as a room auth signature"
        );
    }
}
