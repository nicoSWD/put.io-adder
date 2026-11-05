#!/bin/sh
# Packs the Linux binary into a reproducible tarball.
#
#   tools/package-linux.sh BINARY VERSION ARCH      (ARCH: x86_64 | aarch64)
# Output: dist/putio-adder-v<VERSION>-linux-<ARCH>.tar.gz
set -eu
cd "$(dirname "$0")/.."
binary="$1"; version="$2"; arch="$3"
name="putio-adder-v$version-linux-$arch"
stage="dist/$name"
epoch="${SOURCE_DATE_EPOCH:-0}"

rm -rf "$stage" "dist/$name.tar.gz"
mkdir -p "$stage/licenses"
install -m 755 "$binary" "$stage/putio-adder"
install -m 644 packaging/linux/es.nico.putio-adder.desktop packaging/linux/es.nico.putio-adder.metainfo.xml "$stage/"
install -m 755 packaging/linux/install.sh packaging/linux/uninstall.sh "$stage/"
install -m 644 assets/icon/app-light-256.png "$stage/es.nico.putio-adder.png"
install -m 644 LICENSE "$stage/LICENSE"
install -m 644 assets/fonts/OFL-Montserrat.txt "$stage/licenses/OFL-Montserrat.txt"
[ -f dist/THIRD_PARTY.txt ] && install -m 644 dist/THIRD_PARTY.txt "$stage/licenses/THIRD_PARTY.txt"
cat > "$stage/README.txt" <<TXT
put.io adder $version (Linux, $arch)

  ./install.sh                 install for your user (no root)
  ./install.sh --set-default   ...and use it for magnet links and .torrent files
  ./uninstall.sh               remove it

Needs a desktop session, and a Secret Service (GNOME Keyring, KWallet) to remember your sign-in.
Verify this download: https://github.com/nicoSWD/putio-adder-rs/blob/main/SECURITY.md
TXT

# Reproducible archive: sorted names, fixed owner and time, no gzip timestamp.
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$epoch" -C dist -cf - "$name" | gzip -n -9 > "dist/$name.tar.gz"
rm -rf "$stage"
echo "wrote dist/$name.tar.gz"
