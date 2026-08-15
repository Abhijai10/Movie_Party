pub const DEFAULT_LOG_LEVEL: &str = "INFO";

pub mod certification {
    use crate::network::tailscale::TailscalePath;

    pub const COLLEGE_NETWORK_TARGET_GOODPUT_BPS: u64 = 10_000_000;
    pub const MAX_CERTIFICATION_DRIFT_MS: u32 = 150;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub enum MovieSize {
        TwoGb,
        FourGb,
        EightGb,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub enum CallMode {
        Off,
        VoiceOnly,
        Video,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ProviderSharedScenario {
        ProviderCombination {
            host_provider: String,
            guest_provider: String,
        },
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum CertificationScenario {
        LocalMovie {
            movie_size: MovieSize,
            call_mode: CallMode,
        },
        ProviderShared(ProviderSharedScenario),
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct CertificationSample {
        pub actual_goodput_bps: u64,
        pub buffering_events: u32,
        pub max_sync_drift_ms: u32,
        pub tailscale_path: TailscalePath,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct CertificationRecord {
        pub scenario: CertificationScenario,
        pub sample: CertificationSample,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ScenarioOutcome {
        Pass,
        NeedsTuning {
            low_goodput: bool,
            buffering_observed: bool,
            excessive_drift: bool,
            relayed_or_unknown_path: bool,
        },
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ScenarioSummary {
        pub scenario: CertificationScenario,
        pub outcome: ScenarioOutcome,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum CertificationReadiness {
        ExternalVerificationPending {
            missing_local_movie_runs: Vec<(MovieSize, CallMode)>,
        },
        MeasurementsReady {
            summaries: Vec<ScenarioSummary>,
        },
    }

    pub fn required_local_movie_runs() -> Vec<(MovieSize, CallMode)> {
        let sizes = [MovieSize::TwoGb, MovieSize::FourGb, MovieSize::EightGb];
        let call_modes = [CallMode::Off, CallMode::VoiceOnly, CallMode::Video];

        sizes
            .into_iter()
            .flat_map(|movie_size| call_modes.map(move |call_mode| (movie_size, call_mode)))
            .collect()
    }

    pub fn summarize_record(record: &CertificationRecord) -> ScenarioSummary {
        let low_goodput = record.sample.actual_goodput_bps < COLLEGE_NETWORK_TARGET_GOODPUT_BPS;
        let buffering_observed = record.sample.buffering_events > 0;
        let excessive_drift = record.sample.max_sync_drift_ms > MAX_CERTIFICATION_DRIFT_MS;
        let relayed_or_unknown_path =
            !matches!(record.sample.tailscale_path, TailscalePath::Direct);

        let outcome =
            if low_goodput || buffering_observed || excessive_drift || relayed_or_unknown_path {
                ScenarioOutcome::NeedsTuning {
                    low_goodput,
                    buffering_observed,
                    excessive_drift,
                    relayed_or_unknown_path,
                }
            } else {
                ScenarioOutcome::Pass
            };

        ScenarioSummary {
            scenario: record.scenario.clone(),
            outcome,
        }
    }

    pub fn summarize_certification(records: &[CertificationRecord]) -> CertificationReadiness {
        let missing_local_movie_runs = required_local_movie_runs()
            .into_iter()
            .filter(|(expected_size, expected_call_mode)| {
                !records.iter().any(|record| {
                    matches!(
                        record.scenario,
                        CertificationScenario::LocalMovie {
                            movie_size,
                            call_mode
                        } if movie_size == *expected_size && call_mode == *expected_call_mode
                    )
                })
            })
            .collect::<Vec<_>>();

        if !missing_local_movie_runs.is_empty() {
            return CertificationReadiness::ExternalVerificationPending {
                missing_local_movie_runs,
            };
        }

        CertificationReadiness::MeasurementsReady {
            summaries: records.iter().map(summarize_record).collect(),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn passing_sample() -> CertificationSample {
            CertificationSample {
                actual_goodput_bps: 12_000_000,
                buffering_events: 0,
                max_sync_drift_ms: 90,
                tailscale_path: TailscalePath::Direct,
            }
        }

        fn local_record(movie_size: MovieSize, call_mode: CallMode) -> CertificationRecord {
            CertificationRecord {
                scenario: CertificationScenario::LocalMovie {
                    movie_size,
                    call_mode,
                },
                sample: passing_sample(),
            }
        }

        #[test]
        fn phase_29_requires_every_movie_size_and_call_mode_combination() {
            let readiness =
                summarize_certification(&[local_record(MovieSize::TwoGb, CallMode::Off)]);

            assert_eq!(
                readiness,
                CertificationReadiness::ExternalVerificationPending {
                    missing_local_movie_runs: vec![
                        (MovieSize::TwoGb, CallMode::VoiceOnly),
                        (MovieSize::TwoGb, CallMode::Video),
                        (MovieSize::FourGb, CallMode::Off),
                        (MovieSize::FourGb, CallMode::VoiceOnly),
                        (MovieSize::FourGb, CallMode::Video),
                        (MovieSize::EightGb, CallMode::Off),
                        (MovieSize::EightGb, CallMode::VoiceOnly),
                        (MovieSize::EightGb, CallMode::Video),
                    ],
                }
            );
        }

        #[test]
        fn complete_measurement_matrix_is_summarized_without_claiming_provider_coverage() {
            let records = required_local_movie_runs()
                .into_iter()
                .map(|(movie_size, call_mode)| local_record(movie_size, call_mode))
                .collect::<Vec<_>>();

            let readiness = summarize_certification(&records);

            assert!(matches!(
                readiness,
                CertificationReadiness::MeasurementsReady { summaries } if summaries.len() == 9
            ));
        }

        #[test]
        fn poor_measurement_flags_tuning_inputs() {
            let record = CertificationRecord {
                scenario: CertificationScenario::LocalMovie {
                    movie_size: MovieSize::EightGb,
                    call_mode: CallMode::Video,
                },
                sample: CertificationSample {
                    actual_goodput_bps: 7_000_000,
                    buffering_events: 2,
                    max_sync_drift_ms: 260,
                    tailscale_path: TailscalePath::DerpRelay,
                },
            };

            assert_eq!(
                summarize_record(&record).outcome,
                ScenarioOutcome::NeedsTuning {
                    low_goodput: true,
                    buffering_observed: true,
                    excessive_drift: true,
                    relayed_or_unknown_path: true,
                }
            );
        }
    }
}

pub mod regression {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub enum DesktopPlatform {
        Windows,
        Macos,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct PlatformPair {
        pub host: DesktopPlatform,
        pub guest: DesktopPlatform,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum CoreScenario {
        LocalPerfect,
        LocalPreloaded,
        ProviderSync,
        ProviderShared,
        CallVoice,
        CallVideo,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum RegressionOutcome {
        Passed,
        Failed,
        ExternalVerificationPending,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct RegressionRecord {
        pub pair: PlatformPair,
        pub scenario: CoreScenario,
        pub outcome: RegressionOutcome,
        pub notes: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ReleaseCandidateGate {
        Closed {
            missing_or_failed_pairs: Vec<PlatformPair>,
        },
        Open,
    }

    pub fn required_platform_pairs() -> Vec<PlatformPair> {
        use DesktopPlatform::{Macos, Windows};

        vec![
            PlatformPair {
                host: Windows,
                guest: Windows,
            },
            PlatformPair {
                host: Windows,
                guest: Macos,
            },
            PlatformPair {
                host: Macos,
                guest: Windows,
            },
            PlatformPair {
                host: Macos,
                guest: Macos,
            },
        ]
    }

    pub fn release_candidate_gate(records: &[RegressionRecord]) -> ReleaseCandidateGate {
        let missing_or_failed_pairs = required_platform_pairs()
            .into_iter()
            .filter(|pair| {
                !records.iter().any(|record| {
                    record.pair == *pair
                        && record.scenario == CoreScenario::LocalPerfect
                        && record.outcome == RegressionOutcome::Passed
                })
            })
            .collect::<Vec<_>>();

        if missing_or_failed_pairs.is_empty() {
            ReleaseCandidateGate::Open
        } else {
            ReleaseCandidateGate::Closed {
                missing_or_failed_pairs,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn passed_local_perfect(pair: PlatformPair) -> RegressionRecord {
            RegressionRecord {
                pair,
                scenario: CoreScenario::LocalPerfect,
                outcome: RegressionOutcome::Passed,
                notes: "verified manually".to_string(),
            }
        }

        #[test]
        fn phase_30_requires_all_windows_and_macos_pairings() {
            let pairs = required_platform_pairs();

            assert_eq!(pairs.len(), 4);
            assert!(pairs.contains(&PlatformPair {
                host: DesktopPlatform::Windows,
                guest: DesktopPlatform::Windows,
            }));
            assert!(pairs.contains(&PlatformPair {
                host: DesktopPlatform::Windows,
                guest: DesktopPlatform::Macos,
            }));
            assert!(pairs.contains(&PlatformPair {
                host: DesktopPlatform::Macos,
                guest: DesktopPlatform::Windows,
            }));
            assert!(pairs.contains(&PlatformPair {
                host: DesktopPlatform::Macos,
                guest: DesktopPlatform::Macos,
            }));
        }

        #[test]
        fn release_candidate_gate_stays_closed_without_every_local_perfect_pass() {
            let records = vec![passed_local_perfect(PlatformPair {
                host: DesktopPlatform::Macos,
                guest: DesktopPlatform::Macos,
            })];

            assert_eq!(
                release_candidate_gate(&records),
                ReleaseCandidateGate::Closed {
                    missing_or_failed_pairs: vec![
                        PlatformPair {
                            host: DesktopPlatform::Windows,
                            guest: DesktopPlatform::Windows,
                        },
                        PlatformPair {
                            host: DesktopPlatform::Windows,
                            guest: DesktopPlatform::Macos,
                        },
                        PlatformPair {
                            host: DesktopPlatform::Macos,
                            guest: DesktopPlatform::Windows,
                        },
                    ],
                }
            );
        }

        #[test]
        fn release_candidate_gate_opens_after_every_local_perfect_pair_passes() {
            let records = required_platform_pairs()
                .into_iter()
                .map(passed_local_perfect)
                .collect::<Vec<_>>();

            assert_eq!(release_candidate_gate(&records), ReleaseCandidateGate::Open);
        }
    }
}
