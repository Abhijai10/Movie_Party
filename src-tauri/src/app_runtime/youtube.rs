use super::*;
use crate::providers::{
    playback::{Action, Observation, Outcome, Report},
    selection::{ContentEvidence, ContentLoadingState, SelectionContext},
    youtube::embed::EmbedServer,
};

#[derive(Default)]
pub(super) struct YoutubeState {
    pub enabled: bool,
    pub handoff: bool,
    pub chosen: bool,
    pub lease: Option<String>,
    pub server: Option<Arc<EmbedServer>>,
    pub seq: u64,
    pub serial: u64,
    pub last_seen: Option<Instant>,
    pub command: Option<EmbedCommand>,
    pub metadata: Option<YoutubeMetadata>,
    pub error: Option<u16>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct YoutubeMetadata {
    pub title: String,
    pub channel: String,
    pub thumbnail: String,
}
impl YoutubeMetadata {
    fn valid(&self) -> bool {
        !self.title.trim().is_empty()
            && self.title.len() <= 1200
            && self.channel.len() <= 500
            && !self.channel.chars().any(char::is_control)
            && url::Url::parse(&self.thumbnail).is_ok_and(|u| {
                u.scheme() == "https"
                    && u.host_str() == Some("i.ytimg.com")
                    && u.username().is_empty()
                    && u.password().is_none()
                    && self.thumbnail.len() < 2048
            })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbedCommand {
    pub serial: u64,
    pub context: SelectionContext,
    pub operation_id: Option<String>,
    pub action: Action,
    pub target_ms: u64,
    pub generation: u64,
    pub expires_at_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YoutubeSnapshot {
    pub enabled: bool,
    pub chosen: bool,
    pub lease: Option<String>,
    pub command: Option<EmbedCommand>,
    pub metadata: Option<YoutubeMetadata>,
    pub error: Option<u16>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedAttachment {
    pub url: String,
    pub origin: String,
    pub lease: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbedEvent {
    pub lease: String,
    pub seq: u64,
    pub video_id: Option<String>,
    pub player: Observation,
    pub playable: bool,
    pub metadata: Option<YoutubeMetadata>,
    pub error: Option<u16>,
    pub result: Option<EmbedResult>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbedResult {
    pub serial: u64,
    pub outcome: Outcome,
}

impl AppRuntime {
    fn invalidate_youtube_content(state: &mut AppRuntimeState) {
        let pause_peer = Self::is_host_role(state)
            && (matches!(
                state.room_state,
                RoomState::Playing | RoomState::Seeking | RoomState::Pausing
            ) || state.pending_operation_id.is_some()
                || state.provider_verifying.is_some());
        Self::invalidate_provider_content(state);
        if pause_peer {
            let position_ms = state.sync.position_ms;
            Self::send_host_event(
                state,
                QuicServerEvent::BufferLow {
                    position_ms,
                    buffer_ahead_ms: 0,
                },
            );
        }
    }
    pub(super) fn youtube_snapshot(state: &AppRuntimeState) -> Option<YoutubeSnapshot> {
        Self::selected_provider(state).filter(|s| s.provider == "youtube")?;
        Some(YoutubeSnapshot {
            enabled: state.youtube.enabled && !state.youtube.handoff,
            chosen: state.youtube.chosen,
            lease: state.youtube.lease.clone(),
            command: state.youtube.command.clone(),
            metadata: state.youtube.metadata.clone(),
            error: state.youtube.error,
        })
    }
    pub fn youtube_driver(&self, enabled: bool) -> Result<AppSnapshot, String> {
        let (selection, chrome, generation) = {
            let mut state = self.lock();
            let selection = Self::selected_provider(&state)
                .filter(|s| s.provider == "youtube")
                .ok_or("Choose a YouTube video first")?;
            Self::invalidate_youtube_content(&mut state);
            state.youtube.enabled = enabled;
            state.youtube.handoff = true;
            state.youtube.chosen = true;
            state.youtube.lease = None;
            state.youtube.command = None;
            state.youtube.last_seen = None;
            state.youtube.error = None;
            state.chrome_generation = state.chrome_generation.wrapping_add(1);
            state.provider.readiness = crate::providers::sync::ProviderReadiness::Navigating;
            state.provider.state = "OPENING · Preparing the selected YouTube player".into();
            state.provider_chooser.selected = None;
            state.provider_open_attempt = if enabled {
                None
            } else {
                Some((selection.state_epoch.clone(), selection.host_sequence))
            };
            if !Self::is_host_role(&state) {
                state
                    .guest_provider
                    .observe(ContentLoadingState::Opening, None)
                    .ok();
            }
            (
                selection,
                state.chrome_session.take(),
                state.chrome_generation,
            )
        };
        // Intentional handoff, limited to the tracked Movie Party process tree.
        if let Some(mut chrome) = chrome {
            chrome.close_gracefully();
        }
        if !crate::providers::chrome::wait_for_session_shutdown(Duration::from_secs(8)) {
            self.youtube_handoff_failed(generation);
            return Err(
                "The managed browser is still stopping. Retry before opening another player."
                    .into(),
            );
        }
        if !enabled {
            let launched = self
                .inner
                .room_browser_launcher
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
                .ok_or_else(|| "Managed Chrome launcher is unavailable".to_string())
                .and_then(|launcher| launcher("youtube", &selection.canonical_url));
            let session = match launched {
                Ok(session) => session,
                Err(error) => {
                    self.youtube_handoff_failed(generation);
                    return Err(error);
                }
            };
            let mut state = self.lock();
            if state.chrome_generation != generation
                || Self::selected_provider(&state).as_ref() != Some(&selection)
            {
                drop(state);
                drop(session);
                return Err("The room selection changed during fallback".into());
            }
            state.chrome_session = Some(session);
        }
        let mut state = self.lock();
        if state.chrome_generation != generation
            || Self::selected_provider(&state).as_ref() != Some(&selection)
        {
            return Err("The selected content changed".into());
        }
        state.youtube.handoff = false;
        state.error = None;
        sync_room_snapshot(&mut state);
        drop(state);
        let snapshot = self.snapshot();
        self.inner.emit(snapshot.clone());
        Ok(snapshot)
    }
    fn youtube_handoff_failed(&self, generation: u64) {
        let mut state = self.lock();
        if state.chrome_generation != generation {
            return;
        }
        state.youtube.enabled = false;
        state.youtube.handoff = false;
        state.provider.readiness = crate::providers::sync::ProviderReadiness::Error;
        state.error=Some("Could not prepare the YouTube player. Retry in-app playback or managed Chrome; stored sign-ins are preserved.".into());
        drop(state);
        self.inner.emit(self.snapshot());
    }
    pub fn youtube_attach(&self) -> Result<EmbedAttachment, String> {
        let mut state = self.lock();
        let selection = Self::selected_provider(&state)
            .filter(|s| s.provider == "youtube")
            .ok_or("No selected YouTube content")?;
        if !state.youtube.enabled || state.youtube.handoff {
            return Err("The in-app player is not active".into());
        }
        if state.youtube.server.is_none() {
            state.youtube.server = Some(Arc::new(EmbedServer::start()?));
        }
        Self::invalidate_youtube_content(&mut state);
        state.provider.readiness = crate::providers::sync::ProviderReadiness::Navigating;
        if !Self::is_host_role(&state) {
            state
                .guest_provider
                .observe(ContentLoadingState::Opening, None)
                .ok();
        }
        let lease = Uuid::now_v7().to_string();
        state.youtube.lease = Some(lease.clone());
        state.youtube.seq = 0;
        state.youtube.last_seen = Some(Instant::now());
        state.youtube.command = None;
        state.youtube.metadata = None;
        state.youtube.error = None;
        let server = state.youtube.server.as_ref().unwrap();
        let mut url = url::Url::parse(&server.page).map_err(|_| "Invalid local player origin")?;
        url.set_fragment(Some(&format!(
            "video={}&lease={}",
            selection.content_id, lease
        )));
        Ok(EmbedAttachment {
            url: url.to_string(),
            origin: server.origin.clone(),
            lease,
        })
    }
    pub fn youtube_detach(&self, lease: &str) -> Result<(), String> {
        let mut state = self.lock();
        if state.youtube.lease.as_deref() != Some(lease) {
            return Ok(());
        }
        state.youtube.lease = None;
        state.youtube.last_seen = None;
        state.youtube.command = None;
        Self::invalidate_youtube_content(&mut state);
        state.provider.readiness = crate::providers::sync::ProviderReadiness::Unavailable;
        if !Self::is_host_role(&state) {
            state
                .guest_provider
                .observe(ContentLoadingState::Opening, None)
                .ok();
        }
        Ok(())
    }
    pub(super) fn queue_youtube_command(
        &self,
        action: Action,
        target_ms: u64,
        operation_id: Option<String>,
    ) -> bool {
        let mut state = self.lock();
        if !state.youtube.enabled {
            return false;
        }
        let Some(selection) = Self::selected_provider(&state) else {
            return false;
        };
        if operation_id.is_none()
            && state
                .youtube
                .command
                .as_ref()
                .is_some_and(|c| c.action == Action::Pause && c.operation_id.is_none())
        {
            return true;
        }
        state.youtube.serial = state.youtube.serial.saturating_add(1);
        state.youtube.command = Some(EmbedCommand {
            serial: state.youtube.serial,
            context: selection.context(),
            operation_id,
            action,
            target_ms,
            generation: state.session_generation,
            expires_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
                + 500,
        });
        drop(state);
        self.inner.emit(self.snapshot());
        true
    }
    pub(super) fn check_youtube_health(&self) {
        let expired = {
            let state = self.lock();
            state.youtube.enabled
                && state
                    .youtube
                    .last_seen
                    .is_some_and(|t| t.elapsed() > Duration::from_secs(4))
        };
        if expired {
            self.lock().youtube.last_seen = None;
            self.provider_selection_error(
                "The in-app player stopped responding. Retry the player or use managed Chrome."
                    .into(),
            );
        }
    }
    pub fn youtube_event(
        &self,
        context: SelectionContext,
        event: EmbedEvent,
    ) -> Result<AppSnapshot, String> {
        let (selection, command) = {
            let mut state = self.lock();
            let selection = Self::selected_provider(&state).ok_or("No selected content")?;
            if !state.youtube.enabled
                || state.youtube.handoff
                || state.youtube.lease.as_deref() != Some(&event.lease)
                || selection.context() != context
                || event.seq <= state.youtube.seq
            {
                return Err("Stale YouTube player event".into());
            }
            state.youtube.seq = event.seq;
            state.youtube.last_seen = Some(Instant::now());
            if let Some(error) = event.error {
                if ![2, 5, 100, 101, 150, 153, 1000, 1001].contains(&error) {
                    return Err("Unknown YouTube error".into());
                }
                state.youtube.error = Some(error);
                state.provider_gesture_required = error == 1000;
                state.error=Some(if error==1000 {"Click the YouTube play button to enable playback, then retry Ready."}else{"YouTube could not play this embed. Use managed Chrome for the selected video if it is available there."}.into());
                state.provider.readiness = crate::providers::sync::ProviderReadiness::Error;
                Self::invalidate_youtube_content(&mut state);
                state.room_state = RoomState::Buffering;
                if !Self::is_host_role(&state) {
                    state
                        .guest_provider
                        .observe(ContentLoadingState::Error, None)
                        .ok();
                }
                return Ok(snapshot_from_state(&state));
            }
            if event.video_id.as_deref() != Some(&selection.content_id)
                || !selection.evidence.matches(
                    &ContentEvidence {
                        title: selection.title.clone(),
                        duration_ms: event.player.duration_ms,
                        player_content_id: event.video_id.clone(),
                    },
                    "youtube",
                    &selection.content_id,
                )
            {
                Self::invalidate_youtube_content(&mut state);
                state.provider.readiness = crate::providers::sync::ProviderReadiness::Navigating;
                if !Self::is_host_role(&state) {
                    state
                        .guest_provider
                        .observe(ContentLoadingState::Opening, None)
                        .ok();
                }
                return Err("The IFrame player has not confirmed the selected video yet".into());
            }
            if let Some(metadata) = event.metadata.clone().filter(YoutubeMetadata::valid) {
                state.youtube.metadata = Some(metadata);
            }
            if event.playable && event.player.ready() {
                state.youtube.error = None;
            }
            (selection, state.youtube.command.clone())
        };
        let identity =
            crate::providers::content::parse_content("youtube", &selection.canonical_url)?;
        let tab = crate::providers::content::ContentTab {
            player: Some(event.player.clone()),
            selection_key: "iframe".into(),
            target_id: "youtube-iframe".into(),
            title: selection.title.clone(),
            identity: Some(identity),
            thumbnail_url: None,
            document_id: event.lease.clone(),
            focused: true,
            visible: true,
            playback_ready: event.playable && event.player.ready(),
            evidence: Some(selection.evidence.clone()),
            issue: None,
        };
        let healthy = self.observe_provider_health(std::slice::from_ref(&tab));
        if healthy {
            if Self::is_host_role(&self.lock()) {
                let mut state = self.lock();
                state.provider.readiness = if tab.playback_ready {
                    crate::providers::sync::ProviderReadiness::PlaybackReady
                } else {
                    crate::providers::sync::ProviderReadiness::Navigating
                };
                state.provider.state=if tab.playback_ready {"PLAYBACK_READY · YouTube video confirmed"}else{"CONTENT_CONFIRMED · Click play on YouTube once to prepare, then pause and mark Ready"}.into();
            } else {
                self.apply_guest_provider_observation(vec![tab]);
            }
        }
        if let (Some(result), Some(command)) = (event.result, command) {
            if result.serial == command.serial
                && command.context == selection.context()
                && command.generation == self.lock().session_generation
            {
                self.lock().youtube.command = None;
                if command.operation_id.is_none()
                    && (result.outcome != Outcome::Applied || !event.player.paused)
                {
                    let mut state = self.lock();
                    state.error=Some("Movie Party could not confirm a pause in YouTube. Pause the video manually before continuing.".into());
                    state.provider.readiness = crate::providers::sync::ProviderReadiness::Error;
                }
                if let Some(id) = command.operation_id {
                    let outcome = if result.outcome == Outcome::Applied
                        && !event.player.verifies(command.action, command.target_ms)
                    {
                        Outcome::Rejected
                    } else {
                        result.outcome
                    };
                    self.complete_provider_report(
                        &selection,
                        command.generation,
                        Report {
                            context: selection.context(),
                            operation_id: id,
                            action: command.action,
                            outcome,
                            position_ms: event.player.position_ms,
                        },
                    )?;
                }
            }
        }
        let snapshot = self.snapshot();
        self.inner.emit(snapshot.clone());
        Ok(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn host() -> AppRuntime {
        let runtime = AppRuntime::new();
        let credentials = RoomCredentials::generate();
        {
            let mut s = runtime.lock();
            s.credentials = Some(credentials.clone());
            s.provider.mode = "PROVIDER_SYNC".into();
            s.provider.provider_id = Some("youtube".into());
            s.youtube.enabled = true;
            s.youtube.chosen = true;
            s.provider_room
                .lock()
                .unwrap()
                .publish(
                    &credentials.room_id,
                    &crate::providers::content::parse_content(
                        "youtube",
                        "https://www.youtube.com/watch?v=abcdefghijk",
                    )
                    .unwrap(),
                    ContentEvidence {
                        title: "Fixture".into(),
                        duration_ms: 90000,
                        player_content_id: Some("abcdefghijk".into()),
                    },
                )
                .unwrap();
        }
        runtime
    }
    fn event(lease: &str, seq: u64) -> EmbedEvent {
        EmbedEvent {
            lease: lease.into(),
            seq,
            video_id: Some("abcdefghijk".into()),
            player: Observation {
                position_ms: 1000,
                duration_ms: 90000,
                buffer_ms: 30000,
                ready_state: 4,
                paused: true,
                ended: false,
                seeking: false,
            },
            playable: true,
            metadata: None,
            error: None,
            result: None,
        }
    }
    fn context(r: &AppRuntime) -> SelectionContext {
        AppRuntime::selected_provider(&r.lock()).unwrap().context()
    }
    #[test]
    fn disabled_player_and_local_room_cannot_attach() {
        let r = AppRuntime::new();
        assert!(r.youtube_attach().is_err());
        assert!(r.youtube_driver(true).is_err());
        let r = host();
        r.lock().youtube.enabled = false;
        assert!(r.youtube_attach().is_err());
    }
    #[test]
    fn replacement_lease_rejects_old_events_and_detach() {
        let r = host();
        let first = r.youtube_attach().unwrap();
        let second = r.youtube_attach().unwrap();
        assert_ne!(first.lease, second.lease);
        r.youtube_detach(&first.lease).unwrap();
        assert_eq!(
            r.lock().youtube.lease.as_deref(),
            Some(second.lease.as_str())
        );
        assert!(r
            .youtube_event(context(&r), event(&first.lease, 1))
            .is_err());
        r.youtube_detach(&second.lease).unwrap();
        assert!(r.lock().youtube.lease.is_none());
    }
    #[test]
    fn content_identity_epoch_and_sequence_are_required() {
        let r = host();
        let a = r.youtube_attach().unwrap();
        let c = context(&r);
        let mut wrong = event(&a.lease, 1);
        wrong.video_id = Some("wrongwrong1".into());
        assert!(r.youtube_event(c.clone(), wrong).is_err());
        let mut stale = c.clone();
        stale.content_revision += 1;
        assert!(r.youtube_event(stale, event(&a.lease, 2)).is_err());
        assert!(r.youtube_event(c.clone(), event(&a.lease, 2)).is_ok());
        assert!(r.youtube_event(c, event(&a.lease, 2)).is_err());
    }
    #[test]
    fn homepage_or_cued_only_player_is_never_ready() {
        let r = host();
        let a = r.youtube_attach().unwrap();
        let mut e = event(&a.lease, 1);
        e.playable = false;
        e.player.ready_state = 2;
        let s = r.youtube_event(context(&r), e).unwrap();
        assert_ne!(
            s.provider.readiness,
            crate::providers::sync::ProviderReadiness::PlaybackReady
        );
    }
    #[test]
    fn embed_errors_are_explicit_and_retry_clears_them() {
        for code in [2, 5, 100, 101, 150, 153, 1000, 1001] {
            let r = host();
            let a = r.youtube_attach().unwrap();
            let mut e = event(&a.lease, 1);
            e.error = Some(code);
            r.youtube_event(context(&r), e).unwrap();
            assert_eq!(r.lock().youtube.error, Some(code));
            r.youtube_attach().unwrap();
            assert_eq!(r.lock().youtube.error, None);
        }
    }
    #[test]
    fn rejected_recovery_pause_is_visible() {
        let r = host();
        let a = r.youtube_attach().unwrap();
        assert!(r.queue_youtube_command(Action::Pause, 1000, None));
        let serial = r.lock().youtube.command.as_ref().unwrap().serial;
        let mut e = event(&a.lease, 1);
        e.result = Some(EmbedResult {
            serial,
            outcome: Outcome::Rejected,
        });
        r.youtube_event(context(&r), e).unwrap();
        assert!(r.lock().error.as_ref().unwrap().contains("pause"));
    }
    #[test]
    fn stale_result_cannot_clear_current_command() {
        let r = host();
        let a = r.youtube_attach().unwrap();
        r.queue_youtube_command(Action::Seek, 5000, None);
        let serial = r.lock().youtube.command.as_ref().unwrap().serial;
        let mut e = event(&a.lease, 1);
        e.result = Some(EmbedResult {
            serial: serial + 1,
            outcome: Outcome::Applied,
        });
        r.youtube_event(context(&r), e).unwrap();
        assert_eq!(r.lock().youtube.command.as_ref().unwrap().serial, serial);
    }
    #[test]
    fn missing_fallback_launcher_is_a_real_failure() {
        let r = host();
        assert!(r.youtube_driver(false).unwrap_err().contains("launcher"));
        assert!(!r.lock().youtube.enabled);
        assert!(r.lock().youtube.chosen);
        assert!(r.lock().youtube.lease.is_none());
    }
    #[test]
    fn metadata_rejects_remote_tracking_and_credentials() {
        let mut m = YoutubeMetadata {
            title: "Actual title".into(),
            channel: "Actual channel".into(),
            thumbnail: "https://i.ytimg.com/vi/abcdefghijk/hqdefault.jpg".into(),
        };
        assert!(m.valid());
        for bad in [
            "https://evil.test/image",
            "file:///private/image",
            "https://user:secret@i.ytimg.com/image",
        ] {
            m.thumbnail = bad.into();
            assert!(!m.valid());
        }
    }
    #[test]
    fn lost_player_heartbeat_invalidates_readiness() {
        let r = host();
        r.youtube_attach().unwrap();
        r.lock().youtube.last_seen = Some(Instant::now() - Duration::from_secs(5));
        r.check_youtube_health();
        assert_ne!(
            r.lock().provider.readiness,
            crate::providers::sync::ProviderReadiness::PlaybackReady
        );
    }
    #[test]
    fn two_peer_iframe_observations_confirm_same_revision_and_stale_events_cannot_restore_ready() {
        let host = host();
        let guest = AppRuntime::new();
        let descriptor = host.lock().provider_room.lock().unwrap().descriptor.clone();
        {
            let mut state = guest.lock();
            state.local_participant.role = "Guest".into();
            state.credentials = host.lock().credentials.clone();
        }
        guest
            .receive_provider_descriptor(descriptor.clone())
            .unwrap();
        {
            let mut state = guest.lock();
            state.youtube.enabled = true;
            state.youtube.chosen = true;
        }
        let h = host.youtube_attach().unwrap();
        let g = guest.youtube_attach().unwrap();
        host.youtube_event(context(&host), event(&h.lease, 1))
            .unwrap();
        guest
            .youtube_event(context(&guest), event(&g.lease, 1))
            .unwrap();
        guest
            .youtube_event(context(&guest), event(&g.lease, 2))
            .unwrap();
        assert_eq!(context(&host), context(&guest));
        let ack = guest.lock().guest_provider.current_ack().unwrap();
        host.lock()
            .provider_room
            .lock()
            .unwrap()
            .acknowledge(ack)
            .unwrap();
        assert!(AppRuntime::provider_peer_selection_ready(&host.lock()));
        let old = context(&guest);
        let mut newer = descriptor;
        let selection = newer.selection.as_mut().unwrap();
        selection.host_sequence += 1;
        selection.content_revision += 1;
        guest.receive_provider_descriptor(newer).unwrap();
        assert!(guest.youtube_event(old, event(&g.lease, 3)).is_err());
        assert_ne!(
            guest.lock().guest_provider.state,
            ContentLoadingState::PlaybackReady
        );
    }
    #[test]
    fn handoff_cannot_expose_or_attach_a_second_player() {
        let r = host();
        r.lock().youtube.handoff = true;
        assert!(!AppRuntime::youtube_snapshot(&r.lock()).unwrap().enabled);
        assert!(r.youtube_attach().is_err());
        let generation = r.lock().chrome_generation;
        r.youtube_handoff_failed(generation);
        assert!(!r.lock().youtube.enabled);
        assert!(!r.lock().youtube.handoff);
    }
    #[test]
    fn actual_iframe_eof_enters_ended_state() {
        let r = host();
        let a = r.youtube_attach().unwrap();
        let mut e = event(&a.lease, 1);
        e.player.ended = true;
        e.player.position_ms = 90000;
        e.playable = false;
        r.youtube_event(context(&r), e).unwrap();
        assert_eq!(r.lock().room_state, RoomState::Ended);
    }
    #[test]
    #[ignore = "opens a real visible managed Chrome window using a disposable profile"]
    fn real_managed_chrome_fallback_hands_back_to_single_iframe_driver() {
        assert!(
            std::env::var_os("MOVIE_PARTY_ALLOW_VISIBLE_CHROME").is_some(),
            "Explicit visible Chrome opt-in is required"
        );
        use crate::providers::chrome;
        let root =
            std::env::temp_dir().join(format!("movie-party-youtube-handoff-{}", Uuid::now_v7()));
        let profile = root.clone();
        let r = host();
        r.set_room_browser_launcher(Arc::new(move |provider, url| {
            let executable = chrome::find_chrome(&chrome::default_chrome_candidates())
                .map_err(|e| e.to_string())?;
            let plan = chrome::build_launch_plan(executable, &profile, provider, url)
                .map_err(|e| e.to_string())?;
            chrome::launch_managed_chrome(plan).map_err(|e| e.to_string())
        }));
        // The selected public ID is a test fixture; this checks actual process
        // ownership and handoff, not provider-content playback readiness.
        r.youtube_driver(false).unwrap();
        {
            let mut s = r.lock();
            assert!(s.chrome_session.as_mut().unwrap().is_alive());
            assert!(!s.youtube.enabled);
        }
        r.youtube_driver(true).unwrap();
        {
            let s = r.lock();
            assert!(s.chrome_session.is_none());
            assert!(s.youtube.enabled);
            assert!(!s.youtube.handoff);
        }
        assert!(chrome::wait_for_session_shutdown(Duration::from_secs(2)));
        r.youtube_attach().unwrap();
        drop(r);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn host_player_loss_notifies_peer_and_cancels_scheduled_start() {
        let r = host();
        let (tx, mut rx) = broadcast::channel(8);
        {
            let mut state = r.lock();
            state.host_event_tx = Some(Arc::new(tx));
            state.room_state = RoomState::Playing;
            state.pending_operation_id = Some("old-start".into());
            AppRuntime::invalidate_youtube_content(&mut state);
            assert!(state.pending_operation_id.is_none());
            assert!(state.sync.strict_sync_paused);
        }
        let envelope = rx.try_recv().unwrap();
        assert!(matches!(envelope.event, QuicServerEvent::BufferLow { .. }));
    }
}
