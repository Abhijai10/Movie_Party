#!/bin/bash
# Regenerate the committed H.264 fixture the real_* playback tests decode.
#
#   ./scripts/make-test-media-macos.sh            # 3s fixture (the committed default)
#   ./scripts/make-test-media-macos.sh 6          # 6s fixture
#   ./scripts/make-test-media-macos.sh 3 /tmp/x.mp4
#
# ⚠️  THIS SCRIPT DOES NOT CURRENTLY REPRODUCE THE COMMITTED VIDEO-ONLY FIXTURE.
#     The committed file is 30 fps / 90 frames; this generator writes 15 fps /
#     45 frames (see `fps` in the .swift). Running it over
#     src-tauri/tests/fixtures/movie_party_test_320x240.mp4 therefore CHANGES a
#     test input — the render tests size their windows against that file's frame
#     rate. `tests/fixture_properties.rs` now pins the committed properties, so
#     such a change fails loudly in CI instead of surfacing as a confusing render
#     failure later. Either fix the generator to 30 fps, or change the fixture
#     deliberately and update that test with it. (Found by the post-remediation
#     adversarial audit, ADV-07.)
#
# With `--with-audio` (any position after the script name) the output also gets
# a mono 440 Hz AAC track. The committed default stays video-only: the render
# tests depend on that fixture's duration and frame content, so regenerating it
# with audio would be a silent behaviour change for them.
#
#   ./scripts/make-test-media-macos.sh 3 \
#       src-tauri/tests/fixtures/movie_party_test_with_audio_320x240.mp4 --with-audio
#
# The fixture is COMMITTED to the repository at
#   src-tauri/tests/fixtures/movie_party_test_320x240.mp4
# and must stay committed: the real_* tests require it, and before Batch 5 they
# silently passed whenever a /tmp copy was missing — which was the normal case
# on a clean checkout and in CI. See BATCH5_REPORT.md.
#
# Regeneration uses AVFoundation via scripts/make-test-media-macos.swift, so it
# needs no ffmpeg (ffmpeg is not a dependency of this repository). macOS only,
# which matches the tests: the macOS real_* tests are the ones that decode this
# file. The Windows test reads the same committed fixture.
#
# Every failure is LOUD (no stderr suppression), matching the convention in
# stage-libmpv-macos.sh.

set -euo pipefail

if [ "$(uname -s)" != "Darwin" ]; then
    echo "ERROR: this generator is macOS-only (AVFoundation)."
    echo "The committed fixture is in git, so you only need this to regenerate it."
    exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SECONDS_TO_MAKE="${1:-3}"
OUT="${2:-$REPO_ROOT/src-tauri/tests/fixtures/movie_party_test_320x240.mp4}"
GENERATOR="$REPO_ROOT/scripts/make-test-media-macos.swift"

# Forward `--with-audio` from any position. Collected rather than positional so
# `./script --with-audio` works too. The `${arr[@]+...}` form is required:
# under `set -u`, bash 3.2 (macOS) errors expanding an empty array.
EXTRA_ARGS=()
for arg in "$@"; do
    case "$arg" in
        --with-audio) EXTRA_ARGS+=("$arg") ;;
    esac
done
WITH_AUDIO=0
[ "${#EXTRA_ARGS[@]}" -gt 0 ] && WITH_AUDIO=1

if [ ! -f "$GENERATOR" ]; then
    echo "ERROR: generator not found at $GENERATOR"
    exit 1
fi

if ! command -v swift >/dev/null 2>&1; then
    echo "ERROR: 'swift' not found. Install the Xcode command line tools:"
    echo "  xcode-select --install"
    exit 1
fi

mkdir -p "$(dirname "$OUT")"

echo "Generating a ${SECONDS_TO_MAKE}s fixture at $OUT"
swift "$GENERATOR" "$OUT" "$SECONDS_TO_MAKE" ${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}

# Structural sanity check: an ISO-MP4 with an H.264 (avc1) track. Deliberately
# not a duration check — the tests validate duration themselves, and the point
# here is to catch a truncated or empty write.
if [ ! -s "$OUT" ]; then
    echo "ERROR: $OUT is empty"
    exit 1
fi

for box in ftyp moov mdat avc1; do
    if ! strings -a "$OUT" | grep -q "$box"; then
        echo "ERROR: $OUT has no '$box' box — not a usable H.264 MP4"
        exit 1
    fi
done

# With audio requested, assert the audio sample entry really is there. `mp4a`
# is the AAC sample entry in `stsd`; `soun` is the handler. Checking both means
# a file that silently ended up video-only cannot be mistaken for a success.
if [ "$WITH_AUDIO" = "1" ]; then
    for box in soun mp4a; do
        if ! strings -a "$OUT" | grep -q "$box"; then
            echo "ERROR: --with-audio was requested but $OUT has no '$box' marker;"
            echo "       the audio track was not written."
            exit 1
        fi
    done
fi

echo "OK: $OUT ($(wc -c <"$OUT" | tr -d ' ') bytes)"
echo
echo "The real_* tests also require the bundled libmpv runtime, which is a"
echo "gitignored build artifact. If it is not staged, those tests FAIL loudly:"
echo "  ./scripts/stage-libmpv-macos.sh"
