use uuid::Uuid;

use super::{
    consensus::{all_participants_ready, ParticipantReadiness},
    state_machine::{transition, RoomEvent, RoomState, StateError},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerRole {
    Host,
    Guest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledPlayback {
    pub operation_id: Uuid,
    pub target_position_ms: u64,
    pub execute_at_host_mono_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSyncCoordinator {
    pub room_state: RoomState,
    pub host_position_ms: u64,
    pub guest_position_ms: u64,
    pub host_ready: ParticipantReadiness,
    pub guest_ready: ParticipantReadiness,
    pub paused_by_strict_sync: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LocalSyncError {
    #[error(transparent)]
    State(#[from] StateError),
    #[error("MP-SYNC-004 participants are not ready")]
    NotReady,
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
        }
    }

    pub fn update_readiness(&mut self, host: ParticipantReadiness, guest: ParticipantReadiness) {
        self.host_ready = host;
        self.guest_ready = guest;
    }

    pub fn prepare_play(
        &mut self,
        target_position_ms: u64,
        now_host_mono_us: u64,
        minimum_buffer_ms: u64,
    ) -> Result<ScheduledPlayback, LocalSyncError> {
        if !all_participants_ready(self.host_ready, self.guest_ready, minimum_buffer_ms) {
            return Err(LocalSyncError::NotReady);
        }

        self.room_state = RoomState::ReadyCheck;
        Ok(ScheduledPlayback {
            operation_id: Uuid::now_v7(),
            target_position_ms,
            execute_at_host_mono_us: now_host_mono_us + 750_000,
        })
    }

    pub fn commit_play(&mut self, operation: &ScheduledPlayback) -> Result<(), LocalSyncError> {
        self.host_position_ms = operation.target_position_ms;
        self.guest_position_ms = operation.target_position_ms;
        self.room_state = transition(RoomState::ReadyCheck, RoomEvent::PlayCommitted)?;
        self.paused_by_strict_sync = false;
        Ok(())
    }

    pub fn buffer_low(
        &mut self,
        role: PeerRole,
        reported_position_ms: u64,
    ) -> Result<ScheduledPlayback, LocalSyncError> {
        match role {
            PeerRole::Host => self.host_position_ms = reported_position_ms,
            PeerRole::Guest => self.guest_position_ms = reported_position_ms,
        }

        self.room_state = transition(RoomState::Playing, RoomEvent::BufferLow)?;
        let target = self.host_position_ms.min(self.guest_position_ms);
        self.host_position_ms = target;
        self.guest_position_ms = target;
        self.room_state = transition(RoomState::Pausing, RoomEvent::PauseCommitted)?;
        self.paused_by_strict_sync = true;

        Ok(ScheduledPlayback {
            operation_id: Uuid::now_v7(),
            target_position_ms: target,
            execute_at_host_mono_us: 0,
        })
    }

    pub fn prepare_seek(
        &mut self,
        target_position_ms: u64,
        minimum_buffer_ms: u64,
    ) -> Result<ScheduledPlayback, LocalSyncError> {
        self.room_state = transition(self.room_state, RoomEvent::SeekPrepared)?;
        if !all_participants_ready(self.host_ready, self.guest_ready, minimum_buffer_ms) {
            return Err(LocalSyncError::NotReady);
        }
        self.room_state = transition(RoomState::Seeking, RoomEvent::ReadyConsensus)?;
        self.prepare_play(target_position_ms, 0, minimum_buffer_ms)
    }

    pub fn peer_disconnected(&mut self) -> Result<(), LocalSyncError> {
        self.room_state = transition(self.room_state, RoomEvent::PeerDisconnected)?;
        self.paused_by_strict_sync = true;
        Ok(())
    }

    pub fn recover_from_buffering(
        &mut self,
        minimum_buffer_ms: u64,
    ) -> Result<ScheduledPlayback, LocalSyncError> {
        self.room_state = RoomState::Buffering;
        if !all_participants_ready(self.host_ready, self.guest_ready, minimum_buffer_ms) {
            return Err(LocalSyncError::NotReady);
        }
        self.room_state = transition(RoomState::Buffering, RoomEvent::ReadyConsensus)?;
        self.prepare_play(
            self.host_position_ms.min(self.guest_position_ms),
            0,
            minimum_buffer_ms,
        )
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

        let pause = coordinator
            .buffer_low(PeerRole::Guest, 11_750)
            .expect("pause");

        assert_eq!(pause.target_position_ms, 11_750);
        assert_eq!(coordinator.host_position_ms, 11_750);
        assert_eq!(coordinator.room_state, RoomState::Paused);
        assert!(coordinator.paused_by_strict_sync);
    }

    #[test]
    fn play_requires_ready_consensus() {
        let mut coordinator = LocalSyncCoordinator::new();
        coordinator.update_readiness(
            ParticipantReadiness::ready(5_000),
            ParticipantReadiness::ready(1_000),
        );

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
}
