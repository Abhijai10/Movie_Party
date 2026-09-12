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
