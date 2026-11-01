#!/usr/bin/env bash
# Builds "Claude Companion.app" and a drag-to-Applications DMG in dist/.
#
#   scripts/build-dmg.sh
#
# Uses only tools that ship with macOS and Xcode (sips, iconutil, codesign,
# hdiutil). The app is signed ad hoc, which is enough to run it on this Mac;
# sharing it without Gatekeeper warnings needs a Developer ID signature and
# notarization.
set -euo pipefail

cd "$(dirname "$0")/.."

name="Claude Companion"
executable="claude-companion"
identifier="dev.dylan.claude-companion"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
minimum_macos="13.0"

dist="dist"
app="$dist/$name.app"
dmg="$dist/Claude-Companion-$version.dmg"

echo "Building release binary…"
cargo build --release

echo "Assembling $app…"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "target/release/$executable" "$app/Contents/MacOS/$executable"

# LSUIElement keeps the app out of the Dock and app switcher; it also sets
# the accessory activation policy at runtime, which covers `cargo run`.
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$name</string>
  <key>CFBundleDisplayName</key><string>$name</string>
  <key>CFBundleIdentifier</key><string>$identifier</string>
  <key>CFBundleExecutable</key><string>$executable</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>$minimum_macos</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

echo "Making the icon…"
iconset="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" assets/icon/AppIcon.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" assets/icon/AppIcon.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"
rm -rf "$(dirname "$iconset")"

echo "Signing (ad hoc)…"
codesign --force --deep --sign - "$app"

echo "Packing $dmg…"
staging="$(mktemp -d)"
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
rm -f "$dmg"
hdiutil create -volname "$name" -srcfolder "$staging" -ov -format UDZO "$dmg" >/dev/null
rm -rf "$staging"

echo "Done: $dmg"
