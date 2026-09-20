use uuid::Uuid;

use super::{
    consensus::{all_participants_ready, ParticipantReadiness},
    state_machine::{transition, RoomEvent, RoomState, StateError},
};
use tokio::sync::broadcast;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LocalSyncError {
    #[error(transparent)]
    State(#[from] StateError),
    #[error("MP-SYNC-004 participants are not ready")]
    NotReady,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerRole {
    Host,
    Guest,
}

/// Why playback is being paused. Only a `BufferLow` pause may be resumed by
/// the strict-sync recovery path — a manual pause must never auto-resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseCause {
    Manual,
    BufferLow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledPlayback {
    pub operation_id: Uuid,
    pub target_position_ms: u64,
    pub execute_at_host_mono_us: u64,
}

#[derive(Clone, Debug)]
pub struct LocalSyncCoordinator {
    pub room_state: RoomState,
    pub host_position_ms: u64,
    pub guest_position_ms: u64,
    pub host_ready: ParticipantReadiness,
    pub guest_ready: ParticipantReadiness,
    pub paused_by_strict_sync: bool,
    /// §40 Continue Without Guest (host-only, user-initiated).
    pub guest_abandoned: bool,
    pub pending_scheduled: Option<ScheduledPlayback>,
    pub last_peer_seq_received: u64,
    /// Monotonic sequence counter for host-issued EventEnvelopes. Every
    /// coordinator broadcast AND every `send_host_event` envelope (PlayCommit,
    /// PauseCommit, SeekCommit, chat, ...) consumes one value, so the guest's
    /// stale/duplicate rejection (`seq <= last_peer_seq_received`) never drops
    /// a legitimately new host event.
    pub coordinator_event_seq: u64,
    /// Called after coordinator state changes so the runtime can
    /// broadcast CoordinatorStateUpdate to the peer. Signature:
    /// fn(&LocalSyncCoordinator, &str, &broadcast::Sender<EventEnvelope>)
    pub coordinator_tx: broadcast::Sender<crate::network::quic::EventEnvelope>,
    pub coordinator_local_id: String,
    /// Room whose envelopes this coordinator broadcasts (§10 room_id).
    /// Host setup assigns this from the room credentials; the broadcast fn
    /// pointer cannot capture state, so it reads the room from here.
    pub coordinator_room_id: String,
    pub coordinator_broadcast_fn: Option<
        fn(&LocalSyncCoordinator, &str, &broadcast::Sender<crate::network::quic::EventEnvelope>),
    >,
}

impl LocalSyncCoordinator {
    pub fn new() -> Self {
        Self {
            room_state: RoomState::Lobby,
            host_position_ms: 0,
            guest_position_ms: 0,
            host_ready: ParticipantReadiness::ready(0),
            guest_ready: ParticipantReadiness::ready(0),
            paused_by_strict_sync: false,
            guest_abandoned: false,
            pending_scheduled: None,
            last_peer_seq_received: 0,
            coordinator_event_seq: 0,
            coordinator_tx: {
                let (tx, _) = broadcast::channel(64);
                tx
            },
            coordinator_local_id: String::new(),
            coordinator_room_id: String::new(),
            coordinator_broadcast_fn: None,
        }
    }

    /// Fire coordinator state callback if room_state just changed.
    /// NOTE: fires without re-entrant locking — caller must not hold any
    /// other Mutex<Arc> on the same inner while calling this.
    fn fire_coordinator_cb(&mut self, new_state: RoomState) {
        if self.room_state != new_state {
            self.room_state = new_state;
            self.coordinator_event_seq = self.coordinator_event_seq.wrapping_add(1);
            if let Some(broadcast_fn) = self.coordinator_broadcast_fn {
                broadcast_fn(self, &self.coordinator_local_id, &self.coordinator_tx);
            }
        }
    }

    /// Overwrite local coordinator state from a host-issued
    /// `CoordinatorStateUpdate` event. Called side-effect only.
    /// Does NOT call `fire_coordinator_cb` (peer events call that
    /// separately via `apply_coordinator_state` in apply_peer_event).
    pub fn apply_coordinator_state(
        &mut self,
        host_ready: bool,
        guest_ready: bool,
        coordinator_play_state: &str,
    ) {
        self.host_ready = ParticipantReadiness::ready(if host_ready {
            self.host_ready.buffer_ahead_ms
        } else {
            0
        });
        self.guest_ready = ParticipantReadiness::ready(if guest_ready {
            self.guest_ready.buffer_ahead_ms
        } else {
            0
        });
        let trimmed = coordinator_play_state.trim();
        let parsed = match trimmed {
            "LOBBY" => RoomState::Lobby,
            "CREATED" => RoomState::Created,
            "WAITING_FOR_GUEST" => RoomState::WaitingForGuest,
            "PREPARING" => RoomState::Preparing,
            "READYCHECK" => RoomState::ReadyCheck,
            "PLAYING" => RoomState::Playing,
            "PAUSING" => RoomState::Pausing,
            "PAUSED" => RoomState::Paused,
            "SEEKING" => RoomState::Seeking,
            "BUFFERING" => RoomState::Buffering,
            "RECONNECTING" => RoomState::Reconnecting,
            "ENDED" => RoomState::Ended,
            "ERROR" => RoomState::Error,
            _ => return,
        };
        self.room_state = parsed;
    }

    pub fn is_all_ready(&self, minimum_buffer_ms: u64) -> bool {
        all_participants_ready(self.host_ready, self.guest_ready, minimum_buffer_ms)
    }

    pub fn transition_to(&mut self, target: RoomState) -> Result<RoomState, StateError> {
        let event = match (self.room_state, target) {
            (RoomState::ReadyCheck, RoomState::Playing) => RoomEvent::ReadyConsensus,
            _ => return Err(StateError::InvalidTransition),
        };
        self.room_state = transition(self.room_state, event)?;
        Ok(self.room_state)
    }

    pub fn host_ready(&mut self, readiness: ParticipantReadiness) {
        self.host_ready = readiness;
    }

    pub fn guest_ready(&mut self, readiness: ParticipantReadiness) {
        self.guest_ready = readiness;
    }

    /// READY_CHECK consensus: when both participants are ready and the room is
    /// still in a pre-play state, the coordinator enters READY_CHECK so both
    /// sides can observe consensus. Playback itself still requires a
    /// committed play operation (PROTOCOL_SPEC §40) — this only signals
    /// readiness consensus, it never starts playback.
    pub fn update_readiness_consensus(&mut self, minimum_buffer_ms: u64) {
        if matches!(
            self.room_state,
            RoomState::Created
                | RoomState::WaitingForGuest
                | RoomState::Lobby
                | RoomState::Reconnecting
        ) && self.all_ready(minimum_buffer_ms)
        {
            self.fire_coordinator_cb(RoomState::ReadyCheck);
        }
    }

    pub fn peer_ready(&mut self, readiness: ParticipantReadiness) {
        self.guest_ready = readiness;
    }

    /// "Back to lobby": retract the readiness votes and return the room
    /// to Lobby. This is a user-initiated retreat BEFORE any play commit
    /// (Back is disabled once the countdown commit is in flight), so the
    /// strict-sync state machine has no natural event for it — the same
    /// explicit-user-override reasoning as `abandon_guest` (§40). Any
    /// not-yet-fired countdown (pending_scheduled) is dropped so nothing
    /// starts playback after the users left Ready Check.
    pub fn retract_readiness(&mut self) {
        self.host_ready = ParticipantReadiness::not_ready("back-to-lobby");
        self.guest_ready = ParticipantReadiness::not_ready("back-to-lobby");
        self.pending_scheduled = None;
        self.paused_by_strict_sync = false;
        self.room_state = RoomState::Lobby;
        self.fire_coordinator_cb(RoomState::Lobby);
    }

    pub fn all_ready(&self, minimum_buffer_ms: u64) -> bool {
        if self.guest_abandoned {
            // §40 Continue Without Guest: the host explicitly chose to
            // continue solo; the guest's readiness is vacuously satisfied
            // for the rest of this session. This is the ONLY override of
            // the strict-sync guest gate and it is always user-initiated.
            return all_participants_ready(
                self.host_ready,
                ParticipantReadiness::ready(u64::MAX / 2),
                minimum_buffer_ms,
            );
        }
        all_participants_ready(self.host_ready, self.guest_ready, minimum_buffer_ms)
    }

    /// §40: the host acknowledged the guest's disconnect and chose to
    /// continue without them. Sticky for the session (a returning guest
    /// re-clears it via reconnection bookkeeping).
    pub fn abandon_guest(&mut self) {
        self.guest_abandoned = true;
    }

    pub fn guest_is_abandoned(&self) -> bool {
        self.guest_abandoned
    }

    pub fn prepare_play(
        &mut self,
        target_position_ms: u64,
        now_host_mono_us: u64,
        minimum_buffer_ms: u64,
    ) -> Result<ScheduledPlayback, LocalSyncError> {
        if !self.all_ready(minimum_buffer_ms) {
            return Err(LocalSyncError::NotReady);
        }
        let scheduled = ScheduledPlayback {
            operation_id: Uuid::now_v7(),
            target_position_ms,
            execute_at_host_mono_us: now_host_mono_us + 750_000,
        };
        self.pending_scheduled = Some(scheduled.clone());
        self.fire_coordinator_cb(RoomState::ReadyCheck);
        Ok(scheduled)
    }

    /// Like [`prepare_play`](Self::prepare_play) but with an explicitly
    /// computed execution deadline (MASTER_PRD §19 lead time) instead of the
    /// fixed 750ms baseline. The host AppRuntime computes the deadline from
    /// the peer's p95 RTT before calling this.
    pub fn prepare_play_scheduled(
        &mut self,
        target_position_ms: u64,
        execute_at_host_mono_us: u64,
        minimum_buffer_ms: u64,
    ) -> Result<ScheduledPlayback, LocalSyncError> {
        if !self.all_ready(minimum_buffer_ms) {
            return Err(LocalSyncError::NotReady);
        }
        let scheduled = ScheduledPlayback {
            operation_id: Uuid::now_v7(),
            target_position_ms,
            execute_at_host_mono_us,
        };
        self.pending_scheduled = Some(scheduled.clone());
        self.fire_coordinator_cb(RoomState::ReadyCheck);
        Ok(scheduled)
    }

    pub fn commit_play(&mut self, operation: &ScheduledPlayback) -> Result<(), LocalSyncError> {
        self.host_position_ms = operation.target_position_ms;
        self.guest_position_ms = operation.target_position_ms;
        self.fire_coordinator_cb(RoomState::Playing);
        self.paused_by_strict_sync = false;
        self.pending_scheduled = None;
        Ok(())
    }

    pub fn begin_pause(&mut self) {
        self.fire_coordinator_cb(RoomState::Pausing);
    }

    pub fn commit_pause(&mut self, target_position_ms: u64, cause: PauseCause) {
        self.host_position_ms = target_position_ms;
        self.guest_position_ms = target_position_ms;
        self.fire_coordinator_cb(RoomState::Paused);
        self.paused_by_strict_sync = cause == PauseCause::BufferLow;
    }

    /// The movie finished (AUD-03).
    ///
    /// Playback is over, but the *party* is not: this deliberately does not
    /// tear the session down the way `end_party` does. Leaving `Playing`
    /// stops both sides from correcting drift against a position that can no
    /// longer advance, and the coordinator broadcast carries the same
    /// transition to the guest, so neither side is left believing the film
    /// is still running.
    ///
    /// ADV-05: this deliberately does **not** set `paused_by_strict_sync`.
    ///
    /// It used to, on the reasoning that an ended room must not keep
    /// correcting drift. That was both redundant and wrong:
    ///
    /// * **redundant** — `drift_correction_for_player` already refuses to
    ///   correct unless `room_state == RoomState::Playing`, and this sets
    ///   `Ended`, so the correction is off regardless;
    /// * **wrong** — that flag is mirrored into the snapshot as
    ///   `strict_sync_paused`, which the frontend feeds to `BufferingOverlay`.
    ///   Setting it made the app show **"Paused to keep you together — <peer>
    ///   is buffering"** at the end of *every* movie, with a buffer meter for a
    ///   peer that is not buffering at all.
    pub fn ended(&mut self) {
        self.fire_coordinator_cb(RoomState::Ended);
    }

    pub fn update_position(&mut self, position_ms: u64) {
        self.host_position_ms = position_ms;
        self.guest_position_ms = position_ms;
    }

    pub fn record_peer_seq(&mut self, seq: u64) {
        if seq > self.last_peer_seq_received {
            self.last_peer_seq_received = seq;
        }
    }

    pub fn last_peer_seq_received(&self) -> u64 {
        self.last_peer_seq_received
    }

    pub fn buffer_low(
        &mut self,
        role: PeerRole,
        reported_position_ms: u64,
    ) -> Result<(), LocalSyncError> {
        match role {
            PeerRole::Host => self.host_position_ms = reported_position_ms,
            PeerRole::Guest => self.guest_position_ms = reported_position_ms,
        }
        let target = self.host_position_ms.min(self.guest_position_ms);
        self.host_position_ms = target;
        self.guest_position_ms = target;
        self.fire_coordinator_cb(RoomState::Buffering);
        self.paused_by_strict_sync = true;
        Ok(())
    }

    pub fn peer_disconnected(&mut self) -> Result<(), LocalSyncError> {
        self.guest_ready = ParticipantReadiness::not_ready("guest disconnected");
        self.fire_coordinator_cb(RoomState::Reconnecting);
        self.paused_by_strict_sync = true;
        Ok(())
    }

    /// Strict-sync recovery signal: the previously buffering participant has
    /// recovered enough buffer. This NEVER resumes playback by itself
    /// (PROTOCOL_SPEC §30: BUFFER_RECOVERED does not auto-resume). It only
    /// marks the room state as Buffering→ReadyCheck so the host can run a
    /// fresh PLAY_PREPARE/PLAY_READY/PLAY_COMMIT cycle when appropriate.
    pub fn buffer_recovered(&mut self) -> Result<(), LocalSyncError> {
        if self.room_state != RoomState::Buffering {
            return Ok(());
        }
        self.fire_coordinator_cb(RoomState::ReadyCheck);
        Ok(())
    }

    pub fn begin_seek(&mut self, target_position_ms: u64) {
        self.update_position(target_position_ms);
        self.fire_coordinator_cb(RoomState::Seeking);
    }

    pub fn commit_seek(&mut self, target_position_ms: u64, resume_after_seek: bool) {
        self.update_position(target_position_ms);
        self.fire_coordinator_cb(if resume_after_seek {
            RoomState::ReadyCheck
        } else {
            RoomState::Paused
        });
        self.paused_by_strict_sync = !resume_after_seek;
    }

    pub fn prepare_seek(
        &mut self,
        target_position_ms: u64,
        minimum_buffer_ms: u64,
    ) -> Result<ScheduledPlayback, LocalSyncError> {
        self.fire_coordinator_cb(RoomState::Seeking);
        if !self.all_ready(minimum_buffer_ms) {
            return Err(LocalSyncError::NotReady);
        }
        self.fire_coordinator_cb(RoomState::ReadyCheck);
        self.prepare_play(target_position_ms, 0, minimum_buffer_ms)
    }
}

impl Default for LocalSyncCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{LocalSyncCoordinator, PeerRole};
    use crate::sync::{consensus::ParticipantReadiness, state_machine::RoomState};

    #[test]
    fn guest_buffer_low_pauses_host_at_shared_position() {
        let mut coordinator = LocalSyncCoordinator::new();
        coordinator.room_state = RoomState::Playing;
        coordinator.host_position_ms = 12_000;
        coordinator.guest_position_ms = 11_800;

        coordinator
            .buffer_low(PeerRole::Guest, 11_750)
            .expect("pause");

        assert_eq!(coordinator.host_position_ms, 11_750);
        assert_eq!(coordinator.room_state, RoomState::Buffering);
        assert!(coordinator.paused_by_strict_sync);
    }

    #[test]
    fn play_requires_ready_consensus() {
        let mut coordinator = LocalSyncCoordinator::new();
        coordinator.host_ready(ParticipantReadiness::ready(5_000));
        // guest left as NotReady default -> all_ready is false

        assert!(coordinator.prepare_play(0, 0, 5_000).is_err());
    }

    #[test]
    fn disconnect_moves_to_reconnecting_and_pauses() {
        let mut coordinator = LocalSyncCoordinator::new();
        coordinator.room_state = RoomState::Playing;

        coordinator.peer_disconnected().expect("disconnect");

        assert_eq!(coordinator.room_state, RoomState::Reconnecting);
        assert!(coordinator.paused_by_strict_sync);
    }

    #[test]
    fn buffer_recovery_never_resumes_by_itself() {
        let mut coordinator = LocalSyncCoordinator::new();
        coordinator.host_position_ms = 8_000;
        coordinator.guest_position_ms = 8_000;
        coordinator
            .buffer_low(PeerRole::Guest, 8_000)
            .expect("pause");
        assert_eq!(coordinator.room_state, RoomState::Buffering);
        assert!(coordinator.paused_by_strict_sync);

        // PROTOCOL_SPEC §30: BUFFER_RECOVERED does NOT auto-resume. It only
        // returns the room to ReadyCheck so the host can run a fresh play
        // protocol cycle; the pause stays until an explicit play commit.
        coordinator.room_state = RoomState::Buffering;
        coordinator.buffer_recovered().expect("recovered");
        assert_eq!(coordinator.room_state, RoomState::ReadyCheck);
        assert!(coordinator.paused_by_strict_sync);

        coordinator
            .buffer_recovered()
            .expect("no-op outside buffering");
        assert_eq!(coordinator.room_state, RoomState::ReadyCheck);
    }
}
