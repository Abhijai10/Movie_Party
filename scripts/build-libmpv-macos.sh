#!/bin/bash
# Reproducible LGPL libmpv runtime build for macOS arm64.
#
# Why LGPL: Movie Party is proprietary. mpv is dual-licensed GPL-2.0-or-later
# / LGPL-2.1-or-later; Homebrew builds it GPL, and Homebrew's ffmpeg is
# GPL-3.0. Bundling GPL components into a proprietary app is not
# distributable. Building mpv with -Dgpl=false produces an LGPL-2.1-or-later
# libmpv, and building ffmpeg without --enable-gpl produces LGPL-2.1-or-later
# libraries, which ARE distributable under the LGPL with license texts.
#
# Outputs a self-contained runtime closure into the staging prefix
# /tmp/mpv-build-workspace/stage, then stages it into src-tauri/mpv_runtime/
# with @loader_path-relative install names.
#
# Requires: Homebrew with meson, ninja, pkg-config, freetype, fribidi, libpng,
# plus Python packages glad2/jinja2/markupsafe (for libplacebo's OpenGL loader).
# Run from repo root: ./scripts/build-libmpv-macos.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK=/tmp/mpv-build-workspace
STAGE="$WORK/stage"
JOBS="$(sysctl -n hw.ncpu)"
export PKG_CONFIG_PATH="$STAGE/lib/pkgconfig:/opt/homebrew/opt/freetype/lib/pkgconfig:/opt/homebrew/opt/fribidi/lib/pkgconfig:/opt/homebrew/opt/libpng/lib/pkgconfig:/opt/homebrew/opt/brotli/lib/pkgconfig:/opt/homebrew/opt/gettext/lib/pkgconfig:/opt/homebrew/opt/pcre2/lib/pkgconfig:/opt/homebrew/opt/glib/lib/pkgconfig"

mkdir -p "$WORK" "$STAGE"

# ---------- 1. LGPL ffmpeg ----------
if [ ! -f "$STAGE/lib/libavcodec.dylib" ]; then
    cd "$WORK"
    curl -sL --max-time 180 -o ffmpeg.tar.gz "https://github.com/FFmpeg/FFmpeg/archive/refs/tags/n8.1.tar.gz"
    tar xzf ffmpeg.tar.gz
    mv FFmpeg-n8.1 ffmpeg-8.1
    cd ffmpeg-8.1
    ./configure --prefix="$STAGE" --enable-shared --disable-static --disable-programs \
        --disable-doc --enable-pic --enable-videotoolbox --enable-audiotoolbox --disable-everything \
        --enable-avcodec --enable-avformat --enable-avutil --enable-swresample --enable-swscale --enable-avfilter \
        --enable-decoder=h264 --enable-decoder=hevc --enable-decoder=vp8 --enable-decoder=vp9 --enable-decoder=av1 \
        --enable-decoder=aac --enable-decoder=mp3float --enable-decoder=mp2 --enable-decoder=ac3 --enable-decoder=eac3 --enable-decoder=dca \
        --enable-decoder=vorbis --enable-decoder=opus --enable-decoder=flac --enable-decoder=alac \
        --enable-decoder=pcm_s16le --enable-decoder=pcm_s16be --enable-decoder=pcm_s24le --enable-decoder=pcm_f32le \
        --enable-decoder=mjpeg \
        --enable-parser=h264 --enable-parser=hevc --enable-parser=aac --enable-parser=mpegaudio --enable-parser=vp9 --enable-parser=av1 --enable-parser=opus --enable-parser=flac \
        --enable-demuxer=mov --enable-demuxer=matroska --enable-demuxer=mp3 --enable-demuxer=flac --enable-demuxer=ogg --enable-demuxer=wav --enable-demuxer=avi --enable-demuxer=image2 --enable-demuxer=aac \
        --enable-muxer=null --enable-protocol=file --enable-protocol=pipe --enable-protocol=concat \
        --enable-filter=scale --enable-filter=format --enable-filter=yadif --enable-filter=bwdif --enable-filter=transpose --enable-filter=afade --enable-filter=aformat
    make -j"$JOBS"
    make install
fi

# ---------- 2. libplacebo (OpenGL, no vulkan) ----------
if [ ! -f "$STAGE/lib/libplacebo.dylib" ]; then
    cd "$WORK"
    curl -sL --max-time 120 -o libplacebo.tar.bz2 "https://code.videolan.org/videolan/libplacebo/-/archive/v7.360.1/libplacebo-v7.360.1.tar.bz2"
    tar xjf libplacebo.tar.bz2
    cd libplacebo-v7.360.1
    curl -sL --max-time 60 -o vk-headers.tar.gz "https://github.com/KhronosGroup/Vulkan-Headers/archive/refs/tags/v1.4.318.tar.gz"
    mkdir -p 3rdparty/Vulkan-Headers
    tar xzf vk-headers.tar.gz -C 3rdparty/Vulkan-Headers --strip-components=1
    PYTHONPATH="${MPV_PYTHONPATH:-/opt/homebrew/lib/python3.12/site-packages}" meson setup build-gl \
        --prefix="$STAGE" -Dvulkan=disabled -Dopengl=enabled -Dgl-proc-addr=enabled \
        -Dshaderc=disabled -Dlcms=disabled -Ddemos=false -Dtests=false -Dbench=false \
        -Ddovi=disabled -Dunwind=disabled -Dxxhash=disabled
    PYTHONPATH="${MPV_PYTHONPATH:-/opt/homebrew/lib/python3.12/site-packages}" meson compile -C build-gl
    PYTHONPATH="${MPV_PYTHONPATH:-/opt/homebrew/lib/python3.12/site-packages}" meson install -C build-gl
fi

# ---------- 3. harfbuzz (MIT) ----------
if [ ! -f "$STAGE/lib/libharfbuzz.dylib" ]; then
    cd "$WORK"
    curl -sL --max-time 120 -o harfbuzz.tar.xz "https://github.com/harfbuzz/harfbuzz/releases/download/11.0.0/harfbuzz-11.0.0.tar.xz"
    tar xJf harfbuzz.tar.xz
    cd harfbuzz-11.0.0
    meson setup build-stage --prefix="$STAGE" -Dfreetype=enabled -Dglib=disabled \
        -Dgobject=disabled -Dcairo=disabled -Dicu=disabled -Dgraphite=disabled \
        -Dtests=disabled -Dbenchmark=disabled -Ddocs=disabled -Dintrospection=disabled
    meson compile -C build-stage
    meson install -C build-stage
fi

# ---------- 4. libass (ISC) ----------
if [ ! -f "$STAGE/lib/libass.dylib" ]; then
    cd "$WORK"
    curl -sL --max-time 120 -o libass.tar.gz "https://github.com/libass/libass/releases/download/0.17.3/libass-0.17.3.tar.gz"
    tar xzf libass.tar.gz
    cd libass-0.17.3
    ./configure --prefix="$STAGE" --disable-fontconfig --disable-require-system-font-provider
    make -j"$JOBS"
    make install
fi

# ---------- 5. LGPL libmpv ----------
if [ ! -f "$STAGE/lib/libmpv.dylib" ]; then
    cd "$WORK"
    curl -sL --max-time 120 -o mpv.tar.gz "https://github.com/mpv-player/mpv/archive/refs/tags/v0.41.0.tar.gz"
    tar xzf mpv.tar.gz
    cd mpv-0.41.0
    meson setup build-mpv --prefix="$STAGE" -Dgpl=false -Dlibmpv=true -Dcplayer=false \
        -Dvulkan=disabled -Dgl=enabled -Dgl-cocoa=enabled -Dcocoa=enabled \
        -Daudiounit=disabled -Dcoreaudio=enabled -Dlua=disabled -Djavascript=disabled \
        -Drubberband=disabled -Dvapoursynth=disabled -Dlibarchive=disabled -Dlibbluray=disabled \
        -Ddvdnav=disabled -Dcdda=disabled -Duchardet=disabled -Dzimg=disabled -Dlcms2=disabled \
        -Dlibavdevice=disabled -Djack=disabled -Dopenal=disabled -Dpipewire=disabled \
        -Dpulse=disabled -Dwasapi=disabled -Dsndio=disabled -Doss-audio=disabled \
        -Dsdl2-audio=disabled -Dsdl2-video=disabled -Dsdl2-gamepad=disabled -Dcaca=disabled \
        -Dsixel=disabled -Dd3d11=disabled -Ddirect3d=disabled -Degl=disabled -Degl-angle=disabled \
        -Degl-drm=disabled -Degl-wayland=disabled -Degl-x11=disabled -Dgl-win32=disabled \
        -Dgl-x11=disabled -Dvaapi=disabled -Dvdpau=disabled -Dshaderc=disabled \
        -Dspirv-cross=disabled -Dvideotoolbox-pl=disabled -Dx11-clipboard=disabled \
        -Dplain-gl=disabled -Dvector=disabled -Dwin32-threads=disabled -Dzlib=enabled \
        -Diconv=enabled -Dhtml-build=disabled -Dtests=false
    meson compile -C build-mpv
    meson install -C build-mpv
fi

# ---------- 6. Stage into src-tauri/mpv_runtime ----------
"$REPO_ROOT/scripts/stage-libmpv-macos.sh"

echo "Build complete."