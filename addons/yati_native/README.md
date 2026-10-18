# yati_native: the YATI runtime importer as a GDExtension

A native (Rust, [godot-rust](https://github.com/godot-rust/gdext)) version of this repository's
GDScript runtime importer (`runtime/`): it imports Tiled maps (`.tmx`, `.tmj`, from the file
system or a zip archive) into the same node tree, with the same options, metadata and messages,
several times faster. `Importer.gd` here has the GDScript runtime's option variables, its four
`import*` methods and `last_error_count` / `last_warning_count`, so a project switches by
changing one preload path.

The plan, the measurements behind it and the stages:
[docs/native-importer-plan.md](../../docs/native-importer-plan.md).

## Status: stage 4 (the streamer)

Maps import natively and the GDScript runtime's whole test suite (`test/run_tests.gd`, 103
tests) passes against this facade, the runtime's 3.1 features included: object references as
NodePaths, scenes for classes and node properties from custom properties, map bounds and world
files, int and flag-enum layer masks. `import_async()` imports a map during play without a hitch
(the parse on a worker thread, the build in one frame), and TileSets and textures are cached
across imports. `TiledStreamer` streams an infinite (or large finite) map, or a whole Tiled world,
chunk by chunk as a camera moves, within a per-frame budget ([Streaming](#streaming-tiledstreamer)).

## Requirements

Godot 4.7 or later (`compatibility_minimum = 4.7`). Libraries are built for Linux x86_64,
Windows x86_64 and macOS (universal); Linux arm64 builds locally with cargo; web is planned
(stage 5).

## Installing

Copy `addons/yati_native/` into your project, with the libraries in its `bin/`:

- **Prebuilt:** from this repository's checkout, `bash tools/install_native.sh` downloads the
  current branch's rolling build (release tag `native-<branch>`, falling back to `native-main`),
  or the release named in `.yati-native-version` or as an argument (`v0.1.0`), into `bin/`.
  On a branch whose CI has run, `bin/` is also committed (`.github/workflows/native.yml`).
- **From source:** with a stable Rust toolchain, `bash tools/build_native.sh` builds `native/`
  and copies this platform's library into `bin/` (`--check` only tells whether it is there).
  A local build is not committed.

Godot loads a GDExtension when its `.godot/extension_list.cfg` names it; the editor writes that
file when it imports the project. On a fresh `.godot/`, write
`res://addons/yati_native/yati_native.gdextension` into it before the first
`godot --headless --import`: Godot 4.7.2 otherwise discovers the extension mid-import and
aborts at shutdown (exit code 134; the second run is clean). `test/run_all.sh` does that.

The addon stands alone: `Importer.gd`, `WorldLoader.gd`, `PostProcessing.gd` (the runtime's,
unchanged), a two-line `CommonUtils.gd` and a small `TilemapCreator.gd` stand-in are all the
GDScript it has.

## Usage

```gdscript
const Importer = preload("res://addons/yati_native/Importer.gd")

var importer := Importer.new()
importer.parallax_mode = "parallax2d"   # any option of the GDScript runtime (Runtime.md)
var map: Node = importer.import("res://maps/desert.tmx")
if map != null:
	add_child(map)
print(importer.last_error_count, " errors, ", importer.last_warning_count, " warnings")
```

### Importing during play: `import_async`

```gdscript
const Importer = preload("res://addons/yati_native/Importer.gd")

var importer := Importer.new()

func enter_room(path: String) -> void:
	var room: Node = await importer.import_async(path)   # the game keeps running meanwhile
	if room != null:
		add_child(room)
```

`import_async(map, project = "")` and `import_from_zip_async(zip, map_in_zip, project_in_zip = "")`
are coroutines with the results of `import()` / `import_from_zip()` (the same tree, the same
`last_error_count` / `last_warning_count` and summary line). Frame by frame:

1. `TiledMap.load_async`: the map, its tilesets, templates and project file are parsed on a worker
   thread. Each frame the main thread polls it once: it reads the files the parse asks for (two or
   three rounds) with `FileAccess` and hands them over, which costs about a millisecond per
   megabyte read; the frames in between cost nothing.
2. The map's imported (`res://`) textures load in the engine's threads
   (`ResourceLoader.load_threaded_request`) from the moment the parse names them.
3. The TileSets the map needs are created `async_tiles_per_frame` tiles per frame (default 256),
   unless the cache has them already.
4. The tree is built in one frame: the same work as `import()` without the parse, mostly the
   engine's own `tile_map_data` assignment and node creation (section 1.3 of the plan). That frame
   is the hitch that is left, and only a large map makes it long: the benchmark map's 522 358 cells
   and 2 720 objects take about 170 ms in it; a small map (60 × 33 cells) about 1 ms. Larger maps
   are what the streamer (stage 4) is for.

On the benchmark map no frame before the build costs more than 2 to 3 ms of main-thread time (the
first one, which reads the 2 MB file). The coroutine keeps its importer alive
(`await Importer.new().import_async(path)` works); await it from an object that outlives it.

The same steps by hand, with the classes (for a loading screen with progress, say):

```gdscript
var map = ClassDB.instantiate("TiledMap")
map.load_async("res://maps/level.tmx", "")
while map.poll() < map.READY:      # READING, PARSING, WAITING_FILES, then READY or FAILED
	await get_tree().process_frame
if map.get_status() == map.READY:
	var tilesets: RefCounted = Importer.shared_tilesets()
	while not tilesets.build_step(map, null, 256):
		await get_tree().process_frame
	add_child(ClassDB.instantiate("TiledBuilder").build(map, tilesets, null))
```

`map.cancel()` stops a load (its thread ends on its own, texture requests already made are
collected in the next frames); freeing the `TiledMap` mid-load does the same. `loaded` and `failed`
are emitted from the `poll()` that ends a load, so `await map.loaded` works while something else
polls.

### Streaming: `TiledStreamer`

A map too large to build in one frame, an infinite map, or a world of maps is streamed: a
`TiledStreamer` (a `Node2D`) shows the part of it around a window and keeps nothing else.

```gdscript
const Importer = preload("res://addons/yati_native/Importer.gd")

func _ready() -> void:
	var streamer: TiledStreamer = Importer.new().make_streamer("res://maps/overworld.tmx")
	streamer.follow = $Player              # the window follows this node
	streamer.margin = Vector2i(48, 32)     # cells around the player's chunk (cover the screen)
	streamer.budget_us = 2000              # work per frame
	add_child(streamer)
	await streamer.settled                 # the first window is there
```

`make_streamer(map_or_world, options = null, project_file = "")` takes a map path (loaded with
`TiledMap.load_async`: the streamer starts once it is loaded), a world path (`.world`), or a
`TiledMap` / `TiledWorld`, and gives a streamer with the importer's options and the shared TileSet
cache. The same with the class:

```gdscript
var map := TiledMap.new()
map.load("res://maps/overworld.tmx", "")       # or load_async: the streamer polls it
var streamer := TiledStreamer.new()
streamer.map = map
streamer.window = Rect2i(0, 0, 120, 70)        # cells; or follow + margin
add_child(streamer)
```

**What it does each frame** (`_process`, or `step(budget_us)` by hand, after
`set_process(false)` once the streamer is in the tree): the chunks (`chunk_size`,
16 × 16 cells) of every tile layer that the window covers and that are not loaded are loaded,
nearest first, with `set_cell` into one persistent `TileMapLayer` per Tiled tile layer (made and
configured as `import()` makes it: names, parallax, tint, offsets, blend modes, visibility,
properties); chunks outside the window grown by `keep_margin` (16 cells: a camera moving back and
forth across a chunk border does not reload it) are erased with `erase_cell`. An infinite map's
chunks are decoded from the file's payloads when they are wanted, so a loaded map costs about its
file size in memory (1.6 times it for the 58 MB benchmark map); a finite map's layers are decoded
once, on a thread. Image layers and the background colour are made with the tree, not streamed.

**The budget.** The streamer measures its own work, from the first chunks on: µs per chunk, per
cell, per cell whose tile has a collision polygon (collision bodies are the engine's dearest part),
per y-sorted cell, per object, per erased cell and per freed object (`get_costs()`). Before each
piece of work it estimates its cost and stops the frame's work when the next piece would exceed
`budget_us` (one piece always runs, so a frame does at least that). The engine's work for a chunk
(its rendering quadrant and physics bodies) is done in the streamer's frame
(`TileMapLayer.update_internals()`), so what the budget counts is what the frame pays: on the
benchmark map at a 4 ms budget, frames without the streamer's own work cost what frames with the
streamer paused cost. `get_stats()` counts the work (`tile_chunks_loaded`, `objects_spawned`,
`budget_stops`: frames that stopped at the budget, `max_work_us`...). A budget the window outruns
shows as chunks appearing late, never as a long frame: at two screens per second over four dense
layers of 16 px tiles, 4 ms per frame loads about 200 chunks a second here, about what that speed
needs.

**Objects** are indexed by the chunk their position (`x`, `y`) lies in and created when that
chunk enters the window, by the same code as `import()` (classes, templates, scenes, properties,
`_tiled_import`), under one node per object layer (`draworder="index"` layers keep their order);
they are freed when the chunk leaves. Signals: `object_spawned(node, id)`,
`object_despawned(id)`. An object property that names an object of another chunk is a NodePath
to it while both exist and an empty NodePath otherwise (no warning), set again whenever the
target comes or goes. `keep_objects_resident = true` keeps objects once created (for objects that
carry game state); a world map that leaves still frees its objects.

**World mode.** `streamer.world = world` (a loaded `TiledWorld`) or `make_streamer("....world")`:
each map is placed at its rect, loaded with `TiledMap.load_async` when the window comes within
`world_margin` cells of it, streamed like one map, and unloaded chunk by chunk, then entirely,
once the window is `keep_margin` farther away (`map_loaded(path, root)`, `map_unloaded(path)`).
In world mode `window` and the follow margin are in cells of `world_cell_size` (16 × 16 px);
`project_file` is the project the maps load with. Maps with the same tilesets share one TileSet.

**Everything at once.** `load_all()` loads the whole map (or every map of a world, loaded
synchronously) in one call: the same tile and object content as `import()`, and the window stays
unbounded until `window` or `follow` is set.

**Y-sorted layers** (the tile layers of isometric and staggered maps) are refused by default, with
an error: the engine draws their cells one by one and re-sorts all of a layer's cells in each frame
that changes it, after the streamer's work, where no budget reaches (+4 ms over the idle frame for
one chunk into a layer of 3 500 cells, +8 ms into 87 000, on this repository's software-rendered
test machine). `allow_y_sorted_layers = true` streams them anyway, budgeted at their own measured
cost (about 14 µs per cell, 8 times an ordinary cell's); `load_all()` always takes them. Object
layers sorted by y (`draworder="topdown"`) stream normally.

**Things to know.** Every tile of the map's TileSet is created before the first cell is set: the
engine rebuilds every cell of every layer that uses a TileSet when the TileSet changes (40 to 60 ms
with 30 000 cells). So keep `no_alternative_tiles` on (the default) when streaming: off, a flipped
tile's first appearance creates an alternative tile, which is such a change. The engine's
`TileMapLayer` keeps one object, about 1.3 KB, for every 16 × 16 area it has ever held cells in,
even once they are erased, until the layer is freed (Godot 4.7.2, whatever the quadrant sizes): a
layer streamed across all of a 4 000 × 4 000 map would hold about 80 MB. Most games cross a small
part of such a map; world mode frees a map's layers with the map; a streamer freed and made again
starts from nothing. The streamer's
signals are emitted after its frame's work; a `_tiled_import` hook or a `godot_script` of an object
runs during it and must not call the streamer. `get_layer(name)`, `get_object_layer(name)`,
`get_map_root(path)`, `get_loaded_maps()`, `loaded_chunk_count()`, `get_loaded_chunks(layer)`,
`get_spawned_object_count()`, `is_settled()`, `get_error_count()` and `get_warning_count()` tell
what is there. `test/bench_stream.gd` is the benchmark (an infinite map of 4 000 × 4 000 cells
scrolled at two screens per second, a world of 3 × 3 maps, an isometric map); the plan's section
6, stage 4, has its numbers.

### Shared TileSets and textures

With `share_tilesets` on (the default), every `Importer` uses one TileSet cache,
`Importer.shared_tilesets()` (a `TiledTileSets`: `TiledTileSets.shared()`, which streamers use
by default too): a map whose tileset combination an earlier import
built gets that TileSet instead of a new one. The combination is the ordered list of tilesets with
their first gids (paths of external tilesets, a hash of embedded ones), the map's tile size,
orientation and stagger, the zip archive, the project file's types, and the options that shape a
TileSet (`custom_data_prefix`, `map_wangset_to_terrain`, `animation_mode`, `no_alternative_tiles`).

- **Shared by reference.** A cached TileSet is the very object a previous map used: the builder
  adds the tiles and alternative tiles each map needs to it, and a change a game makes to it (a
  tile's modulate, a custom data value) shows in every later map that gets it.
- **Exclusive.** The shared cache hands a TileSet only to a map no live tree shares it with: while
  a tree holds it, an import of a map with the same tilesets gets a second one (cached too), so
  two trees alive at once never share a TileSet, as with the GDScript runtime. A map loaded after
  the previous one's tree was freed reuses it: no TileSet creation. Set
  `Importer.shared_tilesets().exclusive = false` to share one TileSet between live trees as well
  (one TileSet per combination, whatever is alive).
- **Messages.** A cached TileSet's warnings and errors are printed and counted by every import that
  uses it, so the counts of an import do not depend on what the cache holds.
- **`save_tileset_to`** makes an import build its own TileSet (the saved file is that map's).
- **Files that change.** The cache keys on paths, not on file contents: after a tileset or image
  changed on disk (a mod, a generated `user://` file), call `Importer.shared_tilesets().clear()`
  (TileSets) and `.clear_textures()` (the per-process texture cache, which every `TiledTileSets`
  uses). Cached TileSets and textures stay in memory until then.

`share_tilesets = false` gives every import its own fresh cache (stage 2's behaviour).

### The other methods

`import(map, project = "")`, `import_from_zip(zip, map_in_zip, project_in_zip = "")`,
`import_with_post_processing(map, script, project = "")`,
`import_from_zip_with_post_processing(...)` and `import_world(world_file, project = "")` (every
map of a Tiled `.world` file under one `Node2D` named after it, each map's root at its place)
behave as the GDScript runtime's ([Runtime.md](../../Runtime.md)).

The options are the runtime's, with its defaults ([Reference.md](../../Reference.md) explains
each): `use_default_filter`, `add_class_as_metadata`, `add_id_as_metadata`,
`no_alternative_tiles`, `map_wangset_to_terrain`, `custom_data_prefix`, `save_tileset_to`,
`parallax_mode`, `animation_mode`, `always_wrap_root`, `unknown_class_fallback`, and the part B
ones: `object_refs` (`"nodepath"`, `"geometry"` or `"both"`: what an object property becomes),
`class_scenes` (Tiled class, case-insensitive, to a scene path), `read_godot_scene_from_custom_types`
(a project class's `godot_scene` member maps the class to that scene) and `assign_node_properties`
(custom properties set node properties of the same name, typed as the runtime types them; a node
with a `_tiled_import(info)` method gets it called once the map is complete). Every imported root
carries the metadata `map_rect` (the map's bounds in pixels), `map_size_tiles`, `tile_size` and
`orientation`.

`WorldLoader.gd` is the runtime's: `WorldLoader.parse(world_file, zip_file = "")` returns the
maps of a world file as `{"path": String, "rect": Rect2i}` (explicit `maps` first, then what its
`patterns` list, by file name), `WorldLoader.clear_cache()` exists for the runtime's API (nothing
is cached here: a pattern's map sizes are read on every `parse`). `TilemapCreator.gd` stands in for
the runtime's class of that name where code creates maps through it: `create(map)` (an import with
the default options), `get_map_rect()` and `get_tileset()` of the last one. `Importer.native_version()` returns the loaded library's
version, or `""` when the library is missing for this platform (an import then fails with a
message and returns `null`). `importer.make_options()` gives the options as a
`TiledImportOptions` for the native classes.

### The native classes

The facade is a thin layer over these classes, which a project can use directly (reach them
through `ClassDB` if the script must compile without the library):

| Class | What |
|---|---|
| `TiledImportOptions` (Resource) | every option, same names and defaults as the facade's variables |
| `TiledMap` | `load(path, project_file) -> Error`, `from_zip(zip, path_in_zip, project_in_zip) -> Error`; `load_async(...)`, `from_zip_async(...)`, then `poll() -> int` once per frame (`READING`, `PARSING`, `WAITING_FILES`, `READY`, `FAILED`), `get_status()`, `cancel()`, signals `loaded` and `failed`; `is_loaded()`, `get_path()`, `get_size()` (cells), `get_tile_size()`, `get_orientation()`, `is_infinite()`, `get_layers()` (`{name, type, id, depth}` per layer, depth first), `to_dictionary()` (the GDScript runtime's `DictionaryBuilder` dictionary of the map, custom types merged), `get_errors()`, `get_warnings()` |
| `TiledTileSets` | the TileSet builder and its cache: `TiledTileSets.shared()` (the process-wide one, exclusive), `tileset_for(map, options) -> TileSet` (built on first use, then cached per tileset combination and shared by reference with every later map), `build_step(map, options, tiles_per_step) -> bool` (create a map's TileSets a few tiles per call; true when ready), `exclusive` (hand a TileSet only to a map no live tree shares it with), `clear()`, `clear_textures()` (the per-process texture cache), `get_cached_count()`, `get_created_count()`, `get_texture_count()`, `get_last_error_count()`, `get_last_warning_count()` |
| `TiledBuilder` | `build(map, tilesets, options) -> Node`: the whole tree in one call, the root parentless; `get_error_count()`, `get_warning_count()`, `get_messages()` of that build |
| `TiledWorld` | `load(world_file, zip_file) -> Error` (the world file, its directory and the maps its patterns need read on the main thread, or from the zip), `get_maps()` (`{path, rect}` per map), `get_errors()` |
| `TiledStreamer` (Node2D) | `map` or `world`, `tilesets` (null: `TiledTileSets.shared()`), `options`, `project_file`; `window` (Rect2i, cells) or `follow` (Node2D) + `margin` (cells); `keep_margin`, `world_margin`, `world_cell_size`, `budget_us` (2000; `<= 0` unbounded), `keep_objects_resident`, `chunk_size` (16 × 16), `allow_y_sorted_layers`; `step(budget_us) -> bool`, `load_all()`, `is_settled()`, `loaded_chunk_count()`, `get_loaded_chunks(layer)`, `get_spawned_object_count()`, `get_layer(name)`, `get_object_layer(name)`, `get_map_root(path)`, `get_loaded_maps()`, `get_costs()`, `get_stats()`, `reset_stats()`, `get_error_count()`, `get_warning_count()`; signals `chunk_loaded(layer_name, coords)`, `chunk_unloaded(layer_name, coords)`, `object_spawned(node, id)`, `object_despawned(id)`, `map_loaded(path, root)`, `map_unloaded(path)`, `settled()` |
| `TiledNative` | `version()`, and `sum()` for the smoke test |

```gdscript
var map = ClassDB.instantiate("TiledMap")
if map.load("res://maps/desert.tmx", "") == OK:
	var tilesets = ClassDB.instantiate("TiledTileSets")
	var root: Node = ClassDB.instantiate("TiledBuilder").build(map, tilesets, null)
```

Files are read on the main thread through `FileAccess`, so `res://` paths inside an exported
.pck work; images through `ResourceLoader` (imported textures; in the engine's threads during an
asynchronous load) or `Image.load_from_file`. The worker thread of an asynchronous load runs
the pure-Rust core only (`native/tiled/`, which does not depend on Godot); godot-rust's
`experimental-threads` is not used.
`TiledMap.load` prints the load's errors and warnings as the runtime words them; `build` prints
its own. The facade adds both counts up and prints the runtime's summary line.

## How it differs from the GDScript runtime

The tree, names, metadata, groups, materials, options and messages are the runtime's (the test
suite is the specification). Where this importer differs:

- **Speed.** The benchmark map of `test/benchmark.gd` (320 × 320 cells in 6 layers, 2 720
  objects) imports in about a fifth of the GDScript runtime's time; most of what is left is the
  engine's own `tile_map_data` assignment and node creation. A small map (`layers.tmx`) takes
  about 2 ms warm (1.6 to 2.4 ms here, the GDScript runtime 3 to 5). Numbers: the plan's section 6, stage 2.
- **Tile layers** are filled with one `tile_map_data` assignment each, from blobs decoded on one
  thread per layer. The blob format is engine-internal: it is checked once per process against
  the engine, and should it ever differ the layers are filled with `set_cell` instead (an error
  says so).
- **No internal custom data layer.** The runtime marks tiles with collision shapes in a custom
  data layer `__internal__` that it removes at the end of an import; this importer keeps that on
  the Rust side. Visible only where the runtime forgets to remove it: a TileSet saved with
  `save_tileset_to` and the TileSets of templates have one custom data layer less here.
- **Custom types** from a project file are merged into the tilesets that only templates use as
  well (the runtime's template tilesets get none).
- **References** (external tilesets, templates, images) are resolved relative to the file that
  makes them; the runtime first tries the bare name relative to the working directory.
- **JSON templates** (`.tj`) instantiate (below), so an object property naming such an instance
  resolves to its node; the runtime creates nothing for one and warns about a dangling reference.
- **Property lists** of engine classes (nodes without a script), which `assign_node_properties`
  matches custom properties against, are read once per process; the runtime reads them once per
  import. The same unless an engine class's property list depends on the node's state.
- **`TilemapCreator.gd`** is only a stand-in (`create`, `get_map_rect`, `get_tileset`; options go on
  `Importer.gd`), and its `create` prints the import's summary line.
- **Parsing** follows `tiled_core` ([native/tiled/README.md](../../native/tiled/README.md)):
  JSON templates (`.tj`) instantiate, the format is told by content, and a few malformed inputs
  that no Tiled version writes are read differently.

## Building with fewer checks

`cargo build --release --features unchecked` (in `native/`) turns godot-rust's per-call checks
(is the object still alive) off in the release build: about 30 % off the object phase of a large
map, at the price of undefined behaviour instead of a panic should a node be freed while the
builder holds it (a script's `_init` freeing nodes of the tree being built, say). The CI builds
do not use it.
