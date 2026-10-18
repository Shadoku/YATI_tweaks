# Tiled extension plan: `yati-godot`, the Godot side of a Tiled project inside Tiled

Part C of `docs/runtime-improvement-plan.md` (X1), taken up on 2026-10-18 after parts A and B
and the native importer (`docs/native-importer-plan.md`) landed. A Tiled scripting extension
that brings a Godot project's facts into Tiled (its layer names, its node types, its scenes
and their exported properties) as custom types and dropdowns, checks a map for the mistakes
the importers would only find at run time, and runs the import from Tiled. It lives in
`tiled-extension/yati-godot/` in this repository, is installed by copying into Tiled's
extensions folder or a project's own `extensions/` directory, and talks to the game only
through the `.tiled-project` file, the maps and (for the check) the Godot binary.

## 1. What was checked before planning

- **Tiled 1.12's scripting API** (https://www.mapeditor.org/docs/scripting/): `tiled.project`
  (`Project`, since 1.10.1) has `propertyTypes`, `addClassType(name)`, `addEnumType(name)`,
  `findTypeByName(name)`, `removeTypeByName(name)` (all since 1.12) and custom properties
  (`property`, `setProperty`, saved in the `.tiled-project`); `ClassPropertyType` has
  `setMember(name, value | PropertyType)`, `removeMember`, `members`, `color`, `drawFill`,
  `usageFlags` with the constants `MapObjectClass`, `TileClass`, `LayerClass`, `MapClass`,
  `TilesetClass`, `WangColorClass`, `WangSetClass`, `ProjectClass`, `PropertyValueType`,
  `AnyUsage`; `EnumPropertyType` has `values` and `storageType`; `tiled.registerAction`,
  `tiled.extendMenu`, `tiled.assetAboutToBeSaved`, `tiled.warn(text, activated)` and
  `tiled.error` (Issues panel entries with a callback), `tiled.prompt`, `tiled.promptOpenFile`
  and `promptDirectory` (1.10.2), `Process` (`exec`, `start`, `readStdOut`, `exitCode`,
  `workingDirectory`), `TextFile`, `File`. Extensions may be ES modules (`.mjs`), so the pure
  logic can be shared with Node tests. **To verify first thing in stage 1:** whether
  `EnumPropertyType` exposes `valuesAsFlags` through the API (the reference page summary did
  not list it); if not, the flags bit is set by editing the project file's JSON through
  `TextFile` with a "reload the project" prompt, and the plan entry says so.
- **What the importers read** (both the GDScript runtime and the native one, 3.1): the
  mapping table's class names (`collision`, `staticbody`, `characterbody`, `rigidbody`,
  `animatablebody`, `area`, `navigation`, `occluder`, `line`, `path`, `polygon`, `instance`,
  `parallax`) and the `godot_node_type` property; a class type's `godot_scene` member
  (type `file`, relative to the project file) with `read_godot_scene_from_custom_types`;
  `object` properties resolved to NodePaths (F2), lists of them to `Array[NodePath]`;
  layer masks from an `int` or from a flags enum's value names (F5: value index = bit);
  the `MapRef { map: file, object: int }` convention; node property assignment with its
  conversions (F1; `string` to a script enum by constant name, `file` to a Resource).
- **The game that drove the work** (`goo_pachinko/assets/tmx/goo_pachinko.tiled-project`)
  hand-writes one class type per loader class with typed members and defaults, and one enum;
  its classes are not scenes (a loader reads them), so for it the extension's value is the
  save-time validation against the project's classes, the generated Godot enums, and the
  check-in-Godot action, not scene generation.
- **No Tiled binary runs here**, so the extension cannot be exercised in Tiled by this
  session. Node 22 is present: the extension's logic is plain modules with tests under
  `node --test`, and the Tiled glue is thin and tested against a fake of the documented API.
  The manual checklist in its README is for the user's own Tiled.

## 2. Decisions (what changed from X1 and why)

- **One extension, `tiled-extension/yati-godot/`**, an ES-module entry `yati-godot.mjs` that
  registers the actions and the save hook, and `lib/*.mjs` with the logic (parsing
  `project.godot`, `.tscn` and `.gd`; building the type definitions; validating a map;
  parsing the import's output). Nothing in it knows any particular game.
- **Settings live on the Tiled project as custom properties** (`tiled.project.setProperty`,
  saved in the `.tiled-project`): `godot_project` (the directory holding `project.godot`),
  `godot_binary` (path; default `godot`), `godot_scenes_dir` (where to scan for scenes;
  default the project), `yati_import_options` (a string such as
  `assign_node_properties=false warn_unknown_classes=false`, passed to the check). A
  *YATI → Configure…* action asks for them with the prompt dialogs; the other actions refuse
  with a clear message until `godot_project` is set.
- **X1a, the layer enums**, generated as F5 expects: `GodotPhysicsLayers`,
  `GodotNavigationLayers`, `GodotRenderLayers` (from `[layer_names]` `2d_physics/layer_N`,
  `2d_navigation/layer_N`, `2d_render/layer_N` in `project.godot`), flags, storage `int`,
  one value per layer in layer order up to the highest named layer, unnamed layers as
  `Layer N` so that value index = bit holds for the string form too. Regeneration replaces
  the values and keeps the type's id (Tiled keeps references by name).
- **X1b, `GodotNodeType`**: an enum (storage `string`) of the mapping table's names (without
  `""`), for a `godot_node_type` property. The fallback value `node2d` is not in it: the
  importers do not accept it as a `godot_node_type`.
- **X1c, scenes to classes**, the largest piece. *YATI → Import scenes as classes…* scans
  `godot_scenes_dir` for `.tscn` files whose root has a script, reads the script's
  `@export` declarations (`@export var`, `@export_range`, `@export_enum`, `@export_file`,
  `@export_node_path`, `@export_multiline`, grouped exports; a typed default when present),
  and makes or updates one class type per scene, named after the script's `class_name`,
  else the scene file's name in PascalCase, `useAs` object and tile, with:
  - `godot_scene` (file, the scene path relative to the project file: F1's rule);
  - one member per export, typed `int`, `float`, `bool`, `string` (String, StringName,
    `@export_multiline`), `color`, `file` (Resource-typed, `@export_file`), `object`
    (NodePath or a Node-typed export: F2 resolves it), a generated enum (a script enum or
    `@export_enum`: storage `string`, values the constant names; F1 matches them by name),
    or the class `Vector2` / `Vector2i` (section 3) for those exports; an `Array[...]`
    export a `list`-free `string` member with a comment, since Tiled's class members cannot
    be lists (the importer's property assignment then skips it with a warning: documented);
  - the default from the script where it is a literal;
  - **merge by name**: members the script no longer has are removed only when the extension
    generated them (Tiled types carry no properties of their own, so the record is the project
    property `yati_generated`, JSON of type name to generated member names), members added by
    hand in Tiled are kept, `color` and `drawFill` are set only when the type is new (a stable
    hash of the name picks the colour).
  A scene whose generated name collides with a mapping-table class gets a warning and a
  `Scene` suffix. The action reports what it created, updated and kept.
- **The `MapRef` class** (`map: file`, `object: int`), the Reference's cross-map convention,
  is generated by the sync so a warp system has it without typing it.
- **X1d, save validation** (`tiled.assetAboutToBeSaved` on maps and tilesets), each finding
  an Issues entry whose activation selects the object: an object class that is neither a
  mapping-table class, nor a class type of the project, nor given a `godot_node_type`
  (B8 would warn at import); class `instance` without `res_path`; `res_path` or `godot_scene`
  naming a file that does not exist next to the map or project; an `object` property left at
  0 (F2 would warn); a template file missing; a mask property (`collision_layer`, `_mask`,
  `navigation_layers`, `light_mask`, `visibility_layer`, the `_n` forms) whose string names
  a value no `Godot*Layers` enum has; a `godot_node_type` value outside `GodotNodeType`. A
  project property `yati_validate = false` turns the hook off.
- **X1e becomes *YATI → Check map in Godot***: runs `godot_binary --headless --path
  godot_project -s res://addons/yati_native/check_map.gd -- <map> <options>` with `Process`
  and files every line of the importer's output that is an error or warning as an Issue
  (section 3 adds the script to the addon). "Open in Godot" is dropped: the check is what a
  map author wants from Tiled, and a running editor is one click away anyway. Skipped with
  a message when the game has no `addons/yati_native/`.
- **Tests under Node** (`tiled-extension/test/*.test.mjs`, `node --test`): the parsers on a
  fixture Godot project (`tiled-extension/test/fixtures/godot_project/`: `project.godot` with
  layer names, scenes with scripts covering every export form, an enum, a `class_name`, a
  scene without a script, a tileset), the type builders' output compared with expected
  `.tiled-project` fragments, the validator on fixture maps (reuse `test/fixtures/*.tmx` where
  they fit: `unknown_class.tmx`, `scenes.tmx`, `masks.tmx`, `object_refs.tmx`), the output
  parser on captured importer output, and the glue against a fake `tiled` object implementing
  the documented API surface (so a renamed API member fails a test, not the user). CI runs
  them (`ci.yml`, a Node step; the runner has Node).
- **Not in scope**: automapping, exporting maps from Tiled (the importers read `.tmx`
  directly), editing scenes from Tiled, Tiled versions below 1.12.

## 3. Additions on the importers' side (this repository's addons)

- **`addons/yati_native/check_map.gd`** (and the same script for the GDScript runtime,
  `runtime/check_map.gd`, sharing the output format): a headless `SceneTree` script:
  `godot --headless --path <project> -s res://addons/yati_native/check_map.gd -- <map>
  [--project=<.tiled-project>] [--option=value ...]` imports the map with the facade, the
  options applied, frees the tree, and prints one machine-readable line per message
  (`YATI_CHECK error|warning <object id or -> <text>`) plus a final
  `YATI_CHECK result <errors> <warnings> <nodes> <cells> <ms>` line; exit code 0 when no
  error. The importers' messages name the object (`'Door (Area)'`): the script maps names
  to ids from the imported tree's `id` metadata (`add_id_as_metadata` on for the check) so
  the extension can select the object. A test in `test/run_tests.gd` (or a small script test
  in `test/run_all.sh`) runs it on a fixture and checks the lines.
- **`Vector2` and `Vector2i` from class values** (F1's conversions, both importers): a
  property of type `class` whose members are exactly `x` and `y` (both numbers) assigned to a
  node property of type `Vector2` or `Vector2i` becomes that vector (as metadata it stays the
  Dictionary, as today). Generated by X1c for vector exports as the class types `Vector2`
  (`x`, `y` floats) and `Vector2i` (`x`, `y` ints). One test per importer
  (`test_new_vector_class_property`), the Reference's conversion table and the changelog
  updated.

## 4. Stages and gates

### Stage 1: the extension, its tests, its docs

- `tiled-extension/yati-godot/` as in section 2: the entry module, `lib/godot_project.mjs`
  (`project.godot` layer names), `lib/scene_exports.mjs` (`.tscn` root script and `.gd`
  exports and enums), `lib/types.mjs` (enum and class definitions: pure data), `lib/apply.mjs`
  (the data applied through Tiled's API: add, update, keep), `lib/validate.mjs`,
  `lib/check_output.mjs`, `lib/settings.mjs`; `tiled-extension/test/` with the fixtures and
  tests; `tiled-extension/README.md` (install into Tiled's extensions folder or a project's
  `extensions/`, the actions, the settings, what the generated types look like, the manual
  checklist for a real Tiled: open the sample project, run each action, save a map with a
  known mistake and see the Issue); `tiled-extension/sample/` with a tiny Godot project and
  the `.tiled-project` the sync is expected to produce from it (the test asserts it).
- CI: a `node --test tiled-extension/test` step in `ci.yml`.
- Docs: `README.md` gets a "Tiled extension" line; `Runtime.md` a section; the runtime
  improvement plan's Part C marked as taken up here; `CHANGELOG.md` `[Unreleased]`.
- Gate: `node --test` green; every documented API member the glue uses is one the fake
  implements and the Tiled 1.12 reference lists (the test file names the reference page per
  member); the sample's expected `.tiled-project` equals the generated one.

### Stage 2: the importers' additions

- `check_map.gd` for both importers, the `Vector2`/`Vector2i` conversion, tests, docs
  (section 3).
- Gate: both suites green (105 of 105 with the new test), `test/run_all.sh` for both runtimes
  runs the check script on a fixture and sees its result line, native gates unchanged.

### Stage 3: adoption in the game, and a real Tiled

- The game vendors the extension under its `assets/tmx/extensions/yati-godot/` (Tiled's
  project-local extensions path), sets the project properties, and its CLAUDE.md and
  `assets/tmx/README.md` say how to run the sync and the check. Its hand-written class
  types stay (they are loader classes, not scenes); the sync adds the Godot enums and
  `MapRef`, and the validator knows its classes.
- The user runs the manual checklist in Tiled 1.12 (nothing here can); findings come back
  as issues on this plan's section 5.

## 5. Status

- 2026-10-18: planned; stages 1 and 2 launched in parallel.
- 2026-10-18, stage 2 done (commits `12935d7`, `68349e1`; branch `claude/tiled-extension-importers`):
  - **Vector2 / Vector2i** (`12935d7`): `convert_for_property` and `get_typed_array_for` of both
    importers (`runtime/TilemapCreator.gd`'s `class_to_vector`, `native/src/builder/assign.rs`'s
    `class_to_vector`) turn a class value of exactly the members `x` and `y`, both numbers, into a
    `Vector2`, and into a `Vector2i` when both are integral (`3.0` fits, `1.5` stays metadata with
    the usual warning); lists of them fit `Array[Vector2]` / `Array[Vector2i]`; as metadata the
    Dictionary stays. Fixture: `scenes.tmx`'s object `Scout` (id 8), `scenes.tiled-project`'s class
    types `Vector2` (float members) and `Vector2i` (int members), `enemy.gd`'s `home`, `cell`,
    `route: Array[Vector2]`; `test_new_vector_class_property` (also the misfits, from a map it writes
    to `test/out/`). Parity dumps of `scenes.tmx` regenerated; `cargo test` (parity included),
    clippy `-D warnings`, `fmt --check` clean.
  - **`check_map.gd`** (`68349e1`): one script, the same file in `runtime/` and
    `addons/yati_native/` (it loads the `Importer.gd` next to it), output and exit codes as in
    section 3. How each importer gets its messages: both now have `Importer.last_messages`
    (`[{kind, text}]`, one per counted error or warning, text without colour tags). The GDScript
    runtime cannot intercept its prints, so `CommonUtils` keeps a per-import `messages` list
    (`reset()`, `report_error()`, `report_warning()`, `count_error()`), which replaced the separate
    print and counter lines at every counting site (printed output unchanged); the native facade
    builds the list from `TiledMap.get_errors()`, `get_warnings()` and
    `TiledBuilder.get_messages()` (`E:` / `W:`). A message's object: a quoted name after "of" or
    "object" that a node with `id` metadata has, else a `Class '<c>'` message's object of lowest id
    with that `class` metadata, else any quoted node name, else `-`. The script trims a leading
    `-- `, `ERROR: ` or `WARNING: ` from the text. Options are the facade's `OPTION_NAMES` (the
    runtime's script variables but `last_*`: the same 16), so `share_tilesets` and
    `async_tiles_per_frame` are not accepted. `test/test_check_map.sh` (in `test/run_all.sh`, so
    in CI) runs both scripts on 9 cases and expects the same lines from both.
  - Gates: `test/run_tests.gd` `SUMMARY: 105 passed, 0 failed (new_: 0, bug_: 0, other: 0) of
    105` for `res://runtime` and `res://addons/yati_native`; `test/run_all.sh` exits 0 for both
    runtimes (`SUMMARY: check_map 18 passed, 0 failed`, test_async 71, test_stream 86);
    `tools/native_gate.gd` unchanged and passing (no leaks over 20 rounds of 57 maps, small map
    0.53 ms warm). Every fixture map's `last_messages` matches its counts in both importers.
  - Not identical: the result line's `<ms>`; a map whose parse reports errors and warnings may
    order them differently (the native `TiledMap` keeps them in two lists, errors first); and
    `infinite.tmj`, where the importers already differed before this stage (the runtime warns
    about the object property `ref`, 11 nodes; the native importer about class `enemy`, 13 nodes;
    no test covers it), so the two scripts print different lines for it.
