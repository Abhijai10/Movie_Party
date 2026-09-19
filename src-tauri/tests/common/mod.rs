//! Shared prerequisites for the `real_*` playback tests.
//!
//! # Policy (see `BATCH5_REPORT.md`)
//!
//! These tests drive the *real* bundled libmpv against a *real* H.264 file.
//! They used to `return` early when a prerequisite was missing, which
//! `cargo test` reported as `ok` — a pass that never executed a single
//! assertion. That is worse than a failure, because it is invisible.
//!
//! The rule now is: **a missing prerequisite fails loudly.** There is no
//! environment variable, feature flag or CI setting that turns a missing
//! prerequisite back into a pass.
//!
//! The two prerequisites are deliberately treated the same way, even though
//! only one of them can be satisfied in CI:
//!
//! * the **media fixture** is committed to git, so it is present in every
//!   checkout and its absence is a broken checkout — a genuine failure;
//! * the **libmpv runtime** is a gitignored build artifact, so a clean
//!   checkout legitimately does not have it. CI cannot build it (the staging
//!   script needs a local libmpv build), so CI does not run these tests at
//!   all — it reports that explicitly instead. See `ci.yml`.
//!
//! Anything that cannot satisfy both prerequisites must not pretend to.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// The committed H.264 fixture, relative to `src-tauri/`.
pub const FIXTURE_RELATIVE: &str = "tests/fixtures/movie_party_test_320x240.mp4";

/// Shortest fixture these tests can meaningfully exercise.
///
/// The render loops in this suite run for ~2 s. A fixture shorter than that
/// runs out of frames part-way through, and mpv's SW renderer then emits a
/// fully black frame for every render with no frame available — which is what
/// made `real_sw_render_test` fail with "buffer must contain non-zero pixel
/// data" while the render pipeline was in fact working perfectly. Guarding the
/// duration up front turns that confusing symptom into a clear message.
pub const MIN_FIXTURE_SECONDS: f64 = 2.0;

/// Fewest pixel-bearing frames a render loop must observe to be meaningful.
///
/// One frame could be a fluke or a stale buffer; this many cannot be.
pub const MIN_FRAMES_WITH_PIXELS: u32 = 10;

pub fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_RELATIVE)
}

pub fn libmpv_path() -> PathBuf {
    let name = if cfg!(target_os = "windows") {
        "libmpv.dll"
    } else {
        "libmpv.dylib"
    };
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("mpv_runtime")
        .join(name)
}

/// The committed media fixture, or a loud failure.
pub fn require_fixture() -> PathBuf {
    let path = fixture_path();
    if !path.is_file() {
        panic!(
            "PREREQUISITE MISSING: the committed media fixture is absent at {path:?}.\n\
             \n\
             This file is tracked in git, so a missing copy means the checkout is\n\
             incomplete or the fixture was deleted — NOT that this test may pass.\n\
             \n\
             Restore it from git, or regenerate it:\n\
             \n    ./scripts/make-test-media-macos.sh\n"
        );
    }
    eprintln!("PREREQUISITE OK: fixture at {path:?}");
    path
}

/// The bundled libmpv runtime, or a loud failure.
pub fn require_libmpv() -> PathBuf {
    let path = libmpv_path();
    if !path.is_file() {
        panic!(
            "PREREQUISITE MISSING: the bundled libmpv runtime is absent at {path:?}.\n\
             \n\
             This is a gitignored build artifact, so a clean checkout does not have\n\
             it. Provide it with:\n\
             \n    ./scripts/stage-libmpv-macos.sh\n\
             \n\
             which needs a local libmpv build first (scripts/build-libmpv-macos.sh).\n\
             CI cannot do this, so CI does not run this test — it reports the skip\n\
             explicitly instead of letting the test pass without executing.\n"
        );
    }
    eprintln!("PREREQUISITE OK: libmpv runtime at {path:?}");
    path
}

/// Both prerequisites, for tests that need the runtime *and* the fixture.
pub fn require_runtime_and_fixture() -> (PathBuf, PathBuf) {
    (require_libmpv(), require_fixture())
}
