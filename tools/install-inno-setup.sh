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
# Git Bash rewrites arguments that start with "/" into Windows paths for native programs, so the installer
# would receive "C:/Program Files/Git/VERYSILENT" and open its wizard (and wait for a click). Switch that off.
export MSYS2_ARG_CONV_EXCL='*'
log="${RUNNER_TEMP:-/tmp}/innosetup-$version.log"
# Per-user install: no administrator rights needed. A silent install takes seconds: give it five minutes,
# then fail with the installer's own log instead of hanging until the job is cancelled.
if ! timeout 300 "$file" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /CURRENTUSER "/LOG=$log"; then
    echo "Inno Setup did not install (or took more than five minutes)." >&2
    [ -f "$log" ] && tail -40 "$log" >&2
    exit 1
fi
echo "Inno Setup $version installed"
