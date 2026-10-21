use super::*;
use crate::providers::{
    playback::{Action, Observation, Operation, Outcome, Phase, Report},
    selection::ProviderSelection,
};

impl AppRuntime {
    pub(super) async fn pause_provider_for_recovery(&self) {
        let context = {
            let mut state = self.lock();
            if (!state.sync.strict_sync_paused && state.room_state != RoomState::Ended)
                || Self::selected_provider(&state).is_none()
            {
                return;
            }
            state
                .chrome_session
                .take()
                .map(|session| (session, state.chrome_generation, state.session_generation))
        };
        let Some((session, chrome_generation, generation)) = context else {
            return;
        };
        let result = tokio::task::spawn_blocking(move || {
            let paused = (|| {
                let mut page = session.connect_page()?;
                let command =
                    crate::providers::generic::GenericProviderAdapter.pause(page.next_command_id());
                page.execute(&command)
                    .and_then(|r| crate::providers::sync::verify_action_result(&r))
            })();
            (session, paused.is_ok())
        })
        .await;
        if let Ok((session, paused)) = result {
            self.restore_room_browser(Some(session), chrome_generation);
            let mut state = self.lock();
            if state.session_generation == generation && !paused {
                state.error=Some("Movie Party could not confirm a pause in Chrome. Pause the provider video manually before continuing.".into());
                state.provider.readiness = crate::providers::sync::ProviderReadiness::Error;
            }
        }
    }
    pub(super) fn observe_provider_health(
        &self,
        tabs: &[crate::providers::content::ContentTab],
    ) -> bool {
        let mut state = self.lock();
        let Some(selection) = Self::selected_provider(&state) else {
            return true;
        };
        let matching: Vec<_> = tabs
            .iter()
            .filter(|t| {
                t.identity.as_ref().is_some_and(|i| {
                    i.provider_id == selection.provider && i.content_id == selection.content_id
                }) && t.evidence.as_ref().is_some_and(|e| {
                    selection
                        .evidence
                        .matches(e, &selection.provider, &selection.content_id)
                })
            })
            .collect();
        let tab = state
            .provider_chooser
            .selected
            .as_ref()
            .and_then(|selected| {
                matching
                    .iter()
                    .find(|tab| tab.target_id == selected.target_id)
                    .copied()
            })
            .or_else(|| {
                if matching.len() == 1 {
                    Some(matching[0])
                } else {
                    let focused: Vec<_> =
                        matching.iter().filter(|tab| tab.focused).copied().collect();
                    if focused.len() == 1 {
                        Some(focused[0])
                    } else {
                        None
                    }
                }
            });
        let Some(tab) = tab else {
            return true;
        };
        let Some(player) = &tab.player else {
            return true;
        };
        // Actual provider observations feed the existing playback snapshot and
        // canonical host-clock comparison; never advance a synthetic timer UI.
        state.sync.position_ms = player.position_ms;
        state.player_snapshot.position_ms = player.position_ms;
        state.player_snapshot.duration_ms = Some(player.duration_ms);
        state.player_snapshot.buffered_ahead_ms = Some(player.buffer_ms);
        state.player_snapshot.state = if player.ended {
            "COMPLETED"
        } else if !player.ready() {
            "BUFFERING"
        } else if player.paused {
            "PAUSED"
        } else {
            "PLAYING"
        }
        .into();
        state.local_participant.buffer_ahead_ms = player.buffer_ms;
        state.buffer.guest_buffer_ahead_ms = player.buffer_ms;
        if state.room_state == RoomState::Ended {
            return false;
        }
        if state.provider_gesture_required {
            let previous = if Self::is_host_role(&state) {
                state
                    .provider_room
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .descriptor
                    .playback
                    .as_ref()
                    .and_then(|op| op.host.as_ref())
                    .map(|r| r.position_ms)
            } else {
                state
                    .guest_provider
                    .playback_report
                    .as_ref()
                    .map(|r| r.position_ms)
            };
            if player.ready()
                && (!player.paused
                    || previous.is_some_and(|p| player.position_ms > p.saturating_add(100)))
            {
                state.provider_gesture_required = false;
                state.sync.strict_sync_paused = true;
                state.error = None;
            } else {
                return false;
            }
        }
        if player.ended {
            crate::providers::playback::diagnostic(
                Action::Pause,
                Outcome::Ended,
                selection.content_revision,
            );
            let host = Self::is_host_role(&state);
            Self::invalidate_provider_content(&mut state);
            state.provider.state = "ENDED · The provider reached the end of this video".into();
            state.provider.readiness = crate::providers::sync::ProviderReadiness::NotStarted;
            if host {
                state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .ended();
                state.room_state = RoomState::Ended;
                state.sync.strict_sync_paused = false;
                Self::send_host_event(
                    &mut state,
                    QuicServerEvent::RoomStateUpdate {
                        state: "ENDED".into(),
                        position_ms: player.position_ms,
                    },
                );
            } else {
                state
                    .guest_provider
                    .observe(
                        crate::providers::selection::ContentLoadingState::ContentConfirmed,
                        tab.evidence.clone(),
                    )
                    .ok();
                state.room_state = RoomState::Buffering;
                state.error = Some(
                    "Your provider reached the end. The host must confirm the room has finished."
                        .into(),
                );
            }
            sync_room_snapshot(&mut state);
            return false;
        }
        let out_of_sync = state.room_state == RoomState::Playing
            && (player.paused
                || ((Self::is_host_role(&state) || state.clock_calibrated)
                    && Self::projected_host_position_ms(&state)
                        .is_some_and(|target| target.abs_diff(player.position_ms) > 1500)));
        if (!player.ready() || out_of_sync)
            && (state.room_state == RoomState::Playing || state.provider_verifying.is_some())
        {
            crate::providers::playback::diagnostic(
                Action::Pause,
                Outcome::Buffering,
                selection.content_revision,
            );
            Self::invalidate_provider_content(&mut state);
            state.room_state = RoomState::Buffering;
            state.provider.state = if out_of_sync {
                "PAUSED · Provider playback diverged. Check both players, then retry Ready."
            } else {
                "BUFFERING · Waiting for the selected provider player"
            }
            .into();
            state.provider.readiness = crate::providers::sync::ProviderReadiness::Navigating;
            if Self::is_host_role(&state) {
                if let Some(op) = state
                    .provider_room
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .descriptor
                    .playback
                    .as_mut()
                {
                    if op.phase == Phase::Verifying {
                        op.phase = Phase::Failed;
                    }
                }
                let position = player.position_ms;
                Self::send_host_event(
                    &mut state,
                    QuicServerEvent::BufferLow {
                        position_ms: position,
                        buffer_ahead_ms: player.buffer_ms,
                    },
                );
            } else {
                state
                    .guest_provider
                    .observe(
                        crate::providers::selection::ContentLoadingState::ContentConfirmed,
                        tab.evidence.clone(),
                    )
                    .ok();
            }
            sync_room_snapshot(&mut state);
            return false;
        }
        true
    }
    pub(super) fn selected_provider(state: &AppRuntimeState) -> Option<ProviderSelection> {
        if Self::is_host_role(state) {
            state
                .provider_room
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .descriptor
                .selection
                .clone()
        } else {
            state.guest_provider.selection().cloned()
        }
    }
    pub(super) fn begin_provider_operation(state: &mut AppRuntimeState, event: &QuicServerEvent) {
        let Some(selection) = Self::selected_provider(state) else {
            return;
        };
        let (id, action, target, resume) = match event {
            QuicServerEvent::PlayCommit {
                operation_id,
                target_position_ms,
                ..
            } => (operation_id, Action::Play, *target_position_ms, true),
            QuicServerEvent::PauseCommit {
                operation_id,
                target_position_ms,
                ..
            } => (operation_id, Action::Pause, *target_position_ms, false),
            QuicServerEvent::SeekCommit {
                operation_id,
                target_position_ms,
                resume_after_seek,
                ..
            } => (
                operation_id,
                Action::Seek,
                *target_position_ms,
                *resume_after_seek,
            ),
            _ => return,
        };
        state.provider_prepare_started = None;
        let mut room = state
            .provider_room
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if room
            .descriptor
            .playback
            .as_ref()
            .is_some_and(|op| op.operation_id == *id)
        {
            return;
        }
        room.playback_sequence = room.playback_sequence.saturating_add(1);
        room.playback_started = Some(Instant::now());
        room.descriptor.playback = Some(Operation {
            context: selection.context(),
            operation_id: id.clone(),
            sequence: room.playback_sequence,
            action,
            target_ms: target,
            resume,
            phase: Phase::Verifying,
            host: None,
            guest: None,
        });
    }

    pub(super) fn guard_provider_playback(state: &mut AppRuntimeState) {
        if Self::selected_provider(state).is_none() || state.room_state != RoomState::Playing {
            return;
        }
        let operation = if Self::is_host_role(state) {
            state
                .provider_room
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .descriptor
                .playback
                .clone()
        } else {
            state
                .guest_provider
                .descriptor
                .as_ref()
                .and_then(|d| d.playback.clone())
        };
        let confirmed = operation.as_ref().is_some_and(|op| {
            op.phase == Phase::Applied
                && op.action == Action::Play
                && op.sequence == state.provider_decision_sequence
        });
        let local_ready = if Self::is_host_role(state) {
            Self::provider_peer_selection_ready(state)
        } else {
            state.guest_provider.state
                == crate::providers::selection::ContentLoadingState::PlaybackReady
                && state
                    .guest_provider
                    .playback_report
                    .as_ref()
                    .is_some_and(|r| {
                        operation.as_ref().is_some_and(|op| {
                            r.operation_id == op.operation_id && r.outcome == Outcome::Applied
                        })
                    })
        };
        if !confirmed || !local_ready {
            let failed = operation
                .as_ref()
                .is_some_and(|op| op.phase == Phase::Failed);
            state.room_state = if failed {
                RoomState::Buffering
            } else {
                RoomState::ReadyCheck
            };
            if failed {
                state.sync.strict_sync_paused = true;
            }
        }
    }

    pub(super) fn dispatch_selected_provider(&self, kind: &str, target: u64) {
        let Some(action) = Action::from_commit(kind) else {
            return;
        };
        let (selection, id, generation, chrome_generation, deadline) = {
            let state = self.lock();
            let Some(selection) = Self::selected_provider(&state) else {
                return;
            };
            let Some(id) = state.last_committed_operation_id.clone() else {
                return;
            };
            (
                selection,
                id,
                state.session_generation,
                state.chrome_generation,
                state.committed_playback.as_ref().map(|p| {
                    instant_for_host_mono(
                        p.execute_at_host_mono_us,
                        if Self::is_host_role(&state) {
                            0
                        } else {
                            state.clock_offset_to_host_us
                        },
                    )
                }),
            )
        };
        let runtime = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            // Other observers may temporarily own Chrome. Wait boundedly rather
            // than interpreting that borrow as a browser crash.
            let start = Instant::now();
            let session = loop {
                let mut state = runtime.lock();
                if state.session_generation != generation
                    || Self::selected_provider(&state).as_ref() != Some(&selection)
                {
                    return;
                }
                if let Some(session) = state.chrome_session.take() {
                    break Some(session);
                }
                drop(state);
                if start.elapsed() > Duration::from_secs(4) {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(40));
            };
            let (outcome, position) = if let Some(session) = session {
                let result = (|| {
                    let mut page = session.connect_page().map_err(|_| Outcome::Rejected)?;
                    let before = observe_selected(&mut page, &selection)?;
                    if action == Action::Play && !play_deadline_valid(deadline, Instant::now()) {
                        return Err(Outcome::TimedOut);
                    }
                    {
                        let state = runtime.lock();
                        if state.session_generation != generation
                            || Self::selected_provider(&state).as_ref() != Some(&selection)
                        {
                            return Err(Outcome::Rejected);
                        }
                    }
                    if action != Action::Pause && !before.ready() {
                        return Err(if before.ended {
                            Outcome::Ended
                        } else {
                            Outcome::Buffering
                        });
                    }
                    let response = page
                        .evaluate(&crate::providers::playback::action_expression(
                            action, target,
                        ))
                        .map_err(|_| Outcome::TimedOut)?;
                    match response["result"]["value"].as_str() {
                        Some("GESTURE_REQUIRED") => return Err(Outcome::GestureRequired),
                        Some("APPLIED") if response.get("exceptionDetails").is_none() => {}
                        _ => return Err(Outcome::Rejected),
                    }
                    let after = observe_selected(&mut page, &selection)?;
                    if !after.verifies(action, target) {
                        return Err(Outcome::Rejected);
                    }
                    if action == Action::Play {
                        std::thread::sleep(Duration::from_millis(180));
                        let advancing = observe_selected(&mut page, &selection)?;
                        if !advancing.verifies(action, target)
                            || advancing.position_ms <= after.position_ms
                        {
                            return Err(Outcome::Rejected);
                        }
                    }
                    Ok(after.position_ms)
                })();
                runtime.restore_room_browser(Some(session), chrome_generation);
                match result {
                    Ok(position) => (Outcome::Applied, position),
                    Err(outcome) => (outcome, target),
                }
            } else {
                (Outcome::TimedOut, target)
            };
            let mut state = runtime.lock();
            if state.session_generation != generation
                || Self::selected_provider(&state).as_ref() != Some(&selection)
                || state.last_committed_operation_id.as_deref() != Some(&id)
            {
                return;
            }
            let report = Report {
                context: selection.context(),
                operation_id: id,
                action,
                outcome,
                position_ms: position,
            };
            crate::providers::playback::diagnostic(action, outcome, selection.content_revision);
            if Self::is_host_role(&state) {
                let solo = state
                    .sync_coordinator
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .guest_is_abandoned();
                if let Some(op) = state
                    .provider_room
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .descriptor
                    .playback
                    .as_mut()
                {
                    op.record(report, true, solo);
                }
            } else {
                state.guest_provider.playback_report = Some(report);
                let content_state = if outcome == Outcome::Applied {
                    state.guest_provider.state
                } else {
                    crate::providers::selection::ContentLoadingState::Error
                };
                let observed = if outcome == Outcome::Applied {
                    state.guest_provider.observed.clone()
                } else {
                    None
                };
                state.guest_provider.observe(content_state, observed).ok();
            }
            if outcome != Outcome::Applied {
                state.provider_gesture_required = outcome == Outcome::GestureRequired;
                state.sync.strict_sync_paused = true;
                state.room_state = RoomState::Buffering;
                state.provider.readiness = crate::providers::sync::ProviderReadiness::Error;
                state.error=Some(if outcome==Outcome::GestureRequired {"Click to enable playback. In managed Chrome, click the video play button once, then pause it and retry Ready."}else{"Provider playback was not verified. Playback remains paused; reopen the selected title and retry Ready."}.into());
            }
            drop(state);
            runtime.settle_provider_playback();
            runtime.inner.emit(runtime.snapshot());
        });
    }

    pub(super) fn settle_provider_playback(&self) {
        let prepare_timeout = {
            let state = self.lock();
            Self::is_host_role(&state)
                && Self::selected_provider(&state).is_some()
                && state.pending_operation_id.is_some()
                && state.commit_scheduled_for.is_none()
                && state
                    .provider_prepare_started
                    .is_some_and(|t| t.elapsed() > Duration::from_secs(15))
        };
        if prepare_timeout {
            let position = {
                let mut state = self.lock();
                Self::invalidate_provider_content(&mut state);
                state.error = Some(
                    "Provider preparation timed out. Check both players and retry Ready.".into(),
                );
                state.sync.position_ms
            };
            crate::providers::playback::diagnostic(Action::Play, Outcome::TimedOut, 0);
            self.report_buffer_low(position, 0);
        }

        let operation = {
            let state = self.lock();
            if !Self::is_host_role(&state) {
                return;
            }
            let mut room = state
                .provider_room
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let peer_not_ready = room.guest_ack.as_ref().is_some_and(|ack| {
                ack.state != crate::providers::selection::ContentLoadingState::PlaybackReady
            });
            if peer_not_ready {
                if let Some(op) = room.descriptor.playback.as_mut() {
                    if op.phase == Phase::Verifying && op.action != Action::Pause {
                        op.phase = Phase::Failed;
                    }
                }
            }
            if room
                .playback_started
                .is_some_and(|t| t.elapsed() > Duration::from_secs(15))
            {
                if let Some(op) = room.descriptor.playback.as_mut() {
                    if op.phase == Phase::Verifying {
                        op.phase = Phase::Failed;
                        crate::providers::playback::diagnostic(
                            op.action,
                            Outcome::TimedOut,
                            op.context.content_revision,
                        );
                    }
                }
            }
            room.descriptor.playback.clone()
        };
        if let Some(op) = operation {
            self.apply_provider_decision(op);
        }
    }

    pub(super) fn apply_provider_decision(&self, operation: Operation) {
        let resume = {
            let mut state = self.lock();
            if state.room_state == RoomState::Ended {
                return;
            }
            if Self::selected_provider(&state).is_none_or(|s| s.context() != operation.context) {
                return;
            }
            if operation.sequence < state.provider_decision_sequence {
                return;
            }
            if !Self::is_host_role(&state)
                && operation.phase == Phase::Applied
                && (state.guest_provider.state
                    != crate::providers::selection::ContentLoadingState::PlaybackReady
                    || state
                        .guest_provider
                        .playback_report
                        .as_ref()
                        .is_none_or(|r| {
                            r.operation_id != operation.operation_id
                                || r.context != operation.context
                                || r.outcome != Outcome::Applied
                        }))
            {
                return;
            }
            if !Self::is_host_role(&state) {
                if let Some(descriptor) = state.guest_provider.descriptor.as_mut() {
                    if descriptor
                        .playback
                        .as_ref()
                        .is_some_and(|old| !operation.follows(old))
                    {
                        return;
                    }
                    descriptor.playback = Some(operation.clone());
                }
            }
            if operation.phase == Phase::Verifying
                || state.provider_decision_sequence == operation.sequence
            {
                return;
            }
            state.provider_decision_sequence = operation.sequence;
            state.provider_verifying = None;
            let applied = operation.phase == Phase::Applied;
            state.sync.strict_sync_paused = !applied || operation.action != Action::Play;
            state.room_state = if applied && operation.action == Action::Play {
                RoomState::Playing
            } else if applied {
                RoomState::Paused
            } else {
                RoomState::Buffering
            };
            if !applied {
                state.committed_playback = None;
                state.error=Some("A provider could not verify the operation. Both players are paused; check Chrome and retry Ready.".into());
            } else {
                state.sync.position_ms = operation.target_ms;
                state.error = None;
            }
            state
                .sync_coordinator
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .room_state = state.room_state;
            sync_room_snapshot(&mut state);
            applied
                && operation.action == Action::Seek
                && operation.resume
                && Self::is_host_role(&state)
        };
        if resume {
            self.host_play();
        }
    }

    pub fn focus_provider_playback(&self) -> Result<AppSnapshot, String> {
        if !self.lock().provider_gesture_required {
            return Err("Playback does not currently require a Chrome gesture.".into());
        }
        let (session, generation) = {
            let mut state = self.lock();
            (
                state
                    .chrome_session
                    .take()
                    .ok_or("Chrome is busy. Try again shortly.")?,
                state.chrome_generation,
            )
        };
        let result: Result<(), &str> = (|| {
            let mut page = session
                .connect_page()
                .map_err(|_| "Open the selected provider title in Chrome.")?;
            let id = page.next_command_id();
            page.execute(&crate::providers::chrome::CdpCommand {
                id,
                method: "Page.bringToFront",
                params: serde_json::json!({}),
            })
            .map_err(|_| "Could not focus Chrome.")?;
            Ok(())
        })();
        self.restore_room_browser(Some(session), generation);
        result?;
        Ok(self.snapshot())
    }
}

fn observe_selected(
    page: &mut crate::providers::chrome::CdpPageSession,
    selection: &ProviderSelection,
) -> Result<Observation, Outcome> {
    let response = page
        .evaluate(crate::providers::chrome::content::OBSERVE_PAGE)
        .map_err(|_| Outcome::TimedOut)?;
    let v = &response["result"]["value"];
    let identity = crate::providers::content::parse_content(
        &selection.provider,
        v["url"].as_str().ok_or(Outcome::Rejected)?,
    )
    .map_err(|_| Outcome::Rejected)?;
    let evidence = crate::providers::selection::ContentEvidence {
        title: v["title"].as_str().unwrap_or("").trim().into(),
        duration_ms: v["durationMs"].as_u64().unwrap_or(0),
        player_content_id: v["playerContentId"].as_str().map(str::to_owned),
    };
    if identity.content_id != selection.content_id
        || !selection
            .evidence
            .matches(&evidence, &selection.provider, &selection.content_id)
    {
        return Err(Outcome::Rejected);
    }
    serde_json::from_value(v["player"].clone()).map_err(|_| Outcome::Rejected)
}

fn play_deadline_valid(deadline: Option<Instant>, now: Instant) -> bool {
    deadline.is_some_and(|deadline| {
        now.saturating_duration_since(deadline) <= Duration::from_millis(500)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{
        content::parse_content,
        selection::{ContentEvidence, ContentLoadingState, GuestSelection, SelectionAck},
    };
    fn host() -> AppRuntime {
        let runtime = AppRuntime::new();
        let credentials = RoomCredentials::generate();
        {
            let mut state = runtime.lock();
            state.credentials = Some(credentials.clone());
            state.provider.mode = "PROVIDER_SYNC".into();
            state.provider.provider_id = Some("youtube".into());
            state.provider.readiness = crate::providers::sync::ProviderReadiness::PlaybackReady;
            state
                .provider_room
                .lock()
                .unwrap()
                .publish(
                    &credentials.room_id,
                    &parse_content("youtube", "https://www.youtube.com/watch?v=abcdefghijk")
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
    fn begin(runtime: &AppRuntime, action: Action) -> Operation {
        let id = Uuid::now_v7().to_string();
        let event = match action {
            Action::Play => QuicServerEvent::PlayCommit {
                operation_id: id.clone(),
                target_position_ms: 1000,
                execute_at_host_mono_us: 0,
                presentation_epoch: 0,
            },
            Action::Pause => QuicServerEvent::PauseCommit {
                operation_id: id.clone(),
                target_position_ms: 1000,
                execute_at_host_mono_us: 0,
            },
            Action::Seek => QuicServerEvent::SeekCommit {
                operation_id: id.clone(),
                target_position_ms: 1000,
                execute_at_host_mono_us: 0,
                resume_after_seek: false,
            },
        };
        let mut state = runtime.lock();
        AppRuntime::begin_provider_operation(&mut state, &event);
        state.provider_verifying = Some(id);
        state.room_state = RoomState::Playing;
        sync_room_snapshot(&mut state);
        let op = state
            .provider_room
            .lock()
            .unwrap()
            .descriptor
            .playback
            .clone()
            .unwrap();
        op
    }
    fn report(op: &Operation, outcome: Outcome) -> Report {
        Report {
            context: op.context.clone(),
            operation_id: op.operation_id.clone(),
            action: op.action,
            outcome,
            position_ms: 1000,
        }
    }
    fn acknowledge(runtime: &AppRuntime, op: &Operation, outcome: Outcome) {
        let mut state = runtime.lock();
        let mut room = state.provider_room.lock().unwrap();
        let selection = room.descriptor.selection.clone().unwrap();
        let mut ack = SelectionAck::for_selection(
            &selection,
            1,
            ContentLoadingState::PlaybackReady,
            Some(selection.evidence.clone()),
        );
        ack.playback = Some(report(op, outcome));
        room.acknowledge(ack).unwrap();
        drop(room);
        state.local_participant.media_ready = true;
    }
    #[test]
    fn room_never_claims_playing_until_both_players_observe_success() {
        let runtime = host();
        let op = begin(&runtime, Action::Play);
        assert_eq!(runtime.snapshot().room.state, "READYCHECK");
        runtime
            .lock()
            .provider_room
            .lock()
            .unwrap()
            .descriptor
            .playback
            .as_mut()
            .unwrap()
            .record(report(&op, Outcome::Applied), true, false);
        runtime.settle_provider_playback();
        assert_ne!(runtime.snapshot().room.state, "PLAYING");
        acknowledge(&runtime, &op, Outcome::Applied);
        runtime.settle_provider_playback();
        assert_eq!(runtime.snapshot().room.state, "PLAYING");
        let seq = runtime
            .lock()
            .provider_room
            .lock()
            .unwrap()
            .playback_sequence;
        runtime.host_play();
        assert_eq!(
            runtime
                .lock()
                .provider_room
                .lock()
                .unwrap()
                .playback_sequence,
            seq
        );
    }
    #[test]
    fn failed_seek_and_operation_timeout_pause_without_success() {
        for timeout in [false, true] {
            let runtime = host();
            let op = begin(&runtime, Action::Seek);
            if timeout {
                runtime
                    .lock()
                    .provider_room
                    .lock()
                    .unwrap()
                    .playback_started = Some(Instant::now() - Duration::from_secs(16));
            } else {
                acknowledge(&runtime, &op, Outcome::Rejected);
            }
            runtime.settle_provider_playback();
            assert_eq!(runtime.snapshot().room.state, "BUFFERING");
            assert!(runtime.snapshot().sync.strict_sync_paused);
            assert!(runtime.snapshot().error.is_some());
        }
    }
    #[test]
    fn prepare_timeout_cancels_pending_start_without_waiting_for_a_commit() {
        let runtime = host();
        {
            let mut state = runtime.lock();
            state.pending_operation_id = Some(Uuid::now_v7().to_string());
            state.pending_operation_kind = Some("PLAY".into());
            state.provider_prepare_started = Some(Instant::now() - Duration::from_secs(16));
        }
        runtime.settle_provider_playback();
        assert!(runtime.lock().pending_operation_id.is_none());
        assert_eq!(runtime.snapshot().room.state, "BUFFERING");
        assert!(runtime.snapshot().error.unwrap().contains("timed out"));
    }
    #[test]
    fn late_browser_dispatch_cannot_claim_a_synchronized_start() {
        let deadline = Instant::now();
        assert!(play_deadline_valid(
            Some(deadline),
            deadline + Duration::from_millis(200)
        ));
        assert!(!play_deadline_valid(
            Some(deadline),
            deadline + Duration::from_millis(501)
        ));
        assert!(!play_deadline_valid(None, deadline));
    }
    #[test]
    fn observed_provider_position_updates_snapshot_and_clock_divergence_pauses() {
        let runtime = host();
        let mut t = tab(&runtime, true, false);
        t.player.as_mut().unwrap().position_ms = 12000;
        t.player.as_mut().unwrap().paused = false;
        {
            let mut state = runtime.lock();
            state.room_state = RoomState::Playing;
            state.committed_playback = Some(ScheduledPlayback {
                operation_id: Uuid::now_v7(),
                target_position_ms: 0,
                execute_at_host_mono_us: monotonic_us(),
            });
        }
        assert!(!runtime.observe_provider_health(&[t]));
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.player.position_ms, 12000);
        assert_eq!(snapshot.player.duration_ms, Some(90000));
        assert_eq!(snapshot.room.state, "BUFFERING");
        assert!(snapshot.sync.strict_sync_paused);
    }
    #[test]
    fn same_selection_is_idempotent_new_selection_retracts_operation() {
        let runtime = host();
        let op = begin(&runtime, Action::Play);
        let selection = AppRuntime::selected_provider(&runtime.lock()).unwrap();
        {
            let state = runtime.lock();
            let mut room = state.provider_room.lock().unwrap();
            let identity = parse_content("youtube", &selection.canonical_url).unwrap();
            room.publish(&selection.room_id, &identity, selection.evidence.clone())
                .unwrap();
            assert_eq!(
                room.descriptor.playback.as_ref().unwrap().operation_id,
                op.operation_id
            );
            let identity =
                parse_content("youtube", "https://www.youtube.com/watch?v=dQw4w9WgXcQ").unwrap();
            room.publish(
                &selection.room_id,
                &identity,
                ContentEvidence {
                    title: "Other".into(),
                    duration_ms: 90000,
                    player_content_id: Some("dQw4w9WgXcQ".into()),
                },
            )
            .unwrap();
            assert!(room.descriptor.playback.is_none());
            let mut stale = SelectionAck::for_selection(
                &selection,
                2,
                ContentLoadingState::PlaybackReady,
                Some(selection.evidence.clone()),
            );
            stale.playback = Some(report(&op, Outcome::Applied));
            assert!(room.acknowledge(stale).is_err());
        }
        runtime.apply_provider_decision(Operation {
            phase: Phase::Applied,
            ..op
        });
        assert_ne!(runtime.snapshot().room.state, "PLAYING");
    }
    #[test]
    fn late_join_or_reconnect_cannot_reuse_an_old_applied_result() {
        let host = host();
        let mut op = begin(&host, Action::Play);
        op.host = Some(report(&op, Outcome::Applied));
        op.guest = Some(report(&op, Outcome::Applied));
        op.phase = Phase::Applied;
        let guest = AppRuntime::new();
        let descriptor = host.lock().provider_room.lock().unwrap().descriptor.clone();
        {
            let mut state = guest.lock();
            state.credentials = host.lock().credentials.clone();
            state.local_participant.role = "Guest".into();
            state.room_state = RoomState::Lobby;
        }
        guest
            .receive_provider_descriptor(descriptor.clone())
            .unwrap();
        guest.apply_provider_decision(op.clone());
        assert_ne!(guest.snapshot().room.state, "PLAYING");
        {
            let mut state = guest.lock();
            let evidence = state.guest_provider.selection().unwrap().evidence.clone();
            state
                .guest_provider
                .observe(ContentLoadingState::PlaybackReady, Some(evidence))
                .unwrap();
            state.guest_provider.playback_report = op.guest.clone();
        }
        guest.apply_provider_decision(op.clone());
        assert_eq!(guest.snapshot().room.state, "PLAYING");
        {
            let mut state = guest.lock();
            state.guest_provider = GuestSelection::default();
            state.provider_decision_sequence = 0;
            state.room_state = RoomState::Reconnecting;
        }
        guest.receive_provider_descriptor(descriptor).unwrap();
        guest.apply_provider_decision(op);
        assert_ne!(guest.snapshot().room.state, "PLAYING");
    }
    fn tab(
        runtime: &AppRuntime,
        ready: bool,
        ended: bool,
    ) -> crate::providers::content::ContentTab {
        let selection = AppRuntime::selected_provider(&runtime.lock()).unwrap();
        crate::providers::content::ContentTab {
            selection_key: "fixture".into(),
            target_id: "fixture".into(),
            title: selection.title.clone(),
            identity: Some(parse_content("youtube", &selection.canonical_url).unwrap()),
            thumbnail_url: None,
            document_id: "fixture".into(),
            focused: true,
            visible: true,
            playback_ready: ready,
            evidence: Some(selection.evidence),
            issue: None,
            player: Some(Observation {
                position_ms: 1000,
                duration_ms: 90000,
                buffer_ms: if ready { 10000 } else { 0 },
                ready_state: if ready { 4 } else { 2 },
                paused: !ready,
                ended,
                seeking: false,
            }),
        }
    }
    #[test]
    fn buffering_cancels_pending_start_and_eof_is_not_recoverable_buffering() {
        let runtime = host();
        begin(&runtime, Action::Play);
        runtime.lock().pending_operation_id = Some("pending".into());
        let generation = runtime.lock().session_generation;
        assert!(!runtime.observe_provider_health(&[tab(&runtime, false, false)]));
        assert_eq!(runtime.snapshot().room.state, "BUFFERING");
        assert!(runtime.lock().pending_operation_id.is_none());
        assert_ne!(runtime.lock().session_generation, generation);
        assert!(!runtime.observe_provider_health(&[tab(&runtime, true, true)]));
        assert_eq!(runtime.snapshot().room.state, "ENDED");
        assert!(!runtime.snapshot().sync.strict_sync_paused);
        runtime.host_play();
        assert_eq!(runtime.snapshot().room.state, "ENDED");
    }
    #[test]
    fn gesture_recovery_requires_observed_user_playback_not_focus_success() {
        let runtime = host();
        let op = begin(&runtime, Action::Play);
        runtime
            .lock()
            .provider_room
            .lock()
            .unwrap()
            .descriptor
            .playback
            .as_mut()
            .unwrap()
            .host = Some(report(&op, Outcome::GestureRequired));
        runtime.lock().provider_gesture_required = true;
        let mut t = tab(&runtime, true, false);
        t.player.as_mut().unwrap().paused = true;
        assert!(!runtime.observe_provider_health(&[t.clone()]));
        assert!(!AppRuntime::local_media_genuinely_ready(&runtime.lock()));
        assert!(runtime.focus_provider_playback().is_err());
        assert!(runtime.lock().provider_gesture_required);
        t.player.as_mut().unwrap().paused = false;
        assert!(runtime.observe_provider_health(&[t]));
        assert!(!runtime.lock().provider_gesture_required);
        assert!(runtime.snapshot().sync.strict_sync_paused);
    }
}
