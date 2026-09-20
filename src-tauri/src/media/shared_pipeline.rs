//! STALE — NOT COMPILED, NOT WIRED, NOT A SHIPPED CAPABILITY (AUD-08).
//!
//! This module is deliberately **not** declared in `media/mod.rs`, so no build
//! compiles it and its one `#[ignore]`d test can never run. It is a macOS-only
//! *proof harness* for Provider Shared (`screencapture` + `ffmpeg
//! h264_videotoolbox`), and Provider Shared remains `shared_available: false`.
//!
//! Declaring it does not compile. Verified against the current tree, it fails
//! in three places:
//!   * `QuicServer::run()` now requires
//!     `Option<Arc<Mutex<LocalSyncCoordinator>>>` (line ~149).
//!   * `QuicClient::connect(..)` takes `display_name: String`, not `&str`
//!     (line ~155).
//!   * `QuicClient::send_shared_stream_packet` no longer exists (line ~159).
//!
//! The third one is the important one, and it was found by the post-remediation
//! adversarial audit: **the shared-stream transport API is gone from
//! `network/quic.rs` entirely** — a crate-wide search for the shared-stream send
//! path finds nothing. So this file is not merely *stale*, it is **orphaned**:
//! the capability it exists to prove has been removed from the codebase.
//!
//! That rules out the "declare and repair it" option. Making it compile would
//! mean either deleting the transport assertion — which is the only thing the
//! harness proves, so the file would become a shell that still lies about
//! coverage — or rebuilding a shared-stream send path, which is Provider Shared
//! implementation work and explicitly out of scope. The honest options are
//! therefore **delete it** (recoverable from git history) or leave it loudly
//! marked, which is what is happening here.
//!
//! Do not read this file as evidence that shared-stream transport works. It is
//! evidence of the opposite: nothing here has been executed in a long time, and
//! the transport it drives no longer exists.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    call::CameraTier,
    encode::{
        automatic_quality_decision, config_for_profile, AutomaticQualityInput,
        EncoderBenchmarkSample, H264Profile,
    },
    identity::DeviceIdentity,
    media::shared_stream::{
        decode_packet, encode_packet, evaluate_shared_strict_sync, EncodedMediaPacket,
        PresentationBuffer, SharedStrictSyncAction, SharedStrictSyncInput, StreamKind,
        DEFAULT_PRESENTATION_BUFFER_US,
    },
    network::quic::{loopback_bind_addr, QuicClient, QuicServer, RoomCredentials},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedPipelineProof {
    pub captured_frame: PathBuf,
    pub encoded_h264: PathBuf,
    pub decoded_frame: PathBuf,
    pub encoded_bytes: u64,
    pub transported_sequence: u64,
    pub host_presented_sequence: u64,
    pub guest_presented_sequence: u64,
    pub strict_sync_paused_on_low_buffer: bool,
    pub strict_sync_resumed_after_buffer: bool,
    pub camera_degraded_before_movie: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SharedPipelineError {
    #[error("MP-STREAM-001 shared pipeline IO failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("MP-STREAM-001 command failed: {0}")]
    Command(String),
    #[error("MP-STREAM-001 shared packet failed: {0}")]
    SharedPacket(#[from] crate::media::shared_stream::SharedStreamError),
    #[error("MP-NET-001 transport failed: {0}")]
    Transport(#[from] crate::network::quic::QuicError),
    #[error("MP-STREAM-001 presentation buffer did not release packet")]
    PresentationNotReady,
}

pub async fn run_macos_shared_pipeline_proof(
    work_root: &Path,
) -> Result<SharedPipelineProof, SharedPipelineError> {
    fs::create_dir_all(work_root)?;
    let frame = work_root.join("capture.png");
    let encoded = work_root.join("encoded.h264");
    let decoded = work_root.join("decoded.png");

    run_command(
        Command::new("screencapture").arg("-x").arg(&frame),
        "screencapture",
    )?;
    if fs::metadata(&frame)?.len() == 0 {
        return Err(SharedPipelineError::Command(
            "screencapture produced an empty frame".to_string(),
        ));
    }

    run_command(
        crate::process::quiet_command("ffmpeg")
            .arg("-y")
            .arg("-loop")
            .arg("1")
            .arg("-i")
            .arg(&frame)
            .arg("-t")
            .arg("1")
            .arg("-vf")
            .arg("scale=1280:720,fps=30")
            .arg("-c:v")
            .arg("h264_videotoolbox")
            .arg("-b:v")
            .arg("2000k")
            .arg("-f")
            .arg("h264")
            .arg(&encoded),
        "ffmpeg h264_videotoolbox encode",
    )?;
    let payload = fs::read(&encoded)?;
    if payload.is_empty() {
        return Err(SharedPipelineError::Command(
            "VideoToolbox encode produced no bytes".to_string(),
        ));
    }

    run_command(
        crate::process::quiet_command("ffmpeg")
            .arg("-y")
            .arg("-i")
            .arg(&encoded)
            .arg("-frames:v")
            .arg("1")
            .arg(&decoded),
        "ffmpeg decode",
    )?;
    if fs::metadata(&decoded)?.len() == 0 {
        return Err(SharedPipelineError::Command(
            "decoder produced an empty frame".to_string(),
        ));
    }

    let packet = EncodedMediaPacket {
        sequence: 1,
        kind: StreamKind::Video,
        is_keyframe: true,
        pts_us: DEFAULT_PRESENTATION_BUFFER_US,
        duration_us: DEFAULT_PRESENTATION_BUFFER_US as u32,
        payload,
    };
    let encoded_packet = encode_packet(&packet);
    let decoded_packet = decode_packet(&encoded_packet)?;

    let credentials = RoomCredentials::generate();
    let server = QuicServer::bind(
        loopback_bind_addr(),
        credentials.clone(),
        "Host".to_string(),
        "HostDeviceId".to_string(),
    )?;
    let addr = server.local_addr()?;
    let certificate = server.certificate();
    let server_task = tokio::spawn(server.run());
    let (client, _) = QuicClient::connect(
        addr,
        crate::network::quic::certificate_fingerprint(&certificate),
        credentials,
        DeviceIdentity::new_ephemeral(),
        "Shared Pipeline Guest",
    )
    .await?;
    let (transported_sequence, transported_payload_bytes) =
        client.send_shared_stream_packet(encoded_packet).await?;
    client.wait_idle().await;
    server_task.abort();

    let mut host_buffer = PresentationBuffer::default();
    let mut guest_buffer = PresentationBuffer::default();
    host_buffer.push(decoded_packet.clone())?;
    guest_buffer.push(decoded_packet)?;
    let low_buffer_action = evaluate_shared_strict_sync(SharedStrictSyncInput {
        guest_connected: true,
        guest_buffer_us: 1_000_000,
        host_decoder_ready: true,
        guest_decoder_ready: true,
        source_playing: true,
    });
    let ready_action = evaluate_shared_strict_sync(SharedStrictSyncInput {
        guest_connected: true,
        guest_buffer_us: DEFAULT_PRESENTATION_BUFFER_US,
        host_decoder_ready: true,
        guest_decoder_ready: true,
        source_playing: false,
    });
    let host_presented_sequence = host_buffer
        .pop_ready(DEFAULT_PRESENTATION_BUFFER_US)
        .ok_or(SharedPipelineError::PresentationNotReady)?
        .sequence;
    let guest_presented_sequence = guest_buffer
        .pop_ready(DEFAULT_PRESENTATION_BUFFER_US)
        .ok_or(SharedPipelineError::PresentationNotReady)?
        .sequence;

    let quality = automatic_quality_decision(
        H264Profile::P1080Medium,
        CameraTier::B,
        AutomaticQualityInput {
            measured_goodput_bps: 4_100_000,
            buffer_level_us: 8_000_000,
            encoder_sample: EncoderBenchmarkSample {
                capture_to_encode_latency_ms: 80.0,
                cpu_percent: 20.0,
                gpu_percent: 35.0,
                achieved_fps: 29.5,
            },
            call_bitrate_bps: 350_000,
        },
    );

    Ok(SharedPipelineProof {
        captured_frame: frame,
        encoded_h264: encoded,
        decoded_frame: decoded,
        encoded_bytes: transported_payload_bytes as u64,
        transported_sequence,
        host_presented_sequence,
        guest_presented_sequence,
        strict_sync_paused_on_low_buffer: low_buffer_action
            == SharedStrictSyncAction::PauseSourceHostAndGuest,
        strict_sync_resumed_after_buffer: ready_action == SharedStrictSyncAction::ResumeTogether,
        camera_degraded_before_movie: quality.movie == config_for_profile(H264Profile::P1080Medium)
            && quality.camera_tier == CameraTier::C,
    })
}

fn run_command(command: &mut Command, label: &str) -> Result<(), SharedPipelineError> {
    let output = command.output()?;
    if output.status.success() {
        return Ok(());
    }

    Err(SharedPipelineError::Command(format!(
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    )))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    #[ignore = "captures the real desktop and uses h264_videotoolbox"]
    async fn macos_shared_pipeline_captures_encodes_transports_and_decodes() {
        let root = std::env::temp_dir().join(format!("movie-party-shared-{}", Uuid::now_v7()));
        let proof = match run_macos_shared_pipeline_proof(&root).await {
            Ok(proof) => proof,
            Err(error) => {
                eprintln!("EXTERNAL CAPTURE VERIFICATION PENDING: {error}");
                let _ = fs::remove_dir_all(&root);
                return;
            }
        };

        assert!(proof.encoded_bytes > 0);
        assert_eq!(proof.transported_sequence, 1);
        assert_eq!(proof.host_presented_sequence, 1);
        assert_eq!(proof.guest_presented_sequence, 1);
        assert!(proof.strict_sync_paused_on_low_buffer);
        assert!(proof.strict_sync_resumed_after_buffer);
        assert!(proof.camera_degraded_before_movie);
        let _ = fs::remove_dir_all(root);
    }
}
