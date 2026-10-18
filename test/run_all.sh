#!/bin/sh
# Runs the native importer's smoke test, the YATI runtime test suite, the benchmark and (with the
# native library) the native importer's async test headless.
# Usage: GODOT=/path/to/godot test/run_all.sh [--runtime=res://runtime]
#        (--runtime=res://addons/yati_native tests the native importer's facade)
set -e
cd "$(dirname "$0")/.."
GODOT="${GODOT:-godot}"
RUNTIME="${1:---runtime=res://runtime}"
EXTENSION="res://addons/yati_native/yati_native.gdextension"
status=0

# Godot loads a GDExtension at startup only when .godot/extension_list.cfg names it, and the
# first --import of a fresh .godot/ that discovers one mid-scan aborts at shutdown (exit 134,
# Godot 4.7.2). Naming it before the import avoids both. Only when this platform's library is
# in place, since an entry whose library is missing makes Godot print an error on every start.
HAVE_NATIVE=false
if bash tools/build_native.sh --check >/dev/null 2>&1; then
	HAVE_NATIVE=true
	mkdir -p .godot
	if ! grep -qxF "$EXTENSION" .godot/extension_list.cfg 2>/dev/null; then
		echo "$EXTENSION" >> .godot/extension_list.cfg
	fi
fi

# The fixtures' images are imported textures (their .import files and .godot/ are not
# committed), so a fresh clone needs an import; on an imported checkout it is quick.
"$GODOT" --headless --path . --import >/dev/null 2>&1 || {
	code=$?
	echo "run_all.sh: godot --import failed (exit $code); run it by hand to see why" >&2
	exit 1
}

if [ "$HAVE_NATIVE" = true ]; then
	"$GODOT" --headless --path . -s tools/smoke_native.gd || status=$?
elif [ "$RUNTIME" = "--runtime=res://addons/yati_native" ]; then
	echo "run_all.sh: the native library is missing (bash tools/build_native.sh, or bash tools/install_native.sh)" >&2
	exit 1
else
	echo "run_all.sh: no native library for this platform; its smoke test is skipped (bash tools/build_native.sh, or bash tools/install_native.sh)"
fi

"$GODOT" --headless -s test/run_tests.gd -- "$RUNTIME" || status=$?
"$GODOT" --headless -s test/benchmark.gd -- "$RUNTIME"

# The native importer's asynchronous import and shared TileSet cache (stage 3 of
# docs/native-importer-plan.md), on the benchmark's map: needs the library, not a runtime.
if [ "$HAVE_NATIVE" = true ]; then
	"$GODOT" --headless -s test/test_async.gd || status=$?
fi
exit $status
