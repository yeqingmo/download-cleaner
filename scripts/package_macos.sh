#!/bin/sh
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="Download Cleaner"
APP_DIR="$ROOT/dist/$APP_NAME.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RES_DIR="$CONTENTS_DIR/Resources"
STAGING_DIR="$ROOT/dist/dmg-root"
DMG_PATH="$ROOT/dist/$APP_NAME.dmg"
BIN_PATH="$ROOT/target/release/download-cleaner"

cargo build --release --bin download-cleaner --bin download-cleaner-launcher

rm -rf "$APP_DIR" "$STAGING_DIR" "$DMG_PATH"
mkdir -p "$MACOS_DIR" "$RES_DIR" "$STAGING_DIR"

cp "$BIN_PATH" "$RES_DIR/download-cleaner"
cp "$ROOT/assets/macos/Info.plist" "$CONTENTS_DIR/Info.plist"
if [ -f "$ROOT/assets/macos/AppIcon.icns" ]; then
  cp "$ROOT/assets/macos/AppIcon.icns" "$RES_DIR/AppIcon.icns"
else
  echo "warning: assets/macos/AppIcon.icns 不存在，应用将使用默认图标"
fi

cp "$ROOT/target/release/download-cleaner-launcher" "$MACOS_DIR/DownloadCleanerLauncher"
chmod +x "$MACOS_DIR/DownloadCleanerLauncher" "$RES_DIR/download-cleaner"

cp -R "$APP_DIR" "$STAGING_DIR/"
ln -s /Applications "$STAGING_DIR/Applications"

hdiutil create \
  -volname "$APP_NAME" \
  -srcfolder "$STAGING_DIR" \
  -ov \
  -format UDZO \
  "$DMG_PATH"

echo "Created: $DMG_PATH"
