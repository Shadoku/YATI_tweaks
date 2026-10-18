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

# Reads Tiled world files (.world): where each map of a world is, in pixels. A world streamer
# calls parse() once and imports the maps it needs with Importer.import(); Importer.import_world()
# imports all of them at once.
extends RefCounted

const CommonUtils := preload("CommonUtils.gd")
const DataLoader := preload("DataLoader.gd")
const DictionaryBuilder := preload("DictionaryBuilder.gd")
const TilemapCreator := preload("TilemapCreator.gd")

const MAP_EXTENSIONS = ["tmx", "tmj"]

# The size in pixels of maps a pattern lists, read from their files once:
# "<zip>::<path>" -> {"size": Vector2i, "modified": int (file system: the file's time)}
static var _header_cache: Dictionary = {}


# Parses a Tiled .world file. Returns an Array of {"path": String, "rect": Rect2i}, the maps in the
# order of the file's 'maps' entries, then the maps its 'patterns' list (sorted by file name).
# - 'maps': each entry's fileName (relative to the world file) and x, y, width, height.
# - 'patterns': every .tmx/.tmj file directly in the world file's directory whose name matches
#   the entry's regexp (RegEx.search, as Tiled matches it) and is not listed in 'maps', with the
#   first pattern it matches: position (capture 1 * multiplierX + offsetX, capture 2 * multiplierY
#   + offsetY), size the pattern's mapWidth / mapHeight, else the map's own size in pixels (read
#   from the map file, cached).
# - 'onlyShowAdjacentMaps' is an editor setting and ignored.
# zip_file: read the world file, the directory and the maps from that archive (all paths are paths
# inside it); without, from the file system, or from the zip of an import in progress.
# Errors (world file missing or unreadable, a bad regexp, a map file that cannot be read) are printed
# and counted in CommonUtils.error_count; what could be read is returned.
static func parse(world_file: String, zip_file: String = "") -> Array:
	if zip_file == "":
		return _parse(world_file)
	var ret: Array = []
	if DataLoader.begin_import(zip_file) == OK:
		ret = _parse(world_file)
	DataLoader.end_import()
	return ret


# Forgets the map sizes read for patterns (a map file in a zip archive is not checked for changes).
static func clear_cache() -> void:
	_header_cache.clear()


static func _parse(world_file: String) -> Array:
	var ret: Array = []
	var content = DataLoader.get_tiled_file_content(world_file, "")
	if content == null:
		printerr("ERROR: Tiled world file '" + world_file + "' not found.")
		CommonUtils.error_count += 1
		return ret
	var json := JSON.new()
	if json.parse(content.get_string_from_utf8()) != OK or not json.data is Dictionary:
		printerr("ERROR: Tiled world file '" + world_file + "' could not be parsed.")
		CommonUtils.error_count += 1
		return ret
	var world: Dictionary = json.data
	var dir := world_file.get_base_dir()
	var listed := {}
	var maps = world.get("maps", [])
	if maps is Array:
		for entry in maps:
			if not entry is Dictionary: continue
			var file := str(entry.get("fileName", ""))
			if file == "": continue
			var path := resolve_path(dir, file)
			listed[path] = true
			ret.append({"path": path, "rect": Rect2i(int(entry.get("x", 0)), int(entry.get("y", 0)),
					int(entry.get("width", 0)), int(entry.get("height", 0)))})

	var patterns = world.get("patterns", [])
	if not patterns is Array or patterns.is_empty():
		return ret
	var compiled: Array = []
	for pattern in patterns:
		if not pattern is Dictionary: continue
		var regex := RegEx.new()
		if regex.compile(str(pattern.get("regexp", ""))) != OK:
			printerr("ERROR: Tiled world file '" + world_file + "': pattern '" + str(pattern.get("regexp", "")) + "' is not a valid regular expression. -> Skipped")
			CommonUtils.error_count += 1
			continue
		compiled.append([regex, pattern])
	var files := Array(DataLoader.list_files(dir))
	files.sort()
	for file in files:
		if not str(file).get_extension().to_lower() in MAP_EXTENSIONS: continue
		var path := resolve_path(dir, file)
		if listed.has(path): continue
		for c in compiled:
			var m: RegExMatch = c[0].search(file)
			if m == null: continue
			var pattern: Dictionary = c[1]
			var ix := int(m.get_string(1)) if m.get_group_count() >= 1 else 0
			var iy := int(m.get_string(2)) if m.get_group_count() >= 2 else 0
			var pos := Vector2i(ix * int(pattern.get("multiplierX", 1)) + int(pattern.get("offsetX", 0)),
					iy * int(pattern.get("multiplierY", 1)) + int(pattern.get("offsetY", 0)))
			var size := Vector2i(int(pattern.get("mapWidth", -1)), int(pattern.get("mapHeight", -1)))
			if size.x < 0 or size.y < 0:
				var map_size = get_map_pixel_size(path)
				if map_size == null:
					printerr("ERROR: Map file '" + path + "' of world '" + world_file + "' could not be read. -> Skipped")
					CommonUtils.error_count += 1
					break
				if size.x < 0: size.x = map_size.x
				if size.y < 0: size.y = map_size.y
			ret.append({"path": path, "rect": Rect2i(pos, size)})
			listed[path] = true
			break
	return ret


# A file of the world, as a path: relative to the world file's directory unless it is absolute
static func resolve_path(dir: String, file: String) -> String:
	if file.is_absolute_path() or dir == "":
		return file.simplify_path()
	return dir.path_join(file).simplify_path()


# The size in pixels of a map file (TilemapCreator.get_map_rect_of: Tiled's map size, an infinite
# map's chunks), or null when it cannot be read. Cached per file.
static func get_map_pixel_size(path: String):
	var key := DataLoader.zip_file + "::" + path
	var modified := FileAccess.get_modified_time(path) if DataLoader.zip_file == "" else 0
	var cached = _header_cache.get(key)
	if cached != null and cached["modified"] == modified:
		return cached["size"]
	var content = DataLoader.get_tiled_file_content(path, "")
	if content == null: return null
	var map_dict = DictionaryBuilder.new().get_dictionary(content, path)
	if not map_dict is Dictionary or not map_dict.has("tilewidth"): return null
	var size := Vector2i(TilemapCreator.get_map_rect_of(map_dict).size.round())
	_header_cache[key] = {"size": size, "modified": modified}
	return size
