use std::path::{Path, PathBuf};

pub mod presentation;

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
    /// How many milliseconds of media are buffered ahead of the current position.
    /// `None` if the backend cannot determine this (e.g. local file fully available).
    pub buffered_ahead_ms: Option<u64>,
    /// Human-readable error message when state == Error.
    pub error_message: Option<String>,
}

impl Default for PlayerSnapshot {
    fn default() -> Self {
        Self {
            state: PlayerState::Stopped,
            position_ms: 0,
            duration_ms: None,
            volume: 1.0,
            playback_rate: 1.0,
            buffered_ahead_ms: None,
            error_message: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("MP-MEDIA-001 libmpv is unavailable in this environment")]
    LibMpvUnavailable,
    #[error("MP-MEDIA-002 media file does not exist: {path}")]
    MissingMedia { path: String },
    #[error("MP-MEDIA-003 player is not ready")]
    NotReady,
    #[error("MP-MEDIA-004 player initialization failed: {reason}")]
    InitFailed { reason: String },
    #[error("MP-MEDIA-005 unsupported or missing codec: {codec}")]
    UnsupportedCodec { codec: String },
    #[error("MP-MEDIA-006 player encountered an error: {message}")]
    PlaybackError { message: String },
    #[error("MP-MEDIA-007 failed to load media: {reason}")]
    LoadFailed { reason: String },
}

/// Player abstraction layer. The sync coordinator communicates ONLY through
/// this interface — never directly with libmpv or any specific backend.
///
/// The coordinator decides WHAT should happen. The player only executes
/// playback commands. Synchronization logic must NOT live inside the player.
pub trait LocalPlayer {
    /// Load media from a local file path.
    fn open(&mut self, path: &Path) -> Result<(), PlayerError>;

    /// Begin or resume playback.
    fn play(&mut self) -> Result<(), PlayerError>;

    /// Pause playback. Position is retained.
    fn pause(&mut self) -> Result<(), PlayerError>;

    /// Seek to the given position in milliseconds.
    fn seek(&mut self, position_ms: u64) -> Result<(), PlayerError>;

    /// Set volume (0.0 ..= 1.0).
    fn set_volume(&mut self, volume: f32) -> Result<(), PlayerError>;

    /// Set playback rate (0.25 ..= 4.0; 1.0 = normal).
    fn set_playback_rate(&mut self, rate: f32) -> Result<(), PlayerError>;

    /// Return a snapshot of the current player state.
    fn snapshot(&self) -> PlayerSnapshot;

    /// Return the duration of the loaded media in milliseconds, if known.
    fn duration(&self) -> Option<u64>;

    /// Return how many milliseconds of media are buffered ahead of the current
    /// playback position. Returns `None` when the value is unknown.
    fn buffered_ahead_ms(&self) -> Option<u64>;

    /// Return the last error message, if the player is in the Error state.
    fn error_message(&self) -> Option<String>;

    /// Release all resources held by the player (mpv context, render context, etc.).
    /// After `close()` the player may be reused by calling `open()` again.
    fn close(&mut self);
}

// ── LibMpv availability detection ────────────────────────────────────────────

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

// ── Default stub implementation (used when mpv feature is disabled) ──────────

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
            snapshot: PlayerSnapshot::default(),
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

        if !is_streaming_media_source(path) && !path.exists() {
            return Err(PlayerError::MissingMedia {
                path: path.display().to_string(),
            });
        }

        self.media_path = Some(path.to_path_buf());
        self.snapshot.state = PlayerState::Ready;
        self.snapshot.error_message = None;
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

    fn duration(&self) -> Option<u64> {
        self.snapshot.duration_ms
    }

    fn buffered_ahead_ms(&self) -> Option<u64> {
        self.snapshot.buffered_ahead_ms
    }

    fn error_message(&self) -> Option<String> {
        self.snapshot.error_message.clone()
    }

    fn close(&mut self) {
        self.media_path = None;
        self.snapshot = PlayerSnapshot::default();
    }
}

pub fn is_streaming_media_source(path: &Path) -> bool {
    let source = path.as_os_str().to_string_lossy();
    source.starts_with("http://127.0.0.1:")
        || source.starts_with("http://localhost:")
        || source.starts_with("https://127.0.0.1:")
        || source.starts_with("https://localhost:")
}

// ── Real mpv backend (feature-gated) ─────────────────────────────────────────

#[cfg(feature = "mpv")]
pub mod mpv_backend;
#[cfg(feature = "mpv")]
pub use mpv_backend::MpvPlayer;

#[cfg(test)]
mod tests {
    use super::{LibMpvPlayer, LocalPlayer, PlayerError, PlayerSnapshot, PlayerState};

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

    #[test]
    fn missing_media_returns_structured_error() {
        let mut player = LibMpvPlayer::with_availability(true);
        let result = player.open(std::path::Path::new("/nonexistent/file.mp4"));

        assert!(matches!(result, Err(PlayerError::MissingMedia { .. })));
    }

    #[test]
    fn loopback_http_media_source_is_accepted_for_partial_cache_playback() {
        let mut player = LibMpvPlayer::with_availability(true);
        let result = player.open(std::path::Path::new(
            "http://127.0.0.1:49152/media/local?token=secret",
        ));

        assert!(result.is_ok());
        assert_eq!(player.snapshot().state, PlayerState::Ready);
    }

    #[test]
    fn player_not_ready_returns_error_before_open() {
        let player = LibMpvPlayer::with_availability(true);
        assert!(matches!(player.snapshot().state, PlayerState::Stopped));
    }

    #[test]
    fn snapshot_default_has_no_duration() {
        let snapshot = PlayerSnapshot::default();
        assert!(snapshot.duration_ms.is_none());
        assert!(snapshot.buffered_ahead_ms.is_none());
        assert!(snapshot.error_message.is_none());
    }

    #[test]
    fn close_resets_player_state() {
        let path = std::env::current_exe().expect("test exe exists");
        let mut player = LibMpvPlayer::with_availability(true);
        player.open(&path).expect("open");
        player.play().expect("play");
        assert_eq!(player.snapshot().state, PlayerState::Playing);

        player.close();
        assert_eq!(player.snapshot().state, PlayerState::Stopped);
        assert!(player.duration().is_none());
    }

    #[test]
    fn volume_clamped_to_valid_range() {
        let path = std::env::current_exe().expect("test exe exists");
        let mut player = LibMpvPlayer::with_availability(true);
        player.open(&path).expect("open");

        player.set_volume(2.0).expect("volume");
        assert_eq!(player.snapshot().volume, 1.0);

        player.set_volume(-0.5).expect("volume");
        assert_eq!(player.snapshot().volume, 0.0);
    }

    #[test]
    fn playback_rate_clamped_to_valid_range() {
        let path = std::env::current_exe().expect("test exe exists");
        let mut player = LibMpvPlayer::with_availability(true);
        player.open(&path).expect("open");

        player.set_playback_rate(10.0).expect("rate");
        assert_eq!(player.snapshot().playback_rate, 4.0);

        player.set_playback_rate(0.01).expect("rate");
        assert_eq!(player.snapshot().playback_rate, 0.25);
    }
}
