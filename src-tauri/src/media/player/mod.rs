use std::path::{Path, PathBuf};

pub const LOCAL_PLAYER_BACKEND: &str = "libmpv";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    Stopped,
    Ready,
    Playing,
    Paused,
    Buffering,
    Seeking,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSnapshot {
    pub state: PlayerState,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f32,
    pub playback_rate: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("MP-MEDIA-001 libmpv is unavailable in this environment")]
    LibMpvUnavailable,
    #[error("MP-MEDIA-001 media file does not exist")]
    MissingMedia,
    #[error("MP-MEDIA-001 player is not ready")]
    NotReady,
}

pub trait LocalPlayer {
    fn open(&mut self, path: &Path) -> Result<(), PlayerError>;
    fn play(&mut self) -> Result<(), PlayerError>;
    fn pause(&mut self) -> Result<(), PlayerError>;
    fn seek(&mut self, position_ms: u64) -> Result<(), PlayerError>;
    fn set_volume(&mut self, volume: f32) -> Result<(), PlayerError>;
    fn set_playback_rate(&mut self, rate: f32) -> Result<(), PlayerError>;
    fn snapshot(&self) -> PlayerSnapshot;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibMpvAvailability {
    pub available: bool,
    pub checked_paths: Vec<PathBuf>,
}

pub fn detect_libmpv() -> LibMpvAvailability {
    let checked_paths = candidate_libmpv_paths();
    let available = checked_paths.iter().any(|path| path.exists());

    LibMpvAvailability {
        available,
        checked_paths,
    }
}

pub struct LibMpvPlayer {
    available: bool,
    media_path: Option<PathBuf>,
    snapshot: PlayerSnapshot,
}

impl LibMpvPlayer {
    pub fn new() -> Self {
        Self::with_availability(detect_libmpv().available)
    }

    pub fn with_availability(available: bool) -> Self {
        Self {
            available,
            media_path: None,
            snapshot: PlayerSnapshot {
                state: PlayerState::Stopped,
                position_ms: 0,
                duration_ms: None,
                volume: 1.0,
                playback_rate: 1.0,
            },
        }
    }

    fn ensure_ready(&self) -> Result<(), PlayerError> {
        if !self.available {
            return Err(PlayerError::LibMpvUnavailable);
        }

        if self.media_path.is_none() {
            return Err(PlayerError::NotReady);
        }

        Ok(())
    }
}

impl Default for LibMpvPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalPlayer for LibMpvPlayer {
    fn open(&mut self, path: &Path) -> Result<(), PlayerError> {
        if !self.available {
            return Err(PlayerError::LibMpvUnavailable);
        }

        if !path.exists() {
            return Err(PlayerError::MissingMedia);
        }

        self.media_path = Some(path.to_path_buf());
        self.snapshot.state = PlayerState::Ready;
        Ok(())
    }

    fn play(&mut self) -> Result<(), PlayerError> {
        self.ensure_ready()?;
        self.snapshot.state = PlayerState::Playing;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), PlayerError> {
        self.ensure_ready()?;
        self.snapshot.state = PlayerState::Paused;
        Ok(())
    }

    fn seek(&mut self, position_ms: u64) -> Result<(), PlayerError> {
        self.ensure_ready()?;
        self.snapshot.position_ms = position_ms;
        self.snapshot.state = PlayerState::Seeking;
        Ok(())
    }

    fn set_volume(&mut self, volume: f32) -> Result<(), PlayerError> {
        self.ensure_ready()?;
        self.snapshot.volume = volume.clamp(0.0, 1.0);
        Ok(())
    }

    fn set_playback_rate(&mut self, rate: f32) -> Result<(), PlayerError> {
        self.ensure_ready()?;
        self.snapshot.playback_rate = rate.clamp(0.25, 4.0);
        Ok(())
    }

    fn snapshot(&self) -> PlayerSnapshot {
        self.snapshot.clone()
    }
}

fn candidate_libmpv_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from("/opt/homebrew/lib/libmpv.dylib"));
        paths.push(PathBuf::from("/usr/local/lib/libmpv.dylib"));
        paths.push(PathBuf::from(
            "/Applications/mpv.app/Contents/MacOS/libmpv.dylib",
        ));
    }

    #[cfg(target_os = "windows")]
    {
        paths.push(PathBuf::from("mpv-2.dll"));
        paths.push(PathBuf::from(r"C:\Program Files\mpv\mpv-2.dll"));
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::{LibMpvPlayer, LocalPlayer, PlayerError, PlayerState};

    #[test]
    fn reports_unavailable_libmpv_without_panicking() {
        let mut player = LibMpvPlayer::with_availability(false);
        let result = player.play();

        assert!(matches!(result, Err(PlayerError::LibMpvUnavailable)));
    }

    #[test]
    fn mock_available_player_tracks_basic_state() {
        let path = std::env::current_exe().expect("test exe exists");
        let mut player = LibMpvPlayer::with_availability(true);

        player.open(&path).expect("open");
        player.seek(42_000).expect("seek");
        player.play().expect("play");

        let snapshot = player.snapshot();
        assert_eq!(snapshot.state, PlayerState::Playing);
        assert_eq!(snapshot.position_ms, 42_000);
    }
}
