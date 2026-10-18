# MIT License
#
# Copyright (c) 2023-2026 Roland Helmerichs
#
# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to deal
# in the Software without restriction, including without limitation the rights
# to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
# copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:
#
# The above copyright notice and this permission notice shall be included in all
# copies or substantial portions of the Software.
#
# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
# OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
# SOFTWARE.

extends RefCounted

const CustomTypes := preload("CustomTypes.gd")
const DataLoader := preload("DataLoader.gd")
const CommonUtils := preload("CommonUtils.gd")
const DictionaryBuilder := preload("DictionaryBuilder.gd")

const WARNING_COLOR = "yellow"
const CUSTOM_DATA_INTERNAL = "__internal__"
const GODOT_ATLAS_ID_PROPERTY = "godot_atlas_id"
const CLASS_INTERNAL = "class"

# Wang id index -> Godot terrain peering bit (index 0 = top side)
const PEERING_BITS: Array[int] = [
	TileSet.CELL_NEIGHBOR_TOP_SIDE,
	TileSet.CELL_NEIGHBOR_TOP_RIGHT_CORNER,
	TileSet.CELL_NEIGHBOR_RIGHT_SIDE,
	TileSet.CELL_NEIGHBOR_BOTTOM_RIGHT_CORNER,
	TileSet.CELL_NEIGHBOR_BOTTOM_SIDE,
	TileSet.CELL_NEIGHBOR_BOTTOM_LEFT_CORNER,
	TileSet.CELL_NEIGHBOR_LEFT_SIDE,
	TileSet.CELL_NEIGHBOR_TOP_LEFT_CORNER
]
const TERRAIN_MODES: Dictionary = {
	"corner": TileSet.TERRAIN_MODE_MATCH_CORNERS,
	"edge": TileSet.TERRAIN_MODE_MATCH_SIDES,
	"mixed": TileSet.TERRAIN_MODE_MATCH_CORNERS_AND_SIDES
}
const CUSTOM_DATA_TYPES: Dictionary = {
	"bool": TYPE_BOOL,
	"int": TYPE_INT,
	"string": TYPE_STRING,
	"float": TYPE_FLOAT,
	"color": TYPE_COLOR,
	"class": TYPE_DICTIONARY
}

var _tileset = null
var _current_atlas_source = null
var _current_max_x = 0
var _current_max_y = 0
var _base_path_map = ""
var _base_path_tileset = ""
var _terrain_sets_counter: int = -1
var _terrain_counter: int = 0
var _tile_count: int = 0
var _columns: int = 0
var _tile_size: Vector2i
var _physics_layer_counter: int = -1
var _navigation_layer_counter: int = -1
var _occlusion_layer_counter: int = -1
var _append = false
var _atlas_sources = null
var _map_tile_size: Vector2i
var _grid_size: Vector2i
var _tile_offset: Vector2i
var _object_alignment
var _object_groups = null
var _object_groups_counter: int = 0
var _tileset_orientation
var _map_wangset_to_terrain: bool = false
var _custom_data_prefix: String
var _ct = null
var _current_first_gid = -1
var _has_transparent_color: bool = false
var _current_shader_material: ShaderMaterial = null
var _godot_version: int = Engine.get_version_info()["hex"]
var _animation_mode: String = "strip"
var _collection_tiles: Dictionary = {}	# tile id -> TileSetAtlasSource of the current image collection tileset
var _pending_strips: Array = []			# animations composed into strip atlases once every tileset is done


enum layer_type {
	PHYSICS,
	NAVIGATION,
	OCCLUSION
}


func set_base_path(source_file: String):
	_base_path_map = source_file.get_base_dir()
	_base_path_tileset = _base_path_map


func set_map_parameters(map_tile_size: Vector2i):
	_map_tile_size = map_tile_size


func set_custom_types(ct):
	_ct = ct


func map_wangset_to_terrain():
	_map_wangset_to_terrain = true


func set_custom_data_prefix(value: String):
	_custom_data_prefix = value


# How tile animations are created: "strip" (each animation gets an atlas of its frames in order) or
# "atlas" (Godot's animation in the tileset's own atlas, where the frame layout allows it)
func set_animation_mode(value: String):
	_animation_mode = value.to_lower()
	if _animation_mode != "strip" and _animation_mode != "atlas":
		CommonUtils.report_warning(" -- Option animation_mode '" + value + "' is not supported. -> Using 'strip'", WARNING_COLOR)
		_animation_mode = "strip"


func create_from_dictionary_array(tileSets: Array):
	for tile_set in tileSets:
		var tile_set_dict = tile_set

		if tile_set.has("source"):
			var source_file: String = tile_set["source"]

			# Catch the AutoMap Rules tileset (is Tiled internal)
			if source_file.begins_with(":/automap"):
				continue # This is no error skip it

			var tiled_file_content = DataLoader.get_tiled_file_content(source_file, _base_path_map)
			if tiled_file_content == null:
				CommonUtils.report_error("ERROR: Tileset file '" + source_file + "' not found. -> Continuing but result may be unusable")
				continue

			_base_path_tileset = _base_path_map.path_join(source_file).get_base_dir()

			tile_set_dict = DictionaryBuilder.new().get_dictionary(tiled_file_content, source_file)
			if tile_set_dict != null and tile_set.has("firstgid"):
				tile_set_dict["firstgid"] = tile_set["firstgid"]

		# Possible error condition
		if tile_set_dict == null:
			CommonUtils.count_error("ERROR: Tileset '" + str(tile_set.get("source", "")) + "' could not be read.")
			continue

		create_or_append(tile_set_dict)
		_append = true

	# After all tilesets: strip atlases get source ids after the tilesets' own, and see the final
	# tile data (collision, properties, terrains) of the tiles they animate
	create_animation_strips()
	return _tileset


func get_registered_atlas_sources():
	return _atlas_sources


func get_registered_object_groups():
	return _object_groups


func create_or_append(tile_set: Dictionary):
	# Catch the AutoMap Rules tileset (is Tiled internal)
	if tile_set.has("name") and str(tile_set["name"]) == "AutoMap Rules":
		return # This is no error just skip it

	if not _append:
		_tileset = TileSet.new()
		_tileset.add_custom_data_layer()
		_tileset.set_custom_data_layer_name(0, CUSTOM_DATA_INTERNAL)
		_tileset.set_custom_data_layer_type(0, TYPE_INT)

	_tile_size = Vector2i(tile_set["tilewidth"], tile_set["tileheight"])
	if not _append:
		_tileset.tile_size = _map_tile_size
	_tile_count = tile_set.get("tilecount", 0)
	_columns = tile_set.get("columns", 0)
	_tileset_orientation = "orthogonal"
	_grid_size = _tile_size
	if tile_set.has("tileoffset"):
		var to = tile_set["tileoffset"]
		_tile_offset = Vector2i(to["x"], to["y"])
	else:
		_tile_offset = Vector2i.ZERO

	_current_first_gid = tile_set.get("firstgid", -1)

	if tile_set.has("grid"):
		var grid = tile_set["grid"]
		if grid.has("orientation"):
			_tileset_orientation = grid["orientation"]
		_grid_size.x = grid.get("width", _tile_size.x)
		_grid_size.y = grid.get("height", _tile_size.y)

	if tile_set.has("objectalignment"):
		_object_alignment = tile_set["objectalignment"]
	else:
		_object_alignment = "unspecified"

	if _append:
		_terrain_counter = 0
	_collection_tiles = {}

	if tile_set.has("transparentcolor"):
		_has_transparent_color = true
		var transparent_color = Color(tile_set["transparentcolor"])
		var r_str = ("%f" % [transparent_color.r]).substr(0, 4)
		var g_str = ("%f" % [transparent_color.g]).substr(0, 4)
		var b_str = ("%f" % [transparent_color.b]).substr(0, 4)
		_current_shader_material = ShaderMaterial.new()
		var shad: Shader = Shader.new()
		shad.set_code("shader_type canvas_item;\n" +
			"const vec3 transparent = vec3(" + r_str + ", " + g_str + ", " + b_str + ");\n\n" +
			"void fragment() {\n" +
			"\tvec4 col = texture(TEXTURE, UV);\n" +
			"\tif (col.r >= transparent.r && col.r <= (transparent.r + 0.01) &&\n" +
			"\t    col.g >= transparent.g && col.g <= (transparent.g + 0.01) &&\n" +
			"\t\tcol.b >= transparent.b && col.b <= (transparent.b + 0.01))\n" +
			"\t\tcol.a=0.0;\n" +
			"\tCOLOR=col;\n" +
			"}")
		_current_shader_material.set_shader(shad)
	else:
		_has_transparent_color = false
		_current_shader_material = null

	if "image" in tile_set:
		_current_atlas_source = TileSetAtlasSource.new()
		var added_source_id: int = _tileset.add_source(_current_atlas_source, get_special_property(tile_set, GODOT_ATLAS_ID_PROPERTY))
		_current_atlas_source.texture_region_size = _tile_size
		if tile_set.has("margin"):
			_current_atlas_source.margins = Vector2i(tile_set["margin"], tile_set["margin"])
		if tile_set.has("spacing"):
			_current_atlas_source.separation = Vector2i(tile_set["spacing"], tile_set["spacing"])

		var texture = DataLoader.load_image(tile_set["image"], _base_path_tileset)
		if not texture:
			# Can't continue without texture
			return;

		_current_atlas_source.texture = texture
		_columns = _current_atlas_source.texture.get_width() / _tile_size.x
		_tile_count = _columns * _current_atlas_source.texture.get_height() / _tile_size.y

		register_atlas_source(added_source_id, _tile_count, -1, _tile_offset)
		var atlas_grid_size = _current_atlas_source.get_atlas_grid_size()
		_current_max_x = atlas_grid_size.x - 1
		_current_max_y = atlas_grid_size.y - 1

	if tile_set.has("tiles"):
		handle_tiles(tile_set["tiles"])
	if tile_set.has("wangsets"):
		if _map_wangset_to_terrain:
			handle_wangsets_old_mapping(tile_set["wangsets"])
		else:
			handle_wangsets(tile_set["wangsets"])

	if _ct != null:
		_ct.merge_custom_properties(tile_set, "tileset")

	if tile_set.has("properties"):
		handle_tileset_properties(tile_set["properties"])

	# Tiled 'class' of the tileset (if several tilesets have one, the last one wins as Godot uses a single TileSet)
	var tileset_class: String = str(tile_set.get("class", ""))
	if tileset_class != "":
		_tileset.set_meta(CLASS_INTERNAL, tileset_class)


func register_atlas_source(source_id: int, num_tiles: int, assigned_tile_id: int, tile_offset: Vector2i):
	if _atlas_sources == null:
		_atlas_sources = []
	var atlas_source_item = {}
	atlas_source_item["sourceId"] = source_id
	atlas_source_item["numTiles"] = num_tiles
	atlas_source_item["assignedId"] = assigned_tile_id
	atlas_source_item["tileOffset"] = tile_offset
	atlas_source_item["tilesetOrientation"] = _tileset_orientation
	atlas_source_item["objectAlignment"] = _object_alignment
	atlas_source_item["firstGid"] = _current_first_gid
	atlas_source_item["shaderMaterial"] = _current_shader_material
	_atlas_sources.push_back(atlas_source_item)


func register_object_group(tile_id: int, object_group: Dictionary):
	if _object_groups == null:
		_object_groups = {}
	_object_groups[tile_id] = object_group


func create_tile_if_not_existing_and_get_tiledata(tile_id: int):
	if tile_id < _tile_count:
		@warning_ignore("integer_division")
		var row = tile_id / _columns
		var col = tile_id % _columns
		var tile_coords = Vector2i(col, row)
		if col > _current_max_x or row > _current_max_y:
			CommonUtils.report_warning(" -- Tile " + str(tile_id) + " at " + str(col) + "," + str(row) + " outside texture range. -> Skipped", WARNING_COLOR)
			return null
		var tile_at_coords = _current_atlas_source.get_tile_at_coords(tile_coords)
		if tile_at_coords == Vector2i(-1, -1):
			_current_atlas_source.create_tile(tile_coords)
		elif tile_at_coords != tile_coords:
			print_rich("[color="+WARNING_COLOR+"]WARNING: tile_at_coords not equal tile_coords![/color]")
			print_rich("[color="+WARNING_COLOR+"]         tile_coords:   " + str(col) + "," + str(row) + "[/color]")
			print_rich("[color="+WARNING_COLOR+"]         tile_at_coords: " + str(tile_at_coords.x) + "," + str(tile_at_coords.x) + "[/color]")
			CommonUtils.report_warning("-> Tile skipped", WARNING_COLOR)
			return null
		return _current_atlas_source.get_tile_data(tile_coords, 0)
	CommonUtils.report_warning(" -- Tile id " + str(tile_id) + " outside tile count range (0-" + str(_tile_count-1) + "). -> Skipped.", WARNING_COLOR)
	return null


func handle_tiles(tiles: Array):
	for tile in tiles:
		var tile_id = tile["id"]
		var tile_class = tile.get("class", "")
		if tile_class == "":
			tile_class = tile.get("type", "")

		var current_tile
		if tile.has("image"):
			# Tile with its own image -> separate atlas source
			var texture_path = tile["image"]
			var ext = texture_path.get_extension().to_lower()
			var tile_texture: Texture2D = null
			if ext == "tmx" or ext == "tmj":
				var placeholder_texture = PlaceholderTexture2D.new()
				var width = tile["imagewidth"]
				var height = tile["imageheight"]
				placeholder_texture.size = Vector2(width, height)
				tile_texture = placeholder_texture
			else:
				tile_texture = DataLoader.load_image(texture_path, _base_path_tileset)
			if tile_texture == null:
				# Error already reported by the loader
				continue

			_current_atlas_source = TileSetAtlasSource.new()
			var added_source_id = _tileset.add_source(_current_atlas_source, get_special_property(tile, GODOT_ATLAS_ID_PROPERTY))
			register_atlas_source(added_source_id, 1, tile_id, _tile_offset)
			_collection_tiles[tile_id] = _current_atlas_source
			_current_atlas_source.texture = tile_texture

			_current_atlas_source.resource_name = texture_path.get_file().get_basename()
			var texture_width = _current_atlas_source.texture.get_width()
			if tile.has("width"):
				texture_width = tile["width"]
			var texture_height = _current_atlas_source.texture.get_height()
			if tile.has("height"):
				texture_height = tile["height"]
			_current_atlas_source.texture_region_size = Vector2i(texture_width, texture_height)
			var tile_offset_x = 0
			if tile.has("x"):
				tile_offset_x = tile["x"]
			var tile_offset_y = 0
			if tile.has("y"):
				tile_offset_y = tile["y"]
			_current_atlas_source.margins = Vector2i(tile_offset_x, tile_offset_y)
			# For single image tiles the tileset's tilesize is misleading, use the texture size
			_tile_size = _current_atlas_source.texture_region_size

			_current_atlas_source.create_tile(Vector2(0, 0))
			current_tile = _current_atlas_source.get_tile_data(Vector2(0, 0), 0)
			current_tile.probability = tile.get("probability", 1.0)
		else:
			current_tile = create_tile_if_not_existing_and_get_tiledata(tile_id)
			if current_tile == null:
				#Error occurred
				continue

		if _tile_size.x != _map_tile_size.x or _tile_size.y != _map_tile_size.y:
			var diff_x = _tile_size.x - _map_tile_size.x
			if diff_x % 2 != 0:
				diff_x -= 1
			var diff_y = _tile_size.y - _map_tile_size.y
			if diff_y % 2 != 0:
				diff_y += 1
			@warning_ignore("integer_division")
			current_tile.texture_origin = Vector2i(-diff_x/2, diff_y/2) - _tile_offset

		if _tile_offset != Vector2i.ZERO and current_tile.texture_origin == Vector2i.ZERO:
			current_tile.texture_origin -= _tile_offset

		if _has_transparent_color:
			current_tile.material = _current_shader_material

		if tile.has("probability"):
			current_tile.probability = tile["probability"]
		if tile.has("animation"):
			handle_animation(tile["animation"], tile_id)
		if tile.has("objectgroup"):
			handle_objectgroup(tile["objectgroup"], current_tile, tile_id)

		if tile_class != "":
			current_tile.set_meta(CLASS_INTERNAL, tile_class)

		if _ct != null:
			_ct.merge_custom_properties(tile, "tile")

		if tile.has("properties"):
			handle_tile_properties(tile["properties"], current_tile)


func handle_animation(frames: Array, tile_id: int) -> void:
	if frames.is_empty(): return
	if _animation_mode == "strip":
		queue_animation_strip(frames, tile_id)
		return
	# "atlas" mode: Godot animates a tile through the atlas cells following it, so the first frame
	# must be the tile itself and the frames equidistant in a row or column
	if _collection_tiles.has(tile_id) or _columns <= 0:
		CommonUtils.report_warning(" -- Animated tile " + str(tile_id) + " of an image collection has no atlas to animate in. -> Using a strip", WARNING_COLOR)
		queue_animation_strip(frames, tile_id)
		return
	if int(frames[0]["tileid"]) != tile_id:
		CommonUtils.report_warning(" -- Animated tile " + str(tile_id) + ": the first frame (tile " + str(frames[0]["tileid"]) + ") is not the tile itself. -> Using a strip", WARNING_COLOR)
		queue_animation_strip(frames, tile_id)
		return

	@warning_ignore("integer_division")
	var tile_coords = Vector2i(tile_id % _columns, tile_id / _columns)
	var max_diff_x = _columns - tile_coords.x
	@warning_ignore("integer_division")
	var max_diff_y = _tile_count / _columns - tile_coords.y
	var separation_vect := Vector2i.ZERO
	var anim_columns: int = 0
	# The whole sequence is validated before the atlas source is touched
	if frames.size() > 1:
		var step: int = int(frames[1]["tileid"]) - tile_id
		var diff_x = step % _columns
		@warning_ignore("integer_division")
		var diff_y = step / _columns
		var valid := false
		if diff_x == 0 and diff_y > 0 and diff_y < max_diff_y:
			separation_vect = Vector2i(0, diff_y - 1)
			anim_columns = 1
			valid = true
		elif diff_y == 0 and diff_x > 0 and diff_x < max_diff_x:
			separation_vect = Vector2i(diff_x - 1, 0)
			anim_columns = 0
			valid = true
		for i in range(1, frames.size()):
			if int(frames[i]["tileid"]) - int(frames[i - 1]["tileid"]) != step:
				valid = false
		if not valid:
			CommonUtils.report_warning(" -- Animated tile " + str(tile_id) + ": Succession of tiles not supported in Godot 4 (use animation_mode 'strip'). -> Skipped", WARNING_COLOR)
			return
	if not _current_atlas_source.has_room_for_tile(tile_coords, Vector2i.ONE, anim_columns, separation_vect, frames.size(), tile_coords):
		CommonUtils.report_warning(" -- TileId " + str(tile_id) + ": Not enough room for all " + str(frames.size()) + " animation frames (use animation_mode 'strip'). -> Skipped", WARNING_COLOR)
		return
	_current_atlas_source.set_tile_animation_separation(tile_coords, separation_vect)
	_current_atlas_source.set_tile_animation_columns(tile_coords, anim_columns)
	_current_atlas_source.set_tile_animation_frames_count(tile_coords, frames.size())
	for i in range(frames.size()):
		_current_atlas_source.set_tile_animation_frame_duration(tile_coords, i, get_frame_duration(frames[i]))


static func get_frame_duration(frame: Dictionary) -> float:
	if frame.has("duration"):
		return float(frame["duration"]) / 1000.0
	return 1.0


# Remembers an animation for create_animation_strips(), with what it needs of the current tileset
func queue_animation_strip(frames: Array, tile_id: int) -> void:
	var item := {}
	item["tileId"] = tile_id
	item["frames"] = frames
	item["collection"] = _collection_tiles		# per tileset: frames may name tiles that come later
	item["atlasSource"] = _current_atlas_source if not _collection_tiles.has(tile_id) else null
	item["columns"] = _columns
	item["tileCount"] = _tile_count
	item["tileOffset"] = _tile_offset
	item["tilesetOrientation"] = _tileset_orientation
	item["objectAlignment"] = _object_alignment
	item["firstGid"] = _current_first_gid
	item["shaderMaterial"] = _current_shader_material
	_pending_strips.append(item)


# The atlas source and coordinates of a tile id of a queued animation's tileset (null if unknown)
static func get_strip_frame_tile(item: Dictionary, tile_id: int):
	var collection: Dictionary = item["collection"]
	if collection.has(tile_id):
		return [collection[tile_id], Vector2i.ZERO]
	var source = item["atlasSource"]
	var columns: int = item["columns"]
	if source == null or columns <= 0 or tile_id < 0 or tile_id >= int(item["tileCount"]):
		return null
	@warning_ignore("integer_division")
	return [source, Vector2i(tile_id % columns, tile_id / columns)]


static func get_rgba_image(texture: Texture2D, cache: Dictionary):
	if cache.has(texture):
		return cache[texture]
	var img: Image = texture.get_image() if texture != null else null
	if img != null:
		if img.is_compressed():
			img.decompress()
		if img.get_format() != Image.FORMAT_RGBA8:
			img.convert(Image.FORMAT_RGBA8)
	cache[texture] = img
	return img


# "strip" animations: each animated tile gets an atlas source holding its frames in a row, in the
# animation's order (any tiles, repeats allowed), one tile animated over them with the frame
# durations. The animated tile's data is copied onto it and gid lookups are redirected to it
# (a registered source with the tile's id as assignedId, matched before the tileset's atlas).
func create_animation_strips() -> void:
	if _tileset == null:
		_pending_strips.clear()
		return
	var images := {}
	for item in _pending_strips:
		var tile_id: int = item["tileId"]
		var frames: Array = item["frames"]
		var base = get_strip_frame_tile(item, tile_id)
		if base == null or not base[0].has_tile(base[1]):
			CommonUtils.report_warning(" -- Animated tile " + str(tile_id) + " not found. -> Animation skipped", WARNING_COLOR)
			continue
		var base_source: TileSetAtlasSource = base[0]
		var regions := []
		var frame_size := Vector2i.ZERO
		var failed := ""
		for frame in frames:
			var ft = get_strip_frame_tile(item, int(frame["tileid"]))
			if ft == null:
				failed = "frame tile " + str(frame["tileid"]) + " does not exist"
				break
			var img = get_rgba_image(ft[0].texture, images)
			if img == null:
				failed = "the image of frame tile " + str(frame["tileid"]) + " can't be read"
				break
			var fs: TileSetAtlasSource = ft[0]
			# Computed: get_tile_texture_region() needs the tile to exist, frames need not
			var region := Rect2i(fs.margins + ft[1] * (fs.texture_region_size + fs.separation), fs.texture_region_size)
			if frame_size == Vector2i.ZERO:
				frame_size = region.size
			elif region.size != frame_size:
				failed = "the frames differ in size"
				break
			regions.append([img, region])
		if failed == "" and frame_size.x <= 0:
			failed = "the frames are empty"
		if failed != "":
			CommonUtils.report_warning(" -- Animated tile " + str(tile_id) + ": " + failed + ". -> Animation skipped", WARNING_COLOR)
			continue

		var strip := Image.create(frame_size.x * frames.size(), frame_size.y, false, Image.FORMAT_RGBA8)
		for i in range(regions.size()):
			strip.blit_rect(regions[i][0], regions[i][1], Vector2i(i * frame_size.x, 0))
		var strip_source := TileSetAtlasSource.new()
		strip_source.texture = ImageTexture.create_from_image(strip)
		strip_source.texture_region_size = frame_size
		var base_name: String = base_source.resource_name
		if base_name == "" and base_source.texture != null:
			base_name = base_source.texture.resource_path.get_file().get_basename()
		strip_source.resource_name = base_name
		var strip_id: int = _tileset.add_source(strip_source)
		strip_source.create_tile(Vector2i.ZERO)
		copy_tile_data(base_source.get_tile_data(base[1], 0), strip_source.get_tile_data(Vector2i.ZERO, 0))
		strip_source.set_tile_animation_columns(Vector2i.ZERO, 0)
		strip_source.set_tile_animation_frames_count(Vector2i.ZERO, frames.size())
		for i in range(frames.size()):
			strip_source.set_tile_animation_frame_duration(Vector2i.ZERO, i, get_frame_duration(frames[i]))

		if _atlas_sources == null:
			_atlas_sources = []
		var atlas_source_item = {}
		atlas_source_item["sourceId"] = strip_id
		atlas_source_item["numTiles"] = 1
		atlas_source_item["assignedId"] = tile_id
		atlas_source_item["tileOffset"] = item["tileOffset"]
		atlas_source_item["tilesetOrientation"] = item["tilesetOrientation"]
		atlas_source_item["objectAlignment"] = item["objectAlignment"]
		atlas_source_item["firstGid"] = item["firstGid"]
		atlas_source_item["shaderMaterial"] = item["shaderMaterial"]
		atlas_source_item["isStrip"] = true
		_atlas_sources.push_back(atlas_source_item)
	_pending_strips.clear()


# Copies everything stored in a TileData (collision, navigation and occlusion polygons, custom data,
# terrains, texture origin, material, probability ...) and its metadata onto another of the same TileSet
static func copy_tile_data(from: TileData, to: TileData) -> void:
	for prop in from.get_property_list():
		if not (prop["usage"] & PROPERTY_USAGE_STORAGE): continue
		var prop_name: String = prop["name"]
		if prop_name == "script" or prop_name.begins_with("metadata/"): continue
		to.set(prop_name, from.get(prop_name))
	for meta_name in from.get_meta_list():
		to.set_meta(meta_name, from.get_meta(meta_name))


func handle_objectgroup(object_group: Dictionary, current_tile: TileData, tile_id: int):

	# v1.2:
	_object_groups_counter += 1
	register_object_group(_object_groups_counter, object_group)
	current_tile.set_custom_data(CUSTOM_DATA_INTERNAL, _object_groups_counter)

	var polygon_indices = {}
	var objects = object_group["objects"] as Array
	for obj in objects:
		if obj.has("point") and obj["point"]:
			# print_rich("[color="+WARNING_COLOR+"] -- 'Point' on tile " + str(tile_id) + " skipped as there is no corresponding element in Godot 4.[/color]")
			# CommonUtils.warning_count += 1
			continue

		if _ct != null:
			_ct.merge_custom_properties(obj, "object")

		var object_base_coords = Vector2(obj["x"], obj["y"])
		object_base_coords = transpose_coords(object_base_coords.x, object_base_coords.y)
		object_base_coords -= Vector2(current_tile.texture_origin)
		if _tileset_orientation == "isometric":
			object_base_coords.y -= _grid_size.y / 2.0
			if _grid_size.y != _tile_size.y:
				object_base_coords.y += (_tile_size.y - _grid_size.y) / 2.0
		else:
			object_base_coords -= Vector2(_tile_size / 2.0)

		var rot = obj.get("rotation", 0.0)
		var sin_a = sin(rot * PI / 180.0)
		var cos_a = cos(rot * PI / 180.0)

		var polygon
		if obj.has("polygon") or obj.has("polyline"):
			var polygon_points = (obj["polygon"] if obj.has("polygon") else obj["polyline"]) as Array
			if polygon_points.size() < 3:
				CommonUtils.report_warning(" -- Skipped invalid polygon on tile " + str(tile_id) + " (less than 3 points)", WARNING_COLOR)
				break
			polygon = []
			for pt in polygon_points:
				var p_coord = transpose_coords(pt["x"], pt["y"])
				var p_coord_rot = Vector2(p_coord.x * cos_a - p_coord.y * sin_a, p_coord.x * sin_a + p_coord.y * cos_a)
				polygon.append(object_base_coords + p_coord_rot)
		elif obj.has("ellipse") and obj["ellipse"]:
			# Approximate ellipse as a polygon (Godot 4 has no native ellipse collision)
			var cx: float = (obj.get("width", 0.0) / 2.0)
			var cy: float = (obj.get("height", 0.0) / 2.0)
			const segments: int = 12
			polygon = []
			for i in range(segments):
				var angle := TAU * float(i) / float(segments)
				var pt := Vector2(cx + cx * cos(angle), cy + cy * sin(angle))
				var pt_trans = transpose_coords(pt.x, pt.y)
				var pt_rot := Vector2(
					pt_trans.x * cos_a - pt_trans.y * sin_a,
					pt_trans.x * sin_a + pt_trans.y * cos_a
				)
				polygon.append(object_base_coords + pt_rot)
		elif obj.has("capsule") and obj["capsule"]:
			# Approximate capsule as a polygon (Godot 4 has no native capsule collision)
			var height: float = obj.get("height", 0.0)
			var width: float = obj.get("width", 0.0)
			var r: float = width / 2.0
			if width > height:
				r = height / 2.0
			const segments: int = 12 # must be dividable by 4!
			polygon = []
			for i in range(segments):
				var angle := TAU * float(i) / float(segments)
				var pt := Vector2(width/2.0 + r * cos(angle), height/2.0 + r * sin(angle))
				polygon.append(pt)
			if abs(width - height) > 0.01:
				polygon.append(Vector2.ZERO)
				polygon.append(Vector2.ZERO)
			if height > width and polygon.size() > segments:
				var y_offs: float = (height - width) / 2.0
				polygon[segments+1].x = polygon[0].x
				polygon[segments+1].y = polygon[0].y - y_offs
				for i in range(segments, segments/2, -1):
					polygon[i].x = polygon[i-1].x
					polygon[i].y = polygon[i-1].y - y_offs
				for i in range(segments/2 + 1):
					polygon[i].y += y_offs
			if width > height and polygon.size() > segments:
				var x_offs: float = (width - height) / 2.0
				for i in range(segments+1, segments*3/4 + 1, -1):
					polygon[i].y = polygon[i-2].y
					polygon[i].x = polygon[i-2].x + x_offs
				for i in range(segments*3/4 + 1, segments/4, -1):
					polygon[i].y = polygon[i-1].y
					polygon[i].x = polygon[i-1].x - x_offs
				for i in range(segments/4 + 1):
					polygon[i].x += x_offs
			for i in range(polygon.size()):
				var pt_trans = transpose_coords(polygon[i].x, polygon[i].y)
				var pt_rot := Vector2(
					pt_trans.x * cos_a - pt_trans.y * sin_a,
					pt_trans.x * sin_a + pt_trans.y * cos_a
				)
				polygon[i] = object_base_coords + pt_rot
		else:
			# Should be a simple rectangle
			polygon = [Vector2(), Vector2(), Vector2(), Vector2()]
			polygon[0] = Vector2.ZERO
			polygon[1].y = polygon[0].y + obj.get("height", 0.0)
			polygon[1].x = polygon[0].x
			polygon[2].y = polygon[1].y
			polygon[2].x = polygon[0].x + obj.get("width", 0.0)
			polygon[3].y = polygon[0].y
			polygon[3].x = polygon[2].x
			var i = 0
			for pt in polygon:
				var pt_trans = transpose_coords(pt.x, pt.y)
				var pt_rot = Vector2(pt_trans.x * cos_a - pt_trans.y * sin_a, pt_trans.x * sin_a + pt_trans.y * cos_a)
				polygon[i] = object_base_coords + pt_rot
				i += 1

		# Special properties (single pass, first matching int property wins)
		var nav: int = -1
		var occ: int = -1
		var phys: int = -1
		if obj.has("properties"):
			for property in obj["properties"]:
				if property.get("type", "string") != "int": continue
				var prop_name: String = str(property.get("name", "")).to_lower()
				if prop_name == "navigation_layer":
					if nav < 0: nav = int(property.get("value", 0))
				elif prop_name == "occlusion_layer":
					if occ < 0: occ = int(property.get("value", 0))
				elif prop_name == "physics_layer":
					if phys < 0: phys = int(property.get("value", 0))

		if nav >= 0:
			var nav_p = NavigationPolygon.new()
			nav_p.add_outline(polygon)
			#nav_p.make_polygons_from_outlines()
			# Replaced in 4.2 deprecated function make_polygons_from_outlines
			nav_p.vertices = polygon
			var pg = PackedInt32Array()
			for idx in range(nav_p.vertices.size()):
				pg.push_back(idx)
			nav_p.add_polygon(pg)

			ensure_layer_existing(layer_type.NAVIGATION, nav)
			current_tile.set_navigation_polygon(nav, nav_p)

		if occ >= 0:
			var occ_p = OccluderPolygon2D.new()
			occ_p.polygon = polygon
			ensure_layer_existing(layer_type.OCCLUSION, occ)
			if _godot_version >= 0x040400:
				current_tile.set_occluder_polygons_count(occ, 1)
				current_tile.set_occluder_polygon(occ, 0, occ_p)
			else:
				current_tile.set_occluder(occ, occ_p)

		# If no property is specified assume collision (i.e. default)
		if phys < 0 and nav < 0 and occ < 0:
			phys = 0
		if phys < 0: continue
		var polygon_index = polygon_indices.get(phys, 0)
		polygon_indices[phys] = polygon_index + 1
		ensure_layer_existing(layer_type.PHYSICS, phys)
		current_tile.add_collision_polygon(phys)
		current_tile.set_collision_polygon_points(phys, polygon_index, polygon)
		if not obj.has("properties"): continue
		for property in obj["properties"]:
			var name = property.get("name", "")
			var type = property.get("type", "string")
			var val = str(property.get("value", ""))
			if name == "": continue
			if name.to_lower() == "one_way" and type == "bool":
				current_tile.set_collision_polygon_one_way(phys, polygon_index, val.to_lower() == "true")
			elif name.to_lower() == "one_way_margin" and type == "int":
				current_tile.set_collision_polygon_one_way_margin(phys, polygon_index, int(val))


func transpose_coords(x: float, y: float):
	if _tileset_orientation == "isometric":
		var trans_x = (x - y) * _grid_size.x / _grid_size.y / 2.0
		var trans_y = (x + y) * 0.5
		return Vector2(trans_x, trans_y)

	return Vector2(x, y)


func get_special_property(dict: Dictionary, property_name: String):
	if not dict.has("properties"): return -1
	for	property in dict["properties"]:
		var name = property.get("name", "")
		var type = property.get("type", "string")
		var val = property.get("value", "")
		if name == "": continue
		if name.to_lower() == property_name and type == "int":
			return int(val)
	return -1


func handle_tile_properties(properties: Array, current_tile: TileData):
	for property in properties:
		var name = property.get("name", "")
		var type = property.get("type", "string")
		var list_val = ""
		var class_val: Dictionary = {}
		var val: String = ""
		if type == "list":
			list_val = []
			for item in property["value"]:
				var item_type: String = item["type"]
				var item_val: String = str(item.get("value", ""))
				list_val.append(CommonUtils.get_right_typed_value(item_type, item_val))
		elif type == "class":
			var raw_class_val = property.get("value", {})
			if raw_class_val is Dictionary:
				class_val = raw_class_val
		else:
			val = str(property.get("value", ""))
		if name == "": continue
		if name.to_lower() == "texture_origin_x" and  type == "int":
			current_tile.texture_origin = Vector2i(int(val), current_tile.texture_origin.y)
		elif name.to_lower() == "texture_origin_y" and  type == "int":
			current_tile.texture_origin = Vector2i(current_tile.texture_origin.x, int(val))
		elif name.to_lower() == "modulate" and type == "color":
			current_tile.modulate = CommonUtils.tiled_color(val)
		elif name.to_lower() == "modulate" and  type == "string":
			current_tile.modulate = Color(val)
		elif name.to_lower() == "material" and  type == "file":
			current_tile.material = DataLoader.load_resource_from_file(val, _base_path_tileset)
		elif name.to_lower() == "z_index" and  type == "int":
			current_tile.z_index = int(val)
		elif name.to_lower() == "y_sort_origin" and  type == "int":
			current_tile.y_sort_origin = int(val)
		elif name.to_lower() == "linear_velocity_x" and (type == "int" or type == "float"):
			ensure_layer_existing(layer_type.PHYSICS, 0)
			var lin_velo = current_tile.get_constant_linear_velocity(0)
			lin_velo.x = float(val)
			current_tile.set_constant_linear_velocity(0, lin_velo)
		elif name.to_lower().begins_with("linear_velocity_x_") and (type == "int" or type == "float"):
			if not name.substr(18).is_valid_int(): continue
			var layer_index = int(name.substr(18))
			ensure_layer_existing(layer_type.PHYSICS, layer_index)
			var lin_velo = current_tile.get_constant_linear_velocity(layer_index)
			lin_velo.x = float(val)
			current_tile.set_constant_linear_velocity(layer_index, lin_velo)
		elif name.to_lower() == "linear_velocity_y" and (type == "int" or type == "float"):
			ensure_layer_existing(layer_type.PHYSICS, 0)
			var lin_velo = current_tile.get_constant_linear_velocity(0)
			lin_velo.y = float(val)
			current_tile.set_constant_linear_velocity(0, lin_velo)
		elif name.to_lower().begins_with("linear_velocity_y_") and (type == "int" or type == "float"):
			if not name.substr(18).is_valid_int(): continue
			var layer_index = int(name.substr(18))
			ensure_layer_existing(layer_type.PHYSICS, layer_index)
			var lin_velo = current_tile.get_constant_linear_velocity(layer_index)
			lin_velo.y = float(val)
			current_tile.set_constant_linear_velocity(layer_index, lin_velo)
		elif name.to_lower() == "angular_velocity" and (type == "int" or type == "float"):
			ensure_layer_existing(layer_type.PHYSICS, 0)
			current_tile.set_constant_angular_velocity(0, float(val))
		elif name.to_lower().begins_with("angular_velocity_") and (type == "int" or type == "float"):
			if not name.substr(17).is_valid_int(): continue
			var layer_index = int(name.substr(17))
			ensure_layer_existing(layer_type.PHYSICS, layer_index)
			current_tile.set_constant_angular_velocity(layer_index, float(val))
		elif name.to_lower() != GODOT_ATLAS_ID_PROPERTY:
			# Properties starting with the custom data prefix become TileSet custom data (prefix stripped),
			# all others become tile metadata. An empty prefix means: every property becomes both.
			var is_custom_data: bool = _custom_data_prefix == "" or name.to_lower().begins_with(_custom_data_prefix)
			if is_custom_data:
				var data_name: String = name.substr(len(_custom_data_prefix))

				var custom_layer = _tileset.get_custom_data_layer_by_name(data_name)
				if custom_layer < 0:
					_tileset.add_custom_data_layer()
					custom_layer = _tileset.get_custom_data_layers_count() - 1
					_tileset.set_custom_data_layer_name(custom_layer, data_name)
					_tileset.set_custom_data_layer_type(custom_layer, CUSTOM_DATA_TYPES.get(type, TYPE_STRING))

				if type == "class":
					current_tile.set_custom_data(data_name, class_val)
				else:
					current_tile.set_custom_data(data_name, CommonUtils.get_right_typed_value(type, val))

			if not is_custom_data or _custom_data_prefix == "":
				if type == "list":
					current_tile.set_meta(name, list_val)
				elif type == "class":
					current_tile.set_meta(name, class_val)
				else:
					current_tile.set_meta(name, CommonUtils.get_right_typed_value(type, val))


func handle_tileset_properties(properties: Array):
	for property in properties:
		var name = property.get("name", "")
		var type = property.get("type", "string")
		var list_val = ""
		var class_val: Dictionary = {}
		var val: String = ""
		if type == "list":
			list_val = []
			for item in property["value"]:
				var item_type: String = item["type"]
				var item_val: String = str(item.get("value", ""))
				list_val.append(CommonUtils.get_right_typed_value(item_type, item_val))
		elif type == "class":
			var raw_class_val = property.get("value", {})
			if raw_class_val is Dictionary:
				class_val = raw_class_val
		else:
			val = str(property.get("value", ""))
		if name == "": continue
		var layer_index
		if name.to_lower() == "collision_layer" and (type == "string" or type == "int"):
			ensure_layer_existing(layer_type.PHYSICS, 0)
			_tileset.set_physics_layer_collision_layer(0, CommonUtils.get_mask_value(property, 32, _ct))
		elif name.to_lower().begins_with("collision_layer_") and (type == "string" or type == "int"):
			if not name.substr(16).is_valid_int(): continue
			layer_index = int(name.substr(16))
			ensure_layer_existing(layer_type.PHYSICS, layer_index)
			_tileset.set_physics_layer_collision_layer(layer_index, CommonUtils.get_mask_value(property, 32, _ct))
		elif name.to_lower() == "collision_mask" and (type == "string" or type == "int"):
			ensure_layer_existing(layer_type.PHYSICS, 0)
			_tileset.set_physics_layer_collision_mask(0, CommonUtils.get_mask_value(property, 32, _ct))
		elif name.to_lower().begins_with("collision_mask_") and (type == "string" or type == "int"):
			if not name.substr(15).is_valid_int(): continue
			layer_index = int(name.substr(15))
			ensure_layer_existing(layer_type.PHYSICS, layer_index)
			_tileset.set_physics_layer_collision_mask(layer_index, CommonUtils.get_mask_value(property, 32, _ct))
		elif name.to_lower() == "layers" and (type == "string" or type == "int"):
			ensure_layer_existing(layer_type.NAVIGATION, 0)
			_tileset.set_navigation_layer_layers(0, CommonUtils.get_mask_value(property, 32, _ct))
		elif name.to_lower().begins_with("layers_") and (type == "string" or type == "int"):
			if not name.substr(7).is_valid_int(): continue
			layer_index = int(name.substr(7))
			ensure_layer_existing(layer_type.NAVIGATION, layer_index)
			_tileset.set_navigation_layer_layers(layer_index, CommonUtils.get_mask_value(property, 32, _ct))
		elif name.to_lower() == "light_mask" and (type == "string" or type == "int"):
			ensure_layer_existing(layer_type.OCCLUSION, 0)
			_tileset.set_occlusion_layer_light_mask(0, CommonUtils.get_mask_value(property, 20, _ct))
		elif name.to_lower().begins_with("light_mask_") and (type == "string" or type == "int"):
			if not name.substr(11).is_valid_int(): continue
			layer_index = int(name.substr(11))
			ensure_layer_existing(layer_type.OCCLUSION, layer_index)
			_tileset.set_occlusion_layer_light_mask(layer_index, CommonUtils.get_mask_value(property, 20, _ct))
		elif name.to_lower() == "sdf_collision_" and type == "bool":
			ensure_layer_existing(layer_type.OCCLUSION, 0)
			_tileset.set_occlusion_layer_sdf_collision(0, val.to_lower() == "true")
		elif name.to_lower().begins_with("sdf_collision_") and type == "bool":
			if not name.substr(14).is_valid_int(): continue
			layer_index = int(name.substr(14))
			ensure_layer_existing(layer_type.OCCLUSION, layer_index)
			_tileset.set_occlusion_layer_sdf_collision(layer_index, val.to_lower() == "true")
		elif name.to_lower() == "uv_clipping" and type == "bool":
			_tileset.uv_clipping = val.to_lower() == "true"
		elif name.to_lower() == "resource_local_to_scene" and type == "bool":
			_tileset.resource_local_to_scene = val.to_lower() == "true"
		elif name.to_lower() == "resource_name" and type == "string":
			_tileset.resource_name = val
		elif name.to_lower() != GODOT_ATLAS_ID_PROPERTY:
			if type == "list":
				_tileset.set_meta(name, list_val)
			elif type == "class":
				_tileset.set_meta(name, class_val)
			else:
				_tileset.set_meta(name, CommonUtils.get_right_typed_value(type, val))


func ensure_layer_existing(tp: layer_type, layer: int):
	match tp:
		layer_type.PHYSICS:
			while _physics_layer_counter < layer:
				_tileset.add_physics_layer()
				_physics_layer_counter += 1
		layer_type.NAVIGATION:
			while _navigation_layer_counter < layer:
				_tileset.add_navigation_layer()
				_navigation_layer_counter += 1
		layer_type.OCCLUSION:
			while _occlusion_layer_counter < layer:
				_tileset.add_occlusion_layer()
				_occlusion_layer_counter += 1


func handle_wangsets_old_mapping(wangsets):
	_tileset.add_terrain_set()
	_terrain_sets_counter += 1
	for wangset in wangsets:
		var current_terrain_set = _terrain_sets_counter
		_tileset.add_terrain(current_terrain_set)
		var current_terrain = _terrain_counter
		if "name" in wangset:
			_tileset.set_terrain_name(current_terrain_set, _terrain_counter, wangset["name"])

		var terrain_mode = TileSet.TERRAIN_MODE_MATCH_CORNERS
		if wangset.has("type"):
			terrain_mode = TERRAIN_MODES.get(wangset["type"], terrain_mode)

		_tileset.set_terrain_set_mode(current_terrain_set, terrain_mode)

		if wangset.has("colors"):
			_tileset.set_terrain_color(current_terrain_set, _terrain_counter, CommonUtils.tiled_color(str(wangset["colors"][0]["color"])))

		if wangset.has("wangtiles"):
			for wangtile in wangset["wangtiles"]:
				var tile_id = wangtile["tileid"]
				var current_tile = create_tile_if_not_existing_and_get_tiledata(tile_id)
				if current_tile == null:
					break

				if _tile_size.x != _map_tile_size.x or _tile_size.y != _map_tile_size.y:
					var diff_x = _tile_size.x - _map_tile_size.x
					if diff_x % 2 != 0:
						diff_x -= 1
					var diff_y = _tile_size.y - _map_tile_size.y
					if diff_y % 2 != 0:
						diff_y += 1
					@warning_ignore("integer_division")
					current_tile.texture_origin = Vector2i(-diff_x/2, diff_y/2) - _tile_offset

				current_tile.terrain_set = current_terrain_set
				current_tile.terrain = current_terrain
				var i = 0
				for wi in wangtile["wangid"]:
					var peering_bit: int = PEERING_BITS[i] if i < PEERING_BITS.size() else TileSet.CELL_NEIGHBOR_TOP_SIDE
					if wi > 0:
						current_tile.set_terrain_peering_bit(peering_bit, current_terrain)
					i += 1

		_terrain_counter += 1


func handle_wangsets(wangsets):
	for wangset in wangsets:
		_tileset.add_terrain_set()
		_terrain_sets_counter += 1
		_terrain_counter = -1
		var current_terrain_set = _terrain_sets_counter

		#var current_terrain = _terrain_counter
		var terrain_set_name = ""
		if "name" in wangset:
			terrain_set_name = wangset["name"]

		var terrain_mode = TileSet.TERRAIN_MODE_MATCH_CORNERS
		if wangset.has("type"):
			terrain_mode = TERRAIN_MODES.get(wangset["type"], terrain_mode)

		_tileset.set_terrain_set_mode(current_terrain_set, terrain_mode)

		if wangset.has("colors"):
			for wangcolor in wangset["colors"]:
				_terrain_counter += 1
				_tileset.add_terrain(current_terrain_set)
				_tileset.set_terrain_color(current_terrain_set, _terrain_counter, CommonUtils.tiled_color(str(wangcolor["color"])))
				var col_name = terrain_set_name
				if wangcolor.has("name"):
					if wangcolor["name"] != "":
						col_name = wangcolor["name"]
				_tileset.set_terrain_name(current_terrain_set, _terrain_counter, col_name)

		if wangset.has("wangtiles"):
			for wangtile in wangset["wangtiles"]:
				var tile_id = wangtile["tileid"]
				var current_tile = create_tile_if_not_existing_and_get_tiledata(tile_id)
				if current_tile == null:
					break

				if _tile_size.x != _map_tile_size.x or _tile_size.y != _map_tile_size.y:
					var diff_x = _tile_size.x - _map_tile_size.x
					if diff_x % 2 != 0:
						diff_x -= 1
					var diff_y = _tile_size.y - _map_tile_size.y
					if diff_y % 2 != 0:
						diff_y += 1
					@warning_ignore("integer_division")
					current_tile.texture_origin = Vector2i(-diff_x/2, diff_y/2) - _tile_offset

				current_tile.terrain_set = current_terrain_set
				var i = 0
				for wi in wangtile["wangid"]:
					var peering_bit: int = PEERING_BITS[i] if i < PEERING_BITS.size() else TileSet.CELL_NEIGHBOR_TOP_SIDE
					if wi > 0:
						current_tile.terrain = wi-1
						current_tile.set_terrain_peering_bit(peering_bit, wi-1)
					i += 1
