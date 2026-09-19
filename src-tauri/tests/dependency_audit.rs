//! Dependency-hygiene regression tests.
//!
//! These read `Cargo.lock` rather than a manifest requirement on purpose. A
//! requirement of `rustls = "0.23"` can still resolve to an older crate if the
//! lockfile is stale, and the *resolved* version is the one that ships. A
//! manifest-level assertion would pass while the vulnerable crate was built.
//!
//! The lockfile lives at the workspace root, two levels up from this file
//! (`src-tauri/tests/` → `src-tauri/` → root).

/// RUSTSEC-2026-0285 (GHSA-2mjx-qc3c-rqvc): rustls accepted TLS 1.3 handshake
/// messages sent at the wrong encryption level when they followed a
/// key-changing message in the same record. Patched in `>=0.23.45`;
/// `<0.23.13` is unaffected.
const RUSTLS_FIRST_PATCHED: (u64, u64, u64) = (0, 23, 45);

fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    // A pre-release suffix (`0.23.45-rc.1`) must not parse as the release.
    let patch = parts.next()?;
    if patch.contains('-') || patch.contains('+') {
        return None;
    }
    Some((major, minor, patch.parse().ok()?))
}

/// Every resolved version of `package` in the lockfile.
fn locked_versions(lock: &str, package: &str) -> Vec<String> {
    let target = format!("name = \"{package}\"");
    let mut versions = Vec::new();
    let mut lines = lock.lines();

    while let Some(line) = lines.next() {
        if line.trim() != target {
            continue;
        }

        // `version` is the next `version = "..."` line within this block.
        for candidate in lines.by_ref() {
            let candidate = candidate.trim();
            if candidate.starts_with("[[package]]") {
                break;
            }
            if let Some(rest) = candidate.strip_prefix("version = \"") {
                if let Some(version) = rest.strip_suffix('"') {
                    versions.push(version.to_string());
                }
                break;
            }
        }
    }

    versions
}

#[test]
fn rustls_resolves_to_a_version_that_fixes_rustsec_2026_0285() {
    let lock = include_str!("../../Cargo.lock");
    let versions = locked_versions(lock, "rustls");

    assert_eq!(
        versions.len(),
        1,
        "expected exactly one resolved rustls; got {versions:?} — a second copy would mean one \
         of them is unpatched"
    );

    let version = &versions[0];
    let parsed = parse_version(version)
        .unwrap_or_else(|| panic!("rustls {version} is not a release version"));

    assert!(
        parsed >= RUSTLS_FIRST_PATCHED,
        "rustls {version} is affected by RUSTSEC-2026-0285; >= 0.23.45 is required"
    );
}

#[test]
fn the_rustls_family_resolves_once_each() {
    let lock = include_str!("../../Cargo.lock");

    for package in ["rustls-webpki", "rustls-pki-types"] {
        let versions = locked_versions(lock, package);
        assert_eq!(
            versions.len(),
            1,
            "expected exactly one resolved {package}; got {versions:?}"
        );
    }
}

/// A control for [`locked_versions`]: the parser must actually find things, and
/// must not match a package whose name merely contains the target.
#[test]
fn lockfile_parser_is_not_vacuous() {
    let lock = include_str!("../../Cargo.lock");

    assert!(
        !locked_versions(lock, "rustls").is_empty(),
        "the parser must find a package that is definitely present"
    );
    assert!(
        locked_versions(lock, "a-package-that-does-not-exist").is_empty(),
        "the parser must not invent entries"
    );
}
