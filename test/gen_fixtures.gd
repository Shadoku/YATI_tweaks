extends SceneTree
## Generates the binary / encoded test fixtures for the YATI runtime test suite.
##
## Run headless from the repository root:
##   godot --headless -s test/gen_fixtures.gd
##
## Produces (all under res://test/fixtures/):
##   *.png                      solid-colour tile sheets and images
##   encodings.tmx              csv / base64 / zlib / gzip / zstd tile layers
##   infinite.tmx               infinite map with csv and base64+zlib chunks
##   json_map.tmj               JSON map (embedded tileset, class-typed property)
##   custom.tiled-project       project file with custom class types
##   bundle.zip                 map + tileset + image packed in a zip
## The hand-written fixtures (.tmx/.tsx/.tx) live next to these files.

const OUT_DIR := "res://test/fixtures"
const TILE := 16

const FLIP_H := 0x80000000
const FLIP_V := 0x40000000
const FLIP_D := 0x20000000

## Pattern shared by every layer of encodings.tmx (4x4, last cell H-flipped).
const ENCODING_PATTERN: Array[int] = [
	1, 2, 3, 4,
	9, 10, 11, 12,
	0, 0, 17, 18,
	25, 26, 27, 2 | FLIP_H,
]


func _init() -> void:
	DirAccess.make_dir_recursive_absolute(ProjectSettings.globalize_path(OUT_DIR))
	var ok := true
	ok = gen_images() and ok
	ok = gen_encodings_map() and ok
	ok = gen_infinite_map() and ok
	ok = gen_json_map() and ok
	ok = gen_project_file() and ok
	ok = gen_zip() and ok
	print("gen_fixtures: " + ("done" if ok else "FAILED"))
	quit(0 if ok else 1)


# ---------------------------------------------------------------------------
# Images

static func tile_color(index: int, count: int) -> Color:
	return Color.from_hsv(float(index) / float(max(count, 1)), 0.75, 0.9)


## Draws a grid of solid-colour tiles with an optional margin/spacing.
static func make_sheet(cols: int, rows: int, tw: int, th: int, margin: int = 0, spacing: int = 0, hue_offset: int = 0) -> Image:
	var w := margin * 2 + cols * tw + (cols - 1) * spacing
	var h := margin * 2 + rows * th + (rows - 1) * spacing
	var img := Image.create(w, h, false, Image.FORMAT_RGBA8)
	img.fill(Color(1, 0, 1, 1)) # magenta background shows margin/spacing
	for r in range(rows):
		for c in range(cols):
			var idx := r * cols + c
			var x := margin + c * (tw + spacing)
			var y := margin + r * (th + spacing)
			img.fill_rect(Rect2i(x, y, tw, th), tile_color(idx + hue_offset, cols * rows + hue_offset))
			# darker 1px top/left edge so flips are visible when viewed
			img.fill_rect(Rect2i(x, y, tw, 1), Color(0, 0, 0, 1))
			img.fill_rect(Rect2i(x, y, 1, th / 2), Color(0, 0, 0, 1))
	return img


static func make_solid(w: int, h: int, col: Color) -> Image:
	var img := Image.create(w, h, false, Image.FORMAT_RGBA8)
	img.fill(col)
	img.fill_rect(Rect2i(0, 0, w, 1), Color(0, 0, 0, 1))
	return img


func save_png(img: Image, file_name: String) -> bool:
	var err := img.save_png(OUT_DIR.path_join(file_name))
	if err != OK:
		printerr("could not save " + file_name + ": " + str(err))
		return false
	return true


func gen_images() -> bool:
	var ok := true
	ok = save_png(make_sheet(8, 4, TILE, TILE), "tiles.png") and ok             # 128x64, terrain.tsx
	ok = save_png(make_sheet(4, 2, TILE, TILE, 2, 1, 5), "tiles_ms.png") and ok # 71x37, margin 2 spacing 1
	ok = save_png(make_sheet(2, 1, 32, 16, 0, 0, 3), "iso.png") and ok          # 64x16, iso.tsx
	ok = save_png(make_sheet(2, 1, TILE, TILE, 0, 0, 7), "zip_tiles.png") and ok
	ok = save_png(make_solid(16, 16, Color(0.2, 0.8, 0.2)), "small.png") and ok
	ok = save_png(make_solid(32, 48, Color(0.8, 0.2, 0.2)), "big.png") and ok
	ok = save_png(make_solid(16, 16, Color(0.2, 0.2, 0.8)), "obj.png") and ok
	ok = save_png(make_sheet(4, 2, 16, 16, 0, 0, 11), "bg.png") and ok          # 64x32 image layer
	return ok


# ---------------------------------------------------------------------------
# Encoded tile data

static func gids_to_bytes(gids: Array) -> PackedByteArray:
	var ints := PackedInt32Array()
	for g in gids:
		ints.append(int(g)) # values > 0x7fffffff wrap to the same bit pattern
	return ints.to_byte_array() # little endian, as Tiled writes it


static func encode(gids: Array, compression: String) -> String:
	var bytes := gids_to_bytes(gids)
	match compression:
		"zlib":
			bytes = bytes.compress(FileAccess.COMPRESSION_DEFLATE)
		"gzip":
			bytes = bytes.compress(FileAccess.COMPRESSION_GZIP)
		"zstd":
			bytes = bytes.compress(FileAccess.COMPRESSION_ZSTD)
	return Marshalls.raw_to_base64(bytes)


static func to_csv(gids: Array, width: int) -> String:
	var lines := PackedStringArray()
	var row := PackedStringArray()
	for i in range(gids.size()):
		row.append(str(gids[i]))
		if row.size() == width:
			lines.append(",".join(row))
			row = PackedStringArray()
	if row.size() > 0:
		lines.append(",".join(row))
	return ",\n".join(lines)


func write_text(file_name: String, content: String) -> bool:
	var f := FileAccess.open(OUT_DIR.path_join(file_name), FileAccess.WRITE)
	if f == null:
		printerr("could not write " + file_name + ": " + str(FileAccess.get_open_error()))
		return false
	f.store_string(content)
	f.close()
	return true


func gen_encodings_map() -> bool:
	var s := '<?xml version="1.0" encoding="UTF-8"?>\n'
	s += '<map version="1.10" tiledversion="1.11.2" orientation="orthogonal" renderorder="right-down" width="4" height="4" tilewidth="16" tileheight="16" infinite="0" nextlayerid="6" nextobjectid="1">\n'
	s += ' <tileset firstgid="1" source="terrain.tsx"/>\n'
	s += ' <layer id="1" name="CSV" width="4" height="4">\n  <data encoding="csv">\n' + to_csv(ENCODING_PATTERN, 4) + '\n</data>\n </layer>\n'
	var variants := [["B64", ""], ["B64zlib", "zlib"], ["B64gzip", "gzip"], ["B64zstd", "zstd"]]
	var id := 2
	for v: Array in variants:
		var comp: String = v[1]
		var comp_attr := "" if comp == "" else ' compression="' + comp + '"'
		s += ' <layer id="%d" name="%s" width="4" height="4">\n' % [id, v[0]]
		s += '  <data encoding="base64"%s>\n   %s\n  </data>\n </layer>\n' % [comp_attr, encode(ENCODING_PATTERN, comp)]
		id += 1
	s += '</map>\n'
	return write_text("encodings.tmx", s)


static func chunk_gids(cells: Dictionary) -> Array:
	var arr: Array = []
	arr.resize(256)
	arr.fill(0)
	for idx: int in cells:
		arr[idx] = cells[idx]
	return arr


func gen_infinite_map() -> bool:
	# Chunk A at (-16,0): gid 1 at local index 0 -> (-16,0); gid 2 at index 255 -> (-1,15)
	# Chunk B at (0,16):  gid 3 at local index 0 -> (0,16);  gid 4 at index 17  -> (1,17)
	var chunk_a := chunk_gids({0: 1, 255: 2})
	var chunk_b := chunk_gids({0: 3, 17: 4})
	var s := '<?xml version="1.0" encoding="UTF-8"?>\n'
	s += '<map version="1.10" tiledversion="1.11.2" orientation="orthogonal" renderorder="right-down" width="32" height="32" tilewidth="16" tileheight="16" infinite="1" nextlayerid="3" nextobjectid="1">\n'
	s += ' <tileset firstgid="1" source="terrain.tsx"/>\n'
	s += ' <layer id="1" name="ChunksCSV" width="32" height="32">\n  <data encoding="csv">\n'
	s += '   <chunk x="-16" y="0" width="16" height="16">\n' + to_csv(chunk_a, 16) + '\n</chunk>\n'
	s += '   <chunk x="0" y="16" width="16" height="16">\n' + to_csv(chunk_b, 16) + '\n</chunk>\n'
	s += '  </data>\n </layer>\n'
	s += ' <layer id="2" name="ChunksZlib" width="32" height="32">\n  <data encoding="base64" compression="zlib">\n'
	s += '   <chunk x="-16" y="0" width="16" height="16">\n    ' + encode(chunk_a, "zlib") + '\n   </chunk>\n'
	s += '   <chunk x="0" y="16" width="16" height="16">\n    ' + encode(chunk_b, "zlib") + '\n   </chunk>\n'
	s += '  </data>\n </layer>\n'
	s += '</map>\n'
	return write_text("infinite.tmx", s)


# ---------------------------------------------------------------------------
# JSON map and project file

func gen_json_map() -> bool:
	var data: Array = [1, 2, 3, 4, 9, 10, 0, 0, 0, 0, 0, 0, 2 | FLIP_V, 0, 0, 1]
	var m := {
		"type": "map", "version": "1.10", "tiledversion": "1.11.2",
		"orientation": "orthogonal", "renderorder": "right-down",
		"width": 4, "height": 4, "tilewidth": 16, "tileheight": 16,
		"infinite": false, "nextlayerid": 3, "nextobjectid": 3,
		"tilesets": [{
			"firstgid": 1, "name": "jtiles", "image": "tiles.png",
			"imagewidth": 128, "imageheight": 64, "tilewidth": 16, "tileheight": 16,
			"tilecount": 32, "columns": 8, "margin": 0, "spacing": 0,
			"tiles": [{"id": 0, "properties": [
				{"name": "data_speed", "type": "int", "value": 5},
				{"name": "kind", "type": "string", "value": "json-grass"},
			]}],
		}],
		"layers": [
			{"id": 1, "name": "JTiles", "type": "tilelayer", "width": 4, "height": 4,
				"x": 0, "y": 0, "opacity": 1, "visible": true, "data": data},
			{"id": 2, "name": "JObjects", "type": "objectgroup", "draworder": "topdown",
				"x": 0, "y": 0, "opacity": 1, "visible": true, "objects": [
					{"id": 1, "name": "JRect", "type": "", "x": 16, "y": 16, "width": 16, "height": 16,
						"rotation": 0, "visible": true, "properties": [
							{"name": "p_int", "type": "int", "value": 42},
							{"name": "p_str", "type": "string", "value": "json"},
							{"name": "stats", "type": "class", "propertytype": "Stats",
								"value": {"hp": 5, "label": "orc"}},
						]},
					{"id": 2, "name": "JPoint", "point": true, "x": 40, "y": 48, "width": 0, "height": 0,
						"rotation": 0, "visible": true},
				]},
		],
	}
	return write_text("json_map.tmj", JSON.stringify(m, "  ", false) + "\n")


func gen_project_file() -> bool:
	var p := {
		"automappingRulesFile": "",
		"commands": [],
		"compatibilityVersion": 1100,
		"extensionsPath": "extensions",
		"folders": ["."],
		"properties": [],
		"propertyTypes": [
			{"id": 1, "name": "Enemy", "type": "class", "useAs": ["object"],
				"color": "#ffa0a0a4", "drawFill": true, "members": [
					{"name": "hp", "type": "int", "value": 100},
					{"name": "speed", "type": "float", "value": 1.5},
				]},
			{"id": 2, "name": "Stats", "type": "class", "useAs": ["property"],
				"color": "#ffa0a0a4", "drawFill": true, "members": [
					{"name": "hp", "type": "int", "value": 1},
					{"name": "label", "type": "string", "value": ""},
				]},
		],
	}
	return write_text("custom.tiled-project", JSON.stringify(p, "  ", false) + "\n")


# ---------------------------------------------------------------------------
# Zip bundle: maps/zipmap.tmx + maps/zip_tiles.tsx + maps/zip_tiles.png

func gen_zip() -> bool:
	var tsx := '<?xml version="1.0" encoding="UTF-8"?>\n'
	tsx += '<tileset version="1.10" tiledversion="1.11.2" name="zip_tiles" tilewidth="16" tileheight="16" tilecount="2" columns="2">\n'
	tsx += ' <image source="zip_tiles.png" width="32" height="16"/>\n'
	tsx += '</tileset>\n'
	var tmx := '<?xml version="1.0" encoding="UTF-8"?>\n'
	tmx += '<map version="1.10" tiledversion="1.11.2" orientation="orthogonal" renderorder="right-down" width="4" height="4" tilewidth="16" tileheight="16" infinite="0" nextlayerid="3" nextobjectid="2">\n'
	tmx += ' <tileset firstgid="1" source="zip_tiles.tsx"/>\n'
	tmx += ' <layer id="1" name="ZipLayer" width="4" height="4">\n  <data encoding="csv">\n' + to_csv([1, 2, 1, 2, 2, 1, 2, 1, 0, 0, 0, 0, 0, 0, 0, 1], 4) + '\n</data>\n </layer>\n'
	tmx += ' <objectgroup id="2" name="ZipObjects">\n  <object id="1" name="ZipPoint" x="8" y="8"><point/></object>\n </objectgroup>\n'
	tmx += '</map>\n'
	var png := FileAccess.get_file_as_bytes(OUT_DIR.path_join("zip_tiles.png"))
	if png.is_empty():
		printerr("zip_tiles.png missing")
		return false

	var zip_path := ProjectSettings.globalize_path(OUT_DIR.path_join("bundle.zip"))
	var packer := ZIPPacker.new()
	if packer.open(zip_path) != OK:
		printerr("could not create bundle.zip")
		return false
	var entries := {
		"maps/zipmap.tmx": tmx.to_utf8_buffer(),
		"maps/zip_tiles.tsx": tsx.to_utf8_buffer(),
		"maps/zip_tiles.png": png,
	}
	for entry: String in entries:
		packer.start_file(entry)
		packer.write_file(entries[entry])
		packer.close_file()
	packer.close()
	return true
