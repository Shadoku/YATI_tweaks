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


# A small stand-in for the GDScript runtime's TilemapCreator.gd (runtime/TilemapCreator.gd), for
# code that creates maps through it: create() imports a map with the default options through
# Importer.gd, get_map_rect() and get_tileset() answer for the last created map. Its option setters
# are not here: use Importer.gd's option variables.
extends RefCounted

const Importer := preload("Importer.gd")

var _map_rect := Rect2()
var _tileset: TileSet = null


# Imports a Tiled map (.tmx/.tmj) and returns the root of its tree (no parent), or null.
func create(source_file: String) -> Node:
	var root: Node = Importer.new().import(source_file)
	_map_rect = root.get_meta("map_rect", Rect2()) if root != null else Rect2()
	_tileset = null
	if root != null:
		var layers := root.find_children("*", "TileMapLayer", true, false)
		if root is TileMapLayer: layers.insert(0, root)
		if not layers.is_empty(): _tileset = (layers[0] as TileMapLayer).tile_set
	return root


# The bounds of the last created map in pixels (the root's metadata "map_rect").
func get_map_rect() -> Rect2:
	return _map_rect


# The TileSet of the last created map (null without tile layers).
func get_tileset() -> TileSet:
	return _tileset
