pub mod macos;
pub mod windows;

use crate::call::CameraTier;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderPlatform {
    WindowsMediaFoundation,
    MacosVideoToolbox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H264Profile {
    P1080High,
    P1080Medium,
    P720High,
    P720Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderConfig {
    pub profile: H264Profile,
    pub width: u16,
    pub height: u16,
    pub fps: u8,
    pub target_bitrate_bps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EncoderBenchmarkSample {
    pub capture_to_encode_latency_ms: f32,
    pub cpu_percent: f32,
    pub gpu_percent: f32,
    pub achieved_fps: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchmarkStatus {
    Pass,
    FailsLatency,
    FailsFrameRate,
    ExternalVerificationPending,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutomaticQualityInput {
    pub measured_goodput_bps: u64,
    pub buffer_level_us: u64,
    pub encoder_sample: EncoderBenchmarkSample,
    pub call_bitrate_bps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomaticQualityDecision {
    pub movie: EncoderConfig,
    pub camera_tier: CameraTier,
}

pub fn encoder_api(platform: EncoderPlatform) -> &'static str {
    match platform {
        EncoderPlatform::WindowsMediaFoundation => windows::H264_ENCODER_API,
        EncoderPlatform::MacosVideoToolbox => macos::H264_ENCODER_API,
    }
}

pub fn config_for_profile(profile: H264Profile) -> EncoderConfig {
    match profile {
        H264Profile::P1080High => EncoderConfig {
            profile,
            width: 1920,
            height: 1080,
            fps: 30,
            target_bitrate_bps: 5_500_000,
        },
        H264Profile::P1080Medium => EncoderConfig {
            profile,
            width: 1920,
            height: 1080,
            fps: 30,
            target_bitrate_bps: 4_000_000,
        },
        H264Profile::P720High => EncoderConfig {
            profile,
            width: 1280,
            height: 720,
            fps: 30,
            target_bitrate_bps: 3_000_000,
        },
        H264Profile::P720Low => EncoderConfig {
            profile,
            width: 1280,
            height: 720,
            fps: 30,
            target_bitrate_bps: 2_000_000,
        },
    }
}

pub fn select_profile_for_goodput(measured_goodput_bps: u64) -> EncoderConfig {
    let profile = if measured_goodput_bps >= 7_000_000 {
        H264Profile::P1080High
    } else if measured_goodput_bps >= 5_000_000 {
        H264Profile::P1080Medium
    } else if measured_goodput_bps >= 3_800_000 {
        H264Profile::P720High
    } else {
        H264Profile::P720Low
    };

    config_for_profile(profile)
}

pub fn classify_benchmark(sample: EncoderBenchmarkSample) -> BenchmarkStatus {
    if sample.capture_to_encode_latency_ms > 120.0 {
        return BenchmarkStatus::FailsLatency;
    }

    if sample.achieved_fps < 27.0 {
        return BenchmarkStatus::FailsFrameRate;
    }

    BenchmarkStatus::Pass
}

pub fn automatic_quality_decision(
    current_movie: H264Profile,
    current_camera: CameraTier,
    input: AutomaticQualityInput,
) -> AutomaticQualityDecision {
    if input.buffer_level_us < 2_000_000
        || classify_benchmark(input.encoder_sample) != BenchmarkStatus::Pass
    {
        return AutomaticQualityDecision {
            movie: config_for_profile(H264Profile::P720Low),
            camera_tier: CameraTier::D,
        };
    }

    let current_movie_config = config_for_profile(current_movie);
    let camera_pressure_budget = current_movie_config.target_bitrate_bps as u64
        + u64::from(input.call_bitrate_bps)
        + 500_000;

    if input.measured_goodput_bps < camera_pressure_budget
        && input.measured_goodput_bps >= u64::from(current_movie_config.target_bitrate_bps)
    {
        return AutomaticQualityDecision {
            movie: current_movie_config,
            camera_tier: downgrade_camera(current_camera),
        };
    }

    if input.measured_goodput_bps < u64::from(current_movie_config.target_bitrate_bps) {
        return AutomaticQualityDecision {
            movie: select_profile_for_goodput(input.measured_goodput_bps),
            camera_tier: CameraTier::D,
        };
    }

    if input.buffer_level_us >= 10_000_000
        && input.measured_goodput_bps >= camera_pressure_budget + 2_000_000
    {
        return AutomaticQualityDecision {
            movie: select_profile_for_goodput(input.measured_goodput_bps),
            camera_tier: upgrade_camera(current_camera),
        };
    }

    AutomaticQualityDecision {
        movie: current_movie_config,
        camera_tier: current_camera,
    }
}

fn downgrade_camera(tier: CameraTier) -> CameraTier {
    match tier {
        CameraTier::A => CameraTier::B,
        CameraTier::B => CameraTier::C,
        CameraTier::C | CameraTier::D => CameraTier::D,
    }
}

fn upgrade_camera(tier: CameraTier) -> CameraTier {
    match tier {
        CameraTier::A => CameraTier::A,
        CameraTier::B => CameraTier::A,
        CameraTier::C => CameraTier::B,
        CameraTier::D => CameraTier::C,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_encoder_apis_match_locked_architecture() {
        assert_eq!(
            encoder_api(EncoderPlatform::WindowsMediaFoundation),
            "Media Foundation"
        );
        assert_eq!(
            encoder_api(EncoderPlatform::MacosVideoToolbox),
            "VideoToolbox"
        );
    }

    #[test]
    fn bitrate_ladder_uses_v1_30fps_profiles() {
        let profiles = [
            (H264Profile::P1080High, 1920, 1080, 5_500_000),
            (H264Profile::P1080Medium, 1920, 1080, 4_000_000),
            (H264Profile::P720High, 1280, 720, 3_000_000),
            (H264Profile::P720Low, 1280, 720, 2_000_000),
        ];

        for (profile, width, height, bitrate) in profiles {
            let config = config_for_profile(profile);

            assert_eq!(config.width, width);
            assert_eq!(config.height, height);
            assert_eq!(config.fps, 30);
            assert_eq!(config.target_bitrate_bps, bitrate);
        }
    }

    #[test]
    fn encoder_profile_responds_to_measured_goodput() {
        assert_eq!(
            select_profile_for_goodput(7_200_000).profile,
            H264Profile::P1080High
        );
        assert_eq!(
            select_profile_for_goodput(5_100_000).profile,
            H264Profile::P1080Medium
        );
        assert_eq!(
            select_profile_for_goodput(3_900_000).profile,
            H264Profile::P720High
        );
        assert_eq!(
            select_profile_for_goodput(2_500_000).profile,
            H264Profile::P720Low
        );
    }

    #[test]
    fn benchmark_classification_tracks_latency_and_fps() {
        assert_eq!(
            classify_benchmark(EncoderBenchmarkSample {
                capture_to_encode_latency_ms: 65.0,
                cpu_percent: 20.0,
                gpu_percent: 35.0,
                achieved_fps: 29.7,
            }),
            BenchmarkStatus::Pass
        );
        assert_eq!(
            classify_benchmark(EncoderBenchmarkSample {
                capture_to_encode_latency_ms: 140.0,
                cpu_percent: 20.0,
                gpu_percent: 35.0,
                achieved_fps: 29.7,
            }),
            BenchmarkStatus::FailsLatency
        );
        assert_eq!(
            classify_benchmark(EncoderBenchmarkSample {
                capture_to_encode_latency_ms: 65.0,
                cpu_percent: 20.0,
                gpu_percent: 35.0,
                achieved_fps: 23.0,
            }),
            BenchmarkStatus::FailsFrameRate
        );
    }

    #[test]
    fn automatic_quality_reduces_camera_before_movie_when_movie_budget_is_available() {
        let decision = automatic_quality_decision(
            H264Profile::P1080Medium,
            CameraTier::B,
            AutomaticQualityInput {
                measured_goodput_bps: 4_100_000,
                buffer_level_us: 8_000_000,
                encoder_sample: EncoderBenchmarkSample {
                    capture_to_encode_latency_ms: 60.0,
                    cpu_percent: 20.0,
                    gpu_percent: 40.0,
                    achieved_fps: 29.7,
                },
                call_bitrate_bps: 350_000,
            },
        );

        assert_eq!(decision.movie.profile, H264Profile::P1080Medium);
        assert_eq!(decision.camera_tier, CameraTier::C);
    }

    #[test]
    fn automatic_quality_reduces_movie_only_when_movie_budget_is_unsafe() {
        let decision = automatic_quality_decision(
            H264Profile::P1080High,
            CameraTier::A,
            AutomaticQualityInput {
                measured_goodput_bps: 3_100_000,
                buffer_level_us: 8_000_000,
                encoder_sample: EncoderBenchmarkSample {
                    capture_to_encode_latency_ms: 60.0,
                    cpu_percent: 20.0,
                    gpu_percent: 40.0,
                    achieved_fps: 29.7,
                },
                call_bitrate_bps: 350_000,
            },
        );

        assert_eq!(decision.movie.profile, H264Profile::P720Low);
        assert_eq!(decision.camera_tier, CameraTier::D);
    }

    #[test]
    fn automatic_quality_recovers_movie_and_camera_when_conditions_are_healthy() {
        let decision = automatic_quality_decision(
            H264Profile::P720High,
            CameraTier::C,
            AutomaticQualityInput {
                measured_goodput_bps: 7_500_000,
                buffer_level_us: 15_000_000,
                encoder_sample: EncoderBenchmarkSample {
                    capture_to_encode_latency_ms: 60.0,
                    cpu_percent: 20.0,
                    gpu_percent: 40.0,
                    achieved_fps: 29.7,
                },
                call_bitrate_bps: 250_000,
            },
        );

        assert_eq!(decision.movie.profile, H264Profile::P1080High);
        assert_eq!(decision.camera_tier, CameraTier::B);
    }
}
