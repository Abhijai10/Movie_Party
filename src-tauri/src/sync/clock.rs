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
    use super::{estimate_offset, ClockQuality, ClockSample};

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
}
