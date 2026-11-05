#!/bin/sh
# Wraps a .app into a compressed disk image with an /Applications shortcut, using only hdiutil
# (no third-party packaging tool in the release path).
#
#   tools/make-dmg.sh "dist/<triple>/put.io adder.app" out.dmg
set -eu
app="$1"
out="$2"
[ -d "$app" ] || { echo "no such app: $app" >&2; exit 1; }

staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
rm -f "$out"
hdiutil create -volname "put.io adder" -srcfolder "$staging" -fs HFS+ -format UDZO -imagekey zlib-level=9 -ov "$out" >/dev/null
hdiutil verify "$out" >/dev/null
echo "wrote $out"
