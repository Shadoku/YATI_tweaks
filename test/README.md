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

`test/run_all.sh` runs everything in one go (`GODOT=/path/to/godot test/run_all.sh [--runtime=res://dir]`):
it names the native importer's GDExtension in `.godot/extension_list.cfg` when this platform's library
is in place (Godot loads an extension only when that file names it, and a fresh `.godot/`'s first import
that discovers one mid-scan aborts at shutdown with exit code 134 on Godot 4.7.2), imports the project
(the fixtures' images are imported textures, so a fresh clone needs it), runs the native smoke test,
then the tests and the benchmark. Without the library the smoke test is skipped, unless the runtime
under test is the native one.

```sh
# 4. What the engine itself charges for a map's tile layers (no importer involved): assigning
#    tile_map_data, instantiating a packed layer, the frame after a layer enters the tree,
#    set_cell / erase_cell of 16x16 chunks into a live layer, a layer per 64x64 chunk. Run it
#    with a renderer for the numbers a game sees; headless it skips the rendering quadrants.
#    Options after "--": --size=N --fill=F --no-collision. (docs/native-importer-plan.md)
xvfb-run -a godot --path . --rendering-driver opengl3 --audio-driver Dummy -s test/bench_materialize.gd
```

### Test runner options (after `--`)

| Option | Meaning |
|---|---|
| `--runtime=res://dir` | runtime folder to test (default `res://runtime`; `res://addons/yati_native` is the native importer's facade) |
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

## The native importer (`addons/yati_native/`)

```sh
# Build the library (Rust, native/) into addons/yati_native/bin/, or download a CI build
bash tools/build_native.sh            # --check: only tell whether it is there
bash tools/install_native.sh          # the branch's rolling build, or a tag: v0.1.0

# Smoke test: the extension loads, TiledNative.version() and a round trip through sum()
# (needs .godot/extension_list.cfg naming res://addons/yati_native/yati_native.gdextension)
godot --headless --path . -s tools/smoke_native.gd

# The suite against the native facade. Until stage 2 of docs/native-importer-plan.md every test
# fails (the facade returns null with "not implemented yet"); the run must still reach SUMMARY.
godot --headless -s test/run_tests.gd -- --runtime=res://addons/yati_native
```

The facade has only `Importer.gd`, so the suite's helpers that reach a runtime's `DataLoader.gd`
(zip mode reset) and `CommonUtils.gd` (error count) skip them for it.

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
| `area_gravity.tmx` | B5: Area2D gravity, gravity point, damp properties (a point-gravity area and a directional one) |
| `colors.tmx` | B3/B4: `#AARRGGBB` colours: background, tints on tile/object/group/image layers, text colour; colour-typed `modulate`, `self_modulate`, `debug_color`, Polygon2D `color`, Line2D `default_color`, metadata; a string-typed `modulate`; an embedded tileset with a colour-typed tile `modulate`, custom data and metadata |
| `layer_class.tmx` | B7: tile, object, group and image layers with and without a class |
| `templates.tmx` + `probe_tpl.tx`, `probe_text.tx`, `probe_rect.tx` | B1/B2: template instances that override size, rotation, visibility (tile object and `area` rectangle), their own gid (flipped) and text |
| `unknown_class.tmx` | B8: objects of unknown classes (two `Door` rectangles with a property, a `Door` polygon, a `Lever` ellipse) and one without a class |
