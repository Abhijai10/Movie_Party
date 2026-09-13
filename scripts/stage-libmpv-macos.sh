#!/bin/bash
# Stage the LGPL libmpv runtime into src-tauri/mpv_runtime/.
# The runtime must be pre-built (see build-libmpv-macos.sh).
# Run from repo root: ./scripts/stage-libmpv-macos.sh
#
# Every failure is LOUD (no stderr suppression): with set -euo pipefail
# a suppressed-error exit produces a silent 350ms death with no clue
# about which command failed — which is exactly what the CI log showed.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MPV_RUNTIME="$REPO_ROOT/src-tauri/mpv_runtime"

# Check if already staged
if [ -f "$MPV_RUNTIME/libmpv.dylib" ]; then
    echo "libmpv runtime already staged at $MPV_RUNTIME"
    echo "Use --force to re-stage"
    exit 0
fi

# Locate the build workspace
STAGE=/tmp/mpv-build-workspace/stage
if [ ! -f "$STAGE/lib/libmpv.2.dylib" ]; then
    echo "ERROR: libmpv not found at $STAGE."
    echo "Run scripts/build-libmpv-macos.sh first."
    exit 1
fi

mkdir -p "$MPV_RUNTIME/licenses"

echo "Staging LGPL runtime into $MPV_RUNTIME"

# Copy dylibs
cp "$STAGE/lib/libmpv.2.dylib" "$MPV_RUNTIME/libmpv.dylib"
cp "$STAGE/lib/libass.9.dylib" "$STAGE/lib/libavcodec.62.dylib" \
   "$STAGE/lib/libavfilter.11.dylib" "$STAGE/lib/libavformat.62.dylib" \
   "$STAGE/lib/libavutil.60.dylib" "$STAGE/lib/libplacebo.360.dylib" \
   "$STAGE/lib/libswresample.6.dylib" "$STAGE/lib/libswscale.9.dylib" \
   "$STAGE/lib/libharfbuzz.0.dylib" "$MPV_RUNTIME/"
echo "  copied libmpv + ffmpeg/libass/libplacebo/harfbuzz dylibs"

cp /opt/homebrew/opt/freetype/lib/libfreetype.6.dylib \
   /opt/homebrew/opt/fribidi/lib/libfribidi.0.dylib \
   /opt/homebrew/opt/libpng/lib/libpng16.16.dylib "$MPV_RUNTIME/"
echo "  copied homebrew freetype/fribidi/libpng dylibs"

# Rewrite install names to @loader_path-relative. The dependency scan
# uses process substitution (not a pipeline) so pipefail cannot turn a
# benign non-matching basename into a script-killing status, and the
# rewrite uses an explicit if (not a && list) so a non-match never ends
# the loop body with status 1.
BUNDLED=(libmpv.dylib libass.9.dylib libavcodec.62.dylib libavfilter.11.dylib \
         libavformat.62.dylib libavutil.60.dylib libplacebo.360.dylib \
         libswresample.6.dylib libswscale.9.dylib libharfbuzz.0.dylib \
         libfreetype.6.dylib libfribidi.0.dylib libpng16.16.dylib)

for lib in "${BUNDLED[@]}"; do
    FILE="$MPV_RUNTIME/$lib"
    if [ ! -f "$FILE" ]; then
        echo "MISSING after copy: $FILE"
        exit 1
    fi
    echo "  rewriting load commands in $lib"
    install_name_tool -id "@loader_path/$lib" "$FILE"
    while read -r dep; do
        base=$(basename "$dep")
        for b in "${BUNDLED[@]}"; do
            if [ "$base" = "$b" ]; then
                install_name_tool -change "$dep" "@loader_path/$b" "$FILE"
            fi
        done
    done < <(otool -L "$FILE" | tail -n +2 | awk '{print $1}')
    # Ad-hoc sign after modification
    codesign --force --sign - "$FILE"
done

# Clean AppleDouble files
find "$MPV_RUNTIME" -name '._*' -delete 2>/dev/null || true

echo "Runtime staged at $MPV_RUNTIME ($(du -sh "$MPV_RUNTIME" | cut -f1))"
