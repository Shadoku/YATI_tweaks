#!/usr/bin/env bash
# Download the Godot version pinned in .godot-version (or $GODOT_VERSION) into
# ~/.cache/godot/<version>/godot (on Windows, from Git Bash, the console build
# where the zip put it) and print that path on the last line.
# Idempotent: a cached, working binary is reused. Used by CI (.github/workflows/ci.yml, native.yml).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${GODOT_VERSION:-$(cat "$ROOT/.godot-version" 2>/dev/null || echo 4.7.2-stable)}"
CACHE="${GODOT_CACHE_DIR:-$HOME/.cache/godot}"
DEST="$CACHE/$VERSION"
BIN="$DEST/godot"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)  ASSET="Godot_v${VERSION}_linux.x86_64" ;;
  Linux-aarch64) ASSET="Godot_v${VERSION}_linux.arm64" ;;
  Darwin-*)      ASSET="Godot_v${VERSION}_macos.universal" ;;
  MINGW64_NT-*-x86_64|MSYS_NT-*-x86_64|CYGWIN_NT-*-x86_64)
    ASSET="Godot_v${VERSION}_win64.exe"
    # Windows (Git Bash): the console build is a wrapper that must keep its name
    # next to the main executable, so it is used where the zip put it.
    BIN="$DEST/${ASSET%.exe}_console.exe" ;;
  *) echo "install_godot.sh: unsupported platform $(uname -s)-$(uname -m); install Godot manually and set GODOT=/path/to/godot" >&2; exit 1 ;;
esac

if [ -x "$BIN" ] && "$BIN" --headless --version >/dev/null 2>&1; then
  echo "$BIN"
  exit 0
fi

URL="https://github.com/godotengine/godot/releases/download/${VERSION}/${ASSET}.zip"
echo "Downloading $URL" >&2
mkdir -p "$DEST"
TMP="$DEST/download.zip"
curl -fsSL --retry 3 --retry-delay 2 -o "$TMP" "$URL"
unzip -o -q "$TMP" -d "$DEST"
rm -f "$TMP"

if [ -d "$DEST/Godot.app" ]; then
  ln -sf "$DEST/Godot.app/Contents/MacOS/Godot" "$BIN"
elif [ -f "$BIN" ] && [ "$BIN" != "$DEST/godot" ]; then
  : # Windows: the console executable, in place.
else
  chmod +x "$DEST/$ASSET"
  ln -sf "$DEST/$ASSET" "$BIN"
fi

"$BIN" --headless --version >&2
echo "$BIN"
