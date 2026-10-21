use super::*;
use crate::providers::{
    content::ContentTab,
    selection::{ContentLoadingState, ProviderRoomDescriptor, ProviderSelection},
};

pub type RoomBrowserLauncher = dyn Fn(&str, &str) -> Result<crate::providers::chrome::ManagedChromeSession, String>
    + Send
    + Sync;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderContentSnapshot {
    pub selection: Option<ProviderSelection>,
    pub local_state: ContentLoadingState,
    pub peer_state: ContentLoadingState,
}

impl AppRuntime {
    pub(super) fn provider_peer_selection_ready(state: &AppRuntimeState) -> bool {
        let authority = state
            .provider_room
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        authority.descriptor.selection.is_none()
            || authority.guest_ready()
            || state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .guest_is_abandoned()
    }

    pub fn set_room_browser_launcher(&self, launcher: Arc<RoomBrowserLauncher>) {
        *self
            .inner
            .room_browser_launcher
            .write()
            .unwrap_or_else(|e| e.into_inner()) = Some(launcher);
    }

    pub fn retry_room_provider(&self) -> Result<AppSnapshot, String> {
        let mut state = self.lock();
        if Self::is_host_role(&state) || state.guest_provider.selection().is_none() {
            return Err(
                "Only a guest with selected provider content can retry this browser.".into(),
            );
        }
        state.provider_open_attempt = None;
        state.provider_open_started = Some(Instant::now());
        state
            .guest_provider
            .observe(ContentLoadingState::Opening, None)?;
        state.local_participant.media_ready = false;
        state.error = None;
        state.provider.state =
            "OPENING · Reopening the host's content in your managed Chrome".into();
        Ok(snapshot_from_state(&state))
    }

    pub(super) fn receive_provider_descriptor(
        &self,
        descriptor: ProviderRoomDescriptor,
    ) -> Result<bool, String> {
        let mut state = self.lock();
        if Self::is_host_role(&state) {
            return Err("Guests cannot choose authoritative room content.".into());
        }
        let room = state
            .credentials
            .as_ref()
            .ok_or("No authenticated room")?
            .room_id
            .clone();
        let changed = match state.guest_provider.apply(descriptor, &room) {
            Err(error) if error == "PROVIDER_SELECTION_STALE" => return Ok(false),
            result => result?,
        };
        let mut retired_player = None;
        if changed {
            if let Some(selection) = state.guest_provider.selection().cloned() {
                state.provider.mode = "PROVIDER_SYNC".into();
                state.provider.provider_id = Some(selection.provider);
                state.provider.url = Some(selection.canonical_url);
                state.provider.readiness = crate::providers::sync::ProviderReadiness::Navigating;
                state.provider.state =
                    "OPENING · Opening the host's content in your managed Chrome".into();
                state.provider_open_attempt = None;
                state.provider_open_started = Some(Instant::now());
                Self::invalidate_provider_content(&mut state);
                if let Some(task) = state.player_event_task.take() {
                    task.abort();
                }
                if let Some(task) = state.player_render_task.take() {
                    task.abort();
                }
                retired_player = state.player.take();
                state.player_snapshot = PlayerSnapshot::default();
                if let Some(range) = state.range_server_handle.take() {
                    range.shutdown();
                }
                // Cancel file transfer when a Local Perfect lobby changes source.
                if let Some(task) = state.transfer_task.take() {
                    task.abort();
                }
                state.guest_cache = None;
                state.media = None;
                state.transfer = None;
                state.error = None;
            }
        }
        drop(state);
        if let Some(player) = retired_player {
            tauri::async_runtime::spawn_blocking(move || {
                player.lock().unwrap_or_else(|e| e.into_inner()).close();
            });
        }
        Ok(changed)
    }

    fn invalidate_provider_content(state: &mut AppRuntimeState) {
        // Cancel scheduled operations without invalidating this live browser.
        state.session_generation = state.session_generation.wrapping_add(1);
        state.pending_operation_id = None;
        state.pending_operation_kind = None;
        state.commit_scheduled_for = None;
        state.committed_playback = None;
        state.local_participant.media_ready = false;
        if let Some(peer) = &mut state.peer_participant {
            peer.media_ready = false;
        }
        state.sync.strict_sync_paused = true;
        let mut coord = state
            .sync_coordinator
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        coord.host_ready(ParticipantReadiness::not_ready(
            "provider selection changed",
        ));
        coord.guest_ready(ParticipantReadiness::not_ready(
            "provider selection changed",
        ));
    }

    pub(super) fn publish_provider_tab(&self, tab: ContentTab) -> Result<(), String> {
        let identity = tab.identity.as_ref().ok_or("Choose provider content")?;
        let evidence = tab
            .evidence
            .clone()
            .ok_or("Wait for identifiable video metadata")?;
        if tab.issue.is_some()
            || !tab.playback_ready
            || !evidence.valid_for(&identity.provider_id, &identity.content_id)
        {
            return Err(
                "Wait for the selected video's identity and playback to be verified in Chrome."
                    .into(),
            );
        }
        let mut state = self.lock();
        if !Self::is_host_role(&state) {
            return Err("Only the host selects content".into());
        }
        let room_id = state.credentials.as_ref().ok_or("No room")?.room_id.clone();
        let (selection, changed) = {
            let mut authority = state
                .provider_room
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let previous = authority.descriptor.selection.clone();
            let selection = authority.publish(&room_id, identity, evidence)?;
            let changed = previous.as_ref() != Some(&selection);
            (selection, changed)
        };
        if changed {
            Self::invalidate_provider_content(&mut state);
            state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .provider_context = Some(selection.context());
            state.provider.url = Some(selection.canonical_url.clone());
            state.provider.provider_id = Some(selection.provider.clone());
            state.provider.mode = "PROVIDER_SYNC".into();
            state.provider_chooser.selected = Some(tab.clone());
            state.provider_chooser.confirmed_key = Some(tab.selection_key.clone());
            state.provider_chooser.content_ready = true;
            Self::send_host_event(&mut state, QuicServerEvent::ProviderSelection { selection });
        }
        if state.provider.state.starts_with("ERROR ·") {
            state.error = None;
        }
        state.provider.readiness = crate::providers::sync::ProviderReadiness::PlaybackReady;
        state.provider.state =
            "PLAYBACK_READY · Selected content verified; waiting for both viewers".into();
        Ok(())
    }

    pub(super) fn provider_content_snapshot(
        state: &AppRuntimeState,
    ) -> Option<ProviderContentSnapshot> {
        if Self::is_host_role(state) {
            let authority = state
                .provider_room
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            authority.descriptor.selection.as_ref()?;
            Some(ProviderContentSnapshot {
                selection: authority.descriptor.selection.clone(),
                local_state: if state.provider.readiness
                    == crate::providers::sync::ProviderReadiness::PlaybackReady
                {
                    ContentLoadingState::PlaybackReady
                } else {
                    ContentLoadingState::Error
                },
                peer_state: authority.guest_ack.as_ref().map_or(
                    ContentLoadingState::Unselected,
                    |a| {
                        if a.state == ContentLoadingState::PlaybackReady && !authority.guest_ready()
                        {
                            ContentLoadingState::Error
                        } else {
                            a.state
                        }
                    },
                ),
            })
        } else {
            state.guest_provider.selection()?;
            Some(ProviderContentSnapshot {
                selection: state.guest_provider.selection().cloned(),
                local_state: state.guest_provider.state,
                peer_state: if !state.network.connected {
                    ContentLoadingState::Error
                } else if state
                    .peer_participant
                    .as_ref()
                    .is_some_and(|p| p.media_ready)
                {
                    ContentLoadingState::PlaybackReady
                } else {
                    ContentLoadingState::ContentConfirmed
                },
            })
        }
    }

    pub(super) fn spawn_provider_selection_worker(&self) {
        let (room_id, negotiated) = {
            let mut state = self.lock();
            if let Some(task) = state.provider_selection_task.take() {
                task.abort();
            }
            let Some(credentials) = &state.credentials else {
                return;
            };
            (
                credentials.room_id.clone(),
                Self::is_host_role(&state) || state.guest_provider.descriptor.is_some(),
            )
        };
        if !negotiated {
            return;
        } // An older Local Perfect host has no extension.
        let runtime = self.clone();
        let task = spawn_on_runtime(async move {
            loop {
                if runtime
                    .lock()
                    .credentials
                    .as_ref()
                    .is_none_or(|c| c.room_id != room_id)
                {
                    break;
                }
                let client = runtime.lock().client.clone();
                if let Some(client) = client {
                    let source_current = match tokio::time::timeout(
                        Duration::from_secs(5),
                        client.provider_selection(),
                    )
                    .await
                    {
                        Ok(Ok(descriptor)) => match runtime.receive_provider_descriptor(descriptor)
                        {
                            Ok(_) => true,
                            Err(error) => {
                                runtime.provider_selection_error(error);
                                false
                            }
                        },
                        _ => {
                            runtime.provider_selection_error("Provider selection timed out. Retrying the authenticated host; playback remains paused.".into());
                            false
                        }
                    };
                    if source_current {
                        runtime.open_guest_provider_if_needed(&room_id).await;
                        runtime.observe_room_provider(&room_id).await;
                    }
                    let ack = runtime.lock().guest_provider.current_ack();
                    if let Some(ack) = ack {
                        // Application acknowledgements are idempotent. On loss,
                        // resend the same acknowledgement on a new QUIC request.
                        let _ = tokio::time::timeout(
                            Duration::from_secs(5),
                            client.acknowledge_provider(ack),
                        )
                        .await;
                    }
                } else if AppRuntime::is_host_role(&runtime.lock()) {
                    runtime.observe_room_provider(&room_id).await;
                    let mut state = runtime.lock();
                    let peer_ready = Self::provider_peer_selection_ready(&state);
                    let pause_peer_loss = !peer_ready && state.room_state == RoomState::Playing;
                    if !peer_ready {
                        if let Some(peer) = &mut state.peer_participant {
                            peer.media_ready = false;
                        }
                    }
                    drop(state);
                    if pause_peer_loss {
                        runtime.host_pause();
                    }
                }
                runtime.inner.emit(runtime.snapshot());
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
        self.lock().provider_selection_task = Some(task);
    }

    pub(super) fn provider_selection_error(&self, error: String) {
        let mut state = self.lock();
        if Self::is_host_role(&state) {
            if state
                .provider_room
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .descriptor
                .selection
                .is_none()
            {
                return;
            }
            let first_failure =
                state.provider.readiness != crate::providers::sync::ProviderReadiness::Error;
            if first_failure {
                Self::invalidate_provider_content(&mut state);
            }
            state.provider.readiness = crate::providers::sync::ProviderReadiness::Error;
            state.provider.state =
                "ERROR · Host content could not be verified. Reopen the selected video in Chrome"
                    .into();
            state.error = Some(error);
            state.local_participant.media_ready = false;
            state.sync.strict_sync_paused = true;
            state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .host_ready(ParticipantReadiness::not_ready(
                    "provider identity unavailable",
                ));
            if first_failure {
                let position_ms = state.sync.position_ms;
                Self::send_host_event(
                    &mut state,
                    QuicServerEvent::BufferLow {
                        position_ms,
                        buffer_ahead_ms: 0,
                    },
                );
            }
            return;
        }
        if state.guest_provider.selection().is_none() {
            return;
        }
        if state.guest_provider.state != ContentLoadingState::Error {
            Self::invalidate_provider_content(&mut state);
        }
        let _ = state
            .guest_provider
            .observe(ContentLoadingState::Error, None);
        state.provider.readiness = crate::providers::sync::ProviderReadiness::Unavailable;
        state.provider.state =
            "ERROR · Open the host's selected content in managed Chrome, then retry".into();
        state.error = Some(error);
        state.local_participant.media_ready = false;
        state.sync.strict_sync_paused = true;
    }

    async fn open_guest_provider_if_needed(&self, room_id: &str) {
        let context = {
            let mut state = self.lock();
            let Some(selection) = state.guest_provider.selection().cloned() else {
                return;
            };
            if state.provider_open_attempt.as_ref()
                == Some(&(selection.state_epoch.clone(), selection.host_sequence))
            {
                return;
            }
            state.provider_open_attempt =
                Some((selection.state_epoch.clone(), selection.host_sequence));
            state.provider_open_started = Some(Instant::now());
            state
                .guest_provider
                .observe(ContentLoadingState::Opening, None)
                .ok();
            bump_chrome_generation(&mut state);
            (
                selection,
                state.chrome_session.take(),
                state.chrome_generation,
            )
        };
        let (selection, previous, generation) = context;
        let launcher = self
            .inner
            .room_browser_launcher
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let provider = selection.provider.clone();
        let url = selection.canonical_url.clone();
        let launched = tokio::task::spawn_blocking(move || {
            drop(previous); // Flush only our previous managed profile before reuse.
            launcher.ok_or_else(|| {
                "Managed Chrome launcher unavailable. Reopen Movie Party and retry.".to_string()
            })?(&provider, &url)
        })
        .await;
        let still_current = {
            let state = self.lock();
            state
                .credentials
                .as_ref()
                .is_some_and(|c| c.room_id == room_id)
                && state.guest_provider.selection() == Some(&selection)
                && state.chrome_generation == generation
        };
        if !still_current {
            drop(launched);
            return;
        }
        let session = match launched {
            Ok(Ok(session)) => session,
            Ok(Err(error)) => {
                self.provider_selection_error(error);
                return;
            }
            Err(_) => {
                self.provider_selection_error(
                    "Managed Chrome launch failed. Retry provider content.".into(),
                );
                return;
            }
        };
        let mut stale = Some(session);
        {
            let mut state = self.lock();
            if state
                .credentials
                .as_ref()
                .is_some_and(|c| c.room_id == room_id)
                && state.guest_provider.selection() == Some(&selection)
                && chrome_session_may_be_reinserted(&state, generation)
            {
                state.chrome_session = stale.take();
            }
        }
        drop(stale);
        self.spawn_chrome_crash_watcher();
        self.spawn_provider_watch_worker();
    }

    async fn observe_room_provider(&self, room_id: &str) {
        let failed_without_session = {
            let state = self.lock();
            state.chrome_session.is_none()
                && matches!(
                    state.provider.readiness,
                    crate::providers::sync::ProviderReadiness::Error
                        | crate::providers::sync::ProviderReadiness::Unavailable
                )
        };
        if failed_without_session {
            self.provider_selection_error(
                "Managed Chrome is unavailable. Reopen the selected content.".into(),
            );
            return;
        }
        let context = {
            let mut state = self.lock();
            let provider = if Self::is_host_role(&state) {
                state
                    .provider_room
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .descriptor
                    .selection
                    .as_ref()
                    .map(|s| s.provider.clone())
            } else {
                state.guest_provider.selection().map(|s| s.provider.clone())
            };
            let Some(provider) = provider else {
                return;
            };
            let Some(session) = state.chrome_session.take() else {
                return;
            };
            let expected_selection = if Self::is_host_role(&state) {
                state
                    .provider_room
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .descriptor
                    .selection
                    .clone()
            } else {
                state.guest_provider.selection().cloned()
            };
            (
                session,
                state.chrome_generation,
                provider,
                expected_selection,
            )
        };
        let (mut session, generation, provider, expected_selection) = context;
        let result = tokio::task::spawn_blocking(move || {
            let observed = if session.is_alive() {
                session.content_tabs(&provider).map_err(|e| e.to_string())
            } else {
                Err("Managed Chrome closed. Retry provider content.".into())
            };
            (session, observed)
        })
        .await;
        let Ok((session, observed)) = result else {
            self.provider_selection_error("Managed browser observation failed".into());
            return;
        };
        let mut session = Some(session);
        let current = {
            let state = self.lock();
            state
                .credentials
                .as_ref()
                .is_some_and(|c| c.room_id == room_id)
                && chrome_session_may_be_reinserted(&state, generation)
        };
        if !current {
            drop(session);
            return;
        }
        let selection_current = {
            let state = self.lock();
            let actual = if Self::is_host_role(&state) {
                state
                    .provider_room
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .descriptor
                    .selection
                    .clone()
            } else {
                state.guest_provider.selection().cloned()
            };
            actual == expected_selection
        };
        if !selection_current {
            self.restore_room_browser(session.take(), generation);
            return;
        }
        let tabs = match observed {
            Ok(tabs) => tabs,
            Err(error) => {
                self.restore_room_browser(session.take(), generation);
                self.provider_selection_error(error);
                return;
            }
        };
        let pause_targets: Vec<_> = tabs
            .iter()
            .filter(|t| t.identity.is_some() && t.evidence.is_some())
            .map(|t| t.target_id.clone())
            .collect();
        let host = Self::is_host_role(&self.lock());
        if host {
            let selected_target = self
                .lock()
                .provider_chooser
                .selected
                .as_ref()
                .map(|t| t.target_id.clone());
            let candidates: Vec<_> = tabs
                .into_iter()
                .filter(|t| {
                    if let Some(target) = &selected_target {
                        t.target_id == *target
                    } else {
                        t.identity.as_ref().is_some_and(|i| {
                            expected_selection.as_ref().is_some_and(|s| {
                                i.provider_id == s.provider && i.content_id == s.content_id
                            })
                        })
                    }
                })
                .collect();
            if let [tab] = candidates.as_slice() {
                let tab = tab.clone();
                if self.publish_provider_tab(tab.clone()).is_ok() {
                    session.as_ref().unwrap().pin_content_target(Some(&tab));
                } else {
                    self.provider_selection_error("The host's active video does not expose the selected content identity. Reopen a playable title in Chrome.".into());
                }
            } else {
                self.provider_selection_error("The host's selected provider tab closed. Reopen the selected content in Chrome.".into());
            }
        } else {
            let tab = self.apply_guest_provider_observation(tabs);
            if let Some(tab) = tab {
                session.as_ref().unwrap().pin_content_target(Some(&tab));
            }
        }
        if self.lock().sync.strict_sync_paused {
            let browser = session.take().unwrap();
            let provider = self
                .lock()
                .provider
                .provider_id
                .clone()
                .and_then(|p| crate::providers::sync::provider_id_from_str(&p));
            if let Some(provider) = provider {
                let paused = tokio::task::spawn_blocking(move || {
                    let mut failure = None;
                    for target in pause_targets {
                        let result = browser.connect_target(&target).and_then(|mut page| {
                            let command = crate::providers::sync::command_for_action(
                                provider,
                                page.next_command_id(),
                                crate::providers::sync::ProviderSyncAction::Pause,
                            );
                            page.execute(&command)
                                .and_then(|r| crate::providers::sync::verify_action_result(&r))
                        });
                        if let Err(error) = result {
                            failure = Some(format!(
                                "Could not pause managed provider playback: {error}"
                            ));
                        }
                    }
                    (browser, failure)
                })
                .await;
                match paused {
                    Ok((browser, failure)) => {
                        session = Some(browser);
                        self.restore_room_browser(session.take(), generation);
                        if let Some(error) = failure {
                            self.provider_selection_error(error);
                        }
                        return;
                    }
                    Err(_) => self.provider_selection_error("Managed browser pause failed".into()),
                }
            } else {
                session = Some(browser);
            }
        }
        self.restore_room_browser(session.take(), generation);
    }

    fn restore_room_browser(
        &self,
        session: Option<crate::providers::chrome::ManagedChromeSession>,
        generation: u64,
    ) {
        let mut stale = session;
        {
            let mut state = self.lock();
            if chrome_session_may_be_reinserted(&state, generation) {
                state.chrome_session = stale.take();
            }
        }
        drop(stale);
    }

    fn apply_guest_provider_observation(&self, tabs: Vec<ContentTab>) -> Option<ContentTab> {
        let mut state = self.lock();
        let selection = state.guest_provider.selection()?.clone();
        let was_ready = state.guest_provider.state == ContentLoadingState::PlaybackReady;
        let matching: Vec<_> = tabs
            .iter()
            .filter(|t| {
                t.identity.as_ref().is_some_and(|i| {
                    i.provider_id == selection.provider && i.content_id == selection.content_id
                }) && t.issue.is_none()
                    && t.evidence.as_ref().is_some_and(|e| {
                        selection
                            .evidence
                            .matches(e, &selection.provider, &selection.content_id)
                    })
            })
            .collect();
        let selected = if matching.len() == 1 {
            Some(matching[0])
        } else {
            let focused: Vec<_> = matching.into_iter().filter(|t| t.focused).collect();
            if focused.len() == 1 {
                Some(focused[0])
            } else {
                None
            }
        };
        if let Some(tab) = selected {
            let next = if tab.playback_ready
                && matches!(
                    state.guest_provider.state,
                    ContentLoadingState::ContentConfirmed | ContentLoadingState::PlaybackReady
                ) {
                ContentLoadingState::PlaybackReady
            } else {
                ContentLoadingState::ContentConfirmed
            };
            if was_ready && next != ContentLoadingState::PlaybackReady {
                Self::invalidate_provider_content(&mut state);
            }
            if state
                .guest_provider
                .observe(next, tab.evidence.clone())
                .is_ok()
            {
                state.provider.readiness = if next == ContentLoadingState::PlaybackReady {
                    crate::providers::sync::ProviderReadiness::PlaybackReady
                } else {
                    crate::providers::sync::ProviderReadiness::Navigating
                };
                state.provider.state = if next == ContentLoadingState::PlaybackReady {
                    "PLAYBACK_READY · Your player matches the host's selected content"
                } else {
                    "CONTENT_CONFIRMED · Title and player identity match; checking playback"
                }
                .into();
                state.error = None;
                return Some(tab.clone());
            }
        }
        let auth = tabs
            .iter()
            .any(|t| t.issue.as_ref().is_some_and(|s| s.contains("Sign in")));
        let timed_out = state
            .provider_open_started
            .is_some_and(|t| t.elapsed() > Duration::from_secs(45));
        let next = if auth {
            ContentLoadingState::AuthRequired
        } else if timed_out {
            ContentLoadingState::Error
        } else {
            ContentLoadingState::Opening
        };
        if was_ready {
            Self::invalidate_provider_content(&mut state);
        }
        state.guest_provider.observe(next, None).ok();
        state.provider.readiness = if auth {
            crate::providers::sync::ProviderReadiness::LoginRequired
        } else {
            crate::providers::sync::ProviderReadiness::Navigating
        };
        state.provider.state = match next {
            ContentLoadingState::AuthRequired => {
                "AUTH_REQUIRED · Sign in on the provider's Chrome page, then open the host's title"
            }
            ContentLoadingState::Error => {
                "ERROR · Content could not be verified. Open the host's title in Chrome or retry"
            }
            _ => "OPENING · Waiting for the host's actual video; a homepage is not ready",
        }
        .into();
        state.local_participant.media_ready = false;
        state.sync.strict_sync_paused = true;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{
        content::parse_content,
        selection::{ContentEvidence, ContentLoadingState, ProviderRoomAuthority},
    };
    fn tab(id: &str) -> ContentTab {
        let mut tab = ContentTab {
            target_id: "selected".into(),
            title: format!("Observed fixture {id}"),
            identity: Some(
                parse_content("youtube", &format!("https://www.youtube.com/watch?v={id}")).unwrap(),
            ),
            thumbnail_url: None,
            document_id: "fixture".into(),
            focused: true,
            visible: true,
            playback_ready: true,
            evidence: Some(ContentEvidence {
                title: format!("Observed fixture {id}"),
                duration_ms: 90_000,
                player_content_id: Some(id.into()),
            }),
            issue: None,
            selection_key: String::new(),
        };
        tab.selection_key = tab.selection_key();
        tab
    }
    fn descriptor(room: &str, id: &str) -> ProviderRoomDescriptor {
        let t = tab(id);
        let mut authority = ProviderRoomAuthority::default();
        authority
            .publish(room, t.identity.as_ref().unwrap(), t.evidence.unwrap())
            .unwrap();
        authority.descriptor
    }
    fn guest() -> AppRuntime {
        let runtime = AppRuntime::new();
        let credentials = RoomCredentials::generate();
        {
            let mut state = runtime.lock();
            state.local_participant.role = "Guest".into();
            state.credentials = Some(credentials.clone());
        }
        runtime
            .receive_provider_descriptor(descriptor(&credentials.room_id, "abcdefghijk"))
            .unwrap();
        runtime
    }
    #[test]
    fn local_perfect_never_requires_provider_ack_and_explicit_abandonment_is_preserved() {
        let runtime = AppRuntime::new();
        assert!(AppRuntime::provider_peer_selection_ready(&runtime.lock()));
        let d = descriptor("room", "abcdefghijk");
        runtime.lock().provider_room.lock().unwrap().descriptor = d;
        assert!(!AppRuntime::provider_peer_selection_ready(&runtime.lock()));
        runtime
            .lock()
            .sync_coordinator
            .lock()
            .unwrap()
            .abandon_guest();
        assert!(AppRuntime::provider_peer_selection_ready(&runtime.lock()));
    }

    #[tokio::test]
    async fn provider_monitor_does_not_pause_local_perfect_playback() {
        let runtime = AppRuntime::new();
        {
            let mut state = runtime.lock();
            state.credentials = Some(RoomCredentials::generate());
            state.room_state = RoomState::Playing;
        }
        runtime.spawn_provider_selection_worker();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(runtime.lock().room_state, RoomState::Playing);
        assert!(runtime.lock().pending_operation_id.is_none());
        runtime.leave_party();
    }

    #[test]
    fn guest_homepage_same_url_wrong_video_auth_and_timeout_never_become_ready() {
        let runtime = guest();
        assert_eq!(runtime.snapshot().provider.mode, "PROVIDER_SYNC");
        let mut wrong = tab("abcdefghijk");
        wrong.evidence.as_mut().unwrap().player_content_id = Some("dQw4w9WgXcQ".into());
        assert!(runtime
            .apply_guest_provider_observation(vec![wrong])
            .is_none());
        assert!(!AppRuntime::local_media_genuinely_ready(&runtime.lock()));
        let mut home = tab("abcdefghijk");
        home.identity = None;
        home.evidence = None;
        runtime.apply_guest_provider_observation(vec![home]);
        assert!(!runtime.snapshot().participants[0].media_ready);
        let mut login = tab("abcdefghijk");
        login.issue = Some("Sign in on the provider page".into());
        runtime.apply_guest_provider_observation(vec![login]);
        assert_eq!(
            runtime.lock().guest_provider.state,
            ContentLoadingState::AuthRequired
        );
        runtime.lock().provider_open_started = Some(Instant::now() - Duration::from_secs(46));
        runtime.apply_guest_provider_observation(vec![]);
        assert_eq!(
            runtime.lock().guest_provider.state,
            ContentLoadingState::Error
        );
        let snapshot = runtime.retry_room_provider().unwrap();
        assert_eq!(snapshot.provider.mode, "PROVIDER_SYNC");
        assert_eq!(
            runtime.lock().guest_provider.state,
            ContentLoadingState::Opening
        );
    }
    #[test]
    fn guest_confirms_identity_before_video_ready_and_buffer_loss_retracts_readiness() {
        let runtime = guest();
        let x = tab("abcdefghijk");
        runtime.apply_guest_provider_observation(vec![x.clone()]);
        assert_eq!(
            runtime.lock().guest_provider.state,
            ContentLoadingState::ContentConfirmed
        );
        assert!(!AppRuntime::local_media_genuinely_ready(&runtime.lock()));
        runtime.apply_guest_provider_observation(vec![x.clone()]);
        assert!(AppRuntime::local_media_genuinely_ready(&runtime.lock()));
        let generation = runtime.lock().session_generation;
        runtime.lock().pending_operation_id = Some("scheduled-provider-play".into());
        let mut buffered = x;
        buffered.playback_ready = false;
        runtime.apply_guest_provider_observation(vec![buffered]);
        assert!(!AppRuntime::local_media_genuinely_ready(&runtime.lock()));
        assert!(!runtime.snapshot().participants[0].media_ready);
        assert!(runtime.lock().pending_operation_id.is_none());
        assert_ne!(runtime.lock().session_generation, generation);
        runtime.apply_guest_provider_observation(vec![tab("abcdefghijk")]);
        let generation = runtime.lock().session_generation;
        runtime.lock().pending_operation_id = Some("scheduled-before-timeout".into());
        runtime.provider_selection_error("Host selection request timed out".into());
        assert!(runtime.lock().pending_operation_id.is_none());
        assert_ne!(runtime.lock().session_generation, generation);
    }
    #[test]
    fn stale_selection_preserves_newer_verified_runtime_state() {
        let runtime = guest();
        let old = runtime.lock().guest_provider.descriptor.clone().unwrap();
        let mut newer = old.clone();
        let selected = newer.selection.as_mut().unwrap();
        selected.host_sequence += 1;
        selected.content_revision += 1;
        runtime.receive_provider_descriptor(newer).unwrap();
        runtime.apply_guest_provider_observation(vec![tab("abcdefghijk")]);
        runtime.apply_guest_provider_observation(vec![tab("abcdefghijk")]);
        assert!(!runtime.receive_provider_descriptor(old).unwrap());
        assert_eq!(
            runtime.lock().guest_provider.state,
            ContentLoadingState::PlaybackReady
        );
    }

    #[test]
    fn duplicate_delivery_does_not_reopen_or_reset_confirmed_content() {
        let runtime = guest();
        let x = tab("abcdefghijk");
        runtime.apply_guest_provider_observation(vec![x.clone()]);
        runtime.apply_guest_provider_observation(vec![x]);
        let descriptor = runtime.lock().guest_provider.descriptor.clone().unwrap();
        runtime.lock().provider_open_attempt = Some((descriptor.state_epoch.clone(), 1));
        assert!(!runtime
            .receive_provider_descriptor(descriptor.clone())
            .unwrap());
        assert_eq!(
            runtime.lock().guest_provider.state,
            ContentLoadingState::PlaybackReady
        );
        assert_eq!(
            runtime.lock().provider_open_attempt,
            Some((descriptor.state_epoch, 1))
        );
    }
    #[test]
    fn host_authority_cannot_be_forged_by_guest_or_conflicting_room_event() {
        let runtime = guest();
        let current = runtime.lock().guest_provider.selection().cloned().unwrap();
        let mut forged = current.clone();
        forged.host_sequence += 1;
        forged.content_revision += 1;
        let event = QuicServerEvent::ProviderSelection { selection: forged };
        let envelope = EventEnvelope {
            v_major: 1,
            v_minor: 0,
            room_id: current.room_id,
            seq: 9999,
            sender: "guest-forged-host".into(),
            sent_mono_us: 0,
            event: event.clone(),
        };
        AppRuntime::apply_peer_event(&runtime.inner, &envelope, event);
        assert_eq!(
            runtime
                .lock()
                .guest_provider
                .selection()
                .unwrap()
                .host_sequence,
            1
        );
        assert!(runtime.publish_provider_tab(tab("dQw4w9WgXcQ")).is_err());
        let host = AppRuntime::new();
        assert!(host.retry_room_provider().is_err());
    }
    #[test]
    fn stale_provider_playback_cannot_set_a_new_pending_operation() {
        let runtime = guest();
        let selected = runtime.lock().guest_provider.selection().cloned().unwrap();
        let event = QuicServerEvent::PlayPrepare {
            operation_id: "old-op".into(),
            target_position_ms: 0,
            minimum_buffer_ms: 5000,
        };
        let envelope = EventEnvelope {
            v_major: 1,
            v_minor: 0,
            room_id: selected.room_id.clone(),
            seq: 1000,
            sender: "unauthorized".into(),
            sent_mono_us: 0,
            event: event.clone(),
        };
        AppRuntime::apply_peer_event(&runtime.inner, &envelope, event.clone());
        assert!(runtime.lock().pending_operation_id.is_none());
        let mut context = selected.context();
        context.host_sequence += 1;
        let wrapped = QuicServerEvent::ProviderPlayback {
            context,
            event: Box::new(event),
        };
        AppRuntime::apply_peer_event(&runtime.inner, &envelope, wrapped);
        assert!(runtime.lock().pending_operation_id.is_none());
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn actual_join_preserves_provider_sync_and_propagates_new_host_content() {
        std::env::set_var("MOVIE_PARTY_DEV_LOOPBACK", "1");
        let host = AppRuntime::new();
        let first = tab("dQw4w9WgXcQ");
        {
            let mut state = host.lock();
            state.provider.mode = "PROVIDER_SYNC".into();
            state.provider.provider_id = Some("youtube".into());
            state.provider.url = first.identity.as_ref().map(|i| i.allowed_url.clone());
            state.provider.readiness = crate::providers::sync::ProviderReadiness::PlaybackReady;
            state.provider_chooser.selected = Some(first);
        }
        let hosted = host.create_local_party(None, true).await.unwrap();
        let guest = AppRuntime::new();
        let joined = guest
            .join_party(hosted.room.invite_code.unwrap())
            .await
            .unwrap();
        // This is a simulated player-observation test over real QUIC; do not
        // open Chrome or claim a functional provider desktop test.
        if let Some(task) = guest.lock().provider_selection_task.take() {
            task.abort();
        }
        assert_eq!(joined.provider.mode, "PROVIDER_SYNC");
        assert_eq!(
            joined
                .provider_content
                .unwrap()
                .selection
                .unwrap()
                .content_id,
            "dQw4w9WgXcQ"
        );
        let old_context = guest.lock().guest_provider.selection().unwrap().context();
        host.publish_provider_tab(tab("abcdefghijk")).unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while guest.lock().guest_provider.selection().unwrap().content_id != "abcdefghijk" {
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // A delayed command from the authenticated host must still be rejected
        // when it belongs to the previous content revision.
        let stale_play = QuicServerEvent::ProviderPlayback {
            context: old_context,
            event: Box::new(QuicServerEvent::PlayPrepare {
                operation_id: "delayed-old-content".into(),
                target_position_ms: 0,
                minimum_buffer_ms: 5000,
            }),
        };
        let envelope = {
            let state = guest.lock();
            EventEnvelope {
                v_major: 1,
                v_minor: 0,
                room_id: state.credentials.as_ref().unwrap().room_id.clone(),
                seq: 999_999,
                sender: state.invite.as_ref().unwrap().host_device_id.clone(),
                sent_mono_us: 0,
                event: stale_play.clone(),
            }
        };
        AppRuntime::apply_peer_event(&guest.inner, &envelope, stale_play);
        assert!(guest.lock().pending_operation_id.is_none());
        guest.apply_guest_provider_observation(vec![tab("abcdefghijk")]);
        guest.apply_guest_provider_observation(vec![tab("abcdefghijk")]);
        let (client, ack) = {
            let state = guest.lock();
            (
                state.client.clone().unwrap(),
                state.guest_provider.current_ack().unwrap(),
            )
        };
        assert!(client.acknowledge_provider(ack).await.unwrap());
        assert!(host.lock().provider_room.lock().unwrap().guest_ready());
        assert_eq!(guest.snapshot().provider.mode, "PROVIDER_SYNC");
        guest.leave_party();
        host.leave_party();
        assert!(guest.lock().provider_selection_task.is_none());
    }
    #[tokio::test]
    async fn missing_launcher_errors_and_leave_clears_provider_state() {
        let runtime = guest();
        let room = runtime.lock().credentials.as_ref().unwrap().room_id.clone();
        runtime.open_guest_provider_if_needed(&room).await;
        assert_eq!(
            runtime.lock().guest_provider.state,
            ContentLoadingState::Error
        );
        assert!(runtime.snapshot().error.is_some());
        assert_eq!(runtime.snapshot().provider.mode, "PROVIDER_SYNC");
        runtime.leave_party();
        assert!(runtime.snapshot().provider_content.is_none());
    }
}
