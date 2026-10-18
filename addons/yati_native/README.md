# yati_native: the YATI runtime importer as a GDExtension

Imports [Tiled](https://www.mapeditor.org) maps (`.tmx`, `.tmj`, with `.tsx` / `.tsj` tilesets,
`.tx` / `.tj` templates and `.tiled-project` custom types, from the file system or a zip archive)
into Godot node trees while a game runs, and streams large or infinite maps and Tiled worlds chunk
by chunk. A native ([godot-rust](https://github.com/godot-rust/gdext)) version of the GDScript
runtime importer of [YATI_tweaks](https://github.com/Shadoku/YATI_tweaks) (`runtime/`, itself a
fork of [YATI](https://github.com/Kiamo2/YATI)): the same node tree, options, metadata and
messages, and the same `Importer.gd` API, so a project switches by changing one preload path.
This directory is all a project needs; nothing else of the repository is required.

## Features

- **The GDScript runtime's importer, natively.** Every Tiled layer kind (tile, object, image,
  group), infinite maps, every payload encoding (csv, base64, zlib, gzip, zstd, the legacy XML
  `<tile gid>` form), every orientation, every object kind, templates, tile collisions,
  navigation and occlusion, animations, terrains, custom properties and custom types, parallax,
  blend modes, tints; the runtime's options with its defaults, its metadata (`class`, `id`,
  `map_rect`, ...), object references as NodePaths, scenes for classes, node properties from custom
  properties and the `_tiled_import(info)` hook, world files, int and flag-enum masks. The
  runtime's 105-test suite passes against this facade.
- **4 to 5 times faster.** The benchmark map (320 × 320 cells in 6 layers, 2 720 objects) imports
  in 160 to 200 ms against 670 to 770 ms, most of what is left being the engine's own
  `tile_map_data` assignment and node creation; a small map (60 × 33 cells, 30 objects) in about
  1 ms (the GDScript runtime 3 to 5).
- **`import_async`: no hitch while loading.** The map and its files are parsed, and its tile
  layers decoded, on a worker thread; textures load in the engine's threads; TileSets are created
  a few tiles per frame; the tree is built in one frame. No frame before that one costs more than
  2 to 3 ms of main-thread time on the benchmark map.
- **Shared TileSets.** A map on tilesets an earlier import used gets that TileSet (exclusive: never
  shared between two live trees), and textures are cached per process.
- **`TiledStreamer`: streaming.** One persistent `TileMapLayer` per Tiled layer, chunks loaded and
  erased around a window (or a followed node) within a per-frame budget the streamer calibrates on
  its own work; objects created and freed with their chunks; worlds of maps loaded and unloaded as
  the window approaches and leaves them.
- **Safe threading.** The worker thread runs only the pure-Rust core (no Godot type can reach it);
  files are read on the main thread through `FileAccess`, so `res://` inside an exported `.pck`
  works. godot-rust's `experimental-threads` is not used.

## Requirements

- **Godot 4.7** or later (`compatibility_minimum = 4.7`; built against the 4.7 API with godot-rust
  0.5).
- **A library for the platform** in `bin/`:

  | Platform | Library | How to get it |
  |---|---|---|
  | Linux x86_64 | `libyati_native.linux.x86_64-unknown-linux-gnu.so` | release zip, or build |
  | Windows x86_64 | `libyati_native.windows.x86_64-pc-windows-msvc.dll` | release zip, or build (MSVC toolchain) |
  | macOS (Apple Silicon and Intel) | `libyati_native.macos.framework/` (a universal binary) | release zip, or build on a Mac |
  | Linux arm64 | `libyati_native.linux.aarch64-unknown-linux-gnu.so` | build with cargo (not in the release zip) |
  | Web, Android, iOS | not built ([below](#web-and-other-platforms)) | |

  Only the running platform's library has to exist. Without it the facade prints why and every
  import returns `null`; `Importer.native_version()` is `""`.

## Installing

1. Copy this directory to `res://addons/yati_native/` in your project. (Another path works if
   you change the paths in `yati_native.gdextension`.)
2. Put the libraries into `bin/`:
   - **A release:** download `yati-native.zip` (and its `.sha256`) from the
     [releases](https://github.com/Shadoku/YATI_tweaks/releases): a versioned release
     `v<version>`, or a branch's rolling build `native-<branch>` (`native-main`). The zip holds
     `addons/yati_native/bin/`; unpack it at the project's root. From a checkout of the repository,
     `bash tools/install_native.sh [tag]` does this (and caches the zip).
   - **From source:** with a stable Rust toolchain (rustup), in the repository,
     `bash tools/build_native.sh` builds `native/` (`cargo build --release`) and copies this
     machine's library into `bin/` under its platform name. The first build takes a few minutes
     (godot-rust's bindings).
3. Open the project in the editor once, or, headless, write the line
   `res://addons/yati_native/yati_native.gdextension` into `.godot/extension_list.cfg` before the
   first `godot --headless --import`. Godot loads an extension only when that file names it, and
   Godot 4.7.2 aborts at shutdown (exit code 134) when an import of a fresh `.godot/` discovers one
   midway; with the line written first, the import is clean.

On macOS, a framework downloaded through a browser carries the quarantine attribute, and Godot
cannot load it until it is removed: `xattr -dr com.apple.quarantine addons/yati_native/bin`.

Check: `print(preload("res://addons/yati_native/Importer.gd").native_version())` prints the
library's version.

## Usage

```gdscript
const Importer = preload("res://addons/yati_native/Importer.gd")

var importer := Importer.new()
importer.always_wrap_root = true          # options are plain variables
var map: Node = importer.import("res://maps/desert.tmx")
if map != null:
	add_child(map)                         # the root has no parent
print(importer.last_error_count, " errors, ", importer.last_warning_count, " warnings")
```

| Method | What |
|---|---|
| `import(map_file, project_file = "")` | a map from the file system (`res://`, `user://`, an absolute path); the project file gives custom types |
| `import_from_zip(zip_file, map_in_zip, project_in_zip = "")` | a map and everything it references from a zip archive |
| `import_with_post_processing(map_file, script, project_file = "")`, `import_from_zip_with_post_processing(...)` | then the script's `_post_process(root)` (a project resource) |
| `import_world(world_file, project_file = "")` | every map of a `.world` file under one `Node2D`, each map's root at its place |
| `import_async(map_file, project_file = "")`, `import_from_zip_async(...)` | coroutines (`await` them): the same result without a hitch (below) |
| `make_streamer(map_or_world, options = null, project_file = "")` | a `TiledStreamer` (below) |
| `Importer.shared_tilesets()` | the TileSet cache imports and streamers share |
| `Importer.native_version()` | the library's version, `""` without it |
| `make_options()` | the options as a `TiledImportOptions`, for the classes |

Each import prints a summary line (`Import of '...' succeeded.` or `... finished with N error(s)
and M warning(s).`) after its messages; `last_error_count` and `last_warning_count` hold the counts
(a `null` result counts at least one error), `last_messages` the messages themselves in order
(`{"kind": "error" or "warning", "text": ...}`: the map's errors and warnings, then
`TiledBuilder.get_messages()`). Every root has metadata `map_rect` (the map's bounds
in pixels), `map_size_tiles`, `tile_size` and `orientation`. `WorldLoader.gd`:
`WorldLoader.parse(world_file, zip_file = "")` returns a world's maps as
`{"path": String, "rect": Rect2i}`.

### Options

| Variable | Default | Meaning |
|---|---|---|
| `use_default_filter` | `false` | keep Godot's default texture filter (else 'nearest' on the layers) |
| `add_class_as_metadata` | `true` | the Tiled class as metadata `class` on nodes, tiles and the TileSet |
| `add_id_as_metadata` | `false` | the Tiled object id as metadata `id` |
| `no_alternative_tiles` | `true` | flipped tiles use the engine's transform flags, not generated alternative tiles |
| `map_wangset_to_terrain` | `false` | the old wangset mapping (wangset to terrain, not terrain set) |
| `custom_data_prefix` | `"data_"` | tile properties with this prefix become TileSet custom data layers |
| `save_tileset_to` | `""` | save the map's TileSet to this path (the import then builds its own) |
| `parallax_mode` | `"parallax2d"` | parallax layers wrapped in a `Parallax2D` where they are; `"legacy"`: one ParallaxBackground |
| `animation_mode` | `"strip"` | each animated tile gets an atlas of its frames; `"atlas"`: animate in the tileset's atlas |
| `always_wrap_root` | `false` | a single-layer map returns a `Node2D` root too (recommended) |
| `unknown_class_fallback` | `"staticbody"` | what an object of an unknown class becomes (`"area"`, `"node2d"`, ...) |
| `warn_unknown_classes` | `true` | warn once per import about each unknown class; off for a game that handles its classes itself |
| `class_scenes` | `{}` | Tiled class (case-insensitive) to a scene path: its objects become instances |
| `read_godot_scene_from_custom_types` | `true` | a project class's `godot_scene` member maps the class to that scene |
| `assign_node_properties` | `true` | custom properties named like a node property set it (typed; a class of members `x` and `y` fits a `Vector2` / `Vector2i`); `_tiled_import(info)` is called |
| `object_refs` | `"nodepath"` | object properties: a NodePath to the target's node; `"geometry"`: its points; `"both"` |
| `share_tilesets` | `true` | native only: use the shared TileSet cache (below); off, every import builds its own |
| `async_tiles_per_frame` | `256` | native only: `import_async`'s TileSet tiles created per frame |

The [Reference](https://github.com/Shadoku/YATI_tweaks/blob/main/Reference.md) explains each, and
what every Tiled element and custom property becomes.

### Checking a map from the command line: `check_map.gd`

```sh
godot --headless --path <project> -s res://addons/yati_native/check_map.gd -- <map> \
    [--project=<.tiled-project>] [--zip=<archive>] [--<option>=<value> ...]
```

imports the map with this `Importer.gd` (each `--<option>=<value>` sets that option, typed from its
declaration; `add_id_as_metadata` and `add_class_as_metadata` forced on), frees the tree and prints
one line per error or warning, `YATI_CHECK error|warning <object id or -> <text>`, the id being the
Tiled object the message names (by its node's name, or a class's object of lowest id), then
`YATI_CHECK result <errors> <warnings> <nodes> <cells> <ms>`. Exit code 0 without errors, 1 with
errors, 2 when nothing could be imported or the arguments are wrong. The Tiled extension's *Check map
in Godot* action runs it; the GDScript runtime has the same script, with the same output.

### Importing during play: `import_async`

```gdscript
const Importer = preload("res://addons/yati_native/Importer.gd")

var importer := Importer.new()

func enter_room(path: String) -> void:
	var room: Node = await importer.import_async(path)   # the game keeps running meanwhile
	if room != null:
		add_child(room)
```

The result, counts and summary line are `import()`'s. Frame by frame:

1. `TiledMap.load_async`: the map, its tilesets, templates and project file are parsed on a worker
   thread, which then decodes the tile layers into the blobs the build assigns
   (`TiledMap.prepare_tile_layers`). Each frame the main thread polls it once: it reads the files
   the parse asks for (two or three rounds) with `FileAccess` and hands them over, about a
   millisecond per megabyte; the frames in between cost nothing.
2. The map's imported (`res://`) textures load in the engine's threads
   (`ResourceLoader.load_threaded_request`) from the moment the parse names them.
3. The map's TileSets are created `async_tiles_per_frame` tiles per frame, unless cached.
4. The tree is built in one frame: resolving the tile layers' distinct cell values (creating their
   tiles in the TileSet), the engine's `tile_map_data` assignments, node creation. That frame is
   the hitch that is left: about 0.5 ms for a small map, 150 to 200 ms for the benchmark map's
   522 358 cells and 2 720 objects. A map that large is what the streamer is for.

The coroutine keeps its importer alive (`await Importer.new().import_async(path)` works). The same
steps with the classes, for a loading screen with progress:

```gdscript
var map := TiledMap.new()
map.prepare_tile_layers = true                 # the worker decodes the tile layers too
map.load_async("res://maps/level.tmx", "")
while map.poll() < TiledMap.READY:             # READING, PARSING, WAITING_FILES, then READY or FAILED
	await get_tree().process_frame
if map.get_status() == TiledMap.READY:
	var tilesets: TiledTileSets = TiledTileSets.shared()
	while not tilesets.build_step(map, null, 256):
		await get_tree().process_frame
	add_child(TiledBuilder.new().build(map, tilesets, null))
```

`map.cancel()` stops a load (its thread ends on its own; texture requests already made are
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

`make_streamer` takes a map path (loaded with `TiledMap.load_async`; the streamer starts once it is
loaded), a world path (`.world`), or a `TiledMap` / `TiledWorld`, and gives a streamer with the
importer's options and the shared TileSet cache. With the class:

```gdscript
var map := TiledMap.new()
map.load("res://maps/overworld.tmx", "")       # or load_async: the streamer polls it
var streamer := TiledStreamer.new()
streamer.map = map
streamer.window = Rect2i(0, 0, 120, 70)        # cells; or follow + margin
add_child(streamer)
```

**Each frame** (`_process`, or `step(budget_us)` by hand after `set_process(false)` once the
streamer is in the tree) the chunks (`chunk_size`, 16 × 16 cells) of every tile layer that the
window covers and that are not loaded are loaded, nearest first, with `set_cell` into one
persistent `TileMapLayer` per Tiled tile layer (configured as `import()` configures it); chunks
outside the window grown by `keep_margin` (16 cells, so a camera moving back and forth across a
border does not reload a chunk) are erased. An infinite map's chunks are decoded from the file's
payloads when wanted, so a loaded map costs about its file size in memory (1.6 times it for a
58 MB map); a finite map's layers are decoded once, on a thread. Image layers and the background
colour are made with the tree.

**The budget.** The streamer measures its own work (µs per chunk, per cell, per cell whose tile has
a collision polygon, per y-sorted cell, per object, per erased cell, per freed object:
`get_costs()`), estimates each piece of work before it runs, and stops the frame's work before the
piece that would exceed `budget_us` (one piece always runs). The engine's work for a chunk (its
rendering quadrant and physics bodies) is done in the streamer's frame
(`TileMapLayer.update_internals()`), so the budget counts what the frame pays. A window that
outruns the budget shows as chunks appearing late, never as a long frame: 4 ms per frame loads
about 200 chunks a second of four dense layers on a software-rendered test machine, what two
screens per second need. `get_stats()` counts the work.

**Objects** are indexed by the chunk their position (`x`, `y`) lies in, created when that chunk
enters the window by `import()`'s code (classes, templates, scenes, properties, `_tiled_import`)
under one node per object layer, and freed when it leaves (`object_spawned(node, id)`,
`object_despawned(id)`). An object property naming an object of another chunk is a NodePath while
both exist, an empty NodePath otherwise (no warning). `keep_objects_resident = true` keeps objects
once created.

**World mode.** `streamer.world = world` (a loaded `TiledWorld`) or `make_streamer("....world")`:
each map is placed at its rect, loaded when the window comes within `world_margin` cells of it,
streamed, and unloaded once the window is `keep_margin` farther away (`map_loaded(path, root)`,
`map_unloaded(path)`). `window` and `margin` are then in cells of `world_cell_size` (16 × 16 px).
Maps with the same tilesets share one TileSet.

**Everything at once.** `load_all()` loads the whole map (or every map of a world) in one call:
the content of `import()`.

Queries: `get_layer(name)`, `get_object_layer(name)`, `get_map_root(path)`, `get_loaded_maps()`,
`loaded_chunk_count()`, `get_loaded_chunks(layer)`, `get_spawned_object_count()`, `is_settled()`,
`get_error_count()`, `get_warning_count()`. Signals: `chunk_loaded(layer_name, coords)`,
`chunk_unloaded(layer_name, coords)`, `object_spawned`, `object_despawned`, `map_loaded`,
`map_unloaded`, `settled()`; all emitted after the frame's work (a `_tiled_import` hook or an
object's `godot_script` runs during it and must not call the streamer).

### Shared TileSets and textures

With `share_tilesets` on (the default) every `Importer`, and every streamer by default, uses one
cache, `TiledTileSets.shared()` (`Importer.shared_tilesets()`): a map whose tileset combination an
earlier import built gets that TileSet. The combination is the ordered list of tilesets with their
first gids (paths of external tilesets, a hash of embedded ones), the map's tile size, orientation
and stagger, the zip archive, the project file's types, and the options that shape a TileSet
(`custom_data_prefix`, `map_wangset_to_terrain`, `animation_mode`, `no_alternative_tiles`).

- **By reference.** A cached TileSet is the very object a previous map used: the builder adds the
  tiles each map needs to it, and a change a game makes to it shows in every later map that gets it.
- **Exclusive.** A TileSet goes only to a map no live tree shares it with: while a tree holds it, a
  map on the same tilesets gets a second one. A map loaded after the previous tree was freed reuses
  it. `Importer.shared_tilesets().exclusive = false` shares one between live trees as well.
- **Messages.** A cached TileSet's warnings and errors are reported by every import that uses it,
  so an import's counts do not depend on the cache.
- **Files that change.** The cache keys on paths, not contents: after a tileset or image changed on
  disk, call `Importer.shared_tilesets().clear()` (TileSets) and `.clear_textures()` (the
  per-process texture cache). Cached TileSets and textures stay in memory until then.
- **`save_tileset_to`** makes an import build its own TileSet; `share_tilesets = false` gives every
  import its own.

### The native classes

The facade is a thin layer over these classes, which a project can use directly (through `ClassDB`
if the script must compile without the library):

| Class | What |
|---|---|
| `TiledImportOptions` (Resource) | every option, same names and defaults as the facade's variables |
| `TiledMap` | `load(path, project_file) -> Error`, `from_zip(zip, path_in_zip, project_in_zip) -> Error`; `load_async(...)`, `from_zip_async(...)`, then `poll() -> int` once per frame (`READING`, `PARSING`, `WAITING_FILES`, `READY`, `FAILED`), `get_status()`, `cancel()`, signals `loaded` and `failed`; `prepare_tile_layers` (decode the tile layers during the load, on the worker for `load_async`; the map then holds them, about 12 bytes per non-empty cell; leave it off for a map a streamer reads), `has_prepared_tile_layers()`; `is_loaded()`, `get_path()`, `get_size()` (cells), `get_tile_size()`, `get_orientation()`, `is_infinite()`, `get_layers()` (`{name, type, id, depth}` per layer, depth first), `to_dictionary()` (the GDScript runtime's dictionary of the map), `get_errors()`, `get_warnings()` |
| `TiledTileSets` | the TileSet builder and its cache: `TiledTileSets.shared()` (the process-wide one, exclusive; `get_shared()` on an instance where static calls are not available yet, a `SceneTree` script's `_init`), `tileset_for(map, options) -> TileSet`, `build_step(map, options, tiles_per_step) -> bool` (true when ready), `exclusive`, `clear()`, `clear_textures()`, `get_cached_count()`, `get_created_count()`, `get_texture_count()`, `get_last_error_count()`, `get_last_warning_count()` |
| `TiledBuilder` | `build(map, tilesets, options) -> Node`: the whole tree in one call, the root parentless; `get_error_count()`, `get_warning_count()`, `get_messages()`, `get_timings()` (`prepare_us`, `cells_us`, `assign_us`: the tile layers' main-thread time of that build) |
| `TiledWorld` | `load(world_file, zip_file) -> Error`, `get_maps()` (`{path, rect}` per map), `get_errors()` |
| `TiledStreamer` (Node2D) | `map` or `world`, `tilesets` (null: `TiledTileSets.shared()`), `options`, `project_file`; `window` (Rect2i, cells) or `follow` (Node2D) + `margin` (cells); `keep_margin`, `world_margin`, `world_cell_size`, `budget_us` (2000; `<= 0` unbounded), `keep_objects_resident`, `chunk_size` (16 × 16), `allow_y_sorted_layers`; `step(budget_us) -> bool`, `load_all()`, the queries and signals above, `get_costs()`, `get_stats()`, `reset_stats()` |
| `TiledNative` | `version()`, and `sum()` for the smoke test |

```gdscript
var map := TiledMap.new()
if map.load("res://maps/desert.tmx", "") == OK:
	var root: Node = TiledBuilder.new().build(map, TiledTileSets.new(), null)
```

## Limits

- **Y-sorted tile layers are not streamed by default.** The tile layers of isometric and staggered
  maps are y-sorted: the engine draws their cells one by one and re-sorts all of a layer's cells in
  every frame that changes it, after the streamer's work, where no budget reaches (+4 ms over the
  idle frame for one chunk into a layer of 3 500 cells, +8 ms into 87 000, on a software-rendered
  test machine). A streamer refuses such a map with an error; `allow_y_sorted_layers = true` streams
  it anyway, budgeted at its measured cost (about 14 µs per cell, 8 times an ordinary cell's).
  `load_all()` and `import()` always take them; object layers sorted by y stream normally.
- **A `TileMapLayer` remembers every area it has shown.** Godot 4.7.2 keeps one object, about
  1.3 KB, for each 16 × 16 area of a layer that ever held a cell, even after its cells are erased,
  until the layer is freed, whatever its quadrant sizes. A layer streamed across all of a
  4 000 × 4 000 map would hold about 80 MB. Most games cross a small part of such a map; world mode
  frees a map's layers with the map; a streamer freed and made again starts from nothing.
- **The `tile_map_data` format is the engine's.** Tile layers are filled with one
  `TileMapLayer.tile_map_data` assignment each, in a byte format the engine does not document (a
  version 0 header, then 12 bytes per cell). It is checked once per process against the engine;
  should a Godot version change it, an error says so and the layers are filled with `set_cell`
  instead (slower, the same result).
- **Collision is the dear part of streaming**: about 5 µs per cell whose tile has a collision
  polygon on top of 2 µs per cell (the engine's physics shapes). The budget counts it; a game that
  streams collision-heavy maps may want a larger `physics_quadrant_size` or its collision elsewhere.
- **Keep `no_alternative_tiles` on when streaming** (the default): every tile of a streamed map's
  TileSet is created before the first cell is set, because the engine rebuilds every cell of every
  layer using a TileSet when the TileSet changes (40 to 60 ms with 30 000 cells), and with the
  option off a flipped tile's first appearance creates an alternative tile, which is such a change.
- **The build frame of `import_async` is the engine's work**, about 150 to 200 ms for half a
  million cells and 2 700 objects: stream maps that large.

## How it differs from the GDScript runtime

The tree, names, metadata, groups, materials, options and messages are the runtime's (its test
suite is the specification). The accepted differences, each with its reason, are listed in the
[Reference](https://github.com/Shadoku/YATI_tweaks/blob/main/Reference.md#differences-between-the-gdscript-and-the-native-importer).
In short: TileSets and textures are cached across imports (`share_tilesets`; a file changed on
disk needs `clear()` / `clear_textures()`); no `__internal__` custom data layer is left in saved or
template TileSets; custom types are merged into tilesets only templates use; references resolve
relative to the referencing file only (the runtime first tries the bare name relative to the
working directory) and an embedded tileset after an external one relative to the map; JSON
templates (`.tj`) instantiate; a numeric name (`name="007"`) keeps its spelling; engine classes'
property lists are read once per process; `TilemapCreator.gd` is a stand-in (`create`,
`get_map_rect`, `get_tileset`); a few malformed inputs no Tiled version writes are read differently.

## Building

`native/` in the repository is a Cargo workspace: `yati_native` (this library, godot-rust 0.5 with
`api-4-7`, a `cdylib`) and `tiled_core` (`native/tiled/`: the Tiled model, parsers, payload
decoding and the blobs, pure Rust, no Godot). `bash tools/build_native.sh` runs
`cargo build --release` and copies the library into `bin/` under the name
`yati_native.gdextension` expects (`lib<name>.<os>.<rust target triple>.<ext>`; `--check` only
tells whether it is there). The repository's CI (`.github/workflows/native.yml`) builds Linux
x86_64, Windows x86_64 and a universal macOS framework on every push that touches the crate,
checks each library as an image and loads it in Godot, and publishes them.

`cargo build --release --features unchecked` turns godot-rust's per-call checks (is the object
still alive) off: about 30 % off the object phase of a large map, at the price of undefined
behaviour instead of a panic should a node be freed while the builder holds it (a script's
`_init` freeing nodes of the tree being built). Release builds do not use it.

### Web and other platforms

Not built yet; `yati_native.gdextension`'s web entries name the files a web build would produce
(`bin/yati_native.wasm` for Godot's threads export, `bin/wasm-nothreads/yati_native.wasm` without
threads). What a web build needs:

- **godot-rust's wasm support:** the `godot` dependency with the features `experimental-wasm` and
  `lazy-function-tables` (a web build needs its function tables loaded lazily), plus
  `experimental-wasm-nothreads` for Godot's single-threaded web export.
- **Emscripten matching Godot's:** Godot 4.7.2's web export templates are built with Emscripten
  4.0.11 (`EM_VERSION` in Godot's `.github/workflows/web_builds.yml`); `emcc` of that version on the
  PATH, and the C parts (the `zstd` feature binds the C zstd library) compiled with it.
- **The target `wasm32-unknown-emscripten`** (rustup target, on a nightly toolchain with
  `-Zbuild-std`, as godot-rust's web guide describes), linked as a side module
  (`-C link-args=-sSIDE_MODULE=2`).
- **`-pthread` matching Godot's threads setting:** with threads (`-C link-args=-pthread`,
  `-C target-feature=+atomics,+bulk-memory,+mutable-globals`) for the threads export, without them
  for the nothreads one; the project's web export needs "Extensions Support" on.
- **No threads in the nothreads build:** the loader's worker (`TiledMap.load_async`), the per-layer
  decoding threads and the streamer's decoding thread would have to run their work in budgeted
  slices on the main thread instead (`poll()` stepping the loader one round, a layer or a chunk per
  call), since there is no thread to run them on.

Android and iOS are cross-compiles (`aarch64-linux-android`, `aarch64-apple-ios`) with entries to add
to the `.gdextension`; nothing in the code is platform-specific.

## Versions

The library's version is the crate's (`native/Cargo.toml`, `Importer.native_version()`): 0.2.0
ships with YATI_tweaks 3.1.0. The repository's releases `v3.x.y` version the GDScript runtime and
the repository; the library is released under its own tag `v<crate version>` (`v0.2.0`), which
makes the workflow publish `yati-native.zip`, and every branch has a rolling build
`native-<branch>`. `tools/install_native.sh` downloads the tag in `.yati-native-version` (or
`branch`: the current branch's rolling build).

## License

MIT, as the repository (`LICENSE`). `PostProcessing.gd` is the GDScript runtime's, its header kept.
