use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use crate::media::manifest::MediaManifest;

pub const CACHE_METADATA_FILE: &str = "metadata.cbor";
pub const CACHE_DATA_FILE: &str = "data.part";
pub const CACHE_CHUNK_MAP_FILE: &str = "chunk-map.bin";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkMap {
    chunks: Vec<bool>,
}

impl ChunkMap {
    pub fn new(chunk_count: u64) -> Self {
        Self {
            chunks: vec![false; chunk_count as usize],
        }
    }

    pub fn mark_available(&mut self, index: u64) -> Result<(), CacheError> {
        let chunk = self
            .chunks
            .get_mut(index as usize)
            .ok_or(CacheError::ChunkOutOfRange)?;
        *chunk = true;
        Ok(())
    }

    pub fn is_available(&self, index: u64) -> bool {
        self.chunks.get(index as usize).copied().unwrap_or(false)
    }

    pub fn bytes_available(&self, manifest: &MediaManifest) -> u64 {
        self.chunks
            .iter()
            .enumerate()
            .filter(|(_, available)| **available)
            .map(|(index, _)| chunk_len(manifest, index as u64))
            .sum()
    }

    pub fn complete(&self) -> bool {
        self.chunks.iter().all(|available| *available)
    }

    pub fn missing_chunks(&self) -> Vec<u64> {
        self.chunks
            .iter()
            .enumerate()
            .filter_map(|(index, available)| (!available).then_some(index as u64))
            .collect()
    }

    fn to_bytes(&self) -> Vec<u8> {
        self.chunks
            .iter()
            .map(|available| u8::from(*available))
            .collect()
    }

    fn from_bytes(bytes: Vec<u8>) -> Self {
        Self {
            chunks: bytes.into_iter().map(|value| value != 0).collect(),
        }
    }
}

#[derive(Debug)]
pub struct SparseCache {
    root: PathBuf,
    manifest: MediaManifest,
    chunk_map: ChunkMap,
}

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("MP-MEDIA-002 chunk index out of range")]
    ChunkOutOfRange,
    #[error("MP-MEDIA-002 chunk length is invalid")]
    InvalidChunkLength,
    #[error("MP-MEDIA-002 cache IO failed: {0}")]
    Io(#[from] std::io::Error),
}

impl SparseCache {
    pub fn open(root: impl AsRef<Path>, manifest: MediaManifest) -> Result<Self, CacheError> {
        let root = root.as_ref().join(&manifest.media_id);
        fs::create_dir_all(&root)?;
        let data_path = root.join(CACHE_DATA_FILE);
        let data = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .read(true)
            .open(&data_path)?;
        data.set_len(manifest.file_size)?;

        let chunk_map =
            read_chunk_map(&root).unwrap_or_else(|_| ChunkMap::new(manifest.chunk_count));

        Ok(Self {
            root,
            manifest,
            chunk_map,
        })
    }

    pub fn write_chunk(&mut self, index: u64, bytes: &[u8]) -> Result<(), CacheError> {
        if index >= self.manifest.chunk_count {
            return Err(CacheError::ChunkOutOfRange);
        }

        let expected = chunk_len(&self.manifest, index);
        if bytes.len() as u64 != expected {
            return Err(CacheError::InvalidChunkLength);
        }

        let mut data = OpenOptions::new()
            .write(true)
            .open(self.root.join(CACHE_DATA_FILE))?;
        data.seek(SeekFrom::Start(index * self.manifest.chunk_size))?;
        data.write_all(bytes)?;
        self.chunk_map.mark_available(index)?;
        self.persist_chunk_map()?;
        Ok(())
    }

    pub fn read_range(&self, start: u64, len: u64) -> Result<Option<Vec<u8>>, CacheError> {
        if len == 0 {
            return Ok(Some(Vec::new()));
        }

        let end = start.saturating_add(len).min(self.manifest.file_size);
        let first_chunk = start / self.manifest.chunk_size;
        let last_chunk = (end - 1) / self.manifest.chunk_size;
        for index in first_chunk..=last_chunk {
            if !self.chunk_map.is_available(index) {
                return Ok(None);
            }
        }

        let mut data = File::open(self.root.join(CACHE_DATA_FILE))?;
        data.seek(SeekFrom::Start(start))?;
        let mut bytes = vec![0; (end - start) as usize];
        data.read_exact(&mut bytes)?;
        Ok(Some(bytes))
    }

    pub fn bytes_available(&self) -> u64 {
        self.chunk_map.bytes_available(&self.manifest)
    }

    pub fn complete(&self) -> bool {
        self.chunk_map.complete()
    }

    pub fn chunk_map(&self) -> &ChunkMap {
        &self.chunk_map
    }

    pub fn missing_chunks(&self) -> Vec<u64> {
        self.chunk_map.missing_chunks()
    }

    fn persist_chunk_map(&self) -> Result<(), CacheError> {
        fs::write(
            self.root.join(CACHE_CHUNK_MAP_FILE),
            self.chunk_map.to_bytes(),
        )?;
        Ok(())
    }
}

pub fn chunk_len(manifest: &MediaManifest, index: u64) -> u64 {
    let start = index.saturating_mul(manifest.chunk_size);
    let remaining = manifest.file_size.saturating_sub(start);
    remaining.min(manifest.chunk_size)
}

fn read_chunk_map(root: &Path) -> Result<ChunkMap, CacheError> {
    let bytes = fs::read(root.join(CACHE_CHUNK_MAP_FILE))?;
    Ok(ChunkMap::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use uuid::Uuid;

    use crate::media::manifest::MediaManifest;

    use super::SparseCache;

    #[test]
    fn sparse_cache_writes_reads_and_resumes() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        fs::create_dir_all(&root).expect("temp");
        let manifest = manifest();
        let mut cache = SparseCache::open(&root, manifest.clone()).expect("open");

        cache.write_chunk(1, b"bbbb").expect("write");
        assert_eq!(cache.bytes_available(), 4);
        assert_eq!(
            cache.read_range(4, 4).expect("range"),
            Some(b"bbbb".to_vec())
        );
        assert_eq!(cache.read_range(0, 8).expect("range"), None);

        let resumed = SparseCache::open(&root, manifest).expect("resume");
        assert!(resumed.chunk_map().is_available(1));
        assert_eq!(resumed.missing_chunks(), vec![0, 2]);
        fs::remove_dir_all(&root).expect("cleanup");
    }

    fn manifest() -> MediaManifest {
        MediaManifest {
            media_id: "local-test".to_string(),
            filename: "movie.bin".to_string(),
            file_size: 12,
            container: None,
            full_hash: "hash".to_string(),
            quick_fingerprint: crate::media::manifest::QuickFingerprint {
                file_size: 12,
                first_hash: "first".to_string(),
                last_hash: "last".to_string(),
            },
            chunk_size: 4,
            chunk_count: 3,
        }
    }

    #[test]
    fn does_not_use_remote_paths() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        let manifest = manifest();
        let cache = SparseCache::open(&root, manifest).expect("open");
        assert!(cache.root.starts_with(Path::new(&root)));
        fs::remove_dir_all(&root).expect("cleanup");
    }
}
