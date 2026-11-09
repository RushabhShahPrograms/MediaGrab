#!/usr/bin/env bash
# Offline smoke test: needs only yt-dlp, ffmpeg, python3 and cargo. No YouTube/SoundCloud access.
# It generates small media files, serves them on http://127.0.0.1:8765, then downloads them
# in parallel through the real engine (so it exercises queueing, progress, conversion, tagging).
set -euo pipefail
cd "$(dirname "$0")/.."
WORK="$(mktemp -d)"; trap 'kill $SRV 2>/dev/null || true; rm -rf "$WORK/srv"' EXIT
mkdir -p "$WORK/srv" && cd "$WORK/srv"
for n in alpha beta gamma delta; do
  ffmpeg -loglevel error -y -f lavfi -i "sine=frequency=$((300 + RANDOM % 600)):duration=15" -c:a libmp3lame "$n.mp3"
done
ffmpeg -loglevel error -y -f lavfi -i testsrc=duration=5:size=320x240:rate=15 -f lavfi -i sine=duration=5 \
  -c:v libx264 -pix_fmt yuv420p -c:a aac -shortest clip.mp4
python3 -m http.server 8765 --bind 127.0.0.1 >/dev/null 2>&1 & SRV=$!
sleep 1
cd - >/dev/null
OUT="$WORK/out"
echo "== audio (4 files, 2 at a time) =="
cargo run -q -p mediagrab-core --bin mediagrab-cli -- --audio --format mp3 --parallel 2 --out "$OUT/audio" \
  http://127.0.0.1:8765/alpha.mp3 http://127.0.0.1:8765/beta.mp3 http://127.0.0.1:8765/gamma.mp3 http://127.0.0.1:8765/delta.mp3
echo "== video (title only, no thumbnail) =="
cargo run -q -p mediagrab-core --bin mediagrab-cli -- --video --height 720 --out "$OUT/video" http://127.0.0.1:8765/clip.mp4
echo "== result =="; find "$OUT" -type f | sort
echo "(output kept in $OUT)"; trap 'kill $SRV 2>/dev/null || true' EXIT
