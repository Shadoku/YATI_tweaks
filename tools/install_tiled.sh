#!/usr/bin/env bash
# Download the Tiled version pinned in .tiled-version (or $TILED_VERSION) into
# ~/.cache/tiled/<version>/ and print, on the last line, the path of a `tiled` wrapper that runs
# it. Linux x86_64 only: it takes the release's AppImage and extracts it (--appimage-extract, so
# no FUSE is needed). On another platform install Tiled yourself and set TILED=<binary>.
# Idempotent: an extracted, present copy is reused. Used by CI (.github/workflows/ci.yml) and by
# tiled-extension/test/real_tiled.test.mjs, which runs the extension inside this Tiled
# (tiled --project ... --evaluate ...) and skips when no Tiled is found.
#
# The AppImage ships Qt's xcb platform plugin only, so running it needs an X display: on a
# headless machine use `xvfb-run -a` (the test does). It also needs the system's libEGL, libGL,
# libxkbcommon(-x11), fontconfig and the libxcb-* libraries a Qt 6 desktop has (on Ubuntu:
# apt-get install libegl1 libxkbcommon-x11-0 libxcb-cursor0 libxcb-icccm4 libxcb-keysyms1
# libxcb-shape0 libxcb-xkb1 libxcb-image0 libxcb-render-util0, plus xvfb).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${TILED_VERSION:-$(cat "$ROOT/.tiled-version" 2>/dev/null || echo 1.12.0)}"
CACHE="${TILED_CACHE_DIR:-$HOME/.cache/tiled}"
DEST="$CACHE/$VERSION"
BIN="$DEST/tiled"
APPRUN="$DEST/squashfs-root/AppRun"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) ASSET="Tiled-${VERSION}_Linux_x86_64.AppImage" ;;
  *) echo "install_tiled.sh: only Linux x86_64 has an AppImage; install Tiled $VERSION yourself and set TILED=/path/to/tiled" >&2; exit 1 ;;
esac

if [ -x "$BIN" ] && [ -x "$APPRUN" ]; then
  echo "$BIN"
  exit 0
fi

URL="https://github.com/mapeditor/tiled/releases/download/v${VERSION}/${ASSET}"
echo "Downloading $URL" >&2
mkdir -p "$DEST"
TMP="$DEST/$ASSET"
curl -fsSL --retry 3 --retry-delay 2 -o "$TMP" "$URL"
chmod +x "$TMP"
rm -rf "$DEST/squashfs-root"
(cd "$DEST" && "./$ASSET" --appimage-extract >/dev/null)
rm -f "$TMP"
[ -x "$APPRUN" ] || { echo "install_tiled.sh: the AppImage did not extract to $DEST/squashfs-root" >&2; exit 1; }

# The AppImage's AppRun takes the program to run first: tiled, tmxrasterizer, tmxviewer.
cat > "$BIN" <<WRAPPER
#!/usr/bin/env bash
# Tiled $VERSION, extracted from its AppImage by tools/install_tiled.sh. Needs an X display
# (xvfb-run -a on a headless machine).
exec "$APPRUN" tiled "\$@"
WRAPPER
chmod +x "$BIN"
echo "$BIN"
