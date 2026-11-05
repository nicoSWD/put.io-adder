#!/bin/sh
# Per-user install: no root needed, nothing outside your home directory.
#
#   ./install.sh                 install put.io adder
#   ./install.sh --set-default   also make it your handler for magnet links and .torrent files
set -eu
here=$(cd "$(dirname "$0")" && pwd)
prefix="${XDG_DATA_HOME:-$HOME/.local/share}"
bin="${HOME}/.local/bin"
id=es.nico.putio-adder

mkdir -p "$bin" "$prefix/applications" "$prefix/icons/hicolor/256x256/apps" "$prefix/metainfo"
install -m 755 "$here/putio-adder" "$bin/putio-adder"
install -m 644 "$here/$id.png" "$prefix/icons/hicolor/256x256/apps/$id.png"
install -m 644 "$here/$id.metainfo.xml" "$prefix/metainfo/$id.metainfo.xml"
# Desktop entries need an absolute Exec when the binary is not on the desktop's PATH.
sed "s|^Exec=putio-adder|Exec=$bin/putio-adder|" "$here/$id.desktop" > "$prefix/applications/$id.desktop"
chmod 644 "$prefix/applications/$id.desktop"

command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$prefix/applications" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$prefix/icons/hicolor" 2>/dev/null || true

if [ "${1:-}" = "--set-default" ]; then
    xdg-mime default "$id.desktop" x-scheme-handler/magnet application/x-bittorrent
    echo "put.io adder is now your handler for magnet links and .torrent files."
fi
echo "Installed to $bin/putio-adder"
case ":$PATH:" in *":$bin:"*) ;; *) echo "Note: $bin is not on your PATH." ;; esac
