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
# project switches importers by changing one preload path. docs/native-importer-plan.md, stage 2;
# import_async() and the TileSet cache shared across imports, stage 3.
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

# Share TileSets across imports (and Importer instances): a map whose tilesets an earlier import
# built gets that TileSet, by reference, instead of a new one (see shared_tilesets()). The shared
# cache is exclusive: a TileSet is handed out only while no live tree holds it, so two trees alive
# at once never share one, as with a fresh cache. Off: every import builds its own TileSets. An
# import with save_tileset_to set always builds its own (the saved file is that map's).
var share_tilesets: bool = true
# import_async(): TileSet tiles created per frame before the build frame (TiledTileSets.build_step).
var async_tiles_per_frame: int = 256

# Results of the last import (any of the import methods)
var last_error_count: int = 0
var last_warning_count: int = 0

# The extension's version once it was checked (per process), "" until then or when it is missing.
static var _native_version: String = ""
static var _native_checked: bool = false
# The TileSet cache share_tilesets uses (a TiledTileSets), made on first use.
static var _shared_tilesets: RefCounted = null

# TiledMap.poll()'s values (TiledMap.READING ... FAILED).
const _MAP_READY := 3
const _MAP_FAILED := 4


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


# Imports a Tiled map like import(), without a hitch: a coroutine, so `await` it. The map and its
# files are parsed on a worker thread while the game runs (TiledMap.load_async, one poll() per
# frame: the main thread only reads the files and hands them over), its textures load in the
# engine's threads, its TileSets are created a few tiles per frame (async_tiles_per_frame), and
# the tree is built in one frame at the end. Returns the root node (no parent) or null.
#
#     var map: Node = await importer.import_async("res://maps/level.tmx")
func import_async(source_file: String, project_file: String = "") -> Node:
	return await _import_async(source_file, project_file, "")


# import_from_zip() as a coroutine, as import_async(): the archive is read in the first frame,
# everything else is parsed on the worker thread.
func import_from_zip_async(zip_file: String, source_file_in_zip: String, project_file_in_zip: String = "") -> Node:
	return await _import_async(source_file_in_zip, project_file_in_zip, zip_file)


# The TileSet cache every Importer shares when share_tilesets is on (a TiledTileSets; null without
# the native library). Its clear() forgets the TileSets, clear_textures() the per-process texture
# cache; `exclusive = false` on it shares a TileSet also between trees alive at the same time.
static func shared_tilesets() -> RefCounted:
	if _shared_tilesets == null and native_version() != "":
		_shared_tilesets = ClassDB.instantiate("TiledTileSets")
		_shared_tilesets.set("exclusive", true)
	return _shared_tilesets


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
			# The shared TileSet cache, or a fresh one (share_tilesets)
			var tilesets: RefCounted = _tilesets_for_import()
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


func _import_async(source_file: String, project_file: String, zip_file: String) -> Node:
	var errors := 0
	var warnings := 0
	var ret: Node = null
	if native_version() == "":
		printerr("ERROR: yati_native: the native library is not loaded (class " + NATIVE_CLASS + " is missing); build it with tools/build_native.sh or download it with tools/install_native.sh.")
		errors += 1
	else:
		var tree := Engine.get_main_loop() as SceneTree
		var map: RefCounted = ClassDB.instantiate("TiledMap")
		if zip_file == "":
			map.call("load_async", source_file, project_file)
		else:
			map.call("from_zip_async", zip_file, source_file, project_file)
		var status: int = map.call("poll")
		while status != _MAP_READY and status != _MAP_FAILED:
			await tree.process_frame
			status = map.call("poll")
		errors += (map.call("get_errors") as PackedStringArray).size()
		warnings += (map.call("get_warnings") as PackedStringArray).size()
		if status == _MAP_READY:
			var opts := make_options()
			var tilesets := _tilesets_for_import()
			var per_frame := maxi(1, async_tiles_per_frame)
			while true:
				# TileSets a few tiles per frame; the frame that finishes one is not the build's
				var created: int = tilesets.call("get_created_count")
				var ready: bool = tilesets.call("build_step", map, opts, per_frame)
				if ready and int(tilesets.call("get_created_count")) == created:
					break
				await tree.process_frame
				if ready:
					break
			# The tree in one frame
			var builder: RefCounted = ClassDB.instantiate("TiledBuilder")
			ret = builder.call("build", map, tilesets, opts)
			errors += int(builder.call("get_error_count"))
			warnings += int(builder.call("get_warning_count"))
	return _finish_async(source_file, ret, errors, warnings)


# The TileSet cache of one import: the shared one, or a fresh one (share_tilesets off, or a
# TileSet to be saved).
func _tilesets_for_import() -> RefCounted:
	if share_tilesets and save_tileset_to == "":
		return shared_tilesets()
	return ClassDB.instantiate("TiledTileSets")


# The counts and the summary line of an asynchronous import (as _import's).
func _finish_async(source_file: String, ret: Node, errors: int, warnings: int) -> Node:
	if ret == null and errors == 0:
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
