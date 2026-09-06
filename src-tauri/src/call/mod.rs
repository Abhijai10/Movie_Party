pub const MIC_ENABLED_INITIAL: bool = false;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CallMode {
    VideoVoice,
    VoiceOnly,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CallRuntimeStatus {
    Connecting,
    Connected,
    Degraded,
    Reconnecting,
    Unavailable,
    Ended,
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
#[serde(rename_all = "camelCase")]
pub struct CallSignal {
    pub signal_type: CallSignalType,
    pub data: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallSignalLedger {
    offer_pending: bool,
    answer_seen: bool,
    ice_candidates: std::collections::HashSet<String>,
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

impl CallSignalLedger {
    pub fn reset(&mut self) {
        self.offer_pending = false;
        self.answer_seen = false;
        self.ice_candidates.clear();
    }
}

pub fn validate_signal(signal: &CallSignal, ledger: &mut CallSignalLedger) -> Result<(), String> {
    // PROTOCOL_SPEC §52: CALL_SIGNAL payload max 64 KiB.
    const MAX_SIGNAL_BYTES: usize = 64 * 1024;

    let data = signal.data.trim();
    if data.is_empty() || data.len() > MAX_SIGNAL_BYTES {
        return Err("MP-CALL-001 invalid call signal".to_string());
    }

    match signal.signal_type {
        CallSignalType::Offer => {
            validate_session_description(data, "offer")?;
            if ledger.offer_pending && !ledger.answer_seen {
                return Err("MP-CALL-002 duplicate call offer".to_string());
            }
            ledger.offer_pending = true;
            ledger.answer_seen = false;
            ledger.ice_candidates.clear();
            Ok(())
        }
        CallSignalType::Answer => {
            validate_session_description(data, "answer")?;
            if !ledger.offer_pending && !is_internal_session_description(data, "answer") {
                return Err("MP-CALL-003 stale call answer".to_string());
            }
            if ledger.answer_seen {
                return Err("MP-CALL-004 duplicate call answer".to_string());
            }
            ledger.offer_pending = true;
            ledger.answer_seen = true;
            Ok(())
        }
        CallSignalType::Ice => {
            let candidate = validate_ice_candidate(data)?;
            if candidate.is_empty() || candidate.len() > 16_384 {
                return Err("MP-CALL-005 malformed ICE candidate".to_string());
            }
            if !ledger.offer_pending && !is_internal_ice_candidate(data) {
                return Err("MP-CALL-006 stale ICE candidate".to_string());
            }
            ledger.offer_pending = true;
            if !ledger.ice_candidates.insert(candidate) {
                return Err("MP-CALL-007 duplicate ICE candidate".to_string());
            }
            Ok(())
        }
        CallSignalType::Renegotiate => {
            // Batch 12: a renegotiation REQUEST (not an SDP) — the peer's
            // session restarted (mode change, privacy exit, view remount)
            // and needs a fresh offer from this device. Payload is a small
            // marker object. The ledger reset is CONDITIONAL: only an
            // exchange that never completed (offer pending, no answer)
            // blocks a fresh OFFER (MP-CALL-002), so only that state is
            // superseded. A completed exchange already accepts a new
            // OFFER, and resetting it would poison an ANSWER still in
            // flight (MP-CALL-003).
            if data.len() > 2_048 {
                return Err("MP-CALL-001 invalid call signal".to_string());
            }
            if ledger.offer_pending && !ledger.answer_seen {
                ledger.reset();
            }
            Ok(())
        }
    }
}

fn validate_session_description(data: &str, expected_type: &str) -> Result<(), String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
        return validate_json_session_description(&value, expected_type);
    }

    if is_internal_session_description(data, expected_type) {
        return Ok(());
    }

    Err("MP-CALL-001 malformed call signal".to_string())
}

fn validate_json_session_description(
    value: &serde_json::Value,
    expected_type: &str,
) -> Result<(), String> {
    let description_type = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "MP-CALL-001 malformed call signal".to_string())?;
    if description_type != expected_type {
        return Err("MP-CALL-001 malformed call signal".to_string());
    }
    let sdp = value
        .get("sdp")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .ok_or_else(|| "MP-CALL-001 malformed call signal".to_string())?;
    if sdp.is_empty() || !sdp.starts_with("v=0") {
        return Err("MP-CALL-001 malformed call signal".to_string());
    }
    Ok(())
}

fn validate_ice_candidate(data: &str) -> Result<String, String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
        return value
            .get("candidate")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .map(str::to_string)
            .ok_or_else(|| "MP-CALL-005 malformed ICE candidate".to_string());
    }

    let candidate = data.trim();
    if is_internal_ice_candidate(candidate) {
        return Ok(candidate.to_string());
    }

    Err("MP-CALL-005 malformed ICE candidate".to_string())
}

fn is_internal_session_description(data: &str, expected_type: &str) -> bool {
    let token = data.trim();
    token.len() <= 16_384
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        && token.contains("sdp")
        && token.ends_with(expected_type)
}

fn is_internal_ice_candidate(candidate: &str) -> bool {
    candidate.len() <= 16_384
        && candidate
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        && candidate.contains("ice-candidate")
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
            data: r#"{"type":"offer","sdp":"v=0\r\n"}"#.to_owned(),
        };
        let mut ledger = CallSignalLedger::default();

        assert!(validate_signal(&signal, &mut ledger).is_ok());

        let empty = CallSignal {
            signal_type: CallSignalType::Ice,
            data: " ".to_owned(),
        };

        assert!(validate_signal(&empty, &mut ledger).is_err());
    }

    #[test]
    fn call_signal_rejects_duplicate_offer_before_answer() {
        let signal = CallSignal {
            signal_type: CallSignalType::Offer,
            data: r#"{"type":"offer","sdp":"v=0\r\n"}"#.to_owned(),
        };
        let mut ledger = CallSignalLedger::default();

        assert!(validate_signal(&signal, &mut ledger).is_ok());
        assert!(validate_signal(&signal, &mut ledger).is_err());
    }

    #[test]
    fn call_signal_rejects_stale_answer_without_offer() {
        let signal = CallSignal {
            signal_type: CallSignalType::Answer,
            data: r#"{"type":"answer","sdp":"v=0\r\n"}"#.to_owned(),
        };
        let mut ledger = CallSignalLedger::default();

        assert!(validate_signal(&signal, &mut ledger).is_err());
    }

    // ── Batch 12: Renegotiate marker semantics (§52) ─────────────────────

    const MARKER: &str = r#"{"request":"renegotiate","v":1,"id":"s1-abc123"}"#;
    const OFFER_DATA: &str = r#"{"type":"offer","sdp":"v=0\r\n"}"#;
    const ANSWER_DATA: &str = r#"{"type":"answer","sdp":"v=0\r\n"}"#;

    fn marker_signal() -> CallSignal {
        CallSignal {
            signal_type: CallSignalType::Renegotiate,
            data: MARKER.to_owned(),
        }
    }

    fn offer_signal() -> CallSignal {
        CallSignal {
            signal_type: CallSignalType::Offer,
            data: OFFER_DATA.to_owned(),
        }
    }

    fn answer_signal() -> CallSignal {
        CallSignal {
            signal_type: CallSignalType::Answer,
            data: ANSWER_DATA.to_owned(),
        }
    }

    #[test]
    fn renegotiate_marker_resets_a_never_completed_exchange() {
        // Session restart with a pending, unanswered offer: the fresh
        // session's next OFFER must validate (the marker supersedes the
        // stale exchange) instead of tripping MP-CALL-002.
        let mut ledger = CallSignalLedger::default();
        assert!(validate_signal(&offer_signal(), &mut ledger).is_ok());
        assert!(validate_signal(&marker_signal(), &mut ledger).is_ok());
        assert!(validate_signal(&offer_signal(), &mut ledger).is_ok());
    }

    #[test]
    fn renegotiate_marker_preserves_a_completed_exchange() {
        // A completed exchange already accepts a fresh OFFER; resetting it
        // would poison an ANSWER still in flight (MP-CALL-003).
        let mut ledger = CallSignalLedger::default();
        assert!(validate_signal(&offer_signal(), &mut ledger).is_ok());
        assert!(validate_signal(&answer_signal(), &mut ledger).is_ok());
        assert!(validate_signal(&marker_signal(), &mut ledger).is_ok());
        // The completed state is untouched: another ANSWER without a new
        // OFFER is still rejected (no state was cleared)...
        assert!(validate_signal(&answer_signal(), &mut ledger).is_err());
        // ...and a fresh OFFER proceeds as the normal renegotiation path.
        assert!(validate_signal(&offer_signal(), &mut ledger).is_ok());
    }

    #[test]
    fn renegotiate_marker_allows_answer_after_reset() {
        // Reset sequence: offer pending → marker clears it → fresh offer →
        // answer validates (the ANSWER belongs to the fresh exchange).
        let mut ledger = CallSignalLedger::default();
        assert!(validate_signal(&offer_signal(), &mut ledger).is_ok());
        assert!(validate_signal(&marker_signal(), &mut ledger).is_ok());
        assert!(validate_signal(&offer_signal(), &mut ledger).is_ok());
        assert!(validate_signal(&answer_signal(), &mut ledger).is_ok());
    }

    #[test]
    fn renegotiate_marker_is_idempotent() {
        // Mode-toggle thrash can produce several markers in a row; each is
        // a conditional reset and must remain valid (no error, no state
        // corruption).
        let mut ledger = CallSignalLedger::default();
        assert!(validate_signal(&offer_signal(), &mut ledger).is_ok());
        assert!(validate_signal(&marker_signal(), &mut ledger).is_ok());
        assert!(validate_signal(&marker_signal(), &mut ledger).is_ok());
        assert!(validate_signal(&marker_signal(), &mut ledger).is_ok());
        assert!(validate_signal(&offer_signal(), &mut ledger).is_ok());
    }

    #[test]
    fn renegotiate_marker_rejects_oversized_payload() {
        // The marker is a small request object (≤2 KiB); a session
        // description can never fit, so a mislabeled SDP is rejected.
        let oversized = CallSignal {
            signal_type: CallSignalType::Renegotiate,
            data: "x".repeat(2_049),
        };
        let mut ledger = CallSignalLedger::default();
        assert!(validate_signal(&oversized, &mut ledger).is_err());
    }

    #[test]
    fn renegotiate_marker_at_exact_limit_is_accepted() {
        let at_limit = CallSignal {
            signal_type: CallSignalType::Renegotiate,
            data: "x".repeat(2_048),
        };
        let mut ledger = CallSignalLedger::default();
        assert!(validate_signal(&at_limit, &mut ledger).is_ok());
    }
}
