#!/usr/bin/env bash
# Installs TeXCraft for the current user: the desktop app in ~/.local/bin, its desktop entry and
# metainfo, and its icons in the hicolor theme, so it appears in the application menu with its icon
# and opens .tex files.
#
#   packaging/linux/install.sh            # builds the release binary first
#   PREFIX=/usr/local sudo -E packaging/linux/install.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
ID="ai.storyteller.texcraft"

(cd "$ROOT" && cargo build --release -p texcraft)
install -Dm755 "$ROOT/target/release/texcraft" "$PREFIX/bin/texcraft"
install -Dm644 "$ROOT/packaging/linux/$ID.desktop" "$PREFIX/share/applications/$ID.desktop"
install -Dm644 "$ROOT/packaging/linux/$ID.metainfo.xml" "$PREFIX/share/metainfo/$ID.metainfo.xml"
for dir in "$ROOT"/assets/app-icon/hicolor/*/apps; do
  size="$(basename "$(dirname "$dir")")"
  for icon in "$dir"/"$ID".*; do
    install -Dm644 "$icon" "$PREFIX/share/icons/hicolor/$size/apps/$(basename "$icon")"
  done
done
command -v update-desktop-database >/dev/null && update-desktop-database -q "$PREFIX/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" || true
echo "TeXCraft installed under $PREFIX"
