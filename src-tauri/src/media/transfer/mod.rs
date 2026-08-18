use std::{cmp::Ordering, collections::BinaryHeap};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};

use crate::media::{cache::chunk_len, manifest::MediaManifest};

pub const CHUNK_MAGIC: &[u8; 4] = b"MPCK";
pub const CHUNK_STREAM_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkRequest {
    pub index: u64,
    pub priority: ChunkPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ChunkPriority {
    Critical = 0,
    ImmediateFuture = 1,
    TargetBuffer = 2,
    Background = 3,
    Speculative = 4,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkPacket {
    pub media_id: String,
    pub chunk_index: u32,
    pub hash: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferProgress {
    pub media_id: String,
    pub bytes_available: u64,
    pub bytes_total: u64,
    pub buffer_ahead_ms: u64,
    pub goodput_bps: u64,
}

impl TransferProgress {
    pub fn fraction(&self) -> f32 {
        if self.bytes_total == 0 {
            return 1.0;
        }

        self.bytes_available as f32 / self.bytes_total as f32
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("MP-MEDIA-002 chunk index out of range")]
    ChunkOutOfRange,
    #[error("MP-MEDIA-002 chunk length is invalid")]
    InvalidChunkLength,
    #[error("MP-MEDIA-002 chunk hash mismatch")]
    HashMismatch,
    #[error("MP-MEDIA-002 chunk stream is malformed")]
    MalformedChunk,
}

#[derive(Debug, Clone, Default)]
pub struct ChunkScheduler {
    queue: BinaryHeap<QueuedChunk>,
}

impl ChunkScheduler {
    pub fn request(&mut self, request: ChunkRequest) {
        self.queue.push(QueuedChunk(request));
    }

    pub fn next_request(&mut self) -> Option<ChunkRequest> {
        self.queue.pop().map(|queued| queued.0)
    }

    pub fn prioritize_playback_window(
        &mut self,
        manifest: &MediaManifest,
        playback_position_bytes: u64,
        window_bytes: u64,
    ) {
        let start = playback_position_bytes / manifest.chunk_size;
        let end = playback_position_bytes
            .saturating_add(window_bytes)
            .min(manifest.file_size)
            / manifest.chunk_size;

        for index in start..=end.min(manifest.chunk_count.saturating_sub(1)) {
            self.request(ChunkRequest {
                index,
                priority: ChunkPriority::Critical,
            });
        }
    }

    pub fn restore_missing_as_background(&mut self, missing_chunks: impl IntoIterator<Item = u64>) {
        for index in missing_chunks {
            self.request(ChunkRequest {
                index,
                priority: ChunkPriority::Background,
            });
        }
    }
}

pub fn transfer_progress(
    manifest: &MediaManifest,
    bytes_available: u64,
    buffer_ahead_ms: u64,
    goodput_bps: u64,
) -> TransferProgress {
    TransferProgress {
        media_id: manifest.media_id.clone(),
        bytes_available,
        bytes_total: manifest.file_size,
        buffer_ahead_ms,
        goodput_bps,
    }
}

pub fn build_chunk_packet(
    manifest: &MediaManifest,
    index: u64,
    payload: Vec<u8>,
) -> Result<ChunkPacket, TransferError> {
    if index >= manifest.chunk_count {
        return Err(TransferError::ChunkOutOfRange);
    }

    if payload.len() as u64 != chunk_len(manifest, index) {
        return Err(TransferError::InvalidChunkLength);
    }

    Ok(ChunkPacket {
        media_id: manifest.media_id.clone(),
        chunk_index: u32::try_from(index).map_err(|_| TransferError::ChunkOutOfRange)?,
        hash: chunk_hash(&payload),
        payload,
    })
}

pub fn validate_chunk_packet(
    manifest: &MediaManifest,
    packet: &ChunkPacket,
) -> Result<(), TransferError> {
    if packet.media_id != manifest.media_id || u64::from(packet.chunk_index) >= manifest.chunk_count
    {
        return Err(TransferError::ChunkOutOfRange);
    }

    if packet.payload.len() as u64 != chunk_len(manifest, u64::from(packet.chunk_index)) {
        return Err(TransferError::InvalidChunkLength);
    }

    if packet.hash != chunk_hash(&packet.payload) {
        return Err(TransferError::HashMismatch);
    }

    Ok(())
}

pub fn encode_chunk_stream(packet: &ChunkPacket) -> Result<Vec<u8>, TransferError> {
    let media_id = packet.media_id.as_bytes();
    let media_id_len = u8::try_from(media_id.len()).map_err(|_| TransferError::MalformedChunk)?;
    let payload_len =
        u32::try_from(packet.payload.len()).map_err(|_| TransferError::InvalidChunkLength)?;
    let hash = URL_SAFE_NO_PAD
        .decode(&packet.hash)
        .map_err(|_| TransferError::MalformedChunk)?;
    if hash.len() != 32 {
        return Err(TransferError::MalformedChunk);
    }

    let mut bytes =
        Vec::with_capacity(4 + 1 + 1 + media_id.len() + 4 + 4 + 32 + packet.payload.len());
    bytes.extend_from_slice(CHUNK_MAGIC);
    bytes.push(CHUNK_STREAM_VERSION);
    bytes.push(media_id_len);
    bytes.extend_from_slice(media_id);
    bytes.extend_from_slice(&packet.chunk_index.to_be_bytes());
    bytes.extend_from_slice(&payload_len.to_be_bytes());
    bytes.extend_from_slice(&hash);
    bytes.extend_from_slice(&packet.payload);
    Ok(bytes)
}

pub fn decode_chunk_stream(bytes: &[u8]) -> Result<ChunkPacket, TransferError> {
    if bytes.len() < 46 || &bytes[..4] != CHUNK_MAGIC || bytes[4] != CHUNK_STREAM_VERSION {
        return Err(TransferError::MalformedChunk);
    }

    let media_id_len = bytes[5] as usize;
    let media_start = 6;
    let index_start = media_start + media_id_len;
    let payload_len_start = index_start + 4;
    let hash_start = payload_len_start + 4;
    let payload_start = hash_start + 32;
    if bytes.len() < payload_start {
        return Err(TransferError::MalformedChunk);
    }

    let media_id = std::str::from_utf8(&bytes[media_start..index_start])
        .map_err(|_| TransferError::MalformedChunk)?
        .to_string();
    let chunk_index = u32::from_be_bytes(
        bytes[index_start..payload_len_start]
            .try_into()
            .map_err(|_| TransferError::MalformedChunk)?,
    );
    let payload_len = u32::from_be_bytes(
        bytes[payload_len_start..hash_start]
            .try_into()
            .map_err(|_| TransferError::MalformedChunk)?,
    ) as usize;
    let payload = bytes
        .get(payload_start..payload_start + payload_len)
        .ok_or(TransferError::MalformedChunk)?
        .to_vec();

    Ok(ChunkPacket {
        media_id,
        chunk_index,
        hash: URL_SAFE_NO_PAD.encode(&bytes[hash_start..payload_start]),
        payload,
    })
}

fn chunk_hash(payload: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(blake3::hash(payload).as_bytes())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QueuedChunk(ChunkRequest);

impl Ord for QueuedChunk {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .0
            .priority
            .cmp(&self.0.priority)
            .then_with(|| other.0.index.cmp(&self.0.index))
    }
}

impl PartialOrd for QueuedChunk {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_chunk_packet, decode_chunk_stream, encode_chunk_stream, validate_chunk_packet,
        ChunkPriority, ChunkRequest, ChunkScheduler, TransferError,
    };

    use crate::media::manifest::{MediaManifest, QuickFingerprint};

    #[test]
    fn validates_and_round_trips_chunk_stream() {
        let manifest = manifest();
        let packet = build_chunk_packet(&manifest, 0, b"abcd".to_vec()).expect("packet");
        validate_chunk_packet(&manifest, &packet).expect("valid");

        let decoded =
            decode_chunk_stream(&encode_chunk_stream(&packet).expect("encode")).expect("decode");
        assert_eq!(decoded, packet);
    }

    #[test]
    fn rejects_corrupt_chunk_hash() {
        let manifest = manifest();
        let mut packet = build_chunk_packet(&manifest, 0, b"abcd".to_vec()).expect("packet");
        packet.payload[0] = b'x';

        assert!(matches!(
            validate_chunk_packet(&manifest, &packet),
            Err(TransferError::HashMismatch),
        ));
    }

    #[test]
    fn scheduler_prioritizes_playback_chunks() {
        let mut scheduler = ChunkScheduler::default();
        scheduler.request(ChunkRequest {
            index: 4,
            priority: ChunkPriority::Background,
        });
        scheduler.request(ChunkRequest {
            index: 1,
            priority: ChunkPriority::Critical,
        });

        assert_eq!(scheduler.next_request().expect("next").index, 1);
    }

    #[test]
    fn restores_missing_chunks_after_disconnect() {
        let mut scheduler = ChunkScheduler::default();
        scheduler.restore_missing_as_background([2, 3, 4]);

        assert_eq!(scheduler.next_request().expect("next").index, 2);
        assert_eq!(scheduler.next_request().expect("next").index, 3);
        assert_eq!(scheduler.next_request().expect("next").index, 4);
    }

    #[test]
    fn reports_transfer_progress_fraction() {
        let progress = super::transfer_progress(&manifest(), 4, 1_000, 8_000);

        assert_eq!(progress.bytes_total, 8);
        assert_eq!(progress.fraction(), 0.5);
    }

    fn manifest() -> MediaManifest {
        MediaManifest {
            media_id: "local-test".to_string(),
            filename: "movie.bin".to_string(),
            file_size: 8,
            container: None,
            full_hash: "hash".to_string(),
            quick_fingerprint: QuickFingerprint {
                file_size: 8,
                first_hash: "first".to_string(),
                last_hash: "last".to_string(),
            },
            chunk_size: 4,
            chunk_count: 2,
        }
    }
}
