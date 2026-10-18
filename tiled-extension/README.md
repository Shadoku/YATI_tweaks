# yati-godot: the Godot side of a Tiled project, inside Tiled

A Tiled scripting extension for maps that YATI imports into Godot. It brings the Godot project's
facts into Tiled as custom types: its layer names as flags enums, its node types, its scenes and
their exported properties as classes. It checks a map on save for the mistakes the importers would
only report at run time, and it runs the import in Godot from Tiled. It talks to the game only
through the `.tiled-project` file, the maps, and (for the check) the Godot binary.

> **Status: written and tested without Tiled.** No Tiled ran while it was developed. The logic is
> tested under Node, and the Tiled glue against a fake of Tiled 1.12's documented scripting API.
> Before relying on it, go through the [manual checklist](#manual-checklist-in-a-real-tiled-112)
> in your Tiled 1.12. Start with steps 1 to 3: the extension loads, the sync works, and a second
> sync reports "Unchanged". Report what differs in `docs/tiled-extension-plan.md` (section 5).

- Requires **Tiled 1.12** or newer (the custom-type API is new in 1.12).
- The check action also needs **Godot 4.7** and the game's `addons/yati_native/` with
  `check_map.gd`, which is in releases after YATI 3.1.0.
- The plan with its decisions: [docs/tiled-extension-plan.md](../docs/tiled-extension-plan.md).

## Install

Copy the folder **`yati-godot/`** (with its `lib/`) into one of these:

- **Tiled's extensions folder**, for all projects. Open it from *Edit > Preferences > Plugins >
  Open extensions directory*. It is `~/.config/tiled/extensions/` on Linux,
  `~/Library/Preferences/Tiled/extensions/` on macOS and
  `C:/Users/<you>/AppData/Local/Tiled/extensions/` on Windows.
- **The project's own `extensions/` folder**, next to the `.tiled-project` (the project's
  *Extensions Directory*, `extensions` by default). Tiled asks once whether to enable a project's
  extensions. Answer *Enable Extensions*.

Tiled loads the `.js` and `.mjs` files at the top of the extensions folder and in each folder
directly under it. So `yati-godot/yati-godot.mjs` is loaded, and it imports `lib/*.mjs` itself as
ES modules. Do not copy `tiled-extension/` as a whole: only `yati-godot/` is the extension. Tiled
reloads an extension when its files change. *Help > Show Log* should show
`Importing module '.../yati-godot.mjs'` and no error.

## Settings

The settings are custom properties of the Tiled project (*Project > Project Properties*), so they
are saved in the `.tiled-project` and shared with everyone who opens it. *YATI: Configure…* asks
for the first four. Paths are relative to the project file unless they are absolute.

| Property | Default | Meaning |
|----------|---------|---------|
| `godot_project` | (none: the other actions refuse) | the folder that holds `project.godot` |
| `godot_binary` | `godot` | the Godot 4.7 binary: a path, or a command on `PATH` |
| `godot_scenes_dir` | the Godot project | where *Import scenes as classes* looks for scenes: `res://...` or a path |
| `yati_import_options` | (none) | options for the check, as `name=value` pairs, e.g. `warn_unknown_classes=false assign_node_properties=false` ([Runtime.md's options](../Runtime.md#options); `class_scenes`, a Dictionary, cannot be given here) |
| `yati_validate` | `true` | `false` turns the validation on save off |
| `yati_generated` | (written by the extension) | which types and class members the extension made: `{"Type": ["member", ...]}` |
| `yati_fixups` | (written by the extension) | what it wrote into the project file past the API (see [What the API cannot set](#what-the-api-cannot-set)) |

In *Configure…*, cancelling the first folder dialog cancels everything. An empty answer keeps a
text setting as it is. For the options, `-` clears them.

## Actions

Tiled's `extendMenu` cannot make submenus, so the four actions are at the end of the **Map** menu,
after a separator, each named with the prefix *YATI:*.

| Action | What it does | What it writes |
|--------|--------------|----------------|
| *YATI: Configure…* | asks for the settings with Tiled's dialogs | the settings above |
| *YATI: Sync Godot project…* | reads `project.godot` | the enums `GodotPhysicsLayers`, `GodotNavigationLayers`, `GodotRenderLayers` (flags, stored as int, one value per layer up to the highest named one, an unnamed layer as `Layer N`, so a value's index is its bit); `GodotNodeType` (the mapping table's class names, stored as string, for a `godot_node_type` property); the class `MapRef` (`map`: file, `object`: int), the Reference's convention for a reference into another map |
| *YATI: Import scenes as classes…* | scans `godot_scenes_dir` for `.tscn` files whose root node has a script (asks first) | one class per scene (below); the enums its exports use; `Vector2` / `Vector2i` when an export needs them |
| *YATI: Check map in Godot* | runs the game's check script on the active map (offers to save it first) | Issues: one per error or warning of the import |
| (saving a map or tileset) | validates it | Issues: one per finding (below) |

Each action reports what it created, updated, kept and removed in a message box and in the log.
An Issue opens its map when you activate it (double-click in the *Issues* view) and selects the
object or layer. The *Issues* view keeps old entries until you clear it.

### Types are merged, not replaced

Types are matched by name. A type that exists is updated in place, so it keeps its id and every
property that uses it. The enums' values are regenerated. A class member is added or updated when
the extension made it (`yati_generated` records which). A member you added by hand in Tiled is kept,
even when an export has the same name. A member is removed only when the extension made it and the
script no longer exports it. A class's colour and fill are set only when the class is new; its
usage only gains the flags it needs. Types are never deleted: a class whose scene is gone stays,
and the report tells you.

### Scenes as classes

A class per scene with a script on its root (an inherited scene takes its base's root script). It
is named after the script's `class_name`, or after the scene file in PascalCase
(`big_slime.tscn` → `BigSlime`) when the script has none or several scenes share it. A name that
is also a mapping-table class (`area`, `path`, ... case-insensitive), `node2d`, or one of the
extension's own types gets the suffix `Scene` and a warning. The class can be used on objects and
tiles. Its members:

- `godot_scene` (file): the scene, relative to the project file. With
  `read_godot_scene_from_custom_types` (the default) the importers instantiate it for every object
  of the class (Reference.md, "Scene mapping and node properties").
- One member per export of the script and of the scripts it extends. The default is the export's
  value where the script writes it as a literal (a constant or an expression gives the type's
  zero):

| Export | Tiled member |
|--------|--------------|
| `int`, `@export_range(...) var x: int`, `@export_flags(...)` | `int` |
| `float` | `float` |
| `bool` | `bool` |
| `String`, `StringName`, `@export_multiline`, `@export_placeholder`, `@export_dir` | `string` |
| `Color`, `@export_color_no_alpha` | `color` (`Color()` without a default: opaque black) |
| `NodePath`, `@export_node_path(...)`, a Node type (`Node2D`, a `class_name` extending a node) | `object` (0, unset): the importer turns it into a `NodePath` or the node (F2); left unset, an empty `NodePath` without a warning |
| a Resource type (`Texture2D`, `PackedScene`, a `class_name` extending `Resource`), `@export_file` | `file`: the importer loads it (F1); a `res://` default becomes a path relative to the project file, `user://` none |
| a script enum (`Mood`, `Other.Mood`) | an enum named `<declaring class>.<Enum>` (`Enemy.Mood`; a script without `class_name` by its file name, `BaseUnit.Team`), stored as string, values the constant names: the importer matches them by name |
| `@export_enum("A", "B:5")` | an enum `<class>.<variable>` (`Enemy.gait`), values `A`, `B` |
| `@export_flags_2d_physics` / `_2d_render` / `_2d_navigation` | `int` of type `GodotPhysicsLayers` / `GodotRenderLayers` / `GodotNavigationLayers` when the sync made it, else `int` |
| `Vector2`, `Vector2i` | the class `Vector2` (`x`, `y` float) / `Vector2i` (`x`, `y` int): the importers turn such a value into the vector (stage 2 of the plan) |
| `Dictionary`, `Rect2`, `Callable`, other types; `@export_storage`; an untyped export without a default | none (listed as skipped in the report) |
| `Array[...]`, `Array`, `Packed*Array` | none (listed as skipped: "Tiled class members cannot be lists"). Give an object a `list` property of that name instead: the importer assigns it to the `Array` (a member generated by an earlier version, an empty `string`, is removed on the next run) |

Exports named `godot_scene`, `name` or `script` are skipped (the importer never assigns them).
`@export_group`, `@export_subgroup` and `@export_category` lines are ignored. The scan skips
`.godot/`, hidden folders, folders with a `.gdignore`, and the project's `addons/` when it scans the
whole project. Set `godot_scenes_dir` into `addons/` to scan an addon's scenes. A line the parsers do
not understand is skipped.

## What the API cannot set

Tiled 1.12's scripting API has no "Allow multiple values (flags)" switch for an enum. The
reference's `EnumPropertyType` has only `storageType` and `values`, and Tiled's sources have no
`valuesAsFlags` property either. Its `setMember` also stores a whole JavaScript number (`0`, `40.0`)
as an `int`. So after applying the types through the API, the extension writes these into the
project file that Tiled has just saved: `valuesAsFlags` on the layer enums, and `float` on float
members. The list is kept in `yati_fixups`. Then it asks you to **reopen the project** (*File > Open
File or Project*, or *Project > Recent Projects*) so that Tiled reads the file. Until you do, the
next time Tiled saves the project from memory it overwrites the fixes. That happens on any change to
custom types or project properties. Running any of the actions again writes them back. A second run
after reopening changes nothing and asks for nothing.

## Validation on save

When a map or tileset is saved (and `yati_validate` is not `false`), each finding becomes an Issue:

| Finding | Severity | Why |
|---------|----------|-----|
| an object class that is no mapping-table class, no class of the project and has no `godot_node_type` (with a hint when only the case differs: the project's classes are case-sensitive) | warning | B8: the importer makes it `unknown_class_fallback` (a StaticBody2D), unless the game's `class_scenes` maps it, which the extension cannot see |
| class `instance` without `res_path` | error | the importer skips the object |
| `res_path`, `godot_scene` or `godot_script` naming a file that does not exist (`res://` against the Godot project, a class's default against the project file, else against the map) | error | the import fails for that object |
| an `object` property (or list item) naming an id the map does not have | warning | F2: an empty `NodePath` and a warning (one left at 0, unset, is no finding: the importers give an empty `NodePath` silently) |
| a template file that does not exist | error | read from the map file as last saved: the API does not expose an object's template |
| a layer mask (`collision_layer`, `collision_mask`, `navigation_layers`, `light_mask`, `visibility_layer`, `platform_floor_layers`, `platform_wall_layers`, `parsed_collision_mask`, `occluder_light_mask`, the tileset's `_n` forms and `layers_n`) whose string names a layer its enum does not have, or names layers without being of the enum's type (the importer then reads layer numbers) | warning | F5 |
| a `godot_node_type` outside `GodotNodeType` | warning | the importer warns and falls back |

## Check map in Godot

Runs

```
<godot_binary> --headless --path <godot_project> -s res://addons/yati_native/check_map.gd -- <map> --project=<.tiled-project> --<option>=<value> ...
```

and blocks Tiled until Godot exits. The script (stage 2 of the plan) imports the map with the
native importer and prints one line per message, then a result line:

```
YATI_CHECK error|warning <object id or -> <text>
YATI_CHECK result <errors> <warnings> <nodes> <cells> <ms>
```

Each message becomes an Issue that selects its object. The result line is shown in a message box
and the log. Without the script the action says so and does nothing. When Godot does not start
(`godot_binary` wrong) or prints no result line, the message box says why, with the last line Godot
printed.

## Manual checklist in a real Tiled 1.12

`tiled-extension/sample/` has a tiny Godot project (`godot/`) and the project file the sync and
the scenes action are expected to make from it (`tiled/sample.tiled-project`; a test asserts it).

1. Copy `tiled-extension/sample/` somewhere, and `yati-godot/` into its `tiled/extensions/`. Rename
   `tiled/sample.tiled-project` to `expected.tiled-project`, and copy
   `tiled-extension/test/fixtures/sample_start.tiled-project` (only the `godot_project` setting, no
   types) to `tiled/sample.tiled-project`. Open that in Tiled 1.12 and enable its extensions. The
   log shows the module imported and no error. The Map menu ends with the four *YATI:* entries.
2. *YATI: Sync Godot project…*: the message lists four created types and asks you to reopen. (The
   sample names no navigation layer, so there is no `GodotNavigationLayers`, and the report says
   so.) Reopen the project. In *View > Custom Types Editor*, `GodotPhysicsLayers` has `World,
   Player, Enemies, Layer 4, Pickups` with *Allow multiple values (flags)* checked. Run it again:
   "Unchanged", no request to reopen.
3. *YATI: Import scenes as classes…*: confirm. `Slime` and `Chest` are created, with the enums
   `Chest.material` and `Slime.Mood` and the class `Vector2`, and the report names
   `torch.tscn` as skipped (no script). Reopen. `diff` the project file with
   `expected.tiled-project`: they should be equal. Run the action again: "Unchanged".
4. In the Custom Types Editor add a member `note` to `Slime` by hand. Rename `hp` in
   `godot/scripts/slime.gd` to `health`, and run the scenes action: `health` is added, `hp` is
   removed, `note` is kept, and `Slime`'s colour stays as it is.
5. Open `tiled/maps/demo.tmx` and save it (change something first). The Issues view shows three
   entries: class `Slme`, the `instance` without `res_path`, and the mask naming `Ghosts`.
   Double-click one: the object is selected and the view centres on it.
6. Give an object the class `Slime`. Its properties show the members with the script's defaults:
   `mood` a dropdown, `home` an `x`/`y` pair, `hits` checkboxes.
7. With a game that has `addons/yati_native/check_map.gd`, set *YATI: Configure…* to it and run
   *YATI: Check map in Godot* on a map. The result shows, and messages appear as Issues. Try a
   wrong `godot_binary`: the message box says Godot could not start.
8. Set `yati_validate` to `false` in the project properties and save `demo.tmx` again: no new
   Issues.

Things only a real Tiled can confirm, in the order to try them: the ES module loads with its
relative imports; `ClassPropertyType.MapObjectClass` and the other constants exist on the global
(else Tiled 1.12's values are used); the global `StorageType` the reference documents (its absence
is handled: Tiled's sources register none); assigning `color`, `values`, `storageType` and
`usageFlags`; `setMember` with `tiled.filePath(...)`, `tiled.objectRef(0)` and
`tiled.propertyValue(...)` saving what the tests expect; type names with a dot (`Enemy.Mood`);
the comparison of member values read back through `members` (if it fails, every run reports
"Updated" and asks to reopen); a `res://` file property read as an empty `localFile` with the URL
in `url`; `Process.exec` finding `godot` on `PATH`.

## Limits

- Tiled below 1.12 has no type API: the extension does not work there.
- Class members cannot be lists, so `Array` exports get no member (see the table).
- The flags switch and float members go through the project file and a reopen.
- Validation cannot see the game's `class_scenes`, and checks templates only as last saved.
- *Check map in Godot* blocks Tiled while Godot runs (`Process.exec`).
- The GDScript parser reads declarations, not expressions: a default computed from constants or
  calls is not followed.

## Tests

```
node --test tiled-extension/test
```

Node 22 (CI uses it). `test/` holds the fixtures (`fixtures/godot_project/`, a Godot project with
every export form; `fixtures/maps/`, maps with one of each mistake; `fixtures/importer/`, copies of
the importers' fixtures from `test/fixtures/`; `fixtures/check_output*.txt`, the check's output: one
written by hand, two captured from the real `check_map.gd` with Godot's colour codes), the expected `.tiled-project` fragments
(`expected/`; `UPDATE_EXPECTED=1` rewrites them and the sample's project, then review the diff),
and the fake API (`helpers/fake_tiled.mjs`). `helpers/api_surface.mjs` lists every Tiled member the
extension uses, each with the reference page that documents it. The tests fail when the glue uses
another member or stops using one of them. With `acorn` installed (or `YATI_ACORN=<path to
acorn.mjs>`), a test also parses the modules as ECMAScript 2016, the language of Tiled's
JavaScript engine.

| File | What |
|------|------|
| `yati-godot/yati-godot.mjs` | the glue: actions, menu, save hook, Issues, the API read into plain models |
| `yati-godot/lib/settings.mjs` | the settings, the import options, paths |
| `yati-godot/lib/godot_project.mjs` | `project.godot`, layer names |
| `yati-godot/lib/scene_exports.mjs` | `.tscn` roots, `.gd` exports and enums, GDScript literals, the scene scan |
| `yati-godot/lib/types.mjs` | the generated types as data |
| `yati-godot/lib/apply.mjs` | the types applied through the API, merged; the project file patch |
| `yati-godot/lib/validate.mjs` | the validation |
| `yati-godot/lib/check_output.mjs` | the check's command line and output |
