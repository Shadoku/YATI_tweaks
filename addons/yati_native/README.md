# yati_native: the YATI runtime importer as a GDExtension

A native (Rust, [godot-rust](https://github.com/godot-rust/gdext)) version of this repository's
GDScript runtime importer (`runtime/`), meant to import Tiled maps during play without a hitch
and to stream the chunks of infinite maps. It keeps the GDScript runtime's API: `Importer.gd`
here has the same option variables, the same four `import*` methods and the same
`last_error_count` / `last_warning_count`, so a project switches by changing one preload path.

The plan, the measurements behind it and the stages: [docs/native-importer-plan.md](../../docs/native-importer-plan.md).

## Status: stage 0 (scaffold)

The extension builds and loads, and the facade runs the test suite, but **it imports nothing
yet**: every `import*` method prints `yati_native: not implemented yet (stage 2)`, sets
`last_error_count = 1` and returns `null`. Use the GDScript runtime (`runtime/`) for now.

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

## Usage (from stage 2 on)

```gdscript
var importer = preload("res://addons/yati_native/Importer.gd").new()
var map: Node = importer.import("res://maps/desert.tmx")
if map != null:
	add_child(map)
```

The options and methods are those of the GDScript runtime ([Runtime.md](../../Runtime.md)).
`Importer.native_version()` returns the loaded library's version, or `""` when the library is
missing for this platform. The native class itself, `TiledNative`, offers only `version()` and a
round-trip `sum(PackedInt32Array)` for the smoke test (`tools/smoke_native.gd`) so far.
