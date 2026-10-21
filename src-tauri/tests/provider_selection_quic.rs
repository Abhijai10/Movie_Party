//! Real authenticated loopback QUIC with simulated public-player observations.
//! No browser, credentials or provider network are involved in this harness.
use movie_party_lib::{
    identity::DeviceIdentity,
    network::quic::{
        loopback_bind_addr, ClientRequest, QuicClient, QuicServer, RoomCredentials, ServerEvent,
        ServerResponse,
    },
    providers::{
        content::parse_content,
        selection::{ContentEvidence, ContentLoadingState, GuestSelection, ProviderRoomAuthority},
    },
};
use std::sync::{Arc, Mutex};
use std::time::Duration;
fn evidence(id: &str) -> ContentEvidence {
    ContentEvidence {
        title: format!("Observed fixture {id}"),
        duration_ms: 90_000,
        player_content_id: Some(id.into()),
    }
}
fn publish(state: &Arc<Mutex<ProviderRoomAuthority>>, room: &str, id: &str) {
    state
        .lock()
        .unwrap()
        .publish(
            room,
            &parse_content("youtube", &format!("https://www.youtube.com/watch?v={id}")).unwrap(),
            evidence(id),
        )
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authenticated_two_peer_selection_ack_late_join_changes_and_reconnect() {
    let credentials = RoomCredentials::generate();
    let host_identity = DeviceIdentity::new_ephemeral();
    let guest_identity = DeviceIdentity::new_ephemeral();
    let state = Arc::new(Mutex::new(ProviderRoomAuthority::default()));
    // Host selects Y before any guest exists. AuthAccept must contain it.
    publish(&state, &credentials.room_id, "dQw4w9WgXcQ");
    let server = QuicServer::bind(
        loopback_bind_addr(),
        credentials.clone(),
        "Host".into(),
        host_identity.device_id.clone(),
    )
    .unwrap()
    .with_provider_room(state.clone());
    let address = server.local_addr().unwrap();
    let fingerprint = server.certificate_fingerprint();
    let task = tokio::spawn(server.run(None));
    let (client, auth) = QuicClient::connect(
        address,
        fingerprint.clone(),
        credentials.clone(),
        guest_identity.clone(),
        "Guest".into(),
    )
    .await
    .unwrap();
    let mut guest = GuestSelection::default();
    guest
        .apply(auth.provider_room.unwrap(), &credentials.room_id)
        .unwrap();
    let old = guest.descriptor.clone().unwrap();
    assert_eq!(guest.selection().unwrap().content_id, "dQw4w9WgXcQ");
    assert_eq!(
        client.send_ready_state(true, 5000).await.unwrap(),
        ServerResponse::ReadyAck { ready: false }
    );
    // Broadcast/ack may be lost; authenticated state pull repairs selection.
    publish(&state, &credentials.room_id, "abcdefghijk");
    guest
        .apply(
            client.provider_selection().await.unwrap(),
            &credentials.room_id,
        )
        .unwrap();
    let x = guest.selection().unwrap().clone();
    assert_eq!(x.content_id, "abcdefghijk");
    assert!(guest.apply(old, &credentials.room_id).is_err());
    let confirmed = guest
        .observe(
            ContentLoadingState::ContentConfirmed,
            Some(x.evidence.clone()),
        )
        .unwrap();
    assert!(client.acknowledge_provider(confirmed).await.unwrap());
    assert!(!state.lock().unwrap().guest_ready());
    let ready = guest
        .observe(ContentLoadingState::PlaybackReady, Some(x.evidence.clone()))
        .unwrap();
    assert!(client.acknowledge_provider(ready.clone()).await.unwrap());
    // Simulate losing the acknowledgement response. Repeating its payload on
    // a fresh authenticated request is safe and does not reopen content.
    assert!(client.acknowledge_provider(ready.clone()).await.unwrap());
    assert_eq!(
        client.send_ready_state(true, 5000).await.unwrap(),
        ServerResponse::ReadyAck { ready: true }
    );
    assert!(!guest
        .apply(
            client.provider_selection().await.unwrap(),
            &credentials.room_id
        )
        .unwrap());
    // Replacing transport must expose the same authoritative epoch/revision,
    // retract readiness until the new connection re-acknowledges, and refuse
    // provider writes from the retired authenticated connection.
    let (replacement, reconnect) = QuicClient::connect(
        address,
        fingerprint,
        credentials.clone(),
        guest_identity,
        "Guest".into(),
    )
    .await
    .unwrap();
    assert!(!guest
        .apply(reconnect.provider_room.unwrap(), &credentials.room_id)
        .unwrap());
    assert!(!state.lock().unwrap().guest_ready());
    assert!(client.acknowledge_provider(ready.clone()).await.is_err());
    assert!(replacement.acknowledge_provider(ready).await.unwrap());
    assert!(state.lock().unwrap().guest_ready());
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authenticated_guest_cannot_forge_host_selection_or_another_identity_ack() {
    let credentials = RoomCredentials::generate();
    let host = DeviceIdentity::new_ephemeral();
    let guest = DeviceIdentity::new_ephemeral();
    let authority = Arc::new(Mutex::new(ProviderRoomAuthority::default()));
    publish(&authority, &credentials.room_id, "abcdefghijk");
    let server = QuicServer::bind(
        loopback_bind_addr(),
        credentials.clone(),
        "Host".into(),
        host.device_id.clone(),
    )
    .unwrap()
    .with_provider_room(authority.clone());
    let addr = server.local_addr().unwrap();
    let fp = server.certificate_fingerprint();
    let task = tokio::spawn(server.run(None));
    let (client, _) = QuicClient::connect(addr, fp, credentials, guest, "Guest".into())
        .await
        .unwrap();
    let selection = client
        .provider_selection()
        .await
        .unwrap()
        .selection
        .unwrap();
    // The client request enum intentionally has no authoritative selection
    // variant. A host event masquerading as a guest request is malformed.
    let forged = serde_json::to_value(ServerEvent::ProviderSelection {
        selection: selection.clone(),
    })
    .unwrap();
    assert!(serde_json::from_value::<ClientRequest>(forged).is_err());
    let wrong = movie_party_lib::providers::selection::SelectionAck::for_selection(
        &selection,
        1,
        ContentLoadingState::PlaybackReady,
        Some(evidence("dQw4w9WgXcQ")),
    );
    assert!(!client.acknowledge_provider(wrong).await.unwrap());
    let identity_forgery = client
        .send_control(ClientRequest::ProviderSelectionGet {
            seq: 1000,
            sender: host.device_id,
        })
        .await
        .unwrap();
    assert!(matches!(
        identity_forgery,
        ServerResponse::AuthReject { .. }
    ));
    assert_eq!(
        authority.lock().unwrap().descriptor.selection,
        Some(selection)
    );
    assert!(!authority.lock().unwrap().guest_ready());
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lost_ack_timeout_withdraws_readiness_and_ack_retry_recovers() {
    let credentials = RoomCredentials::generate();
    let authority = Arc::new(Mutex::new(ProviderRoomAuthority::default()));
    publish(&authority, &credentials.room_id, "abcdefghijk");
    let host = DeviceIdentity::new_ephemeral();
    let server = QuicServer::bind(
        loopback_bind_addr(),
        credentials.clone(),
        "Host".into(),
        host.device_id,
    )
    .unwrap()
    .with_provider_room(authority.clone());
    let addr = server.local_addr().unwrap();
    let fp = server.certificate_fingerprint();
    let task = tokio::spawn(server.run(None));
    let (client, auth) = QuicClient::connect(
        addr,
        fp,
        credentials,
        DeviceIdentity::new_ephemeral(),
        "Guest".into(),
    )
    .await
    .unwrap();
    let selected = auth.provider_room.unwrap().selection.unwrap();
    let ack = movie_party_lib::providers::selection::SelectionAck::for_selection(
        &selected,
        1,
        ContentLoadingState::PlaybackReady,
        Some(selected.evidence.clone()),
    );
    assert!(client.acknowledge_provider(ack.clone()).await.unwrap());
    authority.lock().unwrap().acknowledged_at =
        Some(std::time::Instant::now() - Duration::from_secs(13));
    assert_eq!(
        client.send_ready_state(true, 5000).await.unwrap(),
        ServerResponse::ReadyAck { ready: false }
    );
    assert!(client.acknowledge_provider(ack).await.unwrap());
    assert!(authority.lock().unwrap().guest_ready());
    task.abort();
}
