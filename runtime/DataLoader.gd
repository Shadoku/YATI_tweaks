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


const CommonUtils := preload("CommonUtils.gd")

# Zip archive the current import reads from ("" = read from the file system).
# Set by begin_import() and cleared by end_import(). Assigning it directly (legacy) still works,
# the archive is then opened per read.
static var zip_file: String = ""

# Per-import state (only valid between begin_import() and end_import())
static var _in_import: bool = false
static var _zip_reader: ZIPReader = null
static var _texture_cache: Dictionary = {}


# Starts an import scope: optionally opens the zip archive once for all reads of this import
# and enables the per-import texture cache. Always pair with end_import().
static func begin_import(zip_path: String = "") -> Error:
	end_import()
	_in_import = true
	zip_file = zip_path
	if zip_path == "":
		return OK
	var reader := ZIPReader.new()
	var err := reader.open(zip_path)
	if err != OK:
		printerr("ERROR: Unable to open zip file '" + zip_path + "'.")
		CommonUtils.error_count += 1
		return err
	_zip_reader = reader
	return OK


# Ends the import scope: closes the zip archive, clears the texture cache and resets to file system mode.
static func end_import() -> void:
	if _zip_reader != null:
		_zip_reader.close()
		_zip_reader = null
	_texture_cache.clear()
	zip_file = ""
	_in_import = false


# Returns the reader for one zip read: the import-wide reader if available, otherwise
# (no import scope active, legacy use) a freshly opened one. Release it with _release_zip_reader().
static func _acquire_zip_reader() -> ZIPReader:
	if _zip_reader != null:
		return _zip_reader
	if _in_import:
		# begin_import() already failed to open the archive (and reported it)
		return null
	var reader := ZIPReader.new()
	if reader.open(zip_file) != OK:
		printerr("ERROR: Unable to open zip file '" + zip_file + "'.")
		CommonUtils.error_count += 1
		return null
	return reader


static func _release_zip_reader(reader: ZIPReader) -> void:
	if reader != _zip_reader:
		reader.close()


static func _resolve_path_in_zip(reader: ZIPReader, file_name: String, base_path: String) -> String:
	if reader.file_exists(file_name):
		return file_name
	var file_in_zip := CommonUtils.cleanup_path(base_path.path_join(file_name))
	if reader.file_exists(file_in_zip):
		return file_in_zip
	return ""


static func _resolve_path_in_file_system(file_name: String, base_path: String) -> String:
	if FileAccess.file_exists(file_name):
		return file_name
	var checked_file := base_path.path_join(file_name)
	if FileAccess.file_exists(checked_file):
		return checked_file
	return ""


static func get_tiled_file_content(file_name: String, base_path: String):
	if zip_file == "":
		return get_tiled_file_content_from_file(file_name, base_path)
	else:
		return get_tiled_file_content_from_zip(file_name, base_path)


static func get_tiled_file_content_from_file(file_name: String, base_path: String):
	var checked_file := _resolve_path_in_file_system(file_name, base_path)
	if checked_file == "": return null
	return FileAccess.get_file_as_bytes(checked_file)


static func get_tiled_file_content_from_zip(file_name: String, base_path: String):
	var reader := _acquire_zip_reader()
	if reader == null: return null
	var ret = null
	var file_in_zip := _resolve_path_in_zip(reader, file_name, base_path)
	if file_in_zip != "":
		ret = reader.read_file(file_in_zip)
	_release_zip_reader(reader)
	return ret


static func load_image(file_name: String, base_path: String):
	if zip_file == "":
		return load_image_from_file(file_name, base_path)
	else:
		return load_image_from_zip(file_name, base_path)


static func load_image_from_file(file_name: String, base_path: String):
	var checked_file := _resolve_path_in_file_system(file_name, base_path)
	if checked_file == "":
		printerr("ERROR: Image file '" + file_name + "' not found.")
		CommonUtils.error_count += 1
		return null

	if _in_import and _texture_cache.has(checked_file):
		return _texture_cache[checked_file]

	var texture: Texture2D = null
	if ResourceLoader.exists(checked_file, "Image"):
		texture = ResourceLoader.load(checked_file)
	else:
		var image := Image.load_from_file(checked_file)
		if image == null:
			printerr("ERROR: Image file '" + checked_file + "' could not be loaded.")
			CommonUtils.error_count += 1
			return null
		texture = ImageTexture.create_from_image(image)

	if _in_import:
		_texture_cache[checked_file] = texture
	return texture


static func load_image_from_zip(file_name: String, base_path: String):
	var reader := _acquire_zip_reader()
	if reader == null: return null
	var file_in_zip := _resolve_path_in_zip(reader, file_name, base_path)
	if file_in_zip == "":
		_release_zip_reader(reader)
		printerr("ERROR: Image file '" + file_name + "' not found in zip file '" + zip_file + "'.")
		CommonUtils.error_count += 1
		return null

	var cache_key := zip_file + "::" + file_in_zip
	if _in_import and _texture_cache.has(cache_key):
		_release_zip_reader(reader)
		return _texture_cache[cache_key]

	var image_bytes := reader.read_file(file_in_zip)
	_release_zip_reader(reader)
	var image := Image.new()
	var err: Error = ERR_FILE_UNRECOGNIZED
	match file_name.get_extension().to_lower():
		"png":
			err = image.load_png_from_buffer(image_bytes)
		"jpg", "jpeg":
			err = image.load_jpg_from_buffer(image_bytes)
		"bmp":
			err = image.load_bmp_from_buffer(image_bytes)
		"webp":
			err = image.load_webp_from_buffer(image_bytes)
		"tga":
			err = image.load_tga_from_buffer(image_bytes)
	if err != OK:
		printerr("ERROR: Image file '" + file_in_zip + "' in zip file '" + zip_file + "' could not be loaded.")
		CommonUtils.error_count += 1
		return null
	var texture := ImageTexture.create_from_image(image)
	if _in_import:
		_texture_cache[cache_key] = texture
	return texture


static func load_resource_from_file(resource_file: String, base_path: String):
	var checked_file := _resolve_path_in_file_system(resource_file, base_path)
	if checked_file != "":
		return ResourceLoader.load(checked_file)

	printerr("ERROR: Resource file '" + resource_file + "' not found.")
	CommonUtils.error_count += 1
	return null
