extends SceneTree
## Prints the dictionary the GDScript runtime builds for a Tiled file: what
## DictionaryBuilder.get_dictionary() returns for it (DataLoader reads the file), the parity
## target of the native importer's model (native/tiled, tests/parity.rs).
##
## Usage (from the repository root):
##   godot --headless -s test/dump_dictionary.gd -- <file> [project_file] [--out=<file.json>]
##         [--zip=<archive.zip>] [--runtime=res://runtime]
##
## <file> is a .tmx/.tmj map, a .tsx/.tsj tileset or a .tx/.tj template (res://, or relative
## to the repository root). With a project file, its custom types are merged in (CustomTypes.gd)
## where the runtime merges them while it builds a map: the map ("map"), every layer, groups'
## children included ("layer"), every object of an object layer ("object"), every embedded
## tileset ("tileset"), its tiles ("tile") and their collision objects ("object"); a tileset
## file the same way. Templates are dumped as they are (the runtime merges into copies).
## With --zip, <file> and the project are paths inside the archive (DataLoader's zip mode).
##
## The JSON is JSON.stringify(dict, indent, true, true): keys sorted, floats with a decimal
## point (5.0) so ints and floats stay apart. Godot's JSON has one quirk that matters: a .tmj
## file's numbers are all floats once parsed, so its dump has 16.0 where the file has 16; the
## native side reproduces that. Without --out the JSON goes to stdout on one line, after the
## engine's banner; with --out it is written indented to the file.

const RUNTIME_DEFAULT := "res://runtime"


func _init() -> void:
	var positional: Array[String] = []
	var out_path := ""
	var zip_path := ""
	var runtime := RUNTIME_DEFAULT
	for arg in OS.get_cmdline_user_args():
		if arg.begins_with("--out="):
			out_path = arg.substr(6)
		elif arg.begins_with("--zip="):
			zip_path = _res(arg.substr(6))
		elif arg.begins_with("--runtime="):
			runtime = arg.substr(10)
		else:
			positional.append(arg)
	if positional.is_empty():
		printerr("usage: godot --headless -s test/dump_dictionary.gd -- <file> [project_file] [--out=<file.json>] [--zip=<archive>]")
		quit(2)
		return
	var file := positional[0] if zip_path != "" else _res(positional[0])
	var project := ""
	if positional.size() > 1:
		project = positional[1] if zip_path != "" else _res(positional[1])

	var DataLoader = load(runtime.path_join("DataLoader.gd"))
	var DictionaryBuilder = load(runtime.path_join("DictionaryBuilder.gd"))
	var CustomTypes = load(runtime.path_join("CustomTypes.gd"))
	if DataLoader.begin_import(zip_path) != OK:
		quit(1)
		return
	var content = DataLoader.get_tiled_file_content(file, "")
	if content == null:
		printerr("ERROR: file '" + file + "' not found.")
		DataLoader.end_import()
		quit(1)
		return
	var dict = DictionaryBuilder.new().get_dictionary(content, file)
	if not dict is Dictionary:
		printerr("ERROR: file '" + file + "' could not be parsed.")
		DataLoader.end_import()
		quit(1)
		return
	if project != "":
		var ct = CustomTypes.new()
		ct.load_custom_types(project)
		match str(dict.get("type", "")):
			"map":
				_merge_map(ct, dict)
			"tileset":
				_merge_tileset(ct, dict)
	DataLoader.end_import()

	if out_path == "":
		print(JSON.stringify(dict, "", true, true))
	else:
		var f := FileAccess.open(_res(out_path), FileAccess.WRITE)
		if f == null:
			printerr("ERROR: cannot write '" + out_path + "'.")
			quit(1)
			return
		f.store_string(JSON.stringify(dict, "  ", true, true) + "\n")
		f.close()
	quit(0)


static func _res(path: String) -> String:
	if path.begins_with("res://") or path.begins_with("user://") or path.is_absolute_path():
		return path
	return "res://" + path


static func _merge_map(ct, map: Dictionary) -> void:
	ct.merge_custom_properties(map, "map")
	for layer in map.get("layers", []):
		_merge_layer(ct, layer)
	for ts in map.get("tilesets", []):
		if not ts.has("source"):
			_merge_tileset(ct, ts)


static func _merge_layer(ct, layer: Dictionary) -> void:
	ct.merge_custom_properties(layer, "layer")
	for obj in layer.get("objects", []):
		ct.merge_custom_properties(obj, "object")
	for child in layer.get("layers", []):
		_merge_layer(ct, child)


static func _merge_tileset(ct, ts: Dictionary) -> void:
	ct.merge_custom_properties(ts, "tileset")
	for tile in ts.get("tiles", []):
		ct.merge_custom_properties(tile, "tile")
		if tile.has("objectgroup"):
			for obj in tile["objectgroup"].get("objects", []):
				ct.merge_custom_properties(obj, "object")
