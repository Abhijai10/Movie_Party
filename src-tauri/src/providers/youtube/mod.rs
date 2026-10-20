use serde_json::json;

use super::{chrome::CdpCommand, generic::GenericProviderAdapter};

pub const PROVIDER_ID: &str = "youtube";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YoutubeContentId(String);

impl YoutubeContentId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct YoutubeAdapter {
    generic: GenericProviderAdapter,
}

impl YoutubeAdapter {
    pub fn detect_player(&self, id: u64) -> CdpCommand {
        runtime_command(
            id,
            "Boolean(document.querySelector('video') && (document.querySelector('.html5-video-player') || location.hostname.includes('youtube.com')))",
        )
    }

    pub fn play(&self, id: u64) -> CdpCommand {
        self.generic.play(id)
    }

    pub fn pause(&self, id: u64) -> CdpCommand {
        self.generic.pause(id)
    }

    pub fn seek(&self, id: u64, seconds: f64) -> CdpCommand {
        self.generic.seek(id, seconds)
    }

    pub fn get_position(&self, id: u64) -> CdpCommand {
        self.generic.get_position(id)
    }

    pub fn get_buffer_state(&self, id: u64) -> CdpCommand {
        self.generic.get_buffer_state(id)
    }
}

pub fn parse_youtube_content_id(url: &str) -> Option<YoutubeContentId> {
    crate::providers::content::parse_content("youtube", url)
        .ok()
        .map(|identity| YoutubeContentId(identity.content_id))
}

pub fn is_youtube_url(url: &str) -> bool {
    parse_youtube_content_id(url).is_some()
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
    fn parses_supported_youtube_urls() {
        let urls = [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ?t=43",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
        ];

        for url in urls {
            assert_eq!(
                parse_youtube_content_id(url).map(|id| id.as_str().to_owned()),
                Some("dQw4w9WgXcQ".to_owned())
            );
        }
    }

    #[test]
    fn rejects_non_youtube_or_invalid_ids() {
        assert!(!is_youtube_url("https://example.com/watch?v=dQw4w9WgXcQ"));
        assert!(!is_youtube_url("https://www.youtube.com/watch?v=too-short"));
    }

    #[test]
    fn detects_youtube_player_inside_youtube_adapter() {
        let adapter = YoutubeAdapter::default();
        let command = adapter.detect_player(1);
        let expression = expression(&command);

        assert_eq!(command.method, "Runtime.evaluate");
        assert!(expression.contains(".html5-video-player"));
        assert!(expression.contains("youtube.com"));
    }

    #[test]
    fn delegates_media_controls_to_generic_html_video() {
        let adapter = YoutubeAdapter::default();

        assert!(expression(&adapter.play(2)).contains(".play()"));
        assert!(expression(&adapter.pause(3)).contains(".pause()"));
        assert!(expression(&adapter.seek(4, 12.0)).contains("currentTime = 12"));
        assert!(expression(&adapter.get_position(5)).contains("currentTime"));
        assert!(expression(&adapter.get_buffer_state(6)).contains("buffered.end"));
    }
}
