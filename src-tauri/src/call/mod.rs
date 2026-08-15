pub const MIC_ENABLED_INITIAL: bool = false;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CallMode {
    VideoVoice,
    VoiceOnly,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CallState {
    pub mode: CallMode,
    pub connected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CameraTier {
    A,
    B,
    C,
    D,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CameraState {
    pub enabled: bool,
    pub tier: CameraTier,
    pub width: u16,
    pub height: u16,
    pub fps: u8,
    pub target_bitrate_bps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MicState {
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CallSignalType {
    Offer,
    Answer,
    Ice,
    Renegotiate,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CallSignal {
    pub signal_type: CallSignalType,
    pub data: String,
}

impl Default for CallState {
    fn default() -> Self {
        Self {
            mode: CallMode::VideoVoice,
            connected: false,
        }
    }
}

impl Default for MicState {
    fn default() -> Self {
        Self {
            enabled: MIC_ENABLED_INITIAL,
        }
    }
}

impl CameraState {
    pub fn tier_b_enabled() -> Self {
        Self {
            enabled: true,
            tier: CameraTier::B,
            width: 640,
            height: 360,
            fps: 15,
            target_bitrate_bps: 350_000,
        }
    }

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            tier: CameraTier::D,
            width: 0,
            height: 0,
            fps: 0,
            target_bitrate_bps: 0,
        }
    }
}

pub fn validate_signal(signal: &CallSignal) -> bool {
    !signal.data.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microphone_starts_muted() {
        assert!(!MicState::default().enabled);
    }

    #[test]
    fn camera_tier_b_matches_v1_initial_constraints() {
        let state = CameraState::tier_b_enabled();

        assert!(state.enabled);
        assert_eq!(state.tier, CameraTier::B);
        assert_eq!(state.width, 640);
        assert_eq!(state.height, 360);
        assert_eq!(state.fps, 15);
        assert_eq!(state.target_bitrate_bps, 350_000);
    }

    #[test]
    fn call_signal_requires_payload() {
        let signal = CallSignal {
            signal_type: CallSignalType::Offer,
            data: "sdp".to_owned(),
        };

        assert!(validate_signal(&signal));

        let empty = CallSignal {
            signal_type: CallSignalType::Ice,
            data: " ".to_owned(),
        };

        assert!(!validate_signal(&empty));
    }
}
