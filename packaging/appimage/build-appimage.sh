#!/usr/bin/env bash
# Assembles TeXCraft's AppDir from release binaries and packs it into an AppImage with appimagetool.
#
#   cargo build --release -p texcraft -p texcraft-cli      # with CRAFT_FONTS_DIR, see docs/releasing.md
#   APPIMAGETOOL=/path/to/appimagetool packaging/appimage/build-appimage.sh [OUT_DIR]
#
# Environment:
#   APPIMAGETOOL   appimagetool to run (an extracted AppRun or the AppImage itself). Required.
#   RUNTIME_FILE   the AppImage runtime to embed (type2-runtime's runtime-x86_64). Optional: without
#                  it appimagetool downloads the newest one, so CI always passes a pinned file.
#   TARGET_DIR     where the binaries are (default: target/release).
#   VERSION        the version in the file name (default: the workspace version from Cargo.toml).
#
# The AppDir holds the two binaries, the desktop entry, the icons and the metainfo, and no shared
# libraries: the binaries link only glibc (libc, libm, libgcc_s) and open the rest at run time
# (winit: libwayland-client, libxkbcommon, libX11 and its extensions; wgpu: libvulkan or libEGL and
# the system's graphics driver), which a desktop always has and which must come from the system to
# match its display server and driver. The oldest glibc the binaries need is printed at the end and
# is that of the build machine (Debian 12 in CI: glibc 2.36).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ID="io.gitlab.severin_lemaignan.TeXCraft"
ARCH="${ARCH:-x86_64}"
OUT_DIR="${1:-$ROOT/target/appimage}"
TARGET_DIR="${TARGET_DIR:-$ROOT/target/release}"
VERSION="${VERSION:-$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml")}"
: "${APPIMAGETOOL:?set APPIMAGETOOL to appimagetool (see docs/releasing.md)}"

for bin in texcraft texcraft-cli; do
  [ -x "$TARGET_DIR/$bin" ] || { echo "error: $TARGET_DIR/$bin is missing; build the release binaries first" >&2; exit 1; }
done

APPDIR="$OUT_DIR/TeXCraft.AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR"

# The programs.
install -Dm755 "$TARGET_DIR/texcraft" "$APPDIR/usr/bin/texcraft"
install -Dm755 "$TARGET_DIR/texcraft-cli" "$APPDIR/usr/bin/texcraft-cli"
install -Dm755 "$ROOT/packaging/appimage/AppRun" "$APPDIR/AppRun"

# The desktop entry, icons and metainfo where a desktop looks for them, and the entry and icon at
# the top of the AppDir, where appimagetool and AppImage integrators look.
install -Dm644 "$ROOT/packaging/linux/$ID.desktop" "$APPDIR/usr/share/applications/$ID.desktop"
install -Dm644 "$ROOT/packaging/linux/$ID.metainfo.xml" "$APPDIR/usr/share/metainfo/$ID.metainfo.xml"
for dir in "$ROOT"/assets/app-icon/hicolor/*/apps; do
  size="$(basename "$(dirname "$dir")")"
  for icon in "$dir/$ID".*; do
    install -Dm644 "$icon" "$APPDIR/usr/share/icons/hicolor/$size/apps/$(basename "$icon")"
  done
done
ln -s "usr/share/applications/$ID.desktop" "$APPDIR/$ID.desktop"
ln -s "usr/share/icons/hicolor/256x256/apps/$ID.png" "$APPDIR/$ID.png"
ln -s "$ID.png" "$APPDIR/.DirIcon"

# The checks appimagetool would otherwise skip when the tools are missing.
if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$APPDIR/usr/share/applications/$ID.desktop"
fi
if command -v appstreamcli >/dev/null; then
  appstreamcli validate --no-net "$APPDIR/usr/share/metainfo/$ID.metainfo.xml"
fi

OUTPUT="$OUT_DIR/TeXCraft-$VERSION-$ARCH.AppImage"
args=()
if [ -n "${RUNTIME_FILE:-}" ]; then
  args+=(--runtime-file "$RUNTIME_FILE")
fi
ARCH="$ARCH" "$APPIMAGETOOL" "${args[@]}" "$APPDIR" "$OUTPUT"

# What the AppImage asks of the system it runs on.
if command -v objdump >/dev/null; then
  glibc="$(objdump -T "$APPDIR/usr/bin/texcraft" "$APPDIR/usr/bin/texcraft-cli" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -n1)"
  echo "The AppImage needs ${glibc:-an unknown glibc} or newer."
fi
echo "Built $OUTPUT"
