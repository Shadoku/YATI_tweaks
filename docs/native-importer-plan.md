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
  bin/                         the libraries (CI-built, committed), kernel-commit.txt
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
  speed; the GDScript runtime: 716), the small map under 2 ms warm.

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
  the pure-Rust prototype kept outside the repository (its numbers are 1.2's). No stage started.
