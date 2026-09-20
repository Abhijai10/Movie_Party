//! Properties of the committed media fixtures (ADV-07).
//!
//! These are *test inputs*, and other tests depend on their exact properties:
//! the render tests need a fixture long enough to render against, and the audio
//! test needs one with an audio track. Nothing pinned those properties until
//! now — so a regeneration could have changed a test input silently.
//!
//! **It nearly had.** `scripts/make-test-media-macos.sh` generates 15 fps
//! (`fps: Int32 = 15`), while the committed video-only fixture is **30 fps /
//! 90 frames**. The script's header calls itself "how it is reproduced", and it
//! is not: running it produces a file with **half** the frame rate. Anyone
//! regenerating would have quietly changed what the render tests decode.
//!
//! This test does **not** need libmpv, so unlike the `real_*` targets it runs in
//! CI on both platforms — which is the point. A changed fixture fails here
//! instead of surfacing as a confusing render-test failure later.
//!
//! Deliberately a tiny hand-rolled box walker rather than a dependency: the
//! project has no MP4 crate and this needs four fields.

use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn read(name: &str) -> Vec<u8> {
    let path = fixture(name);
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "PREREQUISITE MISSING: the committed fixture is absent or unreadable at {path:?}: {e}.\n\
             It is tracked in git, so a missing copy means the checkout is incomplete — NOT that \
             this test may pass."
        )
    })
}

/// `(box type, offset of the box header, box size, header length)` for every
/// match of `want`, walking only the container boxes we care about.
fn find_boxes(buf: &[u8], want: &[&str]) -> Vec<(String, usize, usize, usize)> {
    fn walk(
        buf: &[u8],
        off: usize,
        end: usize,
        want: &[&str],
        out: &mut Vec<(String, usize, usize, usize)>,
    ) {
        let mut off = off;
        while off + 8 <= end {
            let size32 =
                u32::from_be_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]) as usize;
            let typ = String::from_utf8_lossy(&buf[off + 4..off + 8]).to_string();
            let (size, hdr) = match size32 {
                1 if off + 16 <= end => {
                    let mut s = [0u8; 8];
                    s.copy_from_slice(&buf[off + 8..off + 16]);
                    (u64::from_be_bytes(s) as usize, 16)
                }
                0 => (end - off, 8),
                n => (n, 8),
            };
            if size < hdr || off + size > end {
                break;
            }
            if want.contains(&typ.as_str()) {
                out.push((typ.clone(), off, size, hdr));
            }
            if ["moov", "trak", "mdia", "minf", "stbl"].contains(&typ.as_str()) {
                walk(buf, off + hdr, off + size, want, out);
            }
            off += size;
        }
    }
    let mut out = Vec::new();
    walk(buf, 0, buf.len(), want, &mut out);
    out
}

/// Duration in seconds from `mvhd`.
fn duration_seconds(buf: &[u8]) -> f64 {
    let mvhd = find_boxes(buf, &["mvhd"]);
    let (_, off, _, hdr) = mvhd.first().expect("fixture must contain an mvhd box");
    let version = buf[off + hdr];
    let (timescale, duration) = if version == 0 {
        (
            u32::from_be_bytes([
                buf[off + hdr + 12],
                buf[off + hdr + 13],
                buf[off + hdr + 14],
                buf[off + hdr + 15],
            ]),
            u32::from_be_bytes([
                buf[off + hdr + 16],
                buf[off + hdr + 17],
                buf[off + hdr + 18],
                buf[off + hdr + 19],
            ]) as u64,
        )
    } else {
        let mut d = [0u8; 8];
        d.copy_from_slice(&buf[off + hdr + 24..off + hdr + 32]);
        (
            u32::from_be_bytes([
                buf[off + hdr + 20],
                buf[off + hdr + 21],
                buf[off + hdr + 22],
                buf[off + hdr + 23],
            ]),
            u64::from_be_bytes(d),
        )
    };
    duration as f64 / timescale as f64
}

/// `stsz` sample count — one video track, so the first is the video track's.
fn video_sample_count(buf: &[u8]) -> u32 {
    let stsz = find_boxes(buf, &["stsz"]);
    let (_, off, _, hdr) = stsz.first().expect("fixture must contain an stsz box");
    u32::from_be_bytes([
        buf[off + hdr + 8],
        buf[off + hdr + 9],
        buf[off + hdr + 10],
        buf[off + hdr + 11],
    ])
}

fn has_marker(buf: &[u8], marker: &[u8]) -> bool {
    buf.windows(marker.len()).any(|w| w == marker)
}

#[test]
fn video_only_fixture_has_the_properties_the_tests_depend_on() {
    let buf = read("movie_party_test_320x240.mp4");

    let duration = duration_seconds(&buf);
    assert!(
        (duration - 3.0).abs() < 0.01,
        "the committed fixture must be 3.000 s (the render tests size their windows \
         against it); got {duration:.3} s. If this changed, a regeneration altered a \
         test input — re-validate the render tests before accepting it."
    );

    let frames = video_sample_count(&buf);
    assert_eq!(
        frames, 90,
        "the committed fixture is 30 fps / 90 frames. NOTE: \
         scripts/make-test-media-macos.sh currently generates 15 fps / 45 frames, so it \
         does NOT reproduce this file — running it would silently halve the frame rate \
         the render tests decode. Fix the generator to 30 fps, or accept the change \
         deliberately and update this assertion with it."
    );

    // The render tests assert on non-zero pixel data; an audio-free video track is
    // what makes the audio fixture's existence meaningful by contrast.
    assert!(
        has_marker(&buf, b"avc1"),
        "the fixture must be H.264 (avc1)"
    );
    assert!(
        !has_marker(&buf, b"soun") && !has_marker(&buf, b"mp4a"),
        "the video-only fixture must have NO audio track — that is the negative control \
         for the audio test, which asserts the audio-bearing fixture has one"
    );
}

#[test]
fn audio_fixture_has_an_audio_track_and_the_same_video_properties() {
    let buf = read("movie_party_test_with_audio_320x240.mp4");

    assert!(
        has_marker(&buf, b"avc1"),
        "the audio fixture must still carry video"
    );
    assert!(
        has_marker(&buf, b"soun") && has_marker(&buf, b"mp4a"),
        "the audio fixture must carry a real audio track (soun handler + mp4a sample \
         entry). Without it the audio test's positive assertion is vacuous."
    );

    let duration = duration_seconds(&buf);
    assert!(
        (duration - 3.0).abs() < 0.01,
        "the audio fixture should match the video-only fixture's duration; got {duration:.3} s"
    );
}
