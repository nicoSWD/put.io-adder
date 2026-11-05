#!/bin/sh
# Verifies, on a Mac, that magnet links and .torrent files reach the app through the Apple Event
# handlers in putio-platform, for both a cold launch (app not running) and a warm one.
# It bundles examples/intake_probe.rs as a throw-away .app; nothing is installed or registered
# permanently beyond a Launch Services entry for that bundle.
set -eu
cd "$(dirname "$0")/.."

LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
work=$(mktemp -d "${TMPDIR:-/tmp}/putio-probe.XXXXXX")
app="$work/Probe.app"
# The bundle registers itself as a magnet: handler candidate. Always undo that, even on failure.
cleanup() {
    pkill -f "$app/Contents/MacOS/Probe" 2>/dev/null || true
    "$LSREGISTER" -u "$app" 2>/dev/null || true
    rm -rf "$work"
}
trap cleanup EXIT INT TERM
log=/tmp/putio-adder-probe.log
hash=0123456789abcdef0123456789abcdef01234567

cargo build --locked -p putio-platform --example intake_probe
mkdir -p "$app/Contents/MacOS"
cp target/debug/examples/intake_probe "$app/Contents/MacOS/Probe"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Probe</string>
<key>CFBundleIdentifier</key><string>dev.putio-adder.probe</string>
<key>CFBundleExecutable</key><string>Probe</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleShortVersionString</key><string>0.0.0</string>
<key>CFBundleURLTypes</key><array><dict>
 <key>CFBundleURLName</key><string>magnet</string>
 <key>CFBundleURLSchemes</key><array><string>magnet</string></array>
</dict></array>
<key>CFBundleDocumentTypes</key><array><dict>
 <key>CFBundleTypeExtensions</key><array><string>torrent</string></array>
 <key>CFBundleTypeName</key><string>BitTorrent Document</string>
 <key>CFBundleTypeRole</key><string>Viewer</string>
 <key>LSHandlerRank</key><string>Alternate</string>
</dict></array>
</dict></plist>
PLIST
codesign --force --sign - "$app" >/dev/null 2>&1
"$LSREGISTER" -f "$app"

printf 'd8:announce0:4:infod4:name1:aee' > "$work/Sample Show.torrent"
stop() { pkill -f "$app/Contents/MacOS/Probe" 2>/dev/null || true; sleep 1; }

: > "$log"; stop
echo "cold magnet";  open -a "$app" "magnet:?xt=urn:btih:$hash&dn=Cold+Magnet"; sleep 4
echo "warm magnet";  open -a "$app" "magnet:?xt=urn:btih:$hash&dn=Warm+Magnet"; sleep 2
echo "warm torrent"; open -a "$app" "$work/Sample Show.torrent"; sleep 2
stop
echo "cold torrent"; open -a "$app" "$work/Sample Show.torrent"; sleep 4
stop

cat "$log"
fail=0
for expected in "ok: Cold Magnet" "ok: Warm Magnet" "ok: Sample Show.torrent" "badge set: true"; do
    grep -q "$expected" "$log" || { echo "MISSING: $expected"; fail=1; }
done
cold=$(grep -c "Sample Show.torrent" "$log" || true)
[ "$cold" -ge 2 ] || { echo "MISSING: torrent delivered in both warm and cold launches (saw $cold)"; fail=1; }
[ "$fail" -eq 0 ] && echo "PASS" || { echo "FAIL"; exit 1; }
