//! `handle_properties`, ported: Tiled properties to node properties (by name, type and node
//! class, in the runtime's order: the first matching rule wins), groups, scripts, and
//! everything else as metadata.

use super::Build;
use crate::files::load_resource;
use crate::gd::{
    bitmask, cleanup_path, gd_str, is_absolute_path, property_color, to_float, to_int, typed_value,
    PV,
};
use crate::props::GProp;
use godot::classes::{Node, Resource, ResourceLoader};
use godot::prelude::*;
use tiled_core::Value;

/// What a node is, for the rules (`target_node is ...`), checked once per node.
struct Kind {
    tml: bool,
    p2d: bool,
    co: bool,
    area: bool,
    sb: bool,
    ab: bool,
    cb: bool,
    rb: bool,
    nr: bool,
    lo: bool,
    poly: bool,
    line: bool,
    marker: bool,
}

impl Kind {
    fn none() -> Kind {
        Kind {
            tml: false,
            p2d: false,
            co: false,
            area: false,
            sb: false,
            ab: false,
            cb: false,
            rb: false,
            nr: false,
            lo: false,
            poly: false,
            line: false,
            marker: false,
        }
    }

    /// The node's flags from its class name (one engine call); a class this table does not
    /// know (a GDExtension class, say) is asked with `is_class` for each.
    fn of(n: &Gd<Node>) -> Kind {
        let mut k = Kind::none();
        match n.get_class().to_string().as_str() {
            "TileMapLayer" => k.tml = true,
            "Parallax2D" => k.p2d = true,
            "StaticBody2D" => (k.co, k.sb) = (true, true),
            "AnimatableBody2D" => (k.co, k.sb, k.ab) = (true, true, true),
            "CharacterBody2D" => (k.co, k.cb) = (true, true),
            "RigidBody2D" => (k.co, k.rb) = (true, true),
            "Area2D" => (k.co, k.area) = (true, true),
            "NavigationRegion2D" => k.nr = true,
            "LightOccluder2D" => k.lo = true,
            "Polygon2D" => k.poly = true,
            "Line2D" => k.line = true,
            "Marker2D" => k.marker = true,
            "Node" | "Node2D" | "Sprite2D" | "AnimatedSprite2D" | "Label" | "TextureRect"
            | "Path2D" | "ColorRect" | "ParallaxLayer" | "ParallaxBackground"
            | "CollisionShape2D" | "CollisionPolygon2D" => {}
            _ => {
                let is = |c: &str| n.is_class(c);
                k = Kind {
                    tml: is("TileMapLayer"),
                    p2d: is("Parallax2D"),
                    co: is("CollisionObject2D"),
                    area: is("Area2D"),
                    sb: is("StaticBody2D"),
                    ab: is("AnimatableBody2D"),
                    cb: is("CharacterBody2D"),
                    rb: is("RigidBody2D"),
                    nr: is("NavigationRegion2D"),
                    lo: is("LightOccluder2D"),
                    poly: is("Polygon2D"),
                    line: is("Line2D"),
                    marker: is("Marker2D"),
                };
            }
        }
        k
    }
}

fn set(n: &mut Gd<Node>, prop: &str, v: Variant) {
    n.set(prop, &v);
}

fn vec_part(n: &mut Gd<Node>, prop: &str, x: Option<f32>, y: Option<f32>) {
    let mut v = n.get(prop).try_to::<Vector2>().unwrap_or(Vector2::ZERO);
    if let Some(x) = x {
        v.x = x;
    }
    if let Some(y) = y {
        v.y = y;
    }
    n.set(prop, &v.to_variant());
}

impl Build<'_> {
    /// `handle_list_property`: a list property's items, typed.
    fn list_value(&mut self, items: &Value) -> VarArray {
        let mut out = VarArray::new();
        let Value::Array(items) = items else {
            return out;
        };
        for item in items {
            let ty = item
                .get("type")
                .map(gd_str)
                .unwrap_or_else(|| "string".into());
            let val = item.get("value").cloned().unwrap_or(Value::Null);
            if ty == "list" {
                out.push(&self.list_value(&val).to_variant());
            } else if ty == "object" {
                let id = match &val {
                    Value::Int(i) => *i,
                    Value::Float(f) => *f as i64,
                    Value::String(s) => to_int(s),
                    _ => 0,
                };
                match self.object_polygon(id) {
                    Some(v) => out.push(&v),
                    None => out.push(&typed_value(&ty, &gd_str(&val))),
                }
            } else if ty == "class" && matches!(val, Value::Dict(_)) {
                out.push(&crate::gd::value_to_variant(&val));
            } else {
                out.push(&typed_value(&ty, &gd_str(&val)));
            }
        }
        out
    }

    /// `handle_properties`. `base_dir`: what relative file properties resolve against (`""`:
    /// the map's directory; a template object's properties pass the template's).
    // The rule groups (`self.area_prop(..)` and the like) apply their rule themselves and say
    // whether one matched: their branches of the chain are empty on purpose.
    #[allow(clippy::if_same_then_else)]
    pub(super) fn handle_properties(
        &mut self,
        node: &mut Gd<Node>,
        props: &[GProp],
        base_dir: &str,
    ) {
        if props.is_empty() {
            return;
        }
        let file_base = if base_dir.is_empty() {
            self.base_path.clone()
        } else {
            base_dir.to_string()
        };
        let k = Kind::of(node);
        let body = k.sb || k.area || k.cb || k.rb || k.ab;
        let has_children = body && node.get_child_count() > 0;
        for p in props {
            let name = p.name.as_str();
            let ty = p.kind.as_str();
            let is_class_dict = ty == "class" && p.value.as_ref().is_none_or(PV::is_dict);
            let mut list_val = VarArray::new();
            let mut val = String::new();
            if ty == "list" {
                let items = p
                    .value
                    .as_ref()
                    .and_then(PV::core)
                    .cloned()
                    .unwrap_or(Value::Null);
                list_val = self.list_value(&items);
            } else if !is_class_dict {
                val = p.val();
            }
            let lname = name.to_lowercase();
            if name.is_empty() || lname == "godot_node_type" || lname == "res_path" {
                continue;
            }
            if name.starts_with("__") && has_children {
                let forwarded = GProp {
                    name: name.chars().skip(2).collect(),
                    kind: p.kind.clone(),
                    value: if ty == "list" || is_class_dict {
                        p.value.clone()
                    } else {
                        Some(PV::Str(val.clone()))
                    },
                };
                for mut child in node.get_children().iter_shared() {
                    self.handle_properties(&mut child, std::slice::from_ref(&forwarded), base_dir);
                }
            }
            let num = ty == "float" || ty == "int";
            let b = || val.to_lowercase() == "true";
            let i = || to_int(&val);
            let fl = || to_float(&val);
            let fv = || to_float(&val) as f32;
            let color = || property_color(ty, &val);
            let sc = ty == "string" || ty == "color";

            // Node properties
            if lname == "godot_group" && ty == "string" {
                for g in val.split(',').filter(|g| !g.is_empty()) {
                    node.add_to_group_ex(g.trim()).persistent(true).done();
                }
            } else if lname == "godot_script" && (ty == "file" || ty == "string") {
                let mut path = val.clone();
                if ty == "file" && !is_absolute_path(&val) {
                    path = cleanup_path(&format!("{}/{}", file_base.replace("res://", ""), val));
                }
                let script = ResourceLoader::singleton().load(path.as_str());
                node.set("script", &script.to_variant());
            }
            // CanvasItem properties
            else if lname == "modulate" && sc {
                set(node, "modulate", color().to_variant());
            } else if lname == "self_modulate" && sc {
                set(node, "self_modulate", color().to_variant());
            } else if lname == "show_behind_parent" && ty == "bool" {
                set(node, "show_behind_parent", b().to_variant());
            } else if lname == "top_level" && ty == "bool" {
                set(node, "top_level", b().to_variant());
            } else if lname == "clip_children" && ty == "int" {
                if i() < 3 {
                    set(node, "clip_children", i().to_variant());
                }
            } else if lname == "light_mask" && ty == "string" {
                set(node, "light_mask", bitmask(&val, 20).to_variant());
            } else if lname == "visibility_layer" && ty == "string" {
                set(node, "visibility_layer", bitmask(&val, 20).to_variant());
            } else if lname == "z_index" && ty == "int" {
                set(node, "z_index", i().to_variant());
            } else if lname == "z_as_relative" && ty == "bool" {
                set(node, "z_as_relative", b().to_variant());
            } else if lname == "y_sort_enabled" && ty == "bool" {
                set(node, "y_sort_enabled", b().to_variant());
            } else if lname == "texture_filter" && ty == "int" {
                if i() < 6 {
                    set(node, "texture_filter", i().to_variant());
                }
            } else if lname == "texture_repeat" && ty == "int" {
                if i() < 4 {
                    set(node, "texture_repeat", i().to_variant());
                }
            } else if lname == "material" && ty == "file" {
                let r = load_resource(&val, &file_base, &mut self.diag);
                set(node, "material", r.to_variant());
            } else if lname == "use_parent_material" && ty == "bool" {
                set(node, "use_parent_material", b().to_variant());
            }
            // TileMapLayer properties
            else if lname == "tile_set" && ty == "file" && k.tml {
                let r = load_resource(&val, &file_base, &mut self.diag);
                set(node, "tile_set", r.to_variant());
            } else if lname == "tileset_resource_path" && ty == "string" && k.tml {
                if let Ok(mut ts) = node.get("tile_set").try_to::<Gd<Resource>>() {
                    ts.set_path(val.as_str());
                }
            } else if lname == "y_sort_origin" && ty == "int" && k.tml {
                set(node, "y_sort_origin", i().to_variant());
            } else if lname == "x_draw_order_reversed" && ty == "bool" && k.tml {
                set(node, "x_draw_order_reversed", b().to_variant());
            } else if lname == "rendering_quadrant_size" && ty == "int" && k.tml {
                set(node, "rendering_quadrant_size", i().to_variant());
            } else if lname == "collision_enabled" && ty == "bool" && k.tml {
                set(node, "collision_enabled", b().to_variant());
            } else if lname == "use_kinematic_bodies" && ty == "bool" && k.tml {
                set(node, "use_kinematic_bodies", b().to_variant());
            } else if lname == "collision_visibility_mode" && ty == "int" && k.tml {
                if i() < 3 {
                    set(node, "collision_visibility_mode", i().to_variant());
                }
            } else if lname == "navigation_enabled" && ty == "bool" && k.tml {
                set(node, "navigation_enabled", b().to_variant());
            } else if lname == "navigation_visibility_mode" && ty == "int" && k.tml {
                if i() < 3 {
                    set(node, "navigation_visibility_mode", i().to_variant());
                }
            }
            // Parallax2D properties
            else if k.p2d && num && self.parallax_prop(node, &lname, fv()) {
            } else if lname == "repeat_times" && ty == "int" && k.p2d {
                set(node, "repeat_times", i().to_variant());
            } else if lname == "follow_viewport" && ty == "bool" && k.p2d {
                set(node, "follow_viewport", b().to_variant());
            } else if lname == "ignore_camera_scroll" && ty == "bool" && k.p2d {
                set(node, "ignore_camera_scroll", b().to_variant());
            } else if lname == "screen_offset_x" && num && k.p2d {
                vec_part(node, "screen_offset", Some(fv()), None);
            } else if lname == "screen_offset_y" && num && k.p2d {
                vec_part(node, "screen_offset", None, Some(fv()));
            }
            // CollisionObject2D properties
            else if lname == "disable_mode" && ty == "int" && k.co {
                if i() < 3 {
                    set(node, "disable_mode", i().to_variant());
                }
            } else if lname == "collision_layer" && ty == "string" && k.co {
                set(node, "collision_layer", bitmask(&val, 32).to_variant());
            } else if lname == "collision_mask" && ty == "string" && k.co {
                set(node, "collision_mask", bitmask(&val, 32).to_variant());
            } else if lname == "collision_priority" && num && k.co {
                set(node, "collision_priority", fl().to_variant());
            } else if lname == "input_pickable" && ty == "bool" && k.co {
                set(node, "input_pickable", b().to_variant());
            }
            // CollisionPolygon2D properties
            else if lname == "build_mode" && ty == "int" && has_children && i() < 2 {
                for mut c in node.get_children().iter_shared() {
                    if c.is_class("CollisionPolygon2D") {
                        set(&mut c, "build_mode", i().to_variant());
                    }
                }
            }
            // CollisionPolygon2D and CollisionShape2D properties
            else if lname == "disabled" && ty == "bool" && has_children {
                for mut c in node.get_children().iter_shared() {
                    set(&mut c, "disabled", b().to_variant());
                }
            } else if lname == "one_way_collision" && ty == "bool" && has_children {
                for mut c in node.get_children().iter_shared() {
                    set(&mut c, "one_way_collision", b().to_variant());
                }
            } else if lname == "one_way_collision_margin" && num && has_children {
                for mut c in node.get_children().iter_shared() {
                    set(&mut c, "one_way_collision_margin", fl().to_variant());
                }
            }
            // CollisionShape2D properties
            else if lname == "debug_color" && sc && has_children {
                for mut c in node.get_children().iter_shared() {
                    if c.is_class("CollisionShape2D") {
                        set(&mut c, "debug_color", color().to_variant());
                    }
                }
            }
            // Area2D properties
            else if k.area && self.area_prop(node, &lname, ty, &val) {
            }
            // StaticBody2D properties
            else if lname == "physics_material_override" && ty == "file" && k.sb {
                let r = load_resource(&val, &file_base, &mut self.diag);
                set(node, "physics_material_override", r.to_variant());
            } else if lname == "constant_linear_velocity_x" && num && k.sb {
                vec_part(node, "constant_linear_velocity", Some(fv()), None);
            } else if lname == "constant_linear_velocity_y" && num && k.sb {
                vec_part(node, "constant_linear_velocity", None, Some(fv()));
            } else if lname == "constant_angular_velocity" && num && k.sb {
                set(node, "constant_angular_velocity", fl().to_variant());
            }
            // AnimatableBody2D properties
            else if lname == "sync_to_physics" && ty == "bool" && k.ab {
                set(node, "sync_to_physics", b().to_variant());
            }
            // CharacterBody2D properties
            else if k.cb && self.character_prop(node, &lname, ty, &val) {
            }
            // RigidBody2D properties
            else if k.rb && self.rigid_prop(node, &lname, ty, &val, &file_base) {
            }
            // NavigationRegion2D properties
            else if k.nr && self.navigation_prop(node, &lname, ty, &val, &file_base) {
            }
            // LightOccluder2D properties
            else if lname == "sdf_collision" && ty == "bool" && k.lo {
                set(node, "sdf_collision", b().to_variant());
            } else if lname == "occluder_light_mask" && ty == "string" && k.lo {
                set(node, "occluder_light_mask", bitmask(&val, 20).to_variant());
            }
            // Polygon2D, Line2D, Marker2D properties
            else if lname == "color" && sc && k.poly {
                set(node, "color", color().to_variant());
            } else if lname == "width" && num && k.line {
                set(node, "width", fl().to_variant());
            } else if lname == "default_color" && sc && k.line {
                set(node, "default_color", color().to_variant());
            } else if lname == "gizmo_extents" && num && k.marker {
                set(node, "gizmo_extents", fl().to_variant());
            }
            // Other properties are added as metadata
            else {
                let key = self.sn(name);
                if ty == "object" {
                    match self.object_polygon(to_int(&val)) {
                        Some(v) => node.set_meta(&key, &v),
                        None => node.set_meta(&key, &typed_value(ty, &val)),
                    }
                } else if ty == "list" {
                    node.set_meta(&key, &list_val.to_variant());
                } else if is_class_dict {
                    let v = p
                        .value
                        .as_ref()
                        .map(PV::to_variant)
                        .unwrap_or_else(|| VarDictionary::new().to_variant());
                    node.set_meta(&key, &v);
                } else {
                    node.set_meta(&key, &typed_value(ty, &val));
                }
            }
        }
    }

    /// The numeric Parallax2D properties (vector components and scalars).
    fn parallax_prop(&self, node: &mut Gd<Node>, lname: &str, v: f32) -> bool {
        let (prop, x) = match lname {
            "scroll_scale_x" => ("scroll_scale", true),
            "scroll_scale_y" => ("scroll_scale", false),
            "scroll_offset_x" => ("scroll_offset", true),
            "scroll_offset_y" => ("scroll_offset", false),
            "repeat_size_x" => ("repeat_size", true),
            "repeat_size_y" => ("repeat_size", false),
            "autoscroll_x" => ("autoscroll", true),
            "autoscroll_y" => ("autoscroll", false),
            "limit_begin_x" => ("limit_begin", true),
            "limit_begin_y" => ("limit_begin", false),
            "limit_end_x" => ("limit_end", true),
            "limit_end_y" => ("limit_end", false),
            _ => return false,
        };
        if x {
            vec_part(node, prop, Some(v), None);
        } else {
            vec_part(node, prop, None, Some(v));
        }
        true
    }

    fn area_prop(&self, node: &mut Gd<Node>, lname: &str, ty: &str, val: &str) -> bool {
        let num = ty == "float" || ty == "int";
        let b = val.to_lowercase() == "true";
        let i = to_int(val);
        let fl = to_float(val);
        match lname {
            "monitoring" if ty == "bool" => set(node, "monitoring", b.to_variant()),
            "monitorable" if ty == "bool" => set(node, "monitorable", b.to_variant()),
            "priority" if num => set(node, "priority", (fl as i64).to_variant()),
            "gravity_space_override" if ty == "int" => {
                if i < 5 {
                    set(node, "gravity_space_override", i.to_variant())
                }
            }
            "gravity_point" if ty == "bool" => set(node, "gravity_point", b.to_variant()),
            "gravity_point_center_x" if num => {
                vec_part(node, "gravity_point_center", Some(fl as f32), None)
            }
            "gravity_point_center_y" if num => {
                vec_part(node, "gravity_point_center", None, Some(fl as f32))
            }
            "gravity_point_unit_distance" if num => {
                set(node, "gravity_point_unit_distance", fl.to_variant())
            }
            "gravity_direction_x" if num => {
                vec_part(node, "gravity_direction", Some(fl as f32), None)
            }
            "gravity_direction_y" if num => {
                vec_part(node, "gravity_direction", None, Some(fl as f32))
            }
            "gravity" if num => set(node, "gravity", fl.to_variant()),
            "linear_damp_space_override" if ty == "int" => {
                if i < 5 {
                    set(node, "linear_damp_space_override", i.to_variant())
                }
            }
            "linear_damp" if num => set(node, "linear_damp", fl.to_variant()),
            "angular_damp_space_override" if ty == "int" => {
                if i < 5 {
                    set(node, "angular_damp_space_override", i.to_variant())
                }
            }
            "angular_damp" if num => set(node, "angular_damp", fl.to_variant()),
            _ => return false,
        }
        true
    }

    fn character_prop(&self, node: &mut Gd<Node>, lname: &str, ty: &str, val: &str) -> bool {
        let num = ty == "float" || ty == "int";
        let b = val.to_lowercase() == "true";
        let i = to_int(val);
        let fl = to_float(val);
        match lname {
            "motion_mode" if ty == "int" => {
                if i < 2 {
                    set(node, "motion_mode", i.to_variant())
                }
            }
            "up_direction_x" if num => vec_part(node, "up_direction", Some(fl as f32), None),
            "up_direction_y" if num => vec_part(node, "up_direction", None, Some(fl as f32)),
            "slide_on_ceiling" if ty == "bool" => set(node, "slide_on_ceiling", b.to_variant()),
            "wall_min_slide_angle" if num => set(node, "wall_min_slide_angle", fl.to_variant()),
            "floor_stop_on_slope" if ty == "bool" => {
                set(node, "floor_stop_on_slope", b.to_variant())
            }
            "floor_constant_speed" if ty == "bool" => {
                set(node, "floor_constant_speed", b.to_variant())
            }
            "floor_block_on_wall" if ty == "bool" => {
                set(node, "floor_block_on_wall", b.to_variant())
            }
            "floor_max_angle" if num => set(node, "floor_max_angle", fl.to_variant()),
            "floor_snap_length" if num => set(node, "floor_snap_length", fl.to_variant()),
            "platform_on_leave" if ty == "int" => {
                if i < 3 {
                    set(node, "platform_on_leave", i.to_variant())
                }
            }
            "platform_floor_layers" if ty == "string" => {
                set(node, "platform_floor_layers", bitmask(val, 32).to_variant())
            }
            "platform_wall_layers" if ty == "string" => {
                set(node, "platform_wall_layers", bitmask(val, 32).to_variant())
            }
            "safe_margin" if num => set(node, "safe_margin", fl.to_variant()),
            "collision_layer" if ty == "string" => {
                set(node, "collision_layer", bitmask(val, 32).to_variant())
            }
            "collision_mask" if ty == "string" => {
                set(node, "collision_mask", bitmask(val, 32).to_variant())
            }
            "collision_priority" if num => set(node, "collision_priority", fl.to_variant()),
            _ => return false,
        }
        true
    }

    fn rigid_prop(
        &mut self,
        node: &mut Gd<Node>,
        lname: &str,
        ty: &str,
        val: &str,
        file_base: &str,
    ) -> bool {
        let num = ty == "float" || ty == "int";
        let b = val.to_lowercase() == "true";
        let i = to_int(val);
        let fl = to_float(val);
        match lname {
            "mass" if num => set(node, "mass", fl.to_variant()),
            "inertia" if num => set(node, "inertia", fl.to_variant()),
            "center_of_mass_mode" if ty == "int" => {
                if i < 2 {
                    set(node, "center_of_mass_mode", i.to_variant())
                }
            }
            "physics_material_override" if ty == "file" => {
                let r = load_resource(val, file_base, &mut self.diag);
                set(node, "physics_material_override", r.to_variant())
            }
            "gravity_scale" if num => set(node, "gravity_scale", fl.to_variant()),
            "custom_integrator" if ty == "bool" => set(node, "custom_integrator", b.to_variant()),
            "continuous_cd" if ty == "int" => {
                if i < 3 {
                    set(node, "continuous_cd", i.to_variant())
                }
            }
            "max_contacts_reported" if ty == "int" => {
                set(node, "max_contacts_reported", i.to_variant())
            }
            "contact_monitor" if ty == "bool" => set(node, "contact_monitor", b.to_variant()),
            "sleeping" if ty == "bool" => set(node, "sleeping", b.to_variant()),
            "can_sleep" if ty == "bool" => set(node, "can_sleep", b.to_variant()),
            "lock_rotation" if ty == "bool" => set(node, "lock_rotation", b.to_variant()),
            "freeze" if ty == "bool" => set(node, "freeze", b.to_variant()),
            "freeze_mode" if ty == "int" => {
                if i < 2 {
                    set(node, "freeze_mode", i.to_variant())
                }
            }
            "linear_velocity_x" if num => vec_part(node, "linear_velocity", Some(fl as f32), None),
            "linear_velocity_y" if num => vec_part(node, "linear_velocity", None, Some(fl as f32)),
            "linear_damp_mode" if ty == "int" => {
                if i < 2 {
                    set(node, "linear_damp_mode", i.to_variant())
                }
            }
            "linear_damp" if num => set(node, "linear_damp", fl.to_variant()),
            "angular_velocity" if num => set(node, "angular_velocity", fl.to_variant()),
            "angular_damp_mode" if ty == "int" => {
                if i < 2 {
                    set(node, "angular_damp_mode", i.to_variant())
                }
            }
            "angular_damp" if num => set(node, "angular_damp", fl.to_variant()),
            "constant_force_x" if num => vec_part(node, "constant_force", Some(fl as f32), None),
            "constant_force_y" if num => vec_part(node, "constant_force", None, Some(fl as f32)),
            "constant_torque" if num => set(node, "constant_torque", fl.to_variant()),
            _ => return false,
        }
        true
    }

    fn navigation_prop(
        &mut self,
        node: &mut Gd<Node>,
        lname: &str,
        ty: &str,
        val: &str,
        file_base: &str,
    ) -> bool {
        let num = ty == "float" || ty == "int";
        let b = val.to_lowercase() == "true";
        let i = to_int(val);
        let fl = to_float(val);
        let nav = |n: &Gd<Node>| n.get("navigation_polygon").try_to::<Gd<Resource>>().ok();
        match lname {
            "navigation_polygon" if ty == "file" => {
                let r = load_resource(val, file_base, &mut self.diag);
                set(node, "navigation_polygon", r.to_variant())
            }
            "enabled" if ty == "bool" => set(node, "enabled", b.to_variant()),
            "navigation_layers" if ty == "string" => {
                set(node, "navigation_layers", bitmask(val, 32).to_variant())
            }
            "use_edge_connection" if ty == "bool" => {
                set(node, "use_edge_connection", b.to_variant())
            }
            "enter_cost" if num => set(node, "enter_cost", fl.to_variant()),
            "travel_cost" if num => set(node, "travel_cost", fl.to_variant()),
            // NavigationPolygon properties
            "parsed_geometry_type" if ty == "int" => {
                if i < 3 {
                    if let Some(mut np) = nav(node) {
                        np.set("parsed_geometry_type", &i.to_variant());
                    }
                }
            }
            "parsed_collision_mask" if ty == "string" => {
                if let Some(mut np) = nav(node) {
                    np.set("parsed_collision_mask", &bitmask(val, 32).to_variant());
                }
            }
            "source_geometry_mode" if ty == "int" => {
                if i < 3 {
                    if let Some(mut np) = nav(node) {
                        np.set("source_geometry_mode", &i.to_variant());
                    }
                }
            }
            "source_geometry_group_name" if ty == "string" => {
                if let Some(mut np) = nav(node) {
                    np.set("source_geometry_group_name", &val.to_variant());
                }
            }
            "cell_size" | "border_size" | "agent_radius" if num => {
                if let Some(mut np) = nav(node) {
                    np.set(lname, &fl.to_variant());
                }
            }
            "baking_rect_x" | "baking_rect_y" | "baking_rect_w" | "baking_rect_h" if num => {
                if let Some(mut np) = nav(node) {
                    let mut r = np.get("baking_rect").try_to::<Rect2>().unwrap_or_default();
                    match lname {
                        "baking_rect_x" => r.position.x = fl as f32,
                        "baking_rect_y" => r.position.y = fl as f32,
                        "baking_rect_w" => r.size.x = fl as f32,
                        _ => r.size.y = fl as f32,
                    }
                    np.set("baking_rect", &r.to_variant());
                }
            }
            "baking_rect_offset_x" | "baking_rect_offset_y" if num => {
                if let Some(mut np) = nav(node) {
                    let mut o = np
                        .get("baking_rect_offset")
                        .try_to::<Vector2>()
                        .unwrap_or_default();
                    if lname == "baking_rect_offset_x" {
                        o.x = fl as f32;
                    } else {
                        o.y = fl as f32;
                    }
                    np.set("baking_rect_offset", &o.to_variant());
                }
            }
            "resource_local_to_scene" if ty == "bool" => {
                if let Some(mut np) = nav(node) {
                    np.set_local_to_scene(b);
                }
            }
            "resource_name" if ty == "string" => {
                if let Some(mut np) = nav(node) {
                    np.set_name(val);
                }
            }
            _ => return false,
        }
        true
    }
}
