//! Negotiated host authority over content, independent of transport stream order.
//! This wire model deliberately contains no account/session/browser secrets.
use super::content::{parse_content, ContentIdentity};
use serde::{Deserialize, Serialize};
pub const SELECTION_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionContext {
    pub state_epoch: String,
    pub host_sequence: u64,
    pub content_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderSelection {
    pub version: u16,
    pub room_id: String,
    pub state_epoch: String,
    pub host_sequence: u64,
    pub content_revision: u64,
    pub provider: String,
    pub content_id: String,
    pub canonical_url: String,
    pub title: String,
    /// Public active-player evidence, not a digest of a URL or login state.
    pub evidence: ContentEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentEvidence {
    pub title: String,
    pub duration_ms: u64,
    pub player_content_id: Option<String>,
}
impl ContentEvidence {
    pub fn valid_for(&self, provider: &str, content_id: &str) -> bool {
        !self.title.trim().is_empty()
            && self.title.len() <= 1200
            && !self.title.chars().any(char::is_control)
            && self.duration_ms > 0
            && self.duration_ms < 7 * 24 * 60 * 60 * 1000
            && (provider != "youtube" || self.player_content_id.as_deref() == Some(content_id))
            && self
                .player_content_id
                .as_ref()
                .is_none_or(|id| id == content_id)
    }
    pub fn matches(&self, other: &Self, provider: &str, content_id: &str) -> bool {
        self.valid_for(provider, content_id)
            && other.valid_for(provider, content_id)
            && self.title.split_whitespace().collect::<Vec<_>>()
                == other.title.split_whitespace().collect::<Vec<_>>()
            && self.duration_ms.abs_diff(other.duration_ms) <= 1500
            && self.player_content_id == other.player_content_id
    }
}
impl ProviderSelection {
    pub fn context(&self) -> SelectionContext {
        SelectionContext {
            state_epoch: self.state_epoch.clone(),
            host_sequence: self.host_sequence,
            content_revision: self.content_revision,
        }
    }

    pub fn validate(&self, room: &str, epoch: &str) -> Result<ContentIdentity, String> {
        if self.version != SELECTION_VERSION {
            return Err("PROVIDER_SELECTION_VERSION_UNSUPPORTED".into());
        }
        if self.room_id != room
            || self.state_epoch != epoch
            || uuid::Uuid::parse_str(epoch).is_err()
            || self.host_sequence == 0
            || self.content_revision == 0
        {
            return Err("PROVIDER_SELECTION_CONTEXT_MISMATCH".into());
        }
        let identity = parse_content(&self.provider, &self.canonical_url)?;
        if identity.content_id != self.content_id
            || identity.allowed_url != self.canonical_url
            || self.title != self.evidence.title
            || !self.evidence.valid_for(&self.provider, &self.content_id)
        {
            return Err("PROVIDER_CONTENT_IDENTITY_UNVERIFIED".into());
        }
        Ok(identity)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContentLoadingState {
    #[default]
    Unselected,
    Opening,
    ContentConfirmed,
    PlaybackReady,
    Error,
    AuthRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionAck {
    pub state_epoch: String,
    pub host_sequence: u64,
    pub content_revision: u64,
    pub acknowledgement_sequence: u64,
    pub state: ContentLoadingState,
    pub observed: Option<ContentEvidence>,
}
impl SelectionAck {
    pub fn for_selection(
        selection: &ProviderSelection,
        sequence: u64,
        state: ContentLoadingState,
        observed: Option<ContentEvidence>,
    ) -> Self {
        Self {
            state_epoch: selection.state_epoch.clone(),
            host_sequence: selection.host_sequence,
            content_revision: selection.content_revision,
            acknowledgement_sequence: sequence,
            state,
            observed,
        }
    }
    fn validates(&self, selection: &ProviderSelection) -> bool {
        self.state_epoch == selection.state_epoch
            && self.host_sequence == selection.host_sequence
            && self.content_revision == selection.content_revision
            && self.acknowledgement_sequence > 0
            && match self.state {
                ContentLoadingState::ContentConfirmed | ContentLoadingState::PlaybackReady => {
                    self.observed.as_ref().is_some_and(|e| {
                        selection
                            .evidence
                            .matches(e, &selection.provider, &selection.content_id)
                    })
                }
                ContentLoadingState::Unselected => false,
                _ => self.observed.is_none(),
            }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRoomDescriptor {
    pub version: u16,
    pub state_epoch: String,
    pub selection: Option<ProviderSelection>,
}
impl ProviderRoomDescriptor {
    pub fn validate(&self, room: &str) -> Result<(), String> {
        if self.version != SELECTION_VERSION || uuid::Uuid::parse_str(&self.state_epoch).is_err() {
            return Err("PROVIDER_SELECTION_VERSION_UNSUPPORTED".into());
        }
        if let Some(s) = &self.selection {
            s.validate(room, &self.state_epoch)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ProviderRoomAuthority {
    pub descriptor: ProviderRoomDescriptor,
    pub guest_ack: Option<SelectionAck>,
    pub acknowledged_at: Option<std::time::Instant>,
    pub peer_version: Option<u16>,
    pub peer_connected: bool,
}
impl Default for ProviderRoomAuthority {
    fn default() -> Self {
        Self {
            descriptor: ProviderRoomDescriptor {
                version: SELECTION_VERSION,
                state_epoch: uuid::Uuid::now_v7().to_string(),
                selection: None,
            },
            guest_ack: None,
            acknowledged_at: None,
            peer_version: None,
            peer_connected: false,
        }
    }
}
impl ProviderRoomAuthority {
    pub fn publish(
        &mut self,
        room: &str,
        identity: &ContentIdentity,
        evidence: ContentEvidence,
    ) -> Result<ProviderSelection, String> {
        if self.peer_connected && self.peer_version != Some(SELECTION_VERSION) {
            return Err(
                "The guest must update Movie Party before joining a Provider Sync room.".into(),
            );
        }
        if let Some(old) = &self.descriptor.selection {
            if old.provider == identity.provider_id
                && old.content_id == identity.content_id
                && old.canonical_url == identity.allowed_url
                && old
                    .evidence
                    .matches(&evidence, &identity.provider_id, &identity.content_id)
            {
                return Ok(old.clone());
            }
        }
        let next = self.descriptor.selection.as_ref().map_or(Ok(1), |s| {
            s.host_sequence
                .checked_add(1)
                .ok_or("Provider selection sequence exhausted")
        })?;
        let selection = ProviderSelection {
            version: SELECTION_VERSION,
            room_id: room.into(),
            state_epoch: self.descriptor.state_epoch.clone(),
            host_sequence: next,
            content_revision: next,
            provider: identity.provider_id.clone(),
            content_id: identity.content_id.clone(),
            canonical_url: identity.allowed_url.clone(),
            title: evidence.title.clone(),
            evidence,
        };
        selection.validate(room, &self.descriptor.state_epoch)?;
        self.descriptor.selection = Some(selection.clone());
        self.guest_ack = None;
        self.acknowledged_at = None;
        Ok(selection)
    }
    pub fn acknowledge(&mut self, ack: SelectionAck) -> Result<(), String> {
        let selection = self
            .descriptor
            .selection
            .as_ref()
            .ok_or("No provider selection")?;
        if !ack.validates(selection) {
            return Err("PROVIDER_ACK_IDENTITY_MISMATCH".into());
        }
        if let Some(old) = &self.guest_ack {
            if ack.acknowledgement_sequence < old.acknowledgement_sequence
                || (ack.acknowledgement_sequence == old.acknowledgement_sequence && &ack != old)
            {
                return Err("PROVIDER_ACK_STALE".into());
            }
        }
        self.guest_ack = Some(ack);
        self.acknowledged_at = Some(std::time::Instant::now());
        Ok(())
    }
    pub fn guest_ready(&self) -> bool {
        if self
            .acknowledged_at
            .is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(12))
        {
            return false;
        }
        self.guest_ack.as_ref().is_some_and(|a| {
            a.state == ContentLoadingState::PlaybackReady
                && self
                    .descriptor
                    .selection
                    .as_ref()
                    .is_some_and(|s| a.validates(s))
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct GuestSelection {
    pub descriptor: Option<ProviderRoomDescriptor>,
    pub state: ContentLoadingState,
    pub acknowledgement_sequence: u64,
    pub observed: Option<ContentEvidence>,
}
impl GuestSelection {
    /// Returns true only for a new revision. Equal retransmits never reopen Chrome.
    pub fn apply(
        &mut self,
        descriptor: ProviderRoomDescriptor,
        room: &str,
    ) -> Result<bool, String> {
        descriptor.validate(room)?;
        if let Some(old) = &self.descriptor {
            if old.state_epoch != descriptor.state_epoch {
                return Err("PROVIDER_EPOCH_CONFLICT".into());
            }
            match (&old.selection, &descriptor.selection) {
                (Some(a), Some(b))
                    if b.host_sequence < a.host_sequence
                        || b.content_revision < a.content_revision =>
                {
                    return Err("PROVIDER_SELECTION_STALE".into())
                }
                (Some(a), Some(b))
                    if b.host_sequence == a.host_sequence
                        || b.content_revision == a.content_revision =>
                {
                    if a != b {
                        return Err("PROVIDER_SELECTION_CONFLICT".into());
                    }
                    return Ok(false);
                }
                (Some(_), None) => return Err("PROVIDER_SELECTION_STALE".into()),
                (None, None) => return Ok(false),
                _ => {}
            }
        }
        self.state = if descriptor.selection.is_some() {
            ContentLoadingState::Opening
        } else {
            ContentLoadingState::Unselected
        };
        self.observed = None;
        self.descriptor = Some(descriptor);
        Ok(true)
    }
    pub fn selection(&self) -> Option<&ProviderSelection> {
        self.descriptor.as_ref()?.selection.as_ref()
    }
    pub fn observe(
        &mut self,
        state: ContentLoadingState,
        evidence: Option<ContentEvidence>,
    ) -> Result<SelectionAck, String> {
        let selection = self
            .selection()
            .ok_or("No selected provider content")?
            .clone();
        let sequence = self
            .acknowledgement_sequence
            .checked_add(1)
            .ok_or("Acknowledgement sequence exhausted")?;
        let ack = SelectionAck::for_selection(&selection, sequence, state, evidence.clone());
        if !ack.validates(&selection) {
            return Err("The active player does not match the host's content. Finish sign-in or select that title in Chrome.".into());
        }
        self.state = state;
        self.observed = evidence;
        self.acknowledgement_sequence = sequence;
        Ok(ack)
    }
    pub fn current_ack(&self) -> Option<SelectionAck> {
        Some(SelectionAck::for_selection(
            self.selection()?,
            self.acknowledgement_sequence,
            self.state,
            self.observed.clone(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn evidence(id: &str) -> ContentEvidence {
        ContentEvidence {
            title: format!("Public title {id}"),
            duration_ms: 120_000,
            player_content_id: Some(id.into()),
        }
    }
    fn publish(authority: &mut ProviderRoomAuthority, id: &str) -> ProviderSelection {
        authority
            .publish(
                "room",
                &parse_content("youtube", &format!("https://www.youtube.com/watch?v={id}"))
                    .unwrap(),
                evidence(id),
            )
            .unwrap()
    }
    #[test]
    fn two_peers_confirm_exact_host_content_then_reach_playback_ready() {
        let mut host = ProviderRoomAuthority::default();
        let x = publish(&mut host, "abcdefghijk");
        let mut guest = GuestSelection::default();
        assert!(guest.apply(host.descriptor.clone(), "room").unwrap());
        assert_eq!(guest.state, ContentLoadingState::Opening);
        assert!(!host.guest_ready());
        let confirmed = guest
            .observe(
                ContentLoadingState::ContentConfirmed,
                Some(x.evidence.clone()),
            )
            .unwrap();
        host.acknowledge(confirmed).unwrap();
        assert!(!host.guest_ready());
        host.acknowledge(
            guest
                .observe(ContentLoadingState::PlaybackReady, Some(x.evidence))
                .unwrap(),
        )
        .unwrap();
        assert!(host.guest_ready());
    }
    #[test]
    fn stale_y_duplicate_x_conflict_and_wrong_epoch_cannot_overwrite_new_x() {
        let mut host = ProviderRoomAuthority::default();
        let y = publish(&mut host, "dQw4w9WgXcQ");
        let x = publish(&mut host, "abcdefghijk");
        let mut guest = GuestSelection::default();
        guest.apply(host.descriptor.clone(), "room").unwrap();
        assert!(!guest.apply(host.descriptor.clone(), "room").unwrap());
        let mut stale = host.descriptor.clone();
        stale.selection = Some(y);
        assert!(guest.apply(stale, "room").is_err());
        let mut conflict = host.descriptor.clone();
        conflict.selection.as_mut().unwrap().title = "Forged".into();
        assert!(guest.apply(conflict, "room").is_err());
        let mut epoch = host.descriptor.clone();
        epoch.state_epoch = uuid::Uuid::now_v7().to_string();
        epoch.selection.as_mut().unwrap().state_epoch = epoch.state_epoch.clone();
        assert!(guest.apply(epoch, "room").is_err());
        assert_eq!(guest.selection(), Some(&x));
    }
    #[test]
    fn same_url_with_wrong_actual_player_identity_title_or_duration_never_acknowledges_ready() {
        let mut host = ProviderRoomAuthority::default();
        let x = publish(&mut host, "abcdefghijk");
        let mut guest = GuestSelection::default();
        guest.apply(host.descriptor.clone(), "room").unwrap();
        for kind in 0..4 {
            let mut e = x.evidence.clone();
            match kind {
                0 => e.player_content_id = Some("dQw4w9WgXcQ".into()),
                1 => e.title = "An advertisement".into(),
                2 => e.duration_ms = 30_000,
                _ => e.player_content_id = None,
            }
            assert!(guest
                .observe(ContentLoadingState::PlaybackReady, Some(e))
                .is_err());
        }
        assert!(guest
            .observe(ContentLoadingState::PlaybackReady, None)
            .is_err());
        assert!(!host.guest_ready());
    }
    #[test]
    fn lost_ack_resends_idempotently_and_old_ack_cannot_regress_state_or_cross_revisions() {
        let mut host = ProviderRoomAuthority::default();
        let x = publish(&mut host, "abcdefghijk");
        let opening = SelectionAck::for_selection(&x, 1, ContentLoadingState::Opening, None);
        let ready = SelectionAck::for_selection(
            &x,
            2,
            ContentLoadingState::PlaybackReady,
            Some(x.evidence.clone()),
        );
        host.acknowledge(ready.clone()).unwrap();
        host.acknowledge(ready.clone()).unwrap();
        assert!(host.guest_ready());
        assert!(host.acknowledge(opening).is_err());
        host.acknowledged_at = Some(std::time::Instant::now() - std::time::Duration::from_secs(13));
        assert!(!host.guest_ready());
        host.acknowledge(ready.clone()).unwrap();
        assert!(host.guest_ready());
        publish(&mut host, "dQw4w9WgXcQ");
        assert!(!host.guest_ready());
        assert!(host.acknowledge(ready).is_err());
    }
    #[test]
    fn version_room_canonical_url_and_metadata_are_validated_without_partial_mutation() {
        let mut host = ProviderRoomAuthority::default();
        publish(&mut host, "abcdefghijk");
        for kind in 0..7 {
            let mut descriptor = host.descriptor.clone();
            let selection = descriptor.selection.as_mut().unwrap();
            match kind {
                0 => descriptor.version = 2,
                1 => selection.version = 2,
                2 => selection.room_id = "other".into(),
                3 => selection.canonical_url = "javascript:alert(1)".into(),
                4 => selection.content_id = "dQw4w9WgXcQ".into(),
                5 => selection.host_sequence = 0,
                _ => selection.evidence.duration_ms = 0,
            }
            let mut guest = GuestSelection::default();
            assert!(guest.apply(descriptor, "room").is_err());
            assert!(guest.selection().is_none());
        }
        let mut wire = serde_json::to_value(&host.descriptor.selection).unwrap();
        wire["cookies"] = "never allowed".into();
        assert!(serde_json::from_value::<ProviderSelection>(wire).is_err());
    }
    #[test]
    fn subscription_evidence_is_observed_metadata_not_url_equality_and_auth_can_recover() {
        let identity = parse_content("netflix", "https://www.netflix.com/watch/81234567").unwrap();
        let evidence = ContentEvidence {
            title: "Public selected title".into(),
            duration_ms: 5_400_000,
            player_content_id: None,
        };
        let mut host = ProviderRoomAuthority::default();
        host.publish("room", &identity, evidence.clone()).unwrap();
        let mut guest = GuestSelection::default();
        guest.apply(host.descriptor.clone(), "room").unwrap();
        guest
            .observe(ContentLoadingState::AuthRequired, None)
            .unwrap();
        assert_eq!(guest.state, ContentLoadingState::AuthRequired);
        let mut wrong = evidence.clone();
        wrong.duration_ms = 60_000;
        assert!(guest
            .observe(ContentLoadingState::PlaybackReady, Some(wrong))
            .is_err());
        guest
            .observe(
                ContentLoadingState::ContentConfirmed,
                Some(evidence.clone()),
            )
            .unwrap();
        host.acknowledge(
            guest
                .observe(ContentLoadingState::PlaybackReady, Some(evidence))
                .unwrap(),
        )
        .unwrap();
        assert!(host.guest_ready());
    }
    #[test]
    fn legacy_peer_may_use_local_perfect_but_cannot_silently_change_to_provider_sync() {
        let mut host = ProviderRoomAuthority {
            peer_connected: true,
            ..ProviderRoomAuthority::default()
        };
        assert!(host
            .publish(
                "room",
                &parse_content("youtube", "https://www.youtube.com/watch?v=abcdefghijk").unwrap(),
                evidence("abcdefghijk")
            )
            .is_err());
        assert!(host.descriptor.selection.is_none());
        host.peer_version = Some(SELECTION_VERSION);
        publish(&mut host, "abcdefghijk");
    }
}
