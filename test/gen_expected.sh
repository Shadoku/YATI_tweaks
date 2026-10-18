#!/bin/sh
# Regenerates test/fixtures/expected/*.json: the GDScript runtime's dictionary of every fixture
# map, tileset and template (test/dump_dictionary.gd), the parity target of native/tiled's
# tests/parity.rs. Run from anywhere: GODOT=/path/to/godot test/gen_expected.sh
set -e
cd "$(dirname "$0")/.."
GODOT="${GODOT:-godot}"
OUT=test/fixtures/expected
mkdir -p "$OUT"
dump() { # <file> <out name> [project] [--zip=...]
	"$GODOT" --headless -s test/dump_dictionary.gd -- "$1" ${3:+"$3"} ${4:+"$4"} --out="$OUT/$2" >/dev/null
	echo "$OUT/$2"
}
for f in background.tmx classes.tmx encodings.tmx hexagonal.tmx infinite.tmx infinite.tmj \
		isometric.tmx json_map.tmj json_twin.tmx layers.tmx legacy_xml.tmx missing_tileset.tmx \
		objects.tmx staggered.tmx tilesets.tmx \
		animations.tmx area_gravity.tmx clip.tmj clip.tmx colors.tmx layer_class.tmx \
		parallax.tmx single_layer.tmx template_files.tmx templates.tmx unknown_class.tmx \
		terrain.tsx iso.tsx json_tiles.tsj \
		enemy.tx crate.tx json_enemy.tj probe_rect.tx probe_text.tx probe_tpl.tx; do
	dump "test/fixtures/$f" "$f.json"
done
dump test/fixtures/tpl/door.tx tpl_door.tx.json
dump test/fixtures/objects.tmx objects.tmx+custom.json test/fixtures/custom.tiled-project
dump test/fixtures/classes.tmx classes.tmx+classes.json test/fixtures/classes.tiled-project
dump test/fixtures/json_tiles.tsj json_tiles.tsj+classes.json test/fixtures/classes.tiled-project
dump test/fixtures/terrain.tsx terrain.tsx+classes.json test/fixtures/classes.tiled-project
dump maps/zipmap.tmx "bundle.zip#maps_zipmap.tmx.json" "" --zip=test/fixtures/bundle.zip
dump maps/zip_tiles.tsx "bundle.zip#maps_zip_tiles.tsx.json" "" --zip=test/fixtures/bundle.zip
