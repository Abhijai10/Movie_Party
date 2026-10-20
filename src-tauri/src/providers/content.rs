//! Host-local content identity. Never contains credentials or room wire data.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentIdentity {
    pub provider_id: String,
    pub content_id: String,
    pub allowed_url: String,
    pub playlist_id: Option<String>,
}

fn token(value: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&value.len())
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

pub fn provider_page(provider: &str, input: &str) -> bool {
    let Ok(url) = safe_url(input) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default();
    let domains: &[&str] = match provider {
        "youtube" => &[
            "youtube.com",
            "www.youtube.com",
            "m.youtube.com",
            "youtu.be",
        ],
        "netflix" => &["netflix.com", "www.netflix.com"],
        "prime" => &[
            "primevideo.com",
            "www.primevideo.com",
            "amazon.com",
            "www.amazon.com",
        ],
        "jiohotstar" => &[
            "hotstar.com",
            "www.hotstar.com",
            "jiocinema.com",
            "www.jiocinema.com",
        ],
        _ => return false,
    };
    domains.contains(&host)
}

fn safe_url(input: &str) -> Result<url::Url, String> {
    let input = input.trim();
    if input.len() > 4096 || input.chars().any(|c| c.is_control() || c == '\\') {
        return Err("Use a complete HTTPS title or video URL.".into());
    }
    let url = url::Url::parse(input).map_err(|_| "Use a complete HTTPS title or video URL.")?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.host_str().is_none()
    {
        return Err(
            "Only provider HTTPS URLs without credentials or custom ports are supported.".into(),
        );
    }
    Ok(url)
}

/// Playlist-only URLs are browse destinations. A concrete playlist video must
/// be selected before it can become a two-person synchronized source.
pub fn destination(provider: &str, input: &str) -> Result<String, String> {
    let url = safe_url(input)?;
    if !provider_page(provider, input) {
        return Err("This URL belongs to a different or unsupported provider.".into());
    }
    if provider == "youtube" && url.path() == "/playlist" {
        let lists: Vec<_> = url
            .query_pairs()
            .filter(|(k, _)| k == "list")
            .map(|(_, v)| v.into_owned())
            .collect();
        if lists.len() == 1 && token(&lists[0], 10, 100) {
            let mut out = url::Url::parse("https://www.youtube.com/playlist").unwrap();
            out.query_pairs_mut().append_pair("list", &lists[0]);
            return Ok(out.into());
        }
        return Err("Choose a supported YouTube playlist URL with one playlist ID.".into());
    }
    Ok(parse_content(provider, input)?.allowed_url)
}

pub fn parse_content(provider: &str, input: &str) -> Result<ContentIdentity, String> {
    let url = safe_url(input)?;
    if !provider_page(provider, input) {
        return Err("This URL belongs to a different or unsupported provider.".into());
    }
    let parts: Vec<_> = url
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect();
    let mut playlist_id = None;
    let (id, allowed_url) = match provider {
        "youtube" => {
            let ids: Vec<_> = url
                .query_pairs()
                .filter(|(k, _)| k == "v")
                .map(|(_, v)| v.into_owned())
                .collect();
            let lists: Vec<_> = url
                .query_pairs()
                .filter(|(k, _)| k == "list")
                .map(|(_, v)| v.into_owned())
                .collect();
            if ids.len() > 1 || lists.len() > 1 {
                return Err("The video URL contains ambiguous IDs.".into());
            }
            let id = if url.host_str() == Some("youtu.be") && parts.len() == 1 {
                if !ids.is_empty() {
                    return Err("The video URL contains ambiguous IDs.".into());
                }
                parts[0].to_owned()
            } else if parts == ["watch"] && ids.len() == 1 {
                ids[0].clone()
            } else if parts.len() == 2
                && ["shorts", "embed", "live"].contains(&parts[0])
                && ids.is_empty()
            {
                parts[1].to_owned()
            } else {
                return Err("Choose a YouTube video; home, search and playlist pages are not playable selections.".into());
            };
            if !token(&id, 11, 11) {
                return Err(
                    "The YouTube video ID must contain exactly 11 supported characters.".into(),
                );
            }
            if let Some(list) = lists.first() {
                if !token(list, 10, 100) {
                    return Err("The playlist ID is invalid.".into());
                }
                playlist_id = Some(list.clone());
            }
            let mut canonical = url::Url::parse("https://www.youtube.com/watch").unwrap();
            canonical.query_pairs_mut().append_pair("v", &id);
            if let Some(list) = &playlist_id {
                canonical.query_pairs_mut().append_pair("list", list);
            }
            (id, canonical.into())
        }
        "netflix"
            if parts.len() == 2
                && ["watch", "title"].contains(&parts[0])
                && parts[1].bytes().all(|c| c.is_ascii_digit())
                && token(parts[1], 1, 20) =>
        {
            (
                parts[1].into(),
                format!("https://www.netflix.com/{}/{}", parts[0], parts[1]),
            )
        }
        "prime" => {
            if url.host_str().is_some_and(|h| h.ends_with("amazon.com"))
                && !(parts.len() == 4 && parts[..3] == ["gp", "video", "detail"])
            {
                return Err("Choose an Amazon video detail URL, not a shopping product.".into());
            }
            let id = if parts.len() == 2 && ["detail", "dp"].contains(&parts[0]) {
                parts[1]
            } else if parts.len() == 3 && parts[0] == "detail" {
                parts[2]
            } else if parts.len() == 4 && parts[..3] == ["gp", "video", "detail"] {
                parts[3]
            } else {
                return Err("Choose a Prime Video detail or playback title URL.".into());
            };
            if !token(id, 10, 40) {
                return Err("The Prime Video content ID is invalid.".into());
            }
            (
                id.into(),
                format!("https://{}/{}", url.host_str().unwrap(), parts.join("/")),
            )
        }
        "jiohotstar" => {
            let id = parts
                .iter()
                .rev()
                .find(|p| p.len() >= 8 && p.len() <= 20 && p.bytes().all(|c| c.is_ascii_digit()))
                .ok_or("Choose a movie or episode URL with a content ID.")?;
            if !parts
                .iter()
                .any(|p| ["movies", "shows", "tv", "watch"].contains(p))
            {
                return Err("Choose a movie or episode title, not the provider homepage.".into());
            }
            (
                id.to_string(),
                format!("https://{}/{}", url.host_str().unwrap(), parts.join("/")),
            )
        }
        _ => {
            return Err(
                "Choose a supported title or video URL, not a homepage or search page.".into(),
            )
        }
    };
    Ok(ContentIdentity {
        provider_id: provider.into(),
        content_id: id,
        allowed_url,
        playlist_id,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentTab {
    pub selection_key: String,
    pub target_id: String,
    pub title: String,
    pub identity: Option<ContentIdentity>,
    pub thumbnail_url: Option<String>,
    pub document_id: String,
    pub focused: bool,
    pub visible: bool,
    pub playback_ready: bool,
    pub issue: Option<String>,
}
impl ContentTab {
    pub fn selection_key(&self) -> String {
        format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(
                    &self.target_id,
                    &self.identity,
                    &self.title,
                    &self.document_id
                ))
                .unwrap()
            )
        )
    }
    pub fn selectable(&self) -> bool {
        self.identity.is_some() && !self.title.trim().is_empty() && self.issue.is_none()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChooserSnapshot {
    pub browser_ready: bool,
    pub tabs: Vec<ContentTab>,
    pub suggested_target_id: Option<String>,
    pub selected: Option<ContentTab>,
    pub confirmed_key: Option<String>,
    pub content_ready: bool,
    pub issue: Option<String>,
}
impl ChooserSnapshot {
    pub fn reconcile(&mut self, tabs: Vec<ContentTab>) {
        self.browser_ready = true;
        self.issue = None;
        self.suggested_target_id = preferred_target(&tabs);
        if let Some(selected) = &self.selected {
            let next = tabs.iter().find(|t| {
                t.target_id == selected.target_id
                    && t.selectable()
                    && t.selection_key() == selected.selection_key()
            });
            if let Some(next) = next {
                self.selected = Some(next.clone());
            } else {
                self.selected = None;
                self.confirmed_key = None;
                self.issue = Some(
                    "The selected tab changed or closed. Preview and confirm your content again."
                        .into(),
                );
            }
        }
        self.content_ready = self.selected.as_ref().is_some_and(|t| t.playback_ready)
            && self.confirmed_key.is_some();
        self.tabs = tabs;
    }
    pub fn confirm(&mut self, target: &str, key: &str) -> Result<(), String> {
        let tab = self
            .tabs
            .iter()
            .find(|t| t.target_id == target && t.selectable() && t.selection_key() == key)
            .ok_or("Content changed or is unavailable. Refresh the preview and select again.")?;
        self.selected = Some(tab.clone());
        self.confirmed_key = Some(key.into());
        self.content_ready = tab.playback_ready;
        Ok(())
    }

    pub fn confirmed_url(
        &self,
        provider: &str,
        key: &str,
        requested: &str,
    ) -> Result<String, String> {
        let identity = self.selected.as_ref().and_then(|tab| tab.identity.as_ref());
        match identity {
            Some(identity)
                if self.content_ready
                    && self.confirmed_key.as_deref() == Some(key)
                    && identity.provider_id == provider
                    && identity.allowed_url == requested =>
            {
                Ok(identity.allowed_url.clone())
            }
            _ => Err(
                "The requested content does not match your confirmed selection. Select it again."
                    .into(),
            ),
        }
    }
}
pub fn preferred_target(tabs: &[ContentTab]) -> Option<String> {
    let focused: Vec<_> = tabs.iter().filter(|t| t.focused).collect();
    if focused.len() == 1 {
        return Some(focused[0].target_id.clone());
    }
    let visible: Vec<_> = tabs.iter().filter(|t| t.visible).collect();
    if visible.len() == 1 {
        return Some(visible[0].target_id.clone());
    }
    (tabs.len() == 1).then(|| tabs[0].target_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tab(id: &str) -> ContentTab {
        ContentTab {
            selection_key: String::new(),
            target_id: id.into(),
            title: "A real title".into(),
            identity: Some(parse_content("youtube", "https://youtu.be/dQw4w9WgXcQ").unwrap()),
            thumbnail_url: None,
            document_id: "100".into(),
            focused: false,
            visible: false,
            playback_ready: true,
            issue: None,
        }
    }
    #[test]
    fn canonical_video_identity_strips_tracking_and_keeps_playlist_context() {
        for input in [
            "https://youtu.be/dQw4w9WgXcQ?t=20",
            "https://m.youtube.com/shorts/dQw4w9WgXcQ",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
            "https://www.youtube.com/live/dQw4w9WgXcQ",
        ] {
            let value = parse_content("youtube", input).unwrap();
            assert_eq!(value.content_id, "dQw4w9WgXcQ");
            assert_eq!(
                value.allowed_url,
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
            );
        }
        let value = parse_content(
            "youtube",
            "https://youtube.com/watch?v=dQw4w9WgXcQ&list=PL1234567890&index=2&tracking=private",
        )
        .unwrap();
        assert_eq!(value.playlist_id.as_deref(), Some("PL1234567890"));
        assert_eq!(
            value.allowed_url,
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL1234567890"
        );
    }
    #[test]
    fn playlist_is_a_browse_destination_never_a_ready_video() {
        let url = "https://youtube.com/playlist?list=PL1234567890&tracking=discard";
        assert_eq!(
            destination("youtube", url).unwrap(),
            "https://www.youtube.com/playlist?list=PL1234567890"
        );
        assert!(parse_content("youtube", url).is_err());
        for url in [
            "https://youtube.com/playlist?list=x",
            "https://youtube.com/playlist?list=PL1234567890&list=PLabcdefghij",
        ] {
            assert!(destination("youtube", url).is_err());
        }
    }
    #[test]
    fn rejects_unrelated_schemes_credentials_ports_homepages_and_ambiguous_ids() {
        for url in [
            "javascript:alert(1)",
            "file:///watch?v=dQw4w9WgXcQ",
            "http://youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com.evil.test/watch?v=dQw4w9WgXcQ",
            "https://youtube.com@evil.test/watch?v=dQw4w9WgXcQ",
            "https://u:p@youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com:9443/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/",
            "https://youtube.com/results?v=dQw4w9WgXcQ",
            "https://youtube.com/watch?v=dQw4w9WgXcQ&v=abcdefghijk",
            "https://youtu.be/dQw4w9WgXcQ?v=abcdefghijk",
            "https://youtube.com/watch?v=dQw4w9WgXcQ%2Fbad",
            "https://youtube.com/watch?v=abcdefghijkX",
            "https://youtube.com\\@evil.test/watch?v=dQw4w9WgXcQ",
        ] {
            assert!(parse_content("youtube", url).is_err(), "accepted {url}");
        }
    }
    #[test]
    fn subscription_titles_require_real_routes_and_clean_canonical_urls() {
        for (provider, input, id) in [
            (
                "netflix",
                "https://www.netflix.com/watch/81234567?trackId=private",
                "81234567",
            ),
            (
                "netflix",
                "https://www.netflix.com/title/81234567",
                "81234567",
            ),
            (
                "prime",
                "https://www.primevideo.com/detail/0ABCDEFGHIJK?ref=tracking",
                "0ABCDEFGHIJK",
            ),
            (
                "prime",
                "https://www.amazon.com/gp/video/detail/B012345678?ref=x",
                "B012345678",
            ),
            (
                "jiohotstar",
                "https://www.hotstar.com/in/movies/a-movie/1234567890/watch?ref=x",
                "1234567890",
            ),
        ] {
            let identity = parse_content(provider, input).unwrap();
            assert_eq!(identity.content_id, id);
            assert!(!identity.allowed_url.contains('?'));
        }
        for (provider, url) in [
            ("netflix", "https://netflix.com/browse"),
            ("netflix", "https://netflix.com/search?q=movie"),
            ("prime", "https://amazon.com/dp/B012345678"),
            ("prime", "https://primevideo.com/search"),
            ("jiohotstar", "https://hotstar.com/in"),
            ("netflix", "https://primevideo.com/detail/B012345678"),
        ] {
            assert!(parse_content(provider, url).is_err());
        }
    }
    #[test]
    fn tab_preference_follows_focus_then_visibility_and_refuses_multiple_windows() {
        let mut a = tab("first");
        let mut b = tab("second");
        b.focused = true;
        assert_eq!(
            preferred_target(&[a.clone(), b.clone()]).as_deref(),
            Some("second")
        );
        b.focused = false;
        b.visible = true;
        assert_eq!(
            preferred_target(&[a.clone(), b.clone()]).as_deref(),
            Some("second")
        );
        a.visible = true;
        assert_eq!(preferred_target(&[a, b]), None);
        assert_eq!(preferred_target(&[]), None);
    }
    #[test]
    fn browser_ready_never_means_content_ready_without_confirmation() {
        let mut chooser = ChooserSnapshot::default();
        let video = tab("a");
        chooser.reconcile(vec![video.clone()]);
        assert!(chooser.browser_ready);
        assert!(!chooser.content_ready);
        assert!(chooser.confirm("a", "stale preview").is_err());
        chooser.confirm("a", &video.selection_key()).unwrap();
        assert!(chooser.content_ready);
    }
    #[test]
    fn a_valid_key_cannot_attach_a_different_or_unsafe_requested_url() {
        let video = tab("a");
        let mut chooser = ChooserSnapshot::default();
        chooser.reconcile(vec![video.clone()]);
        let url = &video.identity.as_ref().unwrap().allowed_url;
        let key = video.selection_key();
        assert!(chooser.confirmed_url("youtube", &key, url).is_err());
        chooser.confirm("a", &key).unwrap();
        assert_eq!(chooser.confirmed_url("youtube", &key, url).unwrap(), *url);
        for request in [
            "javascript:alert(1)",
            "https://evil.example/video",
            "https://www.youtube.com/",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        ] {
            if request != url {
                assert!(chooser.confirmed_url("youtube", &key, request).is_err());
            }
        }
        assert!(chooser.confirmed_url("netflix", &key, url).is_err());
        assert!(chooser.confirmed_url("youtube", "stale-key", url).is_err());
    }

    #[test]
    fn changed_url_title_document_or_closed_tab_invalidates_confirmation() {
        let video = tab("a");
        for kind in 0..4 {
            let mut chooser = ChooserSnapshot::default();
            chooser.reconcile(vec![video.clone()]);
            chooser.confirm("a", &video.selection_key()).unwrap();
            let mut changed = video.clone();
            match kind {
                0 => {
                    changed.identity =
                        Some(parse_content("youtube", "https://youtu.be/abcdefghijk").unwrap())
                }
                1 => changed.title = "New title".into(),
                2 => changed.document_id = "reloaded".into(),
                _ => {}
            }
            chooser.reconcile(if kind == 3 { vec![] } else { vec![changed] });
            assert!(!chooser.content_ready);
            assert!(chooser.selected.is_none());
            assert!(chooser.issue.is_some());
        }
    }
    #[test]
    fn buffering_retracts_readiness_without_losing_confirmed_identity() {
        let mut video = tab("a");
        let mut chooser = ChooserSnapshot::default();
        video.playback_ready = false;
        chooser.reconcile(vec![video.clone()]);
        chooser.confirm("a", &video.selection_key()).unwrap();
        assert!(!chooser.content_ready);
        video.playback_ready = true;
        chooser.reconcile(vec![video.clone()]);
        assert!(chooser.content_ready);
        video.playback_ready = false;
        chooser.reconcile(vec![video]);
        assert!(!chooser.content_ready);
        assert!(chooser.confirmed_key.is_some());
    }
    #[test]
    fn homepages_missing_titles_and_errors_cannot_be_confirmed() {
        for kind in 0..3 {
            let mut video = tab("a");
            match kind {
                0 => video.identity = None,
                1 => video.title.clear(),
                _ => video.issue = Some("Sign in".into()),
            };
            let mut chooser = ChooserSnapshot::default();
            chooser.reconcile(vec![video.clone()]);
            assert!(chooser.confirm("a", &video.selection_key()).is_err());
        }
    }
}
