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
    let trimmed = url.trim();
    let host = host_from_url(trimmed)?;

    if !is_supported_youtube_host(host) {
        return None;
    }

    if let Some(id) = parse_youtu_be(trimmed) {
        return Some(YoutubeContentId(id));
    }

    if let Some(id) = parse_path_id(trimmed, "/shorts/") {
        return Some(YoutubeContentId(id));
    }

    if let Some(id) = parse_path_id(trimmed, "/embed/") {
        return Some(YoutubeContentId(id));
    }

    parse_query_value(trimmed, "v").map(YoutubeContentId)
}

pub fn is_youtube_url(url: &str) -> bool {
    parse_youtube_content_id(url).is_some()
}

fn parse_youtu_be(url: &str) -> Option<String> {
    let marker = "youtu.be/";
    let start = url.find(marker)? + marker.len();
    read_video_id(&url[start..])
}

fn parse_path_id(url: &str, marker: &str) -> Option<String> {
    let start = url.find(marker)? + marker.len();
    read_video_id(&url[start..])
}

fn parse_query_value(url: &str, key: &str) -> Option<String> {
    let query_start = url.find('?')? + 1;
    let query = &url[query_start..];

    for pair in query.split('&') {
        if let Some((pair_key, value)) = pair.split_once('=') {
            if pair_key == key {
                return read_video_id(value);
            }
        }
    }

    None
}

fn read_video_id(input: &str) -> Option<String> {
    let candidate: String = input
        .chars()
        .take_while(|character| {
            character.is_ascii_alphanumeric() || *character == '-' || *character == '_'
        })
        .collect();

    if candidate.len() == 11 {
        Some(candidate)
    } else {
        None
    }
}

fn host_from_url(url: &str) -> Option<&str> {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let end = without_scheme
        .find(['/', '?', '#'])
        .unwrap_or(without_scheme.len());
    let host = &without_scheme[..end];

    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

fn is_supported_youtube_host(host: &str) -> bool {
    let lower = host.to_ascii_lowercase();

    lower == "youtu.be" || lower == "youtube.com" || lower.ends_with(".youtube.com")
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
