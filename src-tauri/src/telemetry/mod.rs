pub const DEFAULT_LOG_LEVEL: &str = "INFO";

/// Production/dev build default log level from MASTER_PRD §87.
/// Initialize the local structured logging pipeline exactly once. This is a
/// LOCAL-ONLY stdout subscriber — telemetry is never sent to a cloud service.
///
/// The effective level comes from the environment (`RUST_LOG`) when set, and
/// otherwise from [`DEFAULT_LOG_LEVEL`], so a QA machine can raise verbosity
/// without a rebuild. Per PRD §86/§87 no secrets, cookies, credentials,
/// media paths, or full chat content are ever logged by this pipeline.
pub fn init_local_logging() {
    use tracing_subscriber::EnvFilter;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_LEVEL)),
        )
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::Uptime::default())
        .try_init();
    // try_init is intentionally infallible here: a second init (tests, dual
    // run() calls) is a no-op rather than a startup failure.
}

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

pub mod beta {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum BetaEventKind {
        TrustedFriendInstall,
        BugReport,
        DiagnosticBundleExport,
        RealMovieNight,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct BetaEvent {
        pub kind: BetaEventKind,
        pub notes: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct DiagnosticBundle {
        pub app_version: String,
        pub platform: String,
        pub beta_events: Vec<BetaEvent>,
        pub redacted_log_lines: Vec<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum BetaReadiness {
        ExternalVerificationPending { missing_events: Vec<BetaEventKind> },
        ReadyForManualReview,
    }

    pub fn redact_log_line(line: &str) -> String {
        line.split_whitespace()
            .map(|token| {
                let lower = token.to_ascii_lowercase();
                if lower.contains("token=")
                    || lower.contains("cookie=")
                    || lower.contains("password=")
                    || lower.contains("authorization=")
                {
                    "[REDACTED]".to_string()
                } else {
                    token.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn build_diagnostic_bundle(
        app_version: impl Into<String>,
        platform: impl Into<String>,
        beta_events: Vec<BetaEvent>,
        raw_log_lines: &[String],
    ) -> DiagnosticBundle {
        DiagnosticBundle {
            app_version: app_version.into(),
            platform: platform.into(),
            beta_events,
            redacted_log_lines: raw_log_lines
                .iter()
                .map(|line| redact_log_line(line))
                .collect(),
        }
    }

    pub fn beta_readiness(events: &[BetaEvent]) -> BetaReadiness {
        let required = [
            BetaEventKind::TrustedFriendInstall,
            BetaEventKind::DiagnosticBundleExport,
            BetaEventKind::RealMovieNight,
        ];

        let missing_events = required
            .into_iter()
            .filter(|required_kind| !events.iter().any(|event| event.kind == *required_kind))
            .collect::<Vec<_>>();

        if missing_events.is_empty() {
            BetaReadiness::ReadyForManualReview
        } else {
            BetaReadiness::ExternalVerificationPending { missing_events }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn diagnostic_bundle_redacts_common_secret_tokens() {
            let bundle = build_diagnostic_bundle(
                "0.1.0",
                "macos",
                vec![BetaEvent {
                    kind: BetaEventKind::DiagnosticBundleExport,
                    notes: "manual export".to_string(),
                }],
                &[
                    "room_id=abc token=secret cookie=session".to_string(),
                    "authorization=Bearer password=hunter2 sync_drift_ms=32".to_string(),
                ],
            );

            assert_eq!(
                bundle.redacted_log_lines,
                vec![
                    "room_id=abc [REDACTED] [REDACTED]".to_string(),
                    "[REDACTED] [REDACTED] sync_drift_ms=32".to_string(),
                ]
            );
        }

        #[test]
        fn beta_readiness_requires_external_install_bundle_and_movie_night() {
            let readiness = beta_readiness(&[BetaEvent {
                kind: BetaEventKind::DiagnosticBundleExport,
                notes: "exported locally".to_string(),
            }]);

            assert_eq!(
                readiness,
                BetaReadiness::ExternalVerificationPending {
                    missing_events: vec![
                        BetaEventKind::TrustedFriendInstall,
                        BetaEventKind::RealMovieNight,
                    ],
                }
            );
        }

        #[test]
        fn bug_reports_are_collected_without_being_required_for_initial_readiness() {
            let events = vec![
                BetaEvent {
                    kind: BetaEventKind::TrustedFriendInstall,
                    notes: "trusted friend device".to_string(),
                },
                BetaEvent {
                    kind: BetaEventKind::DiagnosticBundleExport,
                    notes: "manual diagnostics".to_string(),
                },
                BetaEvent {
                    kind: BetaEventKind::RealMovieNight,
                    notes: "movie night completed".to_string(),
                },
                BetaEvent {
                    kind: BetaEventKind::BugReport,
                    notes: "chat overlay polish request".to_string(),
                },
            ];

            assert_eq!(beta_readiness(&events), BetaReadiness::ReadyForManualReview);
        }
    }
}

#[cfg(test)]
mod logging_tests {
    use super::*;

    #[test]
    fn default_log_level_matches_prd_section_87() {
        // MASTER_PRD §87: production/dev default is INFO.
        assert_eq!(DEFAULT_LOG_LEVEL, "INFO");
    }

    #[test]
    fn local_logging_init_is_idempotent_and_never_panics() {
        // The subscriber is process-global; double init (app startup plus a
        // test) must be a no-op, not a startup failure.
        init_local_logging();
        init_local_logging();
        // A real warn! through the installed subscriber proves the pipeline
        // swallows events without panicking (local stdout only, no network).
        tracing::warn!("telemetry self-check event");
    }
}
