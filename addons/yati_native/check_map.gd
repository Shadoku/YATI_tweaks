extends SceneTree
## Checks a Tiled map with the importer next to this script (Importer.gd) and prints its errors and
## warnings in a form a tool can read: the Tiled extension's "Check map in Godot" action
## (docs/tiled-extension-plan.md) runs it and files each line as an Issue.
##
##   godot --headless --path <project> -s res://addons/yati_native/check_map.gd -- <map>
##       [--project=<.tiled-project>] [--zip=<archive>] [--<option>=<value> ...]
##
## <map>: a res://, user:// or absolute path, or one relative to the project (a path inside the
## archive with --zip, as are --project's). Every --<option>=<value> sets the importer option of
## that name (Importer.gd's option variables), typed from its declared type: a bool from
## true/false (or 1/0), an int, a float, a String as given, a Dictionary (class_scenes) from JSON.
## add_id_as_metadata and add_class_as_metadata are always on (they only add metadata), so a
## message can be tied to the object it names. The tree is freed after the import.
##
## Output (stdout; the importer's own lines are printed as usual, around these):
##   YATI_CHECK error <id|-> <text>
##   YATI_CHECK warning <id|-> <text>
##   YATI_CHECK result <errors> <warnings> <nodes> <cells> <ms>
## one line per error or warning of the import, in order, then the summary: the import's error
## and warning counts, the tree's nodes (the root included) and tile layer cells, the import's
## wall time in whole milliseconds. <id> is the Tiled object id of the object the message names
## (by its node's name, or by its class for a class message: the class's object of lowest id),
## "-" when it names none. Exit code: 0 without errors, 1 with errors, 2 when the map could not be
## imported at all (no tree) or the arguments are wrong.
##
## The same script sits next to both importers (addons/yati_native/ and the GDScript runtime's
## directory, runtime/ here) and prints the same lines for the same map.

const PREFIX := "YATI_CHECK"
const FORCED_OPTIONS: Array[String] = ["add_id_as_metadata", "add_class_as_metadata"]


func _initialize() -> void:
	quit(_run())


func _run() -> int:
	var importer_path: String = (get_script() as Script).resource_path.get_base_dir().path_join("Importer.gd")
	var importer_script := load(importer_path) as Script
	if importer_script == null:
		_line("error", "-", "Cannot load the importer '" + importer_path + "'.")
		_result(1, 0, 0, 0, 0)
		return 2
	var importer: Object = importer_script.new()

	# Arguments: the map, --project, --zip and the importer's options
	var errors := 0
	var map_path := ""
	var project_path := ""
	var zip_path := ""
	var options := _option_types(importer, importer_script)
	for arg in OS.get_cmdline_user_args():
		if not arg.begins_with("--"):
			if map_path != "":
				_line("error", "-", "More than one map given ('" + map_path + "', '" + arg + "').")
				errors += 1
			map_path = arg
			continue
		var eq := arg.find("=")
		var name := arg.substr(2, eq - 2) if eq >= 0 else arg.substr(2)
		var value := arg.substr(eq + 1) if eq >= 0 else ""
		if name == "project":
			project_path = value
		elif name == "zip":
			zip_path = value
		elif not options.has(name):
			_line("error", "-", "Unknown importer option '" + name + "'.")
			errors += 1
		elif eq < 0:
			_line("error", "-", "Importer option '" + name + "' has no value (--" + name + "=<value>).")
			errors += 1
		else:
			var typed: Array = _typed_value(value, options[name])
			if typed.is_empty():
				_line("error", "-", "Importer option '" + name + "': '" + value + "' is no " + type_string(options[name]) + ".")
				errors += 1
			else:
				importer.set(name, typed[0])
	if map_path == "":
		_line("error", "-", "No map given (check_map.gd -- <map> [--project=...] [--zip=...] [--option=value ...]).")
		errors += 1
	if errors > 0:
		_result(errors, 0, 0, 0, 0)
		return 2
	for name in FORCED_OPTIONS:
		importer.set(name, true)

	# The import
	var start := Time.get_ticks_usec()
	var root: Node
	if zip_path != "":
		root = importer.call("import_from_zip", zip_path, map_path, project_path)
	else:
		root = importer.call("import", map_path, project_path)
	var ms := int(round((Time.get_ticks_usec() - start) / 1000.0))
	var imported := root != null

	# Object ids by node name and by class, from the tree's metadata
	var ids_by_name := {}
	var ids_by_class := {}
	var nodes := 0
	var cells := 0
	if imported:
		var all: Array[Node] = [root]
		all.append_array(root.find_children("*", "", true, false))
		nodes = all.size()
		for node in all:
			if node is TileMapLayer:
				cells += (node as TileMapLayer).get_used_cells().size()
			if not node.has_meta("id"): continue
			var id := int(node.get_meta("id"))
			_keep_lowest(ids_by_name, String(node.name), id)
			if node.has_meta("class"):
				_keep_lowest(ids_by_class, str(node.get_meta("class")), id)
		root.free()

	var messages: Array = importer.get("last_messages") if "last_messages" in importer else []
	for m: Dictionary in messages:
		var text := _clean(str(m.get("text", "")))
		_line(str(m.get("kind", "error")), _object_id(text, ids_by_name, ids_by_class), text)
	var error_count := int(importer.get("last_error_count"))
	var warning_count := int(importer.get("last_warning_count"))
	_result(error_count, warning_count, nodes, cells, ms)
	if not imported:
		return 2
	return 1 if error_count > 0 else 0


# The importer's options and their types: OPTION_NAMES when the importer lists them, else its
# script variables but the results (last_*).
static func _option_types(importer: Object, script: Script) -> Dictionary:
	var names: Array = script.get_script_constant_map().get("OPTION_NAMES", [])
	var ret := {}
	for p in importer.get_property_list():
		if not (int(p["usage"]) & PROPERTY_USAGE_SCRIPT_VARIABLE): continue
		var pname: String = p["name"]
		if names.is_empty() and pname.begins_with("last_"): continue
		if not names.is_empty() and not names.has(pname): continue
		ret[pname] = int(p["type"])
	return ret


# value as the type: [typed value], or [] when it is no such value.
static func _typed_value(value: String, type: int) -> Array:
	match type:
		TYPE_BOOL:
			var v := value.to_lower()
			if v in ["true", "1", "yes", "on"]: return [true]
			if v in ["false", "0", "no", "off"]: return [false]
		TYPE_INT:
			if value.is_valid_int(): return [value.to_int()]
		TYPE_FLOAT:
			if value.is_valid_float(): return [value.to_float()]
		TYPE_STRING:
			return [value]
		TYPE_STRING_NAME:
			return [StringName(value)]
		TYPE_DICTIONARY:
			var parsed: Variant = JSON.parse_string(value)
			if parsed is Dictionary: return [parsed]
	return []


static func _keep_lowest(by_key: Dictionary, key: String, id: int) -> void:
	if not by_key.has(key) or id < int(by_key[key]):
		by_key[key] = id


# A message as one line: no surrounding blanks, no leading "-- ", "ERROR: " or "WARNING: " (the
# line says which it is), no line breaks.
static func _clean(text: String) -> String:
	var t := text.replace("\r", " ").replace("\n", " ").strip_edges()
	for prefix in ["-- ", "ERROR: ", "WARNING: "]:
		t = t.trim_prefix(prefix).strip_edges()
	return t


# The id of the object a message names: a quoted name after "of" or "object" (the importers'
# "Property 'hp' of 'Grunt'", "Tile object 'Coin'"), a class ("Class 'Door' is not ..."; its
# object of lowest id), else any quoted node name; "-" when none.
static func _object_id(text: String, ids_by_name: Dictionary, ids_by_class: Dictionary) -> String:
	var re := RegEx.create_from_string("(?:\\bof|\\bobject) '([^']*)'")
	for m in re.search_all(text):
		if ids_by_name.has(m.get_string(1)):
			return str(ids_by_name[m.get_string(1)])
	re = RegEx.create_from_string("^Class '([^']*)'")
	var c := re.search(text)
	if c != null and ids_by_class.has(c.get_string(1)):
		return str(ids_by_class[c.get_string(1)])
	re = RegEx.create_from_string("'([^']*)'")
	for m in re.search_all(text):
		if ids_by_name.has(m.get_string(1)):
			return str(ids_by_name[m.get_string(1)])
	return "-"


static func _line(kind: String, id: String, text: String) -> void:
	print(PREFIX + " " + kind + " " + id + " " + text)


static func _result(errors: int, warnings: int, nodes: int, cells: int, ms: int) -> void:
	print("%s result %d %d %d %d %d" % [PREFIX, errors, warnings, nodes, cells, ms])
