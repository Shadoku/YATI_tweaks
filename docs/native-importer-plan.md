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
  erased, objects freed, no orphans). *Outcome (section 6): met against a control run (the
  same scroll over preloaded content, the streamer paused) at p99 and at the worst frame in five
  of six runs, and a streaming frame less the streamer's own work costs what a control frame
  costs; the literal reading (the worst streaming frame against one idle frame) cannot hold on
  this machine, whose still frames alone spread by 8 ms and more. Memory: 1.56 times the file.*

### Stage 5: web build, documentation, adoption

- `experimental-wasm` build with the worker's work sliced on the main thread; CI entry.
- `Runtime.md` gets a "Native importer" section; `addons/yati_native/README.md`; `Reference.md`
  notes any accepted difference; `CHANGELOG.md`.
- The game that drove the 3.0 work switches its board loader to `Importer.import_async` and
  drops its PackedScene cache; its own tests are its gate.

*Outcome (section 6): documentation done, the two loose ends of stages 3 and 4 closed, an option the
game's adoption asked for added (`warn_unknown_classes`); the web build not made (no Emscripten on
this machine): what it needs is written down. The game's switch is a change in its own repository.*

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
  variant before stage 4's budgets are calibrated for them. *Measured in stage 4 (`--ysort`): a
  y-sorted layer re-sorts all of its cells in every frame that changes it, after the streamer's
  work, so the streamer refuses y-sorted tile layers unless told otherwise (section 6).*
- **A `TileMapLayer` remembers every area it has shown** (found in stage 4, Godot 4.7.2): one
  object, about 1.3 KB, per 16 × 16 area that ever held a cell, kept after its cells are erased
  and freed only with the layer, whatever `rendering_quadrant_size`, `physics_quadrant_size` or the
  collision, navigation and occlusion switches say (20 000 areas set and erased: 20 000 objects,
  +24.2 MB, all objects back when the layer is freed). A streamer that crosses a whole huge map
  accumulates it (80 MB per layer for 4 000 × 4 000 cells). World mode frees a map's layers with the
  map; for one huge map, replacing a layer by a fresh one once it has seen many areas would bound it,
  at the price of re-adding its live cells in one frame (an option for later, if games need it).
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

- 2026-10-18, **stage 4 done** (the streamer):
  - **`TiledStreamer`** (`native/src/streamer.rs`, a `Node2D`): `map` (a `TiledMap`, loaded or
    loading: it is polled) or `world` (a `TiledWorld`), `tilesets` (null: `TiledTileSets.shared()`),
    `options`, `project_file` (the world's maps'); `window: Rect2i` in cells, or `follow: Node2D` +
    `margin: Vector2i` (the window is the followed node's chunk grown by `margin` cells, recomputed
    each frame); `keep_margin` (16 × 16: a loaded chunk stays until it is that far outside the
    window), `world_margin` (32 × 32), `world_cell_size` (16 × 16 px: world mode's cells),
    `budget_us` (2000; `<= 0` unbounded), `keep_objects_resident` (false), `chunk_size` (16 × 16),
    `allow_y_sorted_layers` (false). `_process` calls `step(budget_us) -> bool` (also callable by hand
    with processing off); `load_all()`, `is_settled()`, `loaded_chunk_count()`,
    `get_loaded_chunks(layer)`, `get_spawned_object_count()`, `get_layer(name) -> TileMapLayer`,
    `get_object_layer(name)`, `get_map_root(path)`, `get_loaded_maps()`, `get_costs()`, `get_stats()`,
    `reset_stats()`, `get_error_count()`, `get_warning_count()`. Signals `chunk_loaded(layer_name,
    coords)`, `chunk_unloaded(layer_name, coords)`, `object_spawned(node, id)`,
    `object_despawned(id)`, `map_loaded(path, root)`, `map_unloaded(path)`, `settled()` (emitted by
    the frame whose work completes the window), all after the frame's work.
  - **How a map streams.** Its tree is made by the builder with every layer node and nothing in
    them (the `Skeleton`: one persistent `TileMapLayer` per Tiled tile layer and one node per object
    layer, configured exactly as `TiledBuilder.build` configures them; image layers, the background,
    groups, parallax wrappers, layer and map properties made at once), under a root named after the
    map (never unwrapped), at the world rect's origin in world mode. Every tile of its TileSet is
    created before the first cell is set (`tileset_gids`, a budgeted phase): a TileSet changed while
    layers hold cells makes the engine rebuild every cell of every layer that uses it (measured here:
    `create_tile` in a live TileSet with 30 720 cells, 42 to 59 ms in `update_internals` plus a 51 ms
    frame; a `texture_origin` set on an unused tile, 38 ms). A tile layer's cells come from sources: an
    infinite layer's chunks (decoded when wanted, never kept: the model keeps the payloads as the file
    has them) or a finite layer's data (decoded once on a thread of its own, `tiled-decode`, pure
    `tiled_core`); a grid maps each chunk of `chunk_size` to the sources that have cells in it (a
    Tiled chunk of another size spans several). Loading a chunk: its non-empty cells collected, each
    distinct value resolved once per map (the builder's `resolve_cell`), `set_cell` per cell, then
    `TileMapLayer.update_internals()`, so the engine's quadrant and physics work for the chunk is done
    (and measured) in the streamer's frame; a bit per cell records what was set, and unloading erases
    exactly those cells, then `update_internals()`. Objects are indexed by the chunk their `x`, `y`
    lies in (isometric maps: both divided by the tile height, as Tiled stores them) and created by
    the builder's `handle_object` under their layer's node when that chunk enters the window
    (`draworder="index"` layers keep the layer's order with `move_child`), freed when it leaves; each
    object's nodes (a template can add several) are remembered and the id forgotten. Object
    references are kept and resolved again at the end of every frame whose objects came or went: a
    NodePath while both nodes exist, an empty one (no warning) otherwise. `_tiled_import` runs after
    that frame's references. In `load_all()` mode every object of a layer is created in the layer's
    order, so the tree equals the one-shot build's to the node.
  - **The budget.** Each piece of work (a tile chunk in or out, a chunk's objects in or out) is
    estimated before it runs from costs the streamer fits to its own measurements: a least-squares
    fit with forgetting (0.97 per sample) and a ridge prior (the initial guesses), `t = chunk + cell ×
    n + y-sorted cell × n_ysort + collision cell × n_collision` for tile chunks (collision cells:
    tiles with a collision polygon on any physics layer), `chunk + cell × n (+ y-sorted cell)` for
    erasing, `chunk + object × n` for spawning and freeing. A frame stops before the piece whose
    estimate exceeds what is left of `budget_us`; one piece always runs (also when the map's phases,
    loading, TileSets, tiles, decoding, took the frame's first 100 µs and more, which then count as
    that piece). Order: one unload first (so unloading never starves), then loads nearest the window's
    centre first, then the other unloads farthest first. (The first version averaged µs per cell; the
    per-chunk fixed cost made small chunks read 120 µs per cell: hence the fit.)
  - **World mode.** Each map of the world is a slot: loaded with `TiledMap.load_async` (polled once
    per frame) when the window grown by `world_margin` reaches its rect, its TileSet built with
    `build_step` within the budget, streamed like one map; once the window grown by `world_margin +
    keep_margin` no longer reaches it, its chunks unload through the budget and, when none is left,
    its tree is freed (`map_unloaded`). Maps with the same tileset combination (the cache key, now
    `TiledTileSets::map_key`) share one TileSet: the streamer holds a `Weak` per key, so a cached
    TileSet is reused while one of its maps is live and the exclusive cache hands it out again later.
  - **Y-sorted layers: refused by default.** `bench_materialize.gd -- --ysort` (renderer, 218-cell
    chunks set into a live layer, frame time over the idle frame): a y-sorted layer costs +4.2 ms for
    one chunk per frame into a 64 × 64 layer (+4.9 ms for eight), +4.5 ms (+7.7) into 160 × 160, +8.0
    ms (+18.3) into 320 × 320, erasing eight +2.1, +1.2 and +10.7 ms; the same without y-sort: +2.0
    (+4.4), +0.4 (+5.2), -0.9 (+1.5), noise at one chunk. So a frame that changes a y-sorted layer
    pays a re-sort of all of its cells after the streamer's work, growing with the layer and out of
    any budget's reach. The streamer refuses a map whose tile layers are y-sorted (isometric,
    staggered) with an error naming the cause and the ways out; `allow_y_sorted_layers = true`
    streams them anyway, their cells budgeted at their own measured cost (14.5 to 15.2 µs per cell
    against 2.0 for an ordinary one; erasing 4.6 to 7.4 µs against 1.0); `load_all()` always takes
    them (it is the one-shot build). Object layers sorted by y stream normally (their cost is per
    object).
  - **Facade**: `Importer.make_streamer(map_or_world, options = null, project_file = "")` (a map path
    loaded with `load_async`, a `.world` path, a `TiledMap` or a `TiledWorld`; the importer's options
    and the shared cache unless `share_tilesets` is off); `Importer.shared_tilesets()` is now
    `TiledTileSets.shared()` (a static of the extension, released at its shutdown), so streamers and
    imports share one cache.
  - **Refactored in `builder/`** (no behaviour change for `build`): `Build`'s owned state and a
    `Suspended` twin are declared by one macro (`build_state!`) with `suspend()` / `resume(map,
    tilesets, state)`, so the streamer keeps a map's build across frames (names, cell and atlas caches,
    the object registry, scripted nodes' property lists, references, reported warnings) and resumes it
    to create objects and resolve cells. Two new fields: `skel` (a streaming build: `handle_layer`
    records tile and object layer nodes instead of filling them) and `stream_refs`.
    `finish_import`'s metadata part is `root_metadata`; `resolve_object_refs` resolves each reference
    with `resolve_ref`, which the streamer calls again; `ref_path` stays silent about missing targets
    in a streaming build. `builder/stream.rs` holds the streaming entry points (`for_stream`,
    `run_skeleton`, `resolve_value`, `spawn_object`, `forget_object`, `update_refs`, `run_hooks`,
    `tileset_gids`). Verified: the suite against the facade 103 of 103 before and after; an A/B of
    objects-only builds of the benchmark map (40 builds each, alternated three times with the stage 3
    library built from its commit): min 46.2, 47.0, 47.9 ms before, 45.9, 42.9, 50.0 after.
  - **Benchmark** (`test/gen_stream_bench.gd` → `test/out/stream/`: `stream.tmx`, infinite, 4 000 ×
    4 000 cells, 57.9 MB, base64 + zlib chunks drawn from 32 variants per layer: Ground every cell,
    Detail a third (a third of those flipped), Walls about a fifth on `walls.tsx` whose every tile has
    a full-tile collision polygon, Deco sparse in half the chunks; an object in every third chunk,
    every tenth referring to the previous one; `stream_iso.tmx`, the same as an isometric map of 1 024
    × 1 024 cells; `stream.world`, 3 × 3 finite maps of 256 × 256 cells; generated in about 2.3 s).
    `test/bench_stream.gd` (renderer: xvfb, llvmpipe, 4 cores of a shared VM; viewport 1152 × 648,
    budget 4 000 µs, 10 s per run): a warm-up scroll elsewhere (unmeasured: the first frames that draw
    a new kind of node compile shaders, an 80 ms frame with no streamer work in it here), the first
    window unbounded (map 129 to 141 ms, world 121 to 190 ms, iso 0.9 s, in one frame), still frames,
    then the measured scroll at two screens per second (right 40 %, down 30 %, left 30 % of the time;
    the follow margin covers the screen plus a chunk), then the control: the streamer paused over a
    preloaded region, the camera going back and forth at the same speed for as long. Two runs:
    - **Map**: control median 15.78 / 16.33 ms, p95 20.26 / 21.71, p99 23.07 / 24.20, worst 25.81 /
      36.86; streaming median 19.08 / 17.74, p95 24.46 / 23.52, p99 27.23 / 27.35, worst 37.85 /
      30.46, so over the control +3.31 / +1.40 (median), +4.20 / +1.81 (p95), +4.16 / +3.14 (p99),
      +12.05 / -6.40 (worst). Histogram of frame times over the control's median (≤ +1, +2, +4, +6, +8,
      +12, +16, more): streaming 106, 67, 136, 98, 72, 36, 2, 2 / 254, 67, 112, 81, 32, 10, 5, 0;
      control 410, 80, 87, 34, 5, 4, 0, 0 / 364, 67, 97, 42, 20, 1, 1, 1. Streaming frames less the
      streamer's work in them, over the control: median +0.71 / -0.86, p95 +0.99 / -1.76, p99 +0.53 /
      -0.86: the engine charges nothing beyond what the streamer measures. The worst streaming frames
      had 3.5 to 4.0 ms of streamer work in them (run 1's 37.85 ms frame: 3.91 ms, 940 cells). The
      streamer's work per frame: worst 6.89 / 5.27 ms; longest piece 3.64 / 2.79 ms, most beyond its
      estimate 2.95 / 2.46 ms (12 of about 4 500 pieces in a world run overran by 1.5 ms or more, on
      arbitrary chunks, loads and unloads alike: preemption on this VM, not a pattern). 2 018 / 2 015
      tile chunks loaded (202 per second), 1 980 / 1 973 unloaded, 192 objects spawned, 189 / 187
      freed, 229 521 / 229 425 cells (30 038 / 29 959 with collision); the budget stopped 79 % of the
      working frames (286 of 361, 266 of 338): at this speed and density the streamer works at its
      budget. Measured costs: chunk 54.5 / 78.0 µs + 2.01 / 2.08 µs per cell + 4.80 / 4.51 µs per
      collision cell (a full 256-cell chunk about 0.6 ms, with a fifth of it colliding about 0.8 ms);
      erasing 44 to 51 µs + 0.89 to 1.09 per cell; objects 43 to 45 µs + 73 to 75 per object; freeing
      15 to 16 µs + 20 to 21 per object.
    - **World**: maps load and unload as the camera crosses them (run 2: +world_0_1 +world_0_2
      +world_1_1 +world_1_2 during the warm-up, then -world_0_1 -world_1_1 -world_1_2 -world_0_2
      +world_0_0 +world_1_0 -world_0_0 +world_2_0 -world_1_0 +world_2_1 +world_1_1 -world_2_0
      -world_2_1 +world_0_1). Control median 14.76 / 25.07, p95 19.93 / 38.57, p99 23.39 / 48.22,
      worst 100.22 / 59.46 (a noisy session for run 2); streaming median 18.43 / 24.96, p95 23.83 /
      34.90, p99 26.06 / 43.86, worst 36.38 / 44.05: over the control +3.67 / -0.11, +3.90 / -3.67,
      +2.66 / -4.36, -63.84 / -15.40; less the streamer's work: +1.23 / -3.33, +0.38 / -7.17, -1.20 /
      -8.41. Run 1's worst frame had no streamer work in it. Histograms: streaming 112, 43, 138, 135,
      56, 52, 2, 2 / 222, 21, 63, 25, 39, 20, 4, 5; control 408, 76, 100, 46, 13, 5, 0, 3 / 213, 14,
      55, 16, 37, 26, 17, 8. Streamer work per frame worst 6.77 / 7.18 ms, longest piece 5.94 / 4.71,
      most beyond its estimate 5.58 / 4.16; 2 330 / 2 336 chunks loaded (233 per second); budget
      stops 78 % / 83 %; costs chunk 70.6 / 41.3 µs + 1.92 / 1.99 per cell + 5.14 / 5.19 per collision
      cell.
    - **Iso** (`allow_y_sorted_layers`): still frames 24.94 / 26.32 ms, the control (a screen of
      y-sorted cells, scrolling) median 36.50 / 47.05 ms: drawing y-sorted cells one by one is itself
      dear on software GL. The streamer stays in its budget (work per frame worst 7.59 / 6.40 ms, a
      y-sorted chunk being about 3.7 ms) but cannot keep up at two screens per second: 556 / 579
      chunks loaded (56 to 58 per second, against about 200 needed), the budget hit in every working
      frame, the screen partly empty (hence streaming frames cheaper than the control: median 5.5 ms).
      Costs: 14.52 to 15.21 µs per y-sorted cell, 4.61 to 7.37 per erased one.
    - **The gate** as `bench_stream.gd` checks it (streaming minus control at p99 and at the worst
      frame within the budget plus one chunk, 6 ms): run 1 map p99 pass, worst fail (+12.05: a frame
      with 3.91 ms of streamer work); run 1 world, run 2 map and world, both iso runs: pass. The
      literal reading (the worst streaming frame minus the control's median) fails by 14 to 22 ms,
      but the control's own worst frame exceeds its median by 10 to 85 ms: one idle frame is no
      yardstick on this machine. The earlier version of the benchmark, compared with one idle frame,
      also showed that scrolling itself costs nothing here (preloaded content: still median 15.62 to
      16.12 ms, scrolling 16.03 to 16.66) and that a chunk's deferred engine cost is nil (a chunk set
      per frame with a still camera, on or off screen, with or without collision: frame minus measured
      work minus idle, median -1.8 to +0.5 ms).
    - **Memory**: `TiledMap.load` of the 57.9 MB map takes 247 to 280 ms; `Performance.MEMORY_STATIC`
      30.9 MB before and after (it counts the engine's allocations only, not the Rust heap); the
      process's resident memory 229.4 → 319.6 MB, +90.1 MB, 1.56 times the file (the payloads as
      text, about the file, plus the model's per-chunk structs: about 220 000 chunks). After streaming:
      MEMORY_STATIC 35.5 MB (+4.6), resident +56 MB over the loaded map; after a 30 s run (6 028
      chunks loaded, 5 984 unloaded, 575 objects spawned, 570 freed; frames less the streamer's work
      over the control: median +0.32, p95 +0.53, p99 +0.87 ms; budget stops 90 %) MEMORY_STATIC 38.0
      MB, resident +66 MB. What grows is the engine's: a `TileMapLayer` keeps one object (about
      1.3 KB) per 16 × 16 area it has ever shown, until it is freed (section 5). Streaming a window of
      about 95 live chunks along the map headless for 3 200 steps: objects 3 777 → 8 446 and flat once
      the window crosses ground it has crossed before, MEMORY_STATIC 32.4 → 38.2 MB and flat; the
      same without the object layer. Chunks that leave are gone (their cells erased, their objects
      freed, no orphan nodes: the leak test).
  - **Tests**: `test/test_stream.gd` (headless, run by `test/run_all.sh` and CI): `load_all()` of 22
    fixtures (finite, infinite TMX and JSON, isometric, staggered, hexagonal, templates, object
    references, scenes and masks with their projects, parallax, animations...) equals
    `TiledBuilder.build`'s tree with `always_wrap_root` (every node's path, class, child count,
    position, metadata; every cell's source, atlas coordinates and alternative); a window over
    `infinite.tmx` loads exactly the chunk it covers in both layers and erases it when it moves, the
    hysteresis keeps a chunk within `keep_margin`; objects spawn and despawn with their chunks
    (signals), a reference across chunks is an empty path until its target spawns, then resolves,
    then is empty again when the target goes, `draworder="index"` order kept (`spread.tmx`, written by
    the test); `keep_objects_resident`; a 1 µs budget takes 53 `step()`s (the TileSet's tiles first, a
    gid per step, then one piece per step), `settled` fires once, the costs are measured (headless:
    chunk about 30 µs + 1.3 to 2.0 µs per cell, 31 to 67 µs per object); y-sorted maps refused, then streamed when allowed;
    world mode over `fixtures/world/test.world` (the hub only, then area_0_0 only with the hub's tree
    freed, then `load_all()` with all four); `make_streamer` for a map path (asynchronous) and a world;
    no leaks: 20 rounds of three maps and a world streamed in and out, objects 1654 → 1654, nodes and
    orphan nodes unchanged (the baseline taken after two measured rounds: a few objects are created
    once, on the third use, then stay flat over 60 rounds). `SUMMARY: 86 passed, 0 failed` (three
    runs in a row; an earlier version failed the budget checks once under load: it stepped in a tight
    loop with no frame between steps, so the finite map's decoding thread had not finished within
    its 100 steps, and with frames between steps the streamer's own `_process` worked too, since
    entering the tree turns processing on: the test now turns it off after `add_child`). Three
    Rust unit tests (the fit recovers a linear cost and ignores bad samples, the value table).
  - **Gates, measured**: `test/run_tests.gd` `SUMMARY: 103 passed, 0 failed (new_: 0, bug_: 0, other:
    0) of 103` for `res://runtime` and for `res://addons/yati_native`, `test/run_all.sh` exits 0 for
    both; `test/test_async.gd` `SUMMARY: 43 passed, 0 failed` (3 rounds; 45 with `--rounds=5`, as in
    stage 3), worst frame before the build 1.78 to 1.84 ms; `tools/native_gate.gd`: leaks none
    (objects 1900 → 1900, nodes 1 → 1, orphans 0 → 0 after 20 rounds of 57 maps), the small map 1.00
    ms mean (min 0.91), its async import 12 to 40 frames, wall mean 1.19 ms; the engine floor 111.1 ms
    import against 102.3 ms of `tile_map_data`; `cargo test --release --workspace` 56 passed (53 + 3),
    clippy `-D warnings` and `fmt --check` clean. The facade benchmark: `total_ms` min 160.9 to 198.2
    over five runs (stage 3's library, alternated with this one: min 161.9 to 175.5).
  - **Decisions the plan left open**: the window's two modes are distinct properties (`window` cells,
    or `follow` + `margin` around the followed node's chunk), with a separate hysteresis
    (`keep_margin`) rather than reusing `margin` for both; world mode works in world pixels, its
    window in cells of `world_cell_size`, each map converting to its own cells by its tile size (a
    world's maps may have different tile sizes); `follow` converts through the first tile layer's
    `local_to_map` (layer offsets and isometric projection included); an object's chunk is the cell of
    its `x`, `y` (a tile object's bottom-left corner, as Tiled stores it), not its bounds;
    `keep_objects_resident` keeps objects created with their chunk rather than creating all objects
    at the start, and a world map that leaves still frees its objects; `load_all()` keeps the window
    unbounded until `window` or `follow` is set; an unbounded `step(0)` waits for a finite map's
    decoding thread but not for a map still loading; signals are emitted after the frame's work (a
    handler may call back into the streamer; a `_tiled_import` hook, which runs during it, must not);
    the TileSet's every tile is created up front (the builder creates only the tiles a map uses), so a
    streamed map's TileSet can have more tiles than the one-shot build's; with `no_alternative_tiles`
    off a flipped value's first appearance still creates an alternative tile (a TileSet change, so a
    rebuild of the live layers): documented, not pre-scanned; one `TileMapLayer` per Tiled layer per
    world map (not shared across maps), so a map's unload is its own; the collision cost is the
    engine's own per-cell physics shapes, with no merged-polygon option yet (section 5).

- 2026-10-18, **stage 5 done** but for the web build (documentation, the loose ends, adoption):
  - **Tile layers decoded on the worker** (stage 3's loose end). `tiled_core::prepare`
    (`native/tiled/src/prepare.rs`): `PreparedCells::prepare(map)` turns every tile layer into a
    `PreparedLayer`, a blob in `tile_map_data`'s format whose records hold the cell's x and y and, in
    place of the four tile fields, the index of the cell's value in the layer's distinct values
    (`values`, first-appearance order, the hexagonal rotation bit masked off); `PreparedLayer::patch`
    fills the tile fields in place from the resolutions of those values and drops the cells of
    values that resolve to nothing. `Worker::spawn_preparing` prepares the map on the worker after
    the parse and sends the cells with `Event::Done(map, cells)` (the variant gained the field).
    `TiledMap.prepare_tile_layers` (off by default; `load_async` prepares on the worker, `load` on the
    main thread) and `has_prepared_tile_layers()`; the facade's `import_async` turns it on. The
    builder (`builder/cells.rs`) takes the map's prepared layers or prepares them itself with the same
    code, resolves each distinct value once (`resolve_cell`, unchanged: it creates the tiles in the
    TileSet), copies the blob into a `PackedByteArray`, patches it there and assigns it. *Decision:*
    placeholders patched on the main thread rather than resolving on the worker, because a cell's
    source id, atlas coordinates and the tiles' existence depend on the TileSet the main thread
    builds (source ids as `add_source` assigns them, atlas grids from the loaded textures, strips,
    `godot_atlas_id`), which the worker would have to predict; the patch is one 8-byte write per cell
    and both paths share all of the code, so the blob is the same byte for byte. Layers under 32 768
    cells are prepared on the calling thread (a thread per tiny layer cost 3 ms of wall time on a busy
    machine: the small map's async import went from 1.2 to 4 ms before this rule, and its synchronous
    import is now 0.6 to 0.8 ms against 1.0). Memory: the map holds the prepared layers, 12 bytes
    per non-empty cell (6.3 MB for the benchmark map), until it is freed; a streamer's map is never
    prepared (an infinite map prepared whole would cost 12 bytes per cell of every chunk).
    `TiledBuilder.get_timings()` (`prepare_us`, `cells_us`, `assign_us`) measures the build's tile
    layers on the main thread. The streamer's on-demand chunk decode is unchanged.
  - **Measured** (this machine, Godot 4.7.2, 4 cores shared with another agent's Rust builds:
    frame times vary by ±30 ms; `test/test_async.gd -- --rounds=5`). Before (the stage 4 library
    with the timings added): build frame median 186.2 and 247.0 ms (min 171.8, 178.2) in two runs; the
    build's own tile work decode 4.8 to 7.5 ms plus resolve and blob 6.6 to 10.2 ms (the engine's
    `tile_map_data` 108 to 169 ms). After: build frame median 189.7, 168.7 and 165.6 ms (min 167.7,
    156.5, 155.9) in three runs; the prepared map's build decodes for 0.001 to 0.003 ms and resolves
    and patches for 5.3 to 7.3 ms, of which resolving the 1 536 distinct values is 2.7 ms (almost all
    the engine's `create_tile` and `set_texture_origin` for 192 tiles) and copy plus patch 0.3 to
    0.4 ms per layer; `tile_map_data` 102 to 125 ms. So about 10 ms of main-thread work left the
    build frame, less than this machine's noise in the frame itself; an A/B of the facade in one
    session (prepare on, off, on, off) read 164.5, 191.5, 177.8, 174.0 ms. The worst frame before the
    build stays 1.1 to 1.5 ms; the load takes more frames (2 000 to 4 000 instead of 900 to 2 500:
    the worker decodes too, each frame about 5 µs). The small map: build frame 0.50 to 0.60 ms
    (prepare off, same session: 0.80 to 1.02), async wall mean 0.9 to 1.4 ms. `test_async` checks
    thirteen fixtures (encodings, infinite TMX and JSON, hexagonal, legacy XML, isometric, a missing
    tileset, alternative tiles) and the benchmark map: every `TileMapLayer.tile_map_data` of a
    prepared build (worker and main thread) equals the self-decoding build's. What is left in the
    build frame for tiles is the resolution: creating the map's used tiles during the TileSet
    phase, before the build frame, would take its 2.7 ms out too (a step API on the builder; not
    done).
  - **The benchmark's phases** (stage 3's loose end). `test/benchmark.gd` clears the shared TileSet
    and texture caches before every timed step of the native runtime, so each phase is cold, as for
    the GDScript runtime, and "tile layers" no longer subtracts a TileSet time the tiles-only import
    did not spend; the warm-cache import is reported on its own (`cached import avg`). It also showed
    that `Importer.shared_tilesets()` failed in a `SceneTree` script's `_init` (godot-rust's static
    methods are registered after instance methods: `class_has_method("TiledTileSets", "shared")` is
    false there, `clear` true); `TiledTileSets.get_shared()` is the instance route the facade falls
    back to. Facade, two runs: `total_ms min=172.3 avg=175.3 | parse avg=7.1 | tilesets avg=1.7 |
    tile layers avg=127.4 | objects avg=39.2 | cached import avg=187.6` and `min=163.8 avg=168.0 |
    parse 7.1 | tilesets 1.6 | tile layers 115.7 | objects 43.6 | cached 171.5` (the cache saves the
    TileSet's 1.5 ms, below the noise); the GDScript runtime `min=709.0 avg=728.5 | parse 151.6 |
    tilesets 5.4 | tile layers 414.4 | objects 157.1` (another run min 756.6).
  - **Documentation.** `Runtime.md`, "Native importer" (when to use it, installing, the shared API
    and the two extra options, `import_async`, `import_world`, a follow-mode streamer, the shared
    cache, the differences); `Reference.md`, "Differences between the GDScript and the native
    importer" (every accepted difference of the entries above, with its reason, and the
    streamer's own rules); `README.md`'s section and the two version lines; the addon's README
    rewritten for someone who copies only that directory (features, platforms, installing, the API,
    options, the streamer, the limits, building, the web build, versions). `CHANGELOG.md`: release
    3.1.0 (2026-10-18), the breaking changes first, an empty `[Unreleased]` above. The crates
    `yati_native` and `tiled_core` are 0.2.0; the library is released under `v<crate version>`
    (`v0.2.0`), the repository under `v3.x.y`.
  - **Adoption.** The game that drove the 3.0 work handles its Tiled classes itself and got B8's
    warning once per class on every load: `warn_unknown_classes` (default true) in both importers
    skips that warning and its count, the fallback unchanged; `test_new_warn_unknown_classes_off`.
    The game's switch to `import_async` is its own repository's change.
  - **Web build: not made.** No `emcc` on this machine (not installed, as agreed). What it needs:
    godot-rust's `experimental-wasm` feature with `lazy-function-tables` (and
    `experimental-wasm-nothreads` for Godot's nothreads export); the Emscripten Godot's web templates
    are built with (4.0.11 for 4.7.2-stable, `EM_VERSION` in Godot's `web_builds.yml`), also for the
    `zstd` feature's C code; the target `wasm32-unknown-emscripten` on nightly with `-Zbuild-std`,
    linked as a side module (`-sSIDE_MODULE=2`); `-pthread` and the atomics target features for the
    threads export and neither for the nothreads one; the export's "Extensions Support". Without
    threads, the loader's worker, `PreparedCells::prepare`'s layer threads and the streamer's decode
    thread must run their work in budgeted slices on the main thread (`poll()` stepping the loader a
    round, a layer or a chunk per call): `std::thread::spawn` fails there, and `prepare` would panic
    in `thread::scope` for maps with several large layers. The `.gdextension`'s web entries are left
    as they are; no CI entry.
  - **Gates, measured**: `test/run_tests.gd` `SUMMARY: 104 passed, 0 failed (new_: 0, bug_: 0, other:
    0) of 104` for `res://runtime` and for `res://addons/yati_native`; `test/test_async.gd` `SUMMARY:
    73 passed, 0 failed` (5 rounds; 71 with 3); `test/test_stream.gd` `SUMMARY: 86 passed, 0 failed`;
    `tools/native_gate.gd` leaks none (objects 1900 → 1900, nodes 1 → 1, orphans 0 → 0 after 20
    rounds of 57 maps), the small map 0.66 ms mean synchronously, its async import 18 to 67 frames,
    wall mean 1.05 ms, build frame 0.60 ms, the engine floor 112.4 ms import against 99.4 ms of
    `tile_map_data`; `cargo test --release --workspace` 59 passed (56 + 2 for `prepare` + 1 for the
    preparing worker); clippy `-D warnings` and `fmt --check` clean; `test/run_all.sh` exits 0 for
    both runtimes (its benchmark runs there: the GDScript runtime `min=753.4 avg=790.6`, the facade
    `min=183.8 avg=208.6 | tile layers avg=146.5 | cached import avg=226.3`, a busy moment).

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
