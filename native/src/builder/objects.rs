//! Objects (`handle_object`, `create_object` and their helpers, ported): bodies and their
//! shapes, areas, navigation regions, occluders, polygons, lines, paths, points, text, tile
//! objects (sprites, animated sprites, bodies with the tile's collision shapes), scene
//! instances, plain nodes, and template instances merged with their templates.

use super::{Build, GType, Obj};
use crate::files::load_resource;
use crate::gd::{get_basename, get_file, is_absolute_path, path_join, tiled_color, PV};
use crate::props::{get_properties, get_property, godot_node_type_property, gprops, lookup, GProp};
use crate::tileset::Built;
use godot::classes::text_server::AutowrapMode;
use godot::classes::{
    AnimatableBody2D, AnimatedSprite2D, Area2D, AtlasTexture, CanvasItem, CapsuleShape2D,
    CharacterBody2D, CollisionObject2D, CollisionPolygon2D, CollisionShape2D, Control, Curve2D,
    Label, LightOccluder2D, Line2D, Marker2D, Node, Node2D, OccluderPolygon2D, PackedScene, Path2D,
    Polygon2D, RectangleShape2D, RigidBody2D, SegmentShape2D, Shape2D, Sprite2D, SpriteFrames,
    StaticBody2D, SystemFont, TileData, TileSetAtlasSource,
};
use godot::global::{HorizontalAlignment, VerticalAlignment};
use godot::prelude::*;
use std::rc::Rc;
use tiled_core::{Object, Point, Text};

const PI: f64 = std::f64::consts::PI;

/// `ALIGNMENT_UNIT_OFFSETS`.
fn alignment_unit(a: &str) -> Option<Vector2> {
    Some(match a {
        "bottomleft" => Vector2::new(0.5, -0.5),
        "bottom" => Vector2::new(0.0, -0.5),
        "bottomright" => Vector2::new(-0.5, -0.5),
        "left" => Vector2::new(0.5, 0.0),
        "center" => Vector2::new(0.0, 0.0),
        "right" => Vector2::new(-0.5, 0.0),
        "topleft" => Vector2::new(0.5, 0.5),
        "top" => Vector2::new(0.0, 0.5),
        "topright" => Vector2::new(-0.5, 0.5),
        _ => return None,
    })
}

fn sprite_offset(w: f32, h: f32, alignment: &str) -> Vector2 {
    let u = alignment_unit(alignment).unwrap_or(Vector2::new(0.5, -0.5));
    Vector2::new(u.x * w, u.y * h)
}

/// `get_instance_offset`.
fn instance_offset(w: f32, h: f32, r_align: &str, c_align: &str) -> Vector2 {
    let c = alignment_unit(c_align).unwrap_or(Vector2::ZERO);
    let r = alignment_unit(r_align).unwrap_or(Vector2::ZERO);
    Vector2::new(c.x * w - r.x * w, c.y * h - r.y * h)
}

/// `get_position_offset`.
fn position_offset(w: f64, h: f64, rotation: f64) -> Vector2 {
    let (x, y) = (-w / 2.0, -h / 2.0);
    let (s, c) = (rotation * PI / 180.0).sin_cos();
    let (rx, ry) = (x * c - y * s, x * s + y * c);
    Vector2::new((x - rx) as f32, (y - ry) as f32)
}

/// `join_file_path`: a path relative to `dir` (an absolute one stays as it is).
fn join_file_path(dir: &str, path: &str) -> String {
    if dir.is_empty() || path.is_empty() || is_absolute_path(path) {
        return path.to_string();
    }
    path_join(dir, path)
}

fn f(n: Option<tiled_core::Num>, d: f64) -> f64 {
    n.map(|n| n.as_f64()).unwrap_or(d)
}

/// Merges an instance's own text over the template's (`Dictionary.merge(.., true)`).
fn merge_text(t: &mut Text, o: &Text) {
    macro_rules! over {
        ($($f:ident),*) => { $( if o.$f.is_some() { t.$f = o.$f.clone(); } )* };
    }
    over!(
        text, fontfamily, pixelsize, wrap, color, bold, italic, underline, strikeout, kerning,
        halign, valign
    );
    for (k, v) in &o.extra {
        t.extra.insert(k.clone(), v.clone());
    }
}

impl Build<'_> {
    /// `transpose_coords`.
    pub(super) fn transpose(&self, x: f64, y: f64, no_offset_x: bool) -> Vector2 {
        if self.orientation == "isometric" {
            let mut tx = (x - y) * self.tile_w as f64 / self.tile_h as f64 / 2.0;
            if !no_offset_x {
                tx += self.map_h as f64 * self.tile_w as f64 / 2.0;
            }
            let ty = (x + y) * 0.5;
            return Vector2::new(tx as f32, ty as f32);
        }
        Vector2::new(x as f32, y as f32)
    }

    pub(super) fn polygon_from(&self, pts: &[Point]) -> PackedVector2Array {
        pts.iter().map(|p| self.transpose(p.x, p.y, true)).collect()
    }

    fn polygon_from_rectangle(&self, w: f64, h: f64) -> PackedVector2Array {
        PackedVector2Array::from(&[
            Vector2::ZERO,
            self.transpose(0.0, h, true),
            self.transpose(w, h, true),
            self.transpose(w, 0.0, true),
        ])
    }

    fn iso_params(&mut self) -> (f64, f64, Vector2) {
        if let Some(p) = self.iso {
            return p;
        }
        let mut q = self.tile_h as f64 / self.tile_w as f64;
        q *= q;
        let cos_a = (1.0 / (q + 1.0)).sqrt();
        let rot = cos_a.acos() * 180.0 / PI;
        let skew = (90.0 - 2.0 * rot) * PI / 180.0;
        let scale = self.tile_w as f64 / (self.tile_h as f64 * 2.0 * cos_a);
        let p = (rot, skew, Vector2::new(scale as f32, scale as f32));
        self.iso = Some(p);
        p
    }

    /// The end of an object's node: class metadata, the node registered for the object (with
    /// its id metadata), the object's properties, and the node a `_tiled_import` candidate.
    fn finish_object(&mut self, n: &mut Gd<Node>, obj: &Obj, class: &str, id: i64) {
        if self.opts.add_class_as_metadata && !class.is_empty() {
            n.set_meta(&self.meta_class, &class.to_variant());
        }
        self.register(id, n, true);
        self.object_props(n, obj, false);
    }

    /// An object node's properties; then the node is a `_tiled_import` candidate (`scene`: it
    /// is a scene's root).
    fn object_props(&mut self, n: &mut Gd<Node>, obj: &Obj, scene: bool) {
        let scripts = self.scripts_set;
        self.apply_props(n, obj);
        self.hook_candidate(n, obj, scene, scripts);
    }

    fn apply_props(&mut self, n: &mut Gd<Node>, obj: &Obj) {
        if let Some(p) = &obj.props {
            let dir = obj.template_dir.clone().unwrap_or_default();
            self.handle_properties(n, p, &dir);
        }
    }

    /// Whether any class can map to a scene in this import (`class_scenes`, or `godot_scene`
    /// members of the project's classes).
    fn maps_classes_to_scenes(&self) -> bool {
        !self.opts.class_scenes.is_empty()
            || (self.opts.read_godot_scene_from_custom_types && self.map.project().is_some())
    }

    /// `get_class_scene`: the scene path for objects of a Tiled class (F1), resolved, or "":
    /// `class_scenes` (a relative path is relative to the map), else the class's `godot_scene`
    /// file member in the project file (scope `object`, or `tile` for a tile's class) if
    /// `read_godot_scene_from_custom_types`: the object's own `godot_scene` property when it
    /// overrides the member (relative to its file, the map or template), else the member's value
    /// (relative to the project file).
    fn class_scene(&self, obj: &Obj, class: &str, scope: &str) -> String {
        if class.is_empty() {
            return String::new();
        }
        if let Some(mapped) = self.opts.class_scene(class).filter(|m| !m.is_empty()) {
            return join_file_path(&self.base_path, mapped);
        }
        if !self.opts.read_godot_scene_from_custom_types {
            return String::new();
        }
        let Some(project) = self.map.project() else {
            return String::new();
        };
        let find = |scope: &str| {
            project
                .members(class, scope)
                .and_then(|m| m.iter().find(|p| p.name.as_deref() == Some("godot_scene")))
        };
        let member = find(scope).or_else(|| (scope == "tile").then(|| find("object")).flatten());
        let Some(member) = member else {
            return String::new();
        };
        for p in obj.props.iter().flatten() {
            if p.name == "godot_scene" && p.kind == "file" && !p.from_project {
                let own = p.val();
                if own.is_empty() {
                    return String::new();
                }
                let dir = obj.template_dir.as_deref().unwrap_or(&self.base_path);
                return join_file_path(dir, &own);
            }
        }
        let value = GProp::from_property(member, true).val();
        if value.is_empty() {
            return String::new();
        }
        join_file_path(&project.dir(), &value)
    }

    /// `instantiate_scene`: the scene `res_path` for an object (class `instance` with
    /// `res_path`, or a scene for its class), added to `layer_node` at `pos` with the object's
    /// name (else the scene file's), rotation, visibility and `scale` (a tile object's size),
    /// then class metadata, the object's registration and its properties. `replaces`: a node
    /// already made for the object (a tile object's sprite), freed once the scene is loaded.
    #[allow(clippy::too_many_arguments)]
    fn instantiate_scene(
        &mut self,
        res_path: &str,
        obj: &Obj,
        layer_node: &mut Gd<Node>,
        pos: Vector2,
        scale: Vector2,
        class: &str,
        replaces: Option<Gd<Node>>,
    ) {
        let base = self.base_path.clone();
        let Some(res) = load_resource(res_path, &base, &mut self.diag) else {
            return;
        };
        let Ok(scene) = res.try_cast::<PackedScene>() else {
            self.diag
                .error(format!("ERROR: '{res_path}' is not a scene. -> Skipped"));
            return;
        };
        if let Some(mut r) = replaces {
            r.set_owner(Gd::<Node>::null_arg());
            layer_node.remove_child(&r);
            r.free();
        }
        let Some(mut inst) = scene.instantiate() else {
            return;
        };
        layer_node.add_child(&inst);
        self.own(&mut inst);
        let o = &obj.o;
        let name = o.name.clone().unwrap_or_default();
        let n = if name.is_empty() {
            get_basename(get_file(res_path)).to_string()
        } else {
            name
        };
        inst.set_name(n.as_str());
        let rot = f(o.rotation, 0.0) as f32;
        if let Ok(mut n2) = inst.clone().try_cast::<Node2D>() {
            n2.set_position(pos);
            n2.set_rotation_degrees(rot);
            n2.set_scale(scale);
        } else if let Ok(mut c) = inst.clone().try_cast::<Control>() {
            c.set_position(pos);
            c.set_rotation_degrees(rot);
            c.set_scale(scale);
        }
        if let Ok(mut ci) = inst.clone().try_cast::<CanvasItem>() {
            ci.set_visible(o.visible.unwrap_or(true));
        }
        if self.opts.add_class_as_metadata && !class.is_empty() {
            inst.set_meta(&self.meta_class, &class.to_variant());
        }
        self.register(o.id.unwrap_or(0), &inst, true);
        self.object_props(&mut inst, obj, true);
    }

    /// `handle_object`: the object, its opacity applied to whatever got created for it.
    pub(super) fn handle_object(
        &mut self,
        obj: Obj,
        layer_node: &Gd<Node>,
        ts: &Rc<Built>,
        offset: Vector2,
    ) {
        if obj.o.template.is_some() || obj.o.opacity.is_none() {
            self.create_object(obj, layer_node, ts, offset);
            return;
        }
        let opacity = f(obj.o.opacity, 1.0) as f32;
        let before = layer_node.get_child_count();
        self.create_object(obj, layer_node, ts, offset);
        if opacity >= 1.0 {
            return;
        }
        for i in before..layer_node.get_child_count() {
            if let Some(c) = layer_node.get_child(i) {
                if let Ok(mut ci) = c.try_cast::<CanvasItem>() {
                    let mut m = ci.get_modulate();
                    m.a *= opacity;
                    ci.set_modulate(m);
                }
            }
        }
    }

    /// The project's custom type members an object of its class gets (scope `object`).
    fn merge_custom_types(&self, obj: &mut Obj) {
        let Some(project) = self.map.project() else {
            return;
        };
        let class = obj.o.class_name().to_string();
        let Some(members) = project.members(&class, "object") else {
            return;
        };
        for m in members {
            let name = m.name.clone().unwrap_or_default();
            let present = obj.props.iter().flatten().any(|p| p.name == name);
            if present {
                continue;
            }
            obj.props
                .get_or_insert_with(Vec::new)
                .push(GProp::from_property(m, true));
        }
    }

    /// `get_instance_property`: a file property of an instance keeps its path relative to the
    /// map, joined with the map's directory.
    fn instance_property(&self, p: &GProp) -> GProp {
        if p.kind != "file" {
            return p.clone();
        }
        let mut r = p.clone();
        r.value = Some(PV::Core(tiled_core::Value::String(join_file_path(
            &self.base_path,
            &p.val(),
        ))));
        r
    }

    fn create_object(
        &mut self,
        mut obj: Obj,
        layer_node: &Gd<Node>,
        ts: &Rc<Built>,
        offset: Vector2,
    ) {
        self.merge_custom_types(&mut obj);
        let o = &obj.o;
        let id = o.id.unwrap_or(0);
        let x = f(o.x, offset.x as f64);
        let y = f(o.y, offset.y as f64);
        let rot = f(o.rotation, 0.0);
        let mut width = f(o.width, 0.0);
        let mut height = f(o.height, 0.0);
        let visible = o.visible.unwrap_or(true);
        let name = o.name.clone().unwrap_or_default();
        let mut class = o.class_name().to_string();
        let (prop_type, found) = godot_node_type_property(obj.props.as_deref());
        let type_string = if found {
            prop_type.clone()
        } else {
            class.clone()
        };
        let mut gtype = GType::of(&type_string);
        // A scene for the class (class_scenes, or a custom class's godot_scene member) comes
        // after a godot_node_type property and before the class mapping (F1); template
        // instances decide in their template's objects
        let maps_scenes = self.maps_classes_to_scenes();
        let mut scene_path = String::new();
        if maps_scenes && !found && o.template.is_none() {
            scene_path = self.class_scene(&obj, &class, "object");
        }
        if gtype == GType::Unknown && scene_path.is_empty() {
            if found {
                self.diag.warn(format!(
                    " -- Unknown godot_node_type '{type_string}'. -> Using '{}'",
                    self.unknown_fallback
                ));
            } else if self.opts.warn_unknown_classes && self.reported_unknown.insert(class.clone())
            {
                self.diag.warn(format!(
                    " -- Class '{class}' is not a Godot node type -> using '{}'. Set 'godot_node_type', map the class to a scene ('class_scenes') or set the option 'unknown_class_fallback' (see Reference).",
                    self.unknown_fallback
                ));
            }
            gtype = self.unknown_type;
        } else if gtype == GType::Unknown {
            gtype = GType::Empty;
        }

        if let Some(reference) = o.template.clone() {
            self.create_template_instance(
                &obj,
                &reference,
                layer_node,
                ts,
                Vector2::new(x as f32, y as f32),
            );
            return;
        }

        let mut layer_node = layer_node.clone();
        // A scene for the object's class (F1)
        if !scene_path.is_empty() && o.text.is_none() && o.gid.is_none() {
            let pos = self.transpose(x, y, false);
            self.instantiate_scene(
                &scene_path,
                &obj,
                &mut layer_node,
                pos,
                Vector2::ONE,
                &class,
                None,
            );
            return;
        }
        if gtype == GType::Instance && o.text.is_none() && o.gid.is_none() {
            let res_path = get_property(obj.props.as_deref(), "res_path", "file");
            if res_path.is_empty() {
                self.diag.error(
                    "Object of class 'instance': Mandatory file property 'res_path' not found or invalid. -> Skipped",
                );
                return;
            }
            let res_path = match &obj.template_dir {
                Some(d) => join_file_path(d, &res_path),
                None => res_path,
            };
            let pos = self.transpose(x, y, false);
            self.instantiate_scene(
                &res_path,
                &obj,
                &mut layer_node,
                pos,
                Vector2::ONE,
                &class,
                None,
            );
            return;
        }

        if let Some(raw_gid) = o.gid {
            if gtype == GType::Node2D {
                // An unknown class with fallback "node2d": a plain sprite (the tile's class may decide)
                gtype = GType::Empty;
            }
            let flip_h = raw_gid & 0x8000_0000 != 0;
            let flip_v = raw_gid & 0x4000_0000 != 0;
            let gid = raw_gid & 0x0FFF_FFFF;
            let idx = self.atlas_index(gid);
            if idx < 0 {
                return;
            }
            let reg = self.built.sources[idx as usize].clone();
            let source_id = reg.source_id;
            if source_id < 0 {
                return;
            }
            let tile_offset = reg.tile_offset;
            let ts_orientation = reg.orientation.clone();
            let mut alignment = reg.alignment.clone();
            if alignment == "unspecified" {
                alignment = if self.orientation == "orthogonal" {
                    "bottomleft"
                } else {
                    "bottom"
                }
                .into();
            }
            let tileset = ts.tileset.clone();
            if !tileset.has_source(source_id) {
                self.diag.error(format!(
                    "Could not get AtlasSource with id {source_id}. -> Skipped"
                ));
                return;
            }
            let Some(mut src) = tileset
                .get_source(source_id)
                .and_then(|s| s.try_cast::<TileSetAtlasSource>().ok())
            else {
                return;
            };
            let mut sprite = Sprite2D::new_alloc();
            layer_node.add_child(&sprite);
            self.own(&mut sprite);
            let texture = src.get_texture();
            let sprite_name = if !name.is_empty() {
                name.clone()
            } else if !src.get_name().is_empty() {
                src.get_name().to_string()
            } else {
                let p = texture
                    .as_ref()
                    .map(|t| t.get_path().to_string())
                    .unwrap_or_default();
                format!("{}_tile", get_basename(get_file(&p)))
            };
            sprite.set_name(sprite_name.as_str());
            sprite.set_position(
                self.transpose(x, y, false)
                    + Vector2::new(tile_offset.x as f32, tile_offset.y as f32),
            );
            sprite.set_texture(texture.as_ref());
            if rot != 0.0 {
                sprite.set_rotation_degrees(rot as f32);
            }
            if !visible {
                sprite.set_visible(false);
            }
            let clip = get_property(obj.props.as_deref(), "clip_artifacts", "bool") == "true";
            let mut tile_coords = Vector2i::ZERO;
            let td: Gd<TileData>;
            if self.is_partitioned(source_id) {
                let aw = src.get_atlas_grid_size().x as i64;
                if aw <= 0 {
                    return;
                }
                let eg = gid as i64 - self.first_gids[self.first_gid_index(gid)];
                let coords = Vector2i::new((eg % aw) as i32, (eg / aw) as i32);
                if !src.has_tile(coords) {
                    src.create_tile(coords);
                }
                let Some(d) = src.get_tile_data(coords, 0) else {
                    return;
                };
                td = d;
                tile_coords = coords;
                sprite.set_region_enabled(true);
                let rs = src.get_texture_region_size();
                let mut region_size = Vector2::new(rs.x as f32, rs.y as f32);
                let sep = src.get_separation();
                let mar = src.get_margins();
                let mut pos = Vector2::new(
                    coords.x as f32 * (region_size.x + sep.x as f32) + mar.x as f32,
                    coords.y as f32 * (region_size.y + sep.y as f32) + mar.y as f32,
                );
                if clip {
                    pos += Vector2::new(0.5, 0.5);
                    region_size -= Vector2::new(1.0, 1.0);
                    width -= 1.0;
                    height -= 1.0;
                }
                sprite.set_region_rect(Rect2::new(pos, region_size));
                sprite.set_offset(sprite_offset(region_size.x, region_size.y, &alignment));
                if (region_size.x as f64 - width).abs() > 0.01
                    || (region_size.y as f64 - height).abs() > 0.01
                {
                    sprite.set_scale(Vector2::new(
                        (width / region_size.x as f64) as f32,
                        (height / region_size.y as f64) as f32,
                    ));
                }
            } else {
                let rs = src.get_texture_region_size();
                let (gw, gh) = (rs.x, rs.y);
                sprite.set_offset(sprite_offset(gw as f32, gh as f32, &alignment));
                let (tw, th) = texture
                    .as_ref()
                    .map(|t| (t.get_width(), t.get_height()))
                    .unwrap_or((0, 0));
                if gw != tw || gh != th {
                    sprite.set_region_enabled(true);
                    let m = src.get_margins();
                    sprite.set_region_rect(Rect2::new(
                        Vector2::new(m.x as f32, m.y as f32),
                        Vector2::new(gw as f32, gh as f32),
                    ));
                }
                if gw as f64 != width || gh as f64 != height {
                    sprite.set_scale(Vector2::new(
                        (width / gw as f64) as f32,
                        (height / gh as f64) as f32,
                    ));
                }
                let Some(d) = src.get_tile_data(Vector2i::ZERO, 0) else {
                    return;
                };
                td = d;
            }

            let obj_is_instance = gtype == GType::Instance && o.text.is_none();
            let mut tile_class = String::new();
            let tile_has_node_type = td.has_meta("godot_node_type");
            if td.has_meta("class") {
                tile_class = td.get_meta("class").stringify().to_string();
                class = tile_class.clone();
            }
            if tile_has_node_type {
                tile_class = td.get_meta("godot_node_type").stringify().to_string();
            }
            // No scene for the object's own class: the tile's class may have one (F1)
            if scene_path.is_empty()
                && maps_scenes
                && !found
                && !tile_has_node_type
                && !tile_class.is_empty()
                && o.template.is_none()
            {
                scene_path = self.class_scene(&obj, &tile_class, "tile");
            }

            if tile_class.to_lowercase() == "instance" || obj_is_instance || !scene_path.is_empty()
            {
                let found = get_properties(
                    obj.props.as_deref(),
                    &["res_path:file", "res_alignment:string"],
                );
                let mut res_path = scene_path.clone();
                if res_path.is_empty() {
                    res_path = lookup(&found, "res_path:file").to_string();
                    if td.has_meta("res_path") && res_path.is_empty() {
                        res_path = td.get_meta("res_path").stringify().to_string();
                    }
                    if let (false, Some(d)) = (res_path.is_empty(), &obj.template_dir) {
                        res_path = join_file_path(d, &res_path);
                    }
                }
                if res_path.is_empty() {
                    self.diag.error(
                        "Object of class 'instance': Mandatory file property 'res_path' not found or invalid. -> Skipped",
                    );
                    return;
                }
                let ext = crate::gd::get_extension(get_file(&res_path)).to_lowercase();
                let res_alignment = lookup(&found, "res_alignment:string").to_string();
                let mut pos = self.transpose(x, y, false);
                if !res_alignment.is_empty() {
                    pos += instance_offset(width as f32, height as f32, &res_alignment, &alignment);
                } else if ext == "tmx" || ext == "tmj" {
                    pos += instance_offset(width as f32, height as f32, "topleft", &alignment);
                }
                self.convert_metadata(&td, &mut obj);
                let scale = sprite.get_scale();
                self.instantiate_scene(
                    &res_path,
                    &obj,
                    &mut layer_node,
                    pos,
                    scale,
                    &class,
                    Some(sprite.upcast()),
                );
                return;
            }

            // An animated tile is shown animated
            let mut node: Gd<Node2D> = sprite.clone().upcast();
            if src.get_tile_animation_frames_count(tile_coords) > 1 {
                if clip {
                    self.diag.warn(format!(
                        " -- Tile object '{}': 'clip_artifacts' needs a Sprite2D region. -> Not animated",
                        sprite.get_name()
                    ));
                } else {
                    node = self
                        .replace_with_animated_sprite(
                            sprite.clone(),
                            &src,
                            tile_coords,
                            &mut layer_node,
                        )
                        .upcast();
                }
            }

            if !tile_class.is_empty() && gtype == GType::Empty {
                gtype = GType::of(&tile_class);
            }
            self.convert_metadata(&td, &mut obj);

            let group = ts.group_index(source_id, tile_coords);
            if group > 0 {
                let parent: Option<Gd<CollisionObject2D>> = match gtype {
                    GType::Area => Some(Area2D::new_alloc().upcast()),
                    GType::CBody => Some(CharacterBody2D::new_alloc().upcast()),
                    GType::RBody => Some(RigidBody2D::new_alloc().upcast()),
                    GType::ABody => Some(AnimatableBody2D::new_alloc().upcast()),
                    GType::Body => Some(StaticBody2D::new_alloc().upcast()),
                    _ => None,
                };
                if let Some(mut parent) = parent {
                    node.set_owner(Gd::<Node>::null_arg());
                    layer_node.remove_child(&node);
                    layer_node.add_child(&parent);
                    self.own(&mut parent);
                    parent.set_name(&node.get_name());
                    parent.set_position(node.get_position());
                    parent.set_rotation_degrees(node.get_rotation_degrees());
                    node.set_position(Vector2::ZERO);
                    node.set_rotation_degrees(0.0);
                    parent.add_child(&node);
                    self.own(&mut node);
                    let objects = self.built.object_groups.get(&group).cloned();
                    if let Some(objects) = objects {
                        let scale = node.get_scale();
                        self.add_collision_shapes(
                            &mut parent,
                            &objects,
                            width,
                            height,
                            flip_h,
                            flip_v,
                            scale,
                            &ts_orientation,
                        );
                    }
                    // The body stands for the object (its sprite keeps the id metadata)
                    let mut p: Gd<Node> = parent.upcast();
                    self.register(id, &p, false);
                    self.object_props(&mut p, &obj, false);
                }
            }
            node.set("flip_h", &flip_h.to_variant());
            node.set("flip_v", &flip_v.to_variant());
            let mut n: Gd<Node> = node.upcast();
            self.finish_object(&mut n, &obj, &class, id);
        } else if let Some(text) = &o.text {
            let mut label = Label::new_alloc();
            layer_node.add_child(&label);
            self.own(&mut label);
            label.set_name(if name.is_empty() {
                "Text"
            } else {
                name.as_str()
            });
            label.set_position(self.transpose(x, y, false));
            label.set_size(Vector2::new(width as f32, height as f32));
            label.set_clip_text(true);
            if rot != 0.0 {
                label.set_rotation_degrees(rot as f32);
            }
            if !visible {
                label.set_visible(false);
            }
            label.set_text(text.text.as_deref().unwrap_or("Hello World"));
            label.set_autowrap_mode(if text.wrap.unwrap_or(false) {
                AutowrapMode::WORD_SMART
            } else {
                AutowrapMode::OFF
            });
            match text.halign.as_deref().unwrap_or("left") {
                "left" => label.set_horizontal_alignment(HorizontalAlignment::LEFT),
                "center" => label.set_horizontal_alignment(HorizontalAlignment::CENTER),
                "right" => label.set_horizontal_alignment(HorizontalAlignment::RIGHT),
                "justify" => label.set_horizontal_alignment(HorizontalAlignment::FILL),
                _ => {}
            }
            match text.valign.as_deref().unwrap_or("top") {
                "top" => label.set_vertical_alignment(VerticalAlignment::TOP),
                "center" => label.set_vertical_alignment(VerticalAlignment::CENTER),
                "bottom" => label.set_vertical_alignment(VerticalAlignment::BOTTOM),
                _ => {}
            }
            let family = text
                .fontfamily
                .clone()
                .unwrap_or_else(|| "Sans-Serif".into());
            let mut font = SystemFont::new_gd();
            let names: PackedStringArray = [GString::from(family.as_str())].into_iter().collect();
            font.set_font_names(&names);
            font.set_oversampling(5.0);
            if text.bold.unwrap_or(false) {
                font.set_font_weight(700);
            }
            if text.italic.unwrap_or(false) {
                font.set_font_italic(true);
            }
            label.add_theme_font_override("font", &font);
            label.add_theme_font_size_override("font_size", text.pixelsize.unwrap_or(16) as i32);
            let color = text.color.clone().unwrap_or_else(|| "#000000".into());
            label.add_theme_color_override("font_color", tiled_color(&color, Color::BLACK));
            let mut n: Gd<Node> = label.upcast();
            self.register(id, &n, true);
            self.object_props(&mut n, &obj, false);
        } else {
            if gtype == GType::Empty {
                gtype = GType::Body;
            }
            let base_coords = self.transpose(x, y, false);
            if gtype == GType::Node2D && o.point.is_none() {
                self.create_plain_node(&obj, &mut layer_node, base_coords, &class);
            } else if o.point.is_some() {
                let mut marker = Marker2D::new_alloc();
                layer_node.add_child(&marker);
                self.own(&mut marker);
                marker.set_name(if name.is_empty() {
                    "point"
                } else {
                    name.as_str()
                });
                marker.set_position(base_coords);
                if rot != 0.0 {
                    marker.set_rotation_degrees(rot as f32);
                }
                if !visible {
                    marker.set_visible(false);
                }
                let mut n: Gd<Node> = marker.upcast();
                self.finish_object(&mut n, &obj, &class, id);
            } else if let Some(points) = &o.polygon {
                self.create_polygon_object(
                    &obj,
                    gtype,
                    points,
                    &mut layer_node,
                    base_coords,
                    &name,
                    &class,
                    id,
                    rot,
                    visible,
                );
            } else if let Some(points) = &o.polyline {
                self.create_polyline_object(
                    &obj,
                    gtype,
                    points,
                    &mut layer_node,
                    base_coords,
                    &name,
                    &class,
                    id,
                    rot,
                    visible,
                    width,
                    height,
                );
            } else {
                self.create_shape_object(
                    &obj,
                    gtype,
                    &mut layer_node,
                    base_coords,
                    &name,
                    &class,
                    id,
                    rot,
                    visible,
                    width,
                    height,
                );
            }
        }
    }

    /// A template instance: each template object with the instance's overrides.
    fn create_template_instance(
        &mut self,
        obj: &Obj,
        reference: &str,
        layer_node: &Gd<Node>,
        ts: &Rc<Built>,
        pos: Vector2,
    ) {
        let map = self.map;
        // A missing template was reported by the loader (once per instance, as the runtime does)
        let Some(template) = map.template(reference) else {
            return;
        };
        let zip = self.zip.clone();
        let opts = self.opts.clone();
        let template_ts =
            self.tilesets
                .built_for_template(map, template, &zip, &opts, &mut self.diag);
        let tjson = template.format() == tiled_core::Format::Json;
        let dir = template.base_dir();
        let o = &obj.o;
        for t in &template.objects {
            let mut m: Object = t.clone();
            let mut props = gprops(&t.properties, tjson);
            if let Some(iprops) = &obj.props {
                match &mut props {
                    Some(tprops) => {
                        for p in iprops {
                            let inst = self.instance_property(p);
                            match tprops.iter_mut().find(|tp| tp.name == p.name) {
                                Some(tp) => tp.value = inst.value,
                                None => tprops.push(inst),
                            }
                        }
                    }
                    None => {
                        props = Some(iprops.iter().map(|p| self.instance_property(p)).collect())
                    }
                }
            }
            m.id = Some(o.id.unwrap_or(0));
            for (dst, src) in [
                (&mut m.class, &o.class),
                (&mut m.kind, &o.kind),
                (&mut m.name, &o.name),
            ] {
                if let Some(s) = src.as_deref().filter(|s| !s.is_empty()) {
                    *dst = Some(s.to_string());
                }
            }
            if o.opacity.is_some() {
                m.opacity = o.opacity;
            }
            // Attributes saved on the instance override the template's
            if o.width.is_some() {
                m.width = o.width;
            }
            if o.height.is_some() {
                m.height = o.height;
            }
            if o.rotation.is_some() {
                m.rotation = o.rotation;
            }
            if o.visible.is_some() {
                m.visible = o.visible;
            }
            if let Some(it) = &o.text {
                match &mut m.text {
                    Some(tt) => merge_text(tt, it),
                    None => m.text = Some(it.clone()),
                }
            }
            // An instance's own gid (a flipped instance saves one) is one of the map's tilesets
            let mut obj_ts = template_ts.clone().unwrap_or_else(|| empty_built(ts));
            if o.gid.is_some() {
                m.gid = o.gid;
                obj_ts = self.built.clone();
            }
            let child = Obj {
                o: m,
                props,
                template_dir: Some(dir.clone()),
            };
            self.handle_object(child, layer_node, &obj_ts, pos);
        }
    }

    /// `convert_metadata_to_obj_properties`: the tile's metadata as properties of a tile
    /// object, where the object has none of that name.
    fn convert_metadata(&self, td: &Gd<TileData>, obj: &mut Obj) {
        for m in td.get_meta_list().iter_shared() {
            let name = m.to_string();
            let lname = name.to_lowercase();
            if lname == "godot_node_type" {
                continue;
            }
            if lname == "class" && !self.opts.add_class_as_metadata {
                continue;
            }
            let v = td.get_meta(&m);
            let mut kind = match v.get_type() {
                VariantType::BOOL => "bool",
                VariantType::INT => "int",
                VariantType::STRING => "string",
                VariantType::FLOAT => "float",
                VariantType::COLOR => "color",
                _ => "string",
            }
            .to_string();
            if ["godot_script", "material", "physics_material_override"].contains(&lname.as_str()) {
                kind = "file".into();
            }
            let prop = GProp::new(name.clone(), kind, Some(PV::Var(v)));
            match &mut obj.props {
                Some(props) => {
                    if !props.iter().any(|p| p.name.to_lowercase() == lname) {
                        props.push(prop);
                    }
                }
                None => obj.props = Some(vec![prop]),
            }
        }
    }

    /// `replace_with_animated_sprite`.
    fn replace_with_animated_sprite(
        &mut self,
        mut sprite: Gd<Sprite2D>,
        src: &Gd<TileSetAtlasSource>,
        coords: Vector2i,
        layer_node: &mut Gd<Node>,
    ) -> Gd<AnimatedSprite2D> {
        let mut frames = SpriteFrames::new_gd();
        frames.set_animation_speed("default", 1.0);
        frames.set_animation_loop("default", true);
        for i in 0..src.get_tile_animation_frames_count(coords) {
            let mut tex = AtlasTexture::new_gd();
            tex.set_atlas(src.get_texture().as_ref());
            let r = src.get_tile_texture_region_ex(coords).frame(i).done();
            tex.set_region(Rect2::new(
                Vector2::new(r.position.x as f32, r.position.y as f32),
                Vector2::new(r.size.x as f32, r.size.y as f32),
            ));
            frames
                .add_frame_ex("default", &tex)
                .duration(src.get_tile_animation_frame_duration(coords, i))
                .done();
        }
        let mut anim = AnimatedSprite2D::new_alloc();
        anim.set_sprite_frames(&frames);
        anim.set_animation("default");
        anim.set_autoplay("default");
        anim.set_position(sprite.get_position());
        anim.set_rotation_degrees(sprite.get_rotation_degrees());
        anim.set_scale(sprite.get_scale());
        anim.set_offset(sprite.get_offset());
        anim.set_centered(sprite.is_centered());
        anim.set_visible(sprite.is_visible());
        let name = sprite.get_name();
        let index = sprite.get_index();
        sprite.set_owner(Gd::<Node>::null_arg());
        layer_node.remove_child(&sprite);
        sprite.free();
        layer_node.add_child(&anim);
        layer_node.move_child(&anim, index);
        self.own(&mut anim);
        anim.set_name(&name);
        anim
    }

    fn create_plain_node(
        &mut self,
        obj: &Obj,
        layer_node: &mut Gd<Node>,
        position: Vector2,
        class: &str,
    ) {
        let o = &obj.o;
        let id = o.id.unwrap_or(0);
        let name = o.name.clone().unwrap_or_default();
        let w = f(o.width, 0.0) as f32;
        let h = f(o.height, 0.0) as f32;
        let mut node = Node2D::new_alloc();
        layer_node.add_child(&node);
        self.own(&mut node);
        let n = if !name.is_empty() {
            name
        } else if !class.is_empty() {
            class.to_string()
        } else {
            "Node2D".into()
        };
        node.set_name(n.as_str());
        node.set_position(position);
        node.set_rotation_degrees(f(o.rotation, 0.0) as f32);
        node.set_visible(o.visible.unwrap_or(true));
        if let Some(p) = &o.polygon {
            node.set_meta("shape", &self.polygon_from(p).to_variant());
        } else if let Some(p) = &o.polyline {
            node.set_meta("shape", &self.polygon_from(p).to_variant());
        } else {
            node.set_meta(
                "shape",
                &Rect2::new(Vector2::ZERO, Vector2::new(w, h)).to_variant(),
            );
        }
        node.set_meta("size", &Vector2::new(w, h).to_variant());
        let mut n: Gd<Node> = node.upcast();
        self.finish_object(&mut n, obj, class, id);
    }

    /// `create_collision_object`: the body of a shape object, added to the layer.
    fn create_collision_object(
        &mut self,
        gtype: GType,
        name: &str,
        layer_node: &mut Gd<Node>,
    ) -> Gd<CollisionObject2D> {
        let (mut co, suffix, default): (Gd<CollisionObject2D>, &str, &str) = match gtype {
            GType::Area => (Area2D::new_alloc().upcast(), " (Area)", "Area"),
            GType::ABody => (
                AnimatableBody2D::new_alloc().upcast(),
                " (AB)",
                "AnimatableBody",
            ),
            GType::CBody => (
                CharacterBody2D::new_alloc().upcast(),
                " (CB)",
                "CharacterBody",
            ),
            GType::RBody => (RigidBody2D::new_alloc().upcast(), " (RB)", "RigidBody"),
            _ => (StaticBody2D::new_alloc().upcast(), " (SB)", "StaticBody"),
        };
        layer_node.add_child(&co);
        if name.is_empty() {
            co.set_name(default);
        } else {
            co.set_name(format!("{name}{suffix}").as_str());
        }
        co
    }

    fn nav_region(
        &mut self,
        layer_node: &mut Gd<Node>,
        name: &str,
        pos: Vector2,
        rot: f64,
        visible: bool,
        pg: PackedVector2Array,
    ) -> Gd<Node2D> {
        let mut nr = crate::dynobj::new_navigation_region();
        layer_node.add_child(&nr);
        self.own(&mut nr);
        nr.set_name(
            if name.is_empty() {
                "Navigation".to_string()
            } else {
                format!("{name} (NR)")
            }
            .as_str(),
        );
        nr.set_position(pos);
        if rot != 0.0 {
            nr.set_rotation_degrees(rot as f32);
        }
        if !visible {
            nr.set_visible(false);
        }
        let np = crate::dynobj::new_navigation_polygon(&pg);
        crate::dynobj::set(&mut nr, "navigation_polygon", np);
        nr
    }

    fn occluder(
        &mut self,
        layer_node: &mut Gd<Node>,
        name: &str,
        pos: Vector2,
        rot: f64,
        visible: bool,
        pg: PackedVector2Array,
    ) -> Gd<LightOccluder2D> {
        let mut lo = LightOccluder2D::new_alloc();
        layer_node.add_child(&lo);
        self.own(&mut lo);
        lo.set_name(
            if name.is_empty() {
                "Occluder".to_string()
            } else {
                format!("{name} (LO)")
            }
            .as_str(),
        );
        lo.set_position(pos);
        if rot != 0.0 {
            lo.set_rotation_degrees(rot as f32);
        }
        if !visible {
            lo.set_visible(false);
        }
        let mut op = OccluderPolygon2D::new_gd();
        lo.set_occluder_polygon(&op);
        op.set_polygon(&pg);
        lo
    }

    fn polygon2d(
        &mut self,
        layer_node: &mut Gd<Node>,
        name: &str,
        pos: Vector2,
        rot: f64,
        visible: bool,
        pg: PackedVector2Array,
    ) -> Gd<Polygon2D> {
        let mut p = Polygon2D::new_alloc();
        layer_node.add_child(&p);
        self.own(&mut p);
        p.set_name(if name.is_empty() { "Polygon" } else { name });
        p.set_position(pos);
        if rot != 0.0 {
            p.set_rotation_degrees(rot as f32);
        }
        if !visible {
            p.set_visible(false);
        }
        p.set_polygon(&pg);
        p
    }

    #[allow(clippy::too_many_arguments)]
    fn create_polygon_object(
        &mut self,
        obj: &Obj,
        gtype: GType,
        points: &[Point],
        layer_node: &mut Gd<Node>,
        pos: Vector2,
        name: &str,
        class: &str,
        id: i64,
        rot: f64,
        visible: bool,
    ) {
        let pg = self.polygon_from(points);
        let mut node: Gd<Node> = if gtype.is_collision_object() {
            let mut co = self.create_collision_object(gtype, name, layer_node);
            self.own(&mut co);
            co.set_position(pos);
            if !visible {
                co.set_visible(false);
            }
            let mut cp = CollisionPolygon2D::new_alloc();
            cp.set_polygon(&pg);
            co.add_child(&cp);
            self.own(&mut cp);
            cp.set_name(if name.is_empty() {
                "Polygon Shape"
            } else {
                name
            });
            cp.set_position(Vector2::ZERO);
            if rot != 0.0 {
                cp.set_rotation_degrees(rot as f32);
            }
            co.upcast()
        } else if gtype == GType::Navigation {
            self.nav_region(layer_node, name, pos, rot, visible, pg)
                .upcast()
        } else if gtype == GType::Occluder {
            self.occluder(layer_node, name, pos, rot, visible, pg)
                .upcast()
        } else if gtype == GType::Polygon {
            self.polygon2d(layer_node, name, pos, rot, visible, pg)
                .upcast()
        } else {
            return;
        };
        self.finish_object(&mut node, obj, class, id);
    }

    #[allow(clippy::too_many_arguments)]
    fn create_polyline_object(
        &mut self,
        obj: &Obj,
        gtype: GType,
        points: &[Point],
        layer_node: &mut Gd<Node>,
        pos: Vector2,
        name: &str,
        class: &str,
        id: i64,
        rot: f64,
        visible: bool,
        w: f64,
        h: f64,
    ) {
        let mut node: Gd<Node> = if gtype == GType::Line {
            let mut line = Line2D::new_alloc();
            layer_node.add_child(&line);
            self.own(&mut line);
            line.set_name(if name.is_empty() { "Line" } else { name });
            line.set_position(pos);
            if !visible {
                line.set_visible(false);
            }
            if rot != 0.0 {
                line.set_rotation_degrees(rot as f32);
            }
            line.set_width(1.0);
            line.set_points(&self.polygon_from(points));
            line.upcast()
        } else if gtype == GType::Path {
            let mut path = Path2D::new_alloc();
            layer_node.add_child(&path);
            self.own(&mut path);
            path.set_name(if name.is_empty() { "Path" } else { name });
            path.set_position(pos);
            if !visible {
                path.set_visible(false);
            }
            if rot != 0.0 {
                path.set_rotation_degrees(rot as f32);
            }
            let mut curve = Curve2D::new_gd();
            for p in points {
                curve.add_point(Vector2::new(p.x as f32, p.y as f32));
            }
            path.set_curve(&curve);
            path.upcast()
        } else {
            let mut co = self.create_collision_object(gtype, name, layer_node);
            self.own(&mut co);
            co.set_position(pos);
            if !visible {
                co.set_visible(false);
            }
            for i in 0..points.len().saturating_sub(1) {
                let mut cs = CollisionShape2D::new_alloc();
                co.add_child(&cs);
                let mut seg = SegmentShape2D::new_gd();
                seg.set_a(self.transpose(points[i].x, points[i].y, true));
                seg.set_b(self.transpose(points[i + 1].x, points[i + 1].y, true));
                self.own(&mut cs);
                cs.set_shape(&seg);
                cs.set_position(Vector2::new((w / 2.0) as f32, (h / 2.0) as f32));
                if rot != 0.0 {
                    cs.set_rotation_degrees(rot as f32);
                }
                cs.set_name("Segment Shape");
            }
            co.upcast()
        };
        self.finish_object(&mut node, obj, class, id);
    }

    #[allow(clippy::too_many_arguments)]
    fn create_shape_object(
        &mut self,
        obj: &Obj,
        gtype: GType,
        layer_node: &mut Gd<Node>,
        pos: Vector2,
        name: &str,
        class: &str,
        id: i64,
        rot: f64,
        visible: bool,
        w: f64,
        h: f64,
    ) {
        let o = &obj.o;
        let round = o.capsule.is_some() || o.ellipse.is_some();
        let mut node: Gd<Node> = if gtype.is_collision_object() {
            let mut co = self.create_collision_object(gtype, name, layer_node);
            self.own(&mut co);
            co.set_position(pos);
            if !visible {
                co.set_visible(false);
            }
            let mut cs = CollisionShape2D::new_alloc();
            co.add_child(&cs);
            self.own(&mut cs);
            let mut r = rot;
            let is_rect = !round;
            if round {
                let mut cap = CapsuleShape2D::new_gd();
                if h >= w {
                    cap.set_height(h as f32);
                    cap.set_radius((w / 2.0) as f32);
                } else {
                    cap.set_height(w as f32);
                    cap.set_radius((h / 2.0) as f32);
                    r += 90.0;
                }
                cs.set_shape(&cap);
                cs.set_name(if name.is_empty() {
                    "Capsule Shape"
                } else {
                    name
                });
            } else {
                let mut rect = RectangleShape2D::new_gd();
                rect.set_size(Vector2::new(w as f32, h as f32));
                cs.set_shape(&rect);
                cs.set_name(if name.is_empty() {
                    "Rectangle Shape"
                } else {
                    name
                });
            }
            if self.orientation == "isometric" {
                let (irot, iskew, iscale) = self.iso_params();
                if h >= w || is_rect {
                    cs.set_skew(iskew as f32);
                    r += irot;
                } else {
                    cs.set_skew(-iskew as f32);
                    r += -90.0 - irot;
                }
                cs.set_scale(iscale);
            }
            cs.set_position(self.transpose(w / 2.0, h / 2.0, true) + position_offset(w, h, rot));
            if r != 0.0 {
                cs.set_rotation_degrees(r as f32);
            }
            if !visible {
                cs.set_visible(false);
            }
            co.upcast()
        } else if gtype == GType::Navigation || gtype == GType::Occluder || gtype == GType::Polygon
        {
            let what = match gtype {
                GType::Navigation => "NavigationRegion2D",
                GType::Occluder => "LightOccluder2D",
                _ => "Polygon2D",
            };
            if o.capsule.is_some() {
                self.diag
                    .warn(format!(" -- Capsule is unusable for {what}. -> Skipped"));
                return;
            }
            if o.ellipse.is_some() {
                self.diag
                    .warn(format!(" -- Ellipse is unusable for {what}. -> Skipped"));
                return;
            }
            let pg = self.polygon_from_rectangle(w, h);
            match gtype {
                GType::Navigation => self
                    .nav_region(layer_node, name, pos, rot, visible, pg)
                    .upcast(),
                GType::Occluder => self
                    .occluder(layer_node, name, pos, rot, visible, pg)
                    .upcast(),
                _ => self
                    .polygon2d(layer_node, name, pos, rot, visible, pg)
                    .upcast(),
            }
        } else {
            return;
        };
        self.finish_object(&mut node, obj, class, id);
    }

    /// `add_collision_shapes`: a tile's collision objects as shapes of a tile object's body.
    #[allow(clippy::too_many_arguments)]
    fn add_collision_shapes(
        &mut self,
        parent: &mut Gd<CollisionObject2D>,
        objects: &[Object],
        tile_width: f64,
        tile_height: f64,
        flip_h: bool,
        flip_v: bool,
        scale: Vector2,
        ts_orientation: &str,
    ) {
        let (mut tile_width, mut tile_height) = (tile_width, tile_height);
        let iso_ts = ts_orientation == "isometric";
        let (sx, sy) = (scale.x as f64, scale.y as f64);
        for obj in objects {
            let name = obj.name.clone().unwrap_or_default();
            if obj.point == Some(true) {
                self.diag
                    .warn(" -- 'Point' has currently no corresponding collision element in Godot 4. -> Skipped");
                break;
            }
            let fact = tile_height / self.tile_h as f64;
            let ox = f(obj.x, 0.0);
            let oy = f(obj.y, 0.0);
            let mut base = Vector2::new((ox * sx) as f32, (oy * sy) as f32);
            if iso_ts {
                let t = self.transpose(ox, oy, true);
                base = Vector2::new(t.x * scale.x, t.y * scale.y);
                tile_width = self.tile_w as f64;
                tile_height = self.tile_h as f64;
            }
            let props = gprops(&obj.properties, self.json);
            let one_way = get_properties(
                props.as_deref(),
                &["one_way:bool", "one_way_margin:int", "one_way_margin:float"],
            );
            let margin = {
                let m = lookup(&one_way, "one_way_margin:int");
                if m.is_empty() {
                    lookup(&one_way, "one_way_margin:float")
                } else {
                    m
                }
            }
            .to_string();
            let is_one_way = lookup(&one_way, "one_way:bool") == "true";
            if let Some(points) = &obj.polygon {
                let mut rot = f(obj.rotation, 0.0);
                let polygon: PackedVector2Array = points
                    .iter()
                    .map(|p| {
                        let mut c = Vector2::new((p.x * sx) as f32, (p.y * sy) as f32);
                        if iso_ts {
                            c = self.transpose(c.x as f64, c.y as f64, true);
                        }
                        if flip_h {
                            c.x = -c.x;
                        }
                        if flip_v {
                            c.y = -c.y;
                        }
                        c
                    })
                    .collect();
                let mut cp = CollisionPolygon2D::new_alloc();
                parent.add_child(&cp);
                self.own(&mut cp);
                cp.set_polygon(&polygon);
                let mut px = base.x as f64;
                let mut py = base.y as f64 - tile_height;
                if self.orientation == "isometric" && ts_orientation == "orthogonal" {
                    px -= tile_width / 2.0;
                }
                if flip_h {
                    px = tile_width - px;
                    if self.orientation == "isometric" {
                        px -= tile_width;
                    }
                    rot = -rot;
                }
                if flip_v {
                    py = -tile_height - py;
                    if iso_ts {
                        py -= self.tile_h as f64 * fact - tile_height;
                    }
                    rot = -rot;
                }
                if rot != 0.0 {
                    cp.set_rotation_degrees(rot as f32);
                }
                cp.set_position(Vector2::new(px as f32, py as f32));
                cp.set_name(if name.is_empty() {
                    "Collision Polygon"
                } else {
                    name.as_str()
                });
                if is_one_way {
                    cp.set_one_way_collision(true);
                }
                if !margin.is_empty() {
                    cp.set_one_way_collision_margin(crate::gd::to_float(&margin) as f32);
                }
            } else {
                let mut cs = CollisionShape2D::new_alloc();
                parent.add_child(&cs);
                self.own(&mut cs);
                let x = ox * sx;
                let y = oy * sy;
                let w = f(obj.width, 0.0) * sx;
                let h = f(obj.height, 0.0) * sy;
                let mut rot = f(obj.rotation, 0.0);
                let (sin_a, cos_a) = (rot * PI / 180.0).sin_cos();
                let mut px = x + w / 2.0 * cos_a - h / 2.0 * sin_a;
                let mut py = -tile_height + y + h / 2.0 * cos_a + w / 2.0 * sin_a;
                if iso_ts {
                    let t = self.transpose(px, py, true);
                    px = t.x as f64;
                    py = t.y as f64;
                    px -= tile_width / 2.0 - h * fact / 4.0 * sin_a;
                    py -= tile_height / 2.0;
                } else if self.orientation == "isometric" {
                    px -= tile_width / 2.0;
                }
                if flip_h {
                    px = tile_width - px;
                    if self.orientation == "isometric" {
                        px -= tile_width;
                    }
                    rot = -rot;
                }
                if flip_v {
                    py = -tile_height - py;
                    if iso_ts {
                        py -= self.tile_h as f64 * fact - tile_height;
                    }
                    rot = -rot;
                }
                cs.set_position(Vector2::new(px as f32, py as f32));
                cs.set_scale(scale);
                let capsule = obj.capsule == Some(true) || obj.ellipse == Some(true);
                let shape: Gd<Shape2D> = if capsule {
                    let mut c = CapsuleShape2D::new_gd();
                    if h >= w {
                        c.set_height((h / sy) as f32);
                        c.set_radius((w / 2.0 / sx) as f32);
                    } else {
                        c.set_height((w / sy) as f32);
                        c.set_radius((h / 2.0 / sx) as f32);
                        rot += 90.0;
                    }
                    cs.set_name(if name.is_empty() {
                        "Capsule Shape"
                    } else {
                        name.as_str()
                    });
                    c.upcast()
                } else {
                    let mut r = RectangleShape2D::new_gd();
                    r.set_size(Vector2::new((w / sx) as f32, (h / sy) as f32));
                    cs.set_name(if name.is_empty() {
                        "Rectangle Shape"
                    } else {
                        name.as_str()
                    });
                    r.upcast()
                };
                if iso_ts {
                    let (irot, iskew, iscale) = self.iso_params();
                    let (mut er, mut es) = (irot, iskew);
                    if w > h && capsule {
                        er = -irot;
                        es = -iskew;
                        rot -= 90.0;
                    }
                    if flip_h {
                        er = -er;
                        es = -es;
                    }
                    if flip_v {
                        er = -er;
                        es = -es;
                    }
                    cs.set_skew(es as f32);
                    cs.set_scale(iscale);
                    rot += er;
                }
                cs.set_shape(&shape);
                if rot != 0.0 {
                    cs.set_rotation_degrees(rot as f32);
                }
                if is_one_way {
                    cs.set_one_way_collision(true);
                }
                if !margin.is_empty() {
                    cs.set_one_way_collision_margin(crate::gd::to_float(&margin) as f32);
                }
            }
        }
    }

    /// `get_object_polygon`: the shape of a map object (by id) in map coordinates, for object
    /// properties: polygon or polyline points, a point, or a rectangle's corners.
    pub(super) fn object_polygon(&mut self, id: i64) -> Option<Variant> {
        if self.objects_by_id.is_none() {
            let mut idx = std::collections::HashMap::new();
            for l in self.map.all_layers() {
                for o in l.objects.iter().flatten() {
                    idx.entry(o.id.unwrap_or(0)).or_insert(o);
                }
            }
            self.objects_by_id = Some(idx);
        }
        let o = *self.objects_by_id.as_ref()?.get(&id)?;
        let x = f(o.x, 0.0) as f32;
        let y = f(o.y, 0.0) as f32;
        let rot = (f(o.rotation, 0.0) * PI / 180.0) as f32;
        let shape = |pts: PackedVector2Array| -> Variant {
            let out: PackedVector2Array = pts
                .as_slice()
                .iter()
                .map(|v| v.rotated(rot) + Vector2::new(x, y))
                .collect();
            out.to_variant()
        };
        if let Some(p) = &o.polygon {
            Some(shape(self.polygon_from(p)))
        } else if let Some(p) = &o.polyline {
            Some(shape(self.polygon_from(p)))
        } else if o.point.is_some() {
            Some(Vector2::new(x, y).to_variant())
        } else if o.ellipse.is_none() && o.capsule.is_none() && o.gid.is_none() {
            let w = f(o.width, 0.0) as f32;
            let h = f(o.height, 0.0) as f32;
            Some(shape(PackedVector2Array::from(&[
                Vector2::new(0.0, 0.0),
                Vector2::new(w, 0.0),
                Vector2::new(w, h),
                Vector2::new(0.0, h),
            ])))
        } else {
            None
        }
    }
}

/// A Built with no sources: what a template without tilesets gives its tile objects.
fn empty_built(_map_ts: &Rc<Built>) -> Rc<Built> {
    Rc::new(Built {
        tileset: godot::classes::TileSet::new_gd(),
        sources: Vec::new(),
        object_groups: Default::default(),
        internal: Default::default(),
    })
}
