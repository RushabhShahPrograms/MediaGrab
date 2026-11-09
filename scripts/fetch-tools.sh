#!/usr/bin/env bash
# Downloads yt-dlp + ffmpeg (+ ffprobe where available) + deno into dist/tools
# so the installer can ship them. Usage: scripts/fetch-tools.sh <windows|macos|linux> <x64|arm64>
set -euo pipefail

OS="${1:?usage: fetch-tools.sh <windows|macos|linux> <x64|arm64>}"
ARCH="${2:-x64}"
OUT="dist/tools"
TMP="$(mktemp -d)"
GH="https://github.com"

rm -rf "$OUT"; mkdir -p "$OUT"

unz() { # unz <zip> <dest>
  if command -v unzip >/dev/null 2>&1; then unzip -q -o "$1" -d "$2"; else 7z x -y -o"$2" "$1" >/dev/null; fi
}
get() { curl -fL --retry 3 --retry-delay 2 -o "$2" "$1"; }

case "$OS" in
  windows)
    get "$GH/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe" "$OUT/yt-dlp.exe"
    get "$GH/yt-dlp/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-win64-gpl.zip" "$TMP/ff.zip"
    unz "$TMP/ff.zip" "$TMP/ff"
    cp "$TMP"/ff/*/bin/ffmpeg.exe "$TMP"/ff/*/bin/ffprobe.exe "$OUT/"
    get "$GH/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip" "$TMP/deno.zip"
    unz "$TMP/deno.zip" "$OUT"
    ;;
  linux)
    get "$GH/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux" "$OUT/yt-dlp"
    get "$GH/yt-dlp/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-linux64-gpl.tar.xz" "$TMP/ff.tar.xz"
    mkdir -p "$TMP/ff" && tar -xJf "$TMP/ff.tar.xz" -C "$TMP/ff"
    cp "$TMP"/ff/*/bin/ffmpeg "$TMP"/ff/*/bin/ffprobe "$OUT/"
    get "$GH/denoland/deno/releases/latest/download/deno-x86_64-unknown-linux-gnu.zip" "$TMP/deno.zip"
    unz "$TMP/deno.zip" "$OUT"
    ;;
  macos)
    get "$GH/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos" "$OUT/yt-dlp"
    if [ "$ARCH" = "arm64" ]; then
      get "$GH/eugeneware/ffmpeg-static/releases/download/b6.0/ffmpeg-darwin-arm64" "$OUT/ffmpeg"
      DENO="deno-aarch64-apple-darwin.zip"
    else
      get "$GH/eugeneware/ffmpeg-static/releases/download/b6.0/ffmpeg-darwin-x64" "$OUT/ffmpeg"
      DENO="deno-x86_64-apple-darwin.zip"
    fi
    get "$GH/denoland/deno/releases/latest/download/$DENO" "$TMP/deno.zip"
    unz "$TMP/deno.zip" "$OUT"
    ;;
  *) echo "unknown OS: $OS" >&2; exit 1 ;;
esac

chmod +x "$OUT"/* 2>/dev/null || true
echo "Bundled tools:"; ls -lh "$OUT"
