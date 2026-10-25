//! Integration probe: runs the app's REAL detect_status() in a process
//! whose environment mimics a GUI launch (no TERM set by us — detect_status
//! itself injects TERM now). Verifies the "installed but not responding"
//! bug is fixed against the machine's actual Tailscale installation.
//!
//! Skipped (exit 0) when Tailscale is not installed on the host.

#[test]
fn detect_status_works_without_terminal_environment() {
    // Simulate the GUI launch environment: strip TERM if the test runner
    // happens to have one, so we prove the fix does not depend on it.
    if std::env::var("TERM").is_ok() {
        std::env::remove_var("TERM");
    }
    let result = movie_party_lib::network::tailscale::detect_status();
    // .await in a sync test — use a tiny runtime.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let result = rt.block_on(result);
    match result {
        Ok(status) => {
            // A real status came back — the old bug would have failed here
            // with CommandFailed("The Tailscale GUI failed to start…").
            assert!(
                status.backend_state.is_some(),
                "status must carry a backend state"
            );
            println!("detect_status OK: {:?}", status.backend_state);
        }
        Err(e) => {
            // Tailscale may legitimately be absent on CI — the invariant is
            // that the error is NOT the GUI-wrapper banner.
            let text = e.to_string();
            assert!(
                !text.contains("Tailscale GUI failed to start"),
                "regression: the GUI-env TERM fix no longer masks the wrapper failure: {text}"
            );
            println!("Tailscale unavailable on this host (expected on CI): {text}");
        }
    }
}

#[test]
fn verify_peer_connection_reports_cleanly_without_terminal_env() {
    if std::env::var("TERM").is_ok() {
        std::env::remove_var("TERM");
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    // 100.64.0.0 is inside the CGNAT range but never allocated; the probe
    // must return a structured failure, never a panic — and on a real
    // tailnet the message must not be the GUI-wrapper banner.
    let probe = rt.block_on(movie_party_lib::network::tailscale::verify_peer_connection(
        "100.64.0.0".parse().expect("ipv4"),
    ));
    assert!(!probe.reachable);
    assert!(
        !probe.message.contains("Tailscale GUI failed to start"),
        "regression: wrapper banner leaked into ping probe: {}",
        probe.message
    );
    println!("probe message: {}", probe.message);
}

/// Batch 9: the wizard's tailnet/role/key-expiry reading must work against the
/// machine's real CLI output, not only against fixtures. CI (no Tailscale)
/// skips it; a real tailnet must produce a self-consistent identity.
#[test]
fn tailnet_identity_matches_the_real_cli_output() {
    if std::env::var("TERM").is_ok() {
        std::env::remove_var("TERM");
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let Ok(status) = rt.block_on(movie_party_lib::network::tailscale::detect_status()) else {
        println!("Tailscale unavailable on this host (expected on CI): skipped");
        return;
    };
    let Some(tailnet) = status.tailnet.as_ref() else {
        // A stopped or key-only backend legitimately reports no tailnet block.
        println!(
            "no tailnet identity reported (backend: {:?})",
            status.backend_state
        );
        return;
    };
    println!(
        "tailnet stable_id={:?} suffix={:?} role={:?} key_expiry={:?} have_node_key={:?}",
        tailnet.stable_id,
        tailnet.magic_dns_suffix,
        tailnet.role,
        tailnet.key_expiry,
        tailnet.have_node_key
    );

    // A reported node key must classify into a real state, never "unknown".
    if let Some(expiry) = tailnet.key_expiry.as_deref() {
        let state = movie_party_lib::network::tailscale::NodeKeyState::from_expiry(
            Some(expiry),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
        );
        assert_ne!(
            state,
            movie_party_lib::network::tailscale::NodeKeyState::Unknown,
            "the CLI reported expiry {expiry:?} but Movie Party could not parse it"
        );
    }

    // The device's own DNS name must sit under the suffix Movie Party claims.
    if let (Some(dns), Some(suffix)) = (
        status.device_name.as_deref(),
        tailnet.magic_dns_suffix.as_deref(),
    ) {
        assert!(
            dns.trim_end_matches('.').ends_with(suffix),
            "self DNSName {dns:?} does not end with tailnet suffix {suffix:?}"
        );
    }

    // Every peer Movie Party lists must be classifiable, or honestly unknown —
    // it must never be a guess of "same tailnet" for a foreign suffix.
    for peer in &status.peers {
        if let (Some(suffix), Some(same)) = (
            tailnet.magic_dns_suffix.as_deref(),
            movie_party_lib::network::tailscale::peer_is_same_tailnet(&status, peer),
        ) {
            if same && !peer.shared_in {
                assert!(
                    peer.dns_name.trim_end_matches('.').ends_with(suffix),
                    "claimed same-tailnet for peer {:?} which is not under {suffix:?}",
                    peer.dns_name
                );
            }
        }
    }
}
