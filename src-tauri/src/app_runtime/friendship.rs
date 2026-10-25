use super::*;
use crate::friends::{self, FriendshipState};
use crate::identity::identity_fingerprint;
use crate::storage::sqlite::{FriendConnectionState, StoredFriend};

/// An invitation the user is being asked to decide on.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomingInvite {
    pub nonce: String,
    pub display_name: String,
    pub peer_key: String,
    pub fingerprint: String,
    pub expires_at_ms: i64,
    /// A position we already hold on this identity, so an old link does not look
    /// like a fresh request from a stranger.
    pub known_state: Option<FriendshipState>,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SentInvite {
    pub link: String,
    pub expires_at_ms: i64,
    /// Our own fingerprint, so the other person can read it back over any channel
    /// and confirm they are accepting this device rather than a look-alike.
    pub fingerprint: String,
}

impl AppRuntime {
    /// Issue a signed invitation. The link is an offer; only the recipient's own
    /// acceptance makes it a friendship.
    pub async fn friend_invite_signed(&self) -> Result<SentInvite, String> {
        let status = crate::network::tailscale::detect_status()
            .await
            .map_err(|error| error.to_string())?;
        let readiness = crate::network::tailscale::readiness_from_status(status.clone());
        if !readiness.is_usable() && !crate::network::tailscale::dev_loopback_enabled() {
            return Err(readiness
                .stable_error()
                .unwrap_or_else(|| "MP-NET-TS-003 Tailscale is not ready".to_string()));
        }
        let own_name = status
            .device_name
            .filter(|name| name.contains('.'))
            .ok_or("MP-NET-TS-004 this device has no tailnet name yet".to_string())?;
        let peer_key = normalised_peer_key(&own_name);

        let identity = self.inner.identity();
        let display_name = {
            let state = self.lock();
            state
                .db
                .as_ref()
                .and_then(|db| db.get_identity().ok().flatten())
                .map(|record| record.display_name)
                .filter(|name| !name.trim().is_empty())
        }
        .unwrap_or_else(|| crate::network::tailscale::peer_display_name(&peer_key));

        let now_ms = wall_now_ms();
        let invite = friends::create_invite(
            &identity,
            &display_name,
            &peer_key,
            now_ms,
            friends::FRIEND_INVITE_TTL_MS,
        )
        .map_err(|error| error.to_string())?;
        let link = friends::encode_invite_link(&invite).map_err(|error| error.to_string())?;
        if let Some(db) = self.lock().db.clone() {
            db.record_friend_invite(&invite, "SENT", now_ms)
                .map_err(|error| error.to_string())?;
        }
        Ok(SentInvite {
            fingerprint: identity_fingerprint(&invite.from_public_key)
                .map_err(|e| e.to_string())?,
            link,
            expires_at_ms: invite.expires_at_ms,
        })
    }

    /// Read an invitation **without changing anything**. A deep link handed over by
    /// the operating system is something to look at, not something to act on: this
    /// replaces the previous behaviour where arriving at the screen committed it.
    pub fn preview_friend_invite(&self, link: &str) -> Result<IncomingInvite, String> {
        let invite = friends::parse_invite_link(link).map_err(|error| error.to_string())?;
        let our_device_id = self.inner.identity().device_id;
        let verified = friends::verify_invite(
            &invite,
            &our_device_id,
            wall_now_ms(),
            &friends::InviteLedger::new(),
        )
        .map_err(|error| error.to_string())?;

        let now_ms = wall_now_ms();
        let peer_key = normalised_peer_key(&invite.from_peer_key);
        let known_state = self.trust_for(&invite.from_device_id, &invite.from_public_key);
        if let Some(db) = self.lock().db.clone() {
            // Idempotent on the nonce: showing the same link twice cannot create a
            // second decision or extend anything.
            let _ = db.record_friend_invite(&invite, "RECEIVED", now_ms);
        }
        let warning = match known_state {
            Some(FriendshipState::Blocked) => Some(
                "You blocked this device. Accepting will not lift that block by itself.".to_string(),
            ),
            Some(FriendshipState::Removed) => Some(
                "You removed this device before. Its identity key matches, so it is the same device."
                    .to_string(),
            ),
            _ => None,
        };
        Ok(IncomingInvite {
            nonce: verified.nonce,
            display_name: invite.from_display_name,
            peer_key,
            fingerprint: verified.fingerprint,
            expires_at_ms: invite.expires_at_ms,
            known_state,
            warning,
        })
    }

    pub fn pending_friend_invites(&self) -> Vec<(String, String, String)> {
        self.lock()
            .db
            .as_ref()
            .and_then(|db| db.pending_friend_invites(wall_now_ms()).ok())
            .unwrap_or_default()
    }

    /// Accept an invitation. The row is settled atomically, so a double click or a
    /// second window cannot apply the decision twice.
    pub async fn accept_friend_invitation(&self, nonce: String) -> Result<StoredFriend, String> {
        let now_ms = wall_now_ms();
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        let invite = db
            .friend_invite_row(&nonce)
            .map_err(|error| error.to_string())?
            .ok_or("MP-FRIEND-005 invite was already used or has been revoked")?;
        if invite.outcome != "PENDING" {
            return Err("MP-FRIEND-005 invite was already used or has been revoked".to_string());
        }
        if now_ms >= invite.expires_at_ms {
            let _ = db.settle_friend_invite(&nonce, "EXPIRED", now_ms);
            return Err("MP-FRIEND-004 invite has expired".to_string());
        }
        if matches!(
            db.trust_for_identity(&invite.device_id, &invite.public_key)
                .map_err(|error| error.to_string())?,
            Some(FriendshipState::Blocked)
        ) {
            // A block is the user's earlier and more specific decision; an
            // accidental tap on a link they never meant to open must not overturn
            // it silently.
            return Err("MP-FRIEND-008 this identity is blocked".to_string());
        }
        if !db
            .settle_friend_invite(&nonce, "ACCEPTED", now_ms)
            .map_err(|error| error.to_string())?
        {
            return Err("MP-FRIEND-005 invite was already used or has been revoked".to_string());
        }

        let peer_key = normalised_peer_key(&invite.peer_key);
        let observed = self.observed_peer_ip(&peer_key).await;
        let existing = self
            .list_friends()
            .into_iter()
            .find(|friend| friend.peer_key == peer_key);
        let friend = StoredFriend {
            display_name: existing
                .as_ref()
                .map(|saved| saved.display_name.clone())
                // The name came from a signed field, but it is still only a label:
                // a name the user chose locally always wins.
                .unwrap_or(invite.display_name),
            ip: observed.or_else(|| existing.as_ref().and_then(|saved| saved.ip.clone())),
            added_at_ms: existing
                .as_ref()
                .map(|saved| saved.added_at_ms)
                .unwrap_or(now_ms),
            // A link is not a ping. Verification stays `verify_friend`'s job.
            last_verified_at_ms: existing
                .as_ref()
                .and_then(|saved| saved.last_verified_at_ms),
            last_path: existing.as_ref().and_then(|saved| saved.last_path.clone()),
            last_latency_ms: existing.as_ref().and_then(|saved| saved.last_latency_ms),
            connection_state: existing
                .as_ref()
                .map(|saved| saved.connection_state)
                .unwrap_or(FriendConnectionState::Invited),
            peer_key: peer_key.clone(),
        };
        db.upsert_friend(&friend)
            .map_err(|error| error.to_string())?;
        db.set_friend_identity(&peer_key, &invite.device_id, &invite.public_key)
            .map_err(|error| error.to_string())?;
        db.set_friend_trust_state(&peer_key, FriendshipState::Accepted, now_ms)
            .map_err(|error| error.to_string())?;
        Ok(friend)
    }

    pub fn decline_friend_invitation(&self, nonce: String) -> Result<(), String> {
        let now_ms = wall_now_ms();
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        if !db
            .settle_friend_invite(&nonce, "DECLINED", now_ms)
            .map_err(|error| error.to_string())?
        {
            // Declining something already decided would report a success that
            // changed nothing, so it is refused exactly like a second acceptance.
            return Err("MP-FRIEND-005 invite was already used or has been revoked".to_string());
        }
        Ok(())
    }

    /// Every friendship position at once, keyed by peer address. The status UI
    /// needs all of them per refresh, and asking per row would multiply a round
    /// trip for no reason.
    pub fn friendship_states(&self) -> Vec<(String, FriendshipState)> {
        let keys: Vec<String> = self
            .list_friends()
            .into_iter()
            .map(|f| f.peer_key)
            .collect();
        let db = match self.lock().db.clone() {
            Some(db) => db,
            None => return Vec::new(),
        };
        keys.into_iter()
            .filter_map(|key| {
                db.friendship_state(&key)
                    .ok()
                    .flatten()
                    .map(|state| (key, state))
            })
            .collect()
    }

    pub fn block_friend(&self, peer_key: &str) -> Result<FriendshipState, String> {
        self.set_trust(peer_key, FriendshipState::Blocked)
    }

    pub fn unblock_friend(&self, peer_key: &str) -> Result<FriendshipState, String> {
        // Unblocking lands on `REMOVED`, not on friendship: lifting a block is not
        // the same act as re-accepting a device.
        self.set_trust(peer_key, FriendshipState::Removed)
    }

    /// Removal records a position instead of deleting the row, so the revocation
    /// outlives the link that created it.
    pub fn remove_friend_trust(&self, peer_key: &str) -> Result<FriendshipState, String> {
        self.set_trust(peer_key, FriendshipState::Removed)
    }

    pub fn friendship_state(&self, peer_key: &str) -> Option<FriendshipState> {
        self.lock()
            .db
            .as_ref()
            .and_then(|db| db.friendship_state(&normalised_peer_key(peer_key)).ok())
            .flatten()
    }

    fn set_trust(&self, peer_key: &str, state: FriendshipState) -> Result<FriendshipState, String> {
        let now_ms = wall_now_ms();
        let db = self
            .lock()
            .db
            .clone()
            .ok_or_else(|| "MP-STORE-001 no database".to_string())?;
        let key = normalised_peer_key(peer_key);
        if !self.list_friends().iter().any(|f| f.peer_key == key) {
            return Err("MP-FRIEND-012 no saved friend with that address".to_string());
        }
        db.set_friend_trust_state(&key, state, now_ms)
            .map_err(|error| error.to_string())?;
        Ok(state)
    }

    fn trust_for(&self, device_id: &str, public_key: &str) -> Option<FriendshipState> {
        self.lock()
            .db
            .as_ref()
            .and_then(|db| db.trust_for_identity(device_id, public_key).ok())
            .flatten()
    }

    async fn observed_peer_ip(&self, peer_key: &str) -> Option<String> {
        let status = crate::network::tailscale::detect_status().await.ok()?;
        status
            .peers
            .iter()
            .find(|peer| peer.dns_name == peer_key)
            .and_then(|peer| peer.usable_ipv4())
            .map(|ip| ip.to_string())
    }
}

fn normalised_peer_key(value: &str) -> String {
    format!("{}.", value.trim().trim_end_matches('.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_runtime::AppRuntime;

    /// Invites are verified against the real clock, so test invitations have to
    /// be built from it rather than from a fixed instant.
    fn now() -> i64 {
        wall_now_ms()
    }
    const PEER: &str = "priya-mac.tailc930b7.ts.net";

    /// A link from `runtime`, built without touching the network.
    fn link_from(runtime: &AppRuntime, now_ms: i64) -> String {
        let invite = friends::create_invite(
            &runtime.device_identity_for_test(),
            "Priya",
            PEER,
            now_ms,
            friends::FRIEND_INVITE_TTL_MS,
        )
        .expect("invite");
        friends::encode_invite_link(&invite).expect("link")
    }

    fn runtime_pair() -> (AppRuntime, AppRuntime) {
        (AppRuntime::new(), AppRuntime::new())
    }

    fn with_db(runtime: &AppRuntime) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mp-friendship-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        runtime.init_db_at_path(&dir.join("mp.db"));
        assert!(
            runtime.lock().db.is_some(),
            "the test database did not open"
        );
        dir
    }

    #[test]
    fn previewing_a_link_changes_nothing_at_all() {
        let (inviter, recipient) = runtime_pair();
        let dir = with_db(&recipient);
        let link = link_from(&inviter, now());

        let preview = recipient.preview_friend_invite(&link).expect("preview");
        assert_eq!(preview.display_name, "Priya");
        assert_eq!(preview.peer_key, format!("{PEER}."));
        assert!(preview.fingerprint.starts_with("MP-"));
        assert_eq!(preview.known_state, None);
        // The regression this step exists for: reading an offer must not create
        // a friend.
        assert!(recipient.list_friends().is_empty(), "preview committed it");
        assert_eq!(recipient.pending_friend_invites().len(), 1);
        // Showing the same link twice is not two requests.
        recipient.preview_friend_invite(&link).expect("re-preview");
        assert_eq!(recipient.pending_friend_invites().len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn acceptance_creates_the_friendship_and_binds_the_identity() {
        let (inviter, recipient) = runtime_pair();
        let dir = with_db(&recipient);
        let link = link_from(&inviter, now());
        let preview = recipient.preview_friend_invite(&link).expect("preview");
        let friend = recipient
            .accept_friend_invitation(preview.nonce.clone())
            .await
            .expect("accept");
        assert_eq!(friend.peer_key, format!("{PEER}."));
        assert_eq!(friend.display_name, "Priya");
        // A link is not a ping: acceptance claims no verification.
        assert_eq!(friend.last_verified_at_ms, None);
        assert_eq!(
            recipient.friendship_state(&friend.peer_key),
            Some(FriendshipState::Accepted)
        );
        let stored = recipient.lock().db.clone().expect("db");
        let row = stored
            .friend_invite_row(&preview.nonce)
            .expect("read")
            .expect("row");
        assert_eq!(row.outcome, "ACCEPTED");
        assert_eq!(row.device_id, inviter.device_identity_for_test().device_id);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn a_decision_can_be_taken_once_and_only_once() {
        let (inviter, recipient) = runtime_pair();
        let dir = with_db(&recipient);
        let nonce = recipient
            .preview_friend_invite(&link_from(&inviter, now()))
            .expect("preview")
            .nonce;
        recipient
            .accept_friend_invitation(nonce.clone())
            .await
            .expect("first");
        for reason in [
            recipient
                .accept_friend_invitation(nonce.clone())
                .await
                .unwrap_err(),
            recipient
                .decline_friend_invitation(nonce.clone())
                .unwrap_err(),
        ] {
            assert!(reason.contains("MP-FRIEND-005"), "said: {reason}");
        }
        assert_eq!(recipient.list_friends().len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn declining_closes_the_door_without_creating_a_friend() {
        let (inviter, recipient) = runtime_pair();
        let dir = with_db(&recipient);
        let nonce = recipient
            .preview_friend_invite(&link_from(&inviter, now()))
            .expect("preview")
            .nonce;
        recipient
            .decline_friend_invitation(nonce.clone())
            .expect("decline");
        assert!(recipient.list_friends().is_empty());
        assert!(recipient.pending_friend_invites().is_empty());
        assert!(recipient.accept_friend_invitation(nonce).await.is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn an_expired_invitation_cannot_be_accepted_even_from_its_own_row() {
        let (inviter, recipient) = runtime_pair();
        let dir = with_db(&recipient);
        let nonce = recipient
            .preview_friend_invite(&link_from(&inviter, now()))
            .expect("preview")
            .nonce;
        // Time passes for real in production; moving the record back reaches the
        // same state an acceptance has to survive.
        recipient
            .lock()
            .db
            .clone()
            .expect("db")
            .execute_sql_for_test("UPDATE friend_invites SET expires_at_ms = 1")
            .expect("age the invite");
        assert_eq!(
            recipient
                .accept_friend_invitation(nonce.clone())
                .await
                .unwrap_err(),
            "MP-FRIEND-004 invite has expired"
        );
        assert_eq!(
            recipient
                .lock()
                .db
                .clone()
                .expect("db")
                .friend_invite_row(&nonce)
                .expect("read")
                .expect("row")
                .outcome,
            "EXPIRED",
            "an expired invite stayed open for a later retry"
        );
        assert!(recipient.list_friends().is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn a_block_outranks_a_link_that_arrives_after_it() {
        let (inviter, recipient) = runtime_pair();
        let dir = with_db(&recipient);
        let preview = recipient
            .preview_friend_invite(&link_from(&inviter, now()))
            .expect("preview");
        recipient
            .accept_friend_invitation(preview.nonce.clone())
            .await
            .expect("accept");
        recipient.block_friend(&preview.peer_key).expect("block");
        assert_eq!(
            recipient.friendship_state(&preview.peer_key),
            Some(FriendshipState::Blocked)
        );

        let again = recipient
            .preview_friend_invite(&link_from(&inviter, now() + 1_000))
            .expect("preview");
        assert_eq!(again.known_state, Some(FriendshipState::Blocked));
        assert!(again.warning.expect("a warning").contains("blocked"));
        assert_eq!(
            recipient
                .accept_friend_invitation(again.nonce)
                .await
                .unwrap_err(),
            "MP-FRIEND-008 this identity is blocked"
        );
        // Unblocking lands on REMOVED: the peer has to invite again.
        recipient
            .unblock_friend(&preview.peer_key)
            .expect("unblock");
        assert_eq!(
            recipient.friendship_state(&preview.peer_key),
            Some(FriendshipState::Removed)
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn removal_leaves_a_position_a_stale_link_cannot_silently_undo() {
        let (inviter, recipient) = runtime_pair();
        let dir = with_db(&recipient);
        let preview = recipient
            .preview_friend_invite(&link_from(&inviter, now()))
            .expect("preview");
        recipient
            .accept_friend_invitation(preview.nonce.clone())
            .await
            .expect("accept");
        recipient
            .remove_friend_trust(&preview.peer_key)
            .expect("remove");
        assert_eq!(
            recipient.friendship_state(&preview.peer_key),
            Some(FriendshipState::Removed)
        );
        // The row keeps the identity, which is what makes the warning possible:
        // a hard delete would have forgotten who this was.
        let again = recipient
            .preview_friend_invite(&link_from(&inviter, now() + 2_000))
            .expect("preview");
        assert_eq!(again.known_state, Some(FriendshipState::Removed));
        assert!(again.warning.expect("a warning").contains("same device"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn actions_on_an_unknown_peer_are_refused_rather_than_inventing_one() {
        let (_, recipient) = runtime_pair();
        let dir = with_db(&recipient);
        for reason in [
            recipient.block_friend("nobody.ts.net").unwrap_err(),
            recipient.remove_friend_trust("nobody.ts.net").unwrap_err(),
        ] {
            assert!(reason.contains("MP-FRIEND-012"), "said: {reason}");
        }
        assert!(recipient.list_friends().is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn accepting_your_own_invitation_is_refused() {
        let (only, _) = runtime_pair();
        let dir = with_db(&only);
        assert!(
            only.preview_friend_invite(&link_from(&only, now()))
                .unwrap_err()
                .contains("MP-FRIEND-009"),
            "a self-addressed invite was offered for acceptance"
        );
        assert!(only.list_friends().is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_link_from_the_previous_build_is_named_as_too_old() {
        let (_, recipient) = runtime_pair();
        let dir = with_db(&recipient);
        let legacy = format!(
            "movieparty://friend/{}",
            crate::network::tailscale::base64url_encode(
                br#"{"n":"Priya","pk":"priya-mac.tailc930b7.ts.net"}"#
            )
        );
        assert!(
            recipient
                .preview_friend_invite(&legacy)
                .unwrap_err()
                .contains("send a new invite"),
            "an unsigned link was still treated as an invitation"
        );
        assert!(recipient.list_friends().is_empty());
        std::fs::remove_dir_all(dir).ok();
    }
}
