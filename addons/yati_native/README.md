# yati_native: the YATI runtime importer as a GDExtension

A native (Rust, [godot-rust](https://github.com/godot-rust/gdext)) version of this repository's
GDScript runtime importer (`runtime/`): it imports Tiled maps (`.tmx`, `.tmj`, from the file
system or a zip archive) into the same node tree, with the same options, metadata and messages,
several times faster. `Importer.gd` here has the GDScript runtime's option variables, its four
`import*` methods and `last_error_count` / `last_warning_count`, so a project switches by
changing one preload path.

The plan, the measurements behind it and the stages:
[docs/native-importer-plan.md](../../docs/native-importer-plan.md).

## Status: stage 2 (the one-shot builder and the facade)

Maps import natively and the GDScript runtime's test suite (`test/run_tests.gd`) passes against
this facade. Asynchronous loading (stage 3) and the streamer for infinite maps (stage 4) are not
there yet: an import is one synchronous call on the main thread.

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
| `TiledMap` | `load(path, project_file) -> Error`, `from_zip(zip, path_in_zip, project_in_zip) -> Error`; `is_loaded()`, `get_path()`, `get_size()` (cells), `get_tile_size()`, `get_orientation()`, `is_infinite()`, `get_layers()` (`{name, type, id, depth}` per layer, depth first), `to_dictionary()` (the GDScript runtime's `DictionaryBuilder` dictionary of the map, custom types merged), `get_errors()`, `get_warnings()` |
| `TiledTileSets` | the TileSet builder and its cache: `tileset_for(map, options) -> TileSet` (built on first use, then cached per tileset combination), `clear()`, `get_cached_count()`, `get_last_error_count()`, `get_last_warning_count()`; textures cached per path |
| `TiledBuilder` | `build(map, tilesets, options) -> Node`: the whole tree in one call, the root parentless; `get_error_count()`, `get_warning_count()`, `get_messages()` of that build |
| `TiledNative` | `version()`, and `sum()` for the smoke test |

```gdscript
var map = ClassDB.instantiate("TiledMap")
if map.load("res://maps/desert.tmx", "") == OK:
	var tilesets = ClassDB.instantiate("TiledTileSets")
	var root: Node = ClassDB.instantiate("TiledBuilder").build(map, tilesets, null)
```

Files are read on the main thread through `FileAccess`, so `res://` paths inside an exported
.pck work; images through `ResourceLoader` (imported textures) or `Image.load_from_file`.
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
