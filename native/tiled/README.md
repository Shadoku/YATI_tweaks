# tiled_core

The Tiled map format in pure Rust, for the native (GDExtension) variant of the YATI runtime
importer (`docs/native-importer-plan.md`, stage 1). No Godot dependency: everything here runs on
any thread and is tested with `cargo test` alone. The GDExtension crate builds nodes and
resources from this crate's model on the main thread.

## What it does

- Parses TMX/TSX/TX (XML, `quick-xml`) and TMJ/TSJ/TJ (JSON, `serde_json`) into **one model**
  that mirrors the Tiled JSON format; the format is told by content (`<` or `{`), the
  extension only breaking a tie.
- Keeps layer and chunk payloads **undecoded** until they are wanted, and decodes every
  encoding: csv, base64 plain, base64 + zlib / gzip / zstd, the deprecated `<tile gid>` list.
- Resolves gids exactly as the GDScript runtime's `resolve_cell` does and writes the byte blobs
  Godot's `TileMapLayer.tile_map_data` takes (finite layers, per chunk, merged).
- Loads a map with its external tilesets, templates (and their tilesets) and a `.tiled-project`
  through a **file-request protocol**: the loader never opens a file, the caller does (in Godot
  through `FileAccess`, since `res://` lives in the .pck). Zip archives are read directly.
- Merges templates into their instances and custom types into the model by the GDScript
  runtime's rules.
- Gives the model back as **the dictionary `DictionaryBuilder.gd` builds** for the same file
  (`Map::to_value()`, a `Value` tree): the parity target, checked against dumps of the GDScript
  runtime for every fixture.

## Modules

| Module | Contents |
|---|---|
| `model/` | `Map`, `Tileset` (+ `Tile`, `Frame`, `Grid`, `TileOffset`, `Transformations`, `WangSet`, `WangColor`, `WangTile`), `Layer` (+ `Chunk`, `Payload`), `Object` (+ `Point`, `Text`), `Property` / `PropertyValue`, `Template`, `Project` / `PropertyType`; `Num` (a number as written: int or float); loader results (`TilesetSlot`, `Instance`) |
| `parse/` | `xml.rs` (recursive descent mirroring `DictionaryFromXml.gd`), `json.rs` (serde), `detect`, `document`, `map`/`tileset`/`template` |
| `data.rs` | payload decoding to `u32` gids (flip flags kept), `Gid` |
| `cells.rs` | `CellResolver` (gid → source id, tileset, local id, atlas coords, flips), `GidLookup`, `BlobWriter`, `tile_map_data`, `chunk_blobs`, `layer_blob`, `merge_blobs`, `layer_blobs(_parallel)` |
| `load.rs` | `Loader`, `Status`, `load_from_fs`; `Loader::take_discovered_images` (images the parse found so far, for loading textures early) |
| `worker.rs` | `Worker`: a `Loader` on a thread of its own, each batch of files answered with one `Event` (`Need { paths, images }`, `Done`, `Failed`), `try_event` never blocks; `running`, `join_detached` (what the native importer's `TiledMap.load_async` runs) |
| `value.rs` | `Value` (Null, Bool, Int, Float, String, Array, Dict) and the model's dictionaries: `Map/Tileset/Template::to_value()`, `to_json()` |
| `world.rs` | Tiled world files: `World::parse` (explicit `maps`, `patterns` as written), `WorldLoader` (patterns resolved through the request protocol: the world file, `WorldRequest::ListDir` for its directory, the matching maps; `from_zip`, `run`, `load_world_from_fs`), `WorldMap`, `WorldPattern`; `Map::tile_rect` / `map_rect` / `pixel_rect` (the runtime's `map_rect` metadata) |
| `path.rs` | `cleanup` (`CommonUtils.cleanup_path` after a `res://`, `user://`, `/` or drive prefix), `resolve`, `base_dir`, `join` |
| `godot_str.rs` | Godot's `is_valid_int` / `is_valid_float` / `to_int` / `to_float`, which the XML typing needs |
| `error.rs` | `Error`, `ErrorKind` |

## The file-request protocol

```rust
use tiled_core::{Loader, Status};

let mut loader = Loader::new("res://maps/level.tmx", Some("res://game.tiled-project"));
let map = loop {
    match loader.step() {
        Status::NeedFiles => {
            for path in loader.needs().to_vec() {
                match read(&path) {                    // the caller's file access
                    Some(bytes) => loader.supply(&path, bytes),
                    None => loader.missing(&path),
                }
            }
        }
        Status::Done(map) => break map,            // non-fatal errors: map.errors()
        Status::Failed(errors) => return Err(errors),
    }
};
```

A map takes two or three rounds: the map and the project, then external tilesets and
templates, then the templates' tilesets. A reference is resolved relative to the file that
makes it and cleaned (`res://a/b/../t.tsx` asks for `res://a/t.tsx`); a `res://`, `user://`,
absolute or drive path is taken as it is. Each file is asked for once, even when the map and a
template share it. `Loader::run(read)` pumps the loop with a closure;
`Loader::from_zip(bytes, "maps/level.tmx", None)` reads an archive's members itself (as written
first, then relative to the referencing file, like `DataLoader.gd`); `load_from_fs` is the
`std::fs` shortcut for tools.

After `Done`: `map.tileset_slots()` / `map.slot_tileset(slot)` (embedded or external, shared
`Arc`), `map.template(reference)`, `map.instantiate(object)` (the template merge of
`TilemapCreator.create_object`), `map.project()`, `map.images()` (every image reference,
resolved, for the Godot side to load; the core never reads images), `map.errors()`.

## World files

```rust
use tiled_core::world::{WorldLoader, WorldRequest, WorldStatus};

let mut loader = WorldLoader::new("res://maps/overworld.world");
let world = loop {
    match loader.step() {
        WorldStatus::NeedFiles => {
            for request in loader.needs().to_vec() {
                match request {
                    WorldRequest::File(path) => match read(&path) {
                        Some(bytes) => loader.supply(&path, bytes),
                        None => loader.missing(&path),
                    },
                    // the names of the files directly in the directory
                    WorldRequest::ListDir(dir) => loader.supply_listing(&dir, list(&dir)),
                }
            }
        }
        WorldStatus::Done(world) => break world,   // world.maps: path + rect (x, y, w, h) px
        WorldStatus::Failed(errors) => return Err(errors),
    }
};
```

Up to three rounds: the world file; the listing of its directory (only with `patterns`); the files of the maps a
pattern lists without `mapWidth` / `mapHeight` (parsed for `Map::map_rect`'s size). The result is the GDScript
`WorldLoader.parse()`'s: explicit maps first, then the pattern matches by file name. The loader does not cache map
sizes (the GDScript one does): a streamer keeps the `World`.

## Tests

```sh
cd native/tiled
cargo test                      # unit tests, parity, payloads, loader
cargo test --no-default-features   # without the zstd C library
cargo clippy --all-targets -- -D warnings && cargo fmt --check
cargo run --release --example bench   # needs test/out/bench/ (test/benchmark.gd makes it)
```

`tests/parity.rs` loads every fixture map, tileset and template of `test/fixtures/` through the
loader and compares `to_value()` with `test/fixtures/expected/*.json`: same keys, ints exact,
floats approximately, arrays in order. The dumps come from the GDScript runtime itself:

```sh
GODOT=/path/to/godot test/gen_expected.sh       # all of them
godot --headless -s test/dump_dictionary.gd -- test/fixtures/objects.tmx test/fixtures/custom.tiled-project --out=x.json
```

Regenerate them after changing a fixture or the GDScript runtime's `DictionaryFromXml.gd`.
The benchmark map's parity is an ignored test (`cargo test --release -- --ignored bench_map`,
after dumping `test/out/bench/*.json` as its doc comment says).

## The dictionary, and where the model differs from it

The model types what Tiled means (`visible` a bool, a gid a `u32`, a class value a map of
members), and `to_value()` gives the GDScript runtime's spelling back: for XML, attributes typed
by `insert_attributes` (`name="10"` is the int 10, `repeatx="1"` the int 1, a bool property's
value the string `"true"`), csv payloads as int arrays, `<transformations>` as a one-element
`transformationss` array; for JSON, every number a float (Godot's JSON parser). Keys the model
has no field for are kept per struct in `extra`, typed the same way, unknown XML elements as
`DictionaryFromXml.gd` stores them (`<editorsettings>` → `editorsettingss`). A number keeps
whether it was written as an int (`Num`), so `16` and `16.0` stay apart where the runtime keeps
them apart.

Not reproduced (nothing Tiled writes): text content inside `<item>` and unknown elements (the
runtime stores it as a `data` key), an embedded `<image><data>` (the runtime garbles the
tileset; it is skipped), several `<transformations>` in one tileset (the last is kept), JSON
`null` in a typed field (dropped), a non-integral number in an integer JSON field (an error).

## Not in this crate

Anything that touches Godot: TileSet and TileData creation, textures, nodes, metadata,
`TileMapLayer.tile_map_data` assignment, the streamer, the asynchronous `poll()` driver. Those
are the GDExtension crate's (stages 2 to 4 of the plan); this crate gives them the model, the
blobs and the list of files and images.
