use super::*;
use crate::providers::content::{parse_content, provider_page, ContentTab};

impl ManagedChromeSession {
    pub fn content_tabs(&self, provider: &str) -> Result<Vec<ContentTab>, ManagedChromeError> {
        let targets = self.targets()?;
        // Bound per-refresh work; never silently choose from an incomplete list.
        let targets: Vec<_> = targets
            .into_iter()
            .filter(|t| t.target_type == "page" && provider_page(provider, &t.url))
            .collect();
        if targets.len() > 12 {
            return Err(ManagedChromeError::CdpCommand(
                "Close extra provider tabs (at most 12) and refresh.".into(),
            ));
        }
        let mut tabs = Vec::new();
        let started = Instant::now();
        for target in targets {
            if started.elapsed() > Duration::from_secs(8) {
                return Err(ManagedChromeError::CdpCommand("Chrome is taking too long to check its tabs. Close unused provider tabs and refresh.".into()));
            }
            let mut page = match CdpPageSession::connect(&target.web_socket_debugger_url) {
                Ok(page) => page,
                Err(_) => continue,
            };
            let observed = match page.evaluate(OBSERVE_PAGE) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let Some(value) = observed["result"]["value"].as_object() else {
                continue;
            };
            let Some(url) = value.get("url").and_then(|v| v.as_str()) else {
                continue;
            };
            if !provider_page(provider, url) {
                continue;
            }
            // Reuse adapter identity observation. Only its public title is read;
            // canonical IDs always come from the validated URL parser.
            let command = crate::providers::sync::media_identity_command(
                crate::providers::sync::provider_id_from_str(provider)
                    .ok_or(ManagedChromeError::InvalidProviderProfile)?,
                page.next_command_id(),
            );
            let identity_metadata = page.execute(&command)?;
            let login_required = if provider == "youtube" {
                false
            } else {
                let command = crate::providers::sync::login_required_command(
                    crate::providers::sync::provider_id_from_str(provider).unwrap(),
                    page.next_command_id(),
                );
                page.execute(&command)?["result"]["value"]
                    .as_bool()
                    .unwrap_or(true)
            };
            let title = value
                .get("title")
                .and_then(|v| v.as_str())
                .filter(|v| !v.trim().is_empty())
                .or_else(|| identity_metadata["result"]["value"]["title"].as_str())
                .unwrap_or("")
                .trim();
            let title = title
                .chars()
                .filter(|c| !c.is_control())
                .take(300)
                .collect::<String>();
            let identity = parse_content(provider, url).ok();
            let issue = if login_required {
                Some("Sign in on the provider's own Chrome page, then refresh.".into())
            } else if identity.is_none() {
                Some("Choose a title or video in this tab.".into())
            } else if title.is_empty()
                || ["youtube", "netflix", "prime video", "hotstar", "jiohotstar"]
                    .contains(&title.to_lowercase().as_str())
            {
                Some("Waiting for the selected title's metadata.".into())
            } else {
                None
            };
            let thumbnail_url = identity
                .as_ref()
                .filter(|i| i.provider_id == "youtube")
                .map(|i| format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", i.content_id));
            let evidence = value
                .get("durationMs")
                .and_then(|v| v.as_u64())
                .map(|duration_ms| crate::providers::selection::ContentEvidence {
                    title: title.clone(),
                    duration_ms,
                    player_content_id: value
                        .get("playerContentId")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned),
                });
            let mut tab = ContentTab {
                target_id: target.id,
                title,
                identity,
                thumbnail_url,
                document_id: value
                    .get("documentId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .into(),
                focused: value
                    .get("focused")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                visible: value
                    .get("visible")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                playback_ready: !login_required
                    && value
                        .get("ready")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                evidence,
                issue,
                selection_key: String::new(),
            };
            tab.selection_key = tab.selection_key();
            tabs.push(tab);
        }
        Ok(tabs)
    }

    pub fn pin_content_target(&self, tab: Option<&ContentTab>) {
        *self
            .selected_target
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = tab.and_then(|t| {
            t.identity
                .as_ref()
                .map(|i| (t.target_id.clone(), i.allowed_url.clone()))
        });
    }

    pub fn connect_target(&self, id: &str) -> Result<CdpPageSession, ManagedChromeError> {
        let target = self
            .targets()?
            .into_iter()
            .find(|t| t.id == id && t.target_type == "page")
            .ok_or_else(|| {
                ManagedChromeError::CdpCommand(
                    "The provider tab closed. Refresh the chooser.".into(),
                )
            })?;
        CdpPageSession::connect(&target.web_socket_debugger_url)
    }

    pub(super) fn content_page_target(&self) -> Result<CdpTarget, ManagedChromeError> {
        let targets = self.targets()?;
        if let Some((id, expected)) = self
            .selected_target
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            return targets
                .into_iter()
                .find(|t| {
                    t.id == *id
                        && t.target_type == "page"
                        && parse_content(&self.lease.record.provider, &t.url)
                            .is_ok_and(|i| i.allowed_url == *expected)
                })
                .ok_or_else(|| {
                    ManagedChromeError::CdpCommand(
                        "The selected provider tab was closed. Select content again.".into(),
                    )
                });
        }
        let pages: Vec<_> = targets
            .into_iter()
            .filter(|t| t.target_type == "page")
            .collect();
        if pages.len() == 1 {
            return Ok(pages[0].clone());
        }
        if pages.len() > 12 {
            return Err(ManagedChromeError::CdpCommand(
                "Close extra browser tabs and retry.".into(),
            ));
        }
        let mut focused = Vec::new();
        let mut visible = Vec::new();
        for target in &pages {
            let Ok(mut page) = CdpPageSession::connect(&target.web_socket_debugger_url) else {
                continue;
            };
            let Ok(value) = page.evaluate(
                "({focused:document.hasFocus(),visible:document.visibilityState==='visible'})",
            ) else {
                continue;
            };
            if value["result"]["value"]["focused"].as_bool() == Some(true) {
                focused.push(target.clone());
            }
            if value["result"]["value"]["visible"].as_bool() == Some(true) {
                visible.push(target.clone());
            }
        }
        if focused.len() == 1 {
            return Ok(focused.remove(0));
        }
        if visible.len() == 1 {
            return Ok(visible.remove(0));
        }
        Err(ManagedChromeError::CdpCommand(
            "Several browser windows are open. Select the intended provider tab in Movie Party."
                .into(),
        ))
    }
}

// Read only public page metadata and media readiness. No storage, cookies,
// credentials, network interception, authentication or provider internals.
pub(super) const OBSERVE_PAGE: &str = r#"(() => {
 const videos = [...document.querySelectorAll('video')].filter(v => {
   const r=v.getBoundingClientRect(); return r.width>0 && r.height>0;
 });
 const v=videos.length===1 ? videos[0] : null;
 const playerContentId=v?.closest('ytd-watch-flexy[video-id], [data-video-id], [data-videoid], [data-content-id]');
 const contentId=playerContentId?.getAttribute('video-id') || playerContentId?.getAttribute('data-video-id') || playerContentId?.getAttribute('data-videoid') || playerContentId?.getAttribute('data-content-id') || (v && document.querySelector('meta[itemprop=videoId]')?.content) || null;
 return {url:location.href,
 durationMs:v && Number.isFinite(v.duration) && v.duration>0 ? Math.round(v.duration*1000) : null, playerContentId:contentId,
 title:document.querySelector('meta[property="og:title"]')?.content || document.title || '',
 documentId:String(performance.timeOrigin),focused:document.hasFocus(),visible:document.visibilityState==='visible',
 ready:Boolean(v && v.readyState>=2 && !v.error && Number.isFinite(v.currentTime))};
})()"#;
