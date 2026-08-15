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
    pub fn tier_a_enabled() -> Self {
        Self {
            enabled: true,
            tier: CameraTier::A,
            width: 854,
            height: 480,
            fps: 20,
            target_bitrate_bps: 550_000,
        }
    }

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

    pub fn tier_c_enabled() -> Self {
        Self {
            enabled: true,
            tier: CameraTier::C,
            width: 426,
            height: 240,
            fps: 12,
            target_bitrate_bps: 180_000,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraFeedback {
    pub measured_goodput_bps: u64,
    pub estimated_movie_bitrate_bps: u64,
    pub guest_buffer_ms: u64,
    pub rtt_ms: u32,
}

pub fn recommend_camera_state(
    current: CameraTier,
    camera_enabled: bool,
    feedback: CameraFeedback,
) -> CameraState {
    if !camera_enabled || feedback.estimated_movie_bitrate_bps == 0 {
        return CameraState::disabled();
    }

    let ratio = feedback.measured_goodput_bps as f64 / feedback.estimated_movie_bitrate_bps as f64;

    if feedback.guest_buffer_ms < 2_000 || ratio < 1.0 {
        return CameraState::disabled();
    }

    if feedback.guest_buffer_ms < 5_000 || ratio < 1.2 || feedback.rtt_ms > 300 {
        return state_for_tier(downgrade(current));
    }

    if feedback.guest_buffer_ms > 30_000 && ratio >= 1.8 && feedback.rtt_ms <= 150 {
        return state_for_tier(upgrade(current));
    }

    state_for_tier(current)
}

pub fn state_for_tier(tier: CameraTier) -> CameraState {
    match tier {
        CameraTier::A => CameraState::tier_a_enabled(),
        CameraTier::B => CameraState::tier_b_enabled(),
        CameraTier::C => CameraState::tier_c_enabled(),
        CameraTier::D => CameraState::disabled(),
    }
}

fn downgrade(tier: CameraTier) -> CameraTier {
    match tier {
        CameraTier::A => CameraTier::B,
        CameraTier::B => CameraTier::C,
        CameraTier::C | CameraTier::D => CameraTier::D,
    }
}

fn upgrade(tier: CameraTier) -> CameraTier {
    match tier {
        CameraTier::A => CameraTier::A,
        CameraTier::B => CameraTier::A,
        CameraTier::C => CameraTier::B,
        CameraTier::D => CameraTier::C,
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
    fn camera_tiers_never_use_1080p() {
        for tier in [CameraTier::A, CameraTier::B, CameraTier::C] {
            let state = state_for_tier(tier);

            assert!(state.width <= 854);
            assert!(state.height <= 480);
            assert!(state.fps <= 20);
        }
    }

    #[test]
    fn pressure_downgrades_camera_before_movie_is_sacrificed() {
        let state = recommend_camera_state(
            CameraTier::B,
            true,
            CameraFeedback {
                measured_goodput_bps: 5_500_000,
                estimated_movie_bitrate_bps: 5_000_000,
                guest_buffer_ms: 4_500,
                rtt_ms: 80,
            },
        );

        assert_eq!(state.tier, CameraTier::C);
        assert!(state.target_bitrate_bps < CameraState::tier_b_enabled().target_bitrate_bps);
    }

    #[test]
    fn severe_pressure_disables_camera_to_protect_movie_continuity() {
        let state = recommend_camera_state(
            CameraTier::C,
            true,
            CameraFeedback {
                measured_goodput_bps: 4_500_000,
                estimated_movie_bitrate_bps: 5_000_000,
                guest_buffer_ms: 1_500,
                rtt_ms: 240,
            },
        );

        assert_eq!(state.tier, CameraTier::D);
        assert!(!state.enabled);
        assert_eq!(state.target_bitrate_bps, 0);
    }

    #[test]
    fn healthy_feedback_recovers_camera_one_tier_at_a_time() {
        let state = recommend_camera_state(
            CameraTier::C,
            true,
            CameraFeedback {
                measured_goodput_bps: 10_000_000,
                estimated_movie_bitrate_bps: 5_000_000,
                guest_buffer_ms: 45_000,
                rtt_ms: 60,
            },
        );

        assert_eq!(state.tier, CameraTier::B);
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
