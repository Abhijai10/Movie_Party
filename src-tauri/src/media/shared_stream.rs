use std::collections::BTreeMap;

use thiserror::Error;

pub const SHARED_STREAM_MAGIC: &[u8; 4] = b"MPSF";
pub const SHARED_MODE_PRESENTATION_LATENCY_US: u64 = 5_000_000;
pub const DEFAULT_PRESENTATION_BUFFER_US: u64 = SHARED_MODE_PRESENTATION_LATENCY_US;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StreamKind {
    Video = 1,
    Audio = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedMediaPacket {
    pub sequence: u64,
    pub kind: StreamKind,
    pub is_keyframe: bool,
    pub pts_us: u64,
    pub duration_us: u32,
    pub payload: Vec<u8>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SharedStreamError {
    #[error("MP-STREAM-001 packet too short")]
    PacketTooShort,
    #[error("MP-STREAM-002 invalid packet magic")]
    InvalidMagic,
    #[error("MP-STREAM-003 unknown stream kind")]
    UnknownStreamKind,
    #[error("MP-STREAM-004 payload length mismatch")]
    PayloadLengthMismatch,
    #[error("MP-STREAM-005 duplicate or stale packet")]
    DuplicateOrStale,
}

pub fn encode_packet(packet: &EncodedMediaPacket) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(34 + packet.payload.len());
    bytes.extend_from_slice(SHARED_STREAM_MAGIC);
    bytes.push(packet.kind as u8);
    bytes.push(u8::from(packet.is_keyframe));
    bytes.extend_from_slice(&packet.sequence.to_be_bytes());
    bytes.extend_from_slice(&packet.pts_us.to_be_bytes());
    bytes.extend_from_slice(&packet.duration_us.to_be_bytes());
    bytes.extend_from_slice(&(packet.payload.len() as u64).to_be_bytes());
    bytes.extend_from_slice(&packet.payload);
    bytes
}

pub fn decode_packet(bytes: &[u8]) -> Result<EncodedMediaPacket, SharedStreamError> {
    if bytes.len() < 34 {
        return Err(SharedStreamError::PacketTooShort);
    }

    if &bytes[0..4] != SHARED_STREAM_MAGIC {
        return Err(SharedStreamError::InvalidMagic);
    }

    let kind = match bytes[4] {
        1 => StreamKind::Video,
        2 => StreamKind::Audio,
        _ => return Err(SharedStreamError::UnknownStreamKind),
    };
    let is_keyframe = bytes[5] != 0;
    let sequence = u64::from_be_bytes(
        bytes[6..14]
            .try_into()
            .map_err(|_| SharedStreamError::PacketTooShort)?,
    );
    let pts_us = u64::from_be_bytes(
        bytes[14..22]
            .try_into()
            .map_err(|_| SharedStreamError::PacketTooShort)?,
    );
    let duration_us = u32::from_be_bytes(
        bytes[22..26]
            .try_into()
            .map_err(|_| SharedStreamError::PacketTooShort)?,
    );
    let payload_len = u64::from_be_bytes(
        bytes[26..34]
            .try_into()
            .map_err(|_| SharedStreamError::PacketTooShort)?,
    ) as usize;
    let payload = bytes[34..].to_vec();

    if payload.len() != payload_len {
        return Err(SharedStreamError::PayloadLengthMismatch);
    }

    Ok(EncodedMediaPacket {
        sequence,
        kind,
        is_keyframe,
        pts_us,
        duration_us,
        payload,
    })
}

#[derive(Debug, Default)]
pub struct PresentationBuffer {
    packets_by_sequence: BTreeMap<u64, EncodedMediaPacket>,
    last_released_sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedModeTimeline {
    pub source_position_us: u64,
    pub encoded_position_us: u64,
    pub presentation_position_us: u64,
}

impl SharedModeTimeline {
    pub fn from_source(source_position_us: u64, encode_latency_us: u64) -> Self {
        let encoded_position_us = source_position_us.saturating_sub(encode_latency_us);
        let presentation_position_us =
            encoded_position_us.saturating_sub(SHARED_MODE_PRESENTATION_LATENCY_US);

        Self {
            source_position_us,
            encoded_position_us,
            presentation_position_us,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostLoopbackPlan {
    pub host_consumes_encoded_stream: bool,
    pub guest_consumes_encoded_stream: bool,
    pub chrome_source_visible_to_user: bool,
    pub presentation_latency_us: u64,
}

pub fn host_loopback_plan() -> HostLoopbackPlan {
    HostLoopbackPlan {
        host_consumes_encoded_stream: true,
        guest_consumes_encoded_stream: true,
        chrome_source_visible_to_user: false,
        presentation_latency_us: SHARED_MODE_PRESENTATION_LATENCY_US,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedStrictSyncInput {
    pub guest_connected: bool,
    pub guest_buffer_us: u64,
    pub host_decoder_ready: bool,
    pub guest_decoder_ready: bool,
    pub source_playing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedStrictSyncAction {
    Continue,
    PauseSourceHostAndGuest,
    RebuildGuestBuffer,
    ResumeTogether,
}

pub fn evaluate_shared_strict_sync(input: SharedStrictSyncInput) -> SharedStrictSyncAction {
    if !input.guest_connected || !input.host_decoder_ready || !input.guest_decoder_ready {
        return SharedStrictSyncAction::PauseSourceHostAndGuest;
    }

    if input.guest_buffer_us < SHARED_MODE_PRESENTATION_LATENCY_US / 2 {
        return SharedStrictSyncAction::PauseSourceHostAndGuest;
    }

    if !input.source_playing && input.guest_buffer_us < SHARED_MODE_PRESENTATION_LATENCY_US {
        return SharedStrictSyncAction::RebuildGuestBuffer;
    }

    if !input.source_playing && input.guest_buffer_us >= SHARED_MODE_PRESENTATION_LATENCY_US {
        return SharedStrictSyncAction::ResumeTogether;
    }

    SharedStrictSyncAction::Continue
}

impl PresentationBuffer {
    pub fn push(&mut self, packet: EncodedMediaPacket) -> Result<(), SharedStreamError> {
        if packet.sequence <= self.last_released_sequence
            || self.packets_by_sequence.contains_key(&packet.sequence)
        {
            return Err(SharedStreamError::DuplicateOrStale);
        }

        self.packets_by_sequence.insert(packet.sequence, packet);
        Ok(())
    }

    pub fn buffered_duration_us(&self) -> u64 {
        self.packets_by_sequence
            .values()
            .map(|packet| u64::from(packet.duration_us))
            .sum()
    }

    pub fn ready_for_presentation(&self, target_buffer_us: u64) -> bool {
        self.buffered_duration_us() >= target_buffer_us
    }

    pub fn pop_ready(&mut self, playback_pts_us: u64) -> Option<EncodedMediaPacket> {
        let sequence = self
            .packets_by_sequence
            .iter()
            .find(|(_, packet)| packet.pts_us <= playback_pts_us)
            .map(|(sequence, _)| *sequence)?;
        let packet = self.packets_by_sequence.remove(&sequence)?;
        self.last_released_sequence = sequence;
        Some(packet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(
        sequence: u64,
        kind: StreamKind,
        pts_us: u64,
        duration_us: u32,
    ) -> EncodedMediaPacket {
        EncodedMediaPacket {
            sequence,
            kind,
            is_keyframe: sequence == 1,
            pts_us,
            duration_us,
            payload: vec![1, 2, 3, sequence as u8],
        }
    }

    #[test]
    fn packet_round_trips_for_video_and_audio() {
        for kind in [StreamKind::Video, StreamKind::Audio] {
            let original = packet(1, kind, 40_000, 33_333);
            let encoded = encode_packet(&original);
            let decoded = decode_packet(&encoded);

            assert_eq!(decoded, Ok(original));
        }
    }

    #[test]
    fn rejects_malformed_packets() {
        assert_eq!(decode_packet(&[]), Err(SharedStreamError::PacketTooShort));

        let mut encoded = encode_packet(&packet(1, StreamKind::Video, 0, 33_333));
        encoded[0] = b'X';
        assert_eq!(
            decode_packet(&encoded),
            Err(SharedStreamError::InvalidMagic)
        );

        let mut encoded = encode_packet(&packet(1, StreamKind::Video, 0, 33_333));
        encoded[4] = 9;
        assert_eq!(
            decode_packet(&encoded),
            Err(SharedStreamError::UnknownStreamKind)
        );
    }

    #[test]
    fn presentation_buffer_waits_for_seconds_of_media() {
        let mut buffer = PresentationBuffer::default();

        for sequence in 1..=4 {
            buffer
                .push(packet(
                    sequence,
                    StreamKind::Video,
                    sequence * 1_000_000,
                    1_000_000,
                ))
                .expect("packet");
        }

        assert!(!buffer.ready_for_presentation(DEFAULT_PRESENTATION_BUFFER_US));

        buffer
            .push(packet(5, StreamKind::Audio, 5_000_000, 1_000_000))
            .expect("packet");
        assert!(buffer.ready_for_presentation(DEFAULT_PRESENTATION_BUFFER_US));
    }

    #[test]
    fn presentation_buffer_releases_by_pts_and_rejects_stale_packets() {
        let mut buffer = PresentationBuffer::default();

        buffer
            .push(packet(1, StreamKind::Video, 90_000, 33_333))
            .expect("packet");
        buffer
            .push(packet(2, StreamKind::Audio, 20_000, 20_000))
            .expect("packet");

        assert_eq!(
            buffer.pop_ready(30_000).map(|packet| packet.sequence),
            Some(2)
        );
        assert_eq!(
            buffer.push(packet(2, StreamKind::Audio, 20_000, 20_000)),
            Err(SharedStreamError::DuplicateOrStale)
        );
    }

    #[test]
    fn host_loopback_plan_keeps_chrome_source_out_of_user_presentation() {
        let plan = host_loopback_plan();

        assert!(plan.host_consumes_encoded_stream);
        assert!(plan.guest_consumes_encoded_stream);
        assert!(!plan.chrome_source_visible_to_user);
        assert_eq!(plan.presentation_latency_us, 5_000_000);
    }

    #[test]
    fn shared_timeline_uses_delayed_presentation_as_authoritative_position() {
        let timeline = SharedModeTimeline::from_source(730_000_000, 1_000_000);

        assert_eq!(timeline.source_position_us, 730_000_000);
        assert_eq!(timeline.encoded_position_us, 729_000_000);
        assert_eq!(timeline.presentation_position_us, 724_000_000);
    }

    #[test]
    fn shared_strict_sync_pauses_everything_when_guest_buffer_falls() {
        let action = evaluate_shared_strict_sync(SharedStrictSyncInput {
            guest_connected: true,
            guest_buffer_us: 1_000_000,
            host_decoder_ready: true,
            guest_decoder_ready: true,
            source_playing: true,
        });

        assert_eq!(action, SharedStrictSyncAction::PauseSourceHostAndGuest);
    }

    #[test]
    fn shared_strict_sync_rebuilds_then_resumes_together() {
        let rebuilding = evaluate_shared_strict_sync(SharedStrictSyncInput {
            guest_connected: true,
            guest_buffer_us: 3_000_000,
            host_decoder_ready: true,
            guest_decoder_ready: true,
            source_playing: false,
        });
        let ready = evaluate_shared_strict_sync(SharedStrictSyncInput {
            guest_connected: true,
            guest_buffer_us: SHARED_MODE_PRESENTATION_LATENCY_US,
            host_decoder_ready: true,
            guest_decoder_ready: true,
            source_playing: false,
        });

        assert_eq!(rebuilding, SharedStrictSyncAction::RebuildGuestBuffer);
        assert_eq!(ready, SharedStrictSyncAction::ResumeTogether);
    }

    #[test]
    fn shared_strict_sync_pauses_when_any_decoder_or_peer_is_unavailable() {
        for input in [
            SharedStrictSyncInput {
                guest_connected: false,
                guest_buffer_us: SHARED_MODE_PRESENTATION_LATENCY_US,
                host_decoder_ready: true,
                guest_decoder_ready: true,
                source_playing: true,
            },
            SharedStrictSyncInput {
                guest_connected: true,
                guest_buffer_us: SHARED_MODE_PRESENTATION_LATENCY_US,
                host_decoder_ready: false,
                guest_decoder_ready: true,
                source_playing: true,
            },
            SharedStrictSyncInput {
                guest_connected: true,
                guest_buffer_us: SHARED_MODE_PRESENTATION_LATENCY_US,
                host_decoder_ready: true,
                guest_decoder_ready: false,
                source_playing: true,
            },
        ] {
            assert_eq!(
                evaluate_shared_strict_sync(input),
                SharedStrictSyncAction::PauseSourceHostAndGuest
            );
        }
    }
}
