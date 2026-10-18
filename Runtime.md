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

## Native importer

`addons/yati_native/` is a native version of this importer: a GDExtension written in Rust
([godot-rust](https://github.com/godot-rust/gdext)) behind an `Importer.gd` with this runtime's
API. It makes the same node tree, with the same options, metadata and messages (this runtime's
test suite passes against both), and adds what the GDScript runtime cannot do: importing a map
during play without a hitch, sharing TileSets across imports, and streaming a large or infinite
map, or a whole Tiled world, chunk by chunk.

**When to use which.** The GDScript runtime needs nothing but Godot 4.3, runs on every platform
and is the reference: use it for small maps loaded at a loading screen, and on platforms the
native library is not built for. Use the native importer when a map is loaded while the game
runs (it is 4 to 5 times faster, and `import_async` keeps the parse off the main thread), when
maps are large, or when they should stream. It needs Godot 4.7 and a library for the platform
(Linux x86_64, Windows x86_64 and macOS are built; Linux arm64 builds with cargo; web and mobile
are not built yet, see the addon's README).

| Benchmark map (320 × 320 cells in 6 layers, 2 720 objects) | GDScript runtime | native |
|---|---|---|
| `import()` | 670 to 770 ms | 160 to 200 ms (most of it the engine's own `tile_map_data` and node calls) |
| main-thread time per frame of `import_async()` before its build frame | (no async import) | under 2 to 3 ms |
| a small map (60 × 33 cells, 2 layers, 30 objects), warm | 3 to 5 ms | about 1 ms |

### Installation

1. Copy `addons/yati_native/` into your project (it holds its own `Importer.gd`,
   `PostProcessing.gd`, `WorldLoader.gd`; it does not need `runtime/`).
2. Put the libraries into its `bin/`: download `yati-native.zip` from this repository's releases
   (a versioned release `v<version>` or a branch's rolling build `native-<branch>`) and unpack it
   into the project, or, from a checkout of this repository, run `bash tools/install_native.sh`
   (downloads it) or `bash tools/build_native.sh` (builds this machine's library with cargo).
3. Godot loads a GDExtension when `.godot/extension_list.cfg` names it; the editor writes the file
   when it opens the project. Headless, on a fresh `.godot/`, write the line
   `res://addons/yati_native/yati_native.gdextension` into that file before the first
   `godot --headless --import`: Godot 4.7.2 otherwise discovers the extension mid-import and aborts
   at shutdown (exit code 134).

Then change the preload path:

```gdscript
var importer = preload("res://addons/yati_native/Importer.gd").new()
var map = importer.import("res://maps/desert.tmx")   # the same methods, options and results
```

`Importer.native_version()` returns the library's version, or `""` when it is missing for the
platform (an import then prints why and returns `null`).

### The same API

`import`, `import_from_zip`, `import_with_post_processing`, `import_from_zip_with_post_processing`
and `import_world` take the same arguments and return the same trees; `last_error_count` and
`last_warning_count` and the summary line are the same; `WorldLoader.parse()` is there too. Every
option of the table above exists with the same default, plus two:

| Variable | Type | Default | Meaning |
|----------|------|---------|---------|
| `share_tilesets` | bool | `true` | Every importer uses one TileSet cache (`Importer.shared_tilesets()`): a map whose tilesets an earlier import built gets that TileSet instead of a new one (below). Off: every import builds its own, as the GDScript runtime does |
| `async_tiles_per_frame` | int | `256` | `import_async`: TileSet tiles created per frame before the build frame |

### Importing during play: `import_async`

```gdscript
var importer = preload("res://addons/yati_native/Importer.gd").new()

func enter_room(path: String) -> void:
	var room: Node = await importer.import_async(path)   # the game keeps running
	if room != null:
		add_child(room)
```

`import_async(map_file, project_file = "")` and `import_from_zip_async(zip_file, map_in_zip,
project_in_zip = "")` are coroutines with the result of `import()` / `import_from_zip()`. The map,
its tilesets, templates and project file are parsed on a worker thread, which also decodes the tile
layers; the main thread only reads the files the parse asks for (two or three rounds) and hands
them over; the map's imported textures load in the engine's threads meanwhile; the TileSets are
created `async_tiles_per_frame` tiles per frame; then the tree is built in one frame. That last
frame is what is left of the hitch: the engine's own work for the cells and nodes (about 1 ms for a
small map, 150 to 200 ms for the benchmark's 522 358 cells and 2 720 objects). A map that large is
what the streamer is for.

### Worlds: `import_world`

`import_world(world_file, project_file = "")` imports every map of a Tiled `.world` file, as the
GDScript runtime does. To load a world's maps as the player approaches them, stream it (below).

### Streaming: `make_streamer`

`make_streamer(map_or_world, options = null, project_file = "")` returns a `TiledStreamer`, a
`Node2D` that shows the part of a map (finite or infinite) or of a world around a window and keeps
nothing else in the tree. Follow mode:

```gdscript
func _ready() -> void:
	var importer = preload("res://addons/yati_native/Importer.gd").new()
	var streamer = importer.make_streamer("res://maps/overworld.tmx")   # or a .world file
	streamer.follow = $Player              # the window follows this node...
	streamer.margin = Vector2i(48, 32)     # ...this many cells around its chunk (cover the screen)
	streamer.budget_us = 2000              # the streamer's work per frame, in µs
	add_child(streamer)
	await streamer.settled                 # the first window is there
```

Each frame it loads the 16 × 16 chunks the window covers into one `TileMapLayer` per Tiled tile
layer (`set_cell`), nearest first, and erases those that left the window by more than
`keep_margin`, within `budget_us` (it measures its own costs and stops before the piece of work
that would exceed it). Objects are created with the chunk their position lies in and freed with
it (`keep_objects_resident` keeps them), by the same code as `import()`; object references to
objects of other chunks are set whenever both exist. In world mode each map is loaded when the
window approaches it and freed when it leaves. `load_all()` loads everything at once. Signals:
`chunk_loaded`, `chunk_unloaded`, `object_spawned`, `object_despawned`, `map_loaded`,
`map_unloaded`, `settled`. Its properties and limits (y-sorted layers are refused unless
`allow_y_sorted_layers`; a `TileMapLayer` keeps memory for every area it has ever shown) are in
the addon's README.

### The shared TileSet cache

With `share_tilesets` on, a TileSet is cached per tileset combination (the map's tilesets and
their first gids, its tile size and orientation, the zip archive, the project's custom types, and
the options that shape a TileSet) and handed to the next map with that combination:

- **By reference.** A later map gets the very TileSet object; tiles the earlier maps needed stay in
  it, and a change a game makes to it shows in every map that gets it later.
- **Exclusive.** A TileSet goes only to a map no live tree shares it with, so two trees alive at
  once never share one (as with the GDScript runtime); a map loaded after the previous tree was
  freed reuses its TileSet. `Importer.shared_tilesets().exclusive = false` shares it between live
  trees too.
- **Files that change** on disk are not noticed: call `Importer.shared_tilesets().clear()` (the
  TileSets) and `.clear_textures()` (the textures, cached per process) after replacing a tileset
  or image.
- `save_tileset_to` always builds a TileSet of the import's own.

### What differs

| | GDScript runtime | native importer |
|---|---|---|
| Requirements | Godot 4.3, any platform | Godot 4.7, a library for the platform |
| TileSets | one per import | shared across imports (`share_tilesets`) |
| Textures | loaded per import | cached per process (`clear_textures()`) |
| Asynchronous import, streaming | no | `import_async`, `make_streamer` |
| Internal custom data layer `__internal__` | left in saved and template TileSets | never created |
| Custom types on tilesets only templates use | not merged | merged |
| References (tilesets, templates, images) | the bare name relative to the working directory is tried first | relative to the referencing file only |
| JSON templates (`.tj`) | create nothing | instantiate |
| A numeric object or layer name (`name="007"`) | `"7"` | `"007"` |

The full list, each with its reason, is in the [Reference](Reference.md#differences-between-the-gdscript-and-the-native-importer).
