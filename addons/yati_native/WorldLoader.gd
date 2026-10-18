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


# Reads Tiled world files (.world): where each map of a world is, in pixels. The GDScript runtime's
# WorldLoader.gd API (runtime/WorldLoader.gd) over the native class TiledWorld (reached through
# ClassDB, so this script compiles without the library). A world streamer calls parse() once and
# imports the maps it needs with Importer.import(); Importer.import_world() imports all of them.
extends RefCounted

const CommonUtils := preload("CommonUtils.gd")

const NATIVE_CLASS: String = "TiledWorld"


# Parses a Tiled .world file. Returns an Array of {"path": String, "rect": Rect2i}, the maps in the
# order of the file's 'maps' entries, then the maps its 'patterns' list (sorted by file name).
# - 'maps': each entry's fileName (relative to the world file) and x, y, width, height.
# - 'patterns': every .tmx/.tmj file directly in the world file's directory whose name matches
#   the entry's regexp (searched, as Tiled matches it) and is not listed in 'maps', with the
#   first pattern it matches: position (capture 1 * multiplierX + offsetX, capture 2 * multiplierY
#   + offsetY), size the pattern's mapWidth / mapHeight, else the map's own size in pixels (read
#   from the map file).
# - 'onlyShowAdjacentMaps' is an editor setting and ignored.
# zip_file: read the world file, the directory and the maps from that archive (all paths are paths
# inside it); without, from the file system.
# Errors (world file missing or unreadable, a bad regexp, a map file that cannot be read) are printed
# and counted in CommonUtils.error_count; what could be read is returned.
static func parse(world_file: String, zip_file: String = "") -> Array:
	if not ClassDB.class_exists(NATIVE_CLASS):
		printerr("ERROR: yati_native: the native library is not loaded (class " + NATIVE_CLASS + " is missing); build it with tools/build_native.sh or download it with tools/install_native.sh.")
		CommonUtils.error_count += 1
		return []
	var world: RefCounted = ClassDB.instantiate(NATIVE_CLASS)
	world.call("load", world_file, zip_file)
	CommonUtils.error_count += (world.call("get_errors") as PackedStringArray).size()
	return world.call("get_maps")


# The runtime's WorldLoader caches the map sizes it reads for patterns; this one reads them on
# every parse(), so there is nothing to forget. Kept for the runtime's API.
static func clear_cache() -> void:
	pass
