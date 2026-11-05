#!/bin/sh
# Packs the Windows binary into a zip and (when Inno Setup is available) an installer.
# Runs in Git Bash on the Windows runners.
#
#   tools/package-windows.sh EXE VERSION ARCH      (ARCH: x64 | arm64)
# Output: dist/putio-adder-v<VERSION>-windows-<ARCH>.zip and ...-setup.exe
set -eu
cd "$(dirname "$0")/.."
exe="$1"; version="$2"; arch="$3"
name="putio-adder-v$version-windows-$arch"
stage="dist/windows-$arch"

rm -rf "$stage"
mkdir -p "$stage/licenses"
cp "$exe" "$stage/putio-adder.exe"
cp LICENSE "$stage/LICENSE"
cp assets/fonts/OFL-Montserrat.txt "$stage/licenses/"
[ -f dist/THIRD_PARTY.txt ] && cp dist/THIRD_PARTY.txt "$stage/licenses/THIRD_PARTY.txt" && cp dist/THIRD_PARTY.txt "$stage/THIRD_PARTY.txt"
python3 tools/make-ico.py "$stage/putio-adder.ico" assets/icon/old-16.png assets/icon/old-32.png assets/icon/old-64.png assets/icon/old-128.png assets/icon/icon-256.png

rm -f "dist/$name.zip"
(cd "$stage" && 7z a -tzip -mx=9 -bd "../$name.zip" putio-adder.exe putio-adder.ico LICENSE licenses >/dev/null)
echo "wrote dist/$name.zip"

if command -v iscc >/dev/null 2>&1 || [ -x "/c/Program Files (x86)/Inno Setup 6/ISCC.exe" ]; then
    iscc="$(command -v iscc || echo "/c/Program Files (x86)/Inno Setup 6/ISCC.exe")"
    "$iscc" "/DAppVersion=$version" "/DArch=$arch" "/DSourceDir=$(cd "$stage" && pwd -W 2>/dev/null || pwd)" packaging/windows/putio-adder.iss
    echo "wrote dist/$name-setup.exe"
elif [ "$arch" = "x64" ]; then
    echo "Inno Setup is required for the x64 installer" >&2
    exit 1
else
    echo "warning: Inno Setup not found on this runner; no $arch installer" >&2
fi
