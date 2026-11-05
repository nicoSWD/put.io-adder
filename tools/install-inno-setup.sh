#!/bin/sh
# Installs Inno Setup on a Windows runner (Git Bash) from the author's own GitHub release, after
# checking the file's SHA-256 against the digest pinned here. No package feed involved.
#
# To update: read the new digest from the release page / `gh api repos/jrsoftware/issrc/releases/tags/is-X_Y_Z`,
# compare it with your own download, then change both lines below.
set -eu
version=6.7.4
sha256=a549929319f9120716677c685535ca4671e4b937b5fdc87e2585f32d8eb30e8c
tag="is-$(printf '%s' "$version" | tr . _)"
file="${RUNNER_TEMP:-/tmp}/innosetup-$version.exe"

curl -fsSL --retry 3 -o "$file" "https://github.com/jrsoftware/issrc/releases/download/$tag/innosetup-$version.exe"
echo "$sha256  $file" | sha256sum -c -
# Per-user install: no administrator rights needed.
"$file" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /CURRENTUSER
echo "Inno Setup $version installed"
