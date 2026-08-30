pub mod sqlite;

pub const MIGRATIONS_DIR: &str = "migrations";

use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionDecision {
    Remove,
    KeepInMovieParty,
    SaveAs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionPolicy {
    AskEveryTime,
    AlwaysRemove,
    AlwaysKeep,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPrompt {
    pub media_id: String,
    pub filename: String,
    pub default_decision: RetentionDecision,
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("MP-MEDIA-002 cache path is outside cache root")]
    UnsafeCachePath,
    #[error("MP-MEDIA-002 storage IO failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("MP-MEDIA-002 SQLite error: {0}")]
    Sqlite(String),
}

pub fn prompt_for_media(
    media_id: impl Into<String>,
    filename: impl Into<String>,
) -> RetentionPrompt {
    RetentionPrompt {
        media_id: media_id.into(),
        filename: filename.into(),
        default_decision: RetentionDecision::Remove,
    }
}

pub fn apply_retention_decision(
    cache_root: &Path,
    media_cache_dir: &Path,
    data_file: &Path,
    decision: RetentionDecision,
    save_as_path: Option<&Path>,
) -> Result<Option<PathBuf>, StorageError> {
    if !media_cache_dir.starts_with(cache_root) {
        return Err(StorageError::UnsafeCachePath);
    }

    match decision {
        RetentionDecision::Remove => {
            if media_cache_dir.exists() {
                fs::remove_dir_all(media_cache_dir)?;
            }
            Ok(None)
        }
        RetentionDecision::KeepInMovieParty => Ok(Some(media_cache_dir.to_path_buf())),
        RetentionDecision::SaveAs => {
            let destination = save_as_path.ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing Save As path")
            })?;
            fs::copy(data_file, destination)?;
            Ok(Some(destination.to_path_buf()))
        }
    }
}

pub fn decision_for_policy(policy: RetentionPolicy) -> Option<RetentionDecision> {
    match policy {
        RetentionPolicy::AskEveryTime => None,
        RetentionPolicy::AlwaysRemove => Some(RetentionDecision::Remove),
        RetentionPolicy::AlwaysKeep => Some(RetentionDecision::KeepInMovieParty),
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use uuid::Uuid;

    use super::{
        apply_retention_decision, decision_for_policy, prompt_for_media, RetentionDecision,
        RetentionPolicy,
    };

    #[test]
    fn prompt_defaults_to_remove() {
        let prompt = prompt_for_media("media", "movie.mkv");

        assert_eq!(prompt.default_decision, RetentionDecision::Remove);
    }

    #[test]
    fn removes_cache_after_confirmation() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        let media = root.join("media");
        fs::create_dir_all(&media).expect("cache");
        fs::write(media.join("data.part"), b"movie").expect("data");

        apply_retention_decision(
            &root,
            &media,
            &media.join("data.part"),
            RetentionDecision::Remove,
            None,
        )
        .expect("remove");

        assert!(!media.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn saves_as_selected_path() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        let media = root.join("media");
        let destination = root.join("saved.mkv");
        fs::create_dir_all(&media).expect("cache");
        let data = media.join("data.part");
        let mut file = fs::File::create(&data).expect("file");
        file.write_all(b"movie").expect("write");

        apply_retention_decision(
            &root,
            &media,
            &data,
            RetentionDecision::SaveAs,
            Some(&destination),
        )
        .expect("save as");

        assert_eq!(fs::read(destination).expect("saved"), b"movie");
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn maps_policy_to_automatic_decision() {
        assert_eq!(decision_for_policy(RetentionPolicy::AskEveryTime), None);
        assert_eq!(
            decision_for_policy(RetentionPolicy::AlwaysKeep),
            Some(RetentionDecision::KeepInMovieParty),
        );
    }
}
