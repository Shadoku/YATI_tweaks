# YATI runtime tests

Headless test suite, fixtures and benchmark for the GDScript runtime importer in `runtime/`.
The repository root is a minimal Godot 4 project (`project.godot`) so that
`res://runtime/Importer.gd` resolves; nothing here is needed to *use* the runtime.

All commands are run from the repository root with a Godot 4.3+ binary called `godot`
(always with `--headless`). In the development container the binary is
`/home/user/tools/godot/godot` (Godot 4.7.2).

## Commands

```sh
# 1. (Re)generate PNGs, encoded maps (csv/base64/zlib/gzip/zstd, infinite), the .tmj map,
#    the .tiled-project file and bundle.zip. The outputs are committed, so this is only
#    needed after changing the generator (bundle.zip embeds timestamps, so it always changes).
cd /path/to/YATI && godot --headless -s test/gen_fixtures.gd

# 2. Run the tests (exit code 0 = all passed, 1 = failures)
cd /path/to/YATI && godot --headless -s test/run_tests.gd

# 3. Benchmark (the large map is generated into test/out/ on the first run)
cd /path/to/YATI && godot --headless -s test/benchmark.gd -- --runtime=res://runtime
```

`test/run_all.sh` runs the tests and the benchmark in one go (`GODOT=/path/to/godot test/run_all.sh`).

```sh
# 4. What the engine itself charges for a map's tile layers (no importer involved): assigning
#    tile_map_data, instantiating a packed layer, the frame after a layer enters the tree,
#    set_cell / erase_cell of 16x16 chunks into a live layer, a layer per 64x64 chunk. Run it
#    with a renderer for the numbers a game sees; headless it skips the rendering quadrants.
#    Options after "--": --size=N --fill=F --no-collision. (docs/native-importer-plan.md)
xvfb-run -a godot --path . --rendering-driver opengl3 --audio-driver Dummy -s test/bench_materialize.gd
```

```sh
# 5. The dictionary the runtime builds for one Tiled file (DictionaryBuilder.get_dictionary),
#    as JSON: the parity target of the native importer's model (native/tiled). With a project
#    file its custom types are merged where the runtime merges them; --zip reads from an archive.
godot --headless -s test/dump_dictionary.gd -- test/fixtures/objects.tmx test/fixtures/custom.tiled-project
godot --headless -s test/dump_dictionary.gd -- maps/zipmap.tmx --zip=test/fixtures/bundle.zip --out=x.json

# 6. Regenerate test/fixtures/expected/*.json (every fixture map, tileset and template dumped
#    with the script above; native/tiled/tests/parity.rs compares against them). Needed after
#    changing a fixture or DictionaryFromXml.gd.
GODOT=/path/to/godot test/gen_expected.sh
```

The dump prints `JSON.stringify(dict, "", true, true)` (keys sorted, floats always with a
decimal point, so `5` and `5.0` stay apart); with `--out` it writes the same indented. Godot's
JSON quirk to keep in mind when reading a dump of a `.tmj`/`.tsj`/`.tj` file: Godot parses every
JSON number as a float, so those dumps hold `16.0` where the file has `16`. Without `--out` the
engine's banner line comes first on stdout.

### Test runner options (after `--`)

| Option | Meaning |
|---|---|
| `--runtime=res://dir` | runtime folder to test (default `res://runtime`) |
| `--filter=text` | only run tests whose name contains `text` |

Output is one line per test (`PASS name` / `FAIL name: reason`) and a `SUMMARY` line.

Test name prefixes:

- `new_...` asserts behaviour added by the 3.0 runtime rework (draworder, blend modes, object
  opacity, bold/italic, class properties as Dictionary, object references into group layers,
  image layer repeat, legacy `<tile gid>` data, parentless return node, zip mode reset,
  Importer option vars and `last_error_count`, transform-flag flips by default, oversized
  image-collection tile origin). These fail against the pre-change runtime.
- `bug_...` asserts documented/intended behaviour the pre-change runtime gets wrong
  (see the comment in each test).
- everything else must pass on both the old and the new runtime.

### Benchmark options (after `--`)

| Option | Meaning |
|---|---|
| `--runtime=res://dir` | runtime folder to benchmark (default `res://runtime`) |
| `--iterations=N` | measured imports after one warm-up (default 3) |
| `--regen` | regenerate the benchmark map |
| `--with-duplicate` | also time `Node.duplicate()` of the result (what the old docs recommended) |

The benchmark only calls `Importer.import()`, so it also works with the pre-change runtime.
It prints per-run and average times for: parse (DataLoader + DictionaryBuilder), tileset
creation (TilesetCreator), tile layer filling (tiles-only companion map minus its parse and
tilesets) and objects (the rest), plus a final `RESULT` line. Timings on a shared machine are
noisy; compare `min` and run a few more iterations when the numbers matter.

## Comparing against the pre-change runtime

A pristine copy of the runtime before the 3.0 rework is kept outside the repository. Copy it
into the (gitignored) `baseline_runtime/` folder and point the scripts at it:

```sh
cd /path/to/YATI
rm -rf baseline_runtime
cp -r /tmp/claude-0/-home-user-YATI-tweaks/a172844b-f0b6-52e9-9c5a-4cbf9ce379fc/scratchpad/baseline/runtime baseline_runtime
# (or: git archive 5719927 runtime | tar -x --transform 's/^runtime/baseline_runtime/')

godot --headless -s test/benchmark.gd -- --runtime=res://baseline_runtime
godot --headless -s test/benchmark.gd -- --runtime=res://runtime
godot --headless -s test/run_tests.gd -- --runtime=res://baseline_runtime
```

## Fixtures (`test/fixtures/`)

| File | Covers |
|---|---|
| `terrain.tsx` + `tiles.png` | external tileset: custom data (`data_` prefix) and metadata, probability, 3-frame animation, collision objectgroup (rectangle, ellipse, polygon, one-way polygon, `physics_layer=1`), wangset with 2 colours, tileset property |
| `tilesets.tmx` | external + embedded tileset (margin, spacing, tileoffset) + image collection (one 32x48 tile on a 16x16 map), every flip combination of one tile |
| `layers.tmx` | opacity, tintcolor, offsets, visible=0, parallax, class, `mode="add"`/`"multiply"`, `no_import`, group layer (tile + object layer), draworder topdown/index, image layers (plain, parallax, repeatx, Parallax2D with repeaty), map property |
| `objects.tmx` + `enemy.tx` | every object shape and class mapping, text (bold/italic/wrap/halign), tile objects (flipped, with collision, with tile properties), opacity, visibility, every property type incl. class and object references (top level and inside a group), `godot_group`, `godot_node_type`, template used 3 times with one override, custom type from `custom.tiled-project` |
| `encodings.tmx` | csv, base64, base64+zlib, base64+gzip, base64+zstd (generated) |
| `infinite.tmx` | infinite map with csv and base64+zlib chunks (generated) |
| `isometric.tmx`, `staggered.tmx`, `hexagonal.tmx` + `iso.tsx` | map orientations |
| `background.tmx` | map background colour |
| `legacy_xml.tmx` | deprecated `<data><tile gid=".."/></data>` encoding |
| `json_map.tmj` | JSON map with embedded tileset and a class-typed property (generated) |
| `custom.tiled-project` | custom class types (`useAs: ["object"]`) (generated) |
| `bundle.zip` | `maps/zipmap.tmx` + tileset + image for `import_from_zip` (generated) |
| `missing_tileset.tmx` | error path: tileset file not found |
| `postproc.gd` | post-processor for `import_with_post_processing` |
| `classes.tmx` + `classes.tiled-project` + `crate.tx` | for the native parity tests: class `Room` map, `Floor` layers, nested groups with template instances (one overriding properties and opacity), list properties (nested lists and class items), class values with nested classes and lists, multi-line strings, enum and object properties, an embedded tileset with class, transformations, transparent colour, tile collision object, wang colour properties, `editorsettings`, a capsule, a text object, a zstd layer; the project has every `useAs` scope the runtime merges, a class defined twice and an enum |
| `infinite.tmj` + `json_tiles.tsj` + `json_enemy.tj` | for the native parity tests: an infinite JSON map (csv-array and base64+zlib chunks, the same cells as `infinite.tmx`), external JSON tileset (tile offset, grid, transformations, animation, collision objects, wang set) and JSON template, nested groups, every property type incl. list and nested class |
| `json_twin.tmx` | `json_map.tmj` written as TMX: both formats must give the same model |
| `expected/*.json` | the GDScript runtime's dictionary of every fixture map, tileset and template (`test/gen_expected.sh`; `+<project>` with the project's custom types merged, `bundle.zip#...` members of the zip) |
