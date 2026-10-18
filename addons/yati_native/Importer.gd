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


# The native importer's facade: the GDScript runtime's Importer API (runtime/Importer.gd) over
# the GDExtension's classes (TiledMap, TiledTileSets, TiledBuilder, TiledImportOptions), so a
# project switches importers by changing one preload path. docs/native-importer-plan.md, stage 2.
#
# The native classes are reached through ClassDB, never by name, so this script still compiles
# on a platform without a library (an import then fails with a message instead).
extends RefCounted

const CommonUtils := preload("CommonUtils.gd")
const PostProcessing := preload("PostProcessing.gd")

const NATIVE_CLASS: String = "TiledNative"
const OPTION_NAMES: Array[String] = [
	"use_default_filter", "add_class_as_metadata", "add_id_as_metadata", "no_alternative_tiles",
	"map_wangset_to_terrain", "custom_data_prefix", "save_tileset_to", "parallax_mode",
	"animation_mode", "always_wrap_root", "unknown_class_fallback",
]

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

# The extension's version once it was checked (per process), "" until then or when it is missing.
static var _native_version: String = ""
static var _native_checked: bool = false


# Imports a Tiled map (.tmx/.tmj) from the file system (res://, user:// or absolute path).
# Returns the root node of the created tree (no parent) or null on a fatal error.
func import(source_file: String, project_file: String = "") -> Node:
	return _import(source_file, project_file, "", "")


# Imports a Tiled map from a zip archive. All paths except the zip path are paths inside the archive.
# The archive is used for this call only, later calls of import() read from the file system again.
func import_from_zip(zip_file: String, source_file_in_zip: String, project_file_in_zip: String = "") -> Node:
	return _import(source_file_in_zip, project_file_in_zip, zip_file, "")


# Imports a Tiled map and runs a post-processing script (see PostProcessing.gd) on the result.
func import_with_post_processing(source_file: String, post_proc_file: String, project_file: String = "") -> Node:
	return _import(source_file, project_file, "", post_proc_file)


# Note: the post-processing script is loaded with load() i.e. it has to be a project resource, it is not read from the zip.
func import_from_zip_with_post_processing(zip_file: String, source_file_in_zip: String, post_proc_file_in_zip: String, project_file_in_zip: String = "") -> Node:
	return _import(source_file_in_zip, project_file_in_zip, zip_file, post_proc_file_in_zip)


# The native library's version, or "" when it is not loaded. Checked once per process.
static func native_version() -> String:
	if not _native_checked:
		_native_checked = true
		if ClassDB.class_exists(NATIVE_CLASS):
			var native: RefCounted = ClassDB.instantiate(NATIVE_CLASS)
			if native != null:
				_native_version = str(native.call("version"))
	return _native_version


# The options as a TiledImportOptions resource (the native classes take them so).
func make_options() -> Resource:
	var opts: Resource = ClassDB.instantiate("TiledImportOptions")
	for o in OPTION_NAMES:
		opts.set(o, get(o))
	return opts


func _import(source_file: String, project_file: String, zip_file: String, post_proc_file: String) -> Node:
	var errors := 0
	var warnings := 0
	var ret: Node = null
	if native_version() == "":
		printerr("ERROR: yati_native: the native library is not loaded (class " + NATIVE_CLASS + " is missing); build it with tools/build_native.sh or download it with tools/install_native.sh.")
		errors += 1
	else:
		var map: RefCounted = ClassDB.instantiate("TiledMap")
		var err: int
		if zip_file == "":
			err = map.call("load", source_file, project_file)
		else:
			err = map.call("from_zip", zip_file, source_file, project_file)
		errors += (map.call("get_errors") as PackedStringArray).size()
		warnings += (map.call("get_warnings") as PackedStringArray).size()
		if err == OK:
			# A TileSet per import: imports never share one (the cache across imports is stage 3's)
			var tilesets: RefCounted = ClassDB.instantiate("TiledTileSets")
			var builder: RefCounted = ClassDB.instantiate("TiledBuilder")
			ret = builder.call("build", map, tilesets, make_options())
			errors += int(builder.call("get_error_count"))
			warnings += int(builder.call("get_warning_count"))
			if ret != null and post_proc_file != "":
				var post_proc := PostProcessing.new()
				ret = post_proc.call_post_process(ret, post_proc_file)
				if post_proc.get_error() != OK:
					errors += 1

	if ret == null and errors == 0:
		# A fatal error that was reported without being counted
		errors = 1
	last_error_count = errors
	last_warning_count = warnings
	CommonUtils.error_count = errors
	CommonUtils.warning_count = warnings
	if errors == 0 and warnings == 0:
		print("Import of '" + source_file + "' succeeded.")
	else:
		print("Import of '" + source_file + "' finished with " + str(errors) + " error(s) and " + str(warnings) + " warning(s).")
	return ret
