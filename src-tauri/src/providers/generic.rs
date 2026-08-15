use serde_json::json;

use super::chrome::CdpCommand;

pub const GENERIC_MEDIA_SELECTOR: &str = "video,audio";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderPlayerState {
    Playing,
    Paused,
    Buffering,
    Ended,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GenericMediaSnapshot {
    pub duration_seconds: Option<f64>,
    pub position_seconds: f64,
    pub paused: bool,
    pub ended: bool,
    pub ready_state: u8,
    pub buffered_end_seconds: Option<f64>,
}

impl GenericMediaSnapshot {
    pub fn player_state(self) -> ProviderPlayerState {
        if self.ended {
            return ProviderPlayerState::Ended;
        }

        if self.ready_state < 3 {
            return ProviderPlayerState::Buffering;
        }

        if self.paused {
            ProviderPlayerState::Paused
        } else {
            ProviderPlayerState::Playing
        }
    }

    pub fn buffer_ahead_seconds(self) -> Option<f64> {
        self.buffered_end_seconds
            .map(|buffered_end| (buffered_end - self.position_seconds).max(0.0))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GenericProviderAdapter;

impl GenericProviderAdapter {
    pub fn detect_page(&self, id: u64) -> CdpCommand {
        runtime_command(id, "Boolean(document.querySelector('video,audio'))")
    }

    pub fn identify_media(&self, id: u64) -> CdpCommand {
        runtime_command(
            id,
            "(() => { const media = document.querySelector('video,audio'); if (!media) return null; return { title: document.title || '', duration_seconds: Number.isFinite(media.duration) ? media.duration : null }; })()",
        )
    }

    pub fn get_position(&self, id: u64) -> CdpCommand {
        runtime_command(
            id,
            "(() => { const media = document.querySelector('video,audio'); return media ? media.currentTime : null; })()",
        )
    }

    pub fn get_player_state(&self, id: u64) -> CdpCommand {
        runtime_command(
            id,
            "(() => { const media = document.querySelector('video,audio'); if (!media) return null; return { paused: media.paused, ended: media.ended, ready_state: media.readyState }; })()",
        )
    }

    pub fn get_buffer_state(&self, id: u64) -> CdpCommand {
        runtime_command(
            id,
            "(() => { const media = document.querySelector('video,audio'); if (!media || media.buffered.length === 0) return null; return media.buffered.end(media.buffered.length - 1); })()",
        )
    }

    pub fn play(&self, id: u64) -> CdpCommand {
        runtime_command(
            id,
            "(() => { const media = document.querySelector('video,audio'); if (!media) return false; void media.play(); return true; })()",
        )
    }

    pub fn pause(&self, id: u64) -> CdpCommand {
        runtime_command(
            id,
            "(() => { const media = document.querySelector('video,audio'); if (!media) return false; media.pause(); return true; })()",
        )
    }

    pub fn seek(&self, id: u64, seconds: f64) -> CdpCommand {
        let target = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        runtime_command(
            id,
            &format!(
                "(() => {{ const media = document.querySelector('video,audio'); if (!media) return false; media.currentTime = {target}; return true; }})()"
            ),
        )
    }

    pub fn set_playback_rate(&self, id: u64, rate: f64) -> CdpCommand {
        let target = if rate.is_finite() {
            rate.clamp(0.5, 2.0)
        } else {
            1.0
        };
        runtime_command(
            id,
            &format!(
                "(() => {{ const media = document.querySelector('video,audio'); if (!media) return false; media.playbackRate = {target}; return true; }})()"
            ),
        )
    }
}

fn runtime_command(id: u64, expression: &str) -> CdpCommand {
    CdpCommand {
        id,
        method: "Runtime.evaluate",
        params: json!({
            "expression": expression,
            "awaitPromise": true,
            "returnByValue": true
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expression(command: &CdpCommand) -> &str {
        command.params["expression"]
            .as_str()
            .expect("expression string")
    }

    #[test]
    fn generic_adapter_uses_only_html_media_detection() {
        let adapter = GenericProviderAdapter;
        let command = adapter.detect_page(1);
        let expression = expression(&command).to_ascii_lowercase();

        assert_eq!(command.method, "Runtime.evaluate");
        assert!(expression.contains(GENERIC_MEDIA_SELECTOR));
        assert!(!expression.contains("netflix"));
        assert!(!expression.contains("prime"));
        assert!(!expression.contains("hotstar"));
        assert!(!expression.contains("youtube"));
    }

    #[test]
    fn generic_adapter_builds_play_pause_seek_and_rate_commands() {
        let adapter = GenericProviderAdapter;

        assert!(expression(&adapter.play(1)).contains(".play()"));
        assert!(expression(&adapter.pause(2)).contains(".pause()"));
        assert!(expression(&adapter.seek(3, 42.5)).contains("currentTime = 42.5"));
        assert!(expression(&adapter.seek(4, -1.0)).contains("currentTime = 0"));
        assert!(expression(&adapter.set_playback_rate(5, 3.0)).contains("playbackRate = 2"));
    }

    #[test]
    fn snapshot_maps_player_and_buffer_state() {
        let playing = GenericMediaSnapshot {
            duration_seconds: Some(120.0),
            position_seconds: 10.0,
            paused: false,
            ended: false,
            ready_state: 4,
            buffered_end_seconds: Some(22.5),
        };

        assert_eq!(playing.player_state(), ProviderPlayerState::Playing);
        assert_eq!(playing.buffer_ahead_seconds(), Some(12.5));

        let buffering = GenericMediaSnapshot {
            ready_state: 2,
            ..playing
        };

        assert_eq!(buffering.player_state(), ProviderPlayerState::Buffering);

        let ended = GenericMediaSnapshot {
            ended: true,
            ..playing
        };

        assert_eq!(ended.player_state(), ProviderPlayerState::Ended);
    }
}
