#!/usr/bin/env bash
# Builds dist/Flatline.app (release, ad-hoc signed).
# The bundle is required: macOS only grants "System Audio Recording" to an app
# that declares NSAudioCaptureUsageDescription in its Info.plist.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

cargo build --release

if [ ! -f assets/AppIcon.icns ]; then
  mkdir -p assets
  ICONSET=$(mktemp -d)/AppIcon.iconset
  target/release/flatline --write-iconset "$ICONSET"
  iconutil -c icns "$ICONSET" -o assets/AppIcon.icns
fi

APP=dist/Flatline.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/flatline "$APP/Contents/MacOS/flatline"
[ -f assets/AppIcon.icns ] && cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Flatline</string>
  <key>CFBundleDisplayName</key><string>Flatline</string>
  <key>CFBundleIdentifier</key><string>dev.flatline.app</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>CFBundleExecutable</key><string>flatline</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>LSMinimumSystemVersion</key><string>14.2</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSAudioCaptureUsageDescription</key>
  <string>Flatline captures system audio to apply your headphone calibration before it reaches your headphones.</string>
</dict>
</plist>
PLIST

# Ad-hoc signature with an identifier-based designated requirement, so the
# "System Audio Recording" permission survives rebuilds (instead of being
# tied to the binary hash).
codesign --force --sign - --identifier dev.flatline.app \
  -r='designated => identifier "dev.flatline.app"' "$APP"
echo "Built $APP"
