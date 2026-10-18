extends SceneTree
## What the *engine* charges for a Tiled map's tile layers, independent of any importer:
## the floor a native importer cannot go below, and the per-chunk costs a streamer has
## to budget (docs/native-importer-plan.md, "Findings").
##
##   godot --headless -s test/bench_materialize.gd
##   xvfb-run -a godot --path . --rendering-driver opengl3 --audio-driver Dummy -s test/bench_materialize.gd
##
## Headless, the frame after a layer enters the tree still builds its physics bodies, but
## no rendering quadrants: run it with a renderer (second form, or on a desktop) for the
## numbers that matter to a game. The report, per tileset variant (tiles without and with
## a collision polygon each):
##   assign      TileMapLayer.tile_map_data = <bytes> for a W×H layer at FILL (the engine
##               parsing the cell blob; set_cell per cell from GDScript for comparison)
##   instantiate PackedScene.instantiate() of that layer (what preloading a map and
##               instancing it costs)
##   next frame  the frame after the layer enters the tree, when the engine builds its
##               rendering quadrants and physics bodies (the hitch)
##   stream      set_cell / erase_cell of 16×16 chunks into the live layer, 1 and 8
##               chunks per frame (the frame's total time, against the idle frame)
##   chunk layer a separate 64×64 TileMapLayer per chunk, one added per frame
##
## User args (after "--"): --size=N (the layer is N×N cells, default 320), --fill=F
## (default 0.85), --no-collision (skip the second variant).

const SOURCE_ID := 7
const CHUNK := 16

var size := 320
var fill := 0.85
var with_collision := true


func _init() -> void:
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--size="):
			size = max(CHUNK, int(arg.substr(7)))
		elif arg.begins_with("--fill="):
			fill = clampf(float(arg.substr(7)), 0.0, 1.0)
		elif arg == "--no-collision":
			with_collision = false
	call_deferred("run")


func run() -> void:
	print("bench_materialize: %dx%d cells at %.0f%% fill, %s, Godot %s" % [size, size, fill * 100.0,
		"headless (no rendering quadrants)" if DisplayServer.get_name() == "headless" else DisplayServer.get_name() + " + " + str(RenderingServer.get_current_rendering_driver_name()),
		Engine.get_version_info()["string"]])
	var variants: Array[bool] = [false]
	if with_collision:
		variants.append(true)
	for collision in variants:
		var ts := make_tileset(collision)
		await settle()
		var idle := await frame_ms()
		print("tiles %s collision: idle frame %.2f ms" % ["with" if collision else "without", idle])

		# One layer the size of the map.
		var cells := cells_for(0, 0, size, size, 1)
		var bytes := bytes_for(cells)
		var layer := TileMapLayer.new()
		layer.tile_set = ts
		var t0 := Time.get_ticks_usec()
		layer.tile_map_data = bytes
		var t1 := Time.get_ticks_usec()
		var by_cell := TileMapLayer.new()
		by_cell.tile_set = ts
		var t2 := Time.get_ticks_usec()
		for c in cells:
			by_cell.set_cell(Vector2i(c.x, c.y), SOURCE_ID, Vector2i(c.z % 8, c.z / 8), 0)
		var t3 := Time.get_ticks_usec()
		by_cell.free()
		var scene := PackedScene.new()
		scene.pack(layer)
		var t4 := Time.get_ticks_usec()
		var copy := scene.instantiate()
		var t5 := Time.get_ticks_usec()
		copy.free()
		print("  %d cells: assign tile_map_data %.1f ms | set_cell per cell %.1f ms | PackedScene.instantiate %.1f ms" % [
			cells.size(), (t1 - t0) / 1000.0, (t3 - t2) / 1000.0, (t5 - t4) / 1000.0])
		var t6 := Time.get_ticks_usec()
		root.add_child(layer)
		var t7 := Time.get_ticks_usec()
		var f1 := await frame_ms()
		var f2 := await frame_ms()
		print("  enter tree: add_child %.1f ms, next frame %.1f ms (the engine's quadrant and body build), then %.1f ms" % [
			(t7 - t6) / 1000.0, f1, f2])
		var live := await frame_ms()
		print("  idle frame with the layer alive: %.2f ms" % live)

		# Streaming into the live layer: new chunks beside it.
		var chunks: Array[Array] = []
		var columns := 10
		for i in 100:
			chunks.append(cells_for(size + (i % columns) * CHUNK, (i / columns) * CHUNK, CHUNK, CHUNK, 100 + i))
		var one := await stream(layer, chunks.slice(0, 50), 1, false)
		var eight := await stream(layer, chunks.slice(50, 100), 8, false)
		var erase := await stream(layer, chunks, 8, true)
		print("  stream set_cell into the live layer, %d-cell chunks: 1 chunk/frame avg %.2f ms (worst %.2f) | 8 chunks/frame avg %.2f ms (worst %.2f) | erase 8/frame avg %.2f ms (worst %.2f); idle was %.2f" % [
			chunks[0].size(), one[0], one[1], eight[0], eight[1], erase[0], erase[1], live])
		layer.queue_free()
		await settle()

		# A TileMapLayer per 64×64 chunk, one added per frame.
		var holder := Node2D.new()
		root.add_child(holder)
		var worst := 0.0
		var total := 0.0
		var count := 0
		var per_side := maxi(1, size / 64)
		for cy in per_side:
			for cx in per_side:
				var b := bytes_for(cells_for(cx * 64, cy * 64, 64, 64, cy * per_side + cx))
				var ta := Time.get_ticks_usec()
				var chunk_layer := TileMapLayer.new()
				chunk_layer.tile_set = ts
				chunk_layer.tile_map_data = b
				holder.add_child(chunk_layer)
				var tb := Time.get_ticks_usec()
				var cost := (tb - ta) / 1000.0 + await frame_ms()
				worst = maxf(worst, cost)
				total += cost
				count += 1
		print("  one 64x64 TileMapLayer per chunk (%d cells), one added per frame: avg frame %.2f ms, worst %.2f ms; idle with %d alive %.2f ms" % [
			(holder.get_child(0) as TileMapLayer).get_used_cells().size(), total / count, worst, count, await frame_ms()])
		holder.queue_free()
		await settle()
	quit(0)


## set_cell (or erase_cell) `per_frame` chunks per frame into `layer`: [average frame ms, worst].
func stream(layer: TileMapLayer, chunks: Array[Array], per_frame: int, erase: bool) -> Array[float]:
	var worst := 0.0
	var total := 0.0
	var count := 0
	for i in range(0, chunks.size(), per_frame):
		var ta := Time.get_ticks_usec()
		for j in mini(per_frame, chunks.size() - i):
			for c: Vector3i in chunks[i + j]:
				if erase:
					layer.erase_cell(Vector2i(c.x, c.y))
				else:
					layer.set_cell(Vector2i(c.x, c.y), SOURCE_ID, Vector2i(c.z % 8, c.z / 8), 0)
		var tb := Time.get_ticks_usec()
		var cost := (tb - ta) / 1000.0 + await frame_ms()
		worst = maxf(worst, cost)
		total += cost
		count += 1
	return [total / count, worst]


## An 8×8 atlas of 16 px tiles, each with a square collision polygon when asked.
func make_tileset(collision: bool) -> TileSet:
	var ts := TileSet.new()
	ts.tile_size = Vector2i(16, 16)
	if collision:
		ts.add_physics_layer()
	var source := TileSetAtlasSource.new()
	var image := Image.create(128, 128, false, Image.FORMAT_RGBA8)
	image.fill(Color(0.5, 0.5, 0.5))
	source.texture = ImageTexture.create_from_image(image)
	source.texture_region_size = Vector2i(16, 16)
	ts.add_source(source, SOURCE_ID)
	for y in 8:
		for x in 8:
			source.create_tile(Vector2i(x, y))
			if collision:
				var data := source.get_tile_data(Vector2i(x, y), 0)
				data.add_collision_polygon(0)
				data.set_collision_polygon_points(0, 0, PackedVector2Array([
					Vector2(-8, -8), Vector2(8, -8), Vector2(8, 8), Vector2(-8, 8)]))
	return ts


## Random cells (x, y, tile index 0..63) over a w×h rect at `fill`.
func cells_for(x0: int, y0: int, w: int, h: int, seed: int) -> Array[Vector3i]:
	var rng := RandomNumberGenerator.new()
	rng.seed = seed
	var out: Array[Vector3i] = []
	for y in h:
		for x in w:
			if rng.randf() < fill:
				out.append(Vector3i(x0 + x, y0 + y, rng.randi_range(0, 63)))
	return out


## TileMapLayer.tile_map_data: u16 format version 0, then per cell six int16:
## x, y, source id, atlas x, atlas y, alternative (transform flags live in bits 12..14).
static func bytes_for(cells: Array[Vector3i]) -> PackedByteArray:
	var bytes := PackedByteArray()
	bytes.resize(2 + cells.size() * 12)
	bytes.encode_u16(0, 0)
	var at := 2
	for c in cells:
		bytes.encode_s16(at, c.x)
		bytes.encode_s16(at + 2, c.y)
		bytes.encode_s16(at + 4, SOURCE_ID)
		bytes.encode_s16(at + 6, c.z % 8)
		bytes.encode_s16(at + 8, c.z / 8)
		bytes.encode_s16(at + 10, 0)
		at += 12
	return bytes


func frame_ms() -> float:
	var t := Time.get_ticks_usec()
	await process_frame
	return (Time.get_ticks_usec() - t) / 1000.0


func settle() -> void:
	for i in 3:
		await process_frame
