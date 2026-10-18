extends SceneTree
## Headless test suite for the YATI runtime importer.
##
##   godot --headless -s test/run_tests.gd
##   godot --headless -s test/run_tests.gd -- --runtime=res://baseline_runtime --filter=object
##
## Every method named test_* is a test. A test returns done() at its end; a test that hits
## a script error returns null and is reported as aborted.
## Naming:
##   test_new_*  asserts behaviour added by the 3.0 runtime work (PLAN.md T1-T8, F2-F4, B7, ...).
##               Expected to fail against the pre-change runtime.
##   test_bug_*  asserts documented/intended behaviour that the pre-change runtime gets wrong
##               and that no workstream is (yet) assigned to fix.
##   others      must pass on both the old and the new runtime.

const FIX := "res://test/fixtures/"
const OUT := "res://test/out/"

const GOOD_FIXTURES: Array[String] = [
	"tilesets.tmx", "layers.tmx", "objects.tmx", "encodings.tmx", "infinite.tmx",
	"isometric.tmx", "staggered.tmx", "hexagonal.tmx", "background.tmx",
	"legacy_xml.tmx", "json_map.tmj",
]

const FLIP_H := TileSetAtlasSource.TRANSFORM_FLIP_H
const FLIP_V := TileSetAtlasSource.TRANSFORM_FLIP_V
const FLIP_T := TileSetAtlasSource.TRANSFORM_TRANSPOSE

var runtime_dir := "res://runtime"
var _filter := ""
var _importer_script: GDScript
var _fails: PackedStringArray = []
var _to_free: Array[Node] = []


func _init() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--runtime="):
			runtime_dir = arg.substr(10).trim_suffix("/")
		elif arg.begins_with("--filter="):
			_filter = arg.substr(9)
	DirAccess.make_dir_recursive_absolute(ProjectSettings.globalize_path(OUT))
	_importer_script = load(runtime_dir.path_join("Importer.gd"))
	if _importer_script == null:
		printerr("Cannot load " + runtime_dir.path_join("Importer.gd"))
		quit(2)
		return
	print("YATI runtime tests, runtime = " + runtime_dir)

	var passed: PackedStringArray = []
	var failed: PackedStringArray = []
	for m in get_script().get_script_method_list():
		var method: String = m["name"]
		if not method.begins_with("test_"): continue
		var test_name := method.substr(5)
		if _filter != "" and not test_name.contains(_filter): continue
		_fails = []
		var ret: Variant = call(method)
		if ret == null:
			_fails.insert(0, "test aborted (script error, see log above)")
		_cleanup()
		if _fails.is_empty():
			passed.append(test_name)
			print("PASS " + test_name)
		else:
			failed.append(test_name)
			var reason := _fails[0]
			if _fails.size() > 1:
				reason += "  (+%d more: %s)" % [_fails.size() - 1, " | ".join(_fails.slice(1, 4))]
			print("FAIL " + test_name + ": " + reason)

	var new_failed := 0
	var bug_failed := 0
	for t in failed:
		if t.begins_with("new_"): new_failed += 1
		elif t.begins_with("bug_"): bug_failed += 1
	var other_failed := failed.size() - new_failed - bug_failed
	print("")
	print("SUMMARY: %d passed, %d failed (new_: %d, bug_: %d, other: %d) of %d" % [
		passed.size(), failed.size(), new_failed, bug_failed, other_failed, passed.size() + failed.size()])
	quit(0 if failed.is_empty() else 1)


# ---------------------------------------------------------------------------
# Infrastructure

func done() -> bool:
	return true


func _new_importer(opts: Dictionary = {}) -> Object:
	var imp: Object = _importer_script.new()
	for k: String in opts:
		imp.set(k, opts[k]) # silently ignored by runtimes without option vars
	return imp


## Imports a fixture; the resulting tree is freed after the test.
func _import(file: String, opts: Dictionary = {}, project: String = "") -> Node:
	var imp := _new_importer(opts)
	return _import_with(imp, file, project)


func _import_with(imp: Object, file: String, project: String = "") -> Node:
	var path := file if file.begins_with("res://") else FIX + file
	var proj := "" if project == "" else (project if project.begins_with("res://") else FIX + project)
	var node: Node = imp.call("import", path, proj)
	if node == null:
		fail("import of '" + file + "' returned null")
	else:
		_to_free.append(node)
	return node


func _require_option(imp: Object, opt: String) -> bool:
	if not (opt in imp):
		fail("Importer has no option var '" + opt + "'")
		return false
	return true


func _cleanup() -> void:
	for n in _to_free:
		_free_tree(n)
	_to_free.clear()
	_reset_zip_mode()


## Frees the whole tree a node belongs to (works for parented results of the old runtime too).
func _free_tree(n: Node) -> void:
	if not is_instance_valid(n): return
	var top := n
	while top.get_parent() != null and top.get_parent() != root:
		top = top.get_parent()
	if top.get_parent() == root:
		root.remove_child(top)
	top.free()


## The pre-change DataLoader keeps zip mode after import_from_zip (PLAN F2); undo it so that one
## zip test does not break every following test.
func _reset_zip_mode() -> void:
	var dl: Variant = _runtime_script("DataLoader.gd")
	if dl != null:
		dl.set("zip_file", "")


func _error_count() -> int:
	var cu: Variant = _runtime_script("CommonUtils.gd")
	return int(cu.get("error_count")) if cu != null else 0


## One of the runtime's scripts, or null when the runtime has none of that name (the native
## facade, addons/yati_native/, has only Importer.gd): checked first, so no load error is printed.
func _runtime_script(file: String) -> Variant:
	var path := runtime_dir.path_join(file)
	return load(path) if ResourceLoader.exists(path) else null


func fail(msg: String) -> void:
	_fails.append(msg)


func check(cond: bool, msg: String) -> bool:
	if not cond:
		fail(msg)
	return cond


static func _approx(a: Variant, b: Variant) -> bool:
	if (typeof(a) == TYPE_FLOAT or typeof(a) == TYPE_INT) and (typeof(b) == TYPE_FLOAT or typeof(b) == TYPE_INT):
		return is_equal_approx(float(a), float(b))
	if typeof(a) != typeof(b):
		return false
	match typeof(a):
		TYPE_VECTOR2, TYPE_COLOR, TYPE_RECT2:
			return a.is_equal_approx(b)
		TYPE_PACKED_VECTOR2_ARRAY, TYPE_ARRAY:
			if a.size() != b.size(): return false
			for i in range(a.size()):
				if not _approx(a[i], b[i]): return false
			return true
	return a == b


func eq(actual: Variant, expected: Variant, msg: String) -> bool:
	if _approx(actual, expected):
		return true
	fail("%s: expected %s (%s), got %s (%s)" % [msg, str(expected), type_string(typeof(expected)), str(actual), type_string(typeof(actual))])
	return false


func is_type(n: Variant, cls: String, msg: String) -> bool:
	if n == null or not (n is Object) or not is_instance_valid(n):
		fail(msg + ": node missing (expected " + cls + ")")
		return false
	if not (n as Object).is_class(cls):
		fail("%s: expected %s, got %s" % [msg, cls, (n as Object).get_class()])
		return false
	return true


## Recursive search by exact name (the start node itself included).
func find(start: Node, node_name: String) -> Node:
	if start == null: return null
	if String(start.name) == node_name: return start
	return start.find_child(node_name, true, false)


func need(start: Node, node_name: String, cls: String = "") -> Node:
	var n := find(start, node_name)
	if n == null:
		fail("node '" + node_name + "' not found")
		return null
	if cls != "" and not is_type(n, cls, node_name):
		return null
	return n


func meta(n: Node, key: String, expected: Variant) -> bool:
	if n == null:
		fail("meta '" + key + "': node missing")
		return false
	if not n.has_meta(key):
		fail("%s: meta '%s' missing (has %s)" % [n.name, key, str(n.get_meta_list())])
		return false
	return eq(n.get_meta(key), expected, "%s meta '%s'" % [n.name, key])


func cell(layer: TileMapLayer, coords: Vector2i, source: int, atlas: Vector2i, alt: int = 0) -> bool:
	if layer == null:
		fail("cell %s: layer missing" % str(coords))
		return false
	var ok := eq(layer.get_cell_source_id(coords), source, "%s cell %s source" % [layer.name, str(coords)])
	ok = eq(layer.get_cell_atlas_coords(coords), atlas, "%s cell %s atlas" % [layer.name, str(coords)]) and ok
	ok = eq(layer.get_cell_alternative_tile(coords), alt, "%s cell %s alternative" % [layer.name, str(coords)]) and ok
	return ok


func children_of_type(n: Node, cls: String) -> Array[Node]:
	var ret: Array[Node] = []
	if n == null: return ret
	for c in n.get_children():
		if c.is_class(cls):
			ret.append(c)
	return ret


func layer(start: Node, node_name: String) -> TileMapLayer:
	return need(start, node_name, "TileMapLayer") as TileMapLayer


func tileset_of(start: Node, layer_name: String) -> TileSet:
	var l := layer(start, layer_name)
	if l == null: return null
	check(l.tile_set != null, layer_name + " has no tile_set")
	return l.tile_set


func atlas(ts: TileSet, source_id: int) -> TileSetAtlasSource:
	if ts == null or not ts.has_source(source_id):
		fail("tileset has no source " + str(source_id))
		return null
	return ts.get_source(source_id) as TileSetAtlasSource


func tile_data(ts: TileSet, source_id: int, coords: Vector2i, alt: int = 0) -> TileData:
	var src := atlas(ts, source_id)
	if src == null: return null
	if not src.has_tile(coords):
		fail("source %d has no tile at %s" % [source_id, str(coords)])
		return null
	if not src.has_alternative_tile(coords, alt):
		fail("source %d tile %s has no alternative %d" % [source_id, str(coords), alt])
		return null
	return src.get_tile_data(coords, alt)


# ---------------------------------------------------------------------------
# General / lifecycle

func test_smoke_all_fixtures_import() -> bool:
	for f in GOOD_FIXTURES:
		_import(f)
	return done()


func test_good_fixtures_no_errors() -> bool:
	# Works with and without a per-import counter reset (see _error_count usage).
	for f in GOOD_FIXTURES:
		var before := _error_count()
		_import(f, {}, "custom.tiled-project" if f == "objects.tmx" else "")
		var after := _error_count()
		var errors := after - before if after >= before else after
		check(errors == 0, "%s produced %d error(s)" % [f, errors])
	return done()


func test_new_good_fixtures_last_error_count_zero() -> bool:
	for f in GOOD_FIXTURES:
		var imp := _new_importer()
		if not _require_option(imp, "last_error_count"): return done()
		_import_with(imp, f, "custom.tiled-project" if f == "objects.tmx" else "")
		eq(imp.get("last_error_count"), 0, f + " last_error_count")
	return done()


func test_missing_tileset_still_returns_node() -> bool:
	var before := _error_count()
	var n := _import("missing_tileset.tmx")
	check(n != null, "no node returned")
	var after := _error_count()
	var errors := after - before if after > before else after
	check(errors > 0, "missing tileset did not raise the error count")
	return done()


func test_new_missing_tileset_last_error_count() -> bool:
	var imp := _new_importer()
	if not _require_option(imp, "last_error_count"): return done()
	var n := _import_with(imp, "missing_tileset.tmx")
	check(n != null, "no node returned")
	check(int(imp.get("last_error_count")) > 0, "last_error_count should be > 0, is " + str(imp.get("last_error_count")))
	# a following good import must start from zero again
	_import_with(imp, "background.tmx")
	eq(imp.get("last_error_count"), 0, "last_error_count after a good import")
	return done()


func test_multi_layer_root_is_parentless() -> bool:
	var n := _import("layers.tmx")
	if n == null: return done()
	is_type(n, "Node2D", "root")
	eq(String(n.name), "layers", "root name")
	check(n.get_parent() == null, "root has a parent: " + str(n.get_parent()))
	if n.get_parent() == null:
		root.add_child(n)
		check(n.get_parent() == root, "root could not be added to the SceneTree root")
	return done()


func test_new_single_layer_root_is_parentless() -> bool:
	var n := _import("isometric.tmx")
	if n == null: return done()
	is_type(n, "TileMapLayer", "root")
	eq(String(n.name), "isometric", "root name")
	if check(n.get_parent() == null, "single-layer root still has parent '%s'" % (str(n.get_parent().name) if n.get_parent() else "")):
		root.add_child(n)
		check(n.get_parent() == root, "root could not be added to the SceneTree root")
	return done()


func test_new_no_leaked_wrapper_nodes() -> bool:
	# The returned node must be the only thing left over: freeing it must not leave orphans.
	var before := int(Performance.get_monitor(Performance.OBJECT_ORPHAN_NODE_COUNT))
	var imp := _new_importer()
	var n: Node = imp.call("import", FIX + "isometric.tmx", "")
	if not check(n != null, "import returned null"): return done()
	n.free()
	var after := int(Performance.get_monitor(Performance.OBJECT_ORPHAN_NODE_COUNT))
	eq(after - before, 0, "orphan nodes left after freeing the returned node")
	return done()


func test_bug_no_orphan_nodes_after_freeing_tree() -> bool:
	# Freeing the imported tree must release every node the importer created. The pre-change
	# TilemapCreator builds a Dictionary of five *.new() bodies per collision tile object and keeps
	# only one, and queue_free()s the unused ParallaxBackground.
	for f in ["objects.tmx", "layers.tmx"]:
		var before := int(Performance.get_monitor(Performance.OBJECT_ORPHAN_NODE_COUNT))
		_import(f, {}, "custom.tiled-project" if f == "objects.tmx" else "")
		_cleanup()
		var after := int(Performance.get_monitor(Performance.OBJECT_ORPHAN_NODE_COUNT))
		eq(after - before, 0, f + ": orphan nodes left after freeing the imported tree")
	return done()


func test_import_twice_independent() -> bool:
	var imp := _new_importer()
	var a := _import_with(imp, "tilesets.tmx")
	var b := _import_with(imp, "tilesets.tmx")
	if a == null or b == null: return done()
	check(a != b, "same node returned twice")
	var la := layer(a, "Main")
	var lb := layer(b, "Main")
	if la == null or lb == null: return done()
	check(la != lb, "layers are shared")
	check(la.tile_set != lb.tile_set, "TileSet is shared between imports")
	eq(la.get_used_cells().size(), lb.get_used_cells().size(), "used cells")
	la.erase_cell(Vector2i(0, 0))
	eq(lb.get_cell_source_id(Vector2i(0, 0)), 0, "erasing in tree A changed tree B")
	return done()


func test_post_processing() -> bool:
	var imp := _new_importer()
	var n: Node = imp.call("import_with_post_processing", FIX + "layers.tmx", FIX + "postproc.gd", "")
	if not check(n != null, "import_with_post_processing returned null"): return done()
	_to_free.append(n)
	meta(n, "post_processed", true)
	return done()


func test_default_texture_filter_nearest() -> bool:
	var n := _import("layers.tmx")
	var l := layer(n, "Base")
	if l: eq(l.texture_filter, CanvasItem.TEXTURE_FILTER_NEAREST, "Base texture_filter")
	var o := need(n, "TopDown", "Node2D")
	if o: eq((o as CanvasItem).texture_filter, CanvasItem.TEXTURE_FILTER_NEAREST, "TopDown texture_filter")
	return done()


# ---------------------------------------------------------------------------
# Encodings, infinite maps, orientations

func test_encodings_all_layers_equal() -> bool:
	var n := _import("encodings.tmx")
	if n == null: return done()
	var ref := layer(n, "CSV")
	if ref == null: return done()
	eq(ref.get_used_cells().size(), 14, "CSV used cells")
	cell(ref, Vector2i(0, 0), 0, Vector2i(0, 0))
	cell(ref, Vector2i(1, 1), 0, Vector2i(1, 1))
	cell(ref, Vector2i(2, 2), 0, Vector2i(0, 2))
	eq(ref.get_cell_atlas_coords(Vector2i(3, 3)), Vector2i(1, 0), "flipped cell atlas")
	check(ref.get_cell_alternative_tile(Vector2i(3, 3)) != 0, "flipped cell has alternative 0")
	for lname in ["B64", "B64zlib", "B64gzip", "B64zstd"]:
		var l := layer(n, lname)
		if l == null: continue
		eq(l.get_used_cells().size(), 14, lname + " used cells")
		for c in ref.get_used_cells():
			if l.get_cell_source_id(c) != ref.get_cell_source_id(c) \
					or l.get_cell_atlas_coords(c) != ref.get_cell_atlas_coords(c) \
					or l.get_cell_alternative_tile(c) != ref.get_cell_alternative_tile(c):
				fail("%s differs from CSV at %s" % [lname, str(c)])
				break
	return done()


func test_infinite_chunks() -> bool:
	var n := _import("infinite.tmx")
	if n == null: return done()
	for lname in ["ChunksCSV", "ChunksZlib"]:
		var l := layer(n, lname)
		if l == null: continue
		eq(l.get_used_cells().size(), 4, lname + " used cells")
		cell(l, Vector2i(-16, 0), 0, Vector2i(0, 0))
		cell(l, Vector2i(-1, 15), 0, Vector2i(1, 0))
		cell(l, Vector2i(0, 16), 0, Vector2i(2, 0))
		cell(l, Vector2i(1, 17), 0, Vector2i(3, 0))
	return done()


func test_isometric_map() -> bool:
	var n := _import("isometric.tmx")
	if n == null or not is_type(n, "TileMapLayer", "root"): return done()
	var l := n as TileMapLayer
	eq(String(l.name), "isometric", "single layer root takes the map name")
	eq(l.tile_set.tile_shape, TileSet.TILE_SHAPE_ISOMETRIC, "tile_shape")
	eq(l.tile_set.tile_layout, TileSet.TILE_LAYOUT_DIAMOND_DOWN, "tile_layout")
	eq(l.tile_set.tile_size, Vector2i(32, 16), "tile_size")
	check(l.y_sort_enabled, "isometric layer should be y-sorted")
	eq(l.position, Vector2(32, 0), "isometric layer offset")
	eq(l.get_used_cells().size(), 9, "used cells")
	cell(l, Vector2i(1, 0), 0, Vector2i(1, 0))
	return done()


func test_staggered_map() -> bool:
	var n := _import("staggered.tmx")
	if n == null or not is_type(n, "TileMapLayer", "root"): return done()
	var l := n as TileMapLayer
	eq(l.tile_set.tile_shape, TileSet.TILE_SHAPE_ISOMETRIC, "tile_shape")
	eq(l.tile_set.tile_layout, TileSet.TILE_LAYOUT_STACKED, "tile_layout")
	eq(l.tile_set.tile_offset_axis, TileSet.TILE_OFFSET_AXIS_HORIZONTAL, "offset axis")
	eq(l.get_used_cells().size(), 15, "used cells")
	return done()


func test_hexagonal_map() -> bool:
	var n := _import("hexagonal.tmx")
	if n == null or not is_type(n, "TileMapLayer", "root"): return done()
	var l := n as TileMapLayer
	eq(l.tile_set.tile_shape, TileSet.TILE_SHAPE_HEXAGON, "tile_shape")
	eq(l.tile_set.tile_layout, TileSet.TILE_LAYOUT_STACKED_OFFSET, "tile_layout")
	eq(l.tile_set.tile_offset_axis, TileSet.TILE_OFFSET_AXIS_VERTICAL, "offset axis")
	eq(l.get_used_cells().size(), 8, "used cells")
	return done()


# ---------------------------------------------------------------------------
# Tilesets (tilesets.tmx)

func test_tileset_sources() -> bool:
	var n := _import("tilesets.tmx")
	var ts := tileset_of(n, "Main")
	if ts == null: return done()
	eq(ts.get_source_count(), 5, "source count (1 atlas + 1 embedded + 3 image collection)")
	var s0 := atlas(ts, 0)
	if s0:
		eq(s0.texture_region_size, Vector2i(16, 16), "source 0 region size")
		check(s0.texture != null, "source 0 texture missing")
		if s0.texture: eq(s0.texture.get_size(), Vector2(128, 64), "source 0 texture size")
		eq(s0.get_atlas_grid_size(), Vector2i(8, 4), "source 0 grid")
	var s1 := atlas(ts, 1)
	if s1:
		eq(s1.margins, Vector2i(2, 2), "spaced margins")
		eq(s1.separation, Vector2i(1, 1), "spaced separation")
		eq(s1.texture_region_size, Vector2i(16, 16), "spaced region size")
		eq(s1.get_atlas_grid_size(), Vector2i(4, 2), "spaced grid")
	var sizes := [Vector2i(16, 16), Vector2i(32, 48), Vector2i(16, 16)]
	for i in range(3):
		var s := atlas(ts, 2 + i)
		if s: eq(s.texture_region_size, sizes[i], "collection source %d region size" % (2 + i))
	return done()


func test_tileset_main_layer_cells() -> bool:
	var n := _import("tilesets.tmx")
	var l := layer(n, "Main")
	if l == null: return done()
	eq(l.get_used_cells().size(), 15, "used cells")
	cell(l, Vector2i(0, 0), 0, Vector2i(0, 0))
	cell(l, Vector2i(1, 0), 0, Vector2i(4, 0)) # animated
	cell(l, Vector2i(2, 0), 0, Vector2i(0, 1)) # collision tile
	cell(l, Vector2i(3, 0), 0, Vector2i(0, 2)) # wang tile
	cell(l, Vector2i(4, 0), 1, Vector2i(0, 0))
	cell(l, Vector2i(5, 0), 1, Vector2i(1, 0))
	cell(l, Vector2i(6, 0), 2, Vector2i(0, 0))
	cell(l, Vector2i(7, 0), 3, Vector2i(0, 0))
	cell(l, Vector2i(0, 1), 0, Vector2i(3, 0))
	cell(l, Vector2i(4, 1), 1, Vector2i(2, 0))
	cell(l, Vector2i(5, 1), 1, Vector2i(3, 0))
	cell(l, Vector2i(6, 1), 4, Vector2i(0, 0))
	return done()


func test_tile_custom_data_and_meta() -> bool:
	var n := _import("tilesets.tmx")
	var ts := tileset_of(n, "Main")
	if ts == null: return done()
	eq(ts.get_custom_data_layers_count(), 2, "custom data layers (internal layer removed)")
	var speed := ts.get_custom_data_layer_by_name("speed")
	var slippery := ts.get_custom_data_layer_by_name("slippery")
	check(speed >= 0, "custom data layer 'speed' missing")
	check(slippery >= 0, "custom data layer 'slippery' missing")
	check(ts.get_custom_data_layer_by_name("data_speed") < 0, "prefix not stripped from custom data layer")
	check(ts.get_custom_data_layer_by_name("kind") < 0, "un-prefixed property became custom data")
	if speed >= 0: eq(ts.get_custom_data_layer_type(speed), TYPE_INT, "speed layer type")
	if slippery >= 0: eq(ts.get_custom_data_layer_type(slippery), TYPE_BOOL, "slippery layer type")
	var td := tile_data(ts, 0, Vector2i(0, 0))
	if td:
		if speed >= 0: eq(td.get_custom_data("speed"), 3, "tile 0 speed")
		if slippery >= 0: eq(td.get_custom_data("slippery"), true, "tile 0 slippery")
		check(td.has_meta("kind"), "tile 0 meta 'kind' missing")
		if td.has_meta("kind"): eq(td.get_meta("kind"), "grass", "tile 0 meta kind")
	return done()


func test_bug_prefixed_custom_data_not_duplicated_as_meta() -> bool:
	# Reference.md: properties WITH the prefix go to custom data, those WITHOUT go to metadata.
	# The pre-change TilesetCreator strips the prefix first and then also adds the stripped name
	# as metadata (and tile objects then inherit it as a property).
	var n := _import("tilesets.tmx")
	var td := tile_data(tileset_of(n, "Main"), 0, Vector2i(0, 0))
	if td:
		check(not td.has_meta("speed"), "custom data 'speed' also stored as tile metadata")
		check(not td.has_meta("slippery"), "custom data 'slippery' also stored as tile metadata")
	return done()


func test_tileset_metadata() -> bool:
	var ts := tileset_of(_import("tilesets.tmx"), "Main")
	if ts == null: return done()
	check(ts.has_meta("author"), "tileset meta 'author' missing")
	if ts.has_meta("author"): eq(ts.get_meta("author"), "yati-tests", "tileset meta author")
	return done()


func test_tile_probability() -> bool:
	var td := tile_data(tileset_of(_import("tilesets.tmx"), "Main"), 0, Vector2i(3, 0))
	if td: eq(td.probability, 0.25, "probability")
	return done()


func test_tile_animation() -> bool:
	var src := atlas(tileset_of(_import("tilesets.tmx"), "Main"), 0)
	if src == null: return done()
	var c := Vector2i(4, 0)
	eq(src.get_tile_animation_frames_count(c), 3, "frame count")
	eq(src.get_tile_animation_columns(c), 0, "animation columns")
	eq(src.get_tile_animation_separation(c), Vector2i.ZERO, "animation separation")
	for i in range(src.get_tile_animation_frames_count(c)):
		eq(src.get_tile_animation_frame_duration(c, i), 0.1, "frame %d duration" % i)
	return done()


func test_tile_collision_polygons() -> bool:
	var ts := tileset_of(_import("tilesets.tmx"), "Main")
	if ts == null: return done()
	eq(ts.get_physics_layers_count(), 2, "physics layers")
	var td := tile_data(ts, 0, Vector2i(0, 1))
	if td == null: return done()
	eq(td.get_collision_polygons_count(0), 4, "layer 0 polygons (rect, ellipse, polygon, one-way)")
	if ts.get_physics_layers_count() > 1:
		eq(td.get_collision_polygons_count(1), 1, "layer 1 polygons (physics_layer=1)")
	if td.get_collision_polygons_count(0) == 4:
		eq(td.get_collision_polygon_points(0, 0), PackedVector2Array([Vector2(-8, -8), Vector2(-8, 0), Vector2(8, 0), Vector2(8, -8)]), "rectangle polygon")
		eq(td.get_collision_polygon_points(0, 1).size(), 12, "ellipse approximation points")
		eq(td.get_collision_polygon_points(0, 2), PackedVector2Array([Vector2(0, 0), Vector2(8, 0), Vector2(8, 8)]), "polygon points")
		check(not td.is_collision_polygon_one_way(0, 2), "polygon 2 should not be one-way")
		check(td.is_collision_polygon_one_way(0, 3), "polygon with one_way=true is not one-way")
	return done()


func test_wangset_terrains() -> bool:
	var ts := tileset_of(_import("tilesets.tmx"), "Main")
	if ts == null: return done()
	eq(ts.get_terrain_sets_count(), 1, "terrain sets")
	if ts.get_terrain_sets_count() < 1: return done()
	eq(ts.get_terrain_set_mode(0), TileSet.TERRAIN_MODE_MATCH_CORNERS, "terrain mode")
	eq(ts.get_terrains_count(0), 2, "terrains in set 0")
	if ts.get_terrains_count(0) == 2:
		eq(ts.get_terrain_name(0, 0), "Grass", "terrain 0 name")
		eq(ts.get_terrain_name(0, 1), "Dirt", "terrain 1 name")
		eq(ts.get_terrain_color(0, 0), Color("#00ff00"), "terrain 0 color")
	var t16 := tile_data(ts, 0, Vector2i(0, 2))
	if t16:
		eq(t16.terrain_set, 0, "tile 16 terrain set")
		eq(t16.terrain, 0, "tile 16 terrain")
		eq(t16.get_terrain_peering_bit(TileSet.CELL_NEIGHBOR_TOP_RIGHT_CORNER), 0, "tile 16 top-right")
	var t17 := tile_data(ts, 0, Vector2i(1, 2))
	if t17: eq(t17.get_terrain_peering_bit(TileSet.CELL_NEIGHBOR_BOTTOM_LEFT_CORNER), 1, "tile 17 bottom-left")
	var t18 := tile_data(ts, 0, Vector2i(2, 2))
	if t18:
		eq(t18.get_terrain_peering_bit(TileSet.CELL_NEIGHBOR_TOP_RIGHT_CORNER), 0, "tile 18 top-right")
		eq(t18.get_terrain_peering_bit(TileSet.CELL_NEIGHBOR_BOTTOM_RIGHT_CORNER), 1, "tile 18 bottom-right")
	return done()


func test_image_collection_class_meta() -> bool:
	var td := tile_data(tileset_of(_import("tilesets.tmx"), "Main"), 4, Vector2i.ZERO)
	if td:
		check(td.has_meta("class"), "image collection tile class not stored")
		if td.has_meta("class"): eq(td.get_meta("class"), "area", "tile class")
	return done()


func test_new_big_tile_texture_origin() -> bool:
	# F1: oversized image-collection tile (32x48 on a 16x16 grid) is bottom-left aligned like Tiled.
	var td := tile_data(tileset_of(_import("tilesets.tmx"), "Main"), 3, Vector2i.ZERO)
	if td: eq(td.texture_origin, Vector2i(-8, 16), "texture_origin of 32x48 tile")
	return done()


func test_bug_tileoffset_on_plain_tiles() -> bool:
	# <tileoffset x=2 y=-3> must shift every tile of the tileset. The pre-change runtime applies
	# it only to tiles that have a <tile> entry or are flipped (alternative tiles).
	var td := tile_data(tileset_of(_import("tilesets.tmx"), "Main"), 1, Vector2i(1, 0))
	if td: eq(td.texture_origin, Vector2i(-2, 3), "texture_origin of tile with tileoffset (2,-3)")
	return done()


const FLIP_ROW_ALT_IDS: Array[int] = [0, 1, 2, 4, 3, 5, 6, 7]
const FLIP_ROW_TRANSFORMS: Array[int] = [0, FLIP_H, FLIP_V, FLIP_T, FLIP_H | FLIP_V, FLIP_H | FLIP_T, FLIP_V | FLIP_T, FLIP_H | FLIP_V | FLIP_T]


func test_flip_alternative_tiles() -> bool:
	var n := _import("tilesets.tmx", {"no_alternative_tiles": false})
	var l := layer(n, "Flips")
	if l == null: return done()
	eq(l.get_used_cells().size(), 8, "used cells")
	for x in range(8):
		if not cell(l, Vector2i(x, 0), 0, Vector2i(1, 0), FLIP_ROW_ALT_IDS[x]): continue
		var alt := FLIP_ROW_ALT_IDS[x]
		var td := tile_data(l.tile_set, 0, Vector2i(1, 0), alt)
		if td == null: continue
		eq(td.flip_h, (alt & 1) != 0, "alt %d flip_h" % alt)
		eq(td.flip_v, (alt & 2) != 0, "alt %d flip_v" % alt)
		eq(td.transpose, (alt & 4) != 0, "alt %d transpose" % alt)
	return done()


func test_new_flip_default_transform_flags() -> bool:
	# P13/B7: no_alternative_tiles is the new default -> engine transform flags in the alternative id.
	var l := layer(_import("tilesets.tmx"), "Flips")
	if l == null: return done()
	for x in range(8):
		cell(l, Vector2i(x, 0), 0, Vector2i(1, 0), FLIP_ROW_TRANSFORMS[x])
	return done()


func test_new_option_no_alternative_tiles() -> bool:
	var imp := _new_importer({"no_alternative_tiles": true})
	if not _require_option(imp, "no_alternative_tiles"): return done()
	var l := layer(_import_with(imp, "tilesets.tmx"), "Flips")
	if l == null: return done()
	for x in range(8):
		cell(l, Vector2i(x, 0), 0, Vector2i(1, 0), FLIP_ROW_TRANSFORMS[x])
	check(not atlas(l.tile_set, 0).has_alternative_tile(Vector2i(1, 0), 1), "alternative tiles were created")
	return done()


func test_new_option_map_wangset_to_terrain() -> bool:
	var imp := _new_importer({"map_wangset_to_terrain": true})
	if not _require_option(imp, "map_wangset_to_terrain"): return done()
	var ts := tileset_of(_import_with(imp, "tilesets.tmx"), "Main")
	if ts == null: return done()
	eq(ts.get_terrain_sets_count(), 1, "terrain sets")
	if ts.get_terrain_sets_count() > 0:
		eq(ts.get_terrains_count(0), 1, "old mapping: one terrain per wangset")
		if ts.get_terrains_count(0) > 0: eq(ts.get_terrain_name(0, 0), "Ground", "terrain name = wangset name")
	return done()


func test_new_option_custom_data_prefix() -> bool:
	var imp := _new_importer({"custom_data_prefix": "my_"})
	if not _require_option(imp, "custom_data_prefix"): return done()
	var ts := tileset_of(_import_with(imp, "tilesets.tmx"), "Main")
	if ts == null: return done()
	check(ts.get_custom_data_layer_by_name("speed") < 0, "'data_' still treated as prefix")
	var td := tile_data(ts, 0, Vector2i(0, 0))
	if td:
		check(td.has_meta("data_speed"), "with prefix 'my_' data_speed should be metadata")
	return done()


func test_new_option_use_default_filter() -> bool:
	var imp := _new_importer({"use_default_filter": true})
	if not _require_option(imp, "use_default_filter"): return done()
	var l := layer(_import_with(imp, "layers.tmx"), "Base")
	if l: eq(l.texture_filter, CanvasItem.TEXTURE_FILTER_PARENT_NODE, "texture_filter with use_default_filter")
	return done()


func test_new_option_add_class_as_metadata_off() -> bool:
	var imp := _new_importer({"add_class_as_metadata": false})
	if not _require_option(imp, "add_class_as_metadata"): return done()
	var n := _import_with(imp, "layers.tmx")
	var l := layer(n, "Styled")
	if l: check(not l.has_meta("class"), "class stored although add_class_as_metadata=false")
	return done()


func test_new_option_add_id_as_metadata() -> bool:
	var imp := _new_importer({"add_id_as_metadata": true})
	if not _require_option(imp, "add_id_as_metadata"): return done()
	var n := _import_with(imp, "objects.tmx")
	meta(need(n, "Rect (SB)"), "id", 1)
	meta(need(n, "Point"), "id", 11)
	return done()


func test_new_option_save_tileset_to() -> bool:
	var path := OUT + "saved_tileset.tres"
	DirAccess.remove_absolute(ProjectSettings.globalize_path(path))
	var imp := _new_importer({"save_tileset_to": path})
	if not _require_option(imp, "save_tileset_to"): return done()
	var l := layer(_import_with(imp, "background.tmx"), "Ground")
	check(FileAccess.file_exists(path), "tileset not saved to " + path)
	if l: eq(l.tile_set.resource_path, path, "layer tile_set resource_path")
	return done()


# ---------------------------------------------------------------------------
# Layers (layers.tmx, background.tmx, legacy_xml.tmx)

func test_layers_root_children() -> bool:
	var n := _import("layers.tmx")
	if n == null: return done()
	for c in ["Base", "Styled", "Hidden", "AddLayer", "MulLayer", "Group1", "TopDown", "Indexed",
			"ImgPlain", "ImgRepeat", "ImgPx2D", "layers (PBG)"]:
		check(n.get_node_or_null(NodePath(c)) != null, "root child '%s' missing" % c)
	check(find(n, "Skipped") == null, "no_import layer was imported")
	meta(n, "map_prop", "layers-map")
	return done()


func test_layer_attributes() -> bool:
	var n := _import("layers.tmx")
	var styled := layer(n, "Styled")
	if styled:
		eq(styled.modulate, Color(1, 0, 0, 0.5), "Styled modulate (tint + opacity)")
		eq(styled.position, Vector2(8, 4), "Styled offset")
		meta(styled, "class", "decor")
		eq(styled.get_used_cells().size(), 48, "Styled cells")
	var hidden := layer(n, "Hidden")
	if hidden: check(not hidden.visible, "Hidden layer is visible")
	var base := layer(n, "Base")
	if base:
		check(base.visible, "Base not visible")
		eq(base.modulate, Color(1, 1, 1, 1), "Base modulate")
		check(base.is_in_group("ground_layers"), "godot_group on layer not applied")
		eq(base.position, Vector2.ZERO, "Base position")
	return done()


func test_layer_parallax() -> bool:
	var n := _import("layers.tmx")
	var l := layer(n, "Parallaxed")
	if l == null: return done()
	var pl := l.get_parent()
	if is_type(pl, "ParallaxLayer", "parent of Parallaxed"):
		eq(String(pl.name), "Parallaxed (PL)", "ParallaxLayer name")
		eq((pl as ParallaxLayer).motion_scale, Vector2(0.5, 0.75), "motion_scale")
		is_type(pl.get_parent(), "ParallaxBackground", "grand parent of Parallaxed")
	return done()


func test_group_layer() -> bool:
	var n := _import("layers.tmx")
	var g := need(n, "Group1", "Node2D") as Node2D
	if g == null: return done()
	eq(g.position, Vector2(10, 20), "group offset")
	eq(g.modulate.a, 0.8, "group opacity")
	var gt := g.get_node_or_null("GroupTiles") as TileMapLayer
	if is_type(gt, "TileMapLayer", "GroupTiles"):
		eq(gt.get_used_cells().size(), 48, "GroupTiles cells")
		cell(gt, Vector2i(0, 0), 0, Vector2i(3, 1))
	var go := g.get_node_or_null("GroupObjects")
	if is_type(go, "Node2D", "GroupObjects"):
		var body := go.get_node_or_null("GObj (SB)")
		if is_type(body, "StaticBody2D", "GObj (SB)"):
			eq((body as Node2D).position, Vector2(8, 8), "GObj position")
	return done()


func test_new_layer_blend_modes() -> bool:
	var n := _import("layers.tmx")
	var add := layer(n, "AddLayer")
	var mul := layer(n, "MulLayer")
	if add and is_type(add.material, "CanvasItemMaterial", "AddLayer material"):
		eq((add.material as CanvasItemMaterial).blend_mode, CanvasItemMaterial.BLEND_MODE_ADD, "AddLayer blend")
	if mul and is_type(mul.material, "CanvasItemMaterial", "MulLayer material"):
		eq((mul.material as CanvasItemMaterial).blend_mode, CanvasItemMaterial.BLEND_MODE_MUL, "MulLayer blend")
	var base := layer(n, "Base")
	if base: check(base.material == null, "normal layer got a material")
	return done()


func test_new_object_layer_draworder() -> bool:
	var n := _import("layers.tmx")
	var td := need(n, "TopDown", "Node2D") as Node2D
	var ix := need(n, "Indexed", "Node2D") as Node2D
	if td: check(td.y_sort_enabled, "topdown (default) object layer is not y-sorted")
	if ix: check(not ix.y_sort_enabled, "index object layer is y-sorted")
	if ix and ix.get_child_count() == 2:
		eq(String(ix.get_child(0).name), "IX_A", "index order kept")
	return done()


func test_image_layers() -> bool:
	var n := _import("layers.tmx")
	var plain := need(n, "ImgPlain", "TextureRect") as TextureRect
	if plain:
		eq(plain.position, Vector2(4, 6), "ImgPlain position")
		eq(plain.size, Vector2(64, 32), "ImgPlain size")
		if check(plain.texture != null, "ImgPlain texture missing"):
			eq(plain.texture.get_size(), Vector2(64, 32), "ImgPlain texture size")
	var par := need(n, "ImgParallax", "TextureRect")
	if par and is_type(par.get_parent(), "ParallaxLayer", "ImgParallax parent"):
		eq((par.get_parent() as ParallaxLayer).motion_scale.x, 0.5, "ImgParallax motion_scale.x")
	var px := need(n, "ImgPx2D", "Parallax2D") as Parallax2D
	if px:
		eq(px.scroll_scale, Vector2(1, 0.5), "ImgPx2D scroll_scale")
		eq(px.repeat_size, Vector2(0, 32), "ImgPx2D repeat_size")
		eq(px.repeat_times, 2, "ImgPx2D repeat_times")
		var tr := px.get_node_or_null("bg")
		if is_type(tr, "TextureRect", "ImgPx2D/bg"):
			check((tr as TextureRect).texture != null, "ImgPx2D texture missing")
	return done()


func test_bug_parallax_layer_default_factor() -> bool:
	# Tiled's default parallax factor is 1.0 for an axis that is not set; the ParallaxLayer path
	# (layer with only parallaxx) uses 0.0, freezing the other axis.
	var par := need(_import("layers.tmx"), "ImgParallax", "TextureRect")
	if par and is_type(par.get_parent(), "ParallaxLayer", "ImgParallax parent"):
		eq((par.get_parent() as ParallaxLayer).motion_scale.y, 1.0, "ImgParallax motion_scale.y (parallaxy unset)")
	return done()


func test_new_image_layer_repeat_without_parallax() -> bool:
	# T7: repeatx on a plain image layer tiles the texture across the map width (8*16 = 128 px),
	# keeping the phase aligned with offsetx (8).
	var tr := need(_import("layers.tmx"), "ImgRepeat", "TextureRect") as TextureRect
	if tr == null: return done()
	eq(tr.stretch_mode, TextureRect.STRETCH_TILE, "stretch_mode")
	check(tr.position.x <= 8.0 + 0.001, "repeat should start at or before the offset, starts at %s" % tr.position.x)
	check(tr.position.x + tr.size.x >= 128.0 - 0.001, "repeat does not cover the map width: %s..%s" % [tr.position.x, tr.position.x + tr.size.x])
	eq(fposmod(tr.position.x - 8.0, 64.0), 0.0, "tile phase vs offsetx")
	eq(tr.size.y, 32.0, "height (not repeated)")
	eq(tr.position.y, 0.0, "y position")
	return done()


func test_background_color() -> bool:
	var n := _import("background.tmx")
	if n == null: return done()
	eq(String(n.name), "background", "root name")
	var bg := need(n, "Background Color", "ColorRect") as ColorRect
	if bg:
		eq(bg.color, Color("#336699"), "background color")
		eq(bg.size, Vector2(64, 64), "background size")
	var l := layer(n, "Ground")
	if l: eq(l.get_used_cells().size(), 16, "Ground cells")
	return done()


func test_legacy_xml_does_not_break_parsing() -> bool:
	var n := _import("legacy_xml.tmx")
	var l := layer(n, "AfterLegacy")
	if l: eq(l.get_used_cells().size(), 8, "layer after XML-encoded layer")
	return done()


func test_new_legacy_xml_tile_data() -> bool:
	# T8: <data> with <tile gid=".."/> children (no encoding attribute).
	var l := layer(_import("legacy_xml.tmx"), "LegacyXml")
	if l == null: return done()
	eq(l.get_used_cells().size(), 5, "used cells")
	cell(l, Vector2i(0, 0), 0, Vector2i(0, 0))
	cell(l, Vector2i(1, 0), 0, Vector2i(1, 0))
	cell(l, Vector2i(3, 0), 0, Vector2i(3, 0))
	cell(l, Vector2i(0, 1), 0, Vector2i(0, 1))
	cell(l, Vector2i(3, 1), 0, Vector2i(1, 0))
	return done()


# ---------------------------------------------------------------------------
# Objects (objects.tmx)

func _objects() -> Node:
	return _import("objects.tmx", {}, "custom.tiled-project")


func test_objects_layer_structure() -> bool:
	var n := _objects()
	if n == null: return done()
	need(n, "Objects", "Node2D")
	need(n, "IndexObjects", "Node2D")
	var g := need(n, "ObjGroup", "Node2D")
	if g: is_type(g.get_node_or_null("InnerObjects"), "Node2D", "ObjGroup/InnerObjects")
	check(find(n, "NoImportObjs") == null, "no_import object layer imported")
	check(find(n, "ShouldNotExist") == null, "object of no_import layer imported")
	return done()


func test_object_rectangle_default() -> bool:
	var body := need(_objects(), "Rect (SB)", "StaticBody2D") as Node2D
	if body == null: return done()
	eq(body.position, Vector2(16, 16), "position")
	var cs := body.get_node_or_null("Rect") as CollisionShape2D
	if is_type(cs, "CollisionShape2D", "Rect shape") and is_type(cs.shape, "RectangleShape2D", "Rect shape resource"):
		eq((cs.shape as RectangleShape2D).size, Vector2(32, 16), "rectangle size")
		eq(cs.position, Vector2(16, 8), "shape centred in the rectangle")
	return done()


func test_object_classes() -> bool:
	var n := _objects()
	var area := need(n, "AreaRect (Area)", "Area2D")
	if area:
		meta(area, "class", "area")
		eq(children_of_type(area, "CollisionShape2D").size(), 1, "Area2D shape count")
	need(n, "StaticRect (SB)", "StaticBody2D")
	need(n, "AnimRect (AB)", "AnimatableBody2D")
	var nav := need(n, "NavRect (NR)", "NavigationRegion2D") as NavigationRegion2D
	if nav and check(nav.navigation_polygon != null, "navigation polygon missing"):
		eq(nav.navigation_polygon.get_vertices().size(), 4, "navigation vertices")
		eq(nav.position, Vector2(16, 48), "NavRect position")
	var occ := need(n, "OccRect (LO)", "LightOccluder2D") as LightOccluder2D
	if occ and check(occ.occluder != null, "occluder missing"):
		eq(occ.occluder.polygon.size(), 4, "occluder points")
	var poly := need(n, "PolyRect", "Polygon2D") as Polygon2D
	if poly: eq(poly.polygon, PackedVector2Array([Vector2(0, 0), Vector2(0, 16), Vector2(16, 16), Vector2(16, 0)]), "Polygon2D points")
	return done()


func test_object_ellipse_capsule() -> bool:
	var n := _objects()
	var e := need(n, "Ellipse (SB)", "StaticBody2D")
	if e:
		var cs := e.get_node_or_null("Ellipse") as CollisionShape2D
		if is_type(cs, "CollisionShape2D", "Ellipse shape") and is_type(cs.shape, "CapsuleShape2D", "Ellipse shape resource"):
			eq((cs.shape as CapsuleShape2D).height, 32.0, "ellipse capsule height")
			eq((cs.shape as CapsuleShape2D).radius, 8.0, "ellipse capsule radius")
			eq(cs.rotation_degrees, 90.0, "wide ellipse rotated")
			eq(cs.position, Vector2(16, 8), "ellipse centre")
	var c := need(n, "Capsule (SB)", "StaticBody2D")
	if c:
		var cs := c.get_node_or_null("Capsule") as CollisionShape2D
		if is_type(cs, "CollisionShape2D", "Capsule shape") and is_type(cs.shape, "CapsuleShape2D", "Capsule shape resource"):
			eq((cs.shape as CapsuleShape2D).height, 32.0, "capsule height")
			eq((cs.shape as CapsuleShape2D).radius, 8.0, "capsule radius")
			eq(cs.rotation_degrees, 0.0, "tall capsule not rotated")
	return done()


func test_object_point() -> bool:
	var p := need(_objects(), "Point", "Marker2D") as Marker2D
	if p: eq(p.position, Vector2(200, 100), "point position")
	return done()


func test_object_polygon() -> bool:
	var b := need(_objects(), "Poly (SB)", "StaticBody2D") as Node2D
	if b == null: return done()
	eq(b.position, Vector2(160, 160), "polygon body position")
	var cp := b.get_node_or_null("Poly") as CollisionPolygon2D
	if is_type(cp, "CollisionPolygon2D", "Poly shape"):
		eq(cp.polygon, PackedVector2Array([Vector2(0, 0), Vector2(32, 0), Vector2(32, 32)]), "collision polygon")
	return done()


func test_object_polylines() -> bool:
	var n := _objects()
	var b := need(n, "Polyline (SB)", "StaticBody2D")
	if b:
		var segs := children_of_type(b, "CollisionShape2D")
		eq(segs.size(), 2, "segment count")
		if segs.size() == 2 and is_type((segs[0] as CollisionShape2D).shape, "SegmentShape2D", "segment shape"):
			var s := (segs[0] as CollisionShape2D).shape as SegmentShape2D
			eq(s.a, Vector2(0, 0), "segment 0 a")
			eq(s.b, Vector2(20, 0), "segment 0 b")
	var line := need(n, "Line", "Line2D") as Line2D
	if line:
		eq(line.points, PackedVector2Array([Vector2(0, 0), Vector2(10, 10), Vector2(20, 0)]), "Line2D points")
		eq(line.position, Vector2(40, 200), "Line2D position")
	var path := need(n, "Path", "Path2D") as Path2D
	if path and check(path.curve != null, "Path2D curve missing"):
		eq(path.curve.point_count, 3, "curve points")
		eq(path.curve.get_point_position(1), Vector2(10, 10), "curve point 1")
	return done()


func test_object_text() -> bool:
	var t := need(_objects(), "Text", "Label") as Label
	if t == null: return done()
	eq(t.text, "Hello YATI", "text")
	eq(t.position, Vector2(50, 120), "position")
	eq(t.size.x, 100.0, "width")
	check(t.size.y >= 20.0, "height %s < object height 20 (Label may grow to its minimum size)" % t.size.y)
	eq(t.horizontal_alignment, HORIZONTAL_ALIGNMENT_CENTER, "halign")
	eq(t.autowrap_mode, TextServer.AUTOWRAP_WORD_SMART, "wrap")
	eq(t.get_theme_font_size("font_size"), 12, "pixel size")
	var f := t.get_theme_font("font")
	if is_type(f, "SystemFont", "font"):
		check((f as SystemFont).font_names.has("Sans"), "font family not applied")
	return done()


func test_new_text_bold_italic() -> bool:
	var t := need(_objects(), "Text", "Label") as Label
	if t == null: return done()
	var f := t.get_theme_font("font")
	if is_type(f, "SystemFont", "font"):
		eq((f as SystemFont).font_weight, 700, "bold -> font_weight")
		eq((f as SystemFont).font_italic, true, "italic")
	return done()


func test_object_tile_sprites() -> bool:
	var n := _objects()
	var s := need(n, "TileObj", "Sprite2D") as Sprite2D
	if s:
		eq(s.position, Vector2(32, 288), "position")
		check(s.texture != null, "texture missing")
		check(s.region_enabled, "region not enabled")
		eq(s.region_rect, Rect2(0, 0, 16, 16), "region")
		eq(s.offset, Vector2(8, -8), "bottom-left alignment offset")
		check(not s.flip_h, "unexpected flip_h")
		meta(s, "kind", "grass")
		meta(s, "own", "mine")
	var f := need(n, "TileObjFlipped", "Sprite2D") as Sprite2D
	if f:
		check(f.flip_h, "H-flipped tile object not flipped")
		check(not f.flip_v, "unexpected flip_v")
		eq(f.region_rect, Rect2(16, 0, 16, 16), "flipped region")
	return done()


func test_object_tile_meta_to_properties() -> bool:
	var s := need(_objects(), "LootTile", "Sprite2D")
	if s == null: return done()
	meta(s, "loot", "gold")
	meta(s, "weight", 1.5)
	meta(s, "stack", 4)
	meta(s, "own", "mine")
	return done()


func _check_tile_body(n: Node, body_name: String, cls: String) -> void:
	var b := need(n, body_name, cls) as Node2D
	if b == null: return
	eq(children_of_type(b, "Sprite2D").size(), 1, body_name + " sprite child")
	eq(children_of_type(b, "CollisionShape2D").size(), 3, body_name + " CollisionShape2D children (rect, ellipse, layer1 rect)")
	var polys := children_of_type(b, "CollisionPolygon2D")
	eq(polys.size(), 2, body_name + " CollisionPolygon2D children")
	var one_way := 0
	for p in polys:
		if (p as CollisionPolygon2D).one_way_collision: one_way += 1
	eq(one_way, 1, body_name + " one-way polygons")


func test_object_tile_collision_bodies() -> bool:
	var n := _objects()
	_check_tile_body(n, "CollTileObj", "StaticBody2D")
	_check_tile_body(n, "CharTile", "CharacterBody2D")
	_check_tile_body(n, "RigidTile", "RigidBody2D")
	_check_tile_body(n, "AreaTile", "Area2D")
	var b := find(n, "CollTileObj") as Node2D
	if b and b is StaticBody2D:
		eq(b.position, Vector2(96, 288), "CollTileObj position")
		var r := b.get_node_or_null("CRect") as CollisionShape2D
		if is_type(r, "CollisionShape2D", "CRect") and is_type(r.shape, "RectangleShape2D", "CRect shape"):
			eq((r.shape as RectangleShape2D).size, Vector2(16, 8), "CRect size")
		var e := b.get_node_or_null("CEllipse") as CollisionShape2D
		if is_type(e, "CollisionShape2D", "CEllipse"):
			is_type(e.shape, "CapsuleShape2D", "CEllipse shape")
		var l1 := b.get_node_or_null("CLayer1") as CollisionShape2D
		if is_type(l1, "CollisionShape2D", "CLayer1") and is_type(l1.shape, "RectangleShape2D", "CLayer1 shape"):
			eq((l1.shape as RectangleShape2D).size, Vector2(8, 8), "CLayer1 size")
	return done()


func test_new_object_opacity() -> bool:
	var b := need(_objects(), "Faded (SB)", "StaticBody2D") as CanvasItem
	if b: eq(b.modulate.a, 0.5, "opacity -> modulate.a")
	var r := need(_objects(), "Rect (SB)", "StaticBody2D") as CanvasItem
	if r: eq(r.modulate.a, 1.0, "default opacity")
	return done()


func test_object_visibility() -> bool:
	var b := need(_objects(), "HiddenRect (SB)", "StaticBody2D") as CanvasItem
	if b: check(not b.visible, "visible=0 object is visible")
	return done()


func test_object_property_types() -> bool:
	var p := need(_objects(), "Props (Area)", "Area2D")
	if p == null: return done()
	meta(p, "p_string", "hello")
	meta(p, "p_int", 42)
	meta(p, "p_float", 2.5)
	meta(p, "p_bool", true)
	meta(p, "p_color", Color("#ff000080")) # Tiled #AARRGGBB -> Color (a #RRGGBBAA String before B4)
	meta(p, "p_file", "data/info.txt")
	if p.has_meta("p_int"): eq(typeof(p.get_meta("p_int")), TYPE_INT, "p_int type")
	if p.has_meta("p_bool"): eq(typeof(p.get_meta("p_bool")), TYPE_BOOL, "p_bool type")
	check(p.is_in_group("enemies"), "godot_group 'enemies' missing")
	check(p.is_in_group("bosses"), "godot_group 'bosses' missing (comma list with blank)")
	check(not p.has_meta("godot_node_type"), "godot_node_type stored as metadata")
	check(not p.has_meta("godot_group"), "godot_group stored as metadata")
	return done()


func test_object_reference_top_level() -> bool:
	var p := need(_objects(), "Props (Area)", "Area2D")
	if p == null: return done()
	meta(p, "p_obj_poly", PackedVector2Array([Vector2(160, 160), Vector2(192, 160), Vector2(192, 192)]))
	meta(p, "p_obj_point", Vector2(200, 100))
	return done()


func test_new_object_reference_in_group() -> bool:
	# T6: object inside a group layer.
	var p := need(_objects(), "Props (Area)", "Area2D")
	if p: meta(p, "p_obj_group", PackedVector2Array([Vector2(100, 100), Vector2(110, 100), Vector2(110, 110), Vector2(100, 110)]))
	return done()


func test_new_class_property_dictionary() -> bool:
	# T5: class-typed property -> Dictionary {member: typed value}.
	var p := need(_objects(), "Props (Area)", "Area2D")
	if p == null or not p.has_meta("p_class"):
		fail("p_class missing")
		return done()
	var v: Variant = p.get_meta("p_class")
	if not check(typeof(v) == TYPE_DICTIONARY, "p_class is %s (%s), expected Dictionary" % [type_string(typeof(v)), str(v)]): return done()
	var d: Dictionary = v
	eq(d.size(), 2, "member count")
	eq(d.get("hp"), 7, "member hp")
	eq(typeof(d.get("hp")), TYPE_INT, "member hp type")
	eq(d.get("label"), "boss", "member label")
	return done()


func test_templates() -> bool:
	var n := _objects()
	var expect := {"T1": [Vector2(32, 240), 10], "T2": [Vector2(64, 240), 99], "Enemy": [Vector2(96, 240), 10]}
	for nm: String in expect:
		var s := need(n, nm, "Sprite2D") as Sprite2D
		if s == null: continue
		eq(s.position, expect[nm][0], nm + " position")
		check(s.texture != null, nm + " texture missing")
		eq(s.region_rect, Rect2(32, 0, 16, 16), nm + " region (template gid 3)")
		meta(s, "hp", expect[nm][1])
		meta(s, "faction", "orcs")
	return done()


func test_custom_types_project_file() -> bool:
	var b := need(_objects(), "EnemyRect (SB)", "StaticBody2D")
	if b:
		meta(b, "class", "Enemy")
		meta(b, "hp", 100)
		meta(b, "speed", 1.5)
	var without := need(_import("objects.tmx"), "EnemyRect (SB)", "StaticBody2D")
	if without: check(not without.has_meta("hp"), "custom type members merged without project file")
	return done()


# ---------------------------------------------------------------------------
# JSON map

func test_json_map() -> bool:
	var n := _import("json_map.tmj")
	if n == null: return done()
	eq(String(n.name), "json_map", "root name")
	var l := layer(n, "JTiles")
	if l:
		eq(l.get_used_cells().size(), 8, "used cells")
		cell(l, Vector2i(0, 0), 0, Vector2i(0, 0))
		cell(l, Vector2i(1, 1), 0, Vector2i(1, 1))
		eq(l.get_cell_atlas_coords(Vector2i(0, 3)), Vector2i(1, 0), "V-flipped cell atlas")
		check(l.get_cell_alternative_tile(Vector2i(0, 3)) != 0, "V-flipped cell not flipped")
		var td := tile_data(l.tile_set, 0, Vector2i(0, 0))
		if td:
			eq(td.get_custom_data("speed"), 5, "JSON tile custom data")
			check(td.has_meta("kind") and td.get_meta("kind") == "json-grass", "JSON tile meta kind")
	var r := need(n, "JRect (SB)", "StaticBody2D") as Node2D
	if r:
		eq(r.position, Vector2(16, 16), "JRect position")
		meta(r, "p_int", 42)
		if r.has_meta("p_int"): eq(typeof(r.get_meta("p_int")), TYPE_INT, "p_int type")
		meta(r, "p_str", "json")
	var p := need(n, "JPoint", "Marker2D") as Marker2D
	if p: eq(p.position, Vector2(40, 48), "JPoint position")
	return done()


func test_new_json_class_property_dictionary() -> bool:
	var r := need(_import("json_map.tmj"), "JRect (SB)", "StaticBody2D")
	if r == null or not r.has_meta("stats"):
		fail("stats missing")
		return done()
	var v: Variant = r.get_meta("stats")
	if not check(typeof(v) == TYPE_DICTIONARY, "stats is %s (%s), expected Dictionary" % [type_string(typeof(v)), str(v)]): return done()
	eq((v as Dictionary).get("hp"), 5, "member hp")
	eq((v as Dictionary).get("label"), "orc", "member label")
	return done()


# ---------------------------------------------------------------------------
# Runtime improvement plan, part A (docs/runtime-improvement-plan.md, B1-B10)

func test_bug_area_gravity() -> bool:
	# B5: the gravity properties of an Area2D were misspelt (gravitiy...): 'gravity' and
	# 'gravity_point_unit_distance' aborted the property loop with a script error, the
	# centre/direction components became metadata.
	var a := need(_import("area_gravity.tmx"), "Field (Area)", "Area2D") as Area2D
	if a == null: return done()
	eq(a.gravity, 300.0, "gravity")
	check(a.gravity_point, "gravity_point not set")
	eq(a.gravity_point_center, Vector2(0.25, -0.5), "gravity_point_center")
	eq(a.gravity_point_unit_distance, 12.5, "gravity_point_unit_distance")
	eq(a.linear_damp, 0.75, "linear_damp")
	eq(a.angular_damp, 2.5, "angular_damp")
	eq(a.priority, 3, "priority (listed after gravity)")
	for k in ["gravity_point_center_x", "gravity_point_center_y"]:
		check(not a.has_meta(k), k + " stored as metadata")
	# Godot keeps gravity_point_center and gravity_direction in one vector, so a second
	# area (no gravity_point) checks the direction.
	var w := need(a.get_parent(), "Wind (Area)", "Area2D") as Area2D
	if w == null: return done()
	eq(w.gravity_direction, Vector2(1, 0), "gravity_direction")
	eq(w.gravity, 50.0, "gravity (int typed)")
	check(not w.monitorable, "monitorable (listed after gravity) not applied")
	for k in ["gravity_direction_x", "gravity_direction_y"]:
		check(not w.has_meta(k), k + " stored as metadata")
	return done()


func test_bug_argb_colors() -> bool:
	# B3: Tiled writes colours as #AARRGGBB, Godot's Color(String) reads #RRGGBBAA: a tint of
	# #80ff0000 (red, 50 % alpha) became yellow-green, the background colour transparent.
	var red_half := Color(1, 0, 0, 128.0 / 255.0)
	var n := _import("colors.tmx")
	if n == null: return done()
	var bg := need(n, "Background Color", "ColorRect") as ColorRect
	if bg: eq(bg.color, red_half, "background colour")
	var tiles := layer(n, "TintTiles")
	if tiles: eq(tiles.modulate, red_half, "tile layer tint")
	var objs := need(n, "TintObjects", "Node2D") as Node2D
	if objs: eq(objs.modulate, red_half, "object layer tint")
	var grp := need(n, "TintGroup", "Node2D") as Node2D
	if grp: eq(grp.modulate, Color(1, 0, 0, red_half.a * 0.5), "group tint x opacity")
	var img := need(n, "TintImage", "TextureRect") as TextureRect
	if img: eq(img.modulate, red_half, "image layer tint")
	var lbl := need(n, "Caption", "Label") as Label
	if lbl: eq(lbl.get_theme_color("font_color"), red_half, "text colour")
	return done()


func test_bug_color_property_type() -> bool:
	# B4: colour-typed properties became #RRGGBBAA Strings (metadata) and did not drive the
	# colour properties, which matched only type 'string'. Now they are Colors.
	var n := _import("colors.tmx")
	if n == null: return done()
	var a := need(n, "Painted (Area)", "Area2D") as Area2D
	if a:
		eq(a.modulate, Color(0, 1, 0, 1), "modulate (color typed)")
		eq(a.self_modulate, Color(0, 0, 1, 128.0 / 255.0), "self_modulate (color typed)")
		var cs := a.get_child(0) as CollisionShape2D
		if is_type(cs, "CollisionShape2D", "Painted shape"):
			eq(cs.debug_color, Color(1, 1, 1, 64.0 / 255.0), "debug_color (color typed)")
		if check(a.has_meta("tint"), "meta 'tint' missing"):
			eq(typeof(a.get_meta("tint")), TYPE_COLOR, "meta 'tint' type")
			eq(a.get_meta("tint"), Color(1, 0, 0, 128.0 / 255.0), "meta 'tint'")
		# a string property is not reinterpreted
		meta(a, "plain_modulate_string", "#ff000080")
	var s := need(n, "StringTint (Area)", "Area2D") as Area2D
	if s: eq(s.modulate, Color("#ff000080"), "string-typed modulate keeps Godot's #RRGGBBAA")
	var poly := need(n, "Poly", "Polygon2D") as Polygon2D
	if poly: eq(poly.color, Color(0, 0, 1, 128.0 / 255.0), "Polygon2D color (color typed)")
	var line := need(n, "Wire", "Line2D") as Line2D
	if line: eq(line.default_color, Color(0, 1, 0, 1), "Line2D default_color (color typed)")
	# Tiles: modulate, a colour custom data layer (TYPE_COLOR) and colour metadata
	var ts := tileset_of(n, "TintTiles")
	var td := tile_data(ts, 0, Vector2i(1, 0))
	if td:
		eq(td.modulate, Color(0, 0, 1, 1), "tile modulate (color typed)")
		var layer_id := ts.get_custom_data_layer_by_name("glow")
		if check(layer_id >= 0, "custom data layer 'glow' missing"):
			eq(ts.get_custom_data_layer_type(layer_id), TYPE_COLOR, "custom data layer type")
			eq(td.get_custom_data("glow"), Color(0, 1, 0, 128.0 / 255.0), "custom data 'glow'")
		if check(td.has_meta("hue"), "tile meta 'hue' missing"):
			eq(td.get_meta("hue"), Color("#336699"), "tile meta 'hue'")
	return done()


# ---------------------------------------------------------------------------
# Zip (keep last: the pre-change runtime leaks zip mode, see _reset_zip_mode)

func test_zip_import() -> bool:
	var imp := _new_importer()
	var n: Node = imp.call("import_from_zip", FIX + "bundle.zip", "maps/zipmap.tmx", "")
	if not check(n != null, "import_from_zip returned null"): return done()
	_to_free.append(n)
	eq(String(n.name), "zipmap", "root name")
	var l := layer(n, "ZipLayer")
	if l:
		eq(l.get_used_cells().size(), 9, "used cells")
		cell(l, Vector2i(1, 0), 0, Vector2i(1, 0))
		var src := atlas(l.tile_set, 0)
		if src and check(src.texture != null, "texture from zip missing"):
			eq(src.texture.get_size(), Vector2(32, 16), "texture size from zip")
	var pt := need(n, "ZipPoint", "Marker2D") as Marker2D
	if pt: eq(pt.position, Vector2(8, 8), "ZipPoint position")
	return done()


func test_new_plain_import_after_zip_reads_disk() -> bool:
	# F2: import() after import_from_zip() must read from the file system again.
	var imp := _new_importer()
	var z: Node = imp.call("import_from_zip", FIX + "bundle.zip", "maps/zipmap.tmx", "")
	if z: _to_free.append(z)
	var n: Node = imp.call("import", FIX + "background.tmx", "")
	if not check(n != null, "import() after import_from_zip() returned null (still reading the zip?)"): return done()
	_to_free.append(n)
	var l := layer(n, "Ground")
	if l: eq(l.get_used_cells().size(), 16, "Ground cells")
	# a fresh importer instance must not be affected either
	var n2: Node = _new_importer().call("import", FIX + "tilesets.tmx", "")
	if check(n2 != null, "fresh importer after zip import returned null"):
		_to_free.append(n2)
	return done()
