#!/bin/sh
# Tests check_map.gd, the command-line map check of both importers (docs/tiled-extension-plan.md,
# section 3): runs res://runtime/check_map.gd and, with this platform's native library,
# res://addons/yati_native/check_map.gd on fixtures and compares their YATI_CHECK lines (the
# result line without its last field, the time) and exit codes with the expected ones; both
# scripts must print the same lines. test/run_all.sh runs it after importing the project.
# Usage: GODOT=/path/to/godot test/test_check_map.sh
cd "$(dirname "$0")/.."
GODOT="${GODOT:-godot}"
FIX=res://test/fixtures
SCRIPTS="res://runtime/check_map.gd"
if bash tools/build_native.sh --check >/dev/null 2>&1; then
	SCRIPTS="$SCRIPTS res://addons/yati_native/check_map.gd"
else
	echo "test_check_map.sh: no native library for this platform; only the runtime's script is checked"
fi
failed=0
passed=0

# check <name> <expected exit code> <expected lines> -- <check_map.gd arguments...>
check() {
	name="$1"
	want_code="$2"
	want="$3"
	shift 4
	for script in $SCRIPTS; do
		out=$("$GODOT" --headless --path . -s "$script" -- "$@" 2>/dev/null)
		code=$?
		got=$(printf '%s\n' "$out" | grep '^YATI_CHECK ' | sed 's/^\(YATI_CHECK result [0-9]* [0-9]* [0-9]* [0-9]*\) [0-9]*$/\1/')
		if [ "$code" != "$want_code" ]; then
			echo "FAIL check_map $name ($script): exit code $code, expected $want_code"
			failed=$((failed + 1))
		elif [ "$got" != "$want" ]; then
			echo "FAIL check_map $name ($script): the YATI_CHECK lines differ"
			echo "--- expected"
			printf '%s\n' "$want"
			echo "--- got"
			printf '%s\n' "$got"
			failed=$((failed + 1))
		elif ! printf '%s\n' "$out" | grep -q '^YATI_CHECK result [0-9]* [0-9]* [0-9]* [0-9]* [0-9]*$'; then
			echo "FAIL check_map $name ($script): the result line has no time"
			failed=$((failed + 1))
		else
			echo "PASS check_map $name ($script)"
			passed=$((passed + 1))
		fi
	done
}

CLASS_TAIL="is not a Godot node type -> using 'staticbody'. Set 'godot_node_type', map the class to a scene ('class_scenes') or set the option 'unknown_class_fallback' (see Reference)."

# Two unknown classes: a warning each, with the id of the class's first object (Door: FrontDoor 1,
# Lever: 4)
check unknown_class 0 "YATI_CHECK warning 1 Class 'Door' $CLASS_TAIL
YATI_CHECK warning 4 Class 'Lever' $CLASS_TAIL
YATI_CHECK result 0 2 11 0" -- "$FIX/unknown_class.tmx" --warn_unknown_classes=true

# Turned off by the option: no lines but the result
check unknown_class_off 0 "YATI_CHECK result 0 0 11 0" -- "$FIX/unknown_class.tmx" --warn_unknown_classes=false

# The scenes fixture without its project: the classes Enemy (Grunt, 1) and enemy (Plain, 4)
check scenes 0 "YATI_CHECK warning 1 Class 'Enemy' $CLASS_TAIL
YATI_CHECK warning 4 Class 'enemy' $CLASS_TAIL
YATI_CHECK result 0 2 15 0" -- "$FIX/scenes.tmx"

# With its project and a class mapped by an option: a property message names its object (Broken, 6)
check scenes_project 0 "YATI_CHECK warning 6 Property 'hp' of 'Broken': a Tiled 'string' value does not fit its type int. -> Stored as metadata
YATI_CHECK result 0 1 10 0" -- "$FIX/scenes.tmx" --project=$FIX/scenes.tiled-project '--class_scenes={"enemy": "res://test/fixtures/enemy.tscn"}'

# Object references: two warnings on Door (Area), id 1
check object_refs 0 "YATI_CHECK warning 1 Object property 'missing' of 'Door (Area)' names no imported object (id 999). -> Empty NodePath
YATI_CHECK warning 1 Object property 'unset' of 'Door (Area)' is not set. -> Empty NodePath
YATI_CHECK result 0 2 12 0" -- "$FIX/object_refs.tmx"

# An error: exit code 1
check missing_tileset 1 "YATI_CHECK error - Tileset file 'does_not_exist.tsx' not found. -> Continuing but result may be unusable
YATI_CHECK result 1 0 3 0" -- "$FIX/missing_tileset.tmx"

# From a zip archive, with tile cells
check zip 0 "YATI_CHECK result 0 0 4 9" -- maps/zipmap.tmx --zip=$FIX/bundle.zip

# No map: exit code 2
check no_map 2 "YATI_CHECK error - Tiled map file 'res://test/fixtures/nope.tmx' not found.
YATI_CHECK result 1 0 0 0" -- "$FIX/nope.tmx"

# Wrong arguments: an error line each, no import, exit code 2
check bad_options 2 "YATI_CHECK error - Unknown importer option 'bogus'.
YATI_CHECK error - Importer option 'warn_unknown_classes': 'maybe' is no bool.
YATI_CHECK result 2 0 0 0" -- "$FIX/unknown_class.tmx" --bogus=1 --warn_unknown_classes=maybe

echo ""
echo "SUMMARY: check_map $passed passed, $failed failed"
[ "$failed" -eq 0 ]
