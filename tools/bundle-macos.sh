#!/bin/sh
# Builds "put.io adder.app" (release build).
#
#   tools/bundle-macos.sh [TARGET_TRIPLE]       (set PUTIO_BINARY=path to skip the build)
#
# Output: target/<triple>/release/... is the build; the bundle is written to dist/<triple>/put.io adder.app
# (or target/put.io adder.app when no triple is given, for local use).
# Signing is a separate step: tools/sign-macos.sh. Without it the bundle is ad-hoc signed, which is
# enough to run locally.
set -eu
cd "$(dirname "$0")/.."

triple="${1:-}"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
[ -n "$version" ] || { echo "could not read the version from Cargo.toml" >&2; exit 1; }

if [ -n "$triple" ]; then
    binary="target/$triple/release/putio-adder"
    app="dist/$triple/put.io adder.app"
    build="cargo build --locked --release -p putio-adder --target $triple"
else
    binary="target/release/putio-adder"
    app="target/put.io adder.app"
    build="cargo build --locked --release -p putio-adder"
fi
# PUTIO_BINARY: bundle a binary that was already built (the release workflow builds once, with its
# reproducibility flags, and must ship exactly that binary rather than rebuild without them).
if [ -n "${PUTIO_BINARY:-}" ]; then
    binary="$PUTIO_BINARY"
    [ -f "$binary" ] || { echo "no such binary: $binary" >&2; exit 1; }
else
    $build
fi

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/Licenses"
cp "$binary" "$app/Contents/MacOS/putio-adder"
[ -f assets/AppIcon.icns ] || tools/make-icns.sh
cp assets/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
# The font is under the SIL OFL, which wants its text to travel with every copy.
cp LICENSE "$app/Contents/Resources/Licenses/LICENSE-putio-adder.txt"
cp assets/fonts/OFL-Montserrat.txt "$app/Contents/Resources/Licenses/OFL-Montserrat.txt"

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
  <key>NSHumanReadableCopyright</key><string>Copyright © 2013-2026 Nicolas Oelgart. MIT licence.</string>
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

codesign --force --sign - "$app" >/dev/null 2>&1
echo "built: $app"
