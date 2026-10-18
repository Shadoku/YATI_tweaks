#!/usr/bin/env bash
# native.yml's check of addons/yati_native/bin/ after the build jobs' artifacts were gathered
# into it: every platform's library is there, each matches the SHA-256 its build job wrote
# next to it (<job>.sha256), and each is a sound image (tools/check_native_image.py). The
# checksum files are removed afterwards, so neither the zip nor the commit carries them.
# Exits 1 on any failure.
#
#   bash tools/verify_native_bin.sh [directory]   # default: addons/yati_native/bin
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${1:-$ROOT/addons/yati_native/bin}"
ls -laR .
LIBS=(
  libyati_native.linux.x86_64-unknown-linux-gnu.so
  libyati_native.windows.x86_64-pc-windows-msvc.dll
  libyati_native.macos.framework/libyati_native.macos.dylib
)
for lib in "${LIBS[@]}" libyati_native.macos.framework/Resources/Info.plist; do
  test -f "$lib" || { echo "verify_native_bin.sh: $lib is missing" >&2; exit 1; }
done
shopt -s nullglob
sums=(*.sha256)
if [ "${#sums[@]}" -ne 3 ]; then
  echo "verify_native_bin.sh: expected 3 checksum files, found ${#sums[@]}: ${sums[*]}" >&2
  exit 1
fi
for sum in "${sums[@]}"; do
  sha256sum -c "$sum"
done
rm -f -- *.sha256
python3 -I "$ROOT/tools/check_native_image.py" "${LIBS[@]}"
echo "verify_native_bin.sh: every library is the one its build job made"
