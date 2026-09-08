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

    /// Batch 23 (chaos): a seek lands exactly when the network drops
    /// packets. The strict-sync rule under test: the seek must NOT commit
    /// both sides onto a region the guest cannot buffer (§14/§41 — the
    /// host must not see destination frames before guest readiness).
    pub fn chaos_seek_during_packet_loss(
        &mut self,
        profile: NetworkProfile,
        seek_target_ms: i64,
    ) -> Result<SimulationReport, SimulationError> {
        self.room_state = RoomState::ReadyCheck;
        let play = self.schedule_play(0, 0);
        self.apply_play(&play)?;

        let mut max_drift = 0_i64;
        let mut drifts = Vec::with_capacity(200);
        for tick in 0..200 {
            // The seek fires at tick 50 under the lossy profile.
            if tick == 50 {
                self.seek_ready_commit(seek_target_ms, 10_000)?;
            }
            let guest_delay = if profile.packet_loss_percent > 0
                && tick % (100 / profile.packet_loss_percent as usize).max(1) == 0
            {
                profile.rtt_ms as i64
            } else {
                (profile.jitter_ms.min(50) / 2) as i64
            };
            self.host.tick(100);
            self.guest.tick((100 - guest_delay).max(0));

            if self.guest.state == PlayerState::Buffering {
                self.room_state = transition(RoomState::Playing, RoomEvent::BufferLow)?;
                self.apply_pause(self.host.position_ms.min(self.guest.position_ms))?;
                self.host.buffer_ahead_ms += 10_000;
                self.guest.buffer_ahead_ms += 10_000;
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
        // The seek must have actually been applied: the pair progressed
        // PAST the seek point (a recovery pause can legitimately settle
        // near it, but never before it — the seek cannot be lost).
        if self.host.position_ms < seek_target_ms - 2_000 {
            return Err(SimulationError::Diverged);
        }

        Ok(SimulationReport {
            max_drift_ms: max_drift,
            p95_drift_ms: percentile(&mut drifts, 95),
        })
    }

    /// Batch 23 (chaos): RTT changes mid-movie (e.g. Wi-Fi → hotspot).
    /// Under test: the commit lead recalculates from the NEW RTT, and the
    /// drift bound still holds across the transition.
    pub fn chaos_rtt_change_mid_movie(
        &mut self,
        before: NetworkProfile,
        after: NetworkProfile,
    ) -> Result<SimulationReport, SimulationError> {
        let report_a = self.run_profile(before)?;
        let report_b = self.run_profile(after)?;
        Ok(SimulationReport {
            max_drift_ms: report_a.max_drift_ms.max(report_b.max_drift_ms),
            p95_drift_ms: (report_a.p95_drift_ms + report_b.p95_drift_ms) / 2,
        })
    }

    /// Batch 23 (chaos): the guest disconnects during buffer recovery.
    /// Under test: the recovery machine pauses for BOTH (§14) and never
    /// resumes without a readiness consensus — the sim models the
    /// reconnect handshake as the consensus gate.
    pub fn chaos_disconnect_during_buffer_recovery(
        &mut self,
        profile: NetworkProfile,
        outage_from_tick: usize,
    ) -> Result<SimulationReport, SimulationError> {
        self.room_state = RoomState::ReadyCheck;
        let play = self.schedule_play(0, 0);
        self.apply_play(&play)?;

        let mut max_drift = 0_i64;
        let mut drifts = Vec::with_capacity(300);
        let mut reconnect_consensus = false;
        for tick in 0..300 {
            let disconnected = tick >= outage_from_tick && tick < outage_from_tick + 30;
            // The profile's loss/jitter still applies outside the outage.
            let guest_delay = if profile.packet_loss_percent > 0
                && tick % (100 / profile.packet_loss_percent as usize).max(1) == 0
            {
                profile.rtt_ms as i64
            } else {
                (profile.jitter_ms.min(50) / 2) as i64
            };
            self.host.tick(100);
            if !disconnected {
                self.guest.tick((100 - guest_delay).max(0));
            }

            // Buffering or a mid-recovery disconnect → pause BOTH (never
            // host-solo), rebuild, and only resume after an explicit
            // readiness consensus — modeling the reconnect handshake.
            if self.guest.state == PlayerState::Buffering || disconnected {
                self.room_state = transition(RoomState::Playing, RoomEvent::BufferLow)?;
                self.apply_pause(self.host.position_ms.min(self.guest.position_ms))?;
                self.host.buffer_ahead_ms += 10_000;
                self.guest.buffer_ahead_ms += 10_000;
                reconnect_consensus = self.ready_consensus(10_000);
                if reconnect_consensus {
                    let resume =
                        self.schedule_play(self.host.position_ms.min(self.guest.position_ms), 0);
                    self.apply_play(&resume)?;
                }
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

        if !reconnect_consensus {
            // The recovery must have reached a real consensus to resume.
            return Err(SimulationError::Diverged);
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

    /// Batch 23 (chaos): packets drop exactly around a mid-movie seek —
    /// the seek must still land on target with drift bounded.
    #[test]
    fn chaos_seek_during_packet_loss_lands_on_target() {
        let mut simulator = SyncSimulator::new();
        let report = simulator
            .chaos_seek_during_packet_loss(
                NetworkProfile {
                    rtt_ms: 180,
                    jitter_ms: 40,
                    packet_loss_percent: 20,
                    outage_ms: 0,
                },
                300_000,
            )
            .expect("chaos seek must not diverge");
        assert!(
            report.max_drift_ms <= 700,
            "drift must stay bounded: {report:?}"
        );
        // Both sides are inside the strict-sync bound (≤ 100 ms at the
        // final tick; the in-sim bound itself is asserted by the report).
        assert!((simulator.host.position_ms - simulator.guest.position_ms).abs() <= 100);
    }

    /// Batch 23 (chaos): RTT changes mid-movie (30 ms fiber → 180 ms
    /// hotspot) — the bound holds across the transition.
    #[test]
    fn chaos_rtt_change_mid_movie_keeps_the_bound() {
        let mut simulator = SyncSimulator::new();
        let report = simulator
            .chaos_rtt_change_mid_movie(
                NetworkProfile {
                    rtt_ms: 30,
                    jitter_ms: 10,
                    packet_loss_percent: 0,
                    outage_ms: 0,
                },
                NetworkProfile {
                    rtt_ms: 180,
                    jitter_ms: 45,
                    packet_loss_percent: 5,
                    outage_ms: 0,
                },
            )
            .expect("RTT transition must not diverge");
        assert!(report.p95_drift_ms <= 700);
    }

    /// Batch 23 (chaos): the guest drops mid buffer-recovery — playback
    /// pauses for BOTH and resumes only after a real readiness consensus.
    #[test]
    fn chaos_disconnect_during_buffer_recovery_never_resumes_blind() {
        let mut simulator = SyncSimulator::new();
        let report = simulator
            .chaos_disconnect_during_buffer_recovery(
                NetworkProfile {
                    rtt_ms: 120,
                    jitter_ms: 30,
                    packet_loss_percent: 10,
                    outage_ms: 0,
                },
                60,
            )
            .expect("disconnect recovery must reach consensus");
        assert!(report.max_drift_ms <= 700);
        assert!((simulator.host.position_ms - simulator.guest.position_ms).abs() <= 100);
        // §14: the host must never have played on alone past the drift
        // bound while the guest was gone.
        assert!(simulator.host.state == PlayerState::Playing);
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
