use crate::media::{
    cache::SparseCache,
    manifest::MediaManifest,
    transfer::{ChunkPriority, ChunkRequest},
};

pub const LOOPBACK_BIND_ADDRESS: &str = "127.0.0.1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeRequest {
    pub start: u64,
    pub end_inclusive: u64,
}

impl RangeRequest {
    pub fn len(&self) -> u64 {
        self.end_inclusive.saturating_sub(self.start) + 1
    }

    pub fn is_empty(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalMediaRoute {
    pub media_id: String,
    pub session_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeResponse {
    Available {
        status: u16,
        content_range: String,
        bytes: Vec<u8>,
    },
    Pending {
        missing_chunks: Vec<ChunkRequest>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("MP-MEDIA-002 invalid byte range")]
    InvalidRange,
    #[error("MP-MEDIA-002 invalid local media token")]
    InvalidToken,
    #[error("MP-MEDIA-002 cache error: {0}")]
    Cache(#[from] crate::media::cache::CacheError),
}

pub fn parse_http_range(value: &str, file_size: u64) -> Result<RangeRequest, StreamError> {
    let range = value
        .strip_prefix("bytes=")
        .ok_or(StreamError::InvalidRange)?;
    let (start, end) = range.split_once('-').ok_or(StreamError::InvalidRange)?;
    let start = start
        .parse::<u64>()
        .map_err(|_| StreamError::InvalidRange)?;
    let end = if end.is_empty() {
        file_size.saturating_sub(1)
    } else {
        end.parse::<u64>().map_err(|_| StreamError::InvalidRange)?
    };

    if start > end || end >= file_size {
        return Err(StreamError::InvalidRange);
    }

    Ok(RangeRequest {
        start,
        end_inclusive: end,
    })
}

pub fn route_for_media(media_id: impl Into<String>, token: impl Into<String>) -> LocalMediaRoute {
    LocalMediaRoute {
        media_id: media_id.into(),
        session_token: token.into(),
    }
}

pub fn prioritize_range(manifest: &MediaManifest, range: RangeRequest) -> Vec<ChunkRequest> {
    let first = range.start / manifest.chunk_size;
    let last = range.end_inclusive / manifest.chunk_size;

    (first..=last.min(manifest.chunk_count.saturating_sub(1)))
        .map(|index| ChunkRequest {
            index,
            priority: ChunkPriority::Critical,
        })
        .collect()
}

pub fn respond_from_cache(
    cache: &SparseCache,
    manifest: &MediaManifest,
    route: &LocalMediaRoute,
    supplied_token: &str,
    range: RangeRequest,
) -> Result<RangeResponse, StreamError> {
    if supplied_token != route.session_token || route.media_id != manifest.media_id {
        return Err(StreamError::InvalidToken);
    }

    match cache.read_range(range.start, range.len())? {
        Some(bytes) => Ok(RangeResponse::Available {
            status: 206,
            content_range: format!(
                "bytes {}-{}/{}",
                range.start, range.end_inclusive, manifest.file_size
            ),
            bytes,
        }),
        None => Ok(RangeResponse::Pending {
            missing_chunks: prioritize_range(manifest, range),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use crate::media::{
        cache::SparseCache,
        manifest::{MediaManifest, QuickFingerprint},
    };

    use super::{parse_http_range, respond_from_cache, route_for_media, RangeResponse};

    #[test]
    fn parses_standard_http_byte_range() {
        let range = parse_http_range("bytes=4-7", 12).expect("range");

        assert_eq!(range.start, 4);
        assert_eq!(range.end_inclusive, 7);
        assert_eq!(range.len(), 4);
    }

    #[test]
    fn returns_available_range_or_prioritizes_missing_chunks() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        fs::create_dir_all(&root).expect("temp");
        let manifest = manifest();
        let route = route_for_media(&manifest.media_id, "token");
        let mut cache = SparseCache::open(&root, manifest.clone()).expect("cache");

        let pending = respond_from_cache(
            &cache,
            &manifest,
            &route,
            "token",
            parse_http_range("bytes=0-3", manifest.file_size).expect("range"),
        )
        .expect("pending");
        assert!(matches!(pending, RangeResponse::Pending { .. }));

        cache.write_chunk(0, b"abcd").expect("write");
        let available = respond_from_cache(
            &cache,
            &manifest,
            &route,
            "token",
            parse_http_range("bytes=0-3", manifest.file_size).expect("range"),
        )
        .expect("available");
        assert!(matches!(
            available,
            RangeResponse::Available { status: 206, .. }
        ));
        fs::remove_dir_all(&root).expect("cleanup");
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
