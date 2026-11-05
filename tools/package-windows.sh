#!/bin/sh
# Packs the Windows binary into a zip and an installer (Inno Setup: tools/install-inno-setup.sh).
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
cp assets/icon/putio-adder.ico "$stage/putio-adder.ico"

rm -f "dist/$name.zip"
(cd "$stage" && 7z a -tzip -mx=9 -bd "../$name.zip" putio-adder.exe putio-adder.ico LICENSE licenses >/dev/null)
echo "wrote dist/$name.zip"

iscc=""
for candidate in \
    "${LOCALAPPDATA:-}/Programs/Inno Setup 6/ISCC.exe" \
    "/c/Program Files (x86)/Inno Setup 6/ISCC.exe" \
    "/c/Program Files/Inno Setup 6/ISCC.exe"; do
    if [ -x "$candidate" ]; then iscc="$candidate"; break; fi
done
[ -n "$iscc" ] || iscc="$(command -v ISCC.exe || true)"
[ -n "$iscc" ] || { echo "Inno Setup (ISCC.exe) is not installed: run tools/install-inno-setup.sh" >&2; exit 1; }

# Git Bash would rewrite the /D switches as paths.
export MSYS2_ARG_CONV_EXCL='*'
"$iscc" /Q "/DAppVersion=$version" "/DArch=$arch" "/DSourceDir=$(cygpath -w "$(realpath "$stage")")" "$(cygpath -w packaging/windows/putio-adder.iss)"
[ -s "dist/$name-setup.exe" ] || { echo "dist/$name-setup.exe was not produced" >&2; exit 1; }
echo "wrote dist/$name-setup.exe"
