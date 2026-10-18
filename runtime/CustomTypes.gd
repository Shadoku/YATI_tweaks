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

const DataLoader := preload("DataLoader.gd")
const CommonUtils := preload("CommonUtils.gd")
const DictionaryBuilder := preload("DictionaryBuilder.gd")

var _custom_types = null
# The directory of the project file (what its file-typed member values are relative to)
var _project_dir: String = ""
# Index "name|scope" -> Array of member dictionaries (built once after loading)
var _class_members_by_name_and_scope: Dictionary = {}
# Enum name -> Array of its value names (built once after loading)
var _enum_values_by_name: Dictionary = {}

func load_custom_types(project_file: String):
	_project_dir = project_file.get_base_dir()
	var proj_file_content = DataLoader.get_tiled_file_content(project_file, "")
	if proj_file_content == null:
		printerr("ERROR: Tiled project file '" + project_file + "' not found.")
		CommonUtils.error_count += 1
		return

	var project_file_as_dictionary = DictionaryBuilder.new().get_dictionary(proj_file_content, project_file)
	if not project_file_as_dictionary is Dictionary:
		printerr("ERROR: Tiled project file '" + project_file + "' could not be parsed.")
		CommonUtils.error_count += 1
		return
	if project_file_as_dictionary.has("propertyTypes"):
		_custom_types = project_file_as_dictionary["propertyTypes"]
	_build_index()


func _build_index() -> void:
	_class_members_by_name_and_scope.clear()
	_enum_values_by_name.clear()
	if _custom_types == null: return
	for ct_prop in _custom_types:
		if ct_prop.get("type", "") == "enum":
			_enum_values_by_name[str(ct_prop.get("name", ""))] = ct_prop.get("values", [])
		if ct_prop.get("type", "") != "class": continue
		var pt_name: String = str(ct_prop.get("name", ""))
		var pt_members: Array = ct_prop.get("members", [])
		for scope in ct_prop.get("useAs", []):
			var key: String = pt_name + "|" + str(scope)
			if _class_members_by_name_and_scope.has(key):
				# Same class name defined more than once: keep the definition order
				_class_members_by_name_and_scope[key].append_array(pt_members)
			else:
				_class_members_by_name_and_scope[key] = pt_members.duplicate()


# The directory of the loaded project file.
func get_project_dir() -> String:
	return _project_dir


# The member dictionary ({"name", "type", "value", ...}) named member_name of the custom class
# type_name for scope ("object", "tile", "layer", "map", ...), or null. A file member's value is
# relative to the project file (get_project_dir()).
func get_class_member(type_name: String, scope: String, member_name: String):
	var members = _class_members_by_name_and_scope.get(type_name + "|" + scope)
	if members == null: return null
	for mem in members:
		if str(mem.get("name", "")) == member_name:
			return mem
	return null


# The bitmask of a string of value names of the enum enum_name ("Player,Enemy"): each value's index
# in the enum is its bit (a flags enum's layers in their order). -1 if there is no such enum.
func get_enum_mask(enum_name: String, names: String, max_len: int) -> int:
	var values = _enum_values_by_name.get(enum_name)
	if values == null: return -1
	var mask := 0
	for name in names.split(",", false):
		var index: int = values.find(name.strip_edges())
		if index < 0 or index >= max_len:
			print_rich("[color=yellow] -- '" + name.strip_edges() + "' is no layer of enum '" + enum_name + "'. -> Ignored[/color]")
			CommonUtils.warning_count += 1
			continue
		mask |= 1 << index
	return mask


func unload_custom_types():
	if _custom_types != null:
		_custom_types.clear()
	_class_members_by_name_and_scope.clear()


func merge_custom_properties(obj: Dictionary, scope: String):
	if _class_members_by_name_and_scope.is_empty(): return

	var class_string = obj.get("class", "")
	if class_string == "":
		class_string = obj.get("type", "")

	var members = _class_members_by_name_and_scope.get(str(class_string) + "|" + scope)
	if members == null: return

	var existing_names: Dictionary = {}
	if obj.has("properties"):
		for prop in obj["properties"]:
			existing_names[prop.get("name", "")] = true

	for mem in members:
		var name = mem.get("name", "")
		if existing_names.has(name): continue
		if not obj.has("properties"):
			obj["properties"] = []
		obj["properties"].append(mem)
		existing_names[name] = true
