use std::collections::BTreeMap;

use thiserror::Error;

pub const SHARED_STREAM_MAGIC: &[u8; 4] = b"MPSF";
pub const DEFAULT_PRESENTATION_BUFFER_US: u64 = 2_000_000;

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

        buffer
            .push(packet(1, StreamKind::Video, 0, 1_000_000))
            .expect("packet");
        assert!(!buffer.ready_for_presentation(DEFAULT_PRESENTATION_BUFFER_US));

        buffer
            .push(packet(2, StreamKind::Audio, 0, 1_000_000))
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
}
