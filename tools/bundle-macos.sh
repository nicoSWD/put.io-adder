#!/bin/sh
# Builds "put.io adder.app" in target/ (release build, ad-hoc signed).
# Ad-hoc signing is enough to run locally; a distributable build is signed and notarized by the
# release pipeline with a Developer ID.
set -eu
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
[ -n "$version" ] || { echo "could not read the version from Cargo.toml" >&2; exit 1; }

cargo build --locked --release -p putio-adder

app="target/put.io adder.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/putio-adder "$app/Contents/MacOS/putio-adder"
[ -f assets/AppIcon.icns ] && cp assets/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>put.io adder</string>
  <key>CFBundleDisplayName</key><string>put.io adder</string>
  <key>CFBundleIdentifier</key><string>es.nico.putio-adder</string>
  <key>CFBundleExecutable</key><string>putio-adder</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleURLTypes</key>
  <array><dict>
    <key>CFBundleURLName</key><string>BitTorrent magnet link</string>
    <key>CFBundleURLSchemes</key><array><string>magnet</string></array>
  </dict></array>
  <key>CFBundleDocumentTypes</key>
  <array><dict>
    <key>CFBundleTypeName</key><string>BitTorrent Document</string>
    <key>CFBundleTypeExtensions</key><array><string>torrent</string></array>
    <key>CFBundleTypeRole</key><string>Viewer</string>
    <key>LSHandlerRank</key><string>Alternate</string>
  </dict></array>
</dict>
</plist>
PLIST

codesign --force --sign - "$app"
echo "built: $app"
