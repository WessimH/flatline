#!/usr/bin/env bash
# Builds dist/Flatline.app (release, signed).
#
#   ./scripts/bundle.sh                     build and sign
#   ./scripts/bundle.sh --reset-permission  also reset the System Audio Recording
#                                           permission, so macOS asks again on the
#                                           next launch (only needed if it was refused)
# The bundle is required: macOS only grants "System Audio Recording" to an app
# that declares NSAudioCaptureUsageDescription in its Info.plist.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

RESET=0
for arg in "$@"; do
  case "$arg" in
    --reset-permission) RESET=1 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

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

# The "System Audio Recording" permission is keyed to the bundle identifier,
# which both signing modes keep stable, so it survives rebuilds. The optional
# "Flatline Dev" identity (scripts/make-signing-identity.sh) gives the app a
# real signature instead of an ad-hoc one.
IDENTITY="Flatline Dev"
if security find-identity -v -p codesigning | grep -q "\"$IDENTITY\""; then
  codesign --force --sign "$IDENTITY" --identifier dev.flatline.app "$APP"
  echo "Built $APP (signed with \"$IDENTITY\")"
else
  codesign --force --sign - --identifier dev.flatline.app \
    -r='designated => identifier "dev.flatline.app"' "$APP"
  echo "Built $APP (ad-hoc signed)"
fi

if [ "$RESET" = 1 ]; then
  tccutil reset All dev.flatline.app >/dev/null
  echo "System Audio Recording permission reset: launch Flatline and click Allow."
fi
