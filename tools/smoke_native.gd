extends SceneTree
## Loads the native importer the way a game does and quits 0 if it works, 1 if not: the
## GDExtension registered TiledNative, one can be made, its version read and a packed array
## sent through it and back, and the facade (addons/yati_native/Importer.gd) imports a fixture
## map (test/fixtures/layers.tmx: tile, object, group and image layers) without errors. CI and .github/workflows/native.yml run this with the real Godot
## on every platform they build for, so a library that cannot be loaded (a stale incremental
## link, a missing runtime, a corrupt image) fails the build instead of a game.
##
##   godot --headless --path . -s tools/smoke_native.gd
##
## Godot loads a GDExtension at startup only when .godot/extension_list.cfg names it; the
## editor writes that file on an import, test/run_all.sh and the workflows seed it.

const CLASS_NAME := "TiledNative"
const SMOKE_MAP := "res://test/fixtures/layers.tmx"


func _initialize() -> void:
	if not ClassDB.class_exists(CLASS_NAME):
		printerr("smoke_native: %s is not registered: the GDExtension did not load (is the library in addons/yati_native/bin/, and .godot/extension_list.cfg naming res://addons/yati_native/yati_native.gdextension?)" % CLASS_NAME)
		quit(1)
		return
	var native: RefCounted = ClassDB.instantiate(CLASS_NAME)
	if native == null:
		printerr("smoke_native: %s could not be instantiated" % CLASS_NAME)
		quit(1)
		return
	var version: String = native.call("version")
	if version.is_empty() or version.split(".").size() != 3:
		printerr("smoke_native: %s.version() returned '%s', not a x.y.z version" % [CLASS_NAME, version])
		quit(1)
		return
	var values := PackedInt32Array([1, -2, 3, 2147483647, 2147483647])
	var expected: int = 1 - 2 + 3 + 2 * 2147483647
	var total: int = native.call("sum", values)
	if total != expected:
		printerr("smoke_native: %s.sum() returned %d, expected %d" % [CLASS_NAME, total, expected])
		quit(1)
		return
	var importer: Object = load("res://addons/yati_native/Importer.gd").new()
	var map: Node = importer.call("import", SMOKE_MAP, "")
	if map == null or int(importer.get("last_error_count")) != 0:
		printerr("smoke_native: importing %s failed (%d error(s))" % [SMOKE_MAP, int(importer.get("last_error_count"))])
		if map != null:
			map.free()
		quit(1)
		return
	var layer := map.get_node_or_null("Base") as TileMapLayer
	var cells := layer.get_used_cells().size() if layer != null else 0
	var nodes := map.find_children("*", "", true, false).size()
	map.free()
	if cells == 0:
		printerr("smoke_native: %s imported without the cells of its layer 'Base'" % SMOKE_MAP)
		quit(1)
		return
	print("smoke_native: %s %s loaded, sum round trip ok (%d), %s imported (%d nodes, %d cells in 'Base')" % [CLASS_NAME, version, total, SMOKE_MAP.get_file(), nodes, cells])
	quit(0)
