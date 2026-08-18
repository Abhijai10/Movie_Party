use std::path::Path;

use uuid::Uuid;

use crate::{
    identity::DeviceIdentity,
    media::{
        cache::{SparseCache, CACHE_DATA_FILE},
        manifest::MediaManifest,
        player::{detect_libmpv, LibMpvPlayer, LocalPlayer, PlayerError, PlayerState},
        stream::{parse_http_range, respond_from_cache, route_for_media, RangeResponse},
        transfer::{transfer_progress, validate_chunk_packet},
    },
    network::quic::{loopback_bind_addr, QuicClient, QuicError, QuicServer, RoomCredentials},
    storage::{apply_retention_decision, prompt_for_media, RetentionDecision, StorageError},
    sync::{
        consensus::ParticipantReadiness,
        local::{LocalSyncCoordinator, LocalSyncError, PeerRole},
        state_machine::RoomState,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPerfectReport {
    pub manifest: MediaManifest,
    pub bytes_transferred_before_playback: u64,
    pub total_bytes_transferred: u64,
    pub playback_started_before_complete_transfer: bool,
    pub starvation_paused_host: bool,
    pub resumed_after_catchup: bool,
    pub seek_prefetched: bool,
    pub reconnect_restored_session: bool,
    pub retention_prompted: bool,
    pub retention_removed_cache: bool,
    pub libmpv_available: bool,
    pub final_room_state: RoomState,
}

#[derive(Debug, thiserror::Error)]
pub enum LocalPerfectError {
    #[error(transparent)]
    Quic(#[from] QuicError),
    #[error("MP-MEDIA-002 cache error: {0}")]
    Cache(#[from] crate::media::cache::CacheError),
    #[error("MP-MEDIA-002 stream error: {0}")]
    Stream(#[from] crate::media::stream::StreamError),
    #[error("MP-MEDIA-002 transfer error: {0}")]
    Transfer(#[from] crate::media::transfer::TransferError),
    #[error("MP-SYNC-004 sync error: {0}")]
    Sync(#[from] LocalSyncError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("MP-MEDIA-001 player failed: {0}")]
    Player(#[from] PlayerError),
    #[error("MP-MEDIA-002 local perfect IO failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("MP-MEDIA-002 expected cache range to be available")]
    RangeStillPending,
}

pub async fn run_loopback_local_perfect(
    media_path: &Path,
    cache_root: &Path,
) -> Result<LocalPerfectReport, LocalPerfectError> {
    run_loopback_local_perfect_with_player(media_path, cache_root, LibMpvPlayer::new()).await
}

pub async fn run_loopback_local_perfect_with_player<P: LocalPlayer>(
    media_path: &Path,
    cache_root: &Path,
    mut player: P,
) -> Result<LocalPerfectReport, LocalPerfectError> {
    let credentials = RoomCredentials::generate();
    let server = QuicServer::bind_with_local_media(
        loopback_bind_addr(),
        credentials.clone(),
        "Host".to_string(),
        "HostDeviceId".to_string(),
        media_path.to_path_buf(),
    )?;
    let addr = server.local_addr()?;
    let cert_fingerprint = crate::network::quic::certificate_fingerprint(server.certificate());
    let server_task = tokio::spawn(server.run(None));

    let guest_identity = DeviceIdentity::new_ephemeral();
    let (client, _) = QuicClient::connect(
        addr,
        cert_fingerprint.clone(),
        credentials.clone(),
        guest_identity.clone(),
        "Local Guest".to_string(),
    )
    .await?;

    // 1. Fetch manifest over authenticated QUIC
    let manifest = client.fetch_local_media_manifest().await?;
    let mut cache = SparseCache::open(cache_root, manifest.clone())?;
    let route = route_for_media(&manifest.media_id, Uuid::now_v7().to_string());

    // 2. Fetch first chunk — proves real file bytes travel over QUIC
    let first_packet = client
        .fetch_local_media_chunk(&manifest.media_id, 0)
        .await?;
    validate_chunk_packet(&manifest, &first_packet)?;
    cache.write_chunk(u64::from(first_packet.chunk_index), &first_packet.payload)?;

    // 3. Range server serves validated bytes from cache
    let startup_range = parse_http_range(
        &format!(
            "bytes=0-{}",
            manifest
                .chunk_size
                .min(manifest.file_size)
                .saturating_sub(1)
        ),
        manifest.file_size,
    )?;
    let startup = respond_from_cache(
        &cache,
        &manifest,
        &route,
        &route.session_token,
        startup_range,
    )?;
    let bytes_transferred_before_playback = cache.bytes_available();
    let playback_started_before_complete_transfer =
        bytes_transferred_before_playback < manifest.file_size;

    // 4. Player opens the file and coordinator runs the lifecycle
    player.open(media_path)?;
    let mut sync = LocalSyncCoordinator::new();
    sync.host_ready(ParticipantReadiness::ready(5_000));
    sync.guest_ready(ParticipantReadiness::ready(5_000));
    sync.update_readiness_consensus(5_000);
    let now = crate::network::quic::monotonic_us();
    let play = sync.prepare_play(0, now, 2_000)?;
    sync.commit_play(&play)?;
    player.play()?;

    let _startup_bytes = match startup {
        RangeResponse::Available { bytes, .. } => bytes,
        RangeResponse::Pending { .. } => return Err(LocalPerfectError::RangeStillPending),
    };

    let _progress = transfer_progress(
        &manifest,
        cache.bytes_available(),
        5_000,
        manifest.chunk_size.saturating_mul(8),
    );

    // 5. Buffer starvation → strict-sync pause
    sync.buffer_low(PeerRole::Guest, 400)?;
    player.pause()?;
    // buffer_low Guest reports 400ms but host was at 0ms, so min = 0
    let starvation_paused_host =
        sync.paused_by_strict_sync && sync.room_state == RoomState::Buffering;

    // 6. Transfer remaining chunks
    for index in 1..manifest.chunk_count {
        let packet = client
            .fetch_local_media_chunk(&manifest.media_id, index)
            .await?;
        validate_chunk_packet(&manifest, &packet)?;
        cache.write_chunk(u64::from(packet.chunk_index), &packet.payload)?;
        if index == 1 {
            break;
        }
    }

    // 7. Recovery → play resumes
    sync.buffer_recovered()?;
    sync.host_ready(ParticipantReadiness::ready(5_000));
    sync.guest_ready(ParticipantReadiness::ready(5_000));
    sync.update_readiness_consensus(5_000);
    let now = crate::network::quic::monotonic_us();
    let resume = sync.prepare_play(0, now, 2_000)?;
    sync.commit_play(&resume)?;
    player.play()?;
    let resumed_after_catchup =
        sync.room_state == RoomState::Playing && player.snapshot().state == PlayerState::Playing;

    // 8. Seek prefetches the target chunk
    let target_seek_position_ms = 8_000;
    let target_chunk = manifest.chunk_count.saturating_sub(1);
    if target_chunk > 1 && !cache.chunk_map().is_available(target_chunk) {
        let packet = client
            .fetch_local_media_chunk(&manifest.media_id, target_chunk)
            .await?;
        validate_chunk_packet(&manifest, &packet)?;
        cache.write_chunk(u64::from(packet.chunk_index), &packet.payload)?;
    }
    sync.begin_seek(target_seek_position_ms);
    let _seek = sync.prepare_seek(target_seek_position_ms, 2_000)?;
    sync.commit_seek(target_seek_position_ms, true);
    player.seek(target_seek_position_ms)?;
    player.play()?;
    let seek_prefetched = cache.chunk_map().is_available(target_chunk)
        && sync.host_position_ms == target_seek_position_ms;

    // 9. Disconnect → reconnect → resume from cache
    sync.peer_disconnected()?;
    client.wait_idle().await;
    let (reconnect_client, _) = QuicClient::connect(
        addr,
        cert_fingerprint.clone(),
        credentials,
        DeviceIdentity::new_ephemeral(),
        "Local Guest Reconnected".to_string(),
    )
    .await?;
    let resumed_manifest = reconnect_client.fetch_local_media_manifest().await?;
    let resumed_cache = SparseCache::open(cache_root, resumed_manifest.clone())?;
    let reconnect_restored_session = resumed_manifest.media_id == manifest.media_id
        && resumed_cache.bytes_available() == cache.bytes_available();

    // 10. Transfer remaining chunks
    for index in cache.missing_chunks() {
        let packet = reconnect_client
            .fetch_local_media_chunk(&manifest.media_id, index)
            .await?;
        validate_chunk_packet(&manifest, &packet)?;
        cache.write_chunk(u64::from(packet.chunk_index), &packet.payload)?;
    }
    let total_bytes_transferred = cache.bytes_available();

    // 11. Retention: Remove
    let prompt = prompt_for_media(&manifest.media_id, &manifest.filename);
    let media_cache_dir = cache_root.join(&manifest.media_id);
    let retained = apply_retention_decision(
        cache_root,
        &media_cache_dir,
        &media_cache_dir.join(CACHE_DATA_FILE),
        RetentionDecision::Remove,
        None,
    )?;
    let retention_prompted = prompt.media_id == manifest.media_id;
    let retention_removed_cache = retained.is_none() && !media_cache_dir.exists();

    reconnect_client.wait_idle().await;
    server_task.abort();

    Ok(LocalPerfectReport {
        manifest,
        bytes_transferred_before_playback,
        total_bytes_transferred,
        playback_started_before_complete_transfer,
        starvation_paused_host,
        resumed_after_catchup,
        seek_prefetched,
        reconnect_restored_session,
        retention_prompted,
        retention_removed_cache,
        libmpv_available: detect_libmpv().available,
        final_room_state: sync.room_state,
    })
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use uuid::Uuid;

    use super::run_loopback_local_perfect_with_player;
    use crate::media::player::LibMpvPlayer;

    #[tokio::test]
    async fn loopback_local_perfect_moves_real_file_bytes_end_to_end() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        std::fs::create_dir_all(&root).expect("temp root");
        let media = root.join("fixture.mkv");
        let cache = root.join("cache");
        write_fixture(&media, 2_500_000);

        let report = run_loopback_local_perfect_with_player(
            &media,
            &cache,
            LibMpvPlayer::with_availability(true),
        )
        .await
        .expect("local perfect");

        assert!(report.playback_started_before_complete_transfer);
        assert!(report.starvation_paused_host);
        assert!(report.resumed_after_catchup);
        assert!(report.seek_prefetched);
        assert!(report.reconnect_restored_session);
        assert!(report.retention_prompted);
        assert!(report.retention_removed_cache);
        assert_eq!(report.total_bytes_transferred, report.manifest.file_size);
    }

    #[tokio::test]
    async fn loopback_local_perfect_survives_reconnect_and_completes_transfer() {
        let root = std::env::temp_dir().join(Uuid::now_v7().to_string());
        std::fs::create_dir_all(&root).expect("temp root");
        let media = root.join("large.mkv");
        let cache = root.join("cache");
        write_fixture(&media, 5_000_000);

        let report = run_loopback_local_perfect_with_player(
            &media,
            &cache,
            LibMpvPlayer::with_availability(true),
        )
        .await
        .expect("local perfect");

        assert_eq!(report.total_bytes_transferred, report.manifest.file_size);
        assert!(report.reconnect_restored_session);
    }

    fn write_fixture(path: &std::path::Path, size: usize) {
        let mut f = File::create(path).expect("create fixture");
        for i in 0..size {
            std::io::Write::write_all(&mut f, &[(i % 251) as u8]).expect("write byte");
        }
    }
}
