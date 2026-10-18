# YATI runtime (Yet Another Tiled Importer) for Godot 4

A GDScript library that imports maps created with the [Tiled Map Editor](https://www.mapeditor.org)
(.tmx / .tmj, with external .tsx / .tsj tilesets and .tx / .tj templates) **while your game is running**
and turns them into a ready-to-use Godot node tree (`TileMapLayer`s, `Node2D`s, physics bodies, sprites, ...).

This repository is a runtime-only fork of [YATI by Roland Helmerichs (Kiamo2)](https://github.com/Kiamo2/YATI).
It contains only the GDScript runtime importer; the editor import plugin and the C# variants of the
upstream project are not part of it. If you want to import Tiled maps as scenes inside the Godot editor,
use the upstream project.

Requires Godot 4.3 or higher (tested with Godot 4.7.2 and Tiled 1.12.2; maps from older Tiled versions may work too).
Godot 3.x is not supported.

## Installation

Copy the `runtime` folder with its entire content somewhere into your Godot project
(the folder name can be changed). There is nothing to enable: the importer is plain GDScript.

`PostProcessing.gd` is only needed if you use the post-processing import methods.

## Usage

```gdscript
var importer = preload("res://runtime/Importer.gd").new()
var map = importer.import("res://maps/desert.tmx")
if map != null:
	add_child(map)
```

- `import(map_file, project_file = "")` imports from the file system (`res://`, `user://` or an absolute path).
- `import_from_zip(zip_file, map_file_in_zip, project_file_in_zip = "")` reads the map and everything it references from a zip archive.
- The returned node has no parent; add it directly to your tree. `null` is returned on a fatal error.
- Import options are plain variables on the importer instance (e.g. `importer.use_default_filter = true`).
- `importer.last_error_count` / `importer.last_warning_count` report the result of the last import.

Details, all options and examples: [Runtime document](Runtime.md).
What maps to what in Godot, custom properties and unsupported Tiled features: [Reference document](Reference.md).

## Features

(Nearly) all Tiled features are supported:
- all kinds of layers (tile, object, image, group) incl. infinite maps and all tile data encodings/compressions
- all kinds of objects (rectangle, ellipse, capsule, point, polygon, polyline, text, tile, template)
- all map orientations (orthogonal, isometric, staggered, hexagonal)
- visibility, opacity, tint, offsets, parallax, layer blend modes add/multiply, object draw order
- tile collisions (physics, navigation, occlusion), tile animations, terrains (wangsets)
- custom properties incl. lists, class-typed properties and Tiled custom types (project file)

By setting classes and custom properties in Tiled the resulting node tree can be largely customized.

## Limitations & Particularities

1. Ellipses are not available in Godot. Where appropriate (on objects) a capsule is used instead.
   On tiles inside Godot tilesets ellipses and capsules are approximated by polygons; points on tiles are skipped.
2. Tileset animation in Godot requires the involved tiles to be equidistant and either horizontally or vertically arranged.
   Tiled animations with arbitrary frame tiles cannot be mapped and are skipped.
3. Resources (.tres/.res/.tscn) referenced by properties cannot be loaded from a zip archive.
4. See the [Reference document](Reference.md#tiled-features-not-supported) for Tiled features without a Godot equivalent.

## Tests

The `test` folder contains a headless test suite and a benchmark (see the comments in `test/run_tests.gd`),
and `test/bench_materialize.gd`, which measures what the engine itself charges for a map's tile layers.
`test/run_all.sh` runs the native importer's smoke test, the suite, the benchmark and (with the native library)
its asynchronous and streaming tests ([test/README.md](test/README.md)).

## Native importer

`addons/yati_native/` is this runtime as a Rust GDExtension: the same `Importer.gd` API, options and
node tree (the test suite passes against both), 4 to 5 times faster, plus what GDScript cannot do:
`import_async()` imports a map during play without a hitch (the parse on a worker thread),
TileSets are shared across imports, and `TiledStreamer` streams large or infinite maps and whole
Tiled worlds chunk by chunk within a per-frame budget. It needs Godot 4.7 and a library for the
platform (Linux x86_64, Windows x86_64 and macOS are built by CI; Linux arm64 with cargo; web and
mobile not yet). The GDScript runtime stays the reference and the fallback.

Install: copy `addons/yati_native/` into your project and put the libraries into its `bin/`
(`yati-native.zip` from a release, or `bash tools/install_native.sh` / `bash tools/build_native.sh`
from this checkout), then preload `res://addons/yati_native/Importer.gd` instead of the runtime's.
Details: [Runtime.md](Runtime.md#native-importer), the addon's
[README](addons/yati_native/README.md), and the plan with its measurements,
[docs/native-importer-plan.md](docs/native-importer-plan.md).

**Versions.** The repository's releases (`CHANGELOG.md`, tags `v3.x.y`) version the GDScript
runtime and everything in this repository. The native library has its own version, the crate's
(`native/Cargo.toml`, `TiledNative.version()`, `Importer.native_version()`; 0.2.0 with release
3.1.0): pushing the tag `v<crate version>` (`v0.2.0`) makes `.github/workflows/native.yml` publish
`yati-native.zip` as a release of that tag, while a `v3.x.y` tag builds the libraries but publishes
none. `tools/install_native.sh` downloads the release named in `.yati-native-version` (or a
branch's rolling build, `native-<branch>`). The pure-Rust core (`native/tiled/`, crate
`tiled_core`) is versioned with the library.

## Credits

All the importer logic originates from [YATI](https://github.com/Kiamo2/YATI) by Roland Helmerichs.
If you find the importer useful, consider supporting the upstream author ([buy him a coffee](https://www.buymeacoffee.com/kiamo2)).

## License

[MIT License](LICENSE). Copyright (c) 2023-2026 Roland Helmerichs.
