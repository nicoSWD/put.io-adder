#!/bin/sh
# Builds assets/AppIcon.icns (macOS), assets/icon/putio-adder.ico (Windows) and assets/icon/icon-256.png
# (window icon) from the icon sources. The results are committed, so the release runners need no Python.
# 16/32/64/128 px use the original hand-tuned small sizes; larger ones are scaled from the 1024 px master.
set -eu
cd "$(dirname "$0")/.."
src=assets/icon
set_dir=$(mktemp -d)/AppIcon.iconset
mkdir -p "$set_dir"
cp $src/old-16.png  "$set_dir/icon_16x16.png"
cp $src/old-32.png  "$set_dir/icon_16x16@2x.png"
cp $src/old-32.png  "$set_dir/icon_32x32.png"
cp $src/old-64.png  "$set_dir/icon_32x32@2x.png"
cp $src/old-128.png "$set_dir/icon_128x128.png"
sips -z 256  256  $src/source-1024.png --out "$set_dir/icon_128x128@2x.png" >/dev/null
sips -z 256  256  $src/source-1024.png --out "$set_dir/icon_256x256.png" >/dev/null
sips -z 512  512  $src/source-1024.png --out "$set_dir/icon_256x256@2x.png" >/dev/null
sips -z 512  512  $src/source-1024.png --out "$set_dir/icon_512x512.png" >/dev/null
cp $src/source-1024.png "$set_dir/icon_512x512@2x.png"
iconutil -c icns "$set_dir" -o assets/AppIcon.icns
sips -z 256 256 $src/source-1024.png --out $src/icon-256.png >/dev/null
python3 tools/make-ico.py $src/putio-adder.ico $src/old-16.png $src/old-32.png $src/old-64.png $src/old-128.png $src/icon-256.png
rm -rf "$(dirname "$set_dir")"
echo "wrote assets/AppIcon.icns and $src/icon-256.png"
