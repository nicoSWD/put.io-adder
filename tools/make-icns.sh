#!/bin/sh
# Builds the static icon files from the light icon set made by tools/make-app-icon.py:
#   assets/AppIcon.icns            macOS bundle icon (what Finder shows and the Dock shows before the app runs)
#   assets/icon/putio-adder.ico    Windows installer / shortcut icon
# The results are committed, so the release runners need no Python. While the app runs, it switches
# between the light and dark icon to match its theme (see crates/putio-adder/src/app.rs).
set -eu
cd "$(dirname "$0")/.."
src=assets/icon
set_dir=$(mktemp -d)/AppIcon.iconset
mkdir -p "$set_dir"
cp $src/app-light-16.png   "$set_dir/icon_16x16.png"
cp $src/app-light-32.png   "$set_dir/icon_16x16@2x.png"
cp $src/app-light-32.png   "$set_dir/icon_32x32.png"
cp $src/app-light-64.png   "$set_dir/icon_32x32@2x.png"
cp $src/app-light-128.png  "$set_dir/icon_128x128.png"
cp $src/app-light-256.png  "$set_dir/icon_128x128@2x.png"
cp $src/app-light-256.png  "$set_dir/icon_256x256.png"
cp $src/app-light-512.png  "$set_dir/icon_256x256@2x.png"
cp $src/app-light-512.png  "$set_dir/icon_512x512.png"
cp $src/app-light-1024.png "$set_dir/icon_512x512@2x.png"
iconutil -c icns "$set_dir" -o assets/AppIcon.icns
python3 tools/make-ico.py $src/putio-adder.ico $src/app-light-16.png $src/app-light-24.png $src/app-light-32.png $src/app-light-48.png $src/app-light-64.png $src/app-light-128.png $src/app-light-256.png
rm -rf "$(dirname "$set_dir")"
echo "wrote assets/AppIcon.icns and $src/putio-adder.ico"
