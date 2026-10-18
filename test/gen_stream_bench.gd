extends SceneTree
## Generates the streaming benchmark's maps (stage 4 of docs/native-importer-plan.md) into
## res://test/out/stream/ (gitignored), for test/bench_stream.gd:
##
##   stream.tmx          an infinite map of 4000 x 4000 cells in 16 x 16 chunks (base64 + zlib),
##                       4 tile layers: Ground (bench_a, every cell), Detail (bench_b, about a third
##                       of the cells, a third of those flipped), Walls (walls.tsx, whose every tile
##                       has a collision polygon: room outlines, about a fifth of the cells) and
##                       Deco (bench_c, sparse, in half of the chunks); one object layer, Things,
##                       an object in every third chunk (rectangles of class `area` with
##                       properties, points, tile objects, polygons; every tenth references the
##                       previous object, so references cross chunks)
##   stream_iso.tmx     the same as an isometric map of 1024 x 1024 cells (its layers y-sorted:
##                       the engine draws each cell on its own, no rendering quadrants)
##   world_<x>_<y>.tmx   3 x 3 finite maps of 256 x 256 cells on the same tilesets and layers
##   stream.world        the nine maps side by side
##   bench_a/b/c.tsx     the benchmark's tilesets (test/benchmark.gd's: 8 x 8 tiles, tiles 0-7
##                       with properties, tile 9 with a collision rectangle), walls.tsx (16 tiles,
##                       each with a full-tile collision rectangle), and their images
##
## Each layer's chunks are drawn from a few dozen precomputed variants (cheap to generate, still
## varied per chunk). Run from the repository root (bench_stream.gd runs it when the maps are
## missing):
##
##   godot --headless -s test/gen_stream_bench.gd [-- --size=4000 --world-size=256]

const OUT_DIR := "res://test/out/stream"
const TILE := 16
const CHUNK := 16
const VARIANTS := 32
const FLIP_H := 0x80000000
const FLIP_V := 0x40000000
const FLIP_D := 0x20000000
const TILESETS := ["bench_a", "bench_b", "bench_c"]
const WALL_TILES := 16

var size := 4000
var world_size := 256
var rng := RandomNumberGenerator.new()
# first gids: bench_a 1, bench_b 65, bench_c 129, walls 193
var first_gids: Array[int] = [1, 65, 129, 193]
# layer -> Array of PackedInt32Array chunk variants
var variants: Array = []


func _init() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--size="):
			size = maxi(CHUNK, int(arg.substr(7)))
		elif arg.begins_with("--world-size="):
			world_size = maxi(CHUNK, int(arg.substr(13)))
	rng.seed = 20261018
	var t0 := Time.get_ticks_msec()
	var ok := generate()
	print("gen_stream_bench: %s in %d ms (%s)" % ["done" if ok else "FAILED", Time.get_ticks_msec() - t0, OUT_DIR])
	quit(0 if ok else 1)


func generate() -> bool:
	DirAccess.make_dir_recursive_absolute(ProjectSettings.globalize_path(OUT_DIR))
	if not gen_tilesets():
		return false
	make_variants()
	if not gen_infinite("stream.tmx", "orthogonal", size):
		return false
	if not gen_infinite("stream_iso.tmx", "isometric", mini(size, 1024)):
		return false
	var world := PackedStringArray()
	for y in 3:
		for x in 3:
			var file := "world_%d_%d.tmx" % [x, y]
			if not gen_finite(file, x * 3 + y):
				return false
			world.append('        { "fileName": "%s", "x": %d, "y": %d, "width": %d, "height": %d }' % [
				file, x * world_size * TILE, y * world_size * TILE, world_size * TILE, world_size * TILE])
	var w := '{\n    "maps": [\n' + ",\n".join(world) + '\n    ],\n    "onlyShowAdjacentMaps": false,\n    "type": "world"\n}\n'
	return write_text("stream.world", w)


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


func gen_tilesets() -> bool:
	for ts_i in TILESETS.size():
		var ts_name: String = TILESETS[ts_i]
		var img := Image.create(8 * TILE, 8 * TILE, false, Image.FORMAT_RGBA8)
		for t in 64:
			img.fill_rect(Rect2i((t % 8) * TILE, (t / 8) * TILE, TILE, TILE), Color.from_hsv(float(t) / 64, 0.4 + 0.2 * ts_i, 0.9))
		if img.save_png(OUT_DIR.path_join(ts_name + ".png")) != OK:
			return false
		var s := '<?xml version="1.0" encoding="UTF-8"?>\n'
		s += '<tileset version="1.10" tiledversion="1.11.2" name="%s" tilewidth="16" tileheight="16" tilecount="64" columns="8">\n' % ts_name
		s += ' <image source="%s.png" width="128" height="128"/>\n' % ts_name
		for t in 8:
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
	var wimg := Image.create(4 * TILE, 4 * TILE, false, Image.FORMAT_RGBA8)
	for t in WALL_TILES:
		wimg.fill_rect(Rect2i((t % 4) * TILE, (t / 4) * TILE, TILE, TILE), Color(0.3 + 0.03 * t, 0.25, 0.2))
	if wimg.save_png(OUT_DIR.path_join("walls.png")) != OK:
		return false
	var w := '<?xml version="1.0" encoding="UTF-8"?>\n'
	w += '<tileset version="1.10" tiledversion="1.11.2" name="walls" tilewidth="16" tileheight="16" tilecount="%d" columns="4">\n' % WALL_TILES
	w += ' <image source="walls.png" width="64" height="64"/>\n'
	for t in WALL_TILES:
		w += ' <tile id="%d">\n  <objectgroup draworder="index">\n   <object id="1" x="0" y="0" width="16" height="16"/>\n  </objectgroup>\n </tile>\n' % t
	w += '</tileset>\n'
	return write_text("walls.tsx", w)


func tileset_lines() -> String:
	var s := ""
	for i in TILESETS.size():
		s += ' <tileset firstgid="%d" source="%s.tsx"/>\n' % [first_gids[i], TILESETS[i]]
	s += ' <tileset firstgid="%d" source="walls.tsx"/>\n' % first_gids[3]
	return s


## Chunk variants per layer (0 Ground, 1 Detail, 2 Walls, 3 Deco).
func make_variants() -> void:
	variants = [[], [], [], []]
	for v in VARIANTS:
		var ground := PackedInt32Array()
		var detail := PackedInt32Array()
		var walls := PackedInt32Array()
		var deco := PackedInt32Array()
		for a in [ground, detail, walls, deco]:
			a.resize(CHUNK * CHUNK)
		var base := first_gids[0] + rng.randi_range(16, 23)
		var door := rng.randi_range(3, 12)
		for y in CHUNK:
			for x in CHUNK:
				var i := y * CHUNK + x
				ground[i] = base if rng.randf() < 0.85 else first_gids[0] + rng.randi_range(0, 63)
				if rng.randf() < 0.33:
					var g := first_gids[1] + rng.randi_range(0, 15)
					if rng.randf() < 0.33:
						g |= [FLIP_H, FLIP_V, FLIP_H | FLIP_V, FLIP_D][rng.randi_range(0, 3)]
					detail[i] = g
				# A room outline along the chunk's top and left edges, with a door
				if (y == 0 or x == 0) and x != door and y != door:
					walls[i] = first_gids[3] + rng.randi_range(0, WALL_TILES - 1)
				elif rng.randf() < 0.07:
					walls[i] = first_gids[3] + rng.randi_range(0, WALL_TILES - 1)
				if rng.randf() < 0.08:
					deco[i] = first_gids[2] + rng.randi_range(0, 63)
		variants[0].append(ground)
		variants[1].append(detail)
		variants[2].append(walls)
		variants[3].append(deco)


static func encode(ints: PackedInt32Array) -> String:
	return Marshalls.raw_to_base64(ints.to_byte_array().compress(FileAccess.COMPRESSION_DEFLATE))


## The variant of a layer for a chunk, -1 for none (Deco is in half of the chunks).
func variant_of(layer: int, cx: int, cy: int) -> int:
	var h := absi(hash(Vector3i(cx, cy, layer)))
	if layer == 3 and (h >> 8) % 2 == 0:
		return -1
	return h % VARIANTS


const LAYER_NAMES := ["Ground", "Detail", "Walls", "Deco"]


func gen_infinite(file: String, orientation: String, cells: int) -> bool:
	var encoded: Array = [[], [], [], []]
	for l in 4:
		for v in VARIANTS:
			encoded[l].append(encode(variants[l][v]))
	var chunks := cells / CHUNK
	var out := PackedStringArray()
	out.append('<?xml version="1.0" encoding="UTF-8"?>')
	out.append('<map version="1.10" tiledversion="1.11.2" orientation="%s" renderorder="right-down" width="%d" height="%d" tilewidth="16" tileheight="16" infinite="1" nextlayerid="6" nextobjectid="1">' % [orientation, cells, cells])
	out.append(tileset_lines().trim_suffix("\n"))
	for l in 4:
		out.append(' <layer id="%d" name="%s" width="%d" height="%d">' % [l + 1, LAYER_NAMES[l], cells, cells])
		out.append('  <data encoding="base64" compression="zlib">')
		for cy in chunks:
			for cx in chunks:
				var v := variant_of(l, cx, cy)
				if v < 0:
					continue
				out.append('   <chunk x="%d" y="%d" width="16" height="16">%s</chunk>' % [cx * CHUNK, cy * CHUNK, encoded[l][v]])
		out.append('  </data>')
		out.append(' </layer>')
	out.append(' <objectgroup id="5" name="Things">')
	var obj_id := 1
	for cy in chunks:
		for cx in chunks:
			if (cx + cy) % 3 != 0:
				continue
			out.append(object_xml(obj_id, cx * CHUNK * TILE + rng.randi_range(0, 240), cy * CHUNK * TILE + rng.randi_range(16, 240)))
			obj_id += 1
	out.append(' </objectgroup>')
	out.append('</map>')
	return write_text(file, "\n".join(out) + "\n")


func object_xml(id: int, x: int, y: int) -> String:
	var props := prop("label", "string", "thing_%d" % id) + prop("score", "int", str(id % 1000))
	if id % 10 == 0 and id > 1:
		props += prop("target", "object", str(id - 1))
	var head := '  <object id="%d" name="T%d" x="%d" y="%d"' % [id, id, x, y]
	match id % 4:
		0:
			return head + ' class="area" width="32" height="24"><properties>%s</properties></object>' % props
		1:
			return head + '><properties>%s</properties><point/></object>' % props
		2:
			return head + ' gid="%d" width="16" height="16"><properties>%s</properties></object>' % [first_gids[0] + id % 8, props]
		_:
			return head + '><properties>%s</properties><polygon points="0,0 24,0 24,16 4,20"/></object>' % props


func gen_finite(file: String, seed_offset: int) -> bool:
	var out := PackedStringArray()
	out.append('<?xml version="1.0" encoding="UTF-8"?>')
	out.append('<map version="1.10" tiledversion="1.11.2" orientation="orthogonal" renderorder="right-down" width="%d" height="%d" tilewidth="16" tileheight="16" infinite="0" nextlayerid="6" nextobjectid="1">' % [world_size, world_size])
	out.append(tileset_lines().trim_suffix("\n"))
	var chunks := world_size / CHUNK
	for l in 4:
		var cells := PackedInt32Array()
		cells.resize(world_size * world_size)
		for cy in chunks:
			for cx in chunks:
				var v := variant_of(l, cx + seed_offset * 1000, cy)
				if v < 0:
					continue
				var src: PackedInt32Array = variants[l][v]
				for y in CHUNK:
					var row := (cy * CHUNK + y) * world_size + cx * CHUNK
					for x in CHUNK:
						cells[row + x] = src[y * CHUNK + x]
		out.append(' <layer id="%d" name="%s" width="%d" height="%d">' % [l + 1, LAYER_NAMES[l], world_size, world_size])
		out.append('  <data encoding="base64" compression="zlib">')
		out.append('   ' + encode(cells))
		out.append('  </data>')
		out.append(' </layer>')
	out.append(' <objectgroup id="5" name="Things">')
	var obj_id := 1
	for cy in chunks:
		for cx in chunks:
			if (cx + cy) % 3 != 0:
				continue
			out.append(object_xml(obj_id, cx * CHUNK * TILE + rng.randi_range(0, 240), cy * CHUNK * TILE + rng.randi_range(16, 240)))
			obj_id += 1
	out.append(' </objectgroup>')
	out.append('</map>')
	return write_text(file, "\n".join(out) + "\n")
