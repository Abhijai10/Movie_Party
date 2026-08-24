use std::collections::BTreeSet;

use uuid::Uuid;

use super::{
    consensus::{all_participants_ready, ParticipantReadiness},
    state_machine::{transition, RoomEvent, RoomState, StateError},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    Ready,
    Playing,
    Paused,
    Buffering,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakePlayer {
    pub position_ms: i64,
    pub state: PlayerState,
    pub buffer_ahead_ms: u64,
}

impl FakePlayer {
    pub fn ready(buffer_ahead_ms: u64) -> Self {
        Self {
            position_ms: 0,
            state: PlayerState::Ready,
            buffer_ahead_ms,
        }
    }

    fn tick(&mut self, delta_ms: i64) {
        if self.state != PlayerState::Playing {
            return;
        }

        self.position_ms += delta_ms;
        self.buffer_ahead_ms = self.buffer_ahead_ms.saturating_sub(delta_ms.max(0) as u64);
        if self.buffer_ahead_ms == 0 {
            self.state = PlayerState::Buffering;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackOperation {
    pub operation_id: Uuid,
    pub target_position_ms: i64,
    pub execute_at_host_ms: i64,
}

#[derive(Debug, Clone)]
pub struct NetworkProfile {
    pub rtt_ms: u64,
    pub jitter_ms: u64,
    pub packet_loss_percent: u8,
    pub outage_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulationReport {
    pub max_drift_ms: i64,
    pub p95_drift_ms: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum SimulationError {
    #[error(transparent)]
    State(#[from] StateError),
    #[error("MP-SYNC-003 playback state diverged")]
    Diverged,
}

#[derive(Debug)]
pub struct SyncSimulator {
    pub room_state: RoomState,
    pub host: FakePlayer,
    pub guest: FakePlayer,
    applied_operations: BTreeSet<Uuid>,
}

impl SyncSimulator {
    pub fn new() -> Self {
        Self {
            room_state: RoomState::Lobby,
            host: FakePlayer::ready(30_000),
            guest: FakePlayer::ready(30_000),
            applied_operations: BTreeSet::new(),
        }
    }

    pub fn ready_consensus(&mut self, minimum_buffer_ms: u64) -> bool {
        all_participants_ready(
            self.readiness(self.host.clone()),
            self.readiness(self.guest.clone()),
            minimum_buffer_ms,
        )
    }

    pub fn schedule_play(
        &mut self,
        target_position_ms: i64,
        now_host_ms: i64,
    ) -> PlaybackOperation {
        PlaybackOperation {
            operation_id: Uuid::now_v7(),
            target_position_ms,
            execute_at_host_ms: now_host_ms + 750,
        }
    }

    pub fn apply_play(&mut self, operation: &PlaybackOperation) -> Result<(), SimulationError> {
        if !self.applied_operations.insert(operation.operation_id) {
            return Ok(());
        }

        self.host.position_ms = operation.target_position_ms;
        self.guest.position_ms = operation.target_position_ms;
        self.host.state = PlayerState::Playing;
        self.guest.state = PlayerState::Playing;
        self.room_state = transition(RoomState::ReadyCheck, RoomEvent::PlayCommitted)?;
        Ok(())
    }

    pub fn apply_pause(&mut self, target_position_ms: i64) -> Result<(), SimulationError> {
        self.host.position_ms = target_position_ms;
        self.guest.position_ms = target_position_ms;
        self.host.state = PlayerState::Paused;
        self.guest.state = PlayerState::Paused;
        self.room_state = transition(RoomState::Pausing, RoomEvent::PauseCommitted)?;
        Ok(())
    }

    pub fn seek_ready_commit(
        &mut self,
        target_position_ms: i64,
        minimum_buffer_ms: u64,
    ) -> Result<(), SimulationError> {
        self.room_state = transition(RoomState::Playing, RoomEvent::SeekPrepared)?;
        self.host.buffer_ahead_ms = self.host.buffer_ahead_ms.max(minimum_buffer_ms);
        self.guest.buffer_ahead_ms = self.guest.buffer_ahead_ms.max(minimum_buffer_ms);
        self.room_state = transition(RoomState::Seeking, RoomEvent::ReadyConsensus)?;
        let operation = self.schedule_play(target_position_ms, 0);
        self.apply_play(&operation)
    }

    pub fn run_profile(
        &mut self,
        profile: NetworkProfile,
    ) -> Result<SimulationReport, SimulationError> {
        self.room_state = RoomState::ReadyCheck;
        let play = self.schedule_play(0, 0);
        self.apply_play(&play)?;

        let mut max_drift = 0_i64;
        let mut drifts = Vec::with_capacity(300);
        for tick in 0..300 {
            let guest_delay = if profile.packet_loss_percent > 0
                && tick % (100 / profile.packet_loss_percent as usize).max(1) == 0
            {
                profile.rtt_ms as i64
            } else {
                (profile.jitter_ms.min(50) / 2) as i64
            };

            let outage = tick < (profile.outage_ms / 100) as usize;
            self.host.tick(100);
            if !outage {
                self.guest.tick((100 - guest_delay).max(0));
            }

            if self.guest.state == PlayerState::Buffering || outage {
                self.room_state = transition(RoomState::Playing, RoomEvent::BufferLow)?;
                self.apply_pause(self.host.position_ms.min(self.guest.position_ms))?;
                self.host.buffer_ahead_ms += 10_000;
                self.guest.buffer_ahead_ms += 10_000;
                self.room_state = transition(RoomState::Buffering, RoomEvent::ReadyConsensus)
                    .unwrap_or(RoomState::ReadyCheck);
                let resume =
                    self.schedule_play(self.host.position_ms.min(self.guest.position_ms), 0);
                self.apply_play(&resume)?;
            }

            let drift = (self.host.position_ms - self.guest.position_ms).abs();
            max_drift = max_drift.max(drift);
            drifts.push(drift);
            if drift > 700 {
                self.guest.position_ms = self.host.position_ms;
            } else if drift > 80 {
                self.guest.position_ms += drift / 2;
            }
        }

        if (self.host.position_ms - self.guest.position_ms).abs() > 100 {
            return Err(SimulationError::Diverged);
        }

        Ok(SimulationReport {
            max_drift_ms: max_drift,
            p95_drift_ms: percentile(&mut drifts, 95),
        })
    }

    fn readiness(&self, player: FakePlayer) -> ParticipantReadiness {
        ParticipantReadiness {
            player_ready: matches!(player.state, PlayerState::Ready | PlayerState::Paused),
            buffer_ahead_ms: player.buffer_ahead_ms,
            stalled: player.state == PlayerState::Buffering,
        }
    }
}

fn percentile(values: &mut [i64], percentile: usize) -> i64 {
    if values.is_empty() {
        return 0;
    }

    values.sort_unstable();
    let index = ((values.len() - 1) * percentile) / 100;
    values[index]
}

impl Default for SyncSimulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{NetworkProfile, PlayerState, SyncSimulator};

    #[test]
    fn duplicate_play_operation_is_idempotent() {
        let mut simulator = SyncSimulator::new();
        simulator.room_state = super::RoomState::ReadyCheck;
        let operation = simulator.schedule_play(12_000, 0);

        simulator.apply_play(&operation).expect("first apply");
        simulator.host.position_ms = 13_000;
        simulator.apply_play(&operation).expect("duplicate ignored");

        assert_eq!(simulator.host.position_ms, 13_000);
    }

    #[test]
    fn guest_buffer_failure_pauses_host() {
        let mut simulator = SyncSimulator::new();
        simulator.room_state = super::RoomState::ReadyCheck;
        let operation = simulator.schedule_play(0, 0);
        simulator.apply_play(&operation).expect("play");
        simulator.guest.buffer_ahead_ms = 0;
        simulator.guest.state = PlayerState::Buffering;

        simulator.apply_pause(1_000).expect("pause");

        assert_eq!(simulator.host.state, PlayerState::Paused);
        assert_eq!(simulator.guest.state, PlayerState::Paused);
    }

    #[test]
    fn simulation_matrix_does_not_diverge() {
        let profiles = [
            NetworkProfile {
                rtt_ms: 10,
                jitter_ms: 0,
                packet_loss_percent: 0,
                outage_ms: 0,
            },
            NetworkProfile {
                rtt_ms: 50,
                jitter_ms: 20,
                packet_loss_percent: 1,
                outage_ms: 0,
            },
            NetworkProfile {
                rtt_ms: 150,
                jitter_ms: 50,
                packet_loss_percent: 3,
                outage_ms: 10_000,
            },
        ];

        for profile in profiles {
            let report = SyncSimulator::new()
                .run_profile(profile)
                .expect("stable simulation");
            assert!(report.max_drift_ms <= 700);
        }
    }

    #[test]
    fn normal_profile_has_p95_drift_under_target() {
        let report = SyncSimulator::new()
            .run_profile(NetworkProfile {
                rtt_ms: 10,
                jitter_ms: 0,
                packet_loss_percent: 0,
                outage_ms: 0,
            })
            .expect("stable simulation");

        assert!(report.p95_drift_ms < 100);
    }
}
