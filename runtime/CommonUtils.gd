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

static var error_count: int
static var warning_count: int

static func get_bitmask_integer_from_string(mask_string: String, max_len: int):
	var ret: int = 0
	var s1_arr = mask_string.split(",", false)
	for s1 in s1_arr:
		if s1.contains("-"):
			var s2_arr = s1.split("-", false, 1)
			var i1 = int(s2_arr[0]) if s2_arr[0].is_valid_int() else 0
			var i2 = int(s2_arr[1]) if s2_arr[1].is_valid_int() else 0
			if i1 == 0 or i2 == 0 or i1 > i2: continue
			for i in range(i1, i2+1):
				if i <= max_len:
					ret += int(pow(2, i-1))
		elif s1.is_valid_int():
			var i = int(s1)
			if i <= max_len:
				ret += int(pow(2, i-1))
	return ret


static func get_right_typed_value(type: String, val: String):
	if type == "bool":
		return val == "true"
	elif type == "float":
		return float(val)
	elif type == "int":
		return int(val)
	elif type == "color":
		return tiled_color(val)
	else:
		# JSON parsing since Godot 4.4 adds ".0" to integers so remove that
		if val.ends_with(".0"):
			val = val.replace(".0", "")
		return val


# Converts a Tiled colour string (#RRGGBB, or #AARRGGBB with the alpha first) to a Color.
# An empty or unreadable string gives the fallback.
static func tiled_color(s: String, fallback: Color = Color.WHITE) -> Color:
	var hex := s.strip_edges().trim_prefix("#")
	if hex == "": return fallback
	if hex.length() == 8:
		# Tiled's #AARRGGBB -> Godot's RRGGBBAA
		hex = hex.substr(2) + hex.substr(0, 2)
	if not hex.is_valid_hex_number(): return fallback
	return Color.from_string(hex, fallback)


static func cleanup_path(path: String) -> String:
	while true:
		var path_arr = path.split("/")
		var is_clean: bool = true
		for i in range(1, path_arr.size()):
			if path_arr[i] == "..":
				path_arr[i] = ""
				path_arr[i-1] = ""
				is_clean = false
				break
			if path_arr[i] == ".":
				path_arr[i] = ""
				is_clean = false
		var new_path = ""
		for t in path_arr:
			if t == "": continue
			if new_path != "":
				new_path += "/"
			if t != "":
				new_path += t
		if is_clean:
			return new_path
		path = new_path
	return ""
