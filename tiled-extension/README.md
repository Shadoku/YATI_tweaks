# yati-godot: the Godot side of a Tiled project, inside Tiled

A Tiled scripting extension for maps that YATI imports into Godot. It brings the Godot project's
facts into Tiled as custom types: its layer names as flags enums, its node types, its scenes and
their exported properties as classes. It checks a map on save for the mistakes the importers would
only report at run time, and against the project's own [rules](#rules) about what class members
refer to (another map, an object in it, a file, a text key), with actions that pick, follow, open
and preview those references. It runs the import in Godot from Tiled. It talks to the game only
through the `.tiled-project` file, the maps, and (for the check) the Godot binary.

> **Status: tested in a real Tiled 1.12, headless.** The logic is tested under Node, the glue
> against a fake of Tiled 1.12's documented scripting API, and `test/real_tiled.test.mjs` runs the
> sample through a real Tiled (`tools/install_tiled.sh`, `tiled --evaluate`, no editor): the module
> loads, the sync and the scenes action write the committed project file, a second run reports
> "Unchanged", the save hook files the sample map's mistakes, the *Configure* dialog is built with
> Tiled's own widgets and saved. What a headless run cannot reach is the editor: the Issues'
> double-click, the dialog's look, *Check map in Godot* on the active map, the reload of the
> project after a sync (headless there is no editor to reload, and the fallback runs). Go through
> the [manual checklist](#manual-checklist-in-a-real-tiled-112)'s steps 4 to 8 in your Tiled 1.12
> before relying on those, and report what differs in `docs/tiled-extension-plan.md` (section 5)
> or `docs/tiled-extension-ux-plan.md` (section 4). The [rules](#rules) are newer: their save-time
> findings and the setting of two members in one undo step ran in the real Tiled headless, the four
> actions only against the fake; the checklist's steps 9 to 15 are for them.

- Requires **Tiled 1.12** or newer (the custom-type API is new in 1.12).
- The check action also needs **Godot 4.7** and the game's `addons/yati_native/` with
  `check_map.gd`, which is in releases after YATI 3.1.0.
- The plan with its decisions: [docs/tiled-extension-plan.md](../docs/tiled-extension-plan.md); the
  planned UX pass (one configuration dialog, no manual reopen):
  [docs/tiled-extension-ux-plan.md](../docs/tiled-extension-ux-plan.md).

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
are saved in the `.tiled-project` and shared with everyone who opens it. So the extension needs an
open project: without one (Tiled starts without a project unless its session restores one; see
*Project > Recent Projects*) the actions say so, and a saved map or tileset is validated without
the project's custom types. *YATI: Configure…* is one dialog with a field for each of the first
five (`yati_rules` and `yati_open_command` are set in *Project > Project Properties*); the other actions open it for you when the Godot project is not set. Paths are relative to
the project file unless they are absolute.

| Property | Default | Meaning |
|----------|---------|---------|
| `godot_project` | (none: the other actions refuse) | the folder that holds `project.godot` |
| `godot_binary` | `godot` | the Godot 4.7 binary: a path, or a command on `PATH` |
| `godot_scenes_dir` | the Godot project | where *Import scenes as classes* looks for scenes: `res://...` or a path |
| `yati_import_options` | (none) | options for the check, as `name=value` pairs, e.g. `warn_unknown_classes=false assign_node_properties=false` ([Runtime.md's options](../Runtime.md#options); `class_scenes`, a Dictionary, cannot be given here) |
| `yati_validate` | `true` | `false` turns the validation on save off (the rules' checks too) |
| `yati_rules` | (none) | what class members refer to, a JSON array: see [Rules](#rules) |
| `yati_open_command` | (the system's: `xdg-open`, `open`, or `cmd /c start`) | the command *YATI: Open or create file…* opens a file with, the file appended, e.g. `code -r`; it must return at once (a launcher), since Tiled waits for it |
| `yati_generated` | (written by the extension) | which types and class members the extension made: `{"Type": ["member", ...]}` |
| `yati_fixups` | (written by the extension) | what it wrote into the project file past the API, and how many times (see [What the API cannot set](#what-the-api-cannot-set)) |

### The Configure dialog

Every field has a tooltip that says what the setting does, which actions use it, and the property
it is saved as. *Godot project* and *Scenes folder* are folder pickers; *Godot binary* is text (a
command on `PATH` must stay possible) with *Browse…* and *Test*, which runs the binary with
`--version` and shows which Godot it is, or that it could not start; *Import options* is text,
with the option names and their values in its tooltip and its mistakes shown under it as you
type, and *Options…* opens a second dialog with one field per option (a dropdown of *importer
default*, `true`, `false` for a boolean, of the listed values for `parallax_mode`,
`animation_mode`, `object_refs` and `unknown_class_fallback`, text for `custom_data_prefix` and
`save_tileset_to`; each option's meaning and default as its tooltip; a value not in the list is
kept as a last entry; text this version does not know is kept as written) whose *OK* writes the
`name=value` text back into the field, so the stored form stays the string; *Validate maps and
tilesets on save* is a check box. A line at the bottom says how many types the extension manages.

*Save* checks everything first and refuses, naming each problem in red, while something cannot
work: no Godot project or none with a `project.godot`, a binary path that does not exist, a
scenes folder that does not exist, an option with a bad boolean, a `class_scenes`, or text that
is no `name=value` pair. What may still work is a warning: a binary that `--version` could not
start or that is not Godot 4 (*Check map in Godot* will not work until it is fixed; the other
actions do not need Godot), a scenes folder outside the Godot project, an unknown option name
(passed on: a newer importer may know it). A warning your change brought asks you to press
*Save anyway*; the settings as they were save at once. *Cancel*, Escape and the window's close
button change nothing. A save writes only the properties that changed (a default is left unset),
and reports to the log, not a message box.

## Actions

Tiled's `extendMenu` cannot make submenus, so the eight actions are at the end of the **Map** menu,
after a separator, each named with the prefix *YATI:*. The last four act on the one selected object
of the active map and need `yati_rules`.

| Action | What it does | What it writes |
|--------|--------------|----------------|
| *YATI: Configure…* | one dialog with a field per setting, tooltips, and a check of every field before it closes (above) | the settings above |
| *YATI: Sync Godot project…* | reads `project.godot` | the enums `GodotPhysicsLayers`, `GodotNavigationLayers`, `GodotRenderLayers` (flags, stored as int, one value per layer up to the highest named one, an unnamed layer as `Layer N`, so a value's index is its bit); `GodotNodeType` (the mapping table's class names, stored as string, for a `godot_node_type` property); the class `MapRef` (`map`: file, `object`: int), the Reference's convention for a reference into another map |
| *YATI: Import scenes as classes…* | scans `godot_scenes_dir` for `.tscn` files whose root node has a script (asks first) | one class per scene (below); the enums its exports use; `Vector2` / `Vector2i` when an export needs them |
| *YATI: Check map in Godot* | runs the game's check script on the active map (offers to save it first) | Issues: one per error or warning of the import |
| *YATI: Pick map and object…* | for an `object_in_map` rule with a `map` member: asks for a map (file dialog), then offers the names of its objects of the rule's `targetClass` in a list. For an `object_in_project` rule: one list of every named object of the rule's classes in the project's maps, each as `<map>: <name> (<class>)`, sorted by map. Several such rules for the class: a list of them first | both members (`object_in_map`) or the one member (`object_in_project`), in one undo step |
| *YATI: Follow reference* | opens the map an `object_in_map` rule's `map` member names (or stays in the map without one), or for an `object_in_project` rule the project map that holds the named object, and selects and centres the named object; says so when there is none, and when there are several (the first is selected). Without such a value, a `map` rule's map is opened | nothing |
| *YATI: Open or create file…* | opens a `file` rule's file with `yati_open_command` or the system's program for it. Unset member and a rule `template`: copies the template into the rule's `folder` (else beside the map) as `<map>_<object name>.<suffix>` after asking, and sets the member; a set member whose file is missing: makes that file from the template | the file; the member, in one undo step |
| *YATI: Show text for selected object* | for each `key` rule: the table's text in every language, plain text, or that the key is not in the table (with keys that look close) | nothing (a message box and the log) |
| (saving a map or tileset) | validates it, a map also against the rules | Issues: one per finding (below) |

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
members, with the list of them in `yati_fixups` (the number of such writes, `written`, is in it
too). Then it **reloads the project** itself, the way *File > Open File or Project* on the
`.tiled-project` does: the message says so first, Tiled closes the open maps, reads the project
file again and reopens the maps (a map with unsaved changes asks to be saved first; Cancel there
keeps everything as it is). The extension reads back whether the reload happened (the project
then holds the `yati_fixups` that was written into the file). If it did not (a save prompt
cancelled, or no editor), a second message asks you to reopen the project yourself. Until a
reload, the next time Tiled saves the project from memory it overwrites the fixes, which happens
on any change to custom types or project properties; running any of the actions again writes
them back. A second run after a reload changes nothing and reloads nothing.

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

With `yati_rules` set, a saved map's objects are also checked against the rules (next section), and
so are the properties of each world in the rules that holds the map; a rule with a mistake is
reported as a warning (`yati_rules, rule 3: ...`) and left out.

## Rules

The project property `yati_rules` (a string, edited in *Project > Project Properties*; it may span
lines) is a JSON array. Each rule says what one class member refers to:

| Field | Meaning |
|-------|---------|
| `class` | the objects' class (`"Warp"`), a list of classes, or `"*"` for any object. The class is the object's own, else its tile's, else its template's, as Tiled resolves it; class names are case-sensitive |
| `world` | instead of `class`: a `.world` file (relative to the project file) whose own properties the rule checks |
| `member` | the member (property) checked. An object that does not set it has its class's default. `unique_name` takes none: it checks the objects' own names |
| `kind` | `map`, `object_in_map`, `object_in_project`, `unique_name`, `file` or `key` (below) |
| `required` | `true`: an unset or empty value is a finding (for `unique_name`: an object without a name). Otherwise an empty value is fine |
| `severity` | `"error"` (the default) or `"warning"` |

| Kind | The value must | Fields of its own |
|------|----------------|-------------------|
| `map` | name a `.tmx` or `.tmj` file that exists | |
| `object_in_map` | be the name of an object in the map the member `map` names (in the same map without `map`), of class `targetClass` when given; two objects with that name and class are a warning | `map`, `targetClass` |
| `object_in_project` | be the name of exactly one object in any map of the project, of one of the classes `targetClass` (a name or a list) when given. No such object, or more than one (each named with its map and id), is a finding | `targetClass`, `maps` |
| `unique_name` | (no `member`) an object of the rule's classes has a name that no other object of those classes has, in this map or any other map of the project; each clash is a finding naming the other map and object id. With `required`, every such object must have a name | `maps` |
| `file` | name a file that exists, whose text matches every `contains` pattern | `contains`: `[{"pattern": <regular expression>, "label": <what it is, for the message>}]`; `template`: the file *Open or create file…* copies; `folder`: where the copies go (else beside the map) |
| `key` | when it matches `keyPattern` (default `^[A-Z][A-Z0-9]*(_[A-Z0-9]+)+$`: UPPER_SNAKE_CASE with an underscore), be a key of the CSV file `table`, i.e. in its first column (Godot's translation CSV: a header row `keys,en,...`); a value of another shape is plain text and fine | `table`, `keyPattern` |

The project's maps, for `object_in_project` and `unique_name`, are the `.tmx` and `.tmj` files under
the project's folders (*Project > Add Folder to Project*, `Project.folders`; hidden folders are
skipped), or, with `maps`, the files a folder holds (`"maps"`) or a glob matches (`"maps/**/*.tmx"`:
`*` and `?` stay within a folder, `**` spans folders), relative to the project file. The map being
saved counts as it is in Tiled, as does every map open in Tiled; the others are read from their
files on each save, and a file whose text has not changed since the last save is not parsed again.
In a project of 300 maps of 51 objects each, the first save in a session took about 0.33 s in Tiled
1.12.0 (reading and parsing every map) and each later save about 0.07 s; under Node the check of
the same project takes about 60 ms. A map that cannot be read is a warning, and the rules skip it.

Paths in a rule (`world`, `template`, `folder`, `table`, `maps`) are relative to the project file. Values
are read as Tiled stores them: a file member relative to its map (a class's default relative to the
project file), `res://` against the Godot project; a `string` member holding a path works too. A
map that a rule looks into is read as it is open in Tiled (with unsaved changes), else from its
file (`.tmx` or `.tmj`, an object's class from its template or its tile). A world's properties are
read from a Tiled properties list (`"properties": [{"name", "type", "value"}]`) or, failing that,
from the world file's top-level string keys; the world's rules run when a map the world lists (or
one of its patterns matches) is saved. An unknown field is a mistake (it catches `"requird"`).

A complete example, for a game whose `Warp` objects have one member `to` (string) naming another
object anywhere in the project, a `Warp` or a `PlayerSpawn`, whose names are unique across every
map; `Trigger` (`sequence`: file), `Interactable` (`text`, `sequence`) and `Sign` (`text`); whose
sequences are XML files of `<sequence name="...">` sections; whose text is the CSV table
`text/strings.csv`; and whose `.world` names its `start_spawn` (adjust the paths to your layout):

```json
[
  {"class": ["Warp", "PlayerSpawn"], "kind": "unique_name", "required": true},
  {"class": "Warp", "member": "to", "kind": "object_in_project", "targetClass": ["Warp", "PlayerSpawn"]},
  {"class": "Warp", "member": "sequence", "kind": "file"},
  {"class": "Trigger", "member": "sequence", "kind": "file", "required": true,
   "contains": [{"pattern": "<sequence\\s+name=.on_(enter|leave).", "label": "section named on_enter or on_leave"}],
   "template": "sequences/templates/trigger.xml", "folder": "sequences"},
  {"class": "Interactable", "member": "sequence", "kind": "file",
   "contains": [{"pattern": "<sequence\\s+name=.on_interact.", "label": "section named on_interact"}],
   "template": "sequences/templates/interactable.xml", "folder": "sequences"},
  {"class": ["Interactable", "Sign"], "member": "text", "kind": "key", "table": "text/strings.csv"},
  {"world": "maps/overworld/overworld.world", "member": "start_spawn", "kind": "object_in_project", "targetClass": ["Warp", "PlayerSpawn"], "required": true}
]
```

Patterns are JavaScript regular expressions inside JSON strings, so a backslash is written twice;
`name=.on_(enter|leave).` takes either quote. With these rules a save reports, for instance:
`Object 'cellar_door' (id 12, layer Objects): to names 'from_twn', but no map of the project has an
object of that name and class Warp or PlayerSpawn.`, `Object 'gate' (id 3, layer Objects): the name
'gate' is also used by maps/coast/beach.tmx (id 7, class Warp); ...`, `... sequence names
well.xml, which has no section named on_interact.`, `... text is 'SIGN_WELCOM', shaped like a key,
but strings.csv has no such key (did you mean SIGN_WELCOME?).`. `sample/rules/` is a small Tiled
project with rules of every kind (in lower case class names; its warps use `object_in_map` with a
`to_map` member, its portals in `maps/portals/` `object_in_project` and `unique_name`), maps with and
without mistakes, sequences, templates, a table and a world.

What the rules cannot see: a warp in another map that names an object you rename here is reported
when that map is saved, not this one (and a name you give here that another map already uses is
reported here, but not in the other map until it is saved); a world file is read from disk, not
from Tiled's worlds.

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
Steps 1 to 3 (the dialog's fields and a save, but not its look) and the findings of step 5 are
what `test/real_tiled.test.mjs` runs headless; the rest needs the editor, so do it by hand.

1. Copy `tiled-extension/sample/` somewhere, and `yati-godot/` into its `tiled/extensions/`. Rename
   `tiled/sample.tiled-project` to `expected.tiled-project`, and copy
   `tiled-extension/test/fixtures/sample_start.tiled-project` (only the `godot_project` setting, no
   types) to `tiled/sample.tiled-project`. Open that in Tiled 1.12 and enable its extensions. The
   log shows the module imported and no error. The Map menu ends with the four *YATI:* entries.
2. *YATI: Configure…*: one dialog. Hover each field for its tooltip. Clear the Godot project and
   press *Save*: the dialog stays open and says so in red. Set it to `godot/`, type an import
   option such as `warn_unknown_classes=maybe` and see the message under the field; press
   *Options…*: `warn_unknown_classes` shows `maybe (not a listed value)`; choose `false` and *OK*:
   the field reads `warn_unknown_classes=false`. Press *Save*: the dialog closes, *Help > Show Log*
   has "YATI: Configured." and the settings. (Without Godot on `PATH`, *Save* first warns that `godot` could not start and asks for
   *Save anyway*; *Test* says the same.) Clear the option again before going on, or expect it in
   the project file. Then *YATI: Sync Godot project…*: the message lists four created types and
   says the project is reloaded now. (The sample names no navigation layer, so there is no
   `GodotNavigationLayers`, and the report says so.) After the message the project reloads: an
   open map closes and comes back. In *View > Custom Types Editor*, `GodotPhysicsLayers` has
   `World, Player, Enemies, Layer 4, Pickups` with *Allow multiple values (flags)* checked, with no
   reopen by hand. Run it again: "Unchanged", no reload. To see the fallback: change a map, run the
   sync after editing a type by hand (the flags are then lost from the file), and press Cancel in
   the save prompt: a second message asks you to reopen the project yourself.
3. *YATI: Import scenes as classes…*: confirm. `Slime` and `Chest` are created, with the enums
   `Chest.material` and `Slime.Mood` and the class `Vector2`, and the report names
   `torch.tscn` as skipped (no script). The project reloads. `diff` the project file with
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

The rules (`sample/rules/`, its own Tiled project; copy `yati-godot/` into its `extensions/`):

9. Open `rules.tiled-project`, then `maps/broken.tmx`, change something and save: the Issues view
   shows eight rule findings (`nowhere` to `silent`, as `test/rules.test.mjs` lists them).
   Double-click one: its object is selected. Save `maps/town.tmx`: no finding. Edit
   `maps/game.world`'s `start_spawn` to `gate` in a text editor, save `town.tmx` again: one
   finding for the world.
10. In `broken.tmx` select `nowhere` and run *YATI: Pick map and object…*: a file dialog (filtered
    to `.tmx`/`.tmj`) opens; choose `cellar.tmx`; a list offers `from_town`; *OK*. The Properties
    view shows `to_map` and `to_spawn` set; one *Edit > Undo* removes both.
11. Select `to_cellar` in `town.tmx` and run *YATI: Follow reference*: `cellar.tmx` opens (or comes
    to the front) with `from_town` selected and the view centred on it. On `typo` in `broken.tmx`:
    a message says `cellar.tmx` has no object named `from_twn`.
12. Select `well` in `town.tmx` and run *YATI: Open or create file…*: `sequences/well_trigger.xml`
    opens in the system's editor for XML (on Windows through `cmd /c start`, unverified so far).
    Select `empty` in `broken.tmx` and run it: it asks, makes `sequences/broken_empty.xml` from
    `templates/trigger.xml`, sets `sequence`, and opens the file. Set `yati_open_command` to your
    editor's launcher (e.g. `code`) and try again.
13. Select `welcome` in `town.tmx` and run *YATI: Show text for selected object*: the English and
    German text of `SIGN_WELCOME`; on `misspelt` in `broken.tmx`: not a key, with
    `SIGN_WELCOME, SIGN_WELL` suggested.
14. Open `maps/portals/clash.tmx`, change something and save: six findings (`east_gate` also in
    `east.tmx`, `nowhere_gate` in no map, the portal without a name, the two spawns named `twin`,
    and `to_twin` naming both). Save `maps/portals/west.tmx`: none (its portal names `from_town` in
    `maps/cellar.tmx`). Rename `east_gate` in `clash.tmx` to `clash_gate` without saving, then save
    `east.tmx`: no finding (the open map counts as it is).
15. In `east.tmx` select `east_gate` and run *YATI: Pick map and object…*: one list of the project's
    portals and spawns, `maps/cellar.tmx: from_town (player_spawn)` first; choose `maps/town.tmx:
    from_cellar (player_spawn)` and *OK*: `to` is `from_cellar`, one *Edit > Undo* restores it.
    Run *YATI: Follow reference* on it: `town.tmx` opens with `from_cellar` selected and centred. On
    `to_twin` in `clash.tmx`: `twin` (id 3) is selected and a message names both objects.

The headless run (`test/real_tiled.test.mjs`) confirmed in Tiled 1.12.0: the ES module loads with
its relative imports; `ClassPropertyType` and its constants exist on the global, and the global
`StorageType` the reference documents does not (both handled); assigning `color`, `values`,
`storageType` and `usageFlags`; `setMember` with `tiled.filePath(...)`, `tiled.objectRef(0)` and
`tiled.propertyValue(...)` saving what the tests expect; type names with a dot (`Slime.Mood`);
member values read back through `members` comparing equal ("Unchanged" on a second run); the
project file Tiled writes equal to the fake's; `new Dialog(title)` with `addFilePicker`
(`isDirectory`, `fileName` a plain path), `addTextInput` (`text`, `placeholderText`),
`addCheckBox` (`checked`), `addComboBox` (`currentIndex`), `addButton` (`click()` reaching the connected
handler), `addLabel` (`text`), tooltips read back, a `Process` run from a handler, properties written from the
handler reaching the project file; `Project.folders` (absolute paths) and the save hook of the
rules that look across the project's maps (`sample/rules/maps/portals/`, a map read through
Tiled's format given its file name). Not confirmed yet, for the editor: the dialog's look; a
`res://` file property read as an empty `localFile` with the URL in `url`; `Process.exec`
finding `godot` on `PATH`; an Issue's activation; `tiled.extendMenu` (headless there is no Map
menu, and the extension logs that instead of failing).

## Limits

- Tiled below 1.12 has no type API: the extension does not work there.
- Class members cannot be lists, so `Array` exports get no member (see the table).
- The flags switch and float members go through the project file and a reload of the project
  (the open maps close and reopen).
- Validation cannot see the game's `class_scenes`, and checks templates only as last saved.
- *Check map in Godot* blocks Tiled while Godot runs (`Process.exec`), and so does *Open or create
  file…* while its command runs: `yati_open_command` must name a launcher that returns at once.
- Tiled's scripting API has no status bar, so the key preview is an action, not a passive display.
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
extension uses, each with the reference page that documents it (the Qt widgets of `Dialog` have
no page: they are cited as `@mapeditor/tiled-api` 1.12.0's `index.d.ts` declares them). The tests fail when the glue uses
another member or stops using one of them. With `acorn` installed (or `YATI_ACORN=<path to
acorn.mjs>`), a test also parses the modules as ECMAScript 2016, the language of Tiled's
JavaScript engine.

`real_tiled.test.mjs` runs the extension inside a real Tiled. `tools/install_tiled.sh` downloads
the release pinned in `.tiled-version` (the Linux x86_64 AppImage, extracted into `~/.cache/tiled/`,
so no FUSE is needed) and prints a `tiled` wrapper; the test runs it as `tiled --project <file>
--evaluate helpers/real_tiled_harness.mjs <steps>`, under `xvfb-run` when there is no display (the
AppImage has only Qt's xcb platform plugin) and with a config directory of its own (no user
extensions, no session). The harness installs the extension through `install(api)` with the message
boxes stubbed (a confirmation is yes) and drives the actions, the Configure dialog (built with the
real `Dialog`, filled through its widgets, *Save* pressed with `click()`; `exec()` would block) and
the save hook (a map read through Tiled's own format). Each run is a fresh Tiled: without the
editor the extension's reload of the project cannot happen (`tiled.open` throws, and the
fallback message runs), so the reload is the next run. Without a Tiled the test is skipped; on another platform install Tiled 1.12 and set
`TILED=<binary>`. CI installs Tiled and runs it.

| File | What |
|------|------|
| `yati-godot/yati-godot.mjs` | the glue: actions, menu, save hook, Issues, the API read into plain models |
| `yati-godot/lib/settings.mjs` | the settings, the import options, paths |
| `yati-godot/lib/configure.mjs` | the Configure dialog's fields, tooltips, validation and stored values, and the import options as fields, as data and rules |
| `yati-godot/lib/godot_project.mjs` | `project.godot`, layer names |
| `yati-godot/lib/scene_exports.mjs` | `.tscn` roots, `.gd` exports and enums, GDScript literals, the scene scan |
| `yati-godot/lib/types.mjs` | the generated types as data |
| `yati-godot/lib/apply.mjs` | the types applied through the API, merged; the project file patch |
| `yati-godot/lib/validate.mjs` | the validation |
| `yati-godot/lib/rules.mjs` | the project's rules: parsed, checked on a map and a world; maps read from their files; the CSV table; the actions' helpers |
| `yati-godot/lib/check_output.mjs` | the check's command line and output |
| `test/rules.test.mjs`, `test/rules_glue.test.mjs` | the rules under Node, and in the glue against the fake (on `sample/rules/`) |
| `test/rules_project.test.mjs`, `test/rules_project_glue.test.mjs` | the rules that look across the project (`object_in_project`, `unique_name`, the project's maps, a 300-map project's cost) under Node, and in the glue against the fake (on `sample/rules/maps/portals/`) |
| `test/real_tiled.test.mjs` | the sample inside a real Tiled, headless (skipped without one), the rules sample's save hook among it |
| `test/helpers/real_tiled_harness.mjs` | runs inside Tiled under `--evaluate`: the actions and the save hook, the dialogs stubbed |
