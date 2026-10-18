# YATI runtime importer

The runtime importer turns a Tiled map (.tmx/.tmj) into a Godot node tree while your game is running.
(The idea of a runtime package was proposed by Jeff Brooks in upstream issue #16.)

## Installation

Copy the `runtime` folder with its entire content somewhere into your Godot project.
The folder name (`runtime`) can be changed if you wish. Requires Godot 4.3 or higher.

## Usage

Create an importer instance from `Importer.gd`, optionally set options on it and call one of the import methods:

| Method | Parameters |
|--------|------------|
| `import` | path to the .tmx/.tmj, optional path to the Tiled project file (.tiled-project) for Custom Types |
| `import_from_zip` | path to the zip file, path *inside* the zip to the .tmx/.tmj, optional path inside the zip to the project file |
| `import_with_post_processing` | path to the .tmx/.tmj, path to the post-processing script, optional project file |
| `import_from_zip_with_post_processing` | zip file, map path inside the zip, path to the post-processing script, optional project file inside the zip |
| `import_world` | path to a Tiled world file (.world), optional project file: every map of the world under one `Node2D`, each at its place |

- Paths may be `res://`, `user://` or absolute paths. References inside the map (tilesets, images, templates)
  are resolved relative to the file that contains them, as in Tiled.
- The return value is the root node of the created tree (usually a `Node2D`; a map with a single layer returns
  that layer's node directly, with the map's properties applied on top of the layer's, unless `always_wrap_root`
  is set). **The returned node has no parent** - add it directly with `add_child()`, there is
  no need to `duplicate()` it.
- On a fatal error (e.g. map file not found) `null` is returned.
- Each call prints a one-line summary ("Import of '...' succeeded." or "... finished with N error(s) and M warning(s).");
  details are printed before it.

Mini example:

```gdscript
extends Node2D

func _ready():
	var importer = preload("res://runtime/Importer.gd").new()
	var map = importer.import("res://maps/desert.tmx")
	# With Tiled Custom Types:
	# var map = importer.import("res://maps/desert.tmx", "res://maps/desert.tiled-project")
	# From a zip archive:
	# var map = importer.import_from_zip("user://downloads/desert.zip", "tmx/desert.tmx", "desert.tiled-project")
	if map != null:
		add_child(map)
	if importer.last_error_count > 0:
		push_warning("Map imported with %d error(s)" % importer.last_error_count)
```

## Options

Options are plain variables on the importer instance. Set them before calling an import method;
they apply to every following call on that instance. See the [Reference](Reference.md#options) for details.

| Variable | Type | Default | Meaning |
|----------|------|---------|---------|
| `use_default_filter` | bool | `false` | Keep Godot's default texture filter instead of forcing 'nearest' on the created layers |
| `add_class_as_metadata` | bool | `true` | Store the Tiled class as metadata `class` on created nodes, tiles and the TileSet |
| `add_id_as_metadata` | bool | `false` | Store the Tiled object id as metadata `id` on created nodes |
| `no_alternative_tiles` | bool | `true` | Flipped/rotated tiles use the engine's transform flags instead of generated alternative tiles |
| `map_wangset_to_terrain` | bool | `false` | Use the old wangset mapping (wangset -> terrain instead of terrain set) |
| `custom_data_prefix` | String | `"data_"` | Tile properties with this name prefix become TileSet custom data layers |
| `save_tileset_to` | String | `""` | If set, the generated TileSet is saved to this resource path |
| `parallax_mode` | String | `"parallax2d"` | Layers with a parallax factor: `"parallax2d"` wraps each in a Parallax2D at its place in the hierarchy; `"legacy"` moves them into one ParallaxBackground (ParallaxLayers), as before 3.1 |
| `animation_mode` | String | `"strip"` | Tile animations: `"strip"` gives each animated tile an atlas of its frames in their order (any sequence); `"atlas"` animates in the tileset's atlas as before 3.1 (only frames that follow the tile at equal steps) |
| `always_wrap_root` | bool | `false` | Return a Node2D root (named after the map, holding the map's properties) also for a map with a single layer, instead of that layer; recommended, and planned as the default of 4.0 |
| `unknown_class_fallback` | String | `"staticbody"` | What an object whose class is no Godot node type becomes: `"staticbody"`, another class of the mapping table (e.g. `"area"`) or `"node2d"` (a Node2D with the shape as metadata) |
| `class_scenes` | Dictionary | `{}` | Tiled class (case-insensitive) → scene path: objects and tile objects of that class become instances of the scene (like class `instance` with `res_path`) |
| `read_godot_scene_from_custom_types` | bool | `true` | With a project file, a custom class's `godot_scene` (file) member maps its objects and tiles to that scene |
| `assign_node_properties` | bool | `true` | A custom property named like a property of the created node (a script's `@export var`, ...) is assigned to it, converted to its type; others stay metadata. A node method `_tiled_import(info)` is called once the map is built. Off: all metadata, as before 3.1 |
| `object_refs` | String | `"nodepath"` | Object properties: `"nodepath"` gives a NodePath to the target object's node (or the node, for a Node-typed property of that name), lists of objects an `Array[NodePath]`; `"geometry"` the target's points, as before 3.1; `"both"` the NodePath and the points as metadata `<name>_shape` |

```gdscript
var importer = preload("res://runtime/Importer.gd").new()
importer.use_default_filter = true
importer.custom_data_prefix = ""   # every tile property becomes custom data
var map = importer.import("res://maps/desert.tmx")
```

## Result

After every import call:

- `importer.last_error_count` - number of errors reported during the last import (a `null` result always counts at least one)
- `importer.last_warning_count` - number of warnings reported during the last import

The counters are reset at the start of each call.

## Worlds and map bounds

Every imported root has metadata `map_rect` (the map's bounds in pixels, `Rect2`), `map_size_tiles`, `tile_size` and
`orientation`. `WorldLoader.gd` reads Tiled world files:

| Function | Returns |
|----------|---------|
| `WorldLoader.parse(world_file, zip_file = "")` | an Array of `{"path": String, "rect": Rect2i}`: the world's `maps` entries, then the `.tmx`/`.tmj` files its `patterns` match (sizes read from the maps, cached; `WorldLoader.clear_cache()`) |
| `Importer.import_world(world_file, project_file = "")` | a `Node2D` holding every map's root at its position (for small worlds and tests; a streamer imports per map) |

```gdscript
const WorldLoader = preload("res://runtime/WorldLoader.gd")
var importer = preload("res://runtime/Importer.gd").new()
for entry in WorldLoader.parse("res://maps/overworld.world"):
	if entry.rect.intersects(visible_area):
		var map = importer.import(entry.path)
		map.position = Vector2(entry.rect.position)
		add_child(map)
```

See the [Reference](Reference.md#world-files-and-map-bounds).

## Zip archives

`import_from_zip` reads the map, tilesets, templates, images and the project file from the archive.
The archive is opened once per call and closed when the call returns; it is **scoped to that one call** -
a later `import()` on the same (or any other) importer reads from the file system again.

**Note:** Godot resources referenced by properties (.tres/.res/.tscn, e.g. `res_path`, `material`) are always
loaded with Godot's resource loader and cannot come from the zip. The same applies to the post-processing script.

## Post-processing

The post-processing methods load the given GDScript (it must be a project resource), instantiate it and call its
method `_post_process(base_node)` with the imported root node. The method must return the (changed) node:

```gdscript
extends Node

func _post_process(base_node: Node):
	var area = base_node.find_child("Area2")
	var mat = CanvasItemMaterial.new()
	mat.blend_mode = CanvasItemMaterial.BLEND_MODE_MUL
	area.material = mat
	return base_node
```

If you don't use post-processing you can delete `PostProcessing.gd` and the two post-processing methods in `Importer.gd`.

## Performance tips

- Textures are loaded once per import even if referenced many times (image collections, templates).
- Leave `no_alternative_tiles` on (default): generating alternative tiles for flipped tiles is slower.
- Template files are read and parsed once per import, no matter how many instances the map contains.
