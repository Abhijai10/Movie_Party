pub const STRICT_SYNC_DEFAULT: bool = true;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomState {
    Created,
    WaitingForGuest,
    Lobby,
    Preparing,
    ReadyCheck,
    Playing,
    Pausing,
    Paused,
    Seeking,
    Buffering,
    Reconnecting,
    Ended,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomEvent {
    GuestJoined,
    MediaSelected,
    ReadyConsensus,
    PlayCommitted,
    PausePrepared,
    PauseCommitted,
    SeekPrepared,
    BufferLow,
    BufferRecovered,
    PeerDisconnected,
    Reconnected,
    End,
    Fail,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StateError {
    #[error("MP-SYNC-002 invalid room state transition")]
    InvalidTransition,
}

pub fn transition(state: RoomState, event: RoomEvent) -> Result<RoomState, StateError> {
    use RoomEvent::{
        BufferLow, End, Fail, GuestJoined, MediaSelected, PauseCommitted, PausePrepared,
        PeerDisconnected, PlayCommitted, ReadyConsensus, Reconnected, SeekPrepared,
    };
    use RoomState::{
        Buffering, Created, Ended, Error, Lobby, Paused, Pausing, Playing, Preparing, ReadyCheck,
        Reconnecting, Seeking, WaitingForGuest,
    };

    let next = match (state, event) {
        (Created, GuestJoined) => WaitingForGuest,
        (WaitingForGuest, GuestJoined) => Lobby,
        (Lobby, MediaSelected) => Preparing,
        (Preparing, ReadyConsensus) => ReadyCheck,
        (ReadyCheck, PlayCommitted) => Playing,
        (Paused | Buffering, ReadyConsensus) => ReadyCheck,
        (Playing, PausePrepared | BufferLow) => Pausing,
        (Pausing, PauseCommitted) => Paused,
        (Playing | Paused, SeekPrepared) => Seeking,
        (Seeking, ReadyConsensus) => ReadyCheck,
        (Playing | Paused | Buffering | Seeking | Pausing, PeerDisconnected) => Reconnecting,
        (Reconnecting, Reconnected) => ReadyCheck,
        (_, End) => Ended,
        (_, Fail) => Error,
        _ => return Err(StateError::InvalidTransition),
    };

    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::{transition, RoomEvent, RoomState, StateError};

    #[test]
    fn rejects_invalid_play_from_waiting_state() {
        assert_eq!(
            transition(RoomState::WaitingForGuest, RoomEvent::PlayCommitted),
            Err(StateError::InvalidTransition),
        );
    }
}
