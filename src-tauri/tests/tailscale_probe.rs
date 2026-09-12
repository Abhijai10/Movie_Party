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
