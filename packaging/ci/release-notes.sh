#!/bin/sh
# Checks that a release tag matches the workspace version and the changelog, and prints that
# version's section of CHANGELOG.md (without its heading), for the GitLab release's description.
#
#   packaging/ci/release-notes.sh v0.1.0 > release-notes.md
set -eu
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
tag="$1"
version="${tag#v}"
cargo="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml")"
if [ "$version" != "$cargo" ]; then
  echo "error: the tag $tag is not the workspace version $cargo (Cargo.toml)" >&2
  exit 1
fi
notes="$(awk -v v="$version" '
  /^## / { if (inside) exit; if ($2 == v) { inside = 1; next } }
  inside { print }
' "$ROOT/CHANGELOG.md")"
if [ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]; then
  echo "error: CHANGELOG.md has no section \"## $version\"" >&2
  exit 1
fi
if ! grep -q "<release version=\"$version\"" "$ROOT/packaging/linux/ai.storyteller.texcraft.metainfo.xml"; then
  echo "error: the metainfo has no <release version=\"$version\">" >&2
  exit 1
fi
printf '%s\n' "$notes"
