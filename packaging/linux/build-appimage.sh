#!/usr/bin/env bash
# Builds a portable AppImage. Run from the repo root on Linux (x86_64).
# Usage: packaging/linux/build-appimage.sh <version>
set -euo pipefail

VERSION="${1:?version}"
APPDIR="dist/AppDir"

rm -rf "$APPDIR"; mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/lib/mediagrab"
cp target/release/mediagrab "$APPDIR/usr/bin/mediagrab"
cp -R dist/tools "$APPDIR/usr/lib/mediagrab/tools"      # found by tools.rs via ../lib/mediagrab/tools
cp packaging/linux/mediagrab.desktop "$APPDIR/mediagrab.desktop"
cp crates/mediagrab-app/assets/icon.png "$APPDIR/mediagrab.png"
cp crates/mediagrab-app/assets/icon.png "$APPDIR/.DirIcon"

cat > "$APPDIR/AppRun" << 'RUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/mediagrab" "$@"
RUN
chmod +x "$APPDIR/AppRun"

curl -fL --retry 3 -o dist/appimagetool \
  https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
chmod +x dist/appimagetool

# APPIMAGE_EXTRACT_AND_RUN: CI runners have no FUSE
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 dist/appimagetool "$APPDIR" "dist/MediaGrab-$VERSION-linux-x86_64.AppImage"
echo "Created dist/MediaGrab-$VERSION-linux-x86_64.AppImage"
