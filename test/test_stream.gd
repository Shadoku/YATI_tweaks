extends SceneTree
## Stage 4 of docs/native-importer-plan.md: TiledStreamer (addons/yati_native/), headless:
##
## - load_all() of a finite (and an infinite) map equals TiledBuilder.build's tree: every node's
##   path, class, child count, position and metadata, every layer's cells;
## - a window over test/fixtures/infinite.tmx loads exactly the chunks it covers (signals, cells)
##   and erases them when it moves away;
## - objects spawn and despawn with their chunks (signals), references to objects in other chunks
##   resolve when both exist and are empty paths otherwise; keep_objects_resident;
## - the budget stops work: a tiny budget takes several step() calls to settle, `settled` fires
##   once; the measured costs;
## - world mode over test/fixtures/world/test.world: maps load (asynchronously) when the window
##   reaches them and go when it leaves; load_all() loads every map;
## - Importer.make_streamer() for a map path (loaded asynchronously) and a world;
## - no leaks: streaming in and out 20 times leaves Performance.OBJECT_COUNT at its baseline.
##
##   godot --headless -s test/test_stream.gd
##
## Prints one line per check and a SUMMARY; exit code 1 when a check fails, 2 without the library.

const ImporterScript := preload("res://addons/yati_native/Importer.gd")
const FIX := "res://test/fixtures/"
const OUT := "res://test/out/stream_test/"
# Maps whose load_all() must equal TiledBuilder.build (and the project file each needs)
const EQUAL_MAPS := {
	"layers.tmx": "", "objects.tmx": "custom.tiled-project", "templates.tmx": "",
	"template_files.tmx": "", "object_refs.tmx": "", "tilesets.tmx": "", "encodings.tmx": "",
	"isometric.tmx": "", "staggered.tmx": "", "hexagonal.tmx": "", "json_map.tmj": "",
	"infinite.tmx": "", "infinite.tmj": "", "scenes.tmx": "scenes.tiled-project",
	"masks.tmx": "masks.tiled-project", "background.tmx": "", "parallax.tmx": "",
	"animations.tmx": "", "clip.tmx": "", "colors.tmx": "", "single_layer.tmx": "",
	"legacy_xml.tmx": "",
}

var _passed := 0
var _failed: PackedStringArray = []


func _initialize() -> void:
	if not ClassDB.class_exists("TiledStreamer"):
		printerr("test_stream: the native library is not loaded (bash tools/build_native.sh)")
		quit(2)
		return
	main()


func main() -> void:
	write_spread_map()
	await test_load_all_equals_build()
	await test_window_over_infinite()
	await test_objects_with_chunks()
	await test_keep_objects_resident()
	await test_budget()
	await test_y_sorted()
	await test_world()
	await test_make_streamer()
	await test_no_leaks()
	print("")
	print("SUMMARY: %d passed, %d failed" % [_passed, _failed.size()])
	for f in _failed:
		print("  FAILED: " + f)
	quit(0 if _failed.is_empty() else 1)


func check(cond: bool, what: String) -> bool:
	if cond:
		_passed += 1
		print("PASS " + what)
	else:
		_failed.append(what)
		print("FAIL " + what)
	return cond


func settle(frames: int = 5) -> void:
	for i in range(frames):
		await process_frame
		await physics_frame


func load_map(path: String, project: String = "") -> TiledMap:
	var m := TiledMap.new()
	m.load(path, project)
	return m


## A streamer for a map, processing off (driven with step()), with its own TileSet cache.
func streamer_for(map: TiledMap, options: TiledImportOptions = null) -> TiledStreamer:
	var s := TiledStreamer.new()
	s.tilesets = TiledTileSets.new()
	if options != null:
		s.options = options
	s.map = map
	root.add_child(s)
	# After entering the tree: at ready the engine turns processing on for a class with _process
	s.set_process(false)
	return s


## A test map of 64 x 64 cells (4 x 4 chunks), every cell set, with one object in each corner
## chunk; the object in chunk (3, 3) refers to the one in chunk (0, 0).
func write_spread_map() -> void:
	DirAccess.make_dir_recursive_absolute(ProjectSettings.globalize_path(OUT))
	var rows := PackedStringArray()
	for y in 64:
		var row := PackedStringArray()
		for x in 64:
			row.append(str(1 + (x + y) % 8))
		rows.append(",".join(row))
	var tsx := ProjectSettings.globalize_path(FIX + "terrain.tsx")
	var s := '<?xml version="1.0" encoding="UTF-8"?>\n'
	s += '<map version="1.10" tiledversion="1.11.2" orientation="orthogonal" renderorder="right-down" width="64" height="64" tilewidth="16" tileheight="16" infinite="0" nextlayerid="3" nextobjectid="5">\n'
	s += ' <tileset firstgid="1" source="%s"/>\n' % tsx
	s += ' <layer id="1" name="Ground" width="64" height="64">\n  <data encoding="csv">\n' + ",\n".join(rows) + '\n  </data>\n </layer>\n'
	s += ' <objectgroup id="2" name="Things" draworder="index">\n'
	s += '  <object id="1" name="A" x="8" y="8" width="16" height="16"/>\n'
	s += '  <object id="2" name="B" x="776" y="8" width="16" height="16"/>\n'
	s += '  <object id="3" name="C" x="8" y="776"><point/></object>\n'
	s += '  <object id="4" name="D" x="776" y="776" width="16" height="16"><properties><property name="target" type="object" value="1"/></properties></object>\n'
	s += ' </objectgroup>\n</map>\n'
	var f := FileAccess.open(OUT + "spread.tmx", FileAccess.WRITE)
	f.store_string(s)
	f.close()


## Every node of a tree: path, class, child count, position, metadata; every cell of a layer.
func describe(n: Node) -> PackedStringArray:
	var lines: PackedStringArray = []
	_describe(n, "", lines)
	return lines


func _describe(n: Node, prefix: String, lines: PackedStringArray) -> void:
	var p := prefix + "/" + String(n.name)
	var metas := PackedStringArray()
	for k in n.get_meta_list():
		metas.append("%s=%s" % [k, str(n.get_meta(k))])
	var pos := ""
	if n is Node2D:
		pos = str((n as Node2D).position.snapped(Vector2(0.01, 0.01)))
	elif n is Control:
		pos = str((n as Control).position.snapped(Vector2(0.01, 0.01)))
	var line := "%s|%s|%d|%s|%s" % [p, n.get_class(), n.get_child_count(), pos, ",".join(metas)]
	if n is TileMapLayer:
		var l := n as TileMapLayer
		var cells := PackedStringArray()
		for c in l.get_used_cells():
			cells.append("%s:%d:%s:%d" % [c, l.get_cell_source_id(c), l.get_cell_atlas_coords(c), l.get_cell_alternative_tile(c)])
		cells.sort()
		line += "|cells=" + ";".join(cells)
	lines.append(line)
	for c in n.get_children():
		_describe(c, p, lines)


# ---------------------------------------------------------------------------

func test_load_all_equals_build() -> void:
	for f: String in EQUAL_MAPS:
		var project: String = FIX + EQUAL_MAPS[f] if EQUAL_MAPS[f] != "" else ""
		var opts := TiledImportOptions.new()
		opts.always_wrap_root = true
		var built: Node = TiledBuilder.new().build(load_map(FIX + f, project), TiledTileSets.new(), opts)
		var s := streamer_for(load_map(FIX + f, project), opts)
		s.load_all()
		var root_node: Node2D = s.get_map_root("")
		var a := describe(built) if built != null else PackedStringArray()
		var b := describe(root_node) if root_node != null else PackedStringArray()
		var same := a == b and not a.is_empty()
		if not same:
			for i in maxi(a.size(), b.size()):
				var x := a[i] if i < a.size() else "<none>"
				var y := b[i] if i < b.size() else "<none>"
				if x != y:
					print("  build:    " + x.left(300))
					print("  streamer: " + y.left(300))
					break
		check(same, "%s: load_all() equals TiledBuilder.build (%d nodes)" % [f, a.size()])
		check(s.is_settled(), "%s: settled after load_all()" % f)
		if built != null:
			built.free()
		s.free()
	await settle(2)


func test_window_over_infinite() -> void:
	var s := streamer_for(load_map(FIX + "infinite.tmx"))
	var loaded: Array = []
	var unloaded: Array = []
	s.chunk_loaded.connect(func(n: String, c: Vector2i) -> void: loaded.append("%s%s" % [n, c]))
	s.chunk_unloaded.connect(func(n: String, c: Vector2i) -> void: unloaded.append("%s%s" % [n, c]))
	s.keep_margin = Vector2i.ZERO
	s.window = Rect2i(-16, 0, 16, 16)
	s.step(0)
	loaded.sort()
	check(loaded == ["ChunksCSV(-1, 0)", "ChunksZlib(-1, 0)"], "infinite.tmx: a window over chunk (-1, 0) loads it in both layers: %s" % [loaded])
	check(s.loaded_chunk_count() == 2, "infinite.tmx: 2 chunks loaded (%d)" % s.loaded_chunk_count())
	var cells: Array = s.get_layer("ChunksCSV").get_used_cells()
	cells.sort()
	check(cells == [Vector2i(-16, 0), Vector2i(-1, 15)], "infinite.tmx: the chunk's cells are set: %s" % [cells])
	check(s.get_loaded_chunks("ChunksZlib") == [Vector2i(-1, 0)], "infinite.tmx: get_loaded_chunks")
	loaded.clear()
	# A window over the other chunk (two cells of it): the first is erased, the second loaded
	s.window = Rect2i(0, 16, 2, 2)
	s.step(0)
	loaded.sort()
	unloaded.sort()
	check(loaded == ["ChunksCSV(0, 1)", "ChunksZlib(0, 1)"] and unloaded == ["ChunksCSV(-1, 0)", "ChunksZlib(-1, 0)"],
		"infinite.tmx: moving the window unloads the old chunk and loads the new one: %s / %s" % [loaded, unloaded])
	cells = s.get_layer("ChunksCSV").get_used_cells()
	cells.sort()
	check(cells == [Vector2i(0, 16), Vector2i(1, 17)], "infinite.tmx: the old cells are erased, the new ones set: %s" % [cells])
	var zcells: Array = s.get_layer("ChunksZlib").get_used_cells()
	zcells.sort()
	check(zcells == cells, "infinite.tmx: the zlib layer streams like the csv one")
	# Hysteresis: a window one cell away keeps the chunk within keep_margin
	s.keep_margin = Vector2i(16, 16)
	s.window = Rect2i(0, 32, 2, 2)
	unloaded.clear()
	s.step(0)
	check(unloaded.is_empty() and s.loaded_chunk_count() == 2, "infinite.tmx: a chunk within keep_margin of the window stays")
	s.window = Rect2i(200, 200, 2, 2)
	s.step(0)
	check(s.loaded_chunk_count() == 0 and s.get_layer("ChunksCSV").get_used_cells().is_empty(), "infinite.tmx: a far window leaves no chunk")
	check(s.is_settled(), "infinite.tmx: settled")
	s.free()


func test_objects_with_chunks() -> void:
	var opts := TiledImportOptions.new()
	opts.add_id_as_metadata = true
	var s := streamer_for(load_map(OUT + "spread.tmx"), opts)
	var spawned: Array = []
	var despawned: Array = []
	s.object_spawned.connect(func(_n: Node, id: int) -> void: spawned.append(id))
	s.object_despawned.connect(func(id: int) -> void: despawned.append(id))
	s.keep_margin = Vector2i.ZERO
	s.window = Rect2i(0, 0, 16, 16)
	s.step(0)
	var things: Node2D = s.get_object_layer("Things")
	check(spawned == [1] and s.get_spawned_object_count() == 1 and things.get_child_count() == 1,
		"spread.tmx: the object of chunk (0, 0) spawns with it: %s" % [spawned])
	spawned.clear()
	s.window = Rect2i(48, 48, 16, 16)
	s.step(0)
	check(despawned == [1] and spawned == [4], "spread.tmx: moving to chunk (3, 3) despawns A and spawns D: %s / %s" % [despawned, spawned])
	var d: Node = things.get_node_or_null("D (SB)")
	check(d != null and str(d.get_meta("target", "x")) == "", "spread.tmx: D's reference to A (not spawned) is an empty path")
	check(s.get_warning_count() == 0, "spread.tmx: no warning for a reference to an object not spawned")
	# Both in the window: the reference resolves
	s.window = Rect2i(0, 0, 64, 64)
	s.step(0)
	var a: Node = things.get_node_or_null("A (SB)")
	var path: NodePath = d.get_meta("target", NodePath()) if is_instance_valid(d) else NodePath()
	check(a != null and not path.is_empty() and d.get_node_or_null(path) == a, "spread.tmx: with A spawned, D's reference resolves to it (%s)" % path)
	check(s.get_spawned_object_count() == 4 and things.get_child_count() == 4, "spread.tmx: all four objects spawned")
	var order := PackedStringArray()
	for c in things.get_children():
		order.append(String(c.name))
	check(order == PackedStringArray(["A (SB)", "B (SB)", "C", "D (SB)"]), "spread.tmx: draworder index keeps the layer's order: %s" % [order])
	# A leaves again: D's reference becomes an empty path again
	s.window = Rect2i(48, 48, 16, 16)
	s.step(0)
	check(is_instance_valid(d) and str(d.get_meta("target", "x")) == "", "spread.tmx: when A despawns, D's reference is empty again")
	s.free()


func test_keep_objects_resident() -> void:
	var s := streamer_for(load_map(OUT + "spread.tmx"))
	s.keep_objects_resident = true
	s.keep_margin = Vector2i.ZERO
	s.window = Rect2i(0, 0, 16, 16)
	s.step(0)
	s.window = Rect2i(48, 48, 16, 16)
	s.step(0)
	check(s.get_spawned_object_count() == 2 and s.loaded_chunk_count() == 1,
		"keep_objects_resident: A stays when its chunk unloads (%d objects, %d chunks)" % [s.get_spawned_object_count(), s.loaded_chunk_count()])
	s.window = Rect2i(0, 0, 16, 16)
	s.step(0)
	check(s.get_spawned_object_count() == 2 and s.get_object_layer("Things").get_child_count() == 2,
		"keep_objects_resident: A is not spawned twice")
	s.free()


func test_budget() -> void:
	var s := streamer_for(load_map(OUT + "spread.tmx"))
	var settled := [0]
	s.settled.connect(func() -> void: settled[0] += 1)
	s.window = Rect2i(0, 0, 64, 64)
	# A frame between steps: the finite map's layers are decoded on a thread meanwhile
	var steps := 0
	while not s.step(1) and steps < 1000:
		steps += 1
		await process_frame
	steps += 1
	check(steps > 4, "a 1 µs budget does one piece of work per step: %d steps for 16 chunks and 4 objects" % steps)
	check(s.loaded_chunk_count() == 16 and s.get_spawned_object_count() == 4, "the budgeted window is complete when settled")
	s.step(1)
	s.step(1)
	check(settled[0] == 1, "settled fired once (%d)" % settled[0])
	var stats: Dictionary = s.get_stats()
	# The TileSet's tiles are created first (a gid per step at this budget), then a piece of work per step
	check(stats["budget_stops"] >= 16, "the stops are counted (%d)" % stats["budget_stops"])
	var costs: Dictionary = s.get_costs()
	print("  measured costs: chunk %.1f µs + %.2f µs per cell, %.1f µs per object (%d tile samples)" % [costs["chunk"], costs["cell"], costs["object"], costs["tile_samples"]])
	check(costs["tile_samples"] == 16 and costs["object_samples"] == 4 and costs["cell"] > 0.0, "the costs are measured on the chunks")
	# Unbounded: all at once, settled again after the window moves
	s.window = Rect2i(200, 200, 4, 4)
	check(s.step(0) and settled[0] == 2, "an unbounded step settles a moved window at once")
	s.free()


## Y-sorted tile layers (isometric and staggered maps) are refused unless allowed; load_all()
## takes them (it is the one-shot build).
func test_y_sorted() -> void:
	var s := streamer_for(load_map(FIX + "isometric.tmx"))
	s.window = Rect2i(0, 0, 16, 16)
	s.step(0)
	check(s.get_error_count() == 1 and s.loaded_chunk_count() == 0 and s.get_map_root("") == null,
		"isometric.tmx: refused with an error by default (%d errors)" % s.get_error_count())
	s.free()
	s = streamer_for(load_map(FIX + "staggered.tmx"))
	s.allow_y_sorted_layers = true
	s.window = Rect2i(0, 0, 16, 16)
	s.step(0)
	check(s.get_error_count() == 0 and s.loaded_chunk_count() > 0, "staggered.tmx: streamed with allow_y_sorted_layers (%d chunks)" % s.loaded_chunk_count())
	var any_sorted := false
	for l in s.get_map_root("").find_children("*", "TileMapLayer", true, false):
		any_sorted = any_sorted or (l as TileMapLayer).y_sort_enabled
	check(any_sorted, "staggered.tmx: its layers are y-sorted, as the builder makes them")
	var costs: Dictionary = s.get_costs()
	check(costs["tile_samples"] > 0, "staggered.tmx: y-sorted chunks are measured (%.2f µs per y-sorted cell)" % costs["ysort_cell"])
	s.free()


func test_world() -> void:
	var w := TiledWorld.new()
	w.load(FIX + "world/test.world", "")
	var s := TiledStreamer.new()
	s.tilesets = TiledTileSets.new()
	s.world = w
	s.world_margin = Vector2i.ZERO
	s.keep_margin = Vector2i.ZERO
	root.add_child(s)
	s.set_process(false)
	var events: Array = []
	s.map_loaded.connect(func(p: String, _r: Node2D) -> void: events.append("+" + p.get_file()))
	s.map_unloaded.connect(func(p: String) -> void: events.append("-" + p.get_file()))
	# The hub (x -160..0 px, y 0..160 px) only
	s.window = Rect2i(-10, 1, 4, 4)
	await step_until_settled(s)
	check(Array(s.get_loaded_maps()) == [FIX + "world/hub.tmx"] and events == ["+hub.tmx"], "world: a window over the hub loads it only: %s" % [events])
	var hub: Node2D = s.get_map_root(FIX + "world/hub.tmx")
	check(hub != null and hub.position == Vector2(-160, 0), "world: the hub's root is at its world position")
	check(s.loaded_chunk_count() == 1 and s.get_layer("Ground").get_used_cells().size() > 0, "world: the hub's layer is streamed")
	events.clear()
	# area_0_0 (x 16..176, y -32..128)
	s.window = Rect2i(2, 0, 4, 4)
	await step_until_settled(s)
	check(events == ["+area_0_0.tmx", "-hub.tmx"] or events == ["-hub.tmx", "+area_0_0.tmx"], "world: moving the window loads area_0_0 and unloads the hub: %s" % [events])
	check(Array(s.get_loaded_maps()) == [FIX + "world/area_0_0.tmx"], "world: only area_0_0 is loaded")
	check(s.get_map_root(FIX + "world/hub.tmx") == null, "world: the hub's tree is gone")
	s.load_all()
	check(s.get_loaded_maps().size() == 4 and s.is_settled(), "world: load_all() loads every map (%d)" % s.get_loaded_maps().size())
	s.free()


func step_until_settled(s: TiledStreamer) -> void:
	for i in 300:
		if s.step(0):
			return
		await process_frame


func test_make_streamer() -> void:
	var s: Node2D = ImporterScript.new().make_streamer(FIX + "infinite.tmx")
	check(s != null, "make_streamer(path) gives a streamer")
	if s == null:
		return
	s.set("window", Rect2i(-32, -32, 64, 64))
	root.add_child(s)
	for i in 120:
		await process_frame
		if s.call("is_settled"):
			break
	check(s.call("is_settled") and s.call("loaded_chunk_count") == 4, "make_streamer(path): the map loads asynchronously and streams (%d chunks)" % s.call("loaded_chunk_count"))
	check(s.get("tilesets") == ImporterScript.shared_tilesets(), "make_streamer: the shared TileSet cache")
	s.free()
	var w: Node2D = ImporterScript.new().make_streamer(FIX + "world/test.world")
	root.add_child(w)
	w.call("load_all")
	check(w.call("get_loaded_maps").size() == 4, "make_streamer(world path): a world streamer")
	w.free()


func test_no_leaks() -> void:
	var maps: Array[TiledMap] = [load_map(FIX + "infinite.tmx"), load_map(FIX + "objects.tmx", FIX + "custom.tiled-project"), load_map(OUT + "spread.tmx")]
	var world := TiledWorld.new()
	world.load(FIX + "world/test.world", "")
	var round := func() -> void:
		var tilesets := TiledTileSets.new()
		for m in maps:
			var s := streamer_for(m)
			s.tilesets = tilesets
			s.window = Rect2i(-64, -64, 128, 128)
			s.step(0)
			s.window = Rect2i(1000, 1000, 1, 1)
			s.step(0)
			s.free()
		var ws := TiledStreamer.new()
		ws.tilesets = tilesets
		ws.world = world
		root.add_child(ws)
		ws.set_process(false)
		ws.load_all()
		ws.window = Rect2i(1000, 1000, 1, 1)
		ws.step(0)
		ws.free()
	# Warm-up, then 22 rounds: the counts after the second are the baseline (a few objects are
	# created once, on the third round: measured stable over 60 rounds after that)
	round.call()
	round.call()
	var before := 0
	var nodes := 0
	var orphans := 0
	for i in 22:
		round.call()
		await settle(2)
		if i == 1:
			before = int(Performance.get_monitor(Performance.OBJECT_COUNT))
			nodes = int(Performance.get_monitor(Performance.OBJECT_NODE_COUNT))
			orphans = int(Performance.get_monitor(Performance.OBJECT_ORPHAN_NODE_COUNT))
		if OS.get_cmdline_user_args().has("--verbose"):
			print("  round %d: objects %d, resources %d" % [i, Performance.get_monitor(Performance.OBJECT_COUNT), Performance.get_monitor(Performance.OBJECT_RESOURCE_COUNT)])
	var after := int(Performance.get_monitor(Performance.OBJECT_COUNT))
	var nodes_after := int(Performance.get_monitor(Performance.OBJECT_NODE_COUNT))
	var orphans_after := int(Performance.get_monitor(Performance.OBJECT_ORPHAN_NODE_COUNT))
	check(after == before and nodes_after == nodes and orphans_after == orphans,
		"no leaks after streaming in and out 20 times: objects %d -> %d, nodes %d -> %d, orphan nodes %d -> %d" % [before, after, nodes, nodes_after, orphans, orphans_after])
