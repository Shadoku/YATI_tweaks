extends SceneTree
## Import benchmark for the YATI runtime.
##
##   godot --headless -s test/benchmark.gd -- --runtime=res://runtime
##   godot --headless -s test/benchmark.gd -- --runtime=res://baseline_runtime --iterations=5
##
## User args (after "--"):
##   --runtime=res://dir   runtime folder to benchmark (default res://runtime)
##   --iterations=N        measured imports after one warm-up (default 3)
##   --regen               regenerate the benchmark map even if it exists
##   --with-duplicate      also time Node.duplicate() of the result (what the old Runtime.md told
##                         users to do)
##
## On first run a large map is generated into res://test/out/bench/ (gitignored):
## 320x320 cells, 6 tile layers (base64+zlib) over 3 tilesets, ~50% of cells flipped,
## 2400 objects with 3 properties each in 4 object layers (10% tile objects whose tiles carry
## 3 properties), 320 instances of one template. A companion map with only the tile layers is
## used to split the import time into phases:
##   parse        DataLoader + DictionaryBuilder on the full map
##   tilesets     TilesetCreator.create_from_dictionary_array on the parsed tilesets
##   tile layers  import(tiles-only map) - its parse - tilesets
##   objects      the rest (object layers, templates, metadata)

const OUT_DIR := "res://test/out/bench"
const MAP_FILE := "bench.tmx"
const TILES_ONLY_FILE := "bench_tiles_only.tmx" # same tile layers, no objects
const MAP_W := 320
const MAP_H := 320
const TILE := 16
const TILE_LAYERS := 6
const TILESETS := ["bench_a", "bench_b", "bench_c"]
const TILES_PER_SET := 64 # 8x8 tiles per tileset image
const OBJECT_LAYERS := 4
const OBJECTS_PER_LAYER := 600
const TEMPLATE_INSTANCES := 320
const FILL_RATIO := 0.85
const FLIP_RATIO := 0.5

const FLIP_H := 0x80000000
const FLIP_V := 0x40000000
const FLIP_D := 0x20000000

var runtime_dir := "res://runtime"
var iterations := 3
var regen := false
var with_duplicate := false


func _init() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--runtime="):
			runtime_dir = arg.substr(10).trim_suffix("/")
		elif arg.begins_with("--iterations="):
			iterations = max(1, int(arg.substr(13)))
		elif arg == "--regen":
			regen = true
		elif arg == "--with-duplicate":
			with_duplicate = true

	var map_path := OUT_DIR.path_join(MAP_FILE)
	var tiles_path := OUT_DIR.path_join(TILES_ONLY_FILE)
	if regen or not FileAccess.file_exists(map_path) or not FileAccess.file_exists(tiles_path):
		var t0 := Time.get_ticks_usec()
		if not generate():
			printerr("benchmark map generation failed")
			quit(2)
			return
		print("generated %s in %.0f ms" % [map_path, (Time.get_ticks_usec() - t0) / 1000.0])

	var importer_script: GDScript = load(runtime_dir.path_join("Importer.gd"))
	if importer_script == null:
		printerr("cannot load " + runtime_dir.path_join("Importer.gd"))
		quit(2)
		return
	# The phases of a GDScript runtime are timed with its own scripts; a runtime without them (the
	# native importer's facade, addons/yati_native/) is timed with the native classes instead:
	# parse = TiledMap.load, tilesets = TiledTileSets.tileset_for.
	var native := not ResourceLoader.exists(runtime_dir.path_join("DataLoader.gd")) and ClassDB.class_exists("TiledMap")
	var data_loader: GDScript = null
	var dict_builder: GDScript = null
	var tileset_creator: GDScript = null
	if not native:
		data_loader = load(runtime_dir.path_join("DataLoader.gd"))
		dict_builder = load(runtime_dir.path_join("DictionaryBuilder.gd"))
		tileset_creator = load(runtime_dir.path_join("TilesetCreator.gd"))

	print("YATI benchmark  runtime=%s  map=%s  iterations=%d (+1 warm-up)" % [runtime_dir, map_path, iterations])
	print("Godot %s" % Engine.get_version_info()["string"])

	var totals: Array[float] = []
	var parses: Array[float] = []
	var tilesets: Array[float] = []
	var tile_layers: Array[float] = []
	var objects: Array[float] = []
	var dups: Array[float] = []
	var stats := ""
	for i in range(iterations + 1):
		var parse_ms := time_parse_native(map_path) if native else time_parse(data_loader, dict_builder, map_path)
		var tiles_parse_ms := time_parse_native(tiles_path) if native else time_parse(data_loader, dict_builder, tiles_path)
		var tileset_ms := time_tilesets_native(map_path) if native else time_tilesets(data_loader, dict_builder, tileset_creator, map_path)

		var tiles_only := time_import(importer_script, tiles_path)
		free_tree(tiles_only[1])

		var full := time_import(importer_script, map_path)
		var total_ms: float = full[0]
		var node: Node = full[1]
		if node == null:
			printerr("import returned null")
			quit(1)
			return
		var tile_layer_ms: float = tiles_only[0] - tiles_parse_ms - tileset_ms
		var object_ms: float = total_ms - parse_ms - tileset_ms - tile_layer_ms

		var dup_ms := 0.0
		if with_duplicate:
			var t := Time.get_ticks_usec()
			var copy := node.duplicate()
			dup_ms = (Time.get_ticks_usec() - t) / 1000.0
			copy.free()

		if i == 0:
			stats = describe(node)
		free_tree(node)

		var label := "warm-up" if i == 0 else "run %d" % i
		var line := "  %-8s total %8.1f ms | parse %7.1f | tilesets %6.1f | tile layers %8.1f | objects %8.1f" % [
			label, total_ms, parse_ms, tileset_ms, tile_layer_ms, object_ms]
		if with_duplicate:
			line += " | duplicate %8.1f" % dup_ms
		print(line)
		if i > 0:
			totals.append(total_ms)
			parses.append(parse_ms)
			tilesets.append(tileset_ms)
			tile_layers.append(tile_layer_ms)
			objects.append(object_ms)
			dups.append(dup_ms)

	print("  result: " + stats)
	print("RESULT runtime=%s total_ms min=%.1f avg=%.1f | parse avg=%.1f | tilesets avg=%.1f | tile layers avg=%.1f | objects avg=%.1f%s" % [
		runtime_dir, minf_arr(totals), avg(totals), avg(parses), avg(tilesets), avg(tile_layers), avg(objects),
		(" | duplicate avg=%.1f" % avg(dups)) if with_duplicate else ""])
	quit(0)


## Read + parse a map into the Tiled dictionary (ms).
static func time_parse(data_loader: GDScript, dict_builder: GDScript, path: String) -> float:
	var t := Time.get_ticks_usec()
	var content: Variant = data_loader.call("get_tiled_file_content", path, path.get_base_dir())
	var _dict: Variant = dict_builder.new().call("get_dictionary", content, path)
	return (Time.get_ticks_usec() - t) / 1000.0


## TileSet creation from the (already parsed) tileset list of a map (ms).
static func time_tilesets(data_loader: GDScript, dict_builder: GDScript, tileset_creator: GDScript, path: String) -> float:
	var content: Variant = data_loader.call("get_tiled_file_content", path, path.get_base_dir())
	var dict: Dictionary = dict_builder.new().call("get_dictionary", content, path)
	var t := Time.get_ticks_usec()
	var tc: Object = tileset_creator.new()
	tc.call("set_base_path", path)
	tc.call("set_map_parameters", Vector2i(TILE, TILE))
	tc.call("set_custom_data_prefix", "data_")
	var _ts: Variant = tc.call("create_from_dictionary_array", dict["tilesets"])
	return (Time.get_ticks_usec() - t) / 1000.0


## The native importer's parse: TiledMap.load (the map, its tilesets and templates) (ms).
static func time_parse_native(path: String) -> float:
	var t := Time.get_ticks_usec()
	var map: RefCounted = ClassDB.instantiate("TiledMap")
	map.call("load", path, "")
	return (Time.get_ticks_usec() - t) / 1000.0


## The native importer's TileSet creation for an (already loaded) map (ms).
static func time_tilesets_native(path: String) -> float:
	var map: RefCounted = ClassDB.instantiate("TiledMap")
	map.call("load", path, "")
	var t := Time.get_ticks_usec()
	var tilesets: RefCounted = ClassDB.instantiate("TiledTileSets")
	var _ts: Variant = tilesets.call("tileset_for", map, null)
	return (Time.get_ticks_usec() - t) / 1000.0


## Full import through the public API: [ms, node].
static func time_import(importer_script: GDScript, path: String) -> Array:
	var importer: Object = importer_script.new()
	var t := Time.get_ticks_usec()
	var node: Node = importer.call("import", path, "")
	return [(Time.get_ticks_usec() - t) / 1000.0, node]


static func avg(a: Array[float]) -> float:
	var s := 0.0
	for v in a:
		s += v
	return s / max(a.size(), 1)


static func minf_arr(a: Array[float]) -> float:
	var m := INF
	for v in a:
		m = minf(m, v)
	return m


static func free_tree(n: Node) -> void:
	if n == null: return
	var top := n
	while top.get_parent() != null:
		top = top.get_parent()
	top.free()


static func describe(n: Node) -> String:
	var layers := 0
	var cells := 0
	var nodes := 0
	var stack: Array[Node] = [n]
	while not stack.is_empty():
		var c: Node = stack.pop_back()
		nodes += 1
		if c is TileMapLayer:
			layers += 1
			cells += (c as TileMapLayer).get_used_cells().size()
		stack.append_array(c.get_children())
	return "%d nodes, %d tile layers, %d cells" % [nodes, layers, cells]


# ---------------------------------------------------------------------------
# Map generation

func write_text(file_name: String, content: String) -> bool:
	var f := FileAccess.open(OUT_DIR.path_join(file_name), FileAccess.WRITE)
	if f == null:
		printerr("cannot write " + file_name)
		return false
	f.store_string(content)
	f.close()
	return true


static func prop(name: String, type: String, value: String) -> String:
	var t := "" if type == "string" else ' type="%s"' % type
	return '<property name="%s"%s value="%s"/>' % [name, t, value]


func generate() -> bool:
	DirAccess.make_dir_recursive_absolute(ProjectSettings.globalize_path(OUT_DIR))
	var rng := RandomNumberGenerator.new()
	rng.seed = 20261017

	# Tilesets: 8x8 tiles of 16x16; tiles 0-7 have 3 properties (tile objects), tile 9 has a
	# collision rectangle (exercises collision copies on flipped alternative tiles).
	for ts_i in range(TILESETS.size()):
		var ts_name: String = TILESETS[ts_i]
		var img := Image.create(8 * TILE, 8 * TILE, false, Image.FORMAT_RGBA8)
		for t in range(TILES_PER_SET):
			var col := Color.from_hsv(float(t) / TILES_PER_SET, 0.4 + 0.2 * ts_i, 0.9)
			img.fill_rect(Rect2i((t % 8) * TILE, (t / 8) * TILE, TILE, TILE), col)
		if img.save_png(OUT_DIR.path_join(ts_name + ".png")) != OK:
			return false
		var s := '<?xml version="1.0" encoding="UTF-8"?>\n'
		s += '<tileset version="1.10" tiledversion="1.11.2" name="%s" tilewidth="16" tileheight="16" tilecount="64" columns="8">\n' % ts_name
		s += ' <image source="%s.png" width="128" height="128"/>\n' % ts_name
		for t in range(8):
			s += ' <tile id="%d">\n  <properties>\n' % t
			s += '   ' + prop("loot", "string", "item_%d_%d" % [ts_i, t]) + '\n'
			s += '   ' + prop("weight", "float", "%.1f" % (t * 0.5)) + '\n'
			s += '   ' + prop("stack", "int", str(t + 1)) + '\n'
			s += '  </properties>\n </tile>\n'
		s += ' <tile id="9">\n  <objectgroup draworder="index">\n'
		s += '   <object id="1" x="0" y="8" width="16" height="8"/>\n'
		s += '  </objectgroup>\n </tile>\n'
		s += '</tileset>\n'
		if not write_text(ts_name + ".tsx", s):
			return false

	var first_gids: Array[int] = []
	for ts_i in range(TILESETS.size()):
		first_gids.append(1 + ts_i * TILES_PER_SET)

	# Template: a tile object with two properties
	var tpl := '<?xml version="1.0" encoding="UTF-8"?>\n<template>\n'
	tpl += ' <tileset firstgid="1" source="bench_a.tsx"/>\n'
	tpl += ' <object name="Minion" gid="12" width="16" height="16">\n  <properties>\n'
	tpl += '   ' + prop("hp", "int", "10") + '\n   ' + prop("faction", "string", "orcs") + '\n'
	tpl += '  </properties>\n </object>\n</template>\n'
	if not write_text("bench_tpl.tx", tpl):
		return false

	var map := PackedStringArray()
	map.append('<?xml version="1.0" encoding="UTF-8"?>')
	map.append('<map version="1.10" tiledversion="1.11.2" orientation="orthogonal" renderorder="right-down" width="%d" height="%d" tilewidth="16" tileheight="16" infinite="0" nextlayerid="%d" nextobjectid="%d">' % [
		MAP_W, MAP_H, TILE_LAYERS + OBJECT_LAYERS + 2, OBJECT_LAYERS * OBJECTS_PER_LAYER + TEMPLATE_INSTANCES + 1])
	for ts_i in range(TILESETS.size()):
		map.append(' <tileset firstgid="%d" source="%s.tsx"/>' % [first_gids[ts_i], TILESETS[ts_i]])

	var layer_id := 1
	for l in range(TILE_LAYERS):
		var ints := PackedInt32Array()
		ints.resize(MAP_W * MAP_H)
		for i in range(MAP_W * MAP_H):
			if rng.randf() >= FILL_RATIO:
				ints[i] = 0
				continue
			# Each layer prefers one tileset but mixes in the others.
			var ts_i := l % TILESETS.size() if rng.randf() < 0.7 else rng.randi_range(0, TILESETS.size() - 1)
			var gid := first_gids[ts_i] + rng.randi_range(0, TILES_PER_SET - 1)
			if rng.randf() < FLIP_RATIO:
				var flags := 0
				while flags == 0:
					flags = (FLIP_H if rng.randf() < 0.5 else 0) | (FLIP_V if rng.randf() < 0.5 else 0) | (FLIP_D if rng.randf() < 0.5 else 0)
				gid |= flags
			ints[i] = gid
		var blob := Marshalls.raw_to_base64(ints.to_byte_array().compress(FileAccess.COMPRESSION_DEFLATE))
		map.append(' <layer id="%d" name="Tiles%d" width="%d" height="%d">' % [layer_id, l, MAP_W, MAP_H])
		map.append('  <data encoding="base64" compression="zlib">')
		map.append('   ' + blob)
		map.append('  </data>')
		map.append(' </layer>')
		layer_id += 1

	var tiles_only := map.duplicate()
	tiles_only.append('</map>')
	if not write_text(TILES_ONLY_FILE, "\n".join(tiles_only) + "\n"):
		return false

	var obj_id := 1
	var px_w := MAP_W * TILE
	var px_h := MAP_H * TILE
	for l in range(OBJECT_LAYERS):
		map.append(' <objectgroup id="%d" name="Objects%d">' % [layer_id, l])
		layer_id += 1
		for o in range(OBJECTS_PER_LAYER):
			var x := rng.randi_range(0, px_w - 32)
			var y := rng.randi_range(0, px_h - 32)
			var props := '  <properties>\n   %s\n   %s\n   %s\n  </properties>\n' % [
				prop("label", "string", "obj_%d" % obj_id), prop("score", "int", str(rng.randi_range(0, 1000))),
				prop("speed", "float", "%.2f" % rng.randf_range(0.0, 10.0))]
			var r := rng.randf()
			var head := '  <object id="%d" name="O%d" x="%d" y="%d"' % [obj_id, obj_id, x, y]
			if r < 0.10:
				# tile object whose tile carries 3 properties
				var ts_i := rng.randi_range(0, TILESETS.size() - 1)
				var gid := first_gids[ts_i] + rng.randi_range(0, 7)
				if rng.randf() < 0.3:
					gid |= FLIP_H
				map.append(head + ' gid="%d" width="16" height="16">\n%s  </object>' % [gid, props])
			elif r < 0.50:
				var cls: String = ["", "area", "staticbody", ""][rng.randi_range(0, 3)]
				var cls_attr := "" if cls == "" else ' class="%s"' % cls
				map.append(head + '%s width="%d" height="%d">\n%s  </object>' % [cls_attr, rng.randi_range(8, 64), rng.randi_range(8, 64), props])
			elif r < 0.65:
				map.append(head + ' width="24" height="16">\n%s  <ellipse/>\n  </object>' % props)
			elif r < 0.80:
				map.append(head + '>\n%s  <point/>\n  </object>' % props)
			else:
				map.append(head + '>\n%s  <polygon points="0,0 32,0 32,24 8,32"/>\n  </object>' % props)
			obj_id += 1
		map.append(' </objectgroup>')

	map.append(' <objectgroup id="%d" name="Templates">' % layer_id)
	layer_id += 1
	for t in range(TEMPLATE_INSTANCES):
		var x := rng.randi_range(0, px_w - 16)
		var y := rng.randi_range(16, px_h)
		if t % 4 == 0:
			map.append('  <object id="%d" template="bench_tpl.tx" name="M%d" x="%d" y="%d">\n  <properties>\n   %s\n  </properties>\n  </object>' % [
				obj_id, obj_id, x, y, prop("hp", "int", str(t))])
		else:
			map.append('  <object id="%d" template="bench_tpl.tx" x="%d" y="%d"/>' % [obj_id, x, y])
		obj_id += 1
	map.append(' </objectgroup>')
	map.append('</map>')
	return write_text(MAP_FILE, "\n".join(map) + "\n")
