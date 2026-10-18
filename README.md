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

## Native importer (planned)

[docs/native-importer-plan.md](docs/native-importer-plan.md) explores a Rust (GDExtension) version of
this runtime that imports maps during play without a hitch and streams chunks of infinite maps, with the
measurements behind it and the stages to get there. The GDScript runtime stays as the reference.

## Credits

All the importer logic originates from [YATI](https://github.com/Kiamo2/YATI) by Roland Helmerichs.
If you find the importer useful, consider supporting the upstream author ([buy him a coffee](https://www.buymeacoffee.com/kiamo2)).

## License

[MIT License](LICENSE). Copyright (c) 2023-2026 Roland Helmerichs.
