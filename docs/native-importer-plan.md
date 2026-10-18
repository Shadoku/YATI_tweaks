# Native importer plan: the YATI runtime in a Rust GDExtension

An exploration (2026-10-18) of a native version of this repository's runtime importer, with two
goals the GDScript runtime cannot meet:

1. **No preloading.** In the games that use the addon, every map has to be imported before
   play starts and kept as a node tree (or a PackedScene), because importing one during play
   hitches. A map should be loadable when it is needed, without a visible hitch.
2. **Streaming.** Chunks of an infinite Tiled map (`infinite="1"`, 16 × 16 cell chunks) should
   stream in and out as the view moves, fast enough that a scrolling game never drops a frame
   on the map.

The native variant stays **generic**: a drop-in for this runtime (same options, same node tree,
same metadata contract, the same test suite as its gate), usable from any Godot 4 project.
Nothing of any particular game goes into it.

This document holds the findings (measured on this machine, numbers below), the decisions they
force, the architecture, and the stages with their gates. It follows the shape of the Goo2D
kernel plan that moved a GDScript hot path into Rust (godot-rust) before.

## 1. Findings

### 1.1 Where the GDScript import spends its time

`test/benchmark.gd` (320 × 320 map, 6 tile layers over 3 tilesets, base64 + zlib, 85 % filled,
half the cells flipped; 2 400 objects with 3 properties each, 320 template instances; Godot 4.7.2,
headless, 4 cores, a shared container):

| phase | ms (avg of 3) | share |
|---|---|---|
| parse (XML to the Tiled dictionary) | 148 | 21 % |
| tilesets (TileSet creation, 192 tiles) | 3 | 0.5 % |
| tile layers (522 358 cells through `set_cell`) | 401 | 56 % |
| objects (2 720 nodes with metadata) | 164 | 23 % |
| **total** | **716** (min 672) | |

The small map the game that drove this repository's 3.0 work uses (60 × 33 cells, 2 layers,
30 objects): parse 2.5 ms, full import 8 to 10 ms, 19 ms the first time (script and resource
caches cold). That is the hitch a game sees when it imports a small map during play; a large one
costs hundreds of ms.

### 1.2 What Rust does with the same work

A pure-Rust prototype (quick-xml, base64, flate2; no Godot; `std::thread` per layer) parsing the
same benchmark map and emitting, per layer, the byte blob `TileMapLayer.tile_map_data` takes
(the format is in 1.3):

| step | ms |
|---|---|
| parse the 2 MB TMX (structure, layers' payloads as strings, 2 720 objects counted) | 2 to 5 |
| parse the three .tsx | 0.05 |
| decode (base64 + zlib) and emit 522 358 cells, sequential | 19 to 26 |
| the same with one thread per layer | 7.5 to 11 |

So about 550 ms of GDScript (parse + tile layers) becomes about 10 ms, and none of it needs the
main thread. The small map: 0.05 to 0.17 ms to parse, 0.05 ms to emit.

### 1.3 The engine's floor: what no importer can avoid

`test/bench_materialize.gd` measures the engine's own costs, with a renderer (`xvfb-run`,
OpenGL, software GL here, so idle frames are inflated and absolute numbers are pessimistic) and
headless. The layer is 320 × 320 at 85 % (86 931 cells); "with collision" gives every tile a
square collision polygon. Ranges are the spread over three runs on this shared machine; the
ordering between rows is stable, the absolute numbers are not.

| what the engine charges | without collision | with collision |
|---|---|---|
| `tile_map_data = bytes` for 86 931 cells (one call) | 14 ms headless, 27 to 29 ms with the renderer | 18 to 26 ms |
| the same through `set_cell` per cell from GDScript | 33 to 38 ms | 27 ms |
| `PackedScene.instantiate()` of that layer | 15 to 98 ms across runs | 19 ms |
| **the frame after the layer enters the tree** (rendering quadrants, physics bodies) | **145 to 342 ms** | **684 to 729 ms** |
| `set_cell` of one 16 × 16 chunk (~218 cells) into the live layer, per chunk | ~0.4 ms | ~1.9 ms |
| `erase_cell` of one such chunk | ~0.25 ms | ~0.25 ms |
| a separate 64 × 64 TileMapLayer per chunk (3 449 cells), added one per frame | +12 ms per frame | +32 ms per frame |
| 800 live 16 × 16 TileMapLayers (one per chunk) | +14 ms per idle frame (software GL) | |

`TileMapLayer.tile_map_data` is a `PackedByteArray`: a `u16` format version (0), then per cell
six little-endian `int16`: x, y, source id, atlas x, atlas y, alternative id (a flipped or
rotated cell carries the transform flags `TileSetAtlasSource.TRANSFORM_*` in bits 12 to 14 of
the alternative, which is what this runtime's default `no_alternative_tiles = true` produces).
Verified against the engine: two cells set with `set_cell` read back exactly this.

Three conclusions:

- **Preloading does not cure the hitch for large maps.** Instantiating a packed 87k-cell layer
  costs 15 to 98 ms, and the frame in which any such layer enters the tree costs 150 to 730 ms
  whatever made it. The hitch the games see is partly the GDScript import (1.1) and partly the
  engine's deferred quadrant and body build, which arrives one frame later and is charged per
  cell. Only chunking removes the second part: materialize a large map in pieces, a budget of
  cells per frame.
- **Stream into one persistent TileMapLayer per Tiled layer with `set_cell`, not one node per
  chunk.** The engine rebuilds only the quadrants a `set_cell` touches (its quadrant size is
  16 cells, Tiled's chunk size), so a chunk costs 0.4 ms (1.9 ms with collision) and erasing
  one 0.25 ms. A node per chunk pays the per-layer overhead on every frame it is alive and
  costs 12 to 32 ms to add at 64 × 64. Collision is the expensive half: a game that needs
  tile collision pays about 9 µs per cell for the engine's physics shapes, so a streamer's
  budget has to count collision cells separately (`physics_quadrant_size` trades precision
  for fewer bodies; the shapes stay).
- **The parse and the cell data preparation can leave the main thread entirely.** They touch
  no Godot object. What stays on the main thread is the engine's part: TileSet creation (one
  per tileset combination, cacheable across maps and chunks: 3 ms for 192 tiles here, more with
  collision polygons per tile), the `tile_map_data` or `set_cell` calls, and node creation for
  objects.

### 1.4 What Rust does not speed up

Node creation, `set_meta`, TileSet and TileData calls, texture loads: these are engine calls and
cost the same from Rust as from GDScript, less the interpreter's overhead around them (the
benchmark's 2 720 objects at ~60 µs each are mostly that, so expect perhaps 2 to 3× there, not
50×). The native importer wins by moving the parse off the main thread, by replacing 522k
`set_cell` calls with six `tile_map_data` assignments (or, when streaming, by touching only the
chunks that changed), by caching TileSets and textures across maps, and by spreading the rest over
frames with a budget. The GDScript runtime stays as the reference implementation and as the
fallback on a platform without a binary.

## 2. Decisions

- **One crate, one addon directory, generic.** `native/` holds the Rust crate (`yati_native`,
  godot-rust 0.5.5 with `api-4-7`, `cdylib`), `addons/yati_native/` what a project copies: the
  `.gdextension`, `bin/` with the built libraries, a GDScript facade `Importer.gd` with this
  runtime's exact API (so `test/run_tests.gd -- --runtime=res://addons/yati_native` is the
  parity gate), and the streaming node. No game-specific class, property or path anywhere in it;
  Tiled classes and properties come out as the same metadata this runtime produces.
- **The Tiled JSON schema is the data model.** TMX, TSX and TX are parsed into the same Rust
  structs the TMJ/TSJ/TJ formats deserialize into (serde), as `DictionaryFromXml.gd` already
  makes TMX look like TMJ. One model, two parsers; `TiledMap.to_dictionary()` exposes it to
  GDScript for post-processing and for the parity test against `DictionaryBuilder`.
  `rs-tiled` (the `tiled` crate) was considered and set aside: TMX only, its own model, and the
  JSON format and custom-type semantics this runtime needs would have to be bolted on.
- **Pure Rust off the main thread; Godot only on it.** godot-rust's threading is experimental
  (`experimental-threads`: "high risk of unsoundness"), and Godot's scene objects are not
  thread-safe. The worker thread therefore holds only Rust data: bytes in, a parsed map and
  per-layer cell blobs out. Everything that touches the engine runs on the main thread, driven
  by a `poll()` or a per-frame budget.
- **Files are read on the main thread, through Godot.** An exported project's `res://` files
  live in the .pck, invisible to `std::fs`. So the worker never opens files: the main thread
  reads bytes with `FileAccess` (cheap, a few MB in about a millisecond) and hands them over.
  References a parse discovers (external tilesets, templates, the project file, a zip's
  members) come back as a list of paths the main thread resolves on the next `poll()`; a map
  needs two or three such round trips (map, then tilesets and templates, then the templates'
  tilesets), each a frame apart at most. `user://` and absolute paths go through the same
  route (no second code path).
- **Textures through `ResourceLoader`.** `res://` images are imported textures;
  `load_threaded_request` loads them in parallel with the parse. Other paths fall back to
  `Image.load_from_file` on the main thread. Texture handles are cached per path, per importer
  instance and optionally for the process (option).
- **TileSets are cached per tileset combination.** A Godot `TileSet` carries its physics,
  navigation, occlusion and custom data layers; its atlas sources' `TileData` are laid out for
  those layers. So the cache key is the map's ordered list of (tileset path or embedded-tileset
  hash, firstgid) plus the options that shape the result (`custom_data_prefix`,
  `map_wangset_to_terrain`, `no_alternative_tiles`); every map and every chunk with that list
  shares one TileSet. Creation can be spread over frames (N tiles per frame) when a tileset is
  large.
- **Every map is a chunk source; the streamer is the one materializer.** A finite layer is one
  chunk of its own size; an infinite layer's chunks are Tiled's. A full load is the streamer
  with an unbounded window and an unbounded budget, in one frame; a progressive load is the
  same with a budget; streaming is a window following the camera. Each Tiled tile layer is one
  persistent `TileMapLayer`; chunks enter it with `set_cell` and leave with `erase_cell`
  (1.3). Objects are not chunked in Tiled: the parse indexes them into a cell grid, and the
  streamer instantiates and frees them with the chunks they lie in (an option keeps all objects
  resident, for maps whose objects carry game state).
- **Compressed payloads are decoded when needed.** An infinite map's chunks are each a
  separate base64 (+ zlib/gzip/zstd) payload in the file; the parse keeps the payload and
  decodes it the first time its chunk is wanted, so a huge map costs its file size in memory,
  not 4 bytes per cell of its bounding box.
- **Parity, not identity, with the GDScript runtime.** The node tree, names, metadata, options
  and error reporting match what `test/run_tests.gd` asserts; floating-point layouts may differ
  in the last bits; the printed messages may differ in wording. Features the GDScript runtime
  warns about as unsupported stay unsupported (`Reference.md`).
- **The platforms are the desktop three plus web.** Linux x86_64, Windows x86_64, macOS
  universal built by CI and committed back as the Goo2D kernel's workflow does (`kernel.yml`:
  a matrix, checksums, an image check, a load smoke test, a bot commit with the skip-ci
  marker); web (`experimental-wasm`) as a later stage, single-threaded there (the worker's
  work runs in budgeted slices on the main thread instead). Mobile targets are cross-compiles
  to add when someone needs them.

## 3. Architecture

```
addons/yati_native/
  yati_native.gdextension      entry symbol, compatibility_minimum 4.7, bin/ paths
  bin/                         the libraries (CI-built, committed), native-commit.txt
  Importer.gd                  the facade: this runtime's API over the native classes
  TiledStreamer.gd?            none: TiledStreamer is a native Node (below)
  README.md                    install, API, options, what differs from the GDScript runtime
native/                        the crate (Cargo.toml, src/), .gdignore'd
  src/model/                   the Tiled data model (serde structs for TMJ/TSJ/TJ and the
                               .tiled-project), gids, flips, property typing, custom types
  src/parse/{xml,json}.rs      TMX/TSX/TX via quick-xml into the model; JSON via serde_json
  src/parse/data.rs            layer and chunk payloads: csv, base64, zlib/gzip/zstd, the
                               legacy <tile gid> form; emits cell blobs (1.3's format)
  src/load.rs                  the async loader: a worker thread, the file-request protocol,
                               the state machine poll() drives
  src/build/tileset.rs         TileSet builder (main thread): atlas sources, image
                               collections, tile offsets and origins, collision, navigation,
                               occlusion, animation, terrains/wangsets, custom data, metadata
  src/build/layers.rs          TileMapLayer per tile layer, parallax, blend modes, tint,
                               offsets, image layers, group layers, background colour
  src/build/objects.rs         objects to nodes: bodies and shapes, sprites, text, points,
                               polygons, templates, instances, properties to metadata
  src/stream.rs                TiledStreamer: window, budget, chunk grid, object grid
  src/lib.rs                   the classes registered with Godot
```

Classes (RefCounted unless said):

- `TiledMap`: a parsed map. `load(path, project_file = "")` synchronous; `load_async(...)`
  then `poll() -> Status` (`READING`, `PARSING`, `WAITING_FILES`, `READY`, `FAILED`) each
  frame, or `await`ing its `loaded` signal (emitted from a main-thread `poll` the facade or
  the streamer runs); `from_zip(zip, path_in_zip, ...)`. Queries: `size`, `tile_size`,
  `infinite`, `layers`, `chunks_of(layer, rect) -> Array[Vector2i]`, `objects_in(rect)`,
  `to_dictionary()` (the model as the GDScript runtime's dictionary), `errors`, `warnings`.
- `TiledTileSets`: the TileSet cache. `tileset_for(map) -> TileSet` (built on first use, or
  `build_step(map, tiles_per_step) -> bool` to spread it), `clear()`, textures cache.
- `TiledBuilder`: the one-shot materializer: `build(map, options) -> Node` makes the whole
  tree in one call (what `Importer.gd` calls), on the main thread, using `tile_map_data` per
  layer. Options as a `TiledImportOptions` Resource mirroring the facade's variables.
- `TiledStreamer` (Node2D): `map`, `window: Rect2i` (cells) or `follow: Node2D` + `margin`,
  `budget_us` per frame (cells and collision cells counted separately, from measured per-cell
  costs it calibrates on its first chunks), `keep_objects_resident`. Owns the persistent
  layer nodes and the live objects; signals `chunk_loaded(layer, coords)`,
  `chunk_unloaded(...)`, `object_spawned(node, id)`, `object_despawned(id)`. Loading a finite
  map fully in one frame is `window = everything, budget = INF`.
- `Importer.gd` (the facade): the four `import*` methods and the option variables of this
  runtime; `last_error_count`, `last_warning_count`; the summary line. Internally
  `TiledMap.load` + `TiledTileSets` + `TiledBuilder`. A fifth method, `import_async(path) ->
  Node` as a coroutine, is the one-line way to the no-hitch path for a finite map that fits
  one frame's engine work; larger ones go through `TiledStreamer`.

The GDScript runtime in `runtime/` stays as it is: the reference, the fallback, and the
source of truth for behaviour questions.

## 4. Stages and gates

Each stage ends with its gate green and this document's status section updated with the
numbers measured. Stages 1 and 2 are the bulk; 3 and 4 are what the two goals need.

### Stage 0: scaffold and loop

- The crate (`native/`, `godot = { version = "0.5", features = ["api-4-7"] }`, release profile
  with `lto = "thin"`), one class `TiledNative` with `version()`; the `.gdextension`;
  `tools/build_native.sh` (cargo build, copy into `bin/` under the platform's name; `--check`);
  `tools/install_native.sh` (download the branch's rolling build); `.github/workflows/`:
  `ci.yml` builds the crate and runs `test/run_all.sh` for both runtimes, `native.yml` ported
  from the Goo2D kernel's `kernel.yml` (matrix, checksums, image check, load smoke test,
  bot commit back, rolling pre-release per branch, release on a tag).
- `test/run_tests.gd` learns `--runtime=res://addons/yati_native` (it already takes any
  directory holding an `Importer.gd`); the two tests that reach `DataLoader.gd` and
  `CommonUtils.gd` of a runtime tolerate their absence (they do: `load()` returns null).
- Gate: the suite runs against the facade (every test failing, the facade being empty), the
  three platform libraries are built by CI and load.

### Stage 1: the model and the parsers (pure Rust)

- `src/model/`, `src/parse/`: TMX/TSX/TX and TMJ/TSJ/TJ into one model; properties typed as
  Tiled types them (string, int, float, bool, color, file, object, class with nested members,
  lists); custom types from a `.tiled-project` merged in (`CustomTypes.gd`'s rules); layer
  and chunk payloads in every encoding, the legacy `<tile gid>` form included; zip archives
  (the `zip` crate over bytes). Templates resolved and merged as `create_object` merges them
  (object overrides template: properties by name, id, class/type/name, opacity).
- `TiledMap.load` (synchronous, through the file-request protocol run to completion) and
  `to_dictionary()`.
- Parity test (`test/run_native_parity.gd`): for every fixture in `test/fixtures/` and the
  benchmark map, `TiledMap.to_dictionary()` deep-equals `DictionaryBuilder.get_dictionary()`
  of the GDScript runtime (floats approximately), including the dictionary of each referenced
  tileset and template. Fixtures that need adding: an infinite .tmj with chunks, a zstd layer,
  nested groups with templates, a `.tiled-project` with nested classes.
- Gate: parity on every fixture; the benchmark map parsed and its six blobs emitted in under
  15 ms on this machine (1.2's prototype did it in 7.5 to 11 with threads).

### Stage 2: the one-shot builder and the facade

- `src/build/`: TileSet, layers, objects, exactly the tree the GDScript runtime makes (its
  names, suffixes like ` (PBG)` and ` (PL)`, owners, metadata, groups, materials, parallax
  nodes, the single-child parentless return, the wrapper freed). Tile layers filled through
  `tile_map_data`; alternative tiles created only when `no_alternative_tiles` is off (the
  polygon copies of `create_polygons_on_alternative_tiles`).
- `Importer.gd` over `TiledMap.load` + `TiledTileSets.tileset_for` + `TiledBuilder.build`;
  `PostProcessing.gd` reused as is.
- Gate: `test/run_tests.gd -- --runtime=res://addons/yati_native` passes every test the
  GDScript runtime passes (a short list of accepted differences, each explained, is allowed
  in this document); `test/benchmark.gd -- --runtime=res://addons/yati_native` total under
  120 ms for the benchmark map (parse + 6 × 14 ms of `tile_map_data` + the objects at engine
  speed; the GDScript runtime: 716), the small map under 2 ms warm. *Outcome (section 6):
  the suite passes; the total is 160 to 200 ms because six `tile_map_data` assignments cost
  100 ms on this machine, not the 84 the 14 ms figure assumed, and the objects' engine calls
  about 45: the engine's share, which the gate underestimated. Measured again: 4 to 5× the
  GDScript runtime's time. The number to hold from here is "the engine's own cost plus under
  20 ms", read off `tools/native_gate.gd`.*

### Stage 3: asynchronous loading and the caches

- `load_async` + `poll()` + the `loaded` signal; the worker thread; textures through
  `load_threaded_request` in parallel; `TiledTileSets` shared across imports with
  `build_step`; `Importer.import_async`.
- A test that imports the benchmark map asynchronously while a script measures frame times:
  no frame on the main thread above the engine's own `tile_map_data` + object cost (the parse
  must not show), and the result equal to the synchronous import's tree.
- Gate: main-thread time per frame during an async import of the benchmark map under 5 ms
  until the build frame; the build frame itself at the stage 2 number; a second import of a
  map on the same tilesets skips TileSet creation (measured 0 ms for it).

### Stage 4: the streamer

- `TiledStreamer`: window and follow modes, the chunk grid for finite and infinite layers,
  on-demand payload decode, `set_cell`/`erase_cell` per chunk, the object grid, budgets
  calibrated per cell and per collision cell, signals.
- A generated infinite benchmark map (`test/gen_fixtures.gd`: say 4 000 × 4 000 cells in 16 × 16
  chunks, 4 layers, one with collision on every tile, objects every few chunks) and
  `test/bench_stream.gd`: a camera scrolling at a fast game speed (two screens per second)
  over it, reporting the frame time histogram and the worst frame, with the renderer.
- Gate: at a 4 ms budget no streaming frame exceeds the idle frame by more than the budget
  plus one chunk's engine cost from 1.3 (about 2 ms with collision); memory of the loaded
  `TiledMap` near the file size; chunks that leave the window plus margin are gone (cells
  erased, objects freed, no orphans).

### Stage 5: web build, documentation, adoption

- `experimental-wasm` build with the worker's work sliced on the main thread; CI entry.
- `Runtime.md` gets a "Native importer" section; `addons/yati_native/README.md`; `Reference.md`
  notes any accepted difference; `CHANGELOG.md`.
- The game that drove the 3.0 work switches its board loader to `Importer.import_async` and
  drops its PackedScene cache; its own tests are its gate.

## 5. Risks and open questions

- **godot-rust and threads.** The worker holds no Godot types, which sidesteps the
  experimental threading; if a later need arises to call the engine from the worker (texture
  decoding in Rust, say), measure first: `Image` creation on the main thread from bytes decoded
  in Rust is the safe shape.
- **The `tile_map_data` format is engine-internal.** Its version byte is 0 today; the loader
  checks the engine's version at start and the build stage has a test that round-trips a blob
  through `set_cell` + `tile_map_data` so a format change fails loudly. The fallback is
  `set_cell` from Rust (measured 2.3× slower than the blob for a whole layer, and what the
  streamer uses anyway).
- **Y-sorted layers** (isometric, staggered, `draworder` on object layers) do not use rendering
  quadrants: their per-cell engine cost is different and `bench_materialize.gd` should grow a
  variant before stage 4's budgets are calibrated for them.
- **Collision streaming is the expensive case** (1.9 ms per 16 × 16 chunk here, software GL and
  a shared machine). A game that streams collision-heavy maps may want its collision elsewhere
  (one body per chunk from merged polygons, which the streamer could offer as an option later),
  or a larger `physics_quadrant_size`.
- **Exact parity on text objects, fonts and materials** depends on resources loaded by path;
  these stay on the main thread and may force a `WAITING_FILES` round trip during the build,
  which the state machine already allows.
- **Not in scope:** editing or saving maps, the editor import plugin, Godot 3.

## 6. Status

- 2026-10-18: findings measured (sections 1.1 to 1.3), `test/bench_materialize.gd` committed,
  the pure-Rust prototype kept outside the repository (its numbers are 1.2's).
- 2026-10-18, **stage 0 done** (scaffold and loop), except the CI half of its gate, which runs on
  the first push:
  - The crate `native/` (`yati_native` 0.1.0, godot-rust 0.5.5, `api-4-7`, `cdylib`, release
    `opt-level = 3`, `lto = "thin"`, `.gdignore`'d, `target/` gitignored, `Cargo.lock` committed)
    registers `TiledNative` (RefCounted): `version()` and `sum(PackedInt32Array) -> int`, the FFI
    round trip. The pure-Rust core (`native/tiled/`, `tiled_core`) joins it as a path dependency and
    workspace member later. A cold release build takes about 2.5 minutes here (godot-rust's
    bindings), a no-change rebuild about a second.
  - `addons/yati_native/`: `yati_native.gdextension` (`gdext_rust_init`, `compatibility_minimum =
    4.7`, linux x86_64/arm64, windows x86_64, the macos framework, web entries for later), `bin/`
    (gitignored but for `.gitkeep`: a local build is never committed, `native.yml`'s bot adds the
    CI-built libraries with `git add -f`), `README.md`, and `Importer.gd`, the facade with the
    GDScript runtime's options (same defaults), its four `import*` methods and
    `last_error_count` / `last_warning_count`; it checks `TiledNative.version()` once per process
    (`Importer.native_version()`, through `ClassDB` so the script compiles without a library) and
    every import prints `yati_native: not implemented yet (stage 2)`, counts one error and returns
    null.
  - Tools: `build_native.sh` (`--check`), `install_native.sh` (rolling `native-<branch>`, falling
    back to `native-main`, or a tag; `.yati-native-version` = `branch`; `bin/native-commit.txt`
    names the build's commit), `install_godot.sh` (`.godot-version` = `4.7.2-stable`),
    `smoke_native.gd`, `check_native_image.py`, `verify_native_bin.sh` (ported from the Goo2D
    kernel's, generic names).
  - Workflows: `ci.yml` (Godot, stable Rust with `rust-cache`, the build, the smoke test,
    `test/run_all.sh` for `res://runtime`, the suite for `res://addons/yati_native` with
    `continue-on-error` until stage 2) and `native.yml` (ported from `kernel.yml`: the three-platform
    matrix, per-job staging and checksums, the image check, the load smoke test, the bot commit
    back, the rolling pre-release `native-<branch>`, a release on `v<crate version>`). A `v*` tag
    that is not the crate's version builds but publishes nothing, since the GDScript runtime's
    releases are `v*` tags too.
  - The first `--import` of a fresh `.godot/` with the extension present aborts at shutdown here too
    (exit 134, Godot 4.7.2, reproduced); naming the extension in `.godot/extension_list.cfg` first
    avoids it (the import then exits 0) and also makes the extension load in a run without an
    import. `test/run_all.sh` now seeds that file when the library is in place, imports the project
    (a fresh clone needs it anyway: the fixtures' textures), runs the smoke test, the suite and the
    benchmark. `test/run_tests.gd`'s helpers no longer load a runtime's `DataLoader.gd` /
    `CommonUtils.gd` when it has none (they printed two load errors per test for the facade).
  - Gate, measured: `bash tools/build_native.sh` builds and copies, `--check` succeeds; the smoke
    test prints `smoke_native: TiledNative 0.1.0 loaded, sum round trip ok (4294967296)`; the suite
    against the facade runs to `SUMMARY: 0 passed, 74 failed (new_: 23, bug_: 4, other: 47) of 74`
    with no script error; the GDScript runtime still passes `SUMMARY: 74 passed, 0 failed ... of
    74`; `test/run_all.sh` from a deleted `.godot/` exits 0; `check_native_image.py` accepts the
    Linux library (ELF, 33 section headers) and rejects a truncated copy; `verify_native_bin.sh`
    passes on a staged directory (the real Linux library, stand-ins for the other two) and fails on
    a wrong checksum. Not verified here: the Windows and macOS builds and their load tests, the
    release and bot-commit steps (they run on GitHub).

  the pure-Rust prototype kept outside the repository (its numbers are 1.2's). No stage started.
- 2026-10-18, **stage 1 done** (`native/tiled/`, crate `tiled_core`, pure Rust, no Godot; a
  workspace member and path dependency of the GDExtension crate once merged):
  - The model (`src/model/`: structs mirroring the Tiled JSON format, serde for TMJ/TSJ/TJ and
    the `.tiled-project`), the XML parser (`src/parse/xml.rs`, quick-xml, a recursive descent
    that mirrors `DictionaryFromXml.gd`), payloads (`src/data.rs`: csv, base64, zlib, gzip,
    zstd behind a default feature, legacy `<tile gid>`; kept undecoded until `decode()`), gid
    resolution as `resolve_cell` does it and `tile_map_data` blobs (`src/cells.rs`: per layer,
    per chunk, merged, one thread per layer), the loader with the file-request protocol
    (`src/load.rs`: `needs()`/`supply()`/`missing()`/`step()`, external tilesets, templates and
    their tilesets, the project file, zip archives read directly, messages in the runtime's
    wording), the template merge (`Map::instantiate`) and the custom-type merge, and
    `Map::to_value()` (`src/value.rs`).
  - **Parity**: `test/dump_dictionary.gd` dumps `DictionaryBuilder.get_dictionary()` of a file
    (`test/gen_expected.sh` → `test/fixtures/expected/*.json`, 27 dumps); `tests/parity.rs`
    loads every fixture map (15), tileset (3) and template (3) through the loader, with and
    without a project file, and the zip's members, and compares `to_value()`: equal on all of
    them (keys, ints exact, floats approximately). The benchmark map, its tileset and template
    too (an ignored test, the map being generated). New fixtures for it: `classes.tmx` +
    `classes.tiled-project` + `crate.tx` (nested classes and lists, list properties, multi-line
    strings, nested groups with templates, every custom-type scope, zstd), `infinite.tmj` +
    `json_tiles.tsj` + `json_enemy.tj` (infinite JSON map with chunks, JSON tileset and template),
    `json_twin.tmx` (`json_map.tmj` as TMX: both formats give equal models). 35 Rust tests.
  - **Benchmark** (`cargo run --release --example bench`, the benchmark map, files in memory,
    20 runs, this machine): the loader 4.2 to 4.5 ms (median 4.8 to 5.6); loader + the six
    blobs sequentially 18.8 to 19.2 ms (median 19.8 to 22.2); with one thread per layer 9.2 to
    9.5 ms (median 10.4 to 11.3). Gate (under 15 ms threaded) met. Inflating a layer (zlib-rs,
    410 KB) is 1.4 to 1.7 ms of each layer's 2.1 ms; libdeflate measured twice as fast on the same
    payload, an option should the budget tighten (a C dependency, as zstd is).
  - **Accepted differences from `DictionaryBuilder`** (none occurs in a file Tiled writes; each
    is documented in `native/tiled/README.md`): text inside an `<item>` or an unknown element
    (the runtime stores it under a `data` key) is dropped; an embedded `<image><data>` is skipped
    (the runtime's dictionary of it is garbled); of several `<transformations>` in one tileset
    the last is kept; a JSON `null` in a typed field is dropped and a non-integral number in an
    integer JSON field is a parse error.
  - **Decisions the plan left open**: the format is told by content first, the extension
    second (the runtime goes by extension; the result is the same for every Tiled file); the
    model keeps how a number was written (`Num`), so the XML dictionary's int-or-float typing is
    exact; custom types are merged into the model at load time wherever the runtime merges them
    while building (map, every layer, every object of an object layer, tilesets, tiles, tile
    collision objects; also for layers marked `no_import` and for tilesets only templates use,
    which the runtime skips), templates only when instantiated, as the runtime does; a missing
    template is one error per instance, as in the runtime; references resolve relative to the
    referencing file only (the runtime also tries the bare name relative to the working
    directory first); a project file missing is fatal outside a zip and an error inside one, as
    in `Importer.gd`; atlas geometry comes from `imagewidth`/`imageheight`/`margin`/`spacing`
    with Godot's grid formula, `CellResolver::set_texture_size` for the real texture; source ids
    are assigned as `TileSet.add_source` assigns them (`godot_atlas_id` honoured); JSON
    templates (`.tj`) instantiate (the runtime's `create_object` reads only the TMX
    `objects` key and creates nothing for them).
- 2026-10-18, **stage 2 done** (the one-shot builder and the facade), except the benchmark's
  120 ms, which the engine's own share of it exceeds on this machine today (below):
  - **Classes** (`native/src/`, registered with Godot): `TiledImportOptions` (Resource, every
    option of the facade with its default), `TiledMap` (`load(path, project_file)` and
    `from_zip(zip, path_in_zip, project_in_zip)` through the core's file protocol, files read on
    the main thread with `FileAccess`; `is_loaded`, `get_path`, `get_size`, `get_tile_size`,
    `get_orientation`, `is_infinite`, `get_layers`, `to_dictionary`, `get_errors`,
    `get_warnings`), `TiledTileSets` (`tileset_for(map, options)`: the `TilesetCreator` port and its
    cache; `clear`, `get_cached_count`, last error and warning counts; textures per path) and
    `TiledBuilder` (`build(map, tilesets, options) -> Node`: the `TilemapCreator` port, the whole
    tree in one call; `get_error_count`, `get_warning_count`, `get_messages`). Modules:
    `map.rs`, `options.rs`, `tileset.rs`, `builder/` (`mod.rs` layers and the root, `cells.rs`
    tile layers, `objects.rs`, `nodeprops.rs` the property rules), `files.rs` (textures,
    resources, zip members), `props.rs` and `gd.rs` (the runtime's property dictionaries and
    GDScript's `str()`, typing, colours, masks, paths), `diag.rs` (messages), `dynobj.rs`
    (`Parallax2D`, `NavigationPolygon` and `NavigationRegion2D`, which godot-rust lists as
    experimental API, reached through `ClassDB`, so the crate needs no experimental feature).
  - **Tile layers**: every tile layer's payload decoded on one thread per layer (pure Rust; the
    distinct cell values collected on the way), each distinct value resolved once on the main
    thread as `resolve_cell` resolves it (tiles and, with `no_alternative_tiles` off, alternative
    tiles with their flipped polygons created on the way), then one `tile_map_data` assignment of
    the core's blob per layer (an infinite layer's chunks merged). The format check of section 5
    runs once per process: two cells through `set_cell` must read back as our blob and our blob
    back as those cells; otherwise an error says so and `set_cell` fills the layers.
  - **Facade** (`addons/yati_native/Importer.gd`): the four `import*` methods over `TiledMap`,
    a fresh `TiledTileSets` per import (imports never share a TileSet, as
    `test_import_twice_independent` requires) and `TiledBuilder`; `last_error_count` /
    `last_warning_count` (the map's load messages plus the build's), the summary line,
    post-processing through `PostProcessing.gd` (copied from `runtime/`, MIT header kept), and a
    two-line `CommonUtils.gd` whose static counters the facade sets after each import (the
    suite's `_error_count()` reads them). `make_options()` gives the options as a resource.
  - **Core** (`native/tiled/`): three small additions, `Property::to_value(json)` and
    `Property::dict_value(json)` (a property's dictionary, or its value alone, as the runtime
    holds it) and `Property::from_project()`. The parity list has the part A fixtures
    (`animations`, `area_gravity`, `clip` (TMX and TMJ), `colors`, `layer_class`, `parallax`,
    `single_layer`, `template_files`, `templates`, `unknown_class`) and their templates
    (`probe_rect.tx`, `probe_text.tx`, `probe_tpl.tx`, `tpl/door.tx`); all equal.
  - **Tools**: `tools/smoke_native.gd` imports `test/fixtures/layers.tmx` through the facade as well
    (works without a project import: un-imported images load with `Image.load_from_file`);
    `tools/native_gate.gd` checks for leaks (every fixture and the zip, 20 rounds, a few frames
    before each count since the servers release what freed navigation regions handed them only on
    their next iteration), times the small map and measures the engine's floor;
    `test/benchmark.gd` times a runtime without `DataLoader.gd` with the native classes (parse =
    `TiledMap.load`, tilesets = `TiledTileSets.tileset_for`). `ci.yml` runs the suite against the
    facade as a required step, `native_gate.gd`, the benchmark, and tests, lints and formats both
    crates; `native.yml` rebuilds when the facade's scripts change (the smoke test imports).
  - **Gate, measured** (this machine, Godot 4.7.2, a busy shared container: numbers vary by ±20 %
    between runs): `test/run_tests.gd -- --runtime=res://addons/yati_native`: `SUMMARY: 90 passed,
    0 failed (new_: 0, bug_: 0, other: 0) of 90`, no engine error printed but the missing-tileset
    fixture's own; the GDScript runtime still `90 passed ... of 90`, `test/run_all.sh` exits 0;
    `cargo test`, `clippy -D warnings` and `fmt --check` clean for both crates, no `unsafe` but the
    `#[gdextension]` entry point godot-rust requires.
    Benchmark: `RESULT runtime=res://addons/yati_native total_ms min=160.2 avg=182.1 | parse avg=7.2 |
    tilesets avg=1.4 | tile layers avg=124.2 | objects avg=49.4` (other runs: min 168 to 188, avg 171
    to 200); the GDScript runtime in the same session `total_ms min=671.6 avg=772.2 | parse avg=164.4
    | tilesets avg=3.4 | tile layers avg=386.3 | objects avg=218.0`, so 4 to 5 times faster. The
    small map (`layers.tmx`, 10 warm imports): mean 1.57 to 2.44 ms over seven runs, under 2 ms in
    four of them, min 1.42 to 2.30 (the GDScript runtime 3.2 to 4.7); quietly, the native parts
    split as `TiledMap.load` 0.2 ms, TileSet 0.6, build 1.0. Leaks: none (objects 1498 -> 1498, nodes and orphan nodes unchanged after
    20 rounds of 24 maps; the GDScript runtime alike).
  - **Why the 120 ms are not met**: the engine's share. `native_gate.gd` imports the benchmark's
    tiles-only map in 117.9 ms, of which the six `tile_map_data` assignments alone take 102.0 ms
    when repeated on fresh layers (the rest, 16 ms, is parse, TileSet, decoding, resolution and
    blobs). The plan's estimate counted 6 x 14 ms; `bench_materialize.gd` measures 26.7 ms per
    layer on this machine today (14 when the plan was written), so the floor has moved by more than
    the 36 ms the estimate had to spare. The objects phase (about 45 ms for 2 720 objects; the same
    node calls from GDScript, measured bare, take about 20 ms) is the engine calls plus godot-rust's
    per-call checks (object alive, main thread): built with the crate feature `unchecked`
    (`safeguards-release-disengaged`) the objects-only part of the map takes 33 ms instead of 47.
    That feature is off by default and in CI (it turns a use of a freed node into undefined
    behaviour, and user scripts run during a build: `godot_script` properties, scene instances).
    The whole benchmark with `unchecked`: `total_ms min=148.9 avg=155.8 | parse avg=6.0 | tilesets
    avg=1.3 | tile layers avg=113.1 | objects avg=35.4` (best of two runs; the default build in
    the same session, min 169 to 207). Scaled by today's floor (102 ms for the plan's 84), the gate
    would be about 146 ms: within reach with `unchecked` on a quiet machine, not by default.
  - **Accepted differences** (none visible to the suite; `addons/yati_native/README.md` lists
    them for users): no `__internal__` custom data layer, the runtime's marker of tiles with
    collision shapes, which it removes at the end of an import (kept on the Rust side; a TileSet
    saved with `save_tileset_to` and the TileSets of templates, which the runtime never cleans,
    have that layer less); custom types merged into tilesets only templates use (stage 1's
    decision, the runtime merges none there); references resolved relative to the referencing file
    only (stage 1's decision); a tile object of class `instance` keeps its sprite's scale (the
    runtime reads it from the sprite it has just freed, a script error); an embedded tileset after
    an external one resolves its image relative to the map (the runtime, relative to the previous
    external tileset's directory); a numeric object or layer name keeps its spelling (the XML
    dictionary types `name="007"` as the int 7); the template merge also applies the instance's
    `width`, `height`, `rotation`, `visible`, `text` and `gid` in the builder (B1, B2) and joins
    its file properties with the map's directory (B10b), as the runtime's `create_object` does,
    while `tiled_core`'s `Map::instantiate` stays the stage 1 merge; JSON templates (`.tj`) instantiate
    (stage 1's decision; the runtime's `create_object` creates nothing for them).
  - **Decisions the plan left open**: the TileSet cache key adds the map's tile size, orientation
    and stagger, and the zip archive, to the plan's (tileset paths or embedded-tileset hashes with
    first gids, `custom_data_prefix`, `map_wangset_to_terrain`, `animation_mode`; a template's
    TileSet has its own entry, built with an empty prefix as the runtime builds it); strips are
    cached with the TileSet they belong to (a per-(tileset, tile) cache matters once TileSets are
    shared across combinations, stage 3); the facade uses a fresh `TiledTileSets` per import, so
    nothing is shared between imports yet (stage 3's job, with the test's expectation in mind);
    `TiledMap.load` and `from_zip` are instance methods returning an `Error` (the facade reaches the
    classes through `ClassDB`, so they must work on an instance); `TiledMap.load` prints the load's
    messages itself, `build` its own, the facade adds the counts; the messages keep the runtime's
    wording and colour tags (`Yellow` for the map's, `yellow` for the tilesets').
- 2026-10-18, **stage 2b done** (the builder catches up with the runtime improvement plan's part
  B: F1, F2, F3, F5; section 7):
  - **Options** (`TiledImportOptions` and the facade, the runtime's comments and defaults):
    `object_refs` (`"nodepath"`), `class_scenes` (`{}`; keys compared in lower case, a later key
    wins), `read_godot_scene_from_custom_types` (true), `assign_node_properties` (true). An
    unsupported `object_refs` warns and uses `"nodepath"`, as `set_object_refs` does.
  - **F2** (`builder/refs.rs`): every creation site registers its node by object id (the first
    wins: a tile object's body, not its sprite; the id metadata as before, now on text objects'
    Labels too); object properties and lists with object items wait in a pending list, resolved
    after the map's properties (and again for the single-layer root's), with the runtime's
    assignment rules (a `NodePath` export the path, a `Node` export of the right class the node, an
    untyped variable the path, a typed array the list, else metadata; an all-NodePath list an
    `Array[NodePath]`), the empty-path warning, and `"both"`'s `<name>_shape` geometry.
  - **F1** (`builder/objects.rs`, `builder/assign.rs`): the class-to-scene resolution after a
    `godot_node_type` and before the class mapping, for shape objects and (the object's class,
    then the tile's) tile objects; `class_scenes` relative to the map, a `godot_scene` member
    relative to the project file, an object's own `godot_scene` (a property not merged from the
    project) relative to its file; one `instantiate_scene` for scenes, class `instance` and tile
    instances (position, rotation and scale only on a Node2D or Control, visibility on a
    CanvasItem, "is not a scene" as an error, a tile object's sprite freed once the scene loads).
    `godot_script` is attached in a first pass; the metadata fallback assigns to a node property
    of the same name with `convert_for_property`'s coercions (int to float and enum, an enum's name,
    StringName, NodePath, a file's resource for an Object property, a list to the property's typed
    array, anything to an untyped variable) or warns and keeps metadata; `godot_scene` is skipped
    as a property. `_tiled_import(info)` runs at the end with the runtime's `info` keys (`id`,
    `name`, `class`, `properties` typed, references as NodePaths, `rect`, `shape`, `shape_type`).
  - **F3**: `map_rect`, `map_size_tiles`, `tile_size`, `orientation` on every root (also the
    single-layer one and `import_from_zip`'s) from `Map::tile_rect` / `pixel_rect`;
    `native/src/world.rs`'s `TiledWorld` pumps the core's `WorldLoader` (`FileAccess` for files,
    `DirAccess.get_files_at` for a pattern's directory, the core's `from_zip` for an archive);
    `addons/yati_native/WorldLoader.gd` (`parse(world_file, zip_file = "")`, `clear_cache()`) and
    `Importer.import_world(world_file, project_file = "")` over it.
  - **F5** (`props::mask_value`): int masks and flag-enum value names (the project's enums by
    their `propertytype`) on every mask property of nodes (`collision_layer`, `collision_mask`,
    `light_mask`, `visibility_layer`, `platform_floor_layers`, `platform_wall_layers`,
    `navigation_layers`, `parsed_collision_mask`, `occluder_light_mask`; a `__` property forwards
    its `propertytype`) and of the tileset (`collision_layer[_n]`, `collision_mask[_n]`,
    `layers[_n]`, `light_mask[_n]`), the unknown name's warning in `CustomTypes`' colour.
  - **Parity and tools**: `tests/parity.rs` and `test/gen_expected.sh` cover `object_refs.tmx`,
    `scenes.tmx` and `masks.tmx` (also with their project files) and the four maps of `world/`
    (`expected/world_*.json`); all equal, the existing dumps unchanged on regeneration.
    `tools/native_gate.gd` imports those three maps (with their projects), `scenes.tmx` with
    `class_scenes` and the test world every round. `ci.yml` requires the native suite again.
  - **Gate, measured** (this machine, a shared container): `test/run_tests.gd --
    --runtime=res://addons/yati_native`: `SUMMARY: 103 passed, 0 failed (new_: 0, bug_: 0, other:
    0) of 103`; the GDScript runtime still `103 passed ... of 103` and `test/run_all.sh` exits 0
    for both; `native_gate.gd`: no leaks (objects 1502 -> 1502, nodes and orphans unchanged, 20
    rounds of 57 imports; the GDScript runtime alike), the small map 1.96 to 2.01 ms mean; `cargo
    test`, `clippy -D warnings`, `fmt --check` clean. Benchmark, the stage 2 library and this one
    alternated five times in one session: before `total_ms` min 164.3 to 200.6 (mean of the
    averages 193.5), after min 166.4 to 174.7 (mean of the averages 178.9); the objects-only map,
    40 imports each, min 52.5 to 53.8 ms before, 54.2 to 55.8 after (the registry, the hook check
    and the property-list lookups: about 2 ms of 2 720 objects).
  - **Decisions**: the core's `Project` keeps its file's path (`Project::path`, `dir()`; the loader
    sets it), for `godot_scene` members; a node becomes a hook candidate only when it is a scene's
    root or a `godot_script` property ran on its properties (the runtime checks every object node,
    the result is the same); engine classes' property lists are cached for the process (scripted
    nodes per import, as the runtime does); a typed array for a property is made from the property's
    own value (`duplicate`, `clear`, `assign`), godot-rust having no runtime-typed array
    constructor; `WorldLoader.clear_cache()` has nothing to clear (the core reads a pattern's map
    sizes on every parse); `addons/yati_native/TilemapCreator.gd` is a stand-in with `create`,
    `get_map_rect` and `get_tileset`, for `test_new_map_rect_finite`. The TileSet cache key does not
    name the project file (its custom types and now its enums shape a TileSet): stage 3's sharing
    across imports should add it. The stage 2 difference "a tile object of class `instance` keeps
    its sprite's scale" is gone: the runtime's `instantiate_scene` reads the scale before freeing the
    sprite now. New accepted differences: a JSON template's instance exists, so a reference to it
    resolves (the runtime warns about a dangling one in `infinite.tmj`); property lists cached per
    process (above); `TilemapCreator.create` prints the import's summary line.

- 2026-10-18, **stage 3 done** (asynchronous loading and the caches):
  - **The worker** (`native/tiled/src/worker.rs`, in the pure-Rust core): `Worker::spawn(loader,
    first_files)` runs the core's `Loader` on a thread named `tiled-loader`; each batch of files
    the main thread hands over (`supply`) is answered with one `Event` (`Need { paths, images }`,
    `Done(Box<Map>)`, `Failed(errors)`), read without blocking (`try_event`). The thread function
    takes and returns only `tiled_core` and `std` types, and `tiled_core` does not depend on Godot,
    so no engine type can reach the thread (the compiler would refuse: it is not in the crate's
    dependency graph); `experimental-threads` stays off. A dropped worker is never waited for: its
    channel closes, the thread ends at its next message, and its handle is joined when the engine
    shuts down (`join_detached`, from the extension's `MainLoop` deinit). The loader learned
    `take_discovered_images()`: the images of the map's embedded tilesets and image layers and of
    every external tileset parsed so far, sent with each `Need`, so textures start loading before
    the map is done. 5 Rust tests (the threaded load equals the synchronous one on four fixtures,
    images found early, a missing map, a zip parsed entirely on the worker, a worker dropped mid-load).
  - **`TiledMap`** (`map.rs`): one state machine for both ways of loading. `load_async(path,
    project)` / `from_zip_async(zip, path_in_zip, project_in_zip)`, then `poll() -> int` once per
    frame (constants `READING` 0, `PARSING` 1, `WAITING_FILES` 2, `READY` 3, `FAILED` 4): each poll
    does at most one round, reading the requested files with `FileAccess` and handing them over (the
    first poll reads the map and project, a zip archive whole; the loader's first step, which only
    asks for them, runs in `load_async`), requests the map's imported `res://` images with
    `ResourceLoader.load_threaded_request` as soon as the parse names them (the rest of `Map::images()`
    when the map arrives) and collects them with `load_threaded_get` once their status is no longer
    `IN_PROGRESS`, so the build never waits on a texture; the map holds the collected textures, and
    the build's `ResourceLoader.load` finds them in the engine's cache. Non-`res://` images are left
    to the build (`Image.load_from_file`, as before). Signals `loaded` and `failed`, emitted through
    `base_mut()` from the `poll()` that ends the load (a handler may call back into the map; tested);
    `get_status()`, `cancel()` (no signal; `poll()` then returns `FAILED`). `load` / `from_zip` pump
    the same machine with the loader on the main thread and no texture requests. Cancelling or freeing
    a map with texture requests in flight hands them to a per-frame drain (`ExtensionLibrary::
    on_main_loop_frame`, `files::drain_orphans`), which collects each once loaded; at shutdown the
    rest are collected waiting. godot-rust hides the three `load_threaded_*` methods without
    `experimental-threads`; they are called on the main thread through the engine's dynamic call
    (`Object::call`), the loading threads being the engine's own and running no code of ours.
  - **`TiledTileSets`** (`tileset.rs`): the cache holds a list of TileSets per key and is shared
    across imports. The key gained `no_alternative_tiles` (the builder adds alternative tiles to a
    TileSet: `test_new_option_no_alternative_tiles` failed with a shared cache until it was in) and a
    hash of the project file's `propertyTypes` (custom types are merged into tilesets; stage 2b's
    enums shape their properties). The creator (`TilesetCreator`'s port) now owns its parameters,
    messages and zip reader and is resumable: `build_step(map, options, tiles_per_step) -> bool`
    creates the map's TileSet and its templates' a budget of tiles per call (a tileset's header and
    end count one each, the animation strips one each) and returns true when all are cached;
    `tileset_for` and `build` finish a creation `build_step` left half done. A creation records its
    messages quietly; every use of a cached TileSet replays them (printed and counted), so an
    import's counts do not depend on the cache. `exclusive` (default false): hand a cached TileSet
    only while nothing but the cache holds it (its reference count is 1 and no build holds its
    registry), otherwise build another for the key. `clear()` (TileSets and half-made creations),
    `clear_textures()`, `get_created_count()`, `get_texture_count()`. Textures are cached per
    process (`files.rs`, a main-thread `thread_local`, cleared at the extension's `MainLoop` deinit),
    by resolved path or `zip::member`.
  - **Facade**: `import_async(path, project = "")` and `import_from_zip_async(...)` (coroutines):
    `load_async`, one `poll()` per `process_frame` until `READY`/`FAILED`, `build_step` with
    `async_tiles_per_frame` (256) per frame (the frame that finishes a TileSet is not the build's),
    then `build` in one frame; `last_error_count` / `last_warning_count` and the summary line as for
    `import`. `share_tilesets` (default true): every import, synchronous or not, uses one static
    `TiledTileSets` (`Importer.shared_tilesets()`) with `exclusive` on; off, or with
    `save_tileset_to` set, an import gets a fresh one (stage 2's behaviour).
  - **Gate, measured** (this machine, Godot 4.7.2, 4 cores, another build running at times; the
    async test turns the engine's headless frame sleep off, `OS.low_processor_usage_mode_sleep_usec
    = 0`, since headless Godot pads every frame to 6.9 ms and a frame's wall time is then its
    main-thread work): `test/test_async.gd -- --rounds=5`, both paths warmed up once, caches cold
    each round: **worst main-thread frame before the build frame 1.08 ms and 2.77 ms** in two runs
    (3 rounds in `run_all.sh`: 1.62 ms; before the warm-up covered the async path, up to 2.9 ms),
    over 1 072 to 3 109 frames per import (the first poll, reading the 2 MB map, costs 2.4 to 2.7 ms
    when timed alone, every later poll 0.01 to 0.2 ms). **Build frame median 166.3 ms and 171.1 ms
    (min 159.7, 160.7) against the synchronous build's median 175.6 and 175.4 ms (min 159.3,
    160.8)**: the same; the synchronous import's total median 184.5 and 183.0 ms (load 6.2, TileSets
    1.9 to 2.1). The async tree equals the synchronous one (4 562 nodes: names, classes, child counts,
    metadata keys, 522 358 cells), as do six fixtures' and the zip's. **A second map on the same
    tilesets** (`bench_tiles_only.tmx` after `bench.tmx`): `tileset_for` 0.03 to 0.07 ms against
    0.4 to 0.6 ms for the first (textures cached), created and cached counts unchanged (1 -> 1);
    through the facade, after the first tree was freed, created 11 -> 11, cached 2 -> 2. **Cancel**:
    mid-parse, dropped mid-parse, and with texture loads in flight: `OBJECT_COUNT` 1498 -> 1498, no
    loader thread and no texture request left. `SUMMARY: 45 passed, 0 failed`. `tools/native_gate.gd`:
    leaks none (objects 1839 -> 1839 after 20 rounds of 24 maps); the small map 0.87 ms mean (min
    0.78) synchronously, the shared TileSet cache taking its 0.6 ms TileSet out (stage 2: 1.6 to 2.4);
    `import_async` of it: 22 to 51 frames, wall mean 1.35 ms (min 0.99), worst frame before the build
    0.47 ms, build frame mean 0.98 ms. With `share_tilesets` off the small map takes what it took
    before (an A/B against stage 2's code: medians 1.3 to 1.9 ms against 1.7 to 2.4, same session).
    `test/run_tests.gd`: the GDScript runtime `SUMMARY: 103 passed, 0 failed ... of 103`, the facade
    `SUMMARY: 90 passed, 13 failed (new_: 13, bug_: 0, other: 0) of 103` (the 13 part B tests stage 2b
    fixes, as before); `cargo test --release --workspace` 53 passed; clippy `-D warnings` and `fmt
    --check` clean. The benchmark (`test/benchmark.gd`, facade): `total_ms min=185.4 avg=208.6 | parse
    avg=7.7 | tilesets avg=0.5 | tile layers avg=152.2 | objects avg=48.2`; its tile layer phase now
    subtracts a TileSet time the shared cache no longer spends (the tiles-only import reuses the full
    map's TileSet), about 1.5 ms too little.
  - **Decisions the plan left open**: the facade shares by default but exclusively, since
    `test_import_twice_independent` (two live imports of one map must not share a TileSet) is the
    GDScript runtime's contract; `exclusive = false` gives the plan's one-TileSet-per-combination
    sharing. `save_tileset_to` builds unshared. Textures are prefetched for `res://` images only and
    collected before `READY` rather than when the build needs them, so the build never blocks;
    `WAITING_FILES` is that wait. A cached TileSet's messages are replayed on every use. The cache
    key hashes the project's types rather than its path (the project's content is what shapes the
    tilesets). `poll()` reads a round's files in the poll that receives the request (one round per
    poll); `READING` is the status between `load_async` and the first poll. The build frame keeps the
    builder's own per-layer decoding (`builder/cells.rs`); moving the decode onto the worker would
    take about 10 ms off the build frame of the benchmark map and is left for later.

## 7. Behaviour changes from the runtime improvement plan

`docs/runtime-improvement-plan.md` (brought in 2026-10-18) reviews the GDScript runtime and
lists bug fixes (B1 to B10), features (F1 to F5) and a Tiled extension (X1, set aside for
now). They are taken up, and this is where each lands:

- **The GDScript runtime first, the native builder second, the suite as the spec.** Every
  item of parts A and B (except F4, below) is done in `runtime/` with its `test_bug_*` or
  `test_new_*` test and fixture, in the order that plan suggests (B5, B3/B4, B7, B2, B1, B8,
  B6, B9, B10, F2, F1, F3, F5). Stage 2's gate, "every test the GDScript runtime passes",
  then covers them for the native builder without a second specification. None of them
  changes `DictionaryBuilder`'s output, so stage 1's parity gate is untouched.
- **New options** (`parallax_mode`, `unknown_class_fallback`, `animation_mode`,
  `always_wrap_root`, `class_scenes`, `read_godot_scene_from_custom_types`,
  `assign_node_properties`, `object_refs`) become variables of both `Importer.gd` facades
  and fields of `TiledImportOptions`, with the defaults that plan recommends (parallax2d,
  strip, `object_refs = "nodepath"`, `assign_node_properties` on, `always_wrap_root` off,
  `unknown_class_fallback = "staticbody"`). B4 makes colour-typed metadata a `Color`
  (breaking, the next minor version).
- **F4 (tileset cache, shared TileSet resource, async import) is not done in GDScript.**
  Stages 3 and 4 here are its native answer (`TiledTileSets`, `load_async`,
  `TiledStreamer`); the GDScript runtime stays synchronous and its counters static.
- **F3 (world files, map bounds)** is done in both: `runtime/WorldLoader.gd` and `map_rect`
  metadata as that plan says; in the native version `tiled_core` parses `.world` files
  (explicit `maps` and `patterns`; the directory listing a pattern needs is one more request
  of the file protocol, answered by the main thread), `TiledMap` exposes `map_rect`, the
  builder sets the metadata, and stage 4's `TiledStreamer` gets a world mode: several maps
  placed by the world file, each a chunk source, loaded and unloaded as the window reaches
  them.
- **B9's strip animation mode** in the native builder composes the strip image on the main
  thread (`Image.blit_rect`; textures are engine objects) and caches it in `TiledTileSets`
  per (tileset, tile), as that plan's cache note asks.
- **F1's `_tiled_import` hook and property assignment** run on the main thread in the
  builder; the type coercions listed there are the builder's, from the model's typed
  property values.
- **Status, 2026-10-18:** part A (B1 to B10) and F1, F2, F3 and F5 are done in the GDScript
  runtime, each with its tests (`CHANGELOG.md`, `[Unreleased]`); F3's world parsing and
  `Map::map_rect` are in `tiled_core` too (`native/tiled/src/world.rs`, tested on the same
  fixtures). The runtime's new options for the facades: `object_refs = "nodepath"`,
  `class_scenes = {}`, `read_godot_scene_from_custom_types = true`,
  `assign_node_properties = true`; new methods `Importer.import_world()` and
  `WorldLoader.parse()`.
