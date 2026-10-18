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

const CommonUtils := preload("CommonUtils.gd")
const CustomTypes := preload("CustomTypes.gd")
const DataLoader := preload("DataLoader.gd")
const TilemapCreator := preload("TilemapCreator.gd")
const PostProcessing := preload("PostProcessing.gd")

# Import options: set them on the Importer instance before calling one of the import methods.
# Use Godot's default texture filter instead of 'nearest' on the created layers.
var use_default_filter: bool = false
# Store a Tiled 'class' as metadata "class" on the created nodes/tiles.
var add_class_as_metadata: bool = true
# Store the Tiled object id as metadata "id" on the created nodes.
var add_id_as_metadata: bool = false
# Use the engine's transform flags for flipped/rotated tiles instead of creating alternative tiles.
var no_alternative_tiles: bool = true
# Map each wangset to a terrain instead of to a terrain set (old mapping).
var map_wangset_to_terrain: bool = false
# Tile properties starting with this prefix become TileSet custom data layers ("" = all tile properties).
var custom_data_prefix: String = "data_"
# If not empty the created TileSet is saved to this resource path.
var save_tileset_to: String = ""
# How layers with a parallax factor are created: "parallax2d" (wrapped in a Parallax2D at their place
# in the layer hierarchy) or "legacy" (moved into one ParallaxBackground as ParallaxLayers, as before 3.1).
var parallax_mode: String = "parallax2d"
# How tile animations are created: "strip" (each animation gets an atlas of its frames in their order,
# so any frame sequence works) or "atlas" (Godot's animation inside the tileset's atlas, as before 3.1:
# only for frames that follow the tile at equal steps in a row or column).
var animation_mode: String = "strip"
# Return a Node2D root also for a map with a single layer (the layer then is its child and keeps its
# own properties). Off: such a map returns the layer itself, with the map's properties applied on top.
var always_wrap_root: bool = false
# What an object of a class that is no Godot node type becomes: "staticbody", any other class
# name of the Reference's mapping table (e.g. "area"), or "node2d" (a Node2D with the shape as metadata).
var unknown_class_fallback: String = "staticbody"

# Results of the last import (any of the import methods)
var last_error_count: int = 0
var last_warning_count: int = 0


# Imports a Tiled map (.tmx/.tmj) from the file system (res://, user:// or absolute path).
# Returns the root node of the created tree (no parent) or null on a fatal error.
func import(source_file: String, project_file: String = ""):
	return _import(source_file, project_file, "", "")


# Imports a Tiled map from a zip archive. All paths except the zip path are paths inside the archive.
# The archive is used for this call only, later calls of import() read from the file system again.
func import_from_zip(zip_file: String, source_file_in_zip: String, project_file_in_zip: String = ""):
	return _import(source_file_in_zip, project_file_in_zip, zip_file, "")


# If you don't need post-processing better remove the methods below and also PostProcessing.gd to minimize useless code in your build
func import_with_post_processing(source_file: String, post_proc_file: String, project_file: String = ""):
	return _import(source_file, project_file, "", post_proc_file)


# Note: the post-processing script is loaded with load() i.e. it has to be a project resource, it is not read from the zip.
func import_from_zip_with_post_processing(zip_file: String, source_file_in_zip: String, post_proc_file_in_zip: String, project_file_in_zip: String = ""):
	return _import(source_file_in_zip, project_file_in_zip, zip_file, post_proc_file_in_zip)


func _import(source_file: String, project_file: String, zip_file: String, post_proc_file: String):
	CommonUtils.error_count = 0
	CommonUtils.warning_count = 0
	var ret = null
	if DataLoader.begin_import(zip_file) == OK:
		ret = _create(source_file, project_file, zip_file)
		if ret != null and post_proc_file != "":
			var post_proc = PostProcessing.new()
			ret = post_proc.call_post_process(ret, post_proc_file)
			if post_proc.get_error() != OK:
				CommonUtils.error_count += 1
	DataLoader.end_import()

	if ret == null and CommonUtils.error_count == 0:
		# Fatal errors reported by the creator itself are not counted there
		CommonUtils.error_count = 1
	last_error_count = CommonUtils.error_count
	last_warning_count = CommonUtils.warning_count
	if last_error_count == 0 and last_warning_count == 0:
		print("Import of '" + source_file + "' succeeded.")
	else:
		print("Import of '" + source_file + "' finished with " + str(last_error_count) + " error(s) and " + str(last_warning_count) + " warning(s).")
	return ret


func _create(source_file: String, project_file: String, zip_file: String):
	if zip_file == "":
		if not FileAccess.file_exists(source_file):
			printerr("ERROR: Tiled map file '" + source_file + "' not found.")
			CommonUtils.error_count += 1
			return null
		if project_file != "" and not FileAccess.file_exists(project_file):
			printerr("ERROR: Tiled project file '" + project_file + "' not found.")
			CommonUtils.error_count += 1
			return null

	var tilemap_creator = TilemapCreator.new()
	tilemap_creator.set_use_default_filter(use_default_filter)
	tilemap_creator.set_add_class_as_metadata(add_class_as_metadata)
	tilemap_creator.set_add_id_as_metadata(add_id_as_metadata)
	tilemap_creator.set_no_alternative_tiles(no_alternative_tiles)
	tilemap_creator.set_map_wangset_to_terrain(map_wangset_to_terrain)
	tilemap_creator.set_custom_data_prefix(custom_data_prefix)
	tilemap_creator.set_save_tileset_to(save_tileset_to)
	tilemap_creator.set_parallax_mode(parallax_mode)
	tilemap_creator.set_always_wrap_root(always_wrap_root)
	tilemap_creator.set_animation_mode(animation_mode)
	tilemap_creator.set_unknown_class_fallback(unknown_class_fallback)

	if project_file != "":
		var ct = CustomTypes.new()
		ct.load_custom_types(project_file)
		tilemap_creator.set_custom_types(ct)
	return tilemap_creator.create(source_file)
