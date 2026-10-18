# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Native importer, stage 3 of `docs/native-importer-plan.md`: asynchronous loading and shared
  caches. `Importer.import_async(path, project = "")` and `import_from_zip_async(...)` (coroutines:
  `await` them) import a map without a hitch: the parse runs on a worker thread
  (`TiledMap.load_async` / `from_zip_async`, `poll()` once per frame returning `READING`,
  `PARSING`, `WAITING_FILES`, `READY` or `FAILED`, signals `loaded` and `failed`, `cancel()`), the
  main thread only reads the files the parse asks for (`FileAccess`, so `res://` in a .pck works);
  the map's `res://` textures load in the engine's threads (`ResourceLoader.load_threaded_request`)
  while it parses; TileSets are created a few tiles per frame (`TiledTileSets.build_step`) and the
  tree is built in one frame. On the benchmark map no frame before the build takes more than about
  2 to 3 ms of main-thread time, and the build frame equals the synchronous build. TileSets are
  shared across imports (`Importer.share_tilesets`, on by default, through `Importer.shared_tilesets()`,
  an exclusive cache: two live trees never share a TileSet, a later map reuses a freed tree's;
  `TiledTileSets.exclusive` off shares one by reference between live trees too), so a second map on
  the same tilesets creates no TileSet; textures are cached per process
  (`TiledTileSets.clear_textures()`). The cache key now also holds `no_alternative_tiles` and the
  project file's types. `TiledTileSets.get_created_count()`, `get_texture_count()`;
  `TiledMap.get_status()`. A cached TileSet's messages are reported by every import that uses it.
- `test/test_async.gd` (run by `test/run_all.sh` with the native library): the async import's
  frame times and tree, the shared cache, cancellation without leaks; `tools/native_gate.gd` times
  the small map's async import.
- Native importer, stage 2 of `docs/native-importer-plan.md`: maps import natively.
  `addons/yati_native/Importer.gd` (the GDScript runtime's API) now imports through the native
  classes `TiledImportOptions`, `TiledMap` (load from the file system or a zip, queries,
  `to_dictionary()`), `TiledTileSets` (the TileSet builder and its cache) and `TiledBuilder` (the
  whole node tree in one call, tile layers filled with one `tile_map_data` assignment each), and
  passes the runtime's test suite (90 of 90). About 4 to 5 times faster than the GDScript runtime
  on the benchmark map, a small map in about 2 ms. The addon stands alone (`PostProcessing.gd` and
  a `CommonUtils.gd` with the error counters next to `Importer.gd`); its README lists the few
  differences from the GDScript runtime. Crate feature `unchecked` (off by default): godot-rust's
  per-call checks off, about 30 % off the object phase.
- `tools/native_gate.gd`: leak check (every fixture imported and freed 20 times, object and node
  counts unchanged), small-map timing and the engine's `tile_map_data` floor; `tools/smoke_native.gd`
  imports a fixture through the facade too.
- `test/benchmark.gd` times a runtime without `DataLoader.gd` (the native facade) with the native
  classes, so its phase split works for both importers.
- The native parity tests cover the runtime improvement plan's part A fixtures and their templates
  (`test/fixtures/expected/` has their dumps).
- Native importer, stage 0 of `docs/native-importer-plan.md` (the scaffold, it imports nothing yet):
  the Rust crate `native/` (`yati_native`, godot-rust 0.5 for Godot 4.7) registering `TiledNative`
  (`version()`, `sum()`); `addons/yati_native/` with `yati_native.gdextension` and an `Importer.gd`
  facade that has the GDScript runtime's options and methods and reports "not implemented yet"; the
  tools `tools/build_native.sh`, `tools/install_native.sh` (CI builds, `.yati-native-version`),
  `tools/install_godot.sh` (`.godot-version`), `tools/smoke_native.gd` (loads the library),
  `tools/check_native_image.py` and `tools/verify_native_bin.sh`; the workflows `ci.yml` (builds the
  library, runs the suite for both runtimes, the native one for information until stage 2) and
  `native.yml` (Linux x86_64, Windows x86_64 and macOS libraries, committed back to the branch and
  published as the rolling pre-release `native-<branch>` or a release on a `v<crate version>` tag).
- `test/run_all.sh` runs the native smoke test first, and imports the project (seeding
  `.godot/extension_list.cfg` so a fresh `.godot/` does not crash on its first import), so it works
  from a fresh clone.
- `docs/native-importer-plan.md`: the exploration and plan for a native (Rust GDExtension) importer
  that needs no preloading and streams infinite maps, with the measurements behind it.
- `native/tiled/` (`tiled_core`, stage 1 of the native importer plan): a pure-Rust crate, no Godot
  dependency, with the Tiled data model (structs mirroring the JSON format), TMX/TSX/TX and
  TMJ/TSJ/TJ parsers into that one model, payload decoding in every encoding (csv, base64, zlib,
  gzip, zstd, legacy `<tile gid>`) kept lazy, gid resolution as `resolve_cell` does it, Godot
  `tile_map_data` blobs (per layer, per chunk, merged, one thread per layer), a loader with a
  file-request protocol (external tilesets, templates and their tilesets, the project file, zip
  archives), the template and custom-type merges, and `Map::to_value()`: the dictionary
  `DictionaryBuilder.gd` builds, equal to the GDScript runtime's for every fixture.
- `test/dump_dictionary.gd`: prints the runtime's dictionary of a Tiled file as JSON (optionally with a
  project's custom types merged, or from a zip); `test/gen_expected.sh` writes
  `test/fixtures/expected/*.json` with it for the native parity tests.
- Fixtures for the native parity tests: `classes.tmx` + `classes.tiled-project` + `crate.tx`,
  `infinite.tmj` + `json_tiles.tsj` + `json_enemy.tj`, `json_twin.tmx`.
- `test/bench_materialize.gd`: measures the engine's own cost of a map's tile layers (`tile_map_data`
  assignment, packed-scene instantiation, the frame after a layer enters the tree, `set_cell` /
  `erase_cell` of 16x16 chunks into a live layer, a layer per chunk).
- B8: importer option `unknown_class_fallback` (default `"staticbody"`, as before): what an object of a
  class that is no Godot node type becomes; `"node2d"` gives a plain Node2D with the shape as metadata
  `shape` (`Rect2` or `PackedVector2Array`) and `size`, any class name of the mapping table that node.
- B6: importer option `parallax_mode`: `"parallax2d"` (the default) wraps a layer with a parallax factor
  in a `Parallax2D` at its place in the hierarchy, its `scroll_offset` derived from the map's parallax
  origin (`parallaxoriginx` / `parallaxoriginy`, unused until now) and factors of nested parallax groups
  multiplied as in Tiled; `"legacy"` keeps the ParallaxBackground of 3.0.
- B9: importer option `animation_mode`: `"strip"` (the default) composes each animated tile's frames,
  in the animation's order, into an atlas source of its own (any frame sequence, image collection tiles
  too) carrying the tile's data; `"atlas"` animates in the tileset's atlas as before. Tile objects of
  animated tiles become an `AnimatedSprite2D` (looping animation `"default"`, autoplay) in both modes.
  (The plan's per-(tileset, tile) strip cache belongs to F4's tileset cache, which the GDScript runtime
  does not get; each import composes its strips once.)
- B10a: importer option `always_wrap_root` (default `false`): a map with a single layer returns a
  `Node2D` root holding the map's properties, the layer as its child, instead of the layer itself with the
  map's properties applied over its own. Recommended; planned as the default of 4.0.
- F2: importer option `object_refs` (default `"nodepath"`): an object property becomes a `NodePath` from its node
  to the node created for the target object, resolved after every layer is built (`TilemapCreator.register_object_node`
  records each object's node, `resolve_object_refs` is the second pass); a property of the same name on the node
  takes it (a `NodePath` export the path, a `Node` export the node); a list of objects becomes an
  `Array[NodePath]`; an unset or dangling reference an empty NodePath with a warning. `"geometry"` keeps the target's
  points (3.0), `"both"` writes the NodePath and the points as metadata `<name>_shape`. The Reference documents
  a `MapRef { map, object }` class convention for references into other maps.
- F1: importer options `class_scenes` (default `{}`: Tiled class, case-insensitive, -> scene path),
  `read_godot_scene_from_custom_types` (default `true`: a custom class's `godot_scene` file member, relative to the
  project file, does the same) and `assign_node_properties` (default `true`): objects and tile objects of a mapped
  class become instances of the scene without a `res_path` on each (after a `godot_node_type` property, before the
  class mapping and `unknown_class_fallback`), and a custom property named like a property of the created node
  (an `@export var`, an engine property) is assigned to it, converted to its type (int to float or an enum, a
  string to an enum by name, StringName or NodePath, a file to its resource, a list to a typed array), else it
  stays metadata with a warning. `godot_script` is attached before the other properties, so the script's variables
  exist. A node method `_tiled_import(info)` is called once the map is built, with the object's id, name, class,
  typed properties, rect and shape. `CustomTypes.get_class_member()` reads a member of a custom class;
  `TilemapCreator.instantiate_scene()` is the one place scenes are instantiated.
- F3: every imported root (a single-layer one too) has metadata `map_rect` (the map's bounds in pixels: Tiled's
  map size per orientation, an infinite map's union of chunks), `map_size_tiles`, `tile_size` and `orientation`;
  `TilemapCreator.get_map_rect()`. `runtime/WorldLoader.gd`: `WorldLoader.parse(world_file, zip_file = "")` reads a
  Tiled `.world` file into `{path, rect}` entries (explicit `maps`, and `patterns` matched against the world
  directory's `.tmx`/`.tmj` files, sizes from the maps' headers, cached), from the file system or a zip;
  `Importer.import_world(world_file, project_file = "")` imports a whole world under one Node2D.
  `DataLoader.list_files(dir)` lists a directory in either mode.
- F3, native: `tiled_core::world` (`native/tiled/src/world.rs`): `World::parse` for a world file's own content,
  `WorldLoader` resolving its patterns through the file-request protocol (the world file, a `ListDir` request for
  its directory, the matching maps' files; `WorldLoader::from_zip`, `load_world_from_fs`), and `Map::map_rect()` /
  `tile_rect()` / `pixel_rect()` with the GDScript runtime's formula. `tiled_core` depends on `regex` for the patterns.
- F5: layer-mask properties (`collision_layer`, `collision_mask`, `platform_floor_layers`, `platform_wall_layers`,
  `navigation_layers`, `light_mask`, `visibility_layer`, `occluder_light_mask`, `parsed_collision_mask`, and the
  tileset's `collision_layer_n`, `collision_mask_n`, `layers_n`, `light_mask_n`) accept an `int`, the bitmask itself
  (a Tiled flags enum stored as int), and the value names of a flags enum stored as string (`Player,Enemy`), looked up
  in the project file (`CustomTypes.get_enum_mask`, `CommonUtils.get_mask_value`); the `"1,3,5-7"` strings work as before.

### Changed
- F1: with the new default `assign_node_properties = true` a custom property named like a property of the created
  node sets that property instead of becoming metadata (e.g. a `rotation` float on an object, a `hp` int on an
  instance whose script exports `hp`); set it `false` for the 3.0 result. B8's unknown-class warning names
  `class_scenes` as a remedy.
- F2: with the new default `object_refs = "nodepath"` an object property is a `NodePath` instead of the
  target's geometry (`PackedVector2Array` / `Vector2`); set `object_refs = "geometry"` for the 3.0 values. A text
  object's Label now gets metadata `id` with `add_id_as_metadata`, as every other object's node does.
- CI runs the test suite against the native importer's facade as a required step, with
  `tools/native_gate.gd` and the benchmark, and tests, lints and formats both Rust crates.
- `test/run_tests.gd` checks that a runtime has `DataLoader.gd` / `CommonUtils.gd` before loading them,
  so the native facade (only an `Importer.gd`) runs the suite without a load error per test.
- **Breaking** (B4): colour-typed properties that become metadata (on nodes, tiles, the TileSet, in
  lists) and tile custom data are a `Color` instead of a `#RRGGBBAA` String. Code that passes the
  value to `Color()` keeps working; code that treats it as a String does not. Class members and list
  items inside class-typed properties keep Tiled's `#AARRGGBB` String (the parsed dictionary is unchanged).
- **Breaking** (B7): a tile layer without a class no longer has metadata `class` (it was always
  `"tilelayer"`); object, group and image layers with a class now get it as tile layers do
  (with `add_class_as_metadata`, the default).
- B8: an object of an unknown class is reported with a warning once per class name and import,
  whatever `add_class_as_metadata` says (it was silent with the option on, the default, so a `Door`
  became a solid StaticBody2D unnoticed); the warning names the fallback used.
- B6: with the new default `parallax_mode = "parallax2d"` a map with parallax layers gives a different
  tree: no `<map> (PBG)` ParallaxBackground and `(PL)` ParallaxLayers, but a `<layer> (PX)` Parallax2D
  around each parallax layer where the layer is, and the background colour stays a child of the root.
  Set `parallax_mode = "legacy"` for the 3.0 tree. Layers of class `parallax` take the parallax origin
  and their offset into `scroll_offset`.
- B9: with the new default `animation_mode = "strip"` an animated tile's cells use a strip atlas source
  (source ids after the tilesets' own, atlas coords `(0, 0)`) instead of the tile in the tileset's atlas,
  which keeps no animation; `animation_mode = "atlas"` gives the 3.0 result.
- B10c-e: `test_clip_artifacts_xml_and_json` keeps `clip_artifacts` working from .tmx (string) and .tmj
  (bool); the Reference says that `__` property forwarding to children works on collision objects only,
  and why the hexagonal 120° rotation flag stays unsupported.

### Fixed
- B5 (`docs/runtime-improvement-plan.md`): the Area2D gravity properties were misspelt in
  `TilemapCreator.handle_properties` (`gravitiy...`): `gravity` and `gravity_point_unit_distance`
  stopped the import of an object's remaining properties with a script error,
  `gravity_point_center_x/y` and `gravity_direction_x/y` became metadata. The Reference listed
  `gravity` as the custom property of Linear Damp and Angular Damp; it is `linear_damp` / `angular_damp`.
- B3: Tiled colours (`#AARRGGBB`, alpha first) were read as Godot's `#RRGGBBAA`, so a layer tint
  `#80ff0000` (red at 50 %) became yellow-green and a background colour `#80ff0000` transparent.
  Layer tints (tile, object, group and image layers), the map background colour, text object colours
  and wangcolors now go through the new `CommonUtils.tiled_color()`; a layer's opacity multiplies the
  tint's alpha.
- B4: colour-typed properties (`type="color"`) drove no colour property: `modulate`, `self_modulate`,
  `debug_color`, `color` (Polygon2D), `default_color` (Line2D) and a tile's `modulate` matched only
  `type="string"`. Both types are accepted now, a `color` read as Tiled's `#AARRGGBB` and a `string` as
  before (Godot's `#RRGGBB` / `#RRGGBBAA` or a colour name). A colour-typed tile custom data property
  on its `TYPE_COLOR` layer got a String.
- B7: every tile layer without a class got metadata `class = "tilelayer"`: the class fell back to
  the `type` key, which pre-1.9 files use for an object's or tile's class but which on a layer is its
  kind. The fallback (`TilemapCreator.get_class_string()`) now applies to objects and tiles only.
- B2: a template instance that saves its own `gid` (one flipped in Tiled) or its own `text` got a
  second node besides the template's (a `_tile` Sprite2D at scale 0.00001, or a second Label), and the
  template's node ignored the override. The instance's gid (a gid of the map's tilesets) and text
  (merged over the template's) now go into the template's object, and nothing else is created.
- B1: a template instance's own `width`, `height`, `rotation` and `visible` were ignored (only `id`,
  `class`, `type`, `name` and `opacity` were taken over): a 32x32 instance of a 16x16 template, rotated
  45°, imported as 16x16 at 0°. Every attribute saved on the instance now overrides the template's, as
  the TMX spec says.
- B6: a layer with a parallax factor was pulled out of its group into one ParallaxBackground (a
  CanvasLayer at -100): its groups' offset, opacity and visibility were lost, a foreground factor > 1
  drew behind everything, and the map background colour was moved along (fixed by the new default
  `parallax_mode`).
- B9: tile animations: frame 0 was taken to be the animated tile itself (tile 0 with frames 1, 2 showed
  0 -> 1); a sequence the atlas can't show (ping-pong 8, 9, 10, 9) printed "Skipped" after the first
  frames had been set up, leaving a partial animation; an animation without room for all frames was cut
  short; image collection animations did not work; tile objects of animated tiles were static Sprite2Ds.
  In `"atlas"` mode the sequence is now validated before the atlas is touched, and an animation that does
  not start with its tile, or belongs to an image collection, falls back to a strip with a warning.
- B10b: file properties of template objects (`godot_script`, `material`, `physics_material_override`,
  `navigation_polygon`, `tile_set`) were resolved relative to the map instead of the template (.tx) file
  (only `res_path` was); `handle_properties()` takes the base directory now. A file property that the
  instance overrides is resolved relative to the map, where Tiled saves it (an overridden `res_path` was
  joined with the template's directory).

## [3.0.0] — runtime-only fork (2026-10-17)

This repository now contains only the GDScript runtime importer (`runtime/`) of
[YATI](https://github.com/Kiamo2/YATI). The editor import plugin and all C# variants were removed.

### Changed (breaking)
- `TilemapCreator.create()` / `Importer.import*()` return a node **without parent**; the internal wrapper node
  (and an unused ParallaxBackground) are freed. Add the result directly, `duplicate()` is no longer needed
  (and no longer recommended: it doubled the import work).
- Import options are plain variables on the `Importer` instance (`use_default_filter`, `add_class_as_metadata`,
  `add_id_as_metadata`, `no_alternative_tiles`, `map_wangset_to_terrain`, `custom_data_prefix`, `save_tileset_to`)
  instead of commented-out lines in `Importer.gd`.
- `no_alternative_tiles` now defaults to `true` (engine transform flags instead of generated alternative tiles).
- Class-typed properties (Tiled 1.8 nested properties) become `Dictionary` metadata `{member_name: value}`
  (nested classes as nested Dictionaries) instead of a stringified dictionary; .tmx and .tmj give the same result.
- `PostProcessing.gd` is no longer a `@tool` script and accepts/returns any `Node`.
- The Godot 4.2 compatibility code path for alternative tiles was removed (minimum stays Godot 4.3).

### Fixed
- Ported the upstream 2.2.8 fix: the texture origin correction for tiles larger than the map tile size now also
  applies to image collection tiles.
- Zip mode was sticky: after one `import_from_zip()` every later `import()` read from the zip.
  The zip archive is now scoped to a single import call.
- Error/warning counters were never reset and never reported. They are reset per import and exposed as
  `Importer.last_error_count` / `last_warning_count`; a one-line summary is printed after each import.
- Missing map / project files are reported with a clear error before the import starts.
- Exponential property duplication when merging template metadata into object properties.
- Object references (`object` properties) to objects inside group layers resolved to `null`.
- Image collection tiles whose image cannot be loaded no longer abort the tileset creation.
- XML parser: properties nesting level was not decremented at top level (latent bug).
- `<tileoffset>` was ignored for same-size tiles that are created on the fly while filling a layer.
- `parallaxx`/`parallaxy` defaulted to 0 instead of 1 on the ParallaxLayer path when only one of them was set.
- Tile properties carrying the custom data prefix were also stored as tile metadata under the stripped name.
- Rectangle, polygon and polyline objects of class `characterbody` / `rigidbody` were silently dropped;
  they now become CharacterBody2D / RigidBody2D with the matching collision shapes.
- Tile objects with collisions leaked 4 to 5 nodes each (eagerly constructed body candidates); the empty
  wrapper node was never freed.
- Numeric object names, `__`-forwarded list properties and `one_way_collision_margin` no longer cause script errors.

### Added (Tiled feature parity)
- Object layer `draworder`: `topdown` (Tiled default) enables y-sorting on the object layer node, `index` keeps the order.
- Layer blend modes `add` and `multiply` (Tiled 1.12) via `CanvasItemMaterial`; on group layers the material is
  propagated to all descendants via `use_parent_material`; other modes print a warning.
- Object opacity is applied as `modulate.a`.
- Text objects: bold and italic.
- `repeatx` / `repeaty` on plain (non-parallax) image layers (tiled TextureRect over the map area).
- Deprecated XML tile layer data (`<tile gid="..."/>` children without encoding) is imported.
- Tileset class is stored as metadata `class` on the generated TileSet.
- Class-typed tile and tileset properties as Dictionary metadata / tile custom data (`TYPE_DICTIONARY` layer).
- Unsupported Tiled features (renderorder, oblique maps, hexsidelength, tilerendersize/fillmode, transformations,
  wangcolor probability, wangset properties, parallax origin, other blend modes, text underline/strikeout/kerning,
  hex 120° rotation, embedded images) are documented in the Reference.

### Performance
- Per-cell gid resolution uses a per-import cache (gid -> source, atlas coords, offset, material, ...) instead of
  several linear scans of the atlas source list per cell; created tiles/alternatives are tracked locally.
- Zip archives are opened once per import instead of once per file.
- Textures are cached per import (image collections, templates, image layers referencing the same file).
- Template files are read, parsed and turned into a TileSet once per import instead of once per instance.
- CSV tile data parsed with `String.split_floats()`; fewer array copies when decoding layer data.
- Polygon points split once; object lookup by id through a lazily built index (incl. group layers).
- Custom types indexed by (class name, scope) once; member merging uses a name set.
- Lookup tables (peering bits, terrain modes, custom data types, Godot node types, property types, ...) are
  constants instead of being rebuilt per call; the Godot version is read once.
- Fewer property scans per object/tile (one pass for special properties, one `to_lower()` per property).

### Removed
- Editor import plugin, C# runtime and C# plugin, `Reference_4.2.x.md`.

### Internal
- Consistent tab indentation in all scripts.
- Headless test suite and benchmark under `test/` (run with Godot `--headless`).

## [2.2.10] - 2026-10-05

### Refactoring notes (no change in functionalitiy and only C# version is affected)
- Merged pull request #108 to fix CA1305 and CA1310 warnings
- More C# code cleaning done (redundant parentheses etc.)
- Some statements now require .NET 10.0 (if you struggle with that better stay at v2.2.9 for the time being)

## [2.2.9] - 2026-10-04

### Added
- Support for transparent color on tilesets in Tiled 

## [2.2.8] - 2026-10-02

### Fixed
- Fixed issues #99, #100, #103, #107
- Improved internal consistency of list properties handling (unlikely that anyone will be significantly affected) 

## [2.2.7] - 2026-03-19

### Fixed
- Fixed issue #93 (Thank you Ricky!)
- C# only: Ellipse approximation (introduced in 2.2.6) was slipped up (my bad). Fixed now
- Fixed wrong placement of rectangle and ellipse collision shapes if rotated (seems no one ever noticed)

### Added
- Support for list properties introduced with Tiled 1.12
- Object type properties referencing polygons, polylines, rectangles or points are now created as  
Packed Array of Vector2 metadata (i.e. containing the polygon/polyline/rect coords)  
or in case of point as Vector2 metadata (i.e. containing the point coord)
- Runtime versions: The importer script was extended for easier customization regarding  
editor options (including the post-processing option)

## [2.2.6] - 2026-03-08

### Fixed
- GDScript version only: Fixed wrong default values for scroll_scale  
  (Could affect group and object layers qualified as class 'parallax')

### Added
- Set motion_mirroring on ParallaxLayers based on Image layer's repeatx/y setting
  was added by 'pmaerksch', thank you Philip!  
  (Please note: ParallaxLayer is deprecated since Godot version 4.4 but still working)
- Image layers can now also be qualified with class or godot_node_type ‘parallax’.  
  The TextureRect is then placed as a child under a Parallax2D node.

## [2.2.5] - 2026-03-04

### Fixed
- Fixed potential misplacement of tile collision shapes in case of image collection tilesets
- Fixed issue #87

### Added
- Added polygonal approximation of ellipse and capsule collision shapes on tiles  
  Ellipse was done by 'pmaerksch', thank you Philip!

## [2.2.4] - 2025-12-03

### Fixed
- Fixed issue when using a tileset offset (luckily rarely or never used)

### Added
- Added support for the new capsule object / collision shape coming with Tiled 1.12

## [2.2.3] - 2025-11-26

### Fixed
- Fixed issue with misaligned tiles when using multi-image tilesets

## [2.2.2] - 2025-09-18

### Fixed
- Fixed issue #80

## [2.2.1] - 2025-09-16

### Fixed
- Fixed issue #79

## [2.2.0] - 2025-08-20

### Fixed
- Fixed issue #75
- Fixed issue #76

### Added

- Added support for the new Godot Parallax2D node.  
Tiled group or object layers qualified as class 'parallax' will be imported as Godot Parallax2D nodes.  
\+ Godot's Parallax2D node properties can be set by custom properties.

## [2.1.12] - 2025-07-04

### Fixed
- Fixed issue #73

## [2.1.11] - 2025-04-15

### Added

- Added support for 'AnimatableBody2D' (very similar to 'StaticBody2D')

### Fixed

- Added some missing tileset properties (probably rarely used, if at all)
- Fixed issue #70 (get rid of obsolete warnings since Godot 4.4 concerning OccluderPolygons)

## [2.1.10] - 2025-03-29

### Added

- Added handling of Navigation Polygon properties due to specific need

## [2.1.9] - 2025-02-21

### Fixed

- Starting with version 2.1.4 I had screwed up instance tile placements.  
(Fortunately, almost no one has stumbled across it...)  
This is now fixed.

### Added

- In addition to instance property 'res_path' now a property 'res_alignment' can be used to correct misalignments.  
Misalignments can occur if the alignment of an instance is different to its representing (Tiled) tile.  

## [2.1.8] - 2025-02-17

### Fixed
- Fixed issue #68

### Changed
- As of Godot v4.4, the JSON parser no longer distinguishes between integer and float and always returns float.  
C#'s int.Parse does not like this, which has been taken into account with a corresponding change. 

## [2.1.7] - 2025-01-01

### Fixed
- Fixed issue #67

## [2.1.6] - 2024-12-23

### Fixed
- Fixed issue with nested groups (.tmx only)

## [2.1.5] - 2024-12-20

### Refactoring notes (no change in functionalitiy)

- All data loading from Tiled files is now amalgamated into a new module 'DataLoader'  
This is especially beneficial for runtime version users to allow for easier customizing of data loading.  
E.g. loading from Zip file basically only needs one additional line somewhere ahead from the TilsetCreator.create() call:  
```py
DataLoader.zip_file = "<path-to-zip>/xyz.zip"
```

## [2.1.4] - 2024-12-16

### Fixed

- Fixed issue if a tile's image source is a Tiled map (.tmx or .tmj)  
**Please note:** To make full use of this i.e. that it's looking like in Tiled, instanting the .tmx / .tmj (or it's saved .tscn) is additionally needed. \+ The referenced .tmx / .tmj should already have been imported.

### Refactoring notes (no change in functionalitiy)

- Moved some common code into a new module 'CommonUtils'

## [2.1.3] - 2024-12-08

### Added
- If option 'SaveTilesetTo' is used the saved tileset will automatcally be referenced in all TileMapLayer's tilesets 
- Also evaluate a Tiled custom property 'tile_set' (type 'file') on a (Tiled) tile layer - this was missing.  
This allows to force the use of a specific Godot tileset resource on the corresponding TileMapLayer.

## [2.1.2] - 2024-12-03

### Added
- Ability for the Godot 4.3 runtime version to load a Tiled project from a Zip file.  
*Note: This affects the runtime only, the addon version remains at 2.1.1*

## [2.1.1 / 1.7.1] - 2024-11-13

### Added
- Like 'instance' now also propagate 'XXXbody' tile class types to tilemap objects

### Fixed
- Fixed issue #63

## [2.1.0 / 1.7.0] - 2024-10-21

### Added
- Added new string option 'Custom Data Prefix' (Default value: 'data_') to make it possible to explicitly assign  
the custom property of a tile to Godot's Tile Custom Data.  
Properties with a name not starting with that prefix are now assigned only to the tile's meta data.  
(If neccessary, set the option to an empty string to generate the previous behavior)

### Fixed
- Fixed problem with tile objects being slightly off with tileset images having separation and/or margins.
- Fixed problems resulting from unreliable tileset entries (tilecount / columns).  
  (Both are now calculated from the width and height of the tileset image)
- Fixed need for exporting with option 'Resolve object types and properties'.  
Save / Save all should now being sufficient.
- Fixed problem with missing meta data on alternative tiles (merged PR #60, thank you zhtsu)
  

## [2.0.3 / 1.6.10] - 2024-09-30

### Fixed
- Fixed issue #56

## [2.0.2 / 1.6.9] - 2024-09-18

### Fixed
- Fixed issue #46
- Fixed issue #53

## [2.0.1] - 2024-09-09

### Fixed
- Fixed issue #52

## [2.0.0] - 2024-08-19

### Added

- Added support for the new TileMapLayer in Godot 4.3  
  This required incompatible changes i.e. version 2.0.0 will not work with Godot 4.2.x or older

## [1.6.8] - 2024-08-18

### Added

- Added support for 'godot_atlas_id' custom property which can be used to force specific atlas ids.
  This was implemented on behalf of issue #48.

### Fixed

- Merged Nebelun's pull request which fixes an (GDScript only) issue concerning custom types loading.  
  Thank you Nebelun!
- Fixed issue #49
- Fixed issue #50

## [1.6.7] - 2024-03-26

### Fixed

- Merged milana-94888u's pull request which fixes an (GDScript only) issue if a tileset name is a number.  
  Thank you Milana!
- Fixed a rare issue if tile collision rects had zero height or zero width

## [1.6.6] - 2024-01-31

### Added

- Added support for `godot_script` custom property to set a script on to a node  
  This was done by 'atbigelow', thank you August!

## [1.6.5] - 2024-01-27

### Added

- Added new option 'no_alternative_tiles'.  
  If enabled any flipped/rotated tiles are no longer created as alternative tiles.  
  An enhancement in Godot 4.2 makes this possible.  
  *Please note: Rotated non-sqare tiles may appear wrongly placed if this option is `On`.  
  This currently can't be avoided due to Godot specifics.*

### Fixed

- Fixed issue #37 and merged Lertsenem's pull request (issue #38, his GDScript fix for #37).

## [1.6.4] - 2023-12-27

### Added

- In Godot 4.2 TileMap property 'cell_quadrant_size' was renamed to 'rendering_quadrant_size'.  
The list of custom properies was updated accordingly and the new name can be used.

### Fixed

- Merged mark-rafter's pull request (issue #36).   
  This fixes an issue if collision polygons on tiles are created as polylines

## [1.6.3] - 2023-12-09

### Fixed

- Merged lmReef's pull request (issue #33) and added the C# version.   
This fixes an issue with using different tilesets that use the same tile IDs
- Navigation polygons still seem to need an outline (was revised in v1.6.2). This is now fixed.

## [1.6.2] - 2023-12-04

### Added

- Added another option 'add_id_as_metadata' working similar like 'add_class_as_metadata'.  
If enabled the Tiled object id is added to the corresponding Godot element as meta data.  
This was requested in issue #32.

### Fixed

- Revised the creation of navigation polygons to avoid 'Deprecated...' messages in Godot 4.2.  

## [1.6.1] - 2023-11-09

### Fixed

- Revised the gid/firstgid handling to work more properly.  
  (In rare cases unpleasant problems could occur.)

## [1.6.0] - 2023-10-17

### Added

- Added support for Tiled's Custom Types (by use of new option 'Tiled Project File')  
Thanks to a proposal of 'Sythelux', see pull request #25

### Fixed

- Fixed two minor issues that could have occurred in rare cases.

## [1.5.4] - 2023-10-08

### Added

- Added support for `godot_group` custom property to set Group on the Node  
This was done by 'JoelCreatesGames', thank you Joel!
- Some docu extensions to clarify recurring questions/problems

## [1.5.3] - 2023-08-23

### Fixed

- Fixed issue #18
- Fixed 'No loader found for resource...' error (was my bad since the very first version).  
  It's now possible to instantiate a .tmx/.tmj directly by double clicking on it.  
  Thus copying the resulting .tscn parallel to the .tmx/.tmj is no longer done.  
  That's now left to Godot's 'Please confirm...' dialog opening upon double clicking a .tmx/.tmj

## [1.5.2] - 2023-08-22

### Added

- Runtime packages for C# as well as GDScript

### Fixed

- Fixed issue #17

## [1.5.1] - 2023-08-18

### Added

- Object Alignment on Tilesets is now supported.

### Fixed

- Sub rects on Tile objects resulting in sprites do work now.
- Tilesets with unsorted tile ids needed one more fix.

## [1.5.0] - 2023-08-14

### Added

- For defining the mapping of Tiled objects to Godot elements Tiled's Class field was used so far.<br>
  However, this Class field is required by some users for Custom Types.<br>
  As a new feature now a property 'godot_node_type' can be used for this mapping.<br>
  It has priority i.e. if 'godot_node_type' is present the class field is no longer evaluated.
- Post processors sometimes must know the content of the (Tiled) Class field.<br>
  By enabling a new option 'add_class_as_metadata' the Class field is made available in Godot.

### Fixed

- In rare cases tile ids in tilesets turned out to be not sorted.<br>
  This could produce errors and is now fixed.

## [1.4.0] - 2023-08-08

### Fixed

- Tiled wangsets ('terrain sets') were previously mapped to Godot terrains to keep the terrain set names.<br>Strictly speaking, this was not quite correct (issue #14).<br>
  From this version on they are now mapped to Godot terrain sets and the colors ('terrains') are mapped to Godot terrains.<br>So the mapping is as expected.<br>
  Please note: Godot terrain sets are automatically named 'Terrain Set 0', 'Terrain Set 1' etc. and cannot be named differently (Godot 4 "feature").<br><br>
  The previous mapping can still be enforced using the new 'Map Wangset to Terrain' option.

## [1.3.1] - 2023-06-22

### Fixed

- Godot custom data or meta data did not always have the right type (issue #13). This is now fixed.
- For some properties concerning layers non-existing layers were not created. This is now fixed.

## [1.3.0] - 2023-06-18

### Added

- Sub rectangles in Tiled tilesets are now supported (issue #12).

### Fixed

- Fixed a (rare) null reference problem that came up recently.

## [1.2.5] - 2023-05-27

### Fixed

- Fixed issue #11: If 'Use Tilemap Layers' is enabled set 'Z Index' at the respective layer and not on the canvas item.

### Added

- You (optionally) can be explicit with 'Z Index' by naming the property 'layer_z_index' or 'canvas_z_index'.

## [1.2.4] - 2023-05-07

### Fixed

- Fixed issue #9
- Separation setting for animated tilesets had a limit, reworked that
- GDScript: Minor correction regarding wangset color

## [1.2.3] - 2023-04-02

### Fixed

- GDScript: Fixed typo (upcoming at the end of issue #6)
- GDScript: Fixed problem in issue #7
- Changed some property names to match the reference

## [1.2.2] - 2023-04-01

### Fixed

- Isometric: Problem resolved with offsets introduced in 1.2.0 (issue #6)
- Isometric: Tilesets used by isometric maps can be orthogonal or isometric, this now no longer matters
- Isometric: Enhanced position and rotation calculations for tile collision elements

## [1.2.1] - 2023-03-29

### Fixed

- GDScript-only hotfix - some missing string conversions could result in error messages  
  (Probably no one was affected by this yet)

## [1.2.0] - 2023-03-27

### Added

- New classes 'area', 'staticbody', 'characterbody', 'rigidbody' for tile objects  
  Assigning one of this classes to a tile object results in  
  - Parent CollisionObject2D (according to the class) +
  - Child Sprite2D (Tile object) +
  - Child CollisionShapes based on those assigned to the tile in Tiled
- New class 'instance' + file property 'res_path' for all objects  
  for instantiating pre-existing scene files (e.g. *.tscn files)  
  (Feature implemented on request, issue #3)
- New optional custom (bool) property 'no_import' on layers  
  If set to 'true' the layer is not imported  
  (Feature implemented on request, issue #4)

### Fixed

- Collision shapes on isometric tiles now properly handled
- In case of embedded tilesets some elements were skipped

## [1.1.2] - 2023-03-21

### Fixed

- GDScript-only hotfix - somehow crept-in wrongly placed tab removed   
  (Probably no one was affected by this yet)

## [1.1.1] - 2023-03-13

### Added

- Compatibility with Tiled 1.10: both 'class' and 'type' attributes are evaluated

## [1.1.0] - 2023-03-12

### Fixed

- Wrong placement of tiles if tilesets with different tile sizes are used
- Physics, navigation and occluder polygons were missing on alternative tiles  
  (alternative tiles are created if tiles are flipped or rotated)

## [1.0.0] - 2023-03-06

First published release
