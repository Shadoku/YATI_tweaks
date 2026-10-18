<!-- Brought into the repository on 2026-10-18 from a review of the GDScript runtime at 61ef0fc
     (3.0.0). How its items land here, now that a native (Rust) importer is being built beside
     the runtime, is in docs/native-importer-plan.md, section 7: Parts A and B are executed on the
     GDScript runtime (the reference, with a test per item) and then matched by the native
     builder; F4 is superseded by the native plan's stages 3 and 4; Part C is set aside for now.
     Status per item is tracked in the changelog and the test suite, not in this file. -->

# YATI_tweaks — Improvement Plan

Target: `Shadoku/YATI_tweaks` runtime importer, HEAD `61ef0fc` (3.0.0, 2026-10-17).
Verified against Godot 4.5.1 headless with Tiled 1.12-format fixtures.
Line numbers refer to `runtime/TilemapCreator.gd` unless another file is named.

This plan has three parts:

- **Part A — Bug fixes and behaviour changes** (B1–B10). Small, independent, each with a `test_bug_*` regression test. Do these first; several later features depend on them (especially B3/B4 colours and B2 template merging).
- **Part B — New features** (F1–F5). Larger, each behind an importer option so the default output stays stable.
- **Part C — Tiled extension** (X1). Lives in a new `tiled-extension/` folder, independent of the GDScript runtime.

Suggested order: B5 → B3/B4 → B7 → B2 → B1 → B8 → B6 → B9 → B10 → F2 → F1 → F3 → F4 → X1 → F5.
(B5 is a one-line crash fix; B3/B4 are prerequisites for colour tests everywhere; B2 before B1 because B1 extends the same merge block; F2 before F1 because F1's property setter needs F2's id→node map; F3/F4 serve the world streamer and are independent of F1/F2.)

Conventions used throughout:

- Every change gets a fixture under `test/fixtures/` and a test in `test/run_tests.gd` following the existing `test_bug_*` / `test_new_*` naming.
- Every new importer option is a plain `var` on `Importer.gd` with a setter on `TilemapCreator`, documented in `Runtime.md` (options table) and `Reference.md`.
- Every change gets a `CHANGELOG.md` entry under `[Unreleased]`.
- Warnings use the existing `print_rich` + `CommonUtils.warning_count += 1` pattern; errors use `printerr` + `error_count`.

---

## Part A — Bug fixes and behaviour changes

### B1. Template instance overrides (size, rotation, visibility) are ignored

**Status:** verified. A 32×32 instance of a 16×16 template, rotated 45°, imports as 16×16 at 0°.

**Cause:** `create_object()` lines 1059–1068 copy only `id`, `class`, `type`, `name`, `opacity` from the instance onto the template object. Per the TMX spec, any attribute saved on the instance overrides the template.

**Change** (`create_object`, template branch ~line 1031):

```gdscript
# Attributes saved on the instance override the template (TMX spec, "Template files")
for key in ["width", "height", "rotation", "visible", "gid"]:
    if obj.has(key):
        template_obj[key] = obj[key]
```

Notes:
- `gid` override is needed for B2 (a flipped instance saves its own gid).
- `x`/`y` are already passed via the `offset` parameter; leave that.
- The `rotation` of a tile-object instance applies around the object origin exactly as for non-template objects, so no extra math is needed once the value reaches the normal gid/shape branches.

**Test:** `test_bug_template_overrides` — fixture `templates.tmx` with `probe_tpl.tx` (tile object) and `probe_rect.tx` (class `area` rect). Instances resized/rotated/hidden. Assert `Sprite2D.scale`, `rotation_degrees`, `visible`, and `RectangleShape2D.size` match the instance values.

---

### B2. Template instances with an overridden `gid` or `text` create a second node

**Status:** verified. Flipping a template tile object in Tiled writes `gid` on the instance; import yields the template sprite **plus** an extra `_tile` Sprite2D at scale 0.00001.

**Cause:** After the template branch (line 1020) returns nothing, execution falls through to `if obj.has("gid")` (line 1099) and `elif obj.has("text")` (line 1272), which have no `not obj.has("template")` guard (the plain-shape branch at line 1311 does have one).

**Change:**
1. Guard both branches: `if obj.has("gid") and not obj.has("template"):` and `elif obj.has("text") and not obj.has("template"):`.
2. Merge instance `gid` into the template object (done by B1). For `text`, merge the instance's `text` dictionary over the template's (`template_obj["text"].merge(obj["text"], true)`).

**Test:** `test_bug_template_flipped_gid` — instance with `gid="2147483651"` (h-flipped tile 3). Assert exactly one Sprite2D created, `flip_h == true`, no node named `_tile`.

---

### B3. Tiled `#AARRGGBB` colours are decoded as `#RRGGBBAA`

**Status:** verified. `tintcolor="#80ff0000"` (50 % red) produces modulate `(0.502, 1.0, 0.0)` — yellow-green. `backgroundcolor="#80ff0000"` produces a ColorRect with alpha 0.

**Cause:** `Color(string)` in Godot expects `#RRGGBBAA`; Tiled writes alpha first. Affected call sites:
- `handle_layer`: `Color(tint_color, layer_opacity)` ×4 (tile, object, group, image layers)
- `create()` line 290: `Color(_background_color)`
- `create_object` text branch line 1307: `Color(txt.get("color", ...))`
- `CommonUtils.get_right_typed_value("color", …)` already reorders the bytes but returns a **String** (see B4).

**Change:** add to `CommonUtils.gd`:

```gdscript
# Converts a Tiled colour string (#RRGGBB or #AARRGGBB) to a Godot Color
static func tiled_color(s: String, fallback := Color.WHITE) -> Color:
    if s == "": return fallback
    if s.length() == 9:   # #AARRGGBB -> #RRGGBBAA
        s = "#" + s.substr(3) + s.substr(1, 2)
    return Color.from_string(s, fallback)
```

Replace every `Color(<tiled string>)` with `CommonUtils.tiled_color(...)`. For tint + opacity: `var c := tiled_color(tint_color); c.a *= layer_opacity`.

**Test:** `test_bug_argb_colors` — layers with `tintcolor="#80ff0000"`, map `backgroundcolor="#80ff0000"`, text object `color="#80ff0000"`. Assert `modulate ≈ (1, 0, 0, 0.5)`, ColorRect color alpha ≈ 0.5, label font colour alpha ≈ 0.5.

---

### B4. Colour-typed custom properties arrive as Strings and don't drive colour properties

**Status:** verified. A `type="color"` property `modulate="#ff00ff00"` on an Area2D is stored as metadata String `"#00ff00ff"`; `modulate` stays white.

**Cause:**
- `get_right_typed_value` returns a reordered String for `color`.
- `handle_properties` matches `modulate`, `self_modulate`, `color`, `default_color`, `debug_color` only when `type == "string"` (lines 2094, 2096, 2220, 2457, 2463). Same in `TilesetCreator.handle_tile_properties` for tile `modulate`.

**Change:**
1. `get_right_typed_value`: for `"color"` return `tiled_color(val)` (a `Color`). This changes metadata type from String to Color — note as breaking in CHANGELOG (3.1.0).
2. In every colour-property branch accept `type == "string" or type == "color"` and convert via `tiled_color`.
3. Tile custom data layers of Tiled type `color` should be `TYPE_COLOR` (check `CUSTOM_DATA_TYPES` in TilesetCreator; add if missing).

**Test:** `test_bug_color_property_type` — object with colour-typed `modulate` and a colour-typed custom property `tint`. Assert `modulate == Color(0,1,0,1)` and `get_meta("tint") is Color`.

---

### B5. Area2D gravity properties crash and silently drop remaining properties

**Status:** verified. `gravity` (float) on an `area` object → `Invalid assignment of property 'gravitiy'`; the import then reports "succeeded".

**Cause:** typos at lines 2237–2248: `gravitiy_point_center_x/y`, `gravitiy_point_unit_distance`, `gravitiy_direction_x/y`, `target_node.gravitiy`. Also Reference.md lists `linear_damp` and `angular_damp` under the custom-property name `gravity`.

**Change:**
1. Fix all six spellings (`gravity_point_center`, `gravity_point_unit_distance`, `gravity_direction`, `gravity`).
2. Reference.md rows "Linear Damp" / "Angular Damp": custom property `linear_damp` / `angular_damp`.
3. Defensive: wrap the body of the per-property loop in `handle_properties` so that a script error on one property cannot abort the rest. GDScript has no try/catch, so instead validate before assignment where cheap: for the large `elif` chain this is not practical, but a cheap guard is to check `target_node.get_property_list()`-derived name set once per node for the "generic setter" path added in F1. For now, fixing the typos is sufficient; F1 adds the generic path.

**Test:** `test_bug_area_gravity` — area with `gravity`, `gravity_point`, `gravity_point_center_x/y`, `gravity_direction_x/y`, `gravity_point_unit_distance`, `linear_damp`, `angular_damp`. Assert all values land and that a property listed *after* `gravity` is still applied.

---

### B6. Parallax layers are pulled out of the hierarchy into a ParallaxBackground

**Status:** verified. A tile layer with `parallaxx="1.2"` inside a group with `offsetx="100" opacity="0.5"` is reparented under `<root> (PBG)/Fg (PL)/Fg` — the group's offset and opacity are lost.

**Cause:** `handle_parallaxes()` (line 596) moves any layer that has a `parallaxx`/`parallaxy` attribute into a single `ParallaxBackground` (a CanvasLayer at layer -100) using deprecated `ParallaxLayer`. Consequences: group offset/opacity/visibility lost; factors > 1 (foreground) render behind everything; the map background ColorRect gets reparented too (line 599–602).

**Change** — new option `parallax_mode` (String, default `"parallax2d"`; `"legacy"` keeps today's behaviour):

In `"parallax2d"` mode, `handle_parallaxes` becomes:

```gdscript
if layer_dict.has("parallaxx") or layer_dict.has("parallaxy"):
    var px := Parallax2D.new()
    px.name = layer_name + " (PX)"
    px.scroll_scale = Vector2(layer_dict.get("parallaxx", 1.0), layer_dict.get("parallaxy", 1.0))
    # Tiled's parallax origin == the point that does not move; Parallax2D's equivalent is scroll_offset
    px.scroll_offset = Vector2(_parallax_origin_x, _parallax_origin_y) * (Vector2.ONE - px.scroll_scale)
    if layer_dict.get("repeatx", 0): px.repeat_size.x = layer_dict.get("imagewidth", 0); px.repeat_times = 2
    if layer_dict.get("repeaty", 0): px.repeat_size.y = layer_dict.get("imageheight", 0); px.repeat_times = 2
    parent.add_child(px)          # stays at its place in the hierarchy
    px.owner = _base_node
    px.add_child(layer_node)
else:
    parent.add_child(layer_node)
layer_node.owner = _base_node
```

- Remove the ParallaxBackground creation in `create()` (lines 283–286, 314–318) when not in legacy mode; the background ColorRect then stays a direct child of the root.
- The existing `class=parallax` path (Parallax2D by explicit class) remains and now shares this code.
- Document: `map.parallaxoriginx/y` move from "not supported" to "supported" in Reference.md.

**Test:** `test_new_parallax2d_hierarchy` — fixture `parallax.tmx` with a group (offset, opacity) containing a parallax tile layer and a parallax image layer with repeatx. Assert the Parallax2D is a child of the group node, group modulate/position intact, `scroll_scale == (1.2, 1)`, repeat_size.x == image width. Also assert that with `parallax_mode = "legacy"` the old tree shape is produced (keeps existing `test_*parallax*` tests green).

---

### B7. Every tile layer gets `class = "tilelayer"` metadata

**Status:** verified.

**Cause:** `handle_layer` lines 403–405: `class_string = layer.get("class", "")` falls back to `layer.get("type", "")`, but on layers `type` is the layer kind. The same fallback is correct for objects/tiles (pre-1.9 files saved `type`), wrong for layers. Note `get_godot_node_type()` line 884 has the same fallback and is called for object/group/image layers — harmless today because `"objectgroup"` etc. aren't in `GODOT_TYPE_MAP`, but fix for consistency.

**Change:** add `static func get_class_string(d: Dictionary, is_layer: bool) -> String` that only consults `type` when `not is_layer`; use it in `handle_layer` (all four branches), `get_godot_node_type` (pass `is_layer = true` from layer call sites), and `create_object` (`is_layer = false`).

**Test:** `test_bug_layer_class_meta` — tile layer without class has no `class` meta; tile layer with `class="Ground"` has `"Ground"`.

---

### B8. Unknown classes silently become StaticBody2D

**Cause:** `create_object` line 1010–1017: unknown `godot_type` → `BODY`, warning suppressed when `add_class_as_metadata` is true (the default). A rectangle of class `Door` becomes a solid wall.

**Change:**
1. New option `unknown_class_fallback` (String, default `"staticbody"` for compatibility; accepted values: any `GODOT_TYPE_MAP` key plus `"node2d"`).
   - `"node2d"`: create a `Node2D` at the object position; store the shape as metadata `shape` = `Rect2`/`PackedVector2Array`/`Vector2` (reuse `get_object_polygon`), `size` = `Vector2(w, h)`.
2. Always warn once per unknown class name per import (keep a `Dictionary` of names already reported) regardless of `add_class_as_metadata`. Message: `-- Class 'Door' is not a Godot node type -> using <fallback>. Set 'godot_node_type' or 'class_scenes' (see Reference).`
3. F1's `class_scenes` takes precedence over the fallback.

**Test:** `test_new_unknown_class_fallback` — with fallback `"node2d"`, a `Door` rect becomes `Node2D` with `shape` meta of type `Rect2`; warning count == 1 for two `Door` objects.

---

### B9. Tile animations: wrong first frame, half-applied failures, static tile objects

**Status:** verified (first two).

Findings in `TilesetCreator.handle_animation()` (line 406):
- a) Frame 0 is assumed to be the tile that owns the animation. Tile 0 with frames `[1, 2]` animates `0 → 1`.
- b) A non-equidistant sequence (e.g. ping-pong `8, 9, 10, 9`) prints "Skipped" but returns after `set_tile_animation_frames_count` was already called for the first frames, leaving a partial animation.
- c) Tile objects (`gid` objects, `create_object` line 1099) using an animated tile become a static `Sprite2D`.
- d) Durations: Godot supports per-frame durations, Tiled too, so that part is fine.

**Change** — new option `animation_mode` (String, default `"strip"`; `"atlas"` keeps the current behaviour):

`"strip"` mode, in `handle_animation`:
1. Build an `Image` strip of `frames.size()` frames (horizontal) by `Image.blit_rect` from the atlas texture's image (`texture.get_image()`), one blit per frame tile id (any order, repeats allowed).
2. Create a new `TileSetAtlasSource` for the strip (texture_region_size = tile size), one tile at (0,0) with `animation_columns = 0`, `animation_frames_count = n`, per-frame durations.
3. Record a redirect: `_animated_redirect[tile_id] = {source_id, atlas_coords}`; `register_atlas_source` for the gid lookup so `resolve_cell` and `create_object` pick the strip source. The original tile at the atlas coords keeps its collision/metadata; copy `TileData` (collision polygons, custom data, meta, probability, texture_origin) onto the strip tile.
4. Cache strips per `(tsx path, tile id)` so F4's tileset cache reuses them.

In `"atlas"` mode: fix (a) by ignoring a leading frame equal to `tile_id` only when it *is* the tile, otherwise fall back to strip mode with a warning; fix (b) by validating the whole sequence **before** calling any `set_tile_animation_*`.

For (c): in `create_object` gid branch, if the tile is animated (strip or atlas), create an `AnimatedSprite2D` with a `SpriteFrames` built from the same frames (one animation `"default"`, per-frame durations via `set_frame_duration`, `autoplay = "default"`). Keep `Sprite2D` when a `clip_artifacts` property is set (region tricks don't apply to AnimatedSprite2D) and warn.

**Tests:** `test_bug_animation_first_frame` (tile 0 → frames [1,2] shows tile 1 first), `test_bug_animation_pingpong` (8,9,10,9 → 4 frames), `test_new_animated_tile_object` (AnimatedSprite2D with 3 frames, durations 0.1 s).

---

### B10. Smaller fixes

- **B10a. Single-layer maps return the layer as root.** `create()` lines 330–345: a one-layer map returns the `TileMapLayer` itself and applies map properties on top of layer properties (layer wins — `handle_properties` is called twice). Add option `always_wrap_root` (bool, default `false` to keep behaviour; recommend `true` in docs; flip the default in 4.0). When wrapping, map metadata goes on the root Node2D only.
- **B10b. File properties inside templates resolve relative to the map, not the .tx.** Only `res_path` is adjusted (lines 1079–1080, 1197–1198). Generalise: when `obj.has("template_dir_path")`, every property of `type == "file"` (`godot_script`, `material`, `physics_material_override`, `navigation_polygon`, `tile_set`) is joined with `template_dir_path` before `DataLoader.load_resource_from_file`. Implement once in `handle_properties` by passing the base dir as an argument (default `_base_path`).
- **B10c. `clip_artifacts` is looked up as `bool` via `get_property`,** which compares `str(value) == "true"` — JSON maps store a real bool, fine; XML stores the string — fine. No change, but add a test so it stays that way.
- **B10d. `handle_properties` has_children check** (line 2052) only treats bodies/areas; `NavigationRegion2D`/`LightOccluder2D` with `__`-prefixed child forwarding are excluded. Low priority; document.
- **B10e. Hex 120° rotation flag** (gid bit 28) is masked off. Godot 4.5 hex tiles can't rotate 120° natively either; leave documented.

---

## Part B — New features

### F1. Class → scene registry, and exported-property assignment

**Goal:** an object with class `Enemy` becomes `enemy.tscn` with `hp` written into `@export var hp`, with no per-object `res_path` property.

**Options (Importer.gd):**

```gdscript
# Tiled class name (case-insensitive) -> scene path. Objects/tiles of that class are instantiated
# like class 'instance' with res_path. Takes precedence over godot_node_type and the class field mapping.
var class_scenes: Dictionary = {}
# Also accept a 'godot_scene' (file) member on Tiled custom class types (needs the project file).
var read_godot_scene_from_custom_types: bool = true
# Custom properties that match a property on the created node are assigned with set();
# properties that match nothing become metadata (as before). Set false for the old behaviour.
var assign_node_properties: bool = true
```

**Resolution order for a Tiled object/tile** (in `create_object` and the tile-object branch):
1. `godot_node_type` property (unchanged).
2. `class_scenes[class.to_lower()]` → instance.
3. Custom type member `godot_scene` (file) on the class (from `_ct`), if `read_godot_scene_from_custom_types`.
4. Existing `instance` + `res_path` path.
5. Existing class-field mapping (`GODOT_TYPE_MAP`).
6. `unknown_class_fallback` (B8).

Implementation:
- Factor the two existing instance-creation blocks (lines 1073–1097 and 1187–1228) into `func instantiate_scene(res_path, obj, layer_node, pos, rot, scale, alignment) -> Node`.
- `CustomTypes.gd`: add `func get_class_member(class_name, scope, member_name)` to read `godot_scene` from the project file's `propertyTypes`. Resolve the file path relative to the `.tiled-project` file's directory.
- Instances keep the Tiled object's `rotation`, `visible`, size-derived `scale` (tile objects) and `res_alignment` handling.

**Property assignment** (`handle_properties`, final `else` branch ~line 2470, and the generic path):
- Attach `godot_script` **before** iterating other properties (two-pass: first pass only handles `godot_script`; second pass the rest), so script vars exist.
- Build `var prop_names := {}` from `target_node.get_property_list()` once per node (skip when `assign_node_properties` is false).
- In the metadata fallback: `if assign_node_properties and prop_names.has(name): target_node.set(name, typed_value) else set_meta(...)`.
  - Type coercion: Tiled `int` → also accepted for `float`/enum props; `string` → `StringName`/`NodePath` if the property type says so; `file` → `load()`ed resource if the property is a Resource/Object type, else the path string; `class` dictionaries → assigned as Dictionary; `object` → see F2.
  - Name matching is exact (Godot property names are case-sensitive), after the existing `to_lower()` only for YATI's special names.
- After all properties: if the node `has_method("_tiled_import")`, call `_tiled_import({"id": …, "name": …, "class": …, "properties": {name: value}, "rect": Rect2, "shape": …})`.

**Docs:** new Reference section "Scene mapping and node properties" with the resolution order above and the two-pass note.

**Tests:** `test_new_class_scenes` (fixture scene `test/fixtures/enemy.tscn` with `@export var hp: int`, `@export var faction: String`, `@export var patrol: NodePath`; assert instance type, `hp == 10`, no `hp` meta), `test_new_godot_scene_member` (project file with class `Enemy` member `godot_scene`), `test_new_assign_props_off` (option false → metadata as before), `test_new_tiled_import_hook`.

---

### F2. Object references → NodePath (second pass)

**Goal:** a `target` property of type `object` becomes a `NodePath` to the created node (or is assigned to a `NodePath`/`Node` export), instead of the target's geometry.

**Option:** `object_refs = "nodepath"` (String; `"geometry"` keeps today's behaviour; `"both"` writes `NodePath` to the property and geometry to `<name>_shape` meta).

**Implementation:**
1. Maintain `_nodes_by_object_id: Dictionary` — set in every creation site (there are ~12 `set_meta("id", …)` sites; route them through one helper `func register_object_node(obj_id: int, node: Node)` that also handles `add_id_as_metadata`). For bodies register the `CollisionObject2D` (parent), for tile objects with a body the body, otherwise the sprite/instance.
2. During `handle_properties`, when `type == "object"` and mode != geometry, do **not** resolve immediately; append `{node, name, target_id, is_list_item…}` to `_pending_object_refs`.
3. In `create()` after all layers: for each pending ref, `var target = _nodes_by_object_id.get(id)`; if found, `path = node.get_path_to(target)` (both are in the detached tree under `_base_node`, so `get_path_to` works before `add_child` to the scene). Assign: if the node has a property of that name typed `NodePath` → `set(name, path)`; typed `Node`/`Object` → `set(name, target)`; else `set_meta(name, path)`. Unresolved id (0 or missing) → `set_meta(name, NodePath())` + warning.
4. List properties containing objects: resolve each element to a `NodePath` → `Array[NodePath]`.
5. Keep `get_object_polygon` for `"geometry"` and `"both"`.

**Cross-map references** (optional, same workstream): recognise a Tiled custom class `MapRef { map: file, object: int }` by **shape** (members named exactly `map` and `object`): store meta as `{"map": "res://maps/x.tmx", "object": 12}` without resolving; document the convention so a warp system can resolve lazily. No code beyond leaving the dictionary intact (already the case) plus a doc section.

**Tests:** `test_new_object_ref_nodepath` (Door→Spawn: `get_meta("target")` is a `NodePath` that resolves to the Marker2D), `test_new_object_ref_export` (scene with `@export var target: NodePath` receives it), `test_new_object_ref_in_group_layer`, `test_new_object_ref_geometry_mode`.

---

### F3. World files and map bounds

**Goal:** the world streamer reads layout from Tiled's `.world` file; the camera gets exact map bounds.

**F3a. `map_rect` metadata.** In `create()` after layers are handled, compute the pixel bounds:
- Finite map: `Rect2(0, 0, w*tw, h*th)` (isometric: use the transposed diamond bounds — width `(w+h)*tw/2`, height `(w+h)*th/2`).
- Infinite map: union of all chunk rects across all tile layers (`chunk x/y/width/height` in tiles) → pixels.
- Also store `map_size_tiles: Vector2i`, `tile_size: Vector2i`, `orientation: String`.
- Set as metadata on the returned root (even when the root is a single layer, B10a). Also expose `TilemapCreator.get_map_rect()`.

**F3b. `WorldLoader.gd`** (new file, `runtime/WorldLoader.gd`):

```gdscript
# Parses a Tiled .world file. Returns an Array of {"path": String (absolute/res://), "rect": Rect2i}
static func parse(world_file: String) -> Array
```
- Explicit `maps` entries: `fileName`, `x`, `y`, `width`, `height`.
- `patterns` entries: `regexp`, `multiplierX/Y`, `offsetX/Y`; use `RegEx` on every `.tmx/.tmj` in the world file's directory (via `DirAccess`, recursive off by default; `patterns` in Tiled are matched against the file name only). Captures 1 and 2 are x and y indices. `width/height` of a pattern map come from reading the map header (`DictionaryBuilder` on the file, only `width/height/tilewidth/tileheight`) — cache the header read.
- `onlyShowAdjacentMaps` is editor-only; ignore.
- Zip mode: resolve through `DataLoader` like other files.

**F3c. Convenience:** `Importer.import_world(world_file, project_file := "") -> Node2D` that imports every map and positions each root at `rect.position`; mostly for small worlds/tests. The streamer should use `WorldLoader.parse()` directly and `import()` per map on demand.

**Tests:** `test_new_map_rect_finite`, `test_new_map_rect_infinite` (negative chunks), `test_new_world_explicit`, `test_new_world_pattern` (fixtures `world/area_0_0.tmx`, `area_1_0.tmx`, `test.world` with both forms).

---

### F4. Shared TileSet/atlas cache and async import

**F4a. Atlas cache.**
- `Importer.tileset_cache: bool = false` (opt-in; when on, atlases are shared **by reference** across imports on the same `Importer` instance — document that mutating a TileSet from one map affects others).
- Key: resolved absolute path of the `.tsx/.tsj` (embedded tilesets are keyed by `map path + "#" + name`). Value: `{atlas_sources: Array (the registered entries minus firstGid), object_groups, terrain data, strips (B9)}`.
- `TilesetCreator.create_or_append` currently builds one `TileSet` per map containing all sources. Keep that, but when a cache entry exists, `_tileset.add_source(cached_source, forced_id)` instead of rebuilding; re-run only the `firstgid`-dependent registration (`register_atlas_source` with the map's firstgid). `godot_atlas_id` collisions across maps: cached source ids are stable per tsx, so maps that share tsx files get the same source ids — a feature for the streamer (cells are comparable across maps).
- Physics/navigation/occlusion/custom-data **layers** live on the `TileSet`, not the source; they are created per map as now, by name, so cached sources still find their layer indices (`ensure_layer_existing`). Document that layer *order* can differ between maps unless the same tsx set is used.
- `Importer.clear_tileset_cache()`.

**F4b. Shared TileSet resource.** When `save_tileset_to` is set *and* cache is on, reuse the loaded resource for every map instead of saving per map (today each import overwrites the file).

**F4c. Async import.** `Importer.import_async(source_file, project_file := "") -> int` (task id) running `_create` on `WorkerThreadPool.add_task`; `signal import_finished(task_id: int, root: Node)`. The created tree is not in the scene tree so building off-thread is safe, with two caveats to handle:
- `ResourceLoader.load` / `load()` calls (scenes via `res_path`/`class_scenes`, scripts, materials) are thread-safe in Godot 4 but `scene.instantiate()` of scenes containing `@tool` scripts or nodes that touch servers on `_init` may not be; document that instancing happens on the worker and offer `defer_instancing: bool` which records instance requests and performs `instantiate()` + `add_child` on the main thread in the completion callback.
- `ImageTexture.create_from_image` is safe off-thread; `print_rich` is fine.
- Error/warning counters are static on `CommonUtils` → make them per-import (move into a small `ImportContext` RefCounted passed through, or guard with a mutex and per-task maps). Prefer per-context: `TilemapCreator` and `TilesetCreator` already receive the creator instance; add `var ctx` with `error_count/warning_count` and replace the ~60 `CommonUtils.*_count += 1` sites via a helper `ctx.warn(msg)` / `ctx.error(msg)` that also prints.

**Benchmark:** extend `test/benchmark.gd` with cache on/off on a 3-map set sharing one tsx; report ms per import and node/resource counts.

**Tests:** `test_new_tileset_cache_shared_sources` (two maps → same `TileSetAtlasSource` object), `test_new_import_async` (await signal, same tree as sync).

---

### F5. Flag-enum and int values for layer-mask properties

(Prerequisite for X1; small.)

`collision_layer`, `collision_mask`, `platform_floor_layers`, `platform_wall_layers`, `navigation_layers`, `light_mask`, `visibility_layer`, `occluder_light_mask`, `parsed_collision_mask`, and the tileset-level `collision_layer_n`, `collision_mask_n`, `layers_n`, `light_mask_n` currently require `type == "string"` and parse `"1,3,5-7"`.

**Change:** accept `type in ["string", "int"]`; for `int`, use the value as the bitmask directly (this is what a Tiled enum with `valuesAsFlags` + `storageType: int` produces). For enum stored as **string** flags (`"Player,Enemy"`), look the names up in the project file enum (`_ct`) if available and map value index → bit.

**Test:** `test_new_mask_int_and_enum`.

---

## Part C — Tiled extension

### X1. `tiled-extension/yati-godot.js`

A Tiled 1.12 scripting extension, installed by copying into Tiled's extensions folder. Independent of the runtime; communicates only through the `.tiled-project` file and map files.

**X1a. Generate Godot layer enums.** Action *YATI → Sync Godot project…*: user picks `project.godot`; parse `[layer_names]` section (`2d_physics/layer_1="Player"` …, `2d_navigation`, `2d_render`). Create/replace enum property types via `tiled.project` API (`EnumPropertyType`, `valuesAsFlags = true`, `storageType = "int"` — note: confirm the exact property names against the installed Tiled's scripting docs; the class exists since 1.12): `GodotPhysicsLayers`, `GodotNavigationLayers`, `GodotRenderLayers`. Runtime side: F5.

**X1b. `GodotNodeType` enum** with the keys of `GODOT_TYPE_MAP` plus `node2d`, so `godot_node_type` becomes a dropdown. Runtime: already accepts the string; with `storageType: string` nothing changes.

**X1c. Scene → class generation.** Action *YATI → Import scenes as classes…*: user picks a folder; for each `.tscn`, read the attached script's `@export var name: Type = default` lines with a regex (handles `int/float/bool/String/Color/NodePath/Vector2`, `Resource`-typed → `file`, enums → string), create a `ClassPropertyType` named after the scene (or `class_name`), `useAs = ["object","tile"]`, members from the exports, plus member `godot_scene` (file, default = scene path relative to the project file — F1 reads this). Existing members are kept (merge by name) so Tiled-only members survive regeneration. `drawFill`/`color` per class from a stable hash of the name.

**X1d. Save validation.** Hook `tiled.assetAboutToBeSaved`: walk objects; warn via `tiled.warn` (with `activated` callback selecting the object) when: class is unknown to both `GodotNodeType` and the generated scene classes; class `instance` without `res_path`; `res_path`/`godot_scene` pointing at a file that doesn't exist (relative to the map); `object` property pointing at id 0; template instance whose template is missing.

**X1e. World helper.** Action *YATI → Write map_rect comment* is unnecessary (F3 computes it) — skip. Instead: *YATI → Open in Godot* runs `godot --path <project> -e <scene>` via `tiled.executeCommand` for quick checks. Optional.

**Tests:** Tiled extensions have no test harness; provide `tiled-extension/README.md` with a manual checklist and a sample `project.godot` + expected `.tiled-project` diff under `tiled-extension/sample/`.

---

## Documentation and release

- `Reference.md`: new sections — "Options" rows for every option above; "Scene mapping and node properties" (F1); "Object references" (F2); "World files and map bounds" (F3); "Tileset cache / async" (F4); move `parallaxorigin` and animation items out of "Tiled features not supported".
- `Runtime.md`: options table, `import_async`, `import_world`, `WorldLoader.parse`.
- `CHANGELOG.md` `[3.1.0]`: Fixed (B1–B10), Added (F1–F5), Changed (breaking: colour-typed metadata now `Color`; `class` meta no longer set on plain tile layers). Defaults that change behaviour (`parallax_mode`, `animation_mode`, `object_refs`) ship with the **new** behaviour as default only if their legacy mode is kept and documented; otherwise default to legacy and flip in 4.0. Recommendation: parallax2d and strip default on (they fix bugs), `object_refs = "nodepath"` default on (geometry behaviour was surprising), `assign_node_properties` default on, `always_wrap_root` default off until 4.0.
- `test/README.md`: list new fixtures.

## Acceptance checklist

- [ ] `godot --headless -s test/run_tests.gd` passes with 0 fails on 4.3, 4.4, 4.5.
- [ ] `test/benchmark.gd` shows no regression with cache off; ≥ 2× on the second import with cache on.
- [ ] Probe map from this review (`templates + colours + parallax group + object ref + area gravity`) imports with 0 errors and 0 warnings, and the tree matches the expectations listed in B1–B7.
- [ ] Reference/Runtime/CHANGELOG updated; every option appears in both the Importer.gd comment block and the docs.
