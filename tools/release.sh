#!/bin/sh
# Prepares a release: checks, version bump, commit and a SIGNED tag. It never pushes.
#
#   tools/release.sh 0.2.0            stable release
#   tools/release.sh 0.2.0-rc.1       pre-release (published as a GitHub pre-release)
#   tools/release.sh 0.2.0 --dry-run  run every check, change nothing
#
# Afterwards: git push origin master v0.2.0   (the tag push starts .github/workflows/release.yml)
set -eu
cd "$(dirname "$0")/.."

version="${1:-}"
dry_run=no
[ "${2:-}" = "--dry-run" ] && dry_run=yes
case "$version" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) echo "usage: $0 X.Y.Z[-rc.N] [--dry-run]" >&2; exit 2 ;;
esac
printf '%s' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$' || { echo "not a semantic version: $version" >&2; exit 2; }
tag="v$version"

current=$(awk '/^\[/ { in_pkg = ($0 == "[workspace.package]") } in_pkg && /^version = "/ { gsub(/^version = "|"$/, ""); print; exit }' Cargo.toml)
[ "$current" != "$version" ] || { echo "Cargo.toml is already at $version" >&2; exit 1; }
git rev-parse -q --verify "refs/tags/$tag" >/dev/null && { echo "tag $tag already exists" >&2; exit 1; }

branch=$(git rev-parse --abbrev-ref HEAD)
[ "$branch" = "master" ] || { echo "release from master (you are on $branch)" >&2; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "the working tree is not clean" >&2; exit 1; }
if git remote get-url origin >/dev/null 2>&1; then
    git fetch --quiet origin master
    [ "$(git rev-parse HEAD)" = "$(git rev-parse origin/master)" ] || { echo "master differs from origin/master: pull or push first" >&2; exit 1; }
fi
# A tag can only be signed when git knows how: fail now, not after the commit.
git config --get user.signingkey >/dev/null || [ "$(git config --get tag.gpgSign || true)" = "true" ] || \
    [ "$dry_run" = "yes" ] || { echo "no signing key configured (git config user.signingkey / gpg.format ssh)" >&2; exit 1; }

echo "checks…"
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
for t in tools/test_*.py; do python3 -I "$t"; done
sha256sum_check() { (cd crates/putio-api/tests/spec && shasum -a 256 -c putio-2.8.14.json.sha256 >/dev/null); }
sha256sum_check

if [ "$dry_run" = "yes" ]; then
    echo "dry run: would bump $current -> $version, commit 'Release $tag' and create the signed tag $tag"
    exit 0
fi

# Bump the workspace version, refresh the lockfile for the workspace crates only, re-check.
python3 - "$current" "$version" <<'PY'
import re, sys
current, new = sys.argv[1:]
text = open("Cargo.toml").read()
text = re.sub(r'(\[workspace\.package\][^\[]*?\nversion = ")' + re.escape(current) + '"', r'\g<1>' + new + '"', text, count=1, flags=re.S)
open("Cargo.toml", "w").write(text)
PY
cargo update --workspace --offline >/dev/null 2>&1 || cargo update --workspace >/dev/null
cargo check --workspace --locked

git add Cargo.toml Cargo.lock
git commit -m "Release $tag"
git tag -s "$tag" -m "put.io adder $version"
git tag -v "$tag" >/dev/null 2>&1 || echo "warning: could not verify the tag signature locally"
echo
echo "Created $tag. Review it, then publish with:"
echo "    git push origin master $tag"
