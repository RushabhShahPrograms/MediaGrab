#!/usr/bin/env bash
# Builds MediaGrab.app and a .dmg. Run from the repo root on macOS.
# Usage: packaging/macos/build-dmg.sh <version> <rust-target> <label>
#   e.g. packaging/macos/build-dmg.sh 0.1.0 aarch64-apple-darwin arm64
set -euo pipefail

VERSION="${1:?version}"; TARGET="${2:?rust target}"; LABEL="${3:?label}"
APP="dist/MediaGrab.app"
SRC_ICON="crates/mediagrab-app/assets/icon.png"

rm -rf "$APP" dist/dmg-root; mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "target/$TARGET/release/mediagrab" "$APP/Contents/MacOS/mediagrab"
cp -R dist/tools "$APP/Contents/Resources/tools"
sed "s/__VERSION__/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"

# icon.png -> AppIcon.icns
ICONSET="$(mktemp -d)/AppIcon.iconset"; mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s         "$SRC_ICON" --out "$ICONSET/icon_${s}x${s}.png"    >/dev/null
  sips -z $((s*2)) $((s*2)) "$SRC_ICON" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"

# Ad-hoc signing (no Apple Developer account). Apple Silicon refuses to run unsigned native code.
for t in ffmpeg deno; do codesign --force --sign - "$APP/Contents/Resources/tools/$t" || true; done
codesign --force --deep --sign - "$APP"

# .dmg with the usual "drag to Applications" layout
mkdir -p dist/dmg-root
cp -R "$APP" dist/dmg-root/
ln -s /Applications dist/dmg-root/Applications
hdiutil create -volname "MediaGrab" -srcfolder dist/dmg-root -ov -format UDZO \
  "dist/MediaGrab-$VERSION-macos-$LABEL.dmg"
echo "Created dist/MediaGrab-$VERSION-macos-$LABEL.dmg"
