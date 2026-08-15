pub const V1_PARTICIPANT_COUNT: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParticipantReadiness {
    pub player_ready: bool,
    pub buffer_ahead_ms: u64,
    pub stalled: bool,
}

impl ParticipantReadiness {
    pub fn ready(buffer_ahead_ms: u64) -> Self {
        Self {
            player_ready: true,
            buffer_ahead_ms,
            stalled: false,
        }
    }
}

pub fn all_participants_ready(
    host: ParticipantReadiness,
    guest: ParticipantReadiness,
    minimum_buffer_ms: u64,
) -> bool {
    [host, guest].into_iter().all(|participant| {
        participant.player_ready
            && !participant.stalled
            && participant.buffer_ahead_ms >= minimum_buffer_ms
    })
}

#[cfg(test)]
mod tests {
    use super::{all_participants_ready, ParticipantReadiness};

    #[test]
    fn requires_both_participants_to_be_ready() {
        assert!(all_participants_ready(
            ParticipantReadiness::ready(5_000),
            ParticipantReadiness::ready(5_000),
            5_000,
        ));
        assert!(!all_participants_ready(
            ParticipantReadiness::ready(5_000),
            ParticipantReadiness {
                stalled: true,
                ..ParticipantReadiness::ready(5_000)
            },
            5_000,
        ));
    }
}
