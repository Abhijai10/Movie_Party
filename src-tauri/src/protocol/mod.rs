pub const MAX_CONTROL_MESSAGE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MessageType {
    Hello = 1,
    AuthRequest = 2,
    AuthAccept = 3,
    AuthReject = 4,
    Heartbeat = 5,
    CallState = 160,
    CameraState = 161,
    MicState = 162,
    CallSignal = 163,
    ChatMessage = 180,
    Reaction = 181,
    Error = 250,
    ReadyState = 200,
    BufferStatus = 201,
    ControlRequest = 202,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SequenceTracker {
    last_seq_received: u64,
}

impl SequenceTracker {
    pub fn accept(&mut self, seq: u64) -> bool {
        if seq == 0 || seq <= self.last_seq_received {
            return false;
        }

        self.last_seq_received = seq;
        true
    }

    pub fn last_seq_received(&self) -> u64 {
        self.last_seq_received
    }
}

#[cfg(test)]
mod tests {
    use super::{MessageType, SequenceTracker, MAX_CONTROL_MESSAGE_BYTES};

    #[test]
    fn protocol_foundation_uses_locked_limits() {
        assert_eq!(MAX_CONTROL_MESSAGE_BYTES, 262_144);
        assert_eq!(MessageType::Hello as u16, 1);
        assert_eq!(MessageType::CallState as u16, 160);
        assert_eq!(MessageType::CameraState as u16, 161);
        assert_eq!(MessageType::MicState as u16, 162);
        assert_eq!(MessageType::CallSignal as u16, 163);
        assert_eq!(MessageType::ChatMessage as u16, 180);
        assert_eq!(MessageType::Reaction as u16, 181);
        assert_eq!(MessageType::Error as u16, 250);
    }

    #[test]
    fn sequence_tracker_rejects_zero_duplicate_and_stale_sequences() {
        let mut tracker = SequenceTracker::default();

        assert!(!tracker.accept(0));
        assert!(tracker.accept(1));
        assert!(!tracker.accept(1));
        assert!(!tracker.accept(0));
        assert!(tracker.accept(2));
        assert_eq!(tracker.last_seq_received(), 2);
    }
}
