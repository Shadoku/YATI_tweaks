//! Object nodes by id, object references resolved in a second pass (F2 of
//! `docs/runtime-improvement-plan.md`: `register_object_node`, `resolve_object_refs`,
//! `get_object_ref_path`, `assign_object_ref`, `assign_list_ref`) and the `_tiled_import` hook
//! (F1: `finish_import`'s calls, `get_import_info`).

use super::{Build, Obj};
use crate::gd::{gd_str, to_int, typed_value, value_to_variant, PV};
use crate::props::GProp;
use godot::prelude::*;
use tiled_core::Value;

/// An object property waiting for every node to exist.
pub(crate) struct PendingRef {
    pub node: Gd<Node>,
    pub name: String,
    pub what: RefKind,
}

impl PendingRef {
    /// The object ids the reference names.
    pub fn targets(&self) -> Vec<i64> {
        match &self.what {
            RefKind::Single(id) => vec![*id],
            RefKind::List { refs, .. } => refs.iter().map(|r| r.2).collect(),
        }
    }
}

pub(crate) enum RefKind {
    /// An `object` property: the target's id.
    Single(i64),
    /// A list property with object items: the list (its object items `NodePath()` for now) and
    /// where each object item is (the array, inside nested lists too, its index, the id).
    List {
        list: VarArray,
        refs: Vec<(VarArray, usize, i64)>,
    },
}

/// The name of the method a node may have to hear about its object (F1).
const IMPORT_HOOK_METHOD: &str = "_tiled_import";

impl Build<'_> {
    /// `register_object_node`: the node created for a Tiled object, remembered for object
    /// references and given the metadata `id` with `add_id_as_metadata` (`set_id_meta` false: a
    /// tile object's body, whose sprite carries the id). The first node registered for an id
    /// stands for the object.
    pub(super) fn register(&mut self, id: i64, node: &Gd<Node>, set_id_meta: bool) {
        if id == 0 {
            return;
        }
        self.nodes_by_id.entry(id).or_insert_with(|| node.clone());
        if set_id_meta && self.opts.add_id_as_metadata {
            node.clone().set_meta(&self.meta_id, &id.to_variant());
        }
    }

    /// After an object node's properties: a node with a script is a candidate for
    /// `_tiled_import` (the runtime checks every object node at the end; a node gets a script
    /// only from its scene or a `godot_script` property). `scene`: the node is a scene's root.
    pub(super) fn hook_candidate(&mut self, node: &Gd<Node>, obj: &Obj, scene: bool, scripts: u64) {
        if (scene || self.scripts_set != scripts) && node.get_script().is_some() {
            self.hooks.push((node.clone(), obj.clone()));
        }
    }

    /// The second pass of object properties (`object_refs` `nodepath` / `both`): each becomes
    /// a NodePath from its node to the target object's node. The tree is detached but
    /// complete, so `get_path_to` works. A property of the node of the same name takes it.
    pub(super) fn resolve_object_refs(&mut self) {
        let pending = std::mem::take(&mut self.pending_refs);
        for r in &pending {
            self.resolve_ref(r);
        }
    }

    /// One pending reference resolved with the nodes that exist now (again, for a streamer's,
    /// whenever its targets come or go).
    pub(super) fn resolve_ref(&mut self, r: &PendingRef) {
        if !r.node.is_instance_valid() {
            return;
        }
        let mut node = r.node.clone();
        match &r.what {
            RefKind::List { list, refs } => {
                for (arr, i, id) in refs {
                    let path = self.ref_path(&node, &r.name, *id);
                    arr.clone().set(*i, &path.to_variant());
                }
                self.assign_list_ref(&mut node, &r.name, list.clone());
            }
            RefKind::Single(id) => {
                let path = self.ref_path(&node, &r.name, *id);
                let target = self.object_node(*id);
                self.assign_object_ref(&mut node, &r.name, path, target);
            }
        }
    }

    /// The node standing for object `id`, if it was imported (and is still alive).
    fn object_node(&self, id: i64) -> Option<Gd<Node>> {
        if id == 0 {
            return None;
        }
        self.nodes_by_id
            .get(&id)
            .filter(|n| n.is_instance_valid())
            .cloned()
    }

    /// `get_object_ref_path`: the NodePath from `node` to the node of object `id` (an empty
    /// one, with a warning, for an unset reference or an id no imported object has).
    fn ref_path(&mut self, node: &Gd<Node>, name: &str, id: i64) -> NodePath {
        match self.object_node(id) {
            Some(t) => node.get_path_to(&t),
            // A streamer's target may simply not be spawned yet: resolved when it is
            None if self.skel.is_some() => NodePath::default(),
            None => {
                let what = if id == 0 {
                    "is not set".to_string()
                } else {
                    format!("names no imported object (id {id})")
                };
                self.diag.warn(format!(
                    " -- Object property '{name}' of '{}' {what}. -> Empty NodePath",
                    node.get_name()
                ));
                NodePath::default()
            }
        }
    }

    /// `assign_object_ref`: into the node's property of that name when its type takes it (a
    /// NodePath the path, a Node of the right class the node, an untyped one the path), else
    /// metadata.
    fn assign_object_ref(
        &mut self,
        node: &mut Gd<Node>,
        name: &str,
        path: NodePath,
        target: Option<Gd<Node>>,
    ) {
        let info = if self.opts.assign_node_properties {
            {
                let class = node.get_class().to_string();
                self.assignable(node, &class).get(name).cloned()
            }
        } else {
            None
        };
        if let Some(info) = info {
            if info.ty == VariantType::NODE_PATH {
                node.set(name, &path.to_variant());
                return;
            }
            if info.ty == VariantType::OBJECT && !path.is_empty() {
                if let Some(t) = target.filter(|t| t.is_instance_valid()) {
                    if super::assign::is_of_class(&t.clone().upcast(), &info.class_name) {
                        node.set(name, &t.to_variant());
                        return;
                    }
                }
            }
            if info.ty == VariantType::NIL {
                node.set(name, &path.to_variant());
                return;
            }
        }
        let key = self.sn(name);
        node.set_meta(&key, &path.to_variant());
    }

    /// `assign_list_ref`: a list whose object items were resolved, an `Array[NodePath]` when it
    /// holds only NodePaths; into a property of the node when its type takes it.
    fn assign_list_ref(&mut self, node: &mut Gd<Node>, name: &str, list: VarArray) {
        let only_paths = list
            .iter_shared()
            .all(|v| v.get_type() == VariantType::NODE_PATH);
        let value: Variant = if only_paths {
            let typed: Array<NodePath> = list.iter_shared().map(|v| v.to::<NodePath>()).collect();
            typed.to_variant()
        } else {
            list.to_variant()
        };
        let info = if self.opts.assign_node_properties {
            {
                let class = node.get_class().to_string();
                self.assignable(node, &class).get(name).cloned()
            }
        } else {
            None
        };
        if let Some(info) = info {
            if info.ty == VariantType::NIL {
                node.set(name, &value);
                return;
            }
            if info.ty == VariantType::ARRAY {
                if let Some(typed) = super::assign::typed_array_for(&info, &list, node, name, true)
                {
                    node.set(name, &typed);
                    return;
                }
            }
        }
        let key = self.sn(name);
        node.set_meta(&key, &value);
    }

    /// `finish_import`'s hook calls: every candidate whose script has `_tiled_import`.
    pub(super) fn call_import_hooks(&mut self) {
        let hooks = std::mem::take(&mut self.hooks);
        for (mut node, obj) in hooks {
            if !node.is_instance_valid()
                || node.get_script().is_none()
                || !node.has_method(IMPORT_HOOK_METHOD)
            {
                continue;
            }
            let info = self.import_info(&node, &obj);
            node.call(IMPORT_HOOK_METHOD, &[info.to_variant()]);
        }
    }

    /// `get_import_info`: the object's id, name, class, its custom properties as the importer
    /// typed them (an object property a NodePath from the node), its rect in map pixels (as
    /// Tiled saves it) and its shape in object coordinates.
    fn import_info(&mut self, node: &Gd<Node>, obj: &Obj) -> VarDictionary {
        let o = &obj.o;
        let num = |n: Option<tiled_core::Num>| n.map(|n| n.as_f64()).unwrap_or(0.0) as f32;
        let (w, h) = (num(o.width), num(o.height));
        let mut shape = Rect2::new(Vector2::ZERO, Vector2::new(w, h)).to_variant();
        let shape_type = if o.gid.is_some() {
            "tile"
        } else if o.text.is_some() {
            "text"
        } else if o.point.is_some() {
            "point"
        } else if o.ellipse.is_some() {
            "ellipse"
        } else if o.capsule.is_some() {
            "capsule"
        } else if let Some(p) = &o.polygon {
            shape = self.polygon_from(p).to_variant();
            "polygon"
        } else if let Some(p) = &o.polyline {
            shape = self.polygon_from(p).to_variant();
            "polyline"
        } else {
            "rectangle"
        };
        let mut props = VarDictionary::new();
        for p in obj.props.iter().flatten() {
            if p.name.is_empty() {
                continue;
            }
            let v = self.import_info_prop(node, p);
            props.set(p.name.as_str(), &v);
        }
        let mut d = VarDictionary::new();
        d.set("id", o.id.unwrap_or(0));
        d.set("name", o.name.as_deref().unwrap_or(""));
        d.set("class", o.class_name());
        d.set("properties", &props);
        d.set(
            "rect",
            Rect2::new(Vector2::new(num(o.x), num(o.y)), Vector2::new(w, h)),
        );
        d.set("shape", &shape);
        d.set("shape_type", shape_type);
        d
    }

    fn import_info_prop(&mut self, node: &Gd<Node>, p: &GProp) -> Variant {
        match p.kind.as_str() {
            "list" | "class" => {
                let v = p
                    .value
                    .as_ref()
                    .and_then(PV::core)
                    .cloned()
                    .unwrap_or(Value::Null);
                self.import_info_value(node, &p.kind, &v, &p.val())
            }
            _ => self.import_info_value(node, &p.kind, &Value::Null, &p.val()),
        }
    }

    /// `get_import_info_value`. `value`: the core value (lists and classes), `val`: `str()` of
    /// the value.
    fn import_info_value(
        &mut self,
        node: &Gd<Node>,
        ty: &str,
        value: &Value,
        val: &str,
    ) -> Variant {
        match ty {
            "object" => {
                let id = to_int(val);
                if self.object_refs == "geometry" {
                    return self.object_polygon(id).unwrap_or_else(|| id.to_variant());
                }
                match self.object_node(id) {
                    Some(t) => node.get_path_to(&t).to_variant(),
                    None => NodePath::default().to_variant(),
                }
            }
            "list" => {
                let mut out = VarArray::new();
                if let Value::Array(items) = value {
                    for item in items {
                        let ity = item
                            .get("type")
                            .map(gd_str)
                            .unwrap_or_else(|| "string".into());
                        let iv = item
                            .get("value")
                            .cloned()
                            .unwrap_or_else(|| Value::String(String::new()));
                        let v = self.import_info_value(node, &ity, &iv, &gd_str(&iv));
                        out.push(&v);
                    }
                }
                out.to_variant()
            }
            "class" => match value {
                Value::Dict(_) => value_to_variant(value),
                _ => VarDictionary::new().to_variant(),
            },
            _ => typed_value(ty, val),
        }
    }
}
