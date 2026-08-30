use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};

pub const DEFAULT_CHUNK_SIZE_BYTES: u64 = 1_048_576;
pub const FINGERPRINT_EDGE_BYTES: u64 = 4 * 1_048_576;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuickFingerprint {
    pub file_size: u64,
    pub first_hash: String,
    pub last_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaManifest {
    pub media_id: String,
    pub filename: String,
    pub file_size: u64,
    pub container: Option<String>,
    pub full_hash: String,
    pub quick_fingerprint: QuickFingerprint,
    pub chunk_size: u64,
    pub chunk_count: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("MP-MEDIA-001 media file could not be read: {0}")]
    Io(#[from] std::io::Error),
    #[error("MP-MEDIA-001 media path has no filename")]
    MissingFilename,
    #[error("MP-MEDIA-002 media manifest is invalid: {0}")]
    InvalidManifest(&'static str),
}

impl MediaManifest {
    /// Validate a manifest received from a peer before it is allowed to name
    /// any local cache resources. The filename remains metadata only, but it
    /// must never be a path supplied by the remote host.
    pub fn validate_for_guest(&self) -> Result<(), ManifestError> {
        if self.media_id.is_empty()
            || self.media_id.contains("..")
            || self.media_id.contains(['/', '\\'])
        {
            return Err(ManifestError::InvalidManifest("unsafe media id"));
        }
        if self.filename.is_empty()
            || self.filename.contains("..")
            || self.filename.contains(['/', '\\'])
        {
            return Err(ManifestError::InvalidManifest("unsafe filename"));
        }
        if self.file_size == 0 || self.chunk_size != DEFAULT_CHUNK_SIZE_BYTES {
            return Err(ManifestError::InvalidManifest(
                "invalid media size or chunk size",
            ));
        }
        if self.chunk_count != self.file_size.div_ceil(self.chunk_size) {
            return Err(ManifestError::InvalidManifest("inconsistent chunk count"));
        }
        if self.quick_fingerprint.file_size != self.file_size {
            return Err(ManifestError::InvalidManifest(
                "inconsistent fingerprint size",
            ));
        }
        Ok(())
    }
}

pub fn build_manifest(path: &Path) -> Result<MediaManifest, ManifestError> {
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(ManifestError::MissingFilename)?
        .to_string();
    let quick_fingerprint = quick_fingerprint(path)?;
    let full_hash = full_hash(path)?;
    let file_size = quick_fingerprint.file_size;
    let chunk_count = file_size.div_ceil(DEFAULT_CHUNK_SIZE_BYTES);
    let media_id = media_id(file_size, &full_hash);

    Ok(MediaManifest {
        media_id,
        filename,
        file_size,
        container: path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase),
        full_hash,
        quick_fingerprint,
        chunk_size: DEFAULT_CHUNK_SIZE_BYTES,
        chunk_count,
    })
}

pub fn quick_fingerprint(path: &Path) -> Result<QuickFingerprint, ManifestError> {
    let mut file = File::open(path)?;
    let file_size = file.metadata()?.len();
    let first_len = file_size.min(FINGERPRINT_EDGE_BYTES) as usize;
    let mut first = vec![0; first_len];
    file.read_exact(&mut first)?;

    let last_len = file_size.min(FINGERPRINT_EDGE_BYTES) as usize;
    let mut last = vec![0; last_len];
    if last_len > 0 {
        file.seek(SeekFrom::Start(file_size.saturating_sub(last_len as u64)))?;
        file.read_exact(&mut last)?;
    }

    Ok(QuickFingerprint {
        file_size,
        first_hash: blake3_base64(&first),
        last_hash: blake3_base64(&last),
    })
}

pub fn full_hash(path: &Path) -> Result<String, ManifestError> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(URL_SAFE_NO_PAD.encode(hasher.finalize().as_bytes()))
}

pub fn definitely_identical(left: &MediaManifest, right: &MediaManifest) -> bool {
    left.file_size == right.file_size && left.full_hash == right.full_hash
}

fn media_id(file_size: u64, full_hash: &str) -> String {
    let hash_prefix = full_hash.get(..16).unwrap_or(full_hash);
    format!("local-{file_size}-{hash_prefix}")
}

fn blake3_base64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(blake3::hash(bytes).as_bytes())
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        io::Write,
    };

    use uuid::Uuid;

    use super::{
        build_manifest, definitely_identical, MediaManifest, QuickFingerprint,
        DEFAULT_CHUNK_SIZE_BYTES,
    };

    #[test]
    fn detects_identical_and_different_files() {
        let dir = std::env::temp_dir().join(Uuid::now_v7().to_string());
        fs::create_dir_all(&dir).expect("temp dir");
        let a = dir.join("movie-a.mkv");
        let b = dir.join("movie-b.mkv");
        let c = dir.join("movie-c.mkv");
        write_file(&a, b"same bytes");
        write_file(&b, b"same bytes");
        write_file(&c, b"different bytes");

        let manifest_a = build_manifest(&a).expect("a");
        let manifest_b = build_manifest(&b).expect("b");
        let manifest_c = build_manifest(&c).expect("c");

        assert!(definitely_identical(&manifest_a, &manifest_b));
        assert!(!definitely_identical(&manifest_a, &manifest_c));
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn rejects_remote_path_like_manifest_metadata() {
        let mut manifest = MediaManifest {
            media_id: "local-12-safe".to_string(),
            filename: "movie.mkv".to_string(),
            file_size: DEFAULT_CHUNK_SIZE_BYTES,
            container: Some("mkv".to_string()),
            full_hash: "hash".to_string(),
            quick_fingerprint: QuickFingerprint {
                file_size: DEFAULT_CHUNK_SIZE_BYTES,
                first_hash: "first".to_string(),
                last_hash: "last".to_string(),
            },
            chunk_size: DEFAULT_CHUNK_SIZE_BYTES,
            chunk_count: 1,
        };
        assert!(manifest.validate_for_guest().is_ok());
        manifest.filename = "../host-file.mkv".to_string();
        assert!(manifest.validate_for_guest().is_err());
    }

    fn write_file(path: &std::path::Path, bytes: &[u8]) {
        let mut file = File::create(path).expect("create");
        file.write_all(bytes).expect("write");
    }
}
