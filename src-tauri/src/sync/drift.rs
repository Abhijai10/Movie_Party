pub const NORMAL_DRIFT_TARGET_MS: u16 = 100;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DriftCorrection {
    Ignore,
    PlaybackRate { rate: f32 },
    MicroSeek { target_position_ms: i64 },
    HardSeek { target_position_ms: i64 },
}

/// `drift_ms` is local player position minus the canonical host position.
pub fn correction_for_drift(drift_ms: i64, current_position_ms: i64) -> DriftCorrection {
    let magnitude = drift_ms.unsigned_abs();

    match magnitude {
        0..=80 => DriftCorrection::Ignore,
        81..=250 => {
            let rate = if drift_ms > 0 { 0.97 } else { 1.03 };
            DriftCorrection::PlaybackRate { rate }
        }
        251..=700 => DriftCorrection::MicroSeek {
            target_position_ms: current_position_ms - drift_ms,
        },
        _ => DriftCorrection::HardSeek {
            target_position_ms: current_position_ms - drift_ms,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{correction_for_drift, DriftCorrection};

    #[test]
    fn applies_locked_drift_thresholds() {
        assert_eq!(correction_for_drift(40, 1_000), DriftCorrection::Ignore);
        assert_eq!(
            correction_for_drift(120, 1_000),
            DriftCorrection::PlaybackRate { rate: 0.97 },
        );
        assert_eq!(correction_for_drift(-500, 1_000), DriftCorrection::MicroSeek { target_position_ms: 1_500 });
        assert_eq!(
            correction_for_drift(900, 1_000),
            DriftCorrection::HardSeek {
                target_position_ms: 100
            },
        );
    }
}
