use std::time::Instant;

pub type MonotonicInstant = Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockSample {
    pub t0_guest_us: i64,
    pub host_receive_us: i64,
    pub host_send_us: i64,
    pub t3_guest_us: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockEstimate {
    pub offset_to_host_us: i64,
    pub rtt_us: i64,
    pub sample_count: usize,
    pub quality: ClockQuality,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockQuality {
    Excellent,
    Good,
    Poor,
    Unusable,
}

pub fn estimate_offset(samples: &[ClockSample]) -> Option<ClockEstimate> {
    if samples.is_empty() {
        return None;
    }

    let mut scored: Vec<_> = samples
        .iter()
        .filter_map(|sample| {
            let rtt = (sample.t3_guest_us - sample.t0_guest_us)
                - (sample.host_send_us - sample.host_receive_us);
            if rtt < 0 {
                return None;
            }

            let offset = ((sample.host_receive_us - sample.t0_guest_us)
                + (sample.host_send_us - sample.t3_guest_us))
                / 2;
            Some((rtt, offset))
        })
        .collect();

    if scored.is_empty() {
        return None;
    }

    scored.sort_by_key(|(rtt, _)| *rtt);
    let keep = ((scored.len() as f32) * 0.8).ceil() as usize;
    scored.truncate(keep.max(1));
    scored.sort_by_key(|(_, offset)| *offset);

    let median_index = scored.len() / 2;
    let offset = scored[median_index].1;
    let mut rtts: Vec<_> = scored.iter().map(|(rtt, _)| *rtt).collect();
    rtts.sort_unstable();
    let rtt = rtts[median_index];

    Some(ClockEstimate {
        offset_to_host_us: offset,
        rtt_us: rtt,
        sample_count: samples.len(),
        quality: classify_quality(rtt, samples.len()),
    })
}

/// P95 round-trip time over measured samples (or `None` when there are no
/// usable samples). Used for playback lead-time sizing.
pub fn p95_rtt_us(samples: &[ClockSample]) -> Option<u64> {
    let mut rtts: Vec<u64> = samples
        .iter()
        .filter_map(|sample| {
            let rtt = (sample.t3_guest_us - sample.t0_guest_us)
                - (sample.host_send_us - sample.host_receive_us);
            (rtt >= 0).then_some(rtt as u64)
        })
        .collect();
    if rtts.is_empty() {
        return None;
    }
    rtts.sort_unstable();
    let index = ((rtts.len() as f64) * 0.95).ceil() as usize;
    Some(rtts[index.saturating_sub(1)])
}

/// Playback lead time (MASTER_PRD §19):
/// `lead = max(750ms, 2 × p95 RTT + 250ms)`, clamped to 3 seconds.
/// Without measured RTT the minimum 750ms lead applies.
pub fn play_lead_us(rtt_p95_us: Option<u64>) -> u64 {
    let base = match rtt_p95_us {
        Some(rtt) => 2 * rtt + 250_000,
        None => 750_000,
    };
    base.clamp(750_000, 3_000_000)
}

fn classify_quality(rtt_us: i64, sample_count: usize) -> ClockQuality {
    if sample_count < 4 {
        return ClockQuality::Unusable;
    }

    match rtt_us {
        0..=50_000 => ClockQuality::Excellent,
        50_001..=150_000 => ClockQuality::Good,
        150_001..=350_000 => ClockQuality::Poor,
        _ => ClockQuality::Unusable,
    }
}

#[cfg(test)]
mod tests {
    use super::{estimate_offset, p95_rtt_us, play_lead_us, ClockQuality, ClockSample};

    #[test]
    fn estimates_median_offset_from_best_samples() {
        let samples = (0..20)
            .map(|i| ClockSample {
                t0_guest_us: i * 100_000,
                host_receive_us: i * 100_000 + 40_000,
                host_send_us: i * 100_000 + 41_000,
                t3_guest_us: i * 100_000 + 21_000,
            })
            .collect::<Vec<_>>();

        let estimate = estimate_offset(&samples).expect("estimate");
        assert_eq!(estimate.offset_to_host_us, 30_000);
        assert_eq!(estimate.quality, ClockQuality::Excellent);
    }

    #[test]
    fn p95_rtt_returns_expected_percentile() {
        let samples = (1..=100u64)
            .map(|i| ClockSample {
                t0_guest_us: 0,
                host_receive_us: 0,
                host_send_us: 0,
                t3_guest_us: (i * 1_000) as i64,
            })
            .collect::<Vec<_>>();
        let p95 = p95_rtt_us(&samples).expect("p95");
        assert_eq!(p95, 95_000);
    }

    #[test]
    fn play_lead_uses_formula_from_prd() {
        assert_eq!(play_lead_us(None), 750_000);
        assert_eq!(play_lead_us(Some(100_000)), 750_000);
        assert_eq!(play_lead_us(Some(1_000_000)), 2_250_000);
        assert_eq!(play_lead_us(Some(10_000_000)), 3_000_000);
    }
}
