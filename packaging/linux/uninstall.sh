#!/bin/sh
# Removes what install.sh put in your home directory. Your put.io sign-in lives in the system
# keyring (entry "put.io adder"); remove it there if you want it gone too.
set -eu
prefix="${XDG_DATA_HOME:-$HOME/.local/share}"
id=es.nico.putio-adder
rm -f "$HOME/.local/bin/putio-adder" \
      "$prefix/applications/$id.desktop" \
      "$prefix/icons/hicolor/256x256/apps/$id.png" \
      "$prefix/metainfo/$id.metainfo.xml"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$prefix/applications" || true
echo "Removed put.io adder."
