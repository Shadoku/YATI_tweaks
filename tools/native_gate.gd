extends SceneTree
## Two checks of stage 2 of docs/native-importer-plan.md that the test suite does not make:
##
## - leaks: every fixture map (and a world) imported and its tree freed, 20 times over; the engine's object
##   and node counts (Performance.OBJECT_COUNT, OBJECT_NODE_COUNT, OBJECT_ORPHAN_NODE_COUNT)
##   must be back at their values after the warm-up round. Exit code 1 if not. A few frames run
##   before each count: the servers release what freed nodes handed them (navigation regions'
##   polygons, for one) only on their next iteration.
## - small map: test/fixtures/layers.tmx imported 10 times warm, the mean and the minimum; and
##   10 times with import_async() (stage 3; a runtime without it is skipped): the frames it
##   takes, its wall time from the call to the tree, and the main thread's worst frame and build
##   frame (the engine's headless frame sleep turned off meanwhile, so a frame's wall time is
##   its work).
## - engine floor (when test/benchmark.gd has generated its map): the benchmark's tiles-only map
##   imported, against the engine assigning the same six layers' tile_map_data blobs to fresh
##   layers (what no importer can go below), each the best of 3.
##
##   godot --headless -s tools/native_gate.gd [-- --runtime=res://runtime]
##
## The runtime defaults to the native importer's facade, res://addons/yati_native.

const FIX := "res://test/fixtures/"
const MAPS: Array[String] = [
	"tilesets.tmx", "layers.tmx", "objects.tmx", "encodings.tmx", "infinite.tmx", "isometric.tmx",
	"staggered.tmx", "hexagonal.tmx", "background.tmx", "legacy_xml.tmx", "json_map.tmj",
	"animations.tmx", "area_gravity.tmx", "colors.tmx", "layer_class.tmx", "parallax.tmx",
	"single_layer.tmx", "templates.tmx", "template_files.tmx", "unknown_class.tmx", "clip.tmx",
	"clip.tmj", "missing_tileset.tmx", "object_refs.tmx", "masks.tmx", "scenes.tmx",
]
# The project file a map is imported with
const PROJECTS := {
	"objects.tmx": "custom.tiled-project", "masks.tmx": "masks.tiled-project",
	"scenes.tmx": "scenes.tiled-project",
}
const ROUNDS := 20

var runtime_dir := "res://addons/yati_native"


func _initialize() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--runtime="):
			runtime_dir = arg.substr(10).trim_suffix("/")
	var script: GDScript = load(runtime_dir.path_join("Importer.gd"))
	if script == null:
		printerr("native_gate: cannot load " + runtime_dir.path_join("Importer.gd"))
		quit(2)
		return
	main(script)


func main(script: GDScript) -> void:
	var ok: bool = await check_leaks(script)
	small_map(script)
	await small_map_async(script)
	engine_floor(script)
	quit(0 if ok else 1)


func settle() -> void:
	for i in range(10):
		await process_frame
		await physics_frame


func counts() -> Array[int]:
	return [
		int(Performance.get_monitor(Performance.OBJECT_COUNT)),
		int(Performance.get_monitor(Performance.OBJECT_NODE_COUNT)),
		int(Performance.get_monitor(Performance.OBJECT_ORPHAN_NODE_COUNT)),
	]


func import_all(script: GDScript) -> void:
	for f in MAPS:
		var imp: Object = script.new()
		var project: String = FIX + PROJECTS[f] if PROJECTS.has(f) else ""
		var n: Node = imp.call("import", FIX + f, project)
		if n != null:
			n.free()
		var z: Node = imp.call("import_from_zip", FIX + "bundle.zip", "maps/zipmap.tmx", "")
		if z != null:
			z.free()
	# Scenes for classes (with their _tiled_import hooks) and a world, if the importer has them
	var imp: Object = script.new()
	imp.set("class_scenes", {"Enemy": FIX + "enemy.tscn"})
	var s: Node = imp.call("import", FIX + "scenes.tmx", "")
	if s != null:
		s.free()
	if imp.has_method("import_world"):
		var w: Node = imp.call("import_world", FIX + "world/test.world", "")
		if w != null:
			w.free()


func check_leaks(script: GDScript) -> bool:
	# Warm-up: resource caches, the format check, static state; then one more round with frames
	# after it, since the first frames after the warm-up make an object or so of their own.
	import_all(script)
	import_all(script)
	await settle()
	import_all(script)
	await settle()
	var before := counts()
	for i in range(ROUNDS):
		import_all(script)
	await settle()
	var after := counts()
	var ok := after == before
	print("native_gate: leaks %s: objects %d -> %d, nodes %d -> %d, orphan nodes %d -> %d after %d rounds of %d maps" % [
		"none" if ok else "FOUND", before[0], after[0], before[1], after[1], before[2], after[2],
		# Per round: MAPS, the zip's map after each, scenes.tmx with class_scenes, the world's 4 maps
		ROUNDS, 2 * MAPS.size() + 1 + 4])
	return ok


func small_map(script: GDScript) -> void:
	var path := FIX + "layers.tmx"
	var times: Array[float] = []
	for i in range(11):
		var imp: Object = script.new()
		var t := Time.get_ticks_usec()
		var n: Node = imp.call("import", path, "")
		var ms := (Time.get_ticks_usec() - t) / 1000.0
		if n != null:
			n.free()
		if i > 0:
			times.append(ms)
	var total := 0.0
	var lo := INF
	for v in times:
		total += v
		lo = minf(lo, v)
	print("native_gate: small map layers.tmx, %d warm imports: mean %.2f ms, min %.2f ms" % [times.size(), total / times.size(), lo])


func small_map_async(script: GDScript) -> void:
	if not script.new().has_method("import_async"):
		print("native_gate: small map async skipped (the runtime has no import_async)")
		return
	var path := FIX + "layers.tmx"
	var sleep := OS.low_processor_usage_mode_sleep_usec
	OS.low_processor_usage_mode_sleep_usec = 0
	var stamps: PackedInt64Array = []
	var on_frame := func() -> void: stamps.append(Time.get_ticks_usec())
	process_frame.connect(on_frame)
	var walls: Array[float] = []
	var frames: Array[int] = []
	var worst := 0.0
	var builds: Array[float] = []
	for i in range(11):
		var imp: Object = script.new()
		await process_frame
		stamps.clear()
		var t := Time.get_ticks_usec()
		stamps.append(t)
		var n: Node = await imp.call("import_async", path, "")
		var t_done := Time.get_ticks_usec()
		if n != null:
			n.free()
		if i == 0:
			continue
		walls.append((t_done - t) / 1000.0)
		frames.append(stamps.size())
		for k in range(stamps.size() - 1):
			worst = maxf(worst, (stamps[k + 1] - stamps[k]) / 1000.0)
		builds.append((t_done - stamps[stamps.size() - 1]) / 1000.0)
	process_frame.disconnect(on_frame)
	OS.low_processor_usage_mode_sleep_usec = sleep
	var wall_sum := 0.0
	var build_sum := 0.0
	for k in range(walls.size()):
		wall_sum += walls[k]
		build_sum += builds[k]
	print("native_gate: small map layers.tmx, %d warm async imports: %d to %d frames, wall mean %.2f ms (min %.2f), worst frame before the build %.2f ms, build frame mean %.2f ms" % [
		walls.size(), frames.min(), frames.max(), wall_sum / walls.size(), walls.min(), worst, build_sum / builds.size()])


func engine_floor(script: GDScript) -> void:
	var path := "res://test/out/bench/bench_tiles_only.tmx"
	if not FileAccess.file_exists(path):
		print("native_gate: engine floor skipped (run test/benchmark.gd once to generate its map)")
		return
	var best_import := INF
	var best_assign := INF
	for i in range(4):
		var imp: Object = script.new()
		var t := Time.get_ticks_usec()
		var n: Node = imp.call("import", path, "")
		var import_ms := (Time.get_ticks_usec() - t) / 1000.0
		if n == null:
			return
		var blobs: Array[PackedByteArray] = []
		var ts: TileSet = null
		for l in n.find_children("*", "TileMapLayer", true, false):
			blobs.append((l as TileMapLayer).tile_map_data)
			ts = (l as TileMapLayer).tile_set
		var layers: Array[TileMapLayer] = []
		t = Time.get_ticks_usec()
		for b in blobs:
			var l := TileMapLayer.new()
			l.tile_set = ts
			l.tile_map_data = b
			layers.append(l)
		var assign_ms := (Time.get_ticks_usec() - t) / 1000.0
		for l in layers:
			l.free()
		n.free()
		if i > 0:
			best_import = minf(best_import, import_ms)
			best_assign = minf(best_assign, assign_ms)
	print("native_gate: engine floor, benchmark tiles-only map (6 layers): import %.1f ms, the engine's tile_map_data assignments alone %.1f ms" % [best_import, best_assign])
