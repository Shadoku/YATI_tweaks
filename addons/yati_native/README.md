# yati_native: the YATI runtime importer as a GDExtension

A native (Rust, [godot-rust](https://github.com/godot-rust/gdext)) version of this repository's
GDScript runtime importer (`runtime/`): it imports Tiled maps (`.tmx`, `.tmj`, from the file
system or a zip archive) into the same node tree, with the same options, metadata and messages,
several times faster. `Importer.gd` here has the GDScript runtime's option variables, its four
`import*` methods and `last_error_count` / `last_warning_count`, so a project switches by
changing one preload path.

The plan, the measurements behind it and the stages:
[docs/native-importer-plan.md](../../docs/native-importer-plan.md).

## Status: stage 3 (asynchronous loading and shared caches)

Maps import natively and the GDScript runtime's test suite (`test/run_tests.gd`) passes against
this facade. `import_async()` imports a map during play without a hitch (the parse on a worker
thread, the build in one frame), and TileSets and textures are cached across imports. The
streamer for infinite maps (stage 4) is not there yet.

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

The addon stands alone: `Importer.gd`, `PostProcessing.gd` (the runtime's, unchanged) and a
two-line `CommonUtils.gd` are all the GDScript it has.

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

### Shared TileSets and textures

With `share_tilesets` on (the default), every `Importer` uses one TileSet cache,
`Importer.shared_tilesets()` (a `TiledTileSets`): a map whose tileset combination an earlier import
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
`import_with_post_processing(map, script, project = "")` and
`import_from_zip_with_post_processing(...)` behave as the GDScript runtime's
([Runtime.md](../../Runtime.md)). `Importer.native_version()` returns the loaded library's
version, or `""` when the library is missing for this platform (an import then fails with a
message and returns `null`). `importer.make_options()` gives the options as a
`TiledImportOptions` for the native classes.

### The native classes

The facade is a thin layer over four classes, which a project can use directly (reach them
through `ClassDB` if the script must compile without the library):

| Class | What |
|---|---|
| `TiledImportOptions` (Resource) | every option, same names and defaults as the facade's variables |
| `TiledMap` | `load(path, project_file) -> Error`, `from_zip(zip, path_in_zip, project_in_zip) -> Error`; `load_async(...)`, `from_zip_async(...)`, then `poll() -> int` once per frame (`READING`, `PARSING`, `WAITING_FILES`, `READY`, `FAILED`), `get_status()`, `cancel()`, signals `loaded` and `failed`; `is_loaded()`, `get_path()`, `get_size()` (cells), `get_tile_size()`, `get_orientation()`, `is_infinite()`, `get_layers()` (`{name, type, id, depth}` per layer, depth first), `to_dictionary()` (the GDScript runtime's `DictionaryBuilder` dictionary of the map, custom types merged), `get_errors()`, `get_warnings()` |
| `TiledTileSets` | the TileSet builder and its cache: `tileset_for(map, options) -> TileSet` (built on first use, then cached per tileset combination and shared by reference with every later map), `build_step(map, options, tiles_per_step) -> bool` (create a map's TileSets a few tiles per call; true when ready), `exclusive` (hand a TileSet only to a map no live tree shares it with), `clear()`, `clear_textures()` (the per-process texture cache), `get_cached_count()`, `get_created_count()`, `get_texture_count()`, `get_last_error_count()`, `get_last_warning_count()` |
| `TiledBuilder` | `build(map, tilesets, options) -> Node`: the whole tree in one call, the root parentless; `get_error_count()`, `get_warning_count()`, `get_messages()` of that build |
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
- **A tile object of class `instance`** keeps the scale of its tile sprite; the runtime reads the
  scale of the sprite it has just freed (a script error).
- **Parsing** follows `tiled_core` ([native/tiled/README.md](../../native/tiled/README.md)):
  JSON templates (`.tj`) instantiate, the format is told by content, and a few malformed inputs
  that no Tiled version writes are read differently.

## Building with fewer checks

`cargo build --release --features unchecked` (in `native/`) turns godot-rust's per-call checks
(is the object still alive) off in the release build: about 30 % off the object phase of a large
map, at the price of undefined behaviour instead of a panic should a node be freed while the
builder holds it (a script's `_init` freeing nodes of the tree being built, say). The CI builds
do not use it.
