pub const MAX_CONTROL_MESSAGE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum MessageType {
    Hello = 1,
    AuthRequest = 2,
    AuthAccept = 3,
    AuthReject = 4,
    Heartbeat = 5,
    Error = 250,
}

#[cfg(test)]
mod tests {
    use super::{MessageType, MAX_CONTROL_MESSAGE_BYTES};

    #[test]
    fn protocol_foundation_uses_locked_limits() {
        assert_eq!(MAX_CONTROL_MESSAGE_BYTES, 262_144);
        assert_eq!(MessageType::Hello as u16, 1);
        assert_eq!(MessageType::Error as u16, 250);
    }
}
