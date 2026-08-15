pub const DEFAULT_GOODPUT_SAMPLE_COUNT: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreloadRecommendation {
    StreamSoon { minimum_start_buffer_secs: u64 },
    SmartPreload { minimum_start_buffer_secs: u64 },
    DownloadFirst,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GoodputEstimator {
    samples_bps: Vec<u64>,
    max_samples: usize,
}

impl GoodputEstimator {
    pub fn new(max_samples: usize) -> Self {
        Self {
            samples_bps: Vec::new(),
            max_samples: max_samples.max(1),
        }
    }

    pub fn push_sample(&mut self, bytes: u64, elapsed_ms: u64) {
        if elapsed_ms == 0 {
            return;
        }

        let bps = bytes.saturating_mul(8).saturating_mul(1_000) / elapsed_ms;
        self.samples_bps.push(bps);
        if self.samples_bps.len() > self.max_samples {
            self.samples_bps.remove(0);
        }
    }

    pub fn conservative_goodput_bps(&self) -> Option<u64> {
        if self.samples_bps.is_empty() {
            return None;
        }

        let mut samples = self.samples_bps.clone();
        samples.sort_unstable();
        let lower_quartile = samples[(samples.len() - 1) / 4];
        Some(lower_quartile)
    }
}

impl Default for GoodputEstimator {
    fn default() -> Self {
        Self::new(DEFAULT_GOODPUT_SAMPLE_COUNT)
    }
}

pub fn recommend_preload(
    measured_goodput_bps: u64,
    estimated_media_bitrate_bps: u64,
) -> PreloadRecommendation {
    if estimated_media_bitrate_bps == 0 {
        return PreloadRecommendation::DownloadFirst;
    }

    let ratio = measured_goodput_bps as f64 / estimated_media_bitrate_bps as f64;
    if ratio >= 2.0 {
        PreloadRecommendation::StreamSoon {
            minimum_start_buffer_secs: 60,
        }
    } else if ratio >= 1.5 {
        PreloadRecommendation::SmartPreload {
            minimum_start_buffer_secs: 120,
        }
    } else if ratio >= 1.2 {
        PreloadRecommendation::SmartPreload {
            minimum_start_buffer_secs: 300,
        }
    } else if ratio >= 1.05 {
        PreloadRecommendation::SmartPreload {
            minimum_start_buffer_secs: 900,
        }
    } else {
        PreloadRecommendation::DownloadFirst
    }
}

#[cfg(test)]
mod tests {
    use super::{recommend_preload, GoodputEstimator, PreloadRecommendation};

    #[test]
    fn uses_conservative_goodput_sample() {
        let mut estimator = GoodputEstimator::new(4);
        estimator.push_sample(1_000_000, 1_000);
        estimator.push_sample(2_000_000, 1_000);
        estimator.push_sample(3_000_000, 1_000);
        estimator.push_sample(4_000_000, 1_000);

        assert_eq!(estimator.conservative_goodput_bps(), Some(8_000_000));
    }

    #[test]
    fn applies_locked_preload_thresholds() {
        assert_eq!(
            recommend_preload(8_000_000, 4_000_000),
            PreloadRecommendation::StreamSoon {
                minimum_start_buffer_secs: 60
            },
        );
        assert_eq!(
            recommend_preload(5_000_000, 4_000_000),
            PreloadRecommendation::SmartPreload {
                minimum_start_buffer_secs: 300
            },
        );
        assert_eq!(
            recommend_preload(3_000_000, 4_000_000),
            PreloadRecommendation::DownloadFirst,
        );
    }
}
