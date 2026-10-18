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
const TilesetCreator := preload("TilesetCreator.gd")

const FLIPPED_HORIZONTALLY_FLAG = 0x80000000
const FLIPPED_VERTICALLY_FLAG = 0x40000000
const FLIPPED_DIAGONALLY_FLAG = 0x20000000

const BACKGROUND_COLOR_RECT_NAME = "Background Color"
const WARNING_COLOR = "Yellow"
const CUSTOM_DATA_INTERNAL = "__internal__"
const CLASS_INTERNAL = "class"
const GODOT_NODE_TYPE_PROPERTY = "godot_node_type"
const GODOT_GROUP_PROPERTY = "godot_group"
const GODOT_SCRIPT_PROPERTY = "godot_script"
const DEFAULT_ALIGNMENT = "unspecified"

# Unit offsets (multiplied with (width, height)) for the different object alignments
const ALIGNMENT_UNIT_OFFSETS = {
	"bottomleft": Vector2(0.5, -0.5),
	"bottom": Vector2(0.0, -0.5),
	"bottomright": Vector2(-0.5, -0.5),
	"left": Vector2(0.5, 0.0),
	"center": Vector2(0.0, 0.0),
	"right": Vector2(-0.5, 0.0),
	"topleft": Vector2(0.5, 0.5),
	"top": Vector2(0.0, 0.5),
	"topright": Vector2(-0.5, 0.5),
}
const META_TYPE_TO_PROPERTY_TYPE = {
	TYPE_BOOL: "bool",
	TYPE_INT: "int",
	TYPE_STRING: "string",
	TYPE_FLOAT: "float",
	TYPE_COLOR: "color"
}
const FORCED_FILE_PROPERTIES = ["godot_script", "material", "physics_material_override"]
const ONE_WAY_PROPERTY_KEYS = ["one_way:bool", "one_way_margin:int", "one_way_margin:float"]
const TILE_INSTANCE_PROPERTY_KEYS = ["res_path:file", "res_alignment:string"]

var _map_orientation: String
var _map_width: int = 0
var _map_height: int = 0
var _map_tile_width: int = 0
var _map_tile_height: int = 0
var _infinite = false
var _parallax_origin_x: float = 0.0
var _parallax_origin_y: float = 0.0
var _background_color = ""

var _tilemap_layer = null
var _tileset = null
var _current_tileset_orientation: String
var _current_object_alignment: String
var _base_node = null
var _parallax_background = null
var _background = null
var _parallax_layer_existing = false

var _base_path = ""
var _base_name = ""
var _encoding = ""
var _compression = ""
var _first_gids = []
var _atlas_sources = null
var _use_default_filter = false
var _map_wangset_to_terrain = false
var _add_class_as_metadata = false
var _add_id_as_metadata = false
var _dont_use_alternative_tiles = false
var _custom_data_prefix: String = ""
var _tileset_save_path: String = ""
var _object_groups
var _parallax_mode: String = "parallax2d"
var _animation_mode: String = "strip"
var _always_wrap_root: bool = false
var _parallax_scale := Vector2.ONE			# parallax2d mode: product of the enclosing groups' parallax factors
var _unknown_class_fallback: String = "staticbody"
var _unknown_class_type = _godot_type.BODY
var _reported_unknown_classes: Dictionary = {}	# class names already warned about in this import
var _ct: CustomTypes = null
var _base_dictionary: Dictionary

var _iso_rot: float = 0.0
var _iso_skew: float = 0.0
var _iso_scale: Vector2

var _godot_version: int

# Per-import caches (reset in create())
var _atlas_index_cache: Dictionary = {}		# gid -> index into _atlas_sources (or -1)
var _first_gid_index_cache: Dictionary = {}	# gid -> index into _first_gids
var _cell_cache: Dictionary = {}			# raw cell value (incl. flip flags) -> [source_id, atlas_coords, alt_id] or [] (skip)
var _known_tiles: Dictionary = {}			# Vector3i / Vector4i keys of tiles/alternatives known to exist in _tileset
var _template_cache: Dictionary = {}		# template full path -> {"dict": Dictionary, "tileset": TileSet, "missing": bool}
var _objects_by_id = null					# lazily built: object id -> object dictionary

enum _godot_type {
	EMPTY,
	BODY,
	CBODY,
	RBODY,
	ABODY,
	AREA,
	NAVIGATION,
	OCCLUDER,
	LINE,
	PATH,
	POLYGON,
	INSTANCE,
	PARALLAX,
	NODE2D,
	UNKNOWN
}

const COLLISION_OBJECT_TYPES = [_godot_type.BODY, _godot_type.ABODY, _godot_type.AREA, _godot_type.CBODY, _godot_type.RBODY]

const GODOT_TYPE_MAP = {
	"": _godot_type.EMPTY,
	"collision": _godot_type.BODY,
	"staticbody": _godot_type.BODY,
	"characterbody": _godot_type.CBODY,
	"rigidbody": _godot_type.RBODY,
	"animatablebody": _godot_type.ABODY,
	"area": _godot_type.AREA,
	"navigation": _godot_type.NAVIGATION,
	"occluder": _godot_type.OCCLUDER,
	"line": _godot_type.LINE,
	"path": _godot_type.PATH,
	"polygon": _godot_type.POLYGON,
	"instance": _godot_type.INSTANCE,
	"parallax": _godot_type.PARALLAX
}


func custom_compare(a: Dictionary, b: Dictionary):
	return a["sourceId"] < b["sourceId"]


func set_use_default_filter(value: bool):
	_use_default_filter = value


func set_add_class_as_metadata(value: bool):
	_add_class_as_metadata = value


func set_add_id_as_metadata(value: bool):
	_add_id_as_metadata = value
	

func set_no_alternative_tiles(value: bool):
	_dont_use_alternative_tiles = value


func set_map_wangset_to_terrain(value: bool):
	_map_wangset_to_terrain = value


func set_custom_data_prefix(value: String):
	_custom_data_prefix = value


func set_custom_types(ct: CustomTypes):
	_ct = ct


func set_save_tileset_to(path: String):
	_tileset_save_path = path


# How layers with a parallax factor are created: "parallax2d" (a Parallax2D at the layer's place)
# or "legacy" (ParallaxLayers in one ParallaxBackground, as in 3.0)
func set_parallax_mode(value: String):
	_parallax_mode = value.to_lower()
	if _parallax_mode != "parallax2d" and _parallax_mode != "legacy":
		print_rich("[color=" + WARNING_COLOR + "] -- Option parallax_mode '" + value + "' is not supported. -> Using 'parallax2d'[/color]")
		CommonUtils.warning_count += 1
		_parallax_mode = "parallax2d"


# A Node2D root also for single-layer maps (otherwise the layer itself is returned)
func set_always_wrap_root(value: bool):
	_always_wrap_root = value


# How tile animations are created: "strip" or "atlas" (see TilesetCreator.set_animation_mode)
func set_animation_mode(value: String):
	_animation_mode = value


# What objects of an unknown class become: a GODOT_TYPE_MAP key (except instance/parallax) or "node2d"
func set_unknown_class_fallback(value: String):
	_unknown_class_fallback = value.to_lower()
	if _unknown_class_fallback == "node2d":
		_unknown_class_type = _godot_type.NODE2D
		return
	var t = get_godot_type(_unknown_class_fallback)
	if t == _godot_type.UNKNOWN or t == _godot_type.INSTANCE or t == _godot_type.PARALLAX:
		print_rich("[color=" + WARNING_COLOR + "] -- Option unknown_class_fallback '" + value + "' is not supported. -> Using 'staticbody'[/color]")
		CommonUtils.warning_count += 1
		_unknown_class_fallback = "staticbody"
		t = _godot_type.BODY
	_unknown_class_type = t


func get_tileset():
	return _tileset


static func recursively_change_owner(node: Node, new_owner: Node):
	if node != new_owner:
		node.owner = new_owner
	if node.get_child_count() <= 0: return
	for child in node.get_children():
		recursively_change_owner(child, new_owner)


func reset_import_state() -> void:
	_first_gids = []
	_atlas_sources = null
	_object_groups = null
	_tileset = null
	_background = null
	_parallax_layer_existing = false
	_parallax_scale = Vector2.ONE
	_atlas_index_cache.clear()
	_first_gid_index_cache.clear()
	_cell_cache.clear()
	_known_tiles.clear()
	_template_cache.clear()
	_objects_by_id = null
	_reported_unknown_classes.clear()


func create(source_file: String):
	_godot_version = Engine.get_version_info()["hex"]
	reset_import_state()
	_base_path = source_file.get_base_dir()
	var map_content = DataLoader.get_tiled_file_content(source_file, _base_path)
	if map_content == null:
		printerr("FATAL ERROR: Tiled map file '" + source_file + "' not found.")
		return null
	_base_dictionary = DictionaryBuilder.new().get_dictionary(map_content, source_file)
	_map_orientation = _base_dictionary.get("orientation", "othogonal")
	_map_width = _base_dictionary.get("width", 0)
	_map_height = _base_dictionary.get("height", 0)
	_map_tile_width = _base_dictionary.get("tilewidth", 0)
	_map_tile_height = _base_dictionary.get("tileheight", 0)
	_infinite = _base_dictionary.get("infinite", false)
	_parallax_origin_x = float(_base_dictionary.get("parallaxoriginx", 0.0))
	_parallax_origin_y = float(_base_dictionary.get("parallaxoriginy", 0.0))
	_background_color = _base_dictionary.get("backgroundcolor", "")

	if _ct != null:
		_ct.merge_custom_properties(_base_dictionary, "map")

	if _base_dictionary.has("tilesets"):
		var tilesets = _base_dictionary["tilesets"]
		for tileSet in tilesets:
			_first_gids.append(int(tileSet["firstgid"]))
		var tileset_creator = TilesetCreator.new()
		tileset_creator.set_base_path(source_file)
		tileset_creator.set_map_parameters(Vector2i(_map_tile_width, _map_tile_height))
		if _ct != null:
			tileset_creator.set_custom_types(_ct)
		if _map_wangset_to_terrain:
			tileset_creator.map_wangset_to_terrain()
		tileset_creator.set_custom_data_prefix(_custom_data_prefix)
		tileset_creator.set_animation_mode(_animation_mode)
		_tileset = tileset_creator.create_from_dictionary_array(tilesets)
		_atlas_sources = tileset_creator.get_registered_atlas_sources()
		if _atlas_sources != null:
			_atlas_sources.sort_custom(custom_compare)
		_object_groups = tileset_creator.get_registered_object_groups()
	# gid lookups depend on _atlas_sources / _first_gids, which are final from here on
	_atlas_index_cache.clear()
	_first_gid_index_cache.clear()
	if _tileset == null:
		# If tileset still null create an empty one
		_tileset = TileSet.new()
	_tileset.tile_size = Vector2i(_map_tile_width, _map_tile_height)
	match _map_orientation:
		"isometric":
			_tileset.tile_shape = TileSet.TILE_SHAPE_ISOMETRIC
			_tileset.tile_layout = TileSet.TILE_LAYOUT_DIAMOND_DOWN
		"staggered":
			var stagger_axis = _base_dictionary.get("staggeraxis", "y")
			var stagger_index = _base_dictionary.get("staggerindex", "odd")
			_tileset.tile_shape = TileSet.TILE_SHAPE_ISOMETRIC
			_tileset.tile_layout = TileSet.TILE_LAYOUT_STACKED if stagger_index == "odd" else TileSet.TILE_LAYOUT_STACKED_OFFSET
			_tileset.tile_offset_axis = TileSet.TILE_OFFSET_AXIS_VERTICAL if stagger_axis == "x" else TileSet.TILE_OFFSET_AXIS_HORIZONTAL
		"hexagonal":
			var stagger_axis = _base_dictionary.get("staggeraxis", "y")
			var stagger_index = _base_dictionary.get("staggerindex", "odd")
			_tileset.tile_shape = TileSet.TILE_SHAPE_HEXAGON
			_tileset.tile_layout = TileSet.TILE_LAYOUT_STACKED if stagger_index == "odd" else TileSet.TILE_LAYOUT_STACKED_OFFSET
			_tileset.tile_offset_axis = TileSet.TILE_OFFSET_AXIS_VERTICAL if stagger_axis == "x" else TileSet.TILE_OFFSET_AXIS_HORIZONTAL
	
	_base_node = Node2D.new()
	_base_name = source_file.get_file().get_basename()
	_base_node.name = _base_name
	_parallax_background = null
	if _parallax_mode == "legacy":
		_parallax_background = ParallaxBackground.new()
		_base_node.add_child(_parallax_background)
		_parallax_background.name = _base_name + " (PBG)"
		_parallax_background.owner = _base_node
	if _background_color != "":
		_background = ColorRect.new()
		_background.color = CommonUtils.tiled_color(_background_color)
		_background.size = Vector2(_map_width * _map_tile_width, _map_height * _map_tile_height)
		_base_node.add_child(_background)
		_background.name = BACKGROUND_COLOR_RECT_NAME
		_background.owner = _base_node
	
	if _base_dictionary.has("layers"):
		for layer in _base_dictionary["layers"]:
			handle_layer(layer, _base_node)

	if _tileset_save_path != "":
		var save_ret = ResourceSaver.save(_tileset, _tileset_save_path)
		if save_ret == OK:
			print("Successfully saved tileset to '" + _tileset_save_path + "'")
			for node in _base_node.find_children("*", "TileMapLayer"):
				if node is TileMapLayer and node.tile_set.resource_path == "":
					node.tile_set = ResourceLoader.load(_tileset_save_path)
		else:
			printerr("Saving tileset returned error " + str(save_ret))
			CommonUtils.error_count += 1


	if _base_dictionary.has("properties"):
		handle_properties(_base_node, _base_dictionary["properties"])

	if _parallax_background != null and _parallax_background.get_child_count() == 0:
		_base_node.remove_child(_parallax_background)
		# Nothing is inside a scene tree yet, so free immediately
		_parallax_background.free()
	_parallax_background = null

	# Remove internal helper custom data
	if _tileset.get_custom_data_layers_count() > 0:
		_tileset.remove_custom_data_layer(0)

	# Release per-import caches (they hold references to dictionaries and tilesets)
	_cell_cache.clear()
	_known_tiles.clear()
	_template_cache.clear()
	_objects_by_id = null

	if _base_node.get_child_count() != 1 or _always_wrap_root:
		var base = _base_node
		_base_node = null
		return base

	# Single child: return it parentless and free the wrapper node
	var ret = _base_node.get_child(0)
	_base_node.remove_child(ret)
	recursively_change_owner(ret, ret)
	if _base_dictionary.has("properties"):
		handle_properties(ret, _base_dictionary["properties"])
	ret.name = _base_name
	_base_node.free()
	_base_node = null
	_background = null
	return ret


func handle_layer(layer: Dictionary, parent: Node2D):
	var layer_offset_x = layer.get("offsetx", 0)
	var layer_offset_y = layer.get("offsety", 0)
	var layer_opacity = layer.get("opacity", 1.0)
	var layer_visible = layer.get("visible", true)
	_encoding = layer.get("encoding", "csv")
	_compression = layer.get("compression", "")
	var layer_type = layer.get("type", "tilelayer")
	var tint_color = layer.get("tintcolor", "#ffffff")

	if _ct != null:
		_ct.merge_custom_properties(layer, "layer")

	# v1.2: Skip layer
	if str(get_property(layer, "no_import", "bool")) == "true":
		return

	if layer_type == "tilelayer":
		if _map_orientation == "isometric":
			layer_offset_x += _map_tile_width * (_map_height / 2.0 - 0.5)
		var layer_name = str(layer["name"])
		_tilemap_layer = TileMapLayer.new()
		if layer_name != "":
			_tilemap_layer.name = layer_name
		_tilemap_layer.visible = layer_visible
		_tilemap_layer.position = Vector2(layer_offset_x, layer_offset_y)
		if layer_opacity < 1.0 or tint_color != "#ffffff":
			_tilemap_layer.modulate = layer_color(tint_color, layer_opacity)
		_tilemap_layer.tile_set = _tileset
		handle_parallaxes(parent, _tilemap_layer, layer)
		if _map_orientation == "isometric" or _map_orientation == "staggered":
			_tilemap_layer.y_sort_enabled = true
		apply_blend_mode(_tilemap_layer, layer)

		if not _use_default_filter:
			_tilemap_layer.texture_filter = CanvasItem.TEXTURE_FILTER_NEAREST
		
		if _infinite and layer.has("chunks"):
			# Chunks
			for chunk in layer["chunks"]:
				var offset_x = int(chunk["x"])
				var offset_y = int(chunk["y"])
				var chunk_width = int(chunk["width"])
				var chunk_height = int(chunk["height"])
				var chunk_data = handle_data(chunk["data"], chunk_width * chunk_height)
				if chunk_data != null:
					create_map_from_data(chunk_data, offset_x, offset_y, chunk_width)

		elif layer.has("data"):
			# Data
			var data = handle_data(layer["data"], _map_width * _map_height)
			if data != null:
				create_map_from_data(data, 0, 0, _map_width)

		set_layer_class_meta(_tilemap_layer, layer)
		var obj_id = layer.get("id", 0)
		if _add_id_as_metadata and obj_id != 0:
			_tilemap_layer.set_meta("id", obj_id)

		if layer.has("properties"):
			handle_properties(_tilemap_layer, layer["properties"])

	elif layer_type == "objectgroup":
		var node_type = get_godot_node_type(layer, true)

		var layer_node
		if node_type == _godot_type.PARALLAX:
			layer_node = Parallax2D.new()
			parent.add_child(layer_node)
			layer_node.owner = _base_node
			if _parallax_mode != "legacy":
				configure_parallax2d(layer_node, layer)
			elif layer.has("parallaxx") or layer.has("parallaxy"):
				var par_x = layer.get("parallaxx", 1.0)
				var par_y = layer.get("parallaxy", 1.0)
				layer_node.scroll_scale = Vector2(par_x, par_y)
		else:
			layer_node = Node2D.new()
			handle_parallaxes(parent, layer_node, layer)
		
		if "name" in layer:
			layer_node.name = layer["name"]
		if layer_opacity < 1.0 or tint_color != "#ffffff":
			layer_node.modulate = layer_color(tint_color, layer_opacity)
		layer_node.visible = layer.get("visible", true)
		var layer_pos_x = layer.get("x", 0.0)
		var layer_pos_y = layer.get("y", 0.0)
		layer_node.position = Vector2(layer_pos_x + layer_offset_x, layer_pos_y + layer_offset_y)
		if layer_node is Parallax2D and _parallax_mode != "legacy":
			# A Parallax2D sets its own position, so the layer offset goes into its scroll_offset
			layer_node.scroll_offset += layer_node.position
		if not _use_default_filter:
			layer_node.texture_filter = CanvasItem.TEXTURE_FILTER_NEAREST
		# Tiled's default draw order "topdown" sorts objects by y, "index" keeps the object order
		var draw_order := str(layer.get("draworder", "topdown"))
		if _map_orientation == "isometric" or _map_orientation == "staggered" or draw_order != "index":
			layer_node.y_sort_enabled = true
		apply_blend_mode(layer_node, layer)

		if layer.has("objects"):
			for obj in layer["objects"]:
				handle_object(obj, layer_node, _tileset, Vector2.ZERO)

		set_layer_class_meta(layer_node, layer)
		if layer.has("properties"):
			handle_properties(layer_node, layer["properties"])

	elif layer_type == "group":
		var node_type = get_godot_node_type(layer, true)

		var group_node
		if node_type == _godot_type.PARALLAX:
			group_node = Parallax2D.new()
			parent.add_child(group_node)
			group_node.owner = _base_node
			if _parallax_mode != "legacy":
				configure_parallax2d(group_node, layer)
			elif layer.has("parallaxx") or layer.has("parallaxy"):
				var par_x = layer.get("parallaxx", 1.0)
				var par_y = layer.get("parallaxy", 1.0)
				group_node.scroll_scale = Vector2(par_x, par_y)
		else:
			group_node = Node2D.new()
			handle_parallaxes(parent, group_node, layer)

		group_node.name = layer.get("name", "group")
		if layer_opacity < 1.0 or tint_color != "#ffffff":
			group_node.modulate = layer_color(tint_color, layer_opacity)
		group_node.visible = layer.get("visible", true)
		var layer_pos_x = layer.get("x", 0.0)
		var layer_pos_y = layer.get("y", 0.0)
		group_node.position = Vector2(layer_pos_x + layer_offset_x, layer_pos_y + layer_offset_y)
		if group_node is Parallax2D and _parallax_mode != "legacy":
			group_node.scroll_offset += group_node.position
		apply_blend_mode(group_node, layer)

		# Tiled multiplies the parallax factors of nested layers (see configure_parallax2d)
		var enclosing_parallax_scale := _parallax_scale
		if _parallax_mode != "legacy":
			_parallax_scale *= get_parallax_factor(layer)
		if layer.has("layers"):
			for child_layer in layer["layers"]:
				handle_layer(child_layer, group_node)
		_parallax_scale = enclosing_parallax_scale

		# A material on a plain Node2D does not affect its children: make the group's
		# blend mode reach every descendant that has no material of its own.
		if group_node.material != null:
			use_parent_material_recursively(group_node)

		set_layer_class_meta(group_node, layer)
		if layer.has("properties"):
			handle_properties(group_node, layer["properties"])

	elif layer_type == "imagelayer":
		var node_type = get_godot_node_type(layer, true)

		if node_type == _godot_type.PARALLAX:
			var px_2d = Parallax2D.new()
			parent.add_child(px_2d)
			px_2d.owner = _base_node
			px_2d.name = layer.get("name", "parallax")
			if _parallax_mode != "legacy":
				configure_parallax2d(px_2d, layer)
			elif layer.has("parallaxx") or layer.has("parallaxy"):
				var par_x = layer.get("parallaxx", 1.0)
				var par_y = layer.get("parallaxy", 1.0)
				px_2d.scroll_scale = Vector2(par_x, par_y)
			if layer.has("repeatx") and layer["repeatx"] == 1:
				var repeat_size_x = layer.get("imagewidth", 0)
				if repeat_size_x > 0:
					px_2d.repeat_size = Vector2(repeat_size_x, px_2d.repeat_size.y)
					px_2d.repeat_times = 2
			if layer.has("repeaty") and layer["repeaty"] == 1:
				var repeat_size_y = layer.get("imageheight", 0)
				if repeat_size_y > 0:
					px_2d.repeat_size = Vector2(px_2d.repeat_size.x, repeat_size_y)
					px_2d.repeat_times = 2
			var texture_rect = TextureRect.new()
			texture_rect.name = layer["image"].get_file().get_basename()
			texture_rect.position = Vector2(layer_offset_x, layer_offset_y)
			var imagewidth = layer.get("imagewidth", 0)
			var imageheight = layer.get("imageheight", 0)
			texture_rect.size = Vector2(imagewidth, imageheight)
			if layer_opacity < 1.0 or tint_color != "#ffffff":
				texture_rect.modulate = layer_color(tint_color, layer_opacity)
			texture_rect.visible = layer_visible
			texture_rect.texture = DataLoader.load_image(layer["image"], _base_path)
			apply_blend_mode(texture_rect, layer)

			px_2d.add_child(texture_rect)
			texture_rect.owner = _base_node

			set_layer_class_meta(px_2d, layer)
			if layer.has("properties"):
				handle_properties(px_2d, layer["properties"])
		else:
			var texture_rect = TextureRect.new()
			handle_parallaxes(parent, texture_rect, layer)

			texture_rect.name = layer.get("name", "image")
			texture_rect.position = Vector2(layer_offset_x, layer_offset_y)

			var imagewidth = layer.get("imagewidth", 0)
			var imageheight = layer.get("imageheight", 0)
			texture_rect.size = Vector2(imagewidth, imageheight)
			if layer_opacity < 1.0 or tint_color != "#ffffff":
				texture_rect.modulate = layer_color(tint_color, layer_opacity)
			texture_rect.visible = layer_visible
			texture_rect.texture = DataLoader.load_image(layer["image"], _base_path)
			apply_blend_mode(texture_rect, layer)

			# Repeated image (only if not handled by a ParallaxLayer's motion_mirroring or a Parallax2D's repeat_size)
			if not layer.has("parallaxx") and not layer.has("parallaxy"):
				var repeat_x: bool = int(layer.get("repeatx", 0)) == 1 and imagewidth > 0
				var repeat_y: bool = int(layer.get("repeaty", 0)) == 1 and imageheight > 0
				if repeat_x or repeat_y:
					texture_rect.stretch_mode = TextureRect.STRETCH_TILE
					texture_rect.texture_repeat = CanvasItem.TEXTURE_REPEAT_ENABLED
					var rect_pos: Vector2 = texture_rect.position
					var rect_size: Vector2 = texture_rect.size
					if repeat_x:
						var start_x: float = get_repeat_start(float(layer_offset_x), float(imagewidth))
						rect_pos.x = start_x
						rect_size.x = maxf(float(_map_width * _map_tile_width) - start_x, float(imagewidth))
					if repeat_y:
						var start_y: float = get_repeat_start(float(layer_offset_y), float(imageheight))
						rect_pos.y = start_y
						rect_size.y = maxf(float(_map_height * _map_tile_height) - start_y, float(imageheight))
					texture_rect.position = rect_pos
					texture_rect.size = rect_size

			set_layer_class_meta(texture_rect, layer)
			if layer.has("properties"):
				handle_properties(texture_rect, layer["properties"])


# A layer's modulate: its Tiled tint colour (#RRGGBB or #AARRGGBB) with the layer opacity on its alpha
static func layer_color(tint_color: String, opacity: float) -> Color:
	var c := CommonUtils.tiled_color(tint_color)
	c.a *= opacity
	return c


# Returns the start coordinate (<= 0) of a repeated image so that the repeat phase matches the original offset
static func get_repeat_start(offset: float, image_size: float) -> float:
	return offset - ceilf(offset / image_size) * image_size


static func use_parent_material_recursively(node: Node) -> void:
	for child in node.get_children():
		if child is CanvasItem and child.material == null:
			child.use_parent_material = true
		use_parent_material_recursively(child)


func apply_blend_mode(node: CanvasItem, layer: Dictionary) -> void:
	if not layer.has("mode"): return
	var mode := str(layer["mode"]).to_lower()
	if mode == "" or mode == "normal": return
	if mode == "add" or mode == "multiply":
		var mat := CanvasItemMaterial.new()
		mat.blend_mode = CanvasItemMaterial.BLEND_MODE_ADD if mode == "add" else CanvasItemMaterial.BLEND_MODE_MUL
		node.material = mat
	else:
		print_rich("[color=" + WARNING_COLOR + "] -- Blend mode '" + mode + "' of layer '" + str(layer.get("name", "")) + "' is not supported. -> Ignored[/color]")
		CommonUtils.warning_count += 1

# Tiled's parallax factor of a layer (1.0 for an axis that is not set)
static func get_parallax_factor(layer_dict: Dictionary) -> Vector2:
	return Vector2(float(layer_dict.get("parallaxx", 1.0)), float(layer_dict.get("parallaxy", 1.0)))


# Sets up a Parallax2D (parallax2d mode) for a layer: scroll scale, the map's parallax origin, repeat.
# - Tiled multiplies a layer's factor with those of its parallax groups, while a Parallax2D inside a
#   Parallax2D adds its own scrolling to its parent's: for the effective factor F * f (F of the
#   enclosing groups) the nested node scrolls with 1 - F * (1 - f).
# - Tiled moves a layer by (1 - factor) * (view centre + parallax origin); Parallax2D moves itself by
#   scroll_offset + (1 - scroll_scale) * screen_offset (the screen's top-left corner). So
#   scroll_offset = origin * (1 - scroll_scale): the same offset with the top-left corner for the centre.
func configure_parallax2d(px: Parallax2D, layer_dict: Dictionary) -> void:
	var scale := Vector2.ONE - _parallax_scale * (Vector2.ONE - get_parallax_factor(layer_dict))
	px.scroll_scale = scale
	px.scroll_offset = Vector2(_parallax_origin_x, _parallax_origin_y) * (Vector2.ONE - scale)
	if int(layer_dict.get("repeatx", 0)) == 1 and int(layer_dict.get("imagewidth", 0)) > 0:
		px.repeat_size.x = float(layer_dict["imagewidth"])
		px.repeat_times = 2
	if int(layer_dict.get("repeaty", 0)) == 1 and int(layer_dict.get("imageheight", 0)) > 0:
		px.repeat_size.y = float(layer_dict["imageheight"])
		px.repeat_times = 2


func handle_parallaxes(parent: Node, layer_node: Node, layer_dict: Dictionary):
	if (layer_dict.has("parallaxx") or layer_dict.has("parallaxy")) and _parallax_mode != "legacy":
		# The layer stays at its place in the hierarchy, wrapped in a Parallax2D
		var px := Parallax2D.new()
		var px_name := str(layer_dict.get("name", ""))
		px.name = px_name + " (PX)" if px_name != "" else "Parallax2D"
		configure_parallax2d(px, layer_dict)
		parent.add_child(px)
		px.owner = _base_node
		px.add_child(layer_node)
	elif layer_dict.has("parallaxx") or layer_dict.has("parallaxy"):
		if not _parallax_layer_existing:
			if _background != null:
				_background.owner = null
				_background.reparent(_parallax_background)
				_background.owner = _base_node
			_parallax_layer_existing = true
	
		# Tiled's default parallax factor is 1.0 for an axis that is not set
		var par_x = layer_dict.get("parallaxx", 1.0)
		var par_y = layer_dict.get("parallaxy", 1.0)
		var parallax_node = ParallaxLayer.new()
		_parallax_background.add_child(parallax_node)
		parallax_node.owner = _base_node
		var px_name = layer_dict.get("name", "")
		parallax_node.name = px_name + " (PL)" if px_name != "" else "ParallaxLayer"
		parallax_node.motion_scale = Vector2(par_x, par_y)
		var mirror_x = layer_dict.get("imagewidth", 0) if layer_dict.get("repeatx", 0) else 0
		var mirror_y = layer_dict.get("imageheight", 0) if layer_dict.get("repeaty", 0) else 0
		if mirror_x != 0 or mirror_y != 0:
			parallax_node.motion_mirroring = Vector2(mirror_x, mirror_y)
		parallax_node.add_child(layer_node)
	else:
		parent.add_child(layer_node)

	layer_node.owner = _base_node


# Returns the layer's cell values as an Array or a Packed*Array (no element-wise copy)
func handle_data(data, map_size):
	match _encoding:
		"csv":
			if data is Array or data is PackedInt32Array or data is PackedInt64Array \
					or data is PackedFloat32Array or data is PackedFloat64Array:
				return data
			var ret: Array = []
			for cell in data:
				ret.append(cell)
			return ret
		"base64":
			var bytes: PackedByteArray = Marshalls.base64_to_raw(data)
			if _compression != "":
				match _compression:
					"gzip":
						bytes = bytes.decompress(map_size * 4, FileAccess.COMPRESSION_GZIP)
					"zlib":
						bytes = bytes.decompress(map_size * 4, FileAccess.COMPRESSION_DEFLATE)
					"zstd":
						bytes = bytes.decompress(map_size * 4, FileAccess.COMPRESSION_ZSTD)
					_:
						printerr("Decompression for type '" + _compression + "' not yet implemented.")
						CommonUtils.error_count += 1
						return []
			return bytes.to_int32_array()
	return []


func create_polygons_on_alternative_tiles(source_data: TileData, target_data: TileData, alt_id: int):
	var flipped_h = (alt_id & 1) > 0
	var flipped_v = (alt_id & 2) > 0
	var flipped_d = (alt_id & 4) > 0
	var origin = Vector2(source_data.texture_origin)
	var physics_layers_count = _tileset.get_physics_layers_count()
	for layer_id in range(physics_layers_count):
		var collision_polygons_count = source_data.get_collision_polygons_count(layer_id)
		for polygon_id in range(collision_polygons_count):
			var pts = source_data.get_collision_polygon_points(layer_id, polygon_id)
			var pts_new: PackedVector2Array
			var i = 0
			for pt in pts:
				pts_new.append(pt+origin)
				if flipped_d:
					var tmp = pts_new[i].x
					pts_new[i].x = pts_new[i].y
					pts_new[i].y = tmp
				if flipped_h:
					pts_new[i].x = -pts_new[i].x
				if flipped_v:
					pts_new[i].y = -pts_new[i].y
				pts_new[i] -= Vector2(target_data.texture_origin)
				i += 1
			target_data.add_collision_polygon(layer_id)
			target_data.set_collision_polygon_points(layer_id, polygon_id, pts_new)
	var navigation_layers_count = _tileset.get_navigation_layers_count()
	for layer_id in range(navigation_layers_count):
		var nav_p = source_data.get_navigation_polygon(layer_id)
		if nav_p == null: continue
		var pts = nav_p.get_outline(0)
		var pts_new: PackedVector2Array
		var i = 0
		for pt in pts:
			pts_new.append(pt+origin)
			if flipped_d:
				var tmp = pts_new[i].x
				pts_new[i].x = pts_new[i].y
				pts_new[i].y = tmp
			if flipped_h:
				pts_new[i].x = -pts_new[i].x
			if flipped_v:
				pts_new[i].y = -pts_new[i].y
			pts_new[i] -= Vector2(target_data.texture_origin)
			i += 1
		var navigation_polygon = NavigationPolygon.new()
		navigation_polygon.add_outline(pts_new)
		#navigation_polygon.make_polygons_from_outlines()
		# Replaced in 4.2 deprecated function make_polygons_from_outlines
		navigation_polygon.vertices = pts_new
		var polygon = PackedInt32Array()
		for idx in range(navigation_polygon.vertices.size()):
			polygon.push_back(idx)
		navigation_polygon.add_polygon(polygon)
		target_data.set_navigation_polygon(layer_id, navigation_polygon)
	var occlusion_layers_count = _tileset.get_occlusion_layers_count()
	for layer_id in range(occlusion_layers_count):
		var occ: OccluderPolygon2D
		if _godot_version >= 0x040400:
			var occ_count = source_data.get_occluder_polygons_count(layer_id)
			if occ_count == 0: continue
			occ = source_data.get_occluder_polygon(layer_id, 0)
		else:
			occ = source_data.get_occluder(layer_id)
		if occ == null: continue
		var pts = occ.polygon
		var pts_new: PackedVector2Array
		var i = 0
		for pt in pts:
			pts_new.append(pt+origin)
			if flipped_d:
				var tmp = pts_new[i].x
				pts_new[i].x = pts_new[i].y
				pts_new[i].y = tmp
			if flipped_h:
				pts_new[i].x = -pts_new[i].x
			if flipped_v:
				pts_new[i].y = -pts_new[i].y
			pts_new[i] -= Vector2(target_data.texture_origin)
			i += 1
		var occluder_polygon = OccluderPolygon2D.new()
		occluder_polygon.polygon = pts_new
		if _godot_version >= 0x040400:
			target_data.set_occluder_polygons_count(layer_id, 1)
			target_data.set_occluder_polygon(layer_id, 0, occluder_polygon)
		else:
			target_data.set_occluder(layer_id, occluder_polygon)
		

func create_map_from_data(layer_data, offset_x: int, offset_y: int, map_width: int):
	var cell_counter: int = -1
	var cell_cache: Dictionary = _cell_cache
	var tilemap_layer: TileMapLayer = _tilemap_layer
	for cell in layer_data:
		cell_counter += 1
		var int_id: int = int(cell) & 0xFFFFFFFF
		if (int_id & 0x0FFFFFFF) <= 0: continue
		var resolved = cell_cache.get(int_id)
		if resolved == null:
			resolved = resolve_cell(int_id)
			cell_cache[int_id] = resolved
		if resolved.is_empty(): continue
		@warning_ignore("integer_division")
		var cell_coords := Vector2i(cell_counter % map_width + offset_x, cell_counter / map_width + offset_y)
		tilemap_layer.set_cell(cell_coords, resolved[0], resolved[1], resolved[2])


# Resolves a raw cell value (gid incl. flip flags) to [source_id, atlas_coords, alt_id].
# Creates missing tiles / alternative tiles in _tileset on the way.
# Returns an empty array if the cell has to be skipped.
# Called once per distinct cell value and import (results are cached in _cell_cache).
func resolve_cell(int_id: int) -> Array:
	var flipped_h = (int_id & FLIPPED_HORIZONTALLY_FLAG) > 0
	var flipped_v = (int_id & FLIPPED_VERTICALLY_FLAG) > 0
	var flipped_d = (int_id & FLIPPED_DIAGONALLY_FLAG) > 0
	var gid: int = int_id & 0x0FFFFFFF

	var atlas_index: int = get_atlas_source_index(gid)
	# Should not be the case, but who knows...
	if atlas_index < 0: return []
	var atlas_source_item: Dictionary = _atlas_sources[atlas_index]
	var source_id: int = atlas_source_item["sourceId"]
	if source_id < 0: return []

	var tile_offset = atlas_source_item["tileOffset"]
	var tile_shader_material = atlas_source_item["shaderMaterial"]

	var atlas_source
	if _tileset.has_source(source_id):
		atlas_source = _tileset.get_source(source_id)
	else: return []
	var atlas_grid_size: Vector2i = atlas_source.get_atlas_grid_size()
	var atlas_width: int = atlas_grid_size.x
	if atlas_width <= 0: return []

	var effective_gid: int = gid - _first_gids[get_first_gid_index(gid)]
	var atlas_coords := Vector2i.ZERO
	if get_num_tiles_for_source_id(source_id) > 1:
		if atlas_grid_size == Vector2i.ONE:
			atlas_coords = Vector2i.ZERO
		else:
			@warning_ignore("integer_division")
			atlas_coords = Vector2i(effective_gid % atlas_width, effective_gid / atlas_width)
	var tile_key := Vector3i(source_id, atlas_coords.x, atlas_coords.y)
	if not _known_tiles.has(tile_key):
		_known_tiles[tile_key] = true
		if not atlas_source.has_tile(atlas_coords):
			atlas_source.create_tile(atlas_coords)
			var current_tile = atlas_source.get_tile_data(atlas_coords, 0)
			var tile_size = atlas_source.texture_region_size
			if tile_size.x != _map_tile_width or tile_size.y != _map_tile_height:
				var diff_x = tile_size.x - _map_tile_width
				if diff_x % 2 != 0:
					diff_x -= 1
				var diff_y = tile_size.y - _map_tile_height
				if diff_y % 2 != 0:
					diff_y += 1
				current_tile.texture_origin = Vector2i(-diff_x/2, diff_y/2) - tile_offset
			else:
				# The tileset's tileoffset applies to every tile (as for alternative tiles below)
				current_tile.texture_origin = -tile_offset

			if tile_shader_material != null:
				current_tile.material = tile_shader_material

	var alt_id = 0
	if flipped_h or flipped_v or flipped_d:
		if _dont_use_alternative_tiles:
			if flipped_h:
				alt_id |= TileSetAtlasSource.TRANSFORM_FLIP_H
			if flipped_v:
				alt_id |= TileSetAtlasSource.TRANSFORM_FLIP_V
			if flipped_d:
				alt_id |= TileSetAtlasSource.TRANSFORM_TRANSPOSE
		else:
			alt_id = (1 if flipped_h else 0) + (2 if flipped_v else 0) + (4 if flipped_d else 0)
			var alt_key := Vector4i(source_id, atlas_coords.x, atlas_coords.y, alt_id)
			if not _known_tiles.has(alt_key):
				_known_tiles[alt_key] = true
				if not atlas_source.has_alternative_tile(atlas_coords, alt_id):
					atlas_source.create_alternative_tile(atlas_coords, alt_id)
					var tile_data = atlas_source.get_tile_data(atlas_coords, alt_id)
					tile_data.flip_h = flipped_h
					tile_data.flip_v = flipped_v
					tile_data.transpose = flipped_d
					var tile_size = atlas_source.texture_region_size
					if flipped_d:
						tile_size = Vector2i(tile_size.y, tile_size.x)
					if tile_size.x != _map_tile_width or tile_size.y != _map_tile_height:
						var diff_x = tile_size.x - _map_tile_width
						if diff_x % 2 != 0:
							diff_x -= 1
						var diff_y = tile_size.y - _map_tile_height
						if diff_y % 2 != 0:
							diff_y += 1
						tile_data.texture_origin = Vector2i(-diff_x/2, diff_y/2)
					tile_data.texture_origin -= tile_offset

					if tile_shader_material != null:
						tile_data.material = tile_shader_material

					var src_data = atlas_source.get_tile_data(atlas_coords, 0)
					create_polygons_on_alternative_tiles(src_data, tile_data, alt_id)
					# Copy metadata to alternative tile
					for meta_name in src_data.get_meta_list():
						tile_data.set_meta(meta_name, src_data.get_meta(meta_name))

	return [source_id, atlas_coords, alt_id]


static func get_godot_type(godot_type_string: String):
	return GODOT_TYPE_MAP.get(godot_type_string.to_lower(), _godot_type.UNKNOWN)


static func get_godot_node_type_property(obj: Dictionary):
	var ret = ""
	var property_found = false
	if obj.has("properties"):
		for property in obj["properties"]:
			var name: String = property.get("name", "")
			var type: String = property.get("type", "string")
			var val: String = str(property.get("value", ""))
			if name.to_lower() == GODOT_NODE_TYPE_PROPERTY and type == "string":
				property_found = true
				ret = val
				break
	return [ret, property_found]


# The Tiled class of an object, tile or layer. Objects and tiles of files older than Tiled 1.9 saved
# it as 'type'; on a layer 'type' is the layer kind (tilelayer, objectgroup...), never a class.
static func get_class_string(d: Dictionary, is_layer: bool) -> String:
	var class_string := str(d.get("class", ""))
	if class_string == "" and not is_layer:
		class_string = str(d.get("type", ""))
	return class_string


# Sets metadata "class" on a layer's node (if the layer has a class and the option is on)
func set_layer_class_meta(node: Node, layer: Dictionary) -> void:
	var class_string := get_class_string(layer, true)
	if _add_class_as_metadata and class_string != "":
		node.set_meta("class", class_string)


static func get_godot_node_type(obj: Dictionary, is_layer: bool = false):
	var class_string = get_class_string(obj, is_layer)
	var search_result = get_godot_node_type_property(obj)
	var godot_node_type_property_string = search_result[0]
	var godot_node_type_prop_found = search_result[1]
	if not godot_node_type_prop_found:
		godot_node_type_property_string = class_string
	return get_godot_type(godot_node_type_property_string)


static func set_sprite_offset(obj_sprite: Sprite2D, width: float, height: float, alignment: String):
	var unit: Vector2 = ALIGNMENT_UNIT_OFFSETS.get(alignment, ALIGNMENT_UNIT_OFFSETS["bottomleft"])
	obj_sprite.offset = unit * Vector2(width, height)


static func get_instance_offset(width: float, height: float, r_alignment: String, c_alignment: String) -> Vector2:
	var size := Vector2(width, height)
	var centered_alignment: Vector2 = ALIGNMENT_UNIT_OFFSETS.get(c_alignment, Vector2.ZERO) * size
	return centered_alignment - ALIGNMENT_UNIT_OFFSETS.get(r_alignment, Vector2.ZERO) * size


static func get_position_offset(width: float, height: float, rotation: float) -> Vector2:
	var orig_point = Vector2(-width / 2.0, -height / 2.0)
	var rot_rad = rotation * PI / 180.0
	var new_point = orig_point.rotated(rot_rad)
	return orig_point - new_point


func convert_metadata_to_obj_properties(td: TileData, obj: Dictionary) -> void:
	var meta_list = td.get_meta_list()
	for meta_name in meta_list:
		var meta_name_lower: String = String(meta_name).to_lower()
		if meta_name_lower == GODOT_NODE_TYPE_PROPERTY:
			continue
		if meta_name_lower == CLASS_INTERNAL and not _add_class_as_metadata:
			continue
		var meta_val = td.get_meta(meta_name)
		var meta_type = typeof(meta_val)
		var prop_dict = {}
		prop_dict["name"] = meta_name
		var prop_type = META_TYPE_TO_PROPERTY_TYPE.get(meta_type, "string")
		# Type "file" assumed and thus forced for these properties
		if meta_name_lower in FORCED_FILE_PROPERTIES:
			prop_type = "file"

		prop_dict["type"] = prop_type
		prop_dict["value"] = meta_val

		if obj.has("properties"):
			# Add property only if not already contained in properties
			var found = false
			for prop in obj["properties"]:
				if prop["name"].to_lower() == meta_name_lower:
					found = true
					break
			if not found:
				obj["properties"].append(prop_dict)
		else:
			obj["properties"] = [prop_dict]


func handle_object(obj: Dictionary, layer_node: Node, tileset: TileSet, offset: Vector2) -> void:
	# Template instances pass their (overriding) opacity on to the template objects
	if obj.has("template") or not obj.has("opacity"):
		create_object(obj, layer_node, tileset, offset)
		return
	var opacity: float = float(obj["opacity"])
	var child_count_before: int = layer_node.get_child_count()
	create_object(obj, layer_node, tileset, offset)
	if opacity >= 1.0: return
	# Apply the object opacity to whatever node(s) got created for the object
	for i in range(child_count_before, layer_node.get_child_count()):
		var created = layer_node.get_child(i)
		if created is CanvasItem:
			created.modulate.a *= opacity


func load_template(template_file: String) -> Dictionary:
	var template_file_full_path = _base_path.path_join(template_file)
	if _template_cache.has(template_file_full_path):
		return _template_cache[template_file_full_path]
	var entry = {"dict": {}, "tileset": null, "missing": false, "full_path": template_file_full_path}
	var template_content = DataLoader.get_tiled_file_content(template_file, _base_path)
	if template_content == null:
		entry["missing"] = true
	else:
		var template_dict = DictionaryBuilder.new().get_dictionary(template_content, template_file)
		if template_dict == null:
			template_dict = {}
		entry["dict"] = template_dict
		if template_dict.has("tilesets"):
			var tilesets = template_dict["tilesets"]
			var tileset_creator = TilesetCreator.new()
			tileset_creator.set_base_path(template_file_full_path)
			tileset_creator.set_map_parameters(Vector2i(_map_tile_width, _map_tile_height))
			if _map_wangset_to_terrain:
				tileset_creator.map_wangset_to_terrain()
			# (an unsupported value was reported by the map's tileset creator already)
			tileset_creator._animation_mode = "atlas" if _animation_mode.to_lower() == "atlas" else "strip"
			entry["tileset"] = tileset_creator.create_from_dictionary_array(tilesets)
	_template_cache[template_file_full_path] = entry
	return entry


func create_object(obj: Dictionary, layer_node: Node, tileset: TileSet, offset: Vector2) -> void:
	var obj_id = obj.get("id", 0)
	var obj_x = obj.get("x", offset.x)
	var obj_y = obj.get("y", offset.y)
	var obj_rot = obj.get("rotation", 0.0)
	var obj_width = obj.get("width", 0.0)
	var obj_height = obj.get("height", 0.0)
	var obj_visible = obj.get("visible", true)
	var obj_name = obj.get("name", "")

	if _ct != null:
		_ct.merge_custom_properties(obj, "object")

	var class_string = get_class_string(obj, false)
	var search_result = get_godot_node_type_property(obj)
	var godot_node_type_property_string = search_result[0]
	var godot_node_type_prop_found = search_result[1]
	if not godot_node_type_prop_found:
		godot_node_type_property_string = class_string
	var godot_type = get_godot_type(godot_node_type_property_string)

	if godot_type == _godot_type.UNKNOWN:
		if godot_node_type_prop_found:
			print_rich("[color=" + WARNING_COLOR +"] -- Unknown " + GODOT_NODE_TYPE_PROPERTY + " '" + godot_node_type_property_string + "'. -> Using '" + _unknown_class_fallback + "'[/color]")
			CommonUtils.warning_count += 1
		elif not _reported_unknown_classes.has(class_string):
			# Once per class name and import, whatever add_class_as_metadata says
			_reported_unknown_classes[class_string] = true
			print_rich("[color=" + WARNING_COLOR +"] -- Class '" + class_string + "' is not a Godot node type -> using '" + _unknown_class_fallback + "'. Set '" + GODOT_NODE_TYPE_PROPERTY + "' or the option 'unknown_class_fallback' (see Reference).[/color]")
			CommonUtils.warning_count += 1
		godot_type = _unknown_class_type


	if obj.has("template"):
		var template_file = obj["template"]
		# Templates are parsed (and their tilesets created) only once per import
		var template_entry: Dictionary = load_template(template_file)
		var template_file_full_path: String = template_entry["full_path"]
		if template_entry["missing"]:
			printerr("ERROR: Template file '" + template_file + "' not found. -> Continuing but result may be unusable")
			CommonUtils.error_count += 1
		var template_dict: Dictionary = template_entry["dict"]
		var template_tileset = template_entry["tileset"]

		if template_dict.has("objects"):
			# The template objects get modified below (and in handle_object), so work on a copy
			var template_objects: Array = template_dict["objects"].duplicate(true)
			for template_obj in template_objects:
				template_obj["template_dir_path"] = template_file_full_path.get_base_dir()

				# v1.5.3 Fix according to Carlo M (dogezen)
				# override and merge properties defined in obj with properties defined in template
				# since obj may override and define additional properties to those defined in template
				if obj.has("properties"):
					if template_obj.has("properties"):
						# merge obj properties that may have been overridden in the obj instance
						# and add any additional properties defined in instanced obj that are 
						# not defined in template
						for prop in obj["properties"]:
							var found = false
							for templ_prop in template_obj["properties"]:
								if prop.name == templ_prop.name:
									templ_prop.value = prop.value
									found = true
									break
							if not found:
								template_obj["properties"].append(prop)
					else:
						# template comes without properties, since obj has properties
						# then merge them into the template
						template_obj["properties"] = obj.properties

				# Template obj needs id and obj class/type/name overrides template's class/type/name
				template_obj["id"] = obj_id
				if obj.has("class") and obj["class"] != "":
					template_obj["class"] = obj["class"]
				if obj.has("type") and obj["type"] != "":
					template_obj["type"] = obj["type"]
				if obj.has("name") and obj["name"] != "":
					template_obj["name"] = obj["name"]
				if obj.has("opacity"):
					template_obj["opacity"] = obj["opacity"]
				# Attributes saved on the instance override the template's (TMX spec, "Template files")
				for key in ["width", "height", "rotation", "visible"]:
					if obj.has(key):
						template_obj[key] = obj[key]
				# An instance's own text (TMX spec: attributes saved on the instance override the template's)
				if obj.has("text") and obj["text"] is Dictionary:
					if template_obj.has("text") and template_obj["text"] is Dictionary:
						template_obj["text"].merge(obj["text"], true)
					else:
						template_obj["text"] = obj["text"]
				# An instance's own gid (a flipped instance saves one) is a gid of the map's tilesets
				var obj_tileset = template_tileset
				if obj.has("gid"):
					template_obj["gid"] = obj["gid"]
					obj_tileset = tileset

				handle_object(template_obj, layer_node, obj_tileset, Vector2(obj_x, obj_y))
		# Everything the instance defines went into the template's object(s) above
		return

	# v1.2: New class 'instance'
	if godot_type == _godot_type.INSTANCE and not obj.has("template") and not obj.has("text") and not obj.has("gid"):
		var res_path = get_property(obj, "res_path", "file")
		if res_path == "":
			printerr("Object of class 'instance': Mandatory file property 'res_path' not found or invalid. -> Skipped")
			CommonUtils.error_count += 1
		else:
			if obj.has("template_dir_path"):
				res_path = obj.template_dir_path.path_join(res_path)
			var scene = DataLoader.load_resource_from_file(res_path, _base_path)
			# Error check
			if scene == null: return
			var instance = scene.instantiate()
			layer_node.add_child(instance)
			instance.owner = _base_node
			instance.name = obj_name if obj_name != "" else res_path.get_file().get_basename()
			instance.position = transpose_coords(obj_x, obj_y)
			instance.rotation_degrees = obj_rot
			instance.visible = obj_visible
			if _add_class_as_metadata and class_string != "":
				instance.set_meta("class", class_string)
			if _add_id_as_metadata and obj_id != 0:
				instance.set_meta("id", obj_id)
			if obj.has("properties"):
				handle_properties(instance, obj["properties"])
		return

	if obj.has("gid") and not obj.has("template"):
		# gid refers to a tile in a tile set and object is created as sprite
		if godot_type == _godot_type.NODE2D:
			# Unknown class with fallback "node2d": a plain Sprite2D (the tile's class may still decide)
			godot_type = _godot_type.EMPTY
		var int_id: int = obj["gid"]
		var flippedH = (int_id & FLIPPED_HORIZONTALLY_FLAG) > 0
		var flippedV = (int_id & FLIPPED_VERTICALLY_FLAG) > 0
		var gid: int = int_id & 0x0FFFFFFF

		var source_id = get_matching_source_id(gid)
		# Should not be the case, but who knows...
		if source_id < 0: return

		var tile_offset = get_tile_offset(gid)
		_current_tileset_orientation = get_tileset_orientation(gid)
		_current_object_alignment = get_tileset_alignment(gid)
		if _current_object_alignment == DEFAULT_ALIGNMENT:
			_current_object_alignment = "bottomleft" if _map_orientation == "orthogonal" else "bottom"

		if not tileset.has_source(source_id):
			printerr("Could not get AtlasSource with id " + str(source_id) + ". -> Skipped")
			CommonUtils.error_count += 1
			return
	
		var gid_source = tileset.get_source(source_id)
		var obj_sprite = Sprite2D.new()
		layer_node.add_child(obj_sprite)
		obj_sprite.owner = _base_node
		obj_sprite.name = obj_name if obj_name != "" \
							else gid_source.resource_name if gid_source.resource_name != "" \
							else gid_source.texture.resource_path.get_file().get_basename() + "_tile"
		obj_sprite.position = transpose_coords(obj_x, obj_y) + Vector2(tile_offset)
		obj_sprite.texture = gid_source.texture
		obj_sprite.rotation_degrees = obj_rot
		obj_sprite.visible = obj_visible
		var td
		var tile_coords := Vector2i.ZERO
		if is_partitioned_tileset(source_id):
			# Object is tile from partitioned tileset 
			var atlas_width: int = gid_source.get_atlas_grid_size().x

			# Can be zero if tileset had an error
			if atlas_width <= 0: return

			var effective_gid: int = gid - _first_gids[get_first_gid_index(gid)]
			@warning_ignore("integer_division")
			var atlas_coords = Vector2(effective_gid % atlas_width, effective_gid / atlas_width)
			if not gid_source.has_tile(atlas_coords):
				gid_source.create_tile(atlas_coords)
			td = gid_source.get_tile_data(atlas_coords, 0)
			tile_coords = Vector2i(atlas_coords)
			obj_sprite.region_enabled = true
			var region_size = Vector2(gid_source.texture_region_size)
			var separation = Vector2(gid_source.separation)
			var margins = Vector2(gid_source.margins)
			var pos: Vector2 = atlas_coords * (region_size + separation) + margins
			if get_property(obj, "clip_artifacts", "bool") == "true":
				pos += Vector2(0.5, 0.5)
				region_size -= Vector2(1.0, 1.0)
				obj_width -= 1.0
				obj_height -= 1.0
			obj_sprite.region_rect = Rect2(pos, region_size)
			set_sprite_offset(obj_sprite, region_size.x, region_size.y, _current_object_alignment)
			if abs(region_size.x - obj_width) > 0.01 or abs(region_size.y - obj_height) > 0.01:
				var scale_x: float = float(obj_width) / float(region_size.x)
				var scale_y: float = float(obj_height) / float(region_size.y)
				obj_sprite.scale = Vector2(scale_x, scale_y)
		else:
			# Object is single image tile
			var gid_width: int = gid_source.texture_region_size.x
			var gid_height: int = gid_source.texture_region_size.y
			obj_sprite.offset = Vector2(gid_width / 2.0, -gid_height / 2.0)
			set_sprite_offset(obj_sprite, gid_width, gid_height, _current_object_alignment)
			# Tiled sub rects?
			if gid_width != gid_source.texture.get_width() or gid_height != gid_source.texture.get_height():
				obj_sprite.region_enabled = true
				obj_sprite.region_rect = Rect2(gid_source.margins, gid_source.texture_region_size)
			if gid_width != obj_width or gid_height != obj_height:
				var scale_x: float = float(obj_width) / gid_width
				var scale_y: float = float(obj_height) / gid_height
				obj_sprite.scale = Vector2(scale_x, scale_y)
			td = gid_source.get_tile_data(Vector2i.ZERO, 0)

		# Tile objects may already have been classified as instance in the tileset
		var obj_is_instance = godot_type == _godot_type.INSTANCE and not obj.has("template") and not obj.has("text")
		var tile_class = ""
		if td.has_meta(CLASS_INTERNAL):
			tile_class = td.get_meta(CLASS_INTERNAL)
			class_string = tile_class
		if td.has_meta(GODOT_NODE_TYPE_PROPERTY):
			tile_class = td.get_meta(GODOT_NODE_TYPE_PROPERTY)

		if tile_class.to_lower() == "instance" or obj_is_instance:
			var instance_props: Dictionary = get_properties(obj, TILE_INSTANCE_PROPERTY_KEYS)
			var res_path = instance_props.get("res_path:file", "")
			if td.has_meta("res_path"):
				if res_path == "":
					res_path = td.get_meta("res_path")
			if res_path == "":
				printerr("Object of class 'instance': Mandatory file property 'res_path' not found or invalid. -> Skipped")
				CommonUtils.error_count += 1
			else:
				if obj.has("template_dir_path"):
					res_path = obj.template_dir_path.path_join(res_path)
				var scene = DataLoader.load_resource_from_file(res_path, _base_path)
				# Error check
				if scene == null: return

				var res_path_extension = res_path.get_file().get_extension().to_lower()
				var res_alignment = instance_props.get("res_alignment:string", "")

				obj_sprite.owner = null
				layer_node.remove_child(obj_sprite)
				obj_sprite.free()
				var instance = scene.instantiate()
				layer_node.add_child(instance)
				instance.owner = _base_node
				instance.name = obj_name if obj_name != "" else res_path.get_file().get_basename()
				instance.position = transpose_coords(obj_x, obj_y)
				if res_alignment != "":
					instance.position += get_instance_offset(obj_width, obj_height, res_alignment, _current_object_alignment)
				elif res_path_extension == "tmx" or res_path_extension == "tmj":
					instance.position += get_instance_offset(obj_width, obj_height, "topleft", _current_object_alignment)
				instance.rotation_degrees = obj_rot
				instance.visible = obj_visible
				instance.scale = obj_sprite.scale
				convert_metadata_to_obj_properties(td, obj)
				if _add_class_as_metadata and class_string != "":
					instance.set_meta("class", class_string)
				if _add_id_as_metadata and obj_id != 0:
					instance.set_meta("id", obj_id)
				if obj.has("properties"):
					handle_properties(instance, obj["properties"])
			return

		# An animated tile is shown animated
		if gid_source.get_tile_animation_frames_count(tile_coords) > 1:
			if get_property(obj, "clip_artifacts", "bool") == "true":
				print_rich("[color=" + WARNING_COLOR + "] -- Tile object '" + str(obj_sprite.name) + "': 'clip_artifacts' needs a Sprite2D region. -> Not animated[/color]")
				CommonUtils.warning_count += 1
			else:
				obj_sprite = replace_with_animated_sprite(obj_sprite, gid_source, tile_coords, layer_node)

		# Tile objects may already have been classified as ...body in the tileset
		if tile_class != "" and godot_type == _godot_type.EMPTY:
			godot_type = get_godot_type(tile_class)

		convert_metadata_to_obj_properties(td, obj)

		var idx = td.get_custom_data(CUSTOM_DATA_INTERNAL)
		if idx > 0:
			# Only instantiate the needed node (a dictionary of .new() calls leaked the unused ones)
			var parent = null
			match godot_type:
				_godot_type.AREA: parent = Area2D.new()
				_godot_type.CBODY: parent = CharacterBody2D.new()
				_godot_type.RBODY: parent = RigidBody2D.new()
				_godot_type.ABODY: parent = AnimatableBody2D.new()
				_godot_type.BODY: parent = StaticBody2D.new()
			if parent != null:
				obj_sprite.owner = null
				layer_node.remove_child(obj_sprite)
				layer_node.add_child(parent)
				parent.owner = _base_node
				parent.name = obj_sprite.name
				parent.position = obj_sprite.position
				parent.rotation_degrees = obj_sprite.rotation_degrees
				obj_sprite.position = Vector2.ZERO
				obj_sprite.rotation_degrees = 0.0
				parent.add_child(obj_sprite)
				obj_sprite.owner = _base_node
				add_collision_shapes(parent, get_object_group(idx), obj_width, obj_height, flippedH, flippedV, obj_sprite.scale)
				if obj.has("properties"):
					handle_properties(parent, obj["properties"])
			
		obj_sprite.flip_h = flippedH
		obj_sprite.flip_v = flippedV

		if _add_class_as_metadata and class_string != "":
			obj_sprite.set_meta("class", class_string)
		if _add_id_as_metadata and obj_id != 0:
			obj_sprite.set_meta("id", obj_id)
		if obj.has("properties"):
			handle_properties(obj_sprite, obj["properties"])

	elif obj.has("text") and not obj.has("template"):
		var obj_text = Label.new()
		layer_node.add_child(obj_text)
		obj_text.owner = _base_node
		obj_text.name = obj_name if obj_name != "" else "Text"
		obj_text.position = transpose_coords(obj_x, obj_y)
		obj_text.size = Vector2(obj_width, obj_height)
		obj_text.clip_text = true
		obj_text.rotation_degrees = obj_rot
		obj_text.visible = obj_visible
		var txt = obj["text"]
		obj_text.text = txt.get("text", "Hello World")
		var wrap_ = txt.get("wrap", false)
		obj_text.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART if wrap_ else TextServer.AUTOWRAP_OFF
		var align_h = txt.get("halign", "left")
		match align_h:
			"left": obj_text.horizontal_alignment = HORIZONTAL_ALIGNMENT_LEFT
			"center": obj_text.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
			"right": obj_text.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
			"justify": obj_text.horizontal_alignment = HORIZONTAL_ALIGNMENT_FILL
		var align_v = txt.get("valign", "top")
		match align_v:
			"top": obj_text.vertical_alignment = VERTICAL_ALIGNMENT_TOP
			"center": obj_text.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
			"bottom": obj_text.vertical_alignment = VERTICAL_ALIGNMENT_BOTTOM
		var font_family = txt.get("fontfamily", "Sans-Serif")
		var font = SystemFont.new()
		font.font_names = [font_family]
		font.oversampling = 5.0
		if is_true(txt.get("bold", false)):
			font.font_weight = 700
		if is_true(txt.get("italic", false)):
			font.font_italic = true
		obj_text.add_theme_font_override("font", font)
		obj_text.add_theme_font_size_override("font_size", txt.get("pixelsize", 16))
		obj_text.add_theme_color_override("font_color", CommonUtils.tiled_color(str(txt.get("color", "#000000")), Color.BLACK))
		if obj.has("properties"):
			handle_properties(obj_text, obj["properties"])

	elif not obj.has("template"):

		if godot_type == _godot_type.EMPTY:
			godot_type = _godot_type.BODY

		var object_base_coords = transpose_coords(obj_x, obj_y)

		if godot_type == _godot_type.NODE2D and not obj.has("point"):
			create_plain_node(obj, layer_node, object_base_coords, class_string)
		elif obj.has("point"):
			var marker = Marker2D.new()
			layer_node.add_child(marker)
			marker.owner = _base_node
			marker.name = obj_name if obj_name != "" else "point"
			marker.position = object_base_coords
			marker.rotation_degrees = obj_rot
			marker.visible = obj_visible
			if _add_class_as_metadata and class_string != "":
				marker.set_meta("class", class_string)
			if _add_id_as_metadata and obj_id != 0:
				marker.set_meta("id", obj_id)		
			if obj.has("properties"):
				handle_properties(marker, obj["properties"])
		elif obj.has("polygon"):
			if godot_type in COLLISION_OBJECT_TYPES:
				var co: CollisionObject2D = create_collision_object(godot_type, obj_name, layer_node)
				co.owner = _base_node
				co.position = object_base_coords
				co.visible = obj_visible
				var polygon_shape = CollisionPolygon2D.new()
				polygon_shape.polygon = polygon_from_array(obj["polygon"])
				co.add_child(polygon_shape)
				polygon_shape.owner = _base_node
				polygon_shape.name = obj_name if obj_name != "" else "Polygon Shape"
				polygon_shape.position = Vector2.ZERO
				polygon_shape.rotation_degrees = obj_rot
				if _add_class_as_metadata and class_string != "":
					co.set_meta("class", class_string)
				if _add_id_as_metadata and obj_id != 0:
					co.set_meta("id", obj_id)
				if obj.has("properties"):
					handle_properties(co, obj["properties"])
			elif godot_type == _godot_type.NAVIGATION:
				var nav_region = NavigationRegion2D.new()
				layer_node.add_child(nav_region)
				nav_region.owner = _base_node
				nav_region.name = obj_name + " (NR)" if obj_name != "" else "Navigation"
				nav_region.position = object_base_coords
				nav_region.rotation_degrees = obj_rot
				nav_region.visible = obj_visible
				var nav_poly = NavigationPolygon.new()
				nav_region.navigation_polygon = nav_poly
				var pg = polygon_from_array(obj["polygon"])
				nav_poly.add_outline(pg)
				#nav_poly.make_polygons_from_outlines()
				# Replaced in 4.2 deprecated function make_polygons_from_outlines
				nav_poly.vertices = pg
				var polygon = PackedInt32Array()
				for idx in range(nav_poly.vertices.size()):
					polygon.push_back(idx)
				nav_poly.add_polygon(polygon)

				if _add_class_as_metadata and class_string != "":
					nav_region.set_meta("class", class_string)
				if _add_id_as_metadata and obj_id != 0:
					nav_region.set_meta("id", obj_id)
				if obj.has("properties"):
					handle_properties(nav_region, obj["properties"])
			elif godot_type == _godot_type.OCCLUDER:
				var light_occ = LightOccluder2D.new()
				layer_node.add_child(light_occ)
				light_occ.owner = _base_node
				light_occ.name = obj_name + " (LO)" if obj_name != "" else "Occluder"
				light_occ.position = object_base_coords
				light_occ.rotation_degrees = obj_rot
				light_occ.visible = obj_visible
				var occ_poly = OccluderPolygon2D.new()
				light_occ.occluder = occ_poly
				occ_poly.polygon = polygon_from_array(obj["polygon"])
				if _add_class_as_metadata and class_string != "":
					light_occ.set_meta("class", class_string)
				if _add_id_as_metadata and obj_id != 0:
					light_occ.set_meta("id", obj_id)
				if obj.has("properties"):
					handle_properties(light_occ, obj["properties"])
			elif godot_type == _godot_type.POLYGON:
				var polygon = Polygon2D.new()
				layer_node.add_child(polygon)
				polygon.owner = _base_node
				polygon.name = obj_name if obj_name != "" else "Polygon"
				polygon.position = object_base_coords
				polygon.rotation_degrees = obj_rot
				polygon.visible = obj_visible
				polygon.polygon = polygon_from_array(obj["polygon"])
				if _add_class_as_metadata and class_string != "":
					polygon.set_meta("class", class_string)
				if _add_id_as_metadata and obj_id != 0:
					polygon.set_meta("id", obj_id)
				if obj.has("properties"):
					handle_properties(polygon, obj["properties"])	
		elif obj.has("polyline"):
			if godot_type == _godot_type.LINE:
				var line = Line2D.new()
				layer_node.add_child(line)
				line.owner = _base_node
				line.name = obj_name if obj_name != "" else "Line"
				line.position = object_base_coords
				line.visible = obj_visible
				line.rotation_degrees = obj_rot
				line.width = 1.0
				line.points = polygon_from_array(obj["polyline"])
				if _add_class_as_metadata and class_string != "":
					line.set_meta("class", class_string)
				if _add_id_as_metadata and obj_id != 0:
					line.set_meta("id", obj_id)
				if obj.has("properties"):
					handle_properties(line, obj["properties"])
			elif godot_type == _godot_type.PATH:
				var path = Path2D.new()
				layer_node.add_child(path)
				path.owner = _base_node
				path.name = obj_name if obj_name != "" else "Path"
				path.position = object_base_coords
				path.visible = obj_visible
				path.rotation_degrees = obj_rot
				var curve = Curve2D.new()
				for point in obj["polyline"]:
					curve.add_point(Vector2(point.x, point.y))
				path.curve = curve
				if _add_class_as_metadata and class_string != "":
					path.set_meta("class", class_string)
				if _add_id_as_metadata and obj_id != 0:
					path.set_meta("id", obj_id)
				if obj.has("properties"):
					handle_properties(path, obj["properties"])
			else:
				var co: CollisionObject2D = create_collision_object(godot_type, obj_name, layer_node)
				co.owner = _base_node
				co.position = object_base_coords
				co.visible = obj_visible
				var polyline_points: Array = obj["polyline"]
				for i in range(0, polyline_points.size()-1):
					var collision_shape = CollisionShape2D.new()
					co.add_child(collision_shape)
					var segment_shape = SegmentShape2D.new()
					segment_shape.a = transpose_coords(polyline_points[i]["x"], polyline_points[i]["y"], true)
					segment_shape.b = transpose_coords(polyline_points[i+1]["x"], polyline_points[i+1]["y"], true)
					collision_shape.owner = _base_node
					collision_shape.shape = segment_shape
					collision_shape.position = Vector2(obj_width / 2.0, obj_height / 2.0)
					collision_shape.rotation_degrees = obj_rot
					collision_shape.name = "Segment Shape"
				if _add_class_as_metadata and class_string != "":
					co.set_meta("class", class_string)
				if _add_id_as_metadata and obj_id != 0:
					co.set_meta("id", obj_id)
				if obj.has("properties"):
					handle_properties(co, obj["properties"])
		else:
			if godot_type in COLLISION_OBJECT_TYPES:
				var co: CollisionObject2D = create_collision_object(godot_type, obj_name, layer_node)
				co.owner = _base_node
				co.position = object_base_coords
				co.visible = obj_visible

				var collision_shape = CollisionShape2D.new()
				co.add_child(collision_shape)
				collision_shape.owner = _base_node
				var obj_rot_orig = obj_rot
				if obj.has("capsule") or obj.has("ellipse"):
					var capsule_shape = CapsuleShape2D.new()
					if obj_height >= obj_width:
						capsule_shape.height = obj_height
						capsule_shape.radius = obj_width / 2.0
					else:
						capsule_shape.height = obj_width
						capsule_shape.radius = obj_height / 2.0
						obj_rot += 90
					collision_shape.shape = capsule_shape
					collision_shape.name = obj_name if obj_name != "" else "Capsule Shape"
				else: #Rectangle
					var rectangle_shape = RectangleShape2D.new()
					rectangle_shape.size = Vector2(obj_width, obj_height)
					collision_shape.shape = rectangle_shape
					collision_shape.name = obj_name if obj_name != "" else "Rectangle Shape"

				if _map_orientation == "isometric":
					if _iso_rot == 0.0:
						var q = float(_map_tile_height) / float(_map_tile_width)
						q *= q
						var cos_a = sqrt(1 / (q + 1))
						_iso_rot = acos(cos_a) * 180 / PI
						_iso_skew = (90 - 2 * _iso_rot) * PI / 180
						var scale = float(_map_tile_width) / (float(_map_tile_height) * 2 * cos_a)
						_iso_scale = Vector2(scale, scale)

					if obj_height >= obj_width or collision_shape.shape is RectangleShape2D:
						collision_shape.skew = _iso_skew
						obj_rot += _iso_rot
					else:
						collision_shape.skew = -_iso_skew
						obj_rot += -90 - _iso_rot
	
					collision_shape.scale = _iso_scale

				collision_shape.position = transpose_coords(obj_width / 2.0, obj_height / 2.0, true)
				collision_shape.position += get_position_offset(obj_width, obj_height, obj_rot_orig)
				collision_shape.rotation_degrees = obj_rot
				collision_shape.visible = obj_visible
				if _add_class_as_metadata and class_string != "":
					co.set_meta("class", class_string)
				if _add_id_as_metadata and obj_id != 0:
					co.set_meta("id", obj_id)
				if obj.has("properties"):
					handle_properties(co, obj["properties"])
			elif godot_type == _godot_type.NAVIGATION:
				if obj.has("capsule"):
					print_rich("[color="+WARNING_COLOR+"] -- Capsule is unusable for NavigationRegion2D. -> Skipped[/color]")
					CommonUtils.warning_count += 1
				elif obj.has("ellipse"):
					print_rich("[color="+WARNING_COLOR+"] -- Ellipse is unusable for NavigationRegion2D. -> Skipped[/color]")
					CommonUtils.warning_count += 1
				else:
					var nav_region = NavigationRegion2D.new()
					layer_node.add_child(nav_region)
					nav_region.owner = _base_node
					nav_region.name = obj_name + " (NR)" if obj_name != "" else "Navigation"
					nav_region.position = object_base_coords
					nav_region.rotation_degrees = obj_rot
					nav_region.visible = obj_visible
					var nav_poly = NavigationPolygon.new()
					nav_region.navigation_polygon = nav_poly
					var pg = polygon_from_rectangle(obj_width, obj_height)
					nav_poly.add_outline(pg)
					#nav_poly.make_polygons_from_outlines()
					# Replaced in 4.2 deprecated function make_polygons_from_outlines
					nav_poly.vertices = pg
					var polygon = PackedInt32Array()
					for idx in range(nav_poly.vertices.size()):
						polygon.push_back(idx)
					nav_poly.add_polygon(polygon)

					if _add_class_as_metadata and class_string != "":
						nav_region.set_meta("class", class_string)
					if _add_id_as_metadata and obj_id != 0:
						nav_region.set_meta("id", obj_id)		
					if obj.has("properties"):
						handle_properties(nav_region, obj["properties"])
			elif godot_type == _godot_type.OCCLUDER:
				if obj.has("capsule"):
					print_rich("[color="+WARNING_COLOR+"] -- Capsule is unusable for LightOccluder2D. -> Skipped[/color]")
					CommonUtils.warning_count += 1
				elif obj.has("ellipse"):
					print_rich("[color="+WARNING_COLOR+"] -- Ellipse is unusable for LightOccluder2D. -> Skipped[/color]")
					CommonUtils.warning_count += 1
				else:
					var light_occ = LightOccluder2D.new()
					layer_node.add_child(light_occ)
					light_occ.owner = _base_node
					light_occ.name = obj_name + " (LO)" if obj_name != "" else "Occluder"
					light_occ.position = object_base_coords
					light_occ.rotation_degrees = obj_rot
					light_occ.visible = obj_visible
					var occ_poly = OccluderPolygon2D.new()
					light_occ.occluder = occ_poly
					occ_poly.polygon = polygon_from_rectangle(obj_width, obj_height)
					if _add_class_as_metadata and class_string != "":
						light_occ.set_meta("class", class_string)
					if _add_id_as_metadata and obj_id != 0:
						light_occ.set_meta("id", obj_id)
					if obj.has("properties"):
						handle_properties(light_occ, obj["properties"])
			elif godot_type == _godot_type.POLYGON:
				if obj.has("capsule"):
					print_rich("[color="+WARNING_COLOR+"] -- Capsule is unusable for Polygon2D. -> Skipped[/color]")
					CommonUtils.warning_count += 1
				elif obj.has("ellipse"):
					print_rich("[color="+WARNING_COLOR+"] -- Ellipse is unusable for Polygon2D. -> Skipped[/color]")
					CommonUtils.warning_count += 1
				else:
					var polygon = Polygon2D.new()
					layer_node.add_child(polygon)
					polygon.owner = _base_node
					polygon.name = obj_name if obj_name != "" else "Polygon"
					polygon.position = object_base_coords
					polygon.rotation_degrees = obj_rot
					polygon.visible = obj_visible
					polygon.polygon = polygon_from_rectangle(obj_width, obj_height)
					if _add_class_as_metadata and class_string != "":
						polygon.set_meta("class", class_string)
					if _add_id_as_metadata and obj_id != 0:
						polygon.set_meta("id", obj_id)		
					if obj.has("properties"):
						handle_properties(polygon, obj["properties"])	


# Replaces a tile object's Sprite2D (already in layer_node) by an AnimatedSprite2D playing the tile's
# animation: one animation "default" (looped, autoplay) with a frame per animation frame and its duration.
func replace_with_animated_sprite(sprite: Sprite2D, source: TileSetAtlasSource, coords: Vector2i, layer_node: Node) -> AnimatedSprite2D:
	var frames := SpriteFrames.new()
	frames.set_animation_speed("default", 1.0)
	frames.set_animation_loop("default", true)
	for i in range(source.get_tile_animation_frames_count(coords)):
		var tex := AtlasTexture.new()
		tex.atlas = source.texture
		tex.region = Rect2(source.get_tile_texture_region(coords, i))
		# With an animation speed of 1 fps a frame's relative duration is its duration in seconds
		frames.add_frame("default", tex, source.get_tile_animation_frame_duration(coords, i))
	var anim := AnimatedSprite2D.new()
	anim.sprite_frames = frames
	anim.animation = "default"
	anim.autoplay = "default"
	anim.position = sprite.position
	anim.rotation_degrees = sprite.rotation_degrees
	anim.scale = sprite.scale
	anim.offset = sprite.offset
	anim.centered = sprite.centered
	anim.visible = sprite.visible
	var sprite_name := sprite.name
	var index := sprite.get_index()
	sprite.owner = null
	layer_node.remove_child(sprite)
	sprite.free()
	layer_node.add_child(anim)
	layer_node.move_child(anim, index)
	anim.owner = _base_node
	anim.name = sprite_name
	return anim


# A shape object of an unknown class with unknown_class_fallback "node2d": a Node2D at the object's
# position and rotation, the shape as metadata "shape" in object coordinates (Rect2 for a rectangle,
# and the bounds of an ellipse or capsule; PackedVector2Array for a polygon or polyline) and "size".
func create_plain_node(obj: Dictionary, layer_node: Node, position: Vector2, class_string: String) -> void:
	var obj_id = obj.get("id", 0)
	var obj_name = str(obj.get("name", ""))
	var obj_width: float = obj.get("width", 0.0)
	var obj_height: float = obj.get("height", 0.0)
	var node := Node2D.new()
	layer_node.add_child(node)
	node.owner = _base_node
	node.name = obj_name if obj_name != "" else class_string if class_string != "" else "Node2D"
	node.position = position
	node.rotation_degrees = obj.get("rotation", 0.0)
	node.visible = obj.get("visible", true)
	if obj.has("polygon"):
		node.set_meta("shape", PackedVector2Array(polygon_from_array(obj["polygon"])))
	elif obj.has("polyline"):
		node.set_meta("shape", PackedVector2Array(polygon_from_array(obj["polyline"])))
	else:
		node.set_meta("shape", Rect2(0.0, 0.0, obj_width, obj_height))
	node.set_meta("size", Vector2(obj_width, obj_height))
	if _add_class_as_metadata and class_string != "":
		node.set_meta("class", class_string)
	if _add_id_as_metadata and obj_id != 0:
		node.set_meta("id", obj_id)
	if obj.has("properties"):
		handle_properties(node, obj["properties"])


# Creates the CollisionObject2D for a (non tile) shape object and adds it to layer_node.
# Types other than area/animatablebody/characterbody/rigidbody become a StaticBody2D.
func create_collision_object(godot_type, obj_name, layer_node: Node) -> CollisionObject2D:
	var co: CollisionObject2D
	var suffix: String
	var default_name: String
	match godot_type:
		_godot_type.AREA:
			co = Area2D.new()
			suffix = " (Area)"
			default_name = "Area"
		_godot_type.ABODY:
			co = AnimatableBody2D.new()
			suffix = " (AB)"
			default_name = "AnimatableBody"
		_godot_type.CBODY:
			co = CharacterBody2D.new()
			suffix = " (CB)"
			default_name = "CharacterBody"
		_godot_type.RBODY:
			co = RigidBody2D.new()
			suffix = " (RB)"
			default_name = "RigidBody"
		_:
			co = StaticBody2D.new()
			suffix = " (SB)"
			default_name = "StaticBody"
	layer_node.add_child(co)
	var name_string := str(obj_name)
	co.name = name_string + suffix if name_string != "" else default_name
	return co


func add_collision_shapes(parent: CollisionObject2D, object_group: Dictionary, tile_width: float, tile_height: float, flippedH: bool, flippedV: bool, scale: Vector2):
	var objects = object_group["objects"]
	for obj in objects:
		var obj_name = obj.get("name", "")
		if obj.has("point") and obj["point"]:
			print_rich("[color="+WARNING_COLOR+"] -- 'Point' has currently no corresponding collision element in Godot 4. -> Skipped[/color]")
			CommonUtils.warning_count += 1
			break

		var fact = tile_height / _map_tile_height
		var object_base_coords = Vector2(obj["x"], obj["y"]) * scale
		if _current_tileset_orientation == "isometric":
			object_base_coords = transpose_coords(obj["x"], obj["y"], true) * scale
			tile_width = _map_tile_width
			tile_height = _map_tile_height

		if obj.has("polygon"):
			var polygon_points = obj["polygon"] as Array
			var rot = obj.get("rotation", 0.0)			
			var polygon = []
			for pt in polygon_points:
				var p_coord = Vector2(pt["x"], pt["y"]) * scale
				if _current_tileset_orientation == "isometric":
					p_coord = transpose_coords(p_coord.x, p_coord.y, true)
				if flippedH:
					p_coord.x = -p_coord.x
				if flippedV:
					p_coord.y = -p_coord.y
				polygon.append(p_coord)

			var collision_polygon = CollisionPolygon2D.new()
			parent.add_child(collision_polygon)
			collision_polygon.owner = _base_node
			collision_polygon.polygon = polygon
			var pos_x = object_base_coords.x
			var pos_y = object_base_coords.y - tile_height
			if _map_orientation == "isometric" and _current_tileset_orientation == "orthogonal":
				pos_x -= tile_width / 2.0
			if flippedH:
				pos_x = tile_width - pos_x
				if _map_orientation == "isometric":
					pos_x -= tile_width
				rot = -rot
			if flippedV:
				pos_y = -tile_height - pos_y
				if _current_tileset_orientation == "isometric":
					pos_y -= _map_tile_height * fact - tile_height
				rot = -rot
			collision_polygon.rotation_degrees = rot
			collision_polygon.position = Vector2(pos_x, pos_y)
			collision_polygon.name = obj_name if obj_name != "" else "Collision Polygon"
			var one_way_props: Dictionary = get_properties(obj, ONE_WAY_PROPERTY_KEYS)
			if one_way_props.get("one_way:bool", "") == "true":
				collision_polygon.one_way_collision = true
			var coll_margin = one_way_props.get("one_way_margin:int", "")
			if coll_margin == "":
				coll_margin = one_way_props.get("one_way_margin:float", "")
			if coll_margin != "":
				collision_polygon.one_way_collision_margin = float(coll_margin)
		else:
			# Ellipse or rectangle
			var collision_shape = CollisionShape2D.new()
			parent.add_child(collision_shape)
			collision_shape.owner = _base_node
			var x = obj["x"] * scale.x
			var y = obj["y"] * scale.y
			var w = obj["width"] * scale.x
			var h = obj["height"] * scale.y
			var rot = obj.get("rotation", 0.0)
			var sin_a = sin(rot * PI / 180.0)
			var cos_a = cos(rot * PI / 180.0)
			var pos_x = x + w / 2.0 * cos_a - h / 2.0 * sin_a
			var pos_y = -tile_height + y + h / 2.0 * cos_a + w / 2.0 * sin_a
			if _current_tileset_orientation == "isometric":
				var trans_pos = transpose_coords(pos_x, pos_y, true)
				pos_x = trans_pos.x
				pos_y = trans_pos.y
				pos_x -= tile_width / 2.0 - h * fact / 4.0 * sin_a
				pos_y -= tile_height / 2.0
			elif _map_orientation == "isometric":
				pos_x -= tile_width / 2.0
			if flippedH:
				pos_x = tile_width - pos_x 
				if _map_orientation == "isometric":
					pos_x -= tile_width
				rot = -rot
			if flippedV:
				pos_y = -tile_height - pos_y
				if _current_tileset_orientation == "isometric":
					pos_y -= _map_tile_height * fact - tile_height
				rot = -rot
			collision_shape.position = Vector2(pos_x, pos_y)
			collision_shape.scale = scale
			var shape
			if (obj.has("capsule") and obj["capsule"]) or (obj.has("ellipse") and obj["ellipse"]):
				shape = CapsuleShape2D.new()
				if h >= w:
					shape.height = h / scale.y
					shape.radius = w / 2.0 / scale.x
				else:
					shape.height = w / scale.y
					shape.radius = h / 2.0 / scale.x
					rot += 90
				collision_shape.name = obj_name if obj_name != "" else "Capsule Shape"
			else:
				shape = RectangleShape2D.new()
				shape.size = Vector2(w, h) / scale
				collision_shape.name = obj_name if obj_name != "" else "Rectangle Shape"

			if _current_tileset_orientation == "isometric":
				if _iso_rot == 0.0:
					var q = float(_map_tile_height) / float(_map_tile_width)
					q *= q
					var cos_b = sqrt(1 / (q + 1))
					_iso_rot = acos(cos_b) * 180 / PI
					_iso_skew = (90 - 2 * _iso_rot) * PI / 180
					var scale_b = float(_map_tile_width) / (float(_map_tile_height) * 2 * cos_b)
					_iso_scale = Vector2(scale_b, scale_b)

				var effective_rot = _iso_rot
				var effective_skew = _iso_skew

				if w > h and shape is CapsuleShape2D:
					effective_rot = -_iso_rot
					effective_skew = -_iso_skew
					rot -= 90

				if flippedH:
					effective_rot = -effective_rot
					effective_skew = -effective_skew
				if flippedV:
					effective_rot = -effective_rot
					effective_skew = -effective_skew
	
				collision_shape.skew = effective_skew
				collision_shape.scale = _iso_scale
				rot += effective_rot

			collision_shape.shape = shape
			collision_shape.rotation_degrees = rot
			var one_way_props: Dictionary = get_properties(obj, ONE_WAY_PROPERTY_KEYS)
			if one_way_props.get("one_way:bool", "") == "true":
				collision_shape.one_way_collision = true
			var coll_margin = one_way_props.get("one_way_margin:int", "")
			if coll_margin == "":
				coll_margin = one_way_props.get("one_way_margin:float", "")
			if coll_margin != "":
				collision_shape.one_way_collision_margin = float(coll_margin)


static func is_true(value) -> bool:
	match typeof(value):
		TYPE_BOOL:
			return value
		TYPE_INT, TYPE_FLOAT:
			return value != 0
		TYPE_STRING, TYPE_STRING_NAME:
			var s := String(value).to_lower()
			return s == "1" or s == "true"
	return false


# Single pass variant of get_property for several properties at once.
# wanted_keys: Array of "<lowercase name>:<type>" strings.
# Returns a Dictionary "<lowercase name>:<type>" -> String value of the first matching property.
static func get_properties(obj: Dictionary, wanted_keys: Array) -> Dictionary:
	var ret = {}
	if not obj.has("properties"): return ret
	for property in obj["properties"]:
		var key: String = str(property.get("name", "")).to_lower() + ":" + str(property.get("type", "string"))
		if not ret.has(key) and key in wanted_keys:
			ret[key] = str(property.get("value", ""))
	return ret


static func get_property(obj: Dictionary, property_name: String, property_type: String) -> String:
	var ret = ""
	if not obj.has("properties"): return ret
	for property in obj["properties"]:
		var name = property.get("name", "")
		var type = property.get("type", "string")
		var val = str(property.get("value", ""))
		if name.to_lower() == property_name and type == property_type:
			return val
	return ret


func polygon_from_array(poly_array: Array):
	var polygon = []
	for pt in poly_array:
		var p_coord = transpose_coords(pt["x"], pt["y"], true)
		polygon.append(p_coord)
	return polygon


func polygon_from_rectangle(width: float, height: float):
	var polygon = [Vector2(), Vector2(), Vector2(), Vector2()]
	polygon[0] = Vector2.ZERO
	polygon[1].y = polygon[0].y + height
	polygon[1].x = polygon[0].x
	polygon[2].y = polygon[1].y
	polygon[2].x = polygon[0].x + width
	polygon[3].y = polygon[0].y
	polygon[3].x = polygon[2].x
	polygon[1] = transpose_coords(polygon[1].x, polygon[1].y, true)
	polygon[2] = transpose_coords(polygon[2].x, polygon[2].y, true)
	polygon[3] = transpose_coords(polygon[3].x, polygon[3].y, true)
	return polygon


func get_rotated_polygon(polygon: Array, rotation: float) -> Array:
	var ret = []
	var rot_rad = rotation * PI / 180.0
	for vertex in polygon:
		ret.append(vertex.rotated(rot_rad))
	return ret


func transpose_coords(x: float, y: float, no_offset_x: bool = false):
	if _map_orientation == "isometric":
		var trans_x = (x - y) * _map_tile_width / _map_tile_height / 2.0
		if not no_offset_x:
			trans_x += _map_height * _map_tile_width / 2.0
		var trans_y = (x + y) * 0.5
		return Vector2(trans_x, trans_y)

	return Vector2(x, y)


func get_first_gid_index(gid: int) -> int:
	var cached = _first_gid_index_cache.get(gid)
	if cached != null:
		return cached
	var index = 0
	var gid_index = 0
	for first_gid in _first_gids:
		if gid >= first_gid:
			gid_index = index
		index += 1
	_first_gid_index_cache[gid] = gid_index
	return gid_index


func get_atlas_source_index(gid: int) -> int:
	var cached = _atlas_index_cache.get(gid)
	if cached != null:
		return cached
	var ret: int = find_atlas_source_index(gid)
	_atlas_index_cache[gid] = ret
	return ret


func find_atlas_source_index(gid: int) -> int:
	if _atlas_sources == null:
		return -1
	var tileset_first_gid: int = _first_gids[get_first_gid_index(gid)] if not _first_gids.is_empty() else -1
	# Animation strips first: a strip stands in for its tile in the tileset's atlas or image collection
	var idx = -1
	for src in _atlas_sources:
		idx += 1
		if src.get("isStrip", false) and gid - int(src["firstGid"]) == int(src["assignedId"]) and int(src["firstGid"]) == tileset_first_gid:
			return idx
	idx = -1
	for src in _atlas_sources:
		idx += 1
		var first_gid: int = src["firstGid"]
		var effective_gid = gid - first_gid + 1
		var assigned_id: int = src["assignedId"]
		if assigned_id < 0:
			var limit: int = src["numTiles"]
			if effective_gid <= limit and first_gid == tileset_first_gid:
				return idx
		elif effective_gid == (assigned_id + 1):
			return idx
	return -1


func get_matching_source_id(gid: int):
	var idx = get_atlas_source_index(gid)
	if idx < 0:
		return -1
	return _atlas_sources[idx]["sourceId"]
	
	
func get_tile_offset(gid: int):
	var idx = get_atlas_source_index(gid)
	if idx < 0:
		return Vector2i.ZERO
	return _atlas_sources[idx]["tileOffset"]
		
	
func get_tileset_orientation(gid: int):
	var idx = get_atlas_source_index(gid)
	if idx < 0:
		return _map_orientation
	return _atlas_sources[idx]["tilesetOrientation"]
		
	
func get_tileset_alignment(gid: int):
	var idx = get_atlas_source_index(gid)
	if idx < 0:
		return DEFAULT_ALIGNMENT
	return _atlas_sources[idx]["objectAlignment"]	


func get_tileset_shader_material(gid: int):
	var idx = get_atlas_source_index(gid)
	if idx < 0:
		return null
	return _atlas_sources[idx]["shaderMaterial"]	


func is_partitioned_tileset(source_id: int) -> bool:
	for src in _atlas_sources:
		if src["sourceId"] == source_id:
			return (src["assignedId"] < 0)
	return false


func get_num_tiles_for_source_id(source_id: int):
	for src in _atlas_sources:
		if src["sourceId"] == source_id:
			return src["numTiles"]
	return -1


func get_object_group(index: int):
	var ret = null
	if _object_groups != null:
		ret = _object_groups.get(index, null)
	return ret


func get_object(index: int):
	if _objects_by_id == null:
		_objects_by_id = {}
		if _base_dictionary.has("layers"):
			index_objects(_base_dictionary["layers"])
	return _objects_by_id.get(index, null)


# Recursively indexes the objects of all layers (incl. those inside group layers) by id.
# The first object found for an id wins.
func index_objects(layers: Array) -> void:
	for layer in layers:
		if layer.has("objects"):
			for obj in layer["objects"]:
				var obj_id: int = int(obj.get("id", 0))
				if not _objects_by_id.has(obj_id):
					_objects_by_id[obj_id] = obj
		if layer.has("layers"):
			index_objects(layer["layers"])


func get_object_polygon(obj_id: int):
	var obj_ = get_object(obj_id)
	if obj_ == null:
		return null
	var obj = obj_ as Dictionary
	var obj_x = obj.get("x", 0.0)
	var obj_y = obj.get("y", 0.0)
	var obj_rot = obj.get("rotation", 0.0)
	var ret = null
	if obj.has("polygon"):
		var pg = get_rotated_polygon(polygon_from_array(obj["polygon"]), obj_rot)
		var i = 0
		for vertex in pg:
			pg[i] += Vector2(obj_x, obj_y)
			i += 1
		ret = PackedVector2Array(pg)
	elif obj.has("polyline"):
		var pl = get_rotated_polygon(polygon_from_array(obj["polyline"]), obj_rot)
		var i = 0
		for vertex in pl:
			pl[i] += Vector2(obj_x, obj_y)
			i += 1
		ret = PackedVector2Array(pl)
	elif obj.has("point"):
		ret = Vector2(obj_x, obj_y)
	elif not obj.has("ellipse") and not obj.has("capsule") and not obj.has("gid"):
		# Should be a rectangle
		var obj_width = obj.get("width", 0.0)
		var obj_height = obj.get("height", 0.0)
		var rect = []
		rect.append(Vector2(0.0, 0.0))
		rect.append(Vector2(obj_width, 0.0))
		rect.append(Vector2(obj_width, obj_height))
		rect.append(Vector2(0.0, obj_height))
		var rot_rect = get_rotated_polygon(rect, obj_rot)
		var i = 0
		for vertex in rot_rect:
			rot_rect[i] += Vector2(obj_x, obj_y)
			i += 1
		ret = PackedVector2Array(rot_rect)
	return ret


func handle_list_property(current_array: Array, current_value: Array):
	for item in current_value:
		var item_type: String = item.get("type", "string")
		var item_val = item["value"]
		if item_type == "list":
			var list_val = []
			current_array.append(list_val)
			handle_list_property(list_val, item_val)
		elif item_type == "object":
			var obj = get_object_polygon(item_val)
			if obj != null:
				current_array.append(obj)
			else:
				current_array.append(CommonUtils.get_right_typed_value(item_type, str(item_val)))
		elif item_type == "class" and item_val is Dictionary:
			# Class typed items keep their member dictionary (see contract C4)
			current_array.append(item_val)
		else:
			current_array.append(CommonUtils.get_right_typed_value(item_type, str(item_val)))


# The value of a colour property: a Tiled colour (type 'color', #AARRGGBB) or a string as Godot
# reads it (#RRGGBB, #RRGGBBAA or a colour name), as before colour-typed values were accepted.
static func property_color(type: String, val: String) -> Color:
	if type == "color":
		return CommonUtils.tiled_color(val)
	return Color(val)


func handle_properties(target_node: Node, properties: Array):
	var has_children = false
	if target_node is StaticBody2D or target_node is Area2D or target_node is CharacterBody2D or target_node is RigidBody2D or target_node is AnimatableBody2D:
		has_children = target_node.get_child_count() > 0 
	for property in properties:
		var name: String = property.get("name", "")
		var type: String = property.get("type", "string")
		var list_val = ""
		var val: String = ""
		# Class typed property: value is a Dictionary {member_name: member_value} (see contract C4)
		var class_val = property.get("value", {}) if type == "class" else null
		var is_class_dict: bool = class_val is Dictionary
		if type == "list":
			list_val = []
			handle_list_property(list_val, property["value"])
		elif not is_class_dict:
			val = str(property.get("value", ""))
		var lname := name.to_lower()
		if name == "" or lname == GODOT_NODE_TYPE_PROPERTY or lname == "res_path": continue
		if name.begins_with("__") and has_children:
			var child_prop_dict = {}
			child_prop_dict["name"] = name.substr(2)
			child_prop_dict["type"] = type
			# Lists and class dictionaries are forwarded unchanged
			child_prop_dict["value"] = property["value"] if type == "list" or is_class_dict else val
			var child_props = []
			child_props.append(child_prop_dict)
			for child in target_node.get_children():
				handle_properties(child, child_props)
		
		# Node properties
		# v1.5.4: godot_group property
		if lname == GODOT_GROUP_PROPERTY and type == "string":
			for group in val.split(",", false):
				target_node.add_to_group(group.strip_edges(), true)

		# v1.6.6: script resource and godot_script property
		elif lname == GODOT_SCRIPT_PROPERTY and (type == "file" or type == "string"):
			var path = val
			if type == "file":
				path = CommonUtils.cleanup_path(_base_path.replace("res://", "") + "/" + val)
			target_node.set_script(load(path))

		# CanvasItem properties
		elif lname == "modulate" and (type == "string" or type == "color"):
			target_node.modulate = property_color(type, val)
		elif lname == "self_modulate" and (type == "string" or type == "color"):
			target_node.self_modulate = property_color(type, val)
		elif lname == "show_behind_parent" and type == "bool":
			target_node.show_behind_parent = val.to_lower() == "true"
		elif lname == "top_level" and type == "bool":
			target_node.top_level = val.to_lower() == "true"
		elif lname == "clip_children" and type == "int":
			if int(val) < CanvasItem.CLIP_CHILDREN_MAX:
				target_node.clip_children = int(val)
		elif lname == "light_mask" and type == "string":
			target_node.light_mask = CommonUtils.get_bitmask_integer_from_string(val, 20)
		elif lname == "visibility_layer" and type == "string":
			target_node.visibility_layer = CommonUtils.get_bitmask_integer_from_string(val, 20)
		elif lname == "z_index" and type == "int":
			target_node.z_index = int(val)
		elif lname == "z_as_relative" and type == "bool":
			target_node.z_as_relative = val.to_lower() == "true"
		elif lname == "y_sort_enabled" and type == "bool":
			target_node.y_sort_enabled = val.to_lower() == "true"
		elif lname == "texture_filter" and type == "int":
			if int(val) < CanvasItem.TEXTURE_FILTER_MAX:
				target_node.texture_filter = int(val)
		elif lname == "texture_repeat" and type == "int":
			if int(val) < CanvasItem.TEXTURE_REPEAT_MAX:
				target_node.texture_repeat = int(val)
		elif lname == "material" and type == "file":
			target_node.material = DataLoader.load_resource_from_file(val, _base_path)
		elif lname == "use_parent_material" and type == "bool":
			target_node.use_parent_material = val.to_lower() == "true"
	
		# TileMapLayer properties
		elif lname == "tile_set" and type == "file" and target_node is TileMapLayer:
			target_node.tile_set = DataLoader.load_resource_from_file(val, _base_path)

		# Experimental only! Not sure if properly functioning
		elif lname == "tileset_resource_path" and type == "string" and target_node is TileMapLayer:
			var tileset = target_node.tile_set
			tileset.resource_path = val

		elif lname == "y_sort_origin" and type == "int" and target_node is TileMapLayer:
			target_node.y_sort_origin = int(val)
		elif lname == "x_draw_order_reversed" and type == "bool" and target_node is TileMapLayer:
			target_node.x_draw_order_reversed = val.to_lower() == "true"
		elif lname == "rendering_quadrant_size" and type == "int" and target_node is TileMapLayer:
			target_node.rendering_quadrant_size = int(val)
		elif lname == "collision_enabled" and type == "bool" and target_node is TileMapLayer:
			target_node.collision_enabled = val.to_lower() == "true"
		elif lname == "use_kinematic_bodies" and type == "bool" and target_node is TileMapLayer:
			target_node.use_kinematic_bodies = val.to_lower() == "true"
		elif lname == "collision_visibility_mode" and type == "int" and target_node is TileMapLayer:
			if int(val) < 3:
				target_node.collision_visibility_mode = int(val)
		elif lname == "navigation_enabled" and type == "bool" and target_node is TileMapLayer:
			target_node.navigation_enabled = val.to_lower() == "true"
		elif lname == "navigation_visibility_mode" and type == "int" and target_node is TileMapLayer:
			if int(val) < 3:
				target_node.navigation_visibility_mode = int(val)

		# Parallax2D properties
		elif lname == "scroll_scale_x" and (type == "float" or type == "int") and target_node is Parallax2D:
			target_node.scroll_scale = Vector2(float(val), target_node.scroll_scale.y)
		elif lname == "scroll_scale_y" and (type == "float" or type == "int") and target_node is Parallax2D:
			target_node.scroll_scale = Vector2(target_node.scroll_scale.x, float(val))
		elif lname == "scroll_offset_x" and (type == "float" or type == "int") and target_node is Parallax2D:
			target_node.scroll_offset = Vector2(float(val), target_node.scroll_offset.y)
		elif lname == "scroll_offset_y" and (type == "float" or type == "int") and target_node is Parallax2D:
			target_node.scroll_offset = Vector2(target_node.scroll_offset.x, float(val))
		elif lname == "repeat_size_x" and (type == "float" or type == "int") and target_node is Parallax2D:
			target_node.repeat_size = Vector2(float(val), target_node.repeat_size.y)
		elif lname == "repeat_size_y" and (type == "float" or type == "int") and target_node is Parallax2D:
			target_node.repeat_size = Vector2(target_node.repeat_size.x, float(val))
		elif lname == "autoscroll_x" and (type == "float" or type == "int") and target_node is Parallax2D:
			target_node.autoscroll = Vector2(float(val), target_node.autoscroll.y)
		elif lname == "autoscroll_y" and (type == "float" or type == "int") and target_node is Parallax2D:
			target_node.autoscroll = Vector2(target_node.autoscroll.x, float(val))
		elif lname == "repeat_times" and type == "int" and target_node is Parallax2D:
			target_node.repeat_times = int(val)
		elif lname == "limit_begin_x" and (type == "float" or type == "int") and target_node is Parallax2D:
			target_node.limit_begin = Vector2(float(val), target_node.limit_begin.y)
		elif lname == "limit_begin_y" and (type == "float" or type == "int") and target_node is Parallax2D:
			target_node.limit_begin = Vector2(target_node.limit_begin.x, float(val))
		elif lname == "limit_end_x" and (type == "float" or type == "int") and target_node is Parallax2D:
			target_node.limit_end = Vector2(float(val), target_node.limit_end.y)
		elif lname == "limit_end_y" and (type == "float" or type == "int") and target_node is Parallax2D:
			target_node.limit_end = Vector2(target_node.limit_end.x, float(val))
		elif lname == "follow_viewport" and type == "bool" and target_node is Parallax2D:
			target_node.follow_viewport = val.to_lower() == "true"
		elif lname == "ignore_camera_scroll" and type == "bool" and target_node is Parallax2D:
			target_node.ignore_camera_scroll = val.to_lower() == "true"
		elif lname == "screen_offset_x" and (type == "float" or type == "int") and target_node is Parallax2D:
			target_node.screen_offset = Vector2(float(val), target_node.screen_offset.y)
		elif lname == "screen_offset_y" and (type == "float" or type == "int") and target_node is Parallax2D:
			target_node.screen_offset = Vector2(target_node.screen_offset.x, float(val))
		# CollisionObject2D properties
		elif lname == "disable_mode" and type == "int" and target_node is CollisionObject2D:
			if int(val) < 3:
				target_node.disable_mode = int(val)
		elif lname == "collision_layer" and type == "string" and target_node is CollisionObject2D:
			target_node.collision_layer = CommonUtils.get_bitmask_integer_from_string(val, 32)
		elif lname == "collision_mask" and type == "string" and target_node is CollisionObject2D:
			target_node.collision_mask = CommonUtils.get_bitmask_integer_from_string(val, 32)
		elif lname == "collision_priority" and  (type == "float" or type == "int") and target_node is CollisionObject2D:
			target_node.collision_priority = float(val)
		elif lname == "input_pickable" and type == "bool" and target_node is CollisionObject2D:
			target_node.input_pickable = val.to_lower() == "true"

		# CollisionPolygon2D properties
		elif lname == "build_mode" and type == "int" and has_children and int(val) < 2:
			for child in target_node.get_children():
				if child is CollisionPolygon2D:
					child.build_mode = int(val)

		# CollisionPolygon2D & CollisionShape2D properties
		elif lname == "disabled" and type == "bool" and has_children:
			for child in target_node.get_children():
				child.disabled = val.to_lower() == "true"
		elif lname == "one_way_collision" and type == "bool" and has_children:
			for child in target_node.get_children():
				child.one_way_collision = val.to_lower() == "true"
		elif lname == "one_way_collision_margin" and (type == "float" or type == "int") and has_children:
			for child in target_node.get_children():
				child.one_way_collision_margin = float(val)

		# CollisionShape2D properties
		elif lname == "debug_color" and (type == "string" or type == "color") and has_children:
			for child in target_node.get_children():
				if child is CollisionShape2D:
					child.debug_color = property_color(type, val)

		# Area2D properties
		elif lname == "monitoring" and type == "bool" and target_node is Area2D:
			target_node.monitoring = val.to_lower() == "true"
		elif lname == "monitorable" and type == "bool" and target_node is Area2D:
			target_node.monitorable = val.to_lower() == "true"
		elif lname == "priority" and (type == "float" or type == "int") and target_node is Area2D:
			target_node.priority = float(val)
		elif lname == "gravity_space_override" and type == "int" and target_node is Area2D:
			if int(val) < 5:
				target_node.gravity_space_override = int(val)
		elif lname == "gravity_point" and type == "bool" and target_node is Area2D:
			target_node.gravity_point = val.to_lower() == "true"
		elif lname == "gravity_point_center_x" and (type == "float" or type == "int") and target_node is Area2D:
			target_node.gravity_point_center = Vector2(float(val), target_node.gravity_point_center.y)
		elif lname == "gravity_point_center_y" and (type == "float" or type == "int") and target_node is Area2D:
			target_node.gravity_point_center = Vector2(target_node.gravity_point_center.x, float(val))
		elif lname == "gravity_point_unit_distance" and (type == "float" or type == "int") and target_node is Area2D:
			target_node.gravity_point_unit_distance = float(val)
		elif lname == "gravity_direction_x" and (type == "float" or type == "int") and target_node is Area2D:
			target_node.gravity_direction = Vector2(float(val), target_node.gravity_direction.y)
		elif lname == "gravity_direction_y" and (type == "float" or type == "int") and target_node is Area2D:
			target_node.gravity_direction = Vector2(target_node.gravity_direction.x, float(val))
		elif lname == "gravity" and (type == "float" or type == "int") and target_node is Area2D:
			target_node.gravity = float(val)
		elif lname == "linear_damp_space_override" and type == "int" and target_node is Area2D:
			if int(val) < 5:
				target_node.linear_damp_space_override = int(val)
		elif lname == "linear_damp" and (type == "float" or type == "int") and target_node is Area2D:
			target_node.linear_damp = float(val)
		elif lname == "angular_damp_space_override" and type == "int" and target_node is Area2D:
			if int(val) < 5:
				target_node.angular_damp_space_override = int(val)
		elif lname == "angular_damp" and (type == "float" or type == "int") and target_node is Area2D:
			target_node.angular_damp = float(val)
			
		# StaticBody2D properties
		elif lname == "physics_material_override" and type == "file" and target_node is StaticBody2D:
			target_node.physics_material_override = DataLoader.load_resource_from_file(val, _base_path)
		elif lname == "constant_linear_velocity_x" and (type == "float" or type == "int") and target_node is StaticBody2D:
			target_node.constant_linear_velocity = Vector2(float(val), target_node.constant_linear_velocity.y)
		elif lname == "constant_linear_velocity_y" and (type == "float" or type == "int") and target_node is StaticBody2D:
			target_node.constant_linear_velocity = Vector2(target_node.constant_linear_velocity.x, float(val))
		elif lname == "constant_angular_velocity" and (type == "float" or type == "int") and target_node is StaticBody2D:
			target_node.constant_angular_velocity = float(val)

		# AnimatableBody2D properties
		elif lname == "sync_to_physics" and type == "bool" and target_node is AnimatableBody2D:
			target_node.sync_to_physics = val.to_lower() == "true"

		# CharacterBody2D properties
		elif lname == "motion_mode" and type == "int" and target_node is CharacterBody2D:
			if int(val) < 2:
				target_node.motion_mode = int(val)
		elif lname == "up_direction_x" and (type == "float" or type == "int") and target_node is CharacterBody2D:
			target_node.up_direction = Vector2(float(val), target_node.up_direction.y)
		elif lname == "up_direction_y" and (type == "float" or type == "int") and target_node is CharacterBody2D:
			target_node.up_direction = Vector2(target_node.up_direction.x, float(val))
		elif lname == "slide_on_ceiling" and type == "bool" and target_node is CharacterBody2D:
			target_node.slide_on_ceiling = val.to_lower() == "true"
		elif lname == "wall_min_slide_angle" and (type == "float" or type == "int") and target_node is CharacterBody2D:
			target_node.wall_min_slide_angle = float(val)
		elif lname == "floor_stop_on_slope" and type == "bool" and target_node is CharacterBody2D:
			target_node.floor_stop_on_slope = val.to_lower() == "true"
		elif lname == "floor_constant_speed" and type == "bool" and target_node is CharacterBody2D:
			target_node.floor_constant_speed = val.to_lower() == "true"
		elif lname == "floor_block_on_wall" and type == "bool" and target_node is CharacterBody2D:
			target_node.floor_block_on_wall = val.to_lower() == "true"
		elif lname == "floor_max_angle" and (type == "float" or type == "int") and target_node is CharacterBody2D:
			target_node.floor_max_angle = float(val)
		elif lname == "floor_snap_length" and (type == "float" or type == "int") and target_node is CharacterBody2D:
			target_node.floor_snap_length = float(val)
		elif lname == "platform_on_leave" and type == "int" and target_node is CharacterBody2D:
			if int(val) < 3:
				target_node.platform_on_leave = int(val)
		elif lname == "platform_floor_layers" and type == "string" and target_node is CharacterBody2D:
			target_node.platform_floor_layers = CommonUtils.get_bitmask_integer_from_string(val, 32)
		elif lname == "platform_wall_layers" and type == "string" and target_node is CharacterBody2D:
			target_node.platform_wall_layers = CommonUtils.get_bitmask_integer_from_string(val, 32)
		elif lname == "safe_margin" and  (type == "float" or type == "int") and target_node is CharacterBody2D:
			target_node.safe_margin = float(val)
		elif lname == "collision_layer" and type == "string" and target_node is CharacterBody2D:
			target_node.collision_layer = CommonUtils.get_bitmask_integer_from_string(val, 32)
		elif lname == "collision_mask" and type == "string" and target_node is CharacterBody2D:
			target_node.collision_mask = CommonUtils.get_bitmask_integer_from_string(val, 32)
		elif lname == "collision_priority" and  (type == "float" or type == "int") and target_node is CharacterBody2D:
			target_node.collision_priority = float(val)

		# RigidBody2D properties
		elif lname == "mass" and  (type == "float" or type == "int") and target_node is RigidBody2D:
			target_node.mass = float(val)
		elif lname == "inertia" and  (type == "float" or type == "int") and target_node is RigidBody2D:
			target_node.inertia = float(val)
		elif lname == "center_of_mass_mode" and type == "int" and target_node is RigidBody2D:
			if int(val) < 2:
				target_node.center_of_mass_mode = int(val)
		elif lname == "physics_material_override" and type == "file" and target_node is RigidBody2D:
			target_node.physics_material_override = DataLoader.load_resource_from_file(val, _base_path)
		elif lname == "gravity_scale" and  (type == "float" or type == "int") and target_node is RigidBody2D:
			target_node.gravity_scale = float(val)
		elif lname == "custom_integrator" and type == "bool" and target_node is RigidBody2D:
			target_node.custom_integrator = val.to_lower() == "true"
		elif lname == "continuous_cd" and type == "int" and target_node is RigidBody2D:
			if int(val) < 3:
				target_node.continuous_cd = int(val)
		elif lname == "max_contacts_reported" and type == "int" and target_node is RigidBody2D:
			target_node.max_contacts_reported = int(val)
		elif lname == "contact_monitor" and type == "bool" and target_node is RigidBody2D:
			target_node.contact_monitor = val.to_lower() == "true"
		elif lname == "sleeping" and type == "bool" and target_node is RigidBody2D:
			target_node.sleeping = val.to_lower() == "true"
		elif lname == "can_sleep" and type == "bool" and target_node is RigidBody2D:
			target_node.can_sleep = val.to_lower() == "true"
		elif lname == "lock_rotation" and type == "bool" and target_node is RigidBody2D:
			target_node.lock_rotation = val.to_lower() == "true"
		elif lname == "freeze" and type == "bool" and target_node is RigidBody2D:
			target_node.freeze = val.to_lower() == "true"
		elif lname == "freeze_mode" and type == "int" and target_node is RigidBody2D:
			if int(val) < 2:
				target_node.freeze_mode = int(val)
		elif lname == "linear_velocity_x" and (type == "float" or type == "int") and target_node is RigidBody2D:
			target_node.linear_velocity = Vector2(float(val), target_node.linear_velocity.y)
		elif lname == "linear_velocity_y" and (type == "float" or type == "int") and target_node is RigidBody2D:
			target_node.linear_velocity = Vector2(target_node.linear_velocity.x, float(val))
		elif lname == "linear_damp_mode" and type == "int" and target_node is RigidBody2D:
			if int(val) < 2:
				target_node.linear_damp_mode = int(val)
		elif lname == "linear_damp" and  (type == "float" or type == "int") and target_node is RigidBody2D:
			target_node.linear_damp = float(val)
		elif lname == "angular_velocity" and (type == "float" or type == "int") and target_node is RigidBody2D:
			target_node.angular_velocity = float(val)
		elif lname == "angular_damp_mode" and type == "int" and target_node is RigidBody2D:
			if int(val) < 2:
				target_node.angular_damp_mode = int(val)
		elif lname == "angular_damp" and  (type == "float" or type == "int") and target_node is RigidBody2D:
			target_node.angular_damp = float(val)
		elif lname == "constant_force_x" and (type == "float" or type == "int") and target_node is RigidBody2D:
			target_node.constant_force = Vector2(float(val), target_node.constant_force.y)
		elif lname == "constant_force_y" and (type == "float" or type == "int") and target_node is RigidBody2D:
			target_node.constant_force = Vector2(target_node.constant_force.x, float(val))
		elif lname == "constant_torque" and (type == "float" or type == "int") and target_node is RigidBody2D:
			target_node.constant_torque = float(val)
				
		# NavigationRegion2D properties
		elif lname == "navigation_polygon" and type == "file" and target_node is NavigationRegion2D:
			target_node.navigation_polygon = DataLoader.load_resource_from_file(val, _base_path)
		elif lname == "enabled" and type == "bool" and target_node is NavigationRegion2D:
			target_node.enabled = val.to_lower() == "true"
		elif lname == "navigation_layers" and type == "string" and target_node is NavigationRegion2D:
			target_node.navigation_layers = CommonUtils.get_bitmask_integer_from_string(val, 32)
		elif lname == "use_edge_connection" and type == "bool" and target_node is NavigationRegion2D:
			target_node.use_edge_connection = val.to_lower() == "true"
		elif lname == "enter_cost" and (type == "float" or type == "int") and target_node is NavigationRegion2D:
			target_node.enter_cost = float(val)
		elif lname == "travel_cost" and (type == "float" or type == "int") and target_node is NavigationRegion2D:
			target_node.travel_cost = float(val)

		# NavigationPolygon properties
		elif lname == "parsed_geometry_type" and type == "int" and target_node is NavigationRegion2D:
			if int(val) < 3:
				var nav_poly = target_node.navigation_polygon
				nav_poly.parsed_geometry_type = int(val)
		elif lname == "parsed_collision_mask" and type == "string" and target_node is NavigationRegion2D:
			var nav_poly = target_node.navigation_polygon
			nav_poly.parsed_collision_mask = CommonUtils.get_bitmask_integer_from_string(val, 32)
		elif lname == "source_geometry_mode" and type == "int" and target_node is NavigationRegion2D:
			if int(val) < 3:
				var nav_poly = target_node.navigation_polygon
				nav_poly.source_geometry_mode = int(val)
		elif lname == "source_geometry_group_name" and type == "string" and target_node is NavigationRegion2D:
			var nav_poly = target_node.navigation_polygon
			nav_poly.source_geometry_group_name = val
		elif lname == "cell_size" and (type == "float" or type == "int") and target_node is NavigationRegion2D:
			var nav_poly = target_node.navigation_polygon
			nav_poly.cell_size = float(val)
		elif lname == "border_size" and (type == "float" or type == "int") and target_node is NavigationRegion2D:
			var nav_poly = target_node.navigation_polygon
			nav_poly.border_size = float(val)
		elif lname == "agent_radius" and (type == "float" or type == "int") and target_node is NavigationRegion2D:
			var nav_poly = target_node.navigation_polygon
			nav_poly.agent_radius = float(val)
		elif lname == "baking_rect_x" and (type == "float" or type == "int") and target_node is NavigationRegion2D:
			var nav_poly = target_node.navigation_polygon
			var baking_rect = nav_poly.baking_rect
			var br_pos = baking_rect.position
			br_pos.x = float(val)
			baking_rect.position = br_pos
			nav_poly.baking_rect = baking_rect
		elif lname == "baking_rect_y" and (type == "float" or type == "int") and target_node is NavigationRegion2D:
			var nav_poly = target_node.navigation_polygon
			var baking_rect = nav_poly.baking_rect
			var br_pos = baking_rect.position
			br_pos.y = float(val)
			baking_rect.position = br_pos
			nav_poly.baking_rect = baking_rect
		elif lname == "baking_rect_w" and (type == "float" or type == "int") and target_node is NavigationRegion2D:
			var nav_poly = target_node.navigation_polygon
			var baking_rect = nav_poly.baking_rect
			var br_size = baking_rect.size
			br_size.x = float(val)
			baking_rect.size = br_size
			nav_poly.baking_rect = baking_rect
		elif lname == "baking_rect_h" and (type == "float" or type == "int") and target_node is NavigationRegion2D:
			var nav_poly = target_node.navigation_polygon
			var baking_rect = nav_poly.baking_rect
			var br_size = baking_rect.size
			br_size.y = float(val)
			baking_rect.size = br_size
			nav_poly.baking_rect = baking_rect
		elif lname == "baking_rect_offset_x" and (type == "float" or type == "int") and target_node is NavigationRegion2D:
			var nav_poly = target_node.navigation_polygon
			var baking_rect_offset = nav_poly.baking_rect_offset
			baking_rect_offset.x = float(val)
			nav_poly.baking_rect_offset = baking_rect_offset
		elif lname == "baking_rect_offset_y" and (type == "float" or type == "int") and target_node is NavigationRegion2D:
			var nav_poly = target_node.navigation_polygon
			var baking_rect_offset = nav_poly.baking_rect_offset
			baking_rect_offset.y = float(val)
			nav_poly.baking_rect_offset = baking_rect_offset
		elif lname == "resource_local_to_scene" and type == "bool" and target_node is NavigationRegion2D:
			var nav_poly = target_node.navigation_polygon
			nav_poly.resource_local_to_scene = val.to_lower() == "true"
		elif lname == "resource_name" and type == "string" and target_node is NavigationRegion2D:
			var nav_poly = target_node.navigation_polygon
			nav_poly.resource_name = val

		# LightOccluder2D properties
		elif lname == "sdf_collision" and type == "bool" and target_node is LightOccluder2D:
			target_node.sdf_collision = val.to_lower() == "true"
		elif lname == "occluder_light_mask" and type == "string" and target_node is LightOccluder2D:
			target_node.occluder_light_mask = CommonUtils.get_bitmask_integer_from_string(val, 20)

		# Polygon2D properties
		elif lname == "color" and (type == "string" or type == "color") and target_node is Polygon2D:
			target_node.color = property_color(type, val)

		# Line2D properties
		elif lname == "width" and (type == "float" or type == "int") and target_node is Line2D:
			target_node.width = float(val)
		elif lname == "default_color" and (type == "string" or type == "color") and target_node is Line2D:
			target_node.default_color = property_color(type, val)

		# Marker2D properties
		elif lname == "gizmo_extents" and (type == "float" or type == "int") and target_node is Marker2D:
			target_node.gizmo_extents = float(val)

		# Other properties are added as Metadata
		else:
			if type == "object":
				var obj = get_object_polygon(int(val))
				if obj != null:
					target_node.set_meta(name, obj)
				else:
					target_node.set_meta(name, CommonUtils.get_right_typed_value(type, val))
			elif type == "list":
				target_node.set_meta(name, list_val)
			elif is_class_dict:
				target_node.set_meta(name, class_val)
			else:
				target_node.set_meta(name, CommonUtils.get_right_typed_value(type, val))
