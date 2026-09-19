#!/bin/bash
# Regenerate the committed H.264 fixture the real_* playback tests decode.
#
#   ./scripts/make-test-media-macos.sh            # 3s fixture (the committed default)
#   ./scripts/make-test-media-macos.sh 6          # 6s fixture
#   ./scripts/make-test-media-macos.sh 3 /tmp/x.mp4
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
swift "$GENERATOR" "$OUT" "$SECONDS_TO_MAKE"

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

echo "OK: $OUT ($(wc -c <"$OUT" | tr -d ' ') bytes)"
echo
echo "The real_* tests also require the bundled libmpv runtime, which is a"
echo "gitignored build artifact. If it is not staged, those tests FAIL loudly:"
echo "  ./scripts/stage-libmpv-macos.sh"
