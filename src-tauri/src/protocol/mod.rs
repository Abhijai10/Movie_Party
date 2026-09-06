//! Movie Party wire-protocol foundation (PROTOCOL_SPEC v1).
//!
//! This module is the single source of truth for the numeric message-type
//! registry (§11), the control-message size limit (§5), and envelope
//! validation rules (§10). The QUIC transport in [`crate::network::quic`]
//! encodes these values into the JSON wire framing documented by
//! ADR-0001 and enforces the limits through [`MAX_CONTROL_MESSAGE_BYTES`].

use serde::{Deserialize, Serialize};

/// §5: maximum control message size. Any control message larger than this
/// must be rejected with `MP-PROTO-004 MESSAGE_TOO_LARGE`.
pub const MAX_CONTROL_MESSAGE_BYTES: usize = 256 * 1024;

/// §10: protocol envelope version carried by every application message.
pub const ENVELOPE_V_MAJOR: u16 = 1;
pub const ENVELOPE_V_MINOR: u16 = 0;

/// §10 envelope: every application message carries the protocol version,
/// the room it belongs to, and the monotonic send timestamp. Sequence and
/// sender fields are attached by the transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EnvelopeMetadata {
    pub v_major: u16,
    pub v_minor: u16,
    pub room_id: String,
    pub sent_mono_us: u64,
}

impl Default for EnvelopeMetadata {
    fn default() -> Self {
        Self {
            v_major: ENVELOPE_V_MAJOR,
            v_minor: ENVELOPE_V_MINOR,
            room_id: String::new(),
            sent_mono_us: 0,
        }
    }
}

impl EnvelopeMetadata {
    /// Build the metadata for a message being sent in the given room.
    pub fn for_room(room_id: impl Into<String>, sent_mono_us: u64) -> Self {
        Self {
            v_major: ENVELOPE_V_MAJOR,
            v_minor: ENVELOPE_V_MINOR,
            room_id: room_id.into(),
            sent_mono_us,
        }
    }

    /// Validate a received envelope against the local protocol version
    /// (§6: same major required; newer minor tolerated when fields are
    /// ignorable) and the room credentials. Returns a stable
    /// `MP-PROTO-<n>` rejection code on failure.
    ///
    /// Per §10 the room_id may be empty only during initial HELLO; the
    /// transport applies this check post-authentication.
    pub fn validate(
        &self,
        protocol_major: u16,
        protocol_minor: u16,
        room_id: &str,
    ) -> Result<(), &'static str> {
        if self.v_major != protocol_major {
            return Err("MP-PROTO-001 VERSION_MISMATCH");
        }
        if self.v_minor > protocol_minor {
            return Err("MP-PROTO-002 UNSUPPORTED_MINOR");
        }
        if self.room_id != room_id {
            return Err("MP-PROTO-003 ROOM_MISMATCH");
        }
        Ok(())
    }
}

/// §11 message-type registry. Numeric IDs are globally unique across all
/// subsystems; IDs 200–204 belong exclusively to Scheduling and must never
/// be reassigned (§68: never redefine an existing message ID).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MessageType {
    // Connection / authentication
    Hello = 1,
    AuthRequest = 2,
    AuthAccept = 3,
    AuthReject = 4,
    Heartbeat = 5,
    Goodbye = 6,

    // Clock
    ClockPing = 20,
    ClockPong = 21,
    ClockResult = 22,

    // Room
    JoinRequest = 40,
    JoinAccept = 41,
    JoinReject = 42,
    RoomState = 43,
    LeaveRoom = 44,
    DisconnectNotice = 45,
    ReconnectRequest = 46,
    ReconnectComplete = 47,

    // Media
    MediaAnnounce = 60,
    MediaReady = 61,
    MediaNotReady = 62,
    PlayerState = 63,
    BufferStatus = 64,
    BufferLow = 65,
    BufferRecovered = 66,

    // Playback
    PlayPrepare = 80,
    PlayReady = 81,
    PlayCommit = 82,
    PausePrepare = 83,
    PauseReady = 84,
    PauseCommit = 85,
    SeekPrepare = 86,
    SeekReady = 87,
    SeekCommit = 88,

    // Shared controls
    ControlRequest = 100,
    ControlGrant = 101,
    ControlRevoke = 102,
    ControlDeny = 103,

    // File transfer
    TransferManifest = 120,
    ChunkRequest = 121,
    ChunkAck = 122,
    TransferProgress = 123,
    TransferComplete = 124,
    TransferError = 125,

    // Network
    NetworkStats = 140,
    PathState = 141,

    // Call
    CallState = 160,
    CameraState = 161,
    MicState = 162,
    CallSignal = 163,

    // Social
    ChatMessage = 180,
    Reaction = 181,

    // Scheduling
    ScheduleCreate = 200,
    ScheduleAccept = 201,
    ScheduleUpdate = 202,
    ScheduleCancel = 203,
    PreloadState = 204,

    // Provider
    ProviderState = 220,
    ProviderError = 221,
    ProviderModeRequest = 222,
    ProviderModeResult = 223,

    // Errors
    Error = 250,
}

impl MessageType {
    /// Parse a raw wire `type` value into a registered message type.
    /// Unknown values return `None` so callers can reject with
    /// `MP-PROTO-005 UNKNOWN_TYPE` (§67: malformed peer input must not
    /// crash the application; unknown types are ignored/rejected).
    pub fn from_u16(raw: u16) -> Option<Self> {
        match raw {
            1 => Some(Self::Hello),
            2 => Some(Self::AuthRequest),
            3 => Some(Self::AuthAccept),
            4 => Some(Self::AuthReject),
            5 => Some(Self::Heartbeat),
            6 => Some(Self::Goodbye),
            20 => Some(Self::ClockPing),
            21 => Some(Self::ClockPong),
            22 => Some(Self::ClockResult),
            40 => Some(Self::JoinRequest),
            41 => Some(Self::JoinAccept),
            42 => Some(Self::JoinReject),
            43 => Some(Self::RoomState),
            44 => Some(Self::LeaveRoom),
            45 => Some(Self::DisconnectNotice),
            46 => Some(Self::ReconnectRequest),
            47 => Some(Self::ReconnectComplete),
            60 => Some(Self::MediaAnnounce),
            61 => Some(Self::MediaReady),
            62 => Some(Self::MediaNotReady),
            63 => Some(Self::PlayerState),
            64 => Some(Self::BufferStatus),
            65 => Some(Self::BufferLow),
            66 => Some(Self::BufferRecovered),
            80 => Some(Self::PlayPrepare),
            81 => Some(Self::PlayReady),
            82 => Some(Self::PlayCommit),
            83 => Some(Self::PausePrepare),
            84 => Some(Self::PauseReady),
            85 => Some(Self::PauseCommit),
            86 => Some(Self::SeekPrepare),
            87 => Some(Self::SeekReady),
            88 => Some(Self::SeekCommit),
            100 => Some(Self::ControlRequest),
            101 => Some(Self::ControlGrant),
            102 => Some(Self::ControlRevoke),
            103 => Some(Self::ControlDeny),
            120 => Some(Self::TransferManifest),
            121 => Some(Self::ChunkRequest),
            122 => Some(Self::ChunkAck),
            123 => Some(Self::TransferProgress),
            124 => Some(Self::TransferComplete),
            125 => Some(Self::TransferError),
            140 => Some(Self::NetworkStats),
            141 => Some(Self::PathState),
            160 => Some(Self::CallState),
            161 => Some(Self::CameraState),
            162 => Some(Self::MicState),
            163 => Some(Self::CallSignal),
            180 => Some(Self::ChatMessage),
            181 => Some(Self::Reaction),
            200 => Some(Self::ScheduleCreate),
            201 => Some(Self::ScheduleAccept),
            202 => Some(Self::ScheduleUpdate),
            203 => Some(Self::ScheduleCancel),
            204 => Some(Self::PreloadState),
            220 => Some(Self::ProviderState),
            221 => Some(Self::ProviderError),
            222 => Some(Self::ProviderModeRequest),
            223 => Some(Self::ProviderModeResult),
            250 => Some(Self::Error),
            _ => None,
        }
    }
}

/// §64: per-sender monotonic sequence enforcement. Zero is never valid
/// (sequence starts at 1); duplicates and stale values are rejected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SequenceTracker {
    last_seq_received: u64,
}

impl SequenceTracker {
    pub fn accept(&mut self, seq: u64) -> bool {
        if seq == 0 || seq <= self.last_seq_received {
            return false;
        }

        self.last_seq_received = seq;
        true
    }

    pub fn last_seq_received(&self) -> u64 {
        self.last_seq_received
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EnvelopeMetadata, MessageType, SequenceTracker, ENVELOPE_V_MINOR, MAX_CONTROL_MESSAGE_BYTES,
    };

    fn all_registered() -> Vec<MessageType> {
        vec![
            MessageType::Hello,
            MessageType::AuthRequest,
            MessageType::AuthAccept,
            MessageType::AuthReject,
            MessageType::Heartbeat,
            MessageType::Goodbye,
            MessageType::ClockPing,
            MessageType::ClockPong,
            MessageType::ClockResult,
            MessageType::JoinRequest,
            MessageType::JoinAccept,
            MessageType::JoinReject,
            MessageType::RoomState,
            MessageType::LeaveRoom,
            MessageType::DisconnectNotice,
            MessageType::ReconnectRequest,
            MessageType::ReconnectComplete,
            MessageType::MediaAnnounce,
            MessageType::MediaReady,
            MessageType::MediaNotReady,
            MessageType::PlayerState,
            MessageType::BufferStatus,
            MessageType::BufferLow,
            MessageType::BufferRecovered,
            MessageType::PlayPrepare,
            MessageType::PlayReady,
            MessageType::PlayCommit,
            MessageType::PausePrepare,
            MessageType::PauseReady,
            MessageType::PauseCommit,
            MessageType::SeekPrepare,
            MessageType::SeekReady,
            MessageType::SeekCommit,
            MessageType::ControlRequest,
            MessageType::ControlGrant,
            MessageType::ControlRevoke,
            MessageType::ControlDeny,
            MessageType::TransferManifest,
            MessageType::ChunkRequest,
            MessageType::ChunkAck,
            MessageType::TransferProgress,
            MessageType::TransferComplete,
            MessageType::TransferError,
            MessageType::NetworkStats,
            MessageType::PathState,
            MessageType::CallState,
            MessageType::CameraState,
            MessageType::MicState,
            MessageType::CallSignal,
            MessageType::ChatMessage,
            MessageType::Reaction,
            MessageType::ScheduleCreate,
            MessageType::ScheduleAccept,
            MessageType::ScheduleUpdate,
            MessageType::ScheduleCancel,
            MessageType::PreloadState,
            MessageType::ProviderState,
            MessageType::ProviderError,
            MessageType::ProviderModeRequest,
            MessageType::ProviderModeResult,
            MessageType::Error,
        ]
    }

    #[test]
    fn registry_matches_locked_spec_ids() {
        assert_eq!(MAX_CONTROL_MESSAGE_BYTES, 262_144);
        assert_eq!(MessageType::Hello as u16, 1);
        assert_eq!(MessageType::Goodbye as u16, 6);
        assert_eq!(MessageType::ClockPing as u16, 20);
        assert_eq!(MessageType::JoinRequest as u16, 40);
        assert_eq!(MessageType::MediaAnnounce as u16, 60);
        assert_eq!(MessageType::BufferStatus as u16, 64);
        assert_eq!(MessageType::PlayCommit as u16, 82);
        assert_eq!(MessageType::ControlRequest as u16, 100);
        assert_eq!(MessageType::ChunkRequest as u16, 121);
        assert_eq!(MessageType::NetworkStats as u16, 140);
        assert_eq!(MessageType::CallState as u16, 160);
        assert_eq!(MessageType::CameraState as u16, 161);
        assert_eq!(MessageType::MicState as u16, 162);
        assert_eq!(MessageType::CallSignal as u16, 163);
        assert_eq!(MessageType::ChatMessage as u16, 180);
        assert_eq!(MessageType::Reaction as u16, 181);
        assert_eq!(MessageType::ScheduleCreate as u16, 200);
        assert_eq!(MessageType::ScheduleAccept as u16, 201);
        assert_eq!(MessageType::ScheduleUpdate as u16, 202);
        assert_eq!(MessageType::ScheduleCancel as u16, 203);
        assert_eq!(MessageType::PreloadState as u16, 204);
        assert_eq!(MessageType::ProviderState as u16, 220);
        assert_eq!(MessageType::Error as u16, 250);
    }

    #[test]
    fn registry_has_no_colliding_ids() {
        let all = all_registered();
        let mut seen = std::collections::HashSet::new();
        for variant in &all {
            assert!(
                seen.insert(*variant as u16),
                "duplicate id {}",
                *variant as u16
            );
        }
        assert_eq!(all.len(), 61);
    }

    #[test]
    fn from_u16_round_trips_every_registered_id() {
        for variant in all_registered() {
            let raw = variant as u16;
            assert_eq!(MessageType::from_u16(raw), Some(variant));
        }
    }

    #[test]
    fn from_u16_rejects_unknown_types() {
        // Values that collide with nothing and gaps in every range.
        for raw in [
            0u16,
            7,
            19,
            23,
            39,
            48,
            59,
            67,
            79,
            89,
            99,
            104,
            119,
            126,
            139,
            142,
            159,
            164,
            179,
            182,
            199,
            205,
            219,
            224,
            249,
            251,
            u16::MAX,
        ] {
            assert_eq!(
                MessageType::from_u16(raw),
                None,
                "raw {} must be unknown",
                raw
            );
        }
    }

    #[test]
    fn scheduling_range_is_exclusive_and_matches_spec() {
        // §11 Scheduling: 200–204. The old colliding assignments
        // (ReadyState=200, BufferStatus=201, ControlRequest=202) are gone;
        // the scheduling IDs own this range exclusively.
        assert_eq!(MessageType::ScheduleCreate as u16, 200);
        assert_eq!(MessageType::PreloadState as u16, 204);
        for variant in all_registered() {
            let raw = variant as u16;
            if (200..=204).contains(&raw) {
                assert!(
                    matches!(
                        variant,
                        MessageType::ScheduleCreate
                            | MessageType::ScheduleAccept
                            | MessageType::ScheduleUpdate
                            | MessageType::ScheduleCancel
                            | MessageType::PreloadState
                    ),
                    "id {raw} must belong to scheduling"
                );
            }
        }
    }

    #[test]
    fn envelope_metadata_defaults_and_validates() {
        let meta = EnvelopeMetadata::for_room("EjRWeJCrze8BI0VniavN7w", 12_345);
        assert_eq!(meta.v_major, 1);
        assert_eq!(meta.v_minor, 0);
        assert_eq!(meta.room_id, "EjRWeJCrze8BI0VniavN7w");
        assert_eq!(meta.sent_mono_us, 12_345);
        assert!(meta.validate(1, 0, "EjRWeJCrze8BI0VniavN7w").is_ok());

        // Same major, older/equal minor: tolerated.
        let older = EnvelopeMetadata {
            v_minor: 0,
            ..EnvelopeMetadata::for_room("EjRWeJCrze8BI0VniavN7w", 0)
        };
        assert!(older.validate(1, 0, "EjRWeJCrze8BI0VniavN7w").is_ok());
    }

    #[test]
    fn envelope_metadata_rejects_version_and_room_mismatch() {
        let meta = EnvelopeMetadata::for_room("room-a", 0);

        assert_eq!(
            meta.validate(2, 0, "room-a"),
            Err("MP-PROTO-001 VERSION_MISMATCH")
        );
        // A peer advertising a NEWER minor than the local protocol cannot
        // be guaranteed ignorable (§6): reject.
        let newer_minor = EnvelopeMetadata {
            v_minor: ENVELOPE_V_MINOR + 1,
            ..meta.clone()
        };
        assert_eq!(
            newer_minor.validate(1, 0, "room-a"),
            Err("MP-PROTO-002 UNSUPPORTED_MINOR")
        );
        assert_eq!(
            meta.validate(1, 0, "room-b"),
            Err("MP-PROTO-003 ROOM_MISMATCH")
        );
    }

    #[test]
    fn sequence_tracker_rejects_zero_duplicate_and_stale_sequences() {
        let mut tracker = SequenceTracker::default();

        assert!(!tracker.accept(0));
        assert!(tracker.accept(1));
        assert!(!tracker.accept(1));
        assert!(!tracker.accept(0));
        assert!(tracker.accept(2));
        assert_eq!(tracker.last_seq_received(), 2);
    }
}
